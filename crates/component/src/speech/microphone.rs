use std::{
    f32::consts::TAU,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::anyhow;
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _},
};
use gpui::{App, Subscription};
use smol::channel::{Sender, unbounded};

use super::{AudioFormat, AudioInput, AudioSink, SpeechError};

/// How much captured audio may wait for the app by default.
const DEFAULT_MAX_BACKLOG: Duration = Duration::from_secs(15);

/// The default audio input device, captured through the platform's audio API
/// (Core Audio, WASAPI or ALSA).
///
/// The device's own format is mixed down to mono and resampled to the format
/// the recognizer asks for.
///
/// Captured audio waits for the app's main thread to take it. If more than
/// [`Self::max_backlog`] piles up, because the app is busy for that long, the
/// session fails with [`SpeechError::Input`] rather than keep it all in memory
/// or lose some of it.
///
/// On macOS the application's `Info.plist` must describe why it uses the
/// microphone (`NSMicrophoneUsageDescription`), or the system refuses access
/// without asking.
#[derive(Debug, Clone)]
pub struct Microphone {
    max_backlog: Duration,
}

impl Default for Microphone {
    fn default() -> Self {
        Self {
            max_backlog: DEFAULT_MAX_BACKLOG,
        }
    }
}

impl Microphone {
    /// Set how much captured audio may wait for the app before the session
    /// fails, default 15 seconds.
    pub fn max_backlog(mut self, max_backlog: Duration) -> Self {
        self.max_backlog = max_backlog;
        self
    }
}

impl AudioInput for Microphone {
    fn start(
        &self,
        format: AudioFormat,
        sink: AudioSink,
        cx: &mut App,
    ) -> Result<Subscription, SpeechError> {
        #[cfg(target_os = "macos")]
        if super::system::macos::is_microphone_denied() {
            return Err(SpeechError::PermissionDenied);
        }

        let device = cpal::default_host()
            .default_input_device()
            .ok_or(SpeechError::NoInputDevice)?;
        let supported = device.default_input_config().map_err(SpeechError::input)?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let source_rate = config.sample_rate.0;

        // The channel is unbounded and the backlog bounds it, by frames of audio
        // rather than by messages, whose sizes vary.
        let (tx, rx) = unbounded();
        let max_backlog = self.max_backlog;
        let backlog = Arc::new(Backlog::new(frames_in(max_backlog, source_rate)));
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, tx, backlog.clone()),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, tx, backlog.clone()),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, tx, backlog.clone()),
            SampleFormat::I32 => build_stream::<i32>(&device, &config, tx, backlog.clone()),
            other => {
                return Err(SpeechError::input(anyhow!(
                    "unsupported sample format {other}"
                )));
            }
        }?;
        stream.play().map_err(SpeechError::input)?;

        let task = cx.spawn(async move |cx| {
            let mut converter = Converter::new(source_rate, format);
            let mut mono = Vec::new();
            while let Ok(first) = rx.recv().await {
                // The device delivers ~10 ms chunks. What has queued up goes on
                // as one push, so the state updates once per batch.
                let mut error = None;
                for capture in
                    std::iter::once(first).chain(std::iter::from_fn(|| rx.try_recv().ok()))
                {
                    match capture {
                        Capture::Samples(samples) => {
                            backlog.take(samples.len());
                            mono.extend_from_slice(&samples);
                        }
                        Capture::Overflow => {
                            error = Some(anyhow!(
                                "the app fell more than {max_backlog:?} behind the microphone"
                            ));
                        }
                        Capture::Error(message) => error = Some(anyhow!(message)),
                    }
                }

                let samples = converter.convert(&mono);
                mono.clear();
                cx.update(|cx| {
                    if !samples.is_empty() {
                        sink.push(samples, cx);
                    }
                    if let Some(error) = error.take() {
                        sink.error(SpeechError::input(error), cx);
                    }
                });
            }
        });

        Ok(Subscription::new(move || {
            drop(stream);
            drop(task);
        }))
    }
}

enum Capture {
    Samples(Vec<f32>),
    /// The backlog was full. The stream sends nothing after it.
    Overflow,
    /// The device failed. The stream sends nothing after it.
    Error(String),
}

/// The frames of `duration` at `rate`, at least one.
fn frames_in(duration: Duration, rate: u32) -> usize {
    ((duration.as_secs_f64() * rate as f64) as usize).max(1)
}

/// The captured audio that waits for the app, counted in frames, shared by the
/// audio thread and the app's task.
struct Backlog {
    max_frames: usize,
    frames: AtomicUsize,
    /// Set once the stream has sent an overflow or an error, which ends the
    /// session, so that it sends nothing more.
    failed: AtomicBool,
}

/// Whether the audio thread may send what it captured.
#[derive(Debug, PartialEq)]
enum Admission {
    Send,
    /// The backlog is full. Send [`Capture::Overflow`] in its place.
    Overflow,
    /// The stream already failed. Send nothing.
    Failed,
}

impl Backlog {
    fn new(max_frames: usize) -> Self {
        Self {
            max_frames,
            frames: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
        }
    }

    /// Count `frames` more as waiting, if they fit. The first that don't fit
    /// fail the stream.
    fn admit(&self, frames: usize) -> Admission {
        if self.failed.load(Ordering::Acquire) {
            return Admission::Failed;
        }
        let fits = self
            .frames
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
                waiting
                    .checked_add(frames)
                    .filter(|&total| total <= self.max_frames)
            })
            .is_ok();
        if fits {
            Admission::Send
        } else if self.fail() {
            Admission::Overflow
        } else {
            Admission::Failed
        }
    }

    /// Mark the stream failed, and return whether this call did.
    fn fail(&self) -> bool {
        !self.failed.swap(true, Ordering::AcqRel)
    }

    /// Count `frames` as taken by the app.
    fn take(&self, frames: usize) {
        self.frames.fetch_sub(frames, Ordering::AcqRel);
    }
}

/// Open an input stream that sends each callback's frames, mixed down to mono,
/// through `tx`, while they fit in `backlog`. Runs on the audio thread, so it
/// only converts and sends.
fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    tx: Sender<Capture>,
    backlog: Arc<Backlog>,
) -> Result<cpal::Stream, SpeechError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let error_tx = tx.clone();
    let error_backlog = backlog.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| {
                        frame
                            .iter()
                            .map(|&sample| f32::from_sample(sample))
                            .sum::<f32>()
                            / frame.len() as f32
                    })
                    .collect();
                match backlog.admit(mono.len()) {
                    Admission::Send => _ = tx.try_send(Capture::Samples(mono)),
                    Admission::Overflow => _ = tx.try_send(Capture::Overflow),
                    Admission::Failed => {}
                }
            },
            move |error| {
                if error_backlog.fail() {
                    _ = error_tx.try_send(Capture::Error(error.to_string()));
                }
            },
            None,
        )
        .map_err(|error| match error {
            cpal::BuildStreamError::DeviceNotAvailable => SpeechError::NoInputDevice,
            error => SpeechError::input(error),
        })
}

/// Converts mono `f32` audio at the device rate to interleaved `i16` audio in
/// the recognizer's format, carrying state across chunks.
struct Converter {
    /// Source samples per output sample.
    step: f64,
    /// Position of the next output sample, in source samples, where `0.0` is
    /// the last sample of the previous chunk.
    position: f64,
    previous: f32,
    low_pass: Option<LowPass>,
    channels: usize,
}

impl Converter {
    fn new(source_rate: u32, format: AudioFormat) -> Self {
        let target_rate = format.sample_rate().max(1);
        let source_rate = source_rate.max(1);
        Self {
            step: source_rate as f64 / target_rate as f64,
            position: 1.,
            previous: 0.,
            // Downsampling folds everything above the new Nyquist rate back
            // into the band speech lives in, so that is filtered out first.
            low_pass: (source_rate > target_rate)
                .then(|| LowPass::new(target_rate as f32 * 0.45, source_rate as f32)),
            channels: format.channels().max(1) as usize,
        }
    }

    fn convert(&mut self, input: &[f32]) -> Vec<i16> {
        if input.is_empty() {
            return Vec::new();
        }
        let filtered;
        let input = match self.low_pass.as_mut() {
            Some(low_pass) => {
                filtered = input
                    .iter()
                    .map(|&x| low_pass.process(x))
                    .collect::<Vec<_>>();
                &filtered[..]
            }
            None => input,
        };

        let sample = |ix: usize| {
            if ix == 0 {
                self.previous
            } else {
                input[ix - 1]
            }
        };
        let last = input.len();
        let len = last as f64;
        let mut output = Vec::with_capacity(((len / self.step) as usize + 1) * self.channels);
        while self.position <= len {
            let ix = self.position.floor() as usize;
            let frac = (self.position - ix as f64) as f32;
            let next = sample((ix + 1).min(last));
            let value = sample(ix) * (1. - frac) + next * frac;
            let value = (value.clamp(-1., 1.) * i16::MAX as f32) as i16;
            output.extend(std::iter::repeat_n(value, self.channels));
            self.position += self.step;
        }
        self.position -= len;
        self.previous = input[input.len() - 1];
        output
    }
}

/// Two cascaded one-pole low-pass filters, 12 dB per octave.
struct LowPass {
    alpha: f32,
    stages: [f32; 2],
}

impl LowPass {
    fn new(cutoff: f32, sample_rate: f32) -> Self {
        Self {
            alpha: 1. - (-TAU * cutoff / sample_rate).exp(),
            stages: [0.; 2],
        }
    }

    fn process(&mut self, mut x: f32) -> f32 {
        for stage in &mut self.stages {
            *stage += self.alpha * (x - *stage);
            x = *stage;
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_fails_once_when_audio_piles_up() {
        let backlog = Backlog::new(1_000);
        assert_eq!(backlog.admit(600), Admission::Send);
        assert_eq!(backlog.admit(400), Admission::Send);
        // What the app takes makes room again.
        backlog.take(500);
        assert_eq!(backlog.admit(500), Admission::Send);
        // Past the limit the stream fails once and sends nothing more, even
        // after the app catches up.
        assert_eq!(backlog.admit(1), Admission::Overflow);
        backlog.take(1_000);
        assert_eq!(backlog.admit(1), Admission::Failed);
    }

    #[test]
    fn the_stream_sends_one_error_and_nothing_after_it() {
        let backlog = Backlog::new(1_000);
        assert!(backlog.fail());
        assert!(!backlog.fail());
        assert_eq!(backlog.admit(1), Admission::Failed);
    }

    #[test]
    fn the_backlog_counts_frames_at_the_device_rate() {
        assert_eq!(frames_in(Duration::from_secs(15), 48_000), 720_000);
        assert_eq!(frames_in(Duration::ZERO, 48_000), 1);
    }

    #[test]
    fn converter_keeps_rate_and_duplicates_channels() {
        let mut converter = Converter::new(16_000, AudioFormat::new(16_000, 2));
        let output = converter.convert(&[0., 0.5, -0.5]);
        assert_eq!(output, vec![0, 0, 16383, 16383, -16383, -16383]);
    }

    #[test]
    fn converter_downsamples_across_chunks() {
        let mut converter = Converter::new(48_000, AudioFormat::default());
        let total: usize = (0..10).map(|_| converter.convert(&[0.; 480]).len()).sum();
        // 100 ms at 48 kHz is 100 ms at 16 kHz, whatever the chunking.
        assert_eq!(total, 1_600);
    }

    #[test]
    fn converter_upsamples() {
        let mut converter = Converter::new(8_000, AudioFormat::default());
        let total: usize = (0..10).map(|_| converter.convert(&[0.; 80]).len()).sum();
        // The first output lands on the first input sample, so the half step
        // before it is never produced.
        assert_eq!(total, 1_599);
    }
}
