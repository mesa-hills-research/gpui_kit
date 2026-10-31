//! Where a speech waveform's bars are painted while they move, read from frames rendered by
//! Mesa's software Vulkan driver (see `screenshots.rs`). A bar that is still on its way in
//! sits past the waveform's trailing edge, where nothing shows, so a bar that shows too early
//! means its motion ended too early.
#![cfg(target_os = "linux")]

use std::{cell::Cell, cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Subscription, Window,
    assets::Assets,
    component::{
        Theme, ThemeMode,
        speech::{
            AudioFormat, AudioInput, AudioSink, RecognitionSession, SpeechError, SpeechRecognizer,
            SpeechSink, SpeechState, SpeechWaveform,
        },
    },
    div, hsla, px, size,
};
use gpui_screenshot::{Screenshot, ScreenshotApp};

/// Where the waveforms sit in the window, in logical pixels.
const LEFT: f32 = 16.;
const TOPS: [f32; 2] = [16., 56.];
/// A waveform's default width and its default (medium) height.
const WIDTH: f32 = 96.;
const HEIGHT: f32 = 20.;
/// The left edge of the newest bar at rest. A medium waveform's bars are 3 px wide.
const AT_REST: u32 = (LEFT + WIDTH) as u32 - 3;

struct Recognizer;

struct Session;

impl SpeechRecognizer for Recognizer {
    fn start(
        &self,
        _: SpeechSink,
        _: &mut App,
    ) -> Result<Box<dyn RecognitionSession>, SpeechError> {
        Ok(Box::new(Session))
    }
}

impl RecognitionSession for Session {
    fn push_audio(&mut self, _: &[i16], _: &mut App) {}

    fn finish(&mut self, _: &mut App) {}
}

/// An input that hands its latest sink to the test.
#[derive(Clone, Default)]
struct Input(Rc<RefCell<Option<AudioSink>>>);

impl AudioInput for Input {
    fn start(
        &self,
        _: AudioFormat,
        sink: AudioSink,
        _: &mut App,
    ) -> Result<Subscription, SpeechError> {
        *self.0.borrow_mut() = Some(sink);
        Ok(Subscription::new(|| {}))
    }
}

/// A listening state and the input it captures from.
struct Listener {
    state: Entity<SpeechState>,
    input: Input,
}

impl Listener {
    fn new(cx: &mut App) -> Self {
        let input = Input::default();
        let state = cx.new(|cx| {
            SpeechState::new(cx)
                .recognizer(Recognizer)
                .input(input.clone())
        });
        state.update(cx, |state, cx| state.start(cx));
        Self { state, input }
    }

    /// Pushes a level in an update, which applies it when the update ends, as the
    /// microphone's task does.
    fn push_level(&self, cx: &mut App) {
        let input = self.input.clone();
        self.state.update(cx, |_, cx| push_level(&input, cx));
    }

    /// Cancels the session, starts another and pushes its first level, all in one update, so
    /// the window can't draw in between.
    fn restart_with_a_level(&self, cx: &mut App) {
        let input = self.input.clone();
        self.state.update(cx, |state, cx| {
            state.cancel(cx);
            state.start(cx);
            push_level(&input, cx);
        });
    }
}

/// Pushes 80 ms of 16 kHz mono, one level, through the input's latest sink.
fn push_level(input: &Input, cx: &mut App) {
    let sink = input.0.borrow().clone().unwrap();
    sink.push(vec![8_000; 1_280], cx);
}

/// Waveforms side by side in one parent, on black, with no ids of their own.
struct Host {
    states: Vec<Entity<SpeechState>>,
    draws: Rc<Cell<usize>>,
    _observe: Vec<Subscription>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.draws.set(self.draws.get() + 1);
        div()
            .size_full()
            .bg(hsla(0., 0., 0., 1.))
            .children(self.states.iter().zip(TOPS).map(|(state, top)| {
                div()
                    .absolute()
                    .left(px(LEFT))
                    .top(px(top))
                    .child(SpeechWaveform::new(state))
            }))
    }
}

/// What one drawn frame showed.
struct Frame {
    /// Milliseconds since the first level.
    at: u32,
    /// The leftmost column with ink in each waveform, in device pixels, or `None` where it
    /// shows nothing.
    leftmost: Vec<Option<u32>>,
}

impl Frame {
    fn read(at: u32, shot: &Screenshot, waveforms: usize) -> Self {
        let leftmost = TOPS[..waveforms]
            .iter()
            .map(|&top| {
                (LEFT as u32..(LEFT + WIDTH) as u32).find(|&x| {
                    (top as u32..(top + HEIGHT) as u32)
                        .any(|y| shot.image.get_pixel(x, y).0[..3].iter().any(|&c| c > 64))
                })
            })
            .collect();
        Self { at, leftmost }
    }
}

/// A window with a waveform for each of `listeners`, whose bars move (motion is on).
fn open(
    app: &mut ScreenshotApp,
    listeners: &[&Listener],
    draws: Rc<Cell<usize>>,
) -> AnyWindowHandle {
    let states: Vec<_> = listeners
        .iter()
        .map(|listener| listener.state.clone())
        .collect();
    let window = app
        .open_window_with(size(px(128.), px(96.)), 1., |options, cx| {
            gpui_kit::open_window(options, cx, |_, cx| {
                cx.new(|cx| Host {
                    _observe: states
                        .iter()
                        .map(|state| cx.observe(state, |_, _, cx| cx.notify()))
                        .collect(),
                    states,
                    draws,
                })
            })
            .map(|(window, _)| window)
        })
        .unwrap();
    app.run_until_parked();
    window
}

fn app() -> ScreenshotApp {
    let mut app = ScreenshotApp::with_assets(Assets).unwrap();
    app.update(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.set_reduce_motion(false);
    });
    app
}

/// Runs the window for `until` milliseconds of the test clock, with a frame every 1/60 s, and
/// calls `at` at each millisecond so the test can push audio then. Reads each frame the window
/// draws.
fn run(
    app: &mut ScreenshotApp,
    window: AnyWindowHandle,
    draws: &Cell<usize>,
    waveforms: usize,
    until: u32,
    mut at: impl FnMut(u32, &mut App),
) -> Vec<Frame> {
    let vsync = Duration::from_micros(16_667);
    let mut next_vsync = vsync;
    let mut seen = draws.get();
    let mut frames = Vec::new();
    for ms in 0..=until {
        if ms > 0 {
            app.advance_clock(Duration::from_millis(1));
        }
        app.update(|cx| at(ms, cx));
        app.run_until_parked();
        if Duration::from_millis(u64::from(ms)) >= next_vsync {
            next_vsync += vsync;
            app.update_window(window, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            app.run_until_parked();
        }
        if draws.get() != seen {
            seen = draws.get();
            let shot = Screenshot {
                image: app.capture_screenshot(window).unwrap(),
                source: String::new(),
            };
            frames.push(Frame::read(ms, &shot, waveforms));
        }
    }
    frames
}

/// The frames from `from` to `to` milliseconds.
fn between(frames: &[Frame], from: u32, to: u32) -> impl Iterator<Item = &Frame> {
    frames
        .iter()
        .filter(move |frame| (from..=to).contains(&frame.at))
}

/// A first level's bar slides in from 1.75 steps past the trailing edge, one step per 80 ms,
/// so it comes into view about 118 ms after it arrived and rests at 140 ms. Two waveforms of
/// different states in one parent, whose first levels come 60 ms apart, each move on their
/// own clock. The later one doesn't take the earlier one's start and rest 60 ms early.
#[test]
fn sibling_waveforms_move_on_their_own_clocks() {
    let mut app = app();
    let (first, second) = app.update(|cx| (Listener::new(cx), Listener::new(cx)));
    let draws = Rc::new(Cell::new(0));
    let window = open(&mut app, &[&first, &second], draws.clone());

    let frames = run(&mut app, window, &draws, 2, 260, |ms, cx| match ms {
        0 => first.push_level(cx),
        60 => second.push_level(cx),
        _ => {}
    });

    for frame in between(&frames, 150, 260) {
        assert_eq!(
            frame.leftmost[0],
            Some(AT_REST),
            "the first at {} ms",
            frame.at
        );
    }
    // The second's bar is still past the edge until about 178 ms.
    for frame in between(&frames, 60, 170) {
        assert_eq!(frame.leftmost[1], None, "the second at {} ms", frame.at);
    }
    for frame in between(&frames, 210, 260) {
        assert_eq!(
            frame.leftmost[1],
            Some(AT_REST),
            "the second at {} ms",
            frame.at
        );
    }
    // In between it slides in, and never moves back out.
    let sliding: Vec<u32> = between(&frames, 171, 209)
        .filter_map(|frame| frame.leftmost[1])
        .collect();
    assert!(
        sliding.windows(2).all(|pair| pair[1] <= pair[0]),
        "{sliding:?}"
    );
}

/// A session cancelled and started again, with its first level before the window draws, moves
/// on the new session's clock rather than the old one's.
#[test]
fn a_restarted_session_moves_on_its_own_clock() {
    let mut app = app();
    let listener = app.update(Listener::new);
    let draws = Rc::new(Cell::new(0));
    let window = open(&mut app, &[&listener], draws.clone());

    let frames = run(&mut app, window, &draws, 1, 300, |ms, cx| match ms {
        0 => listener.push_level(cx),
        100 => listener.restart_with_a_level(cx),
        _ => {}
    });

    // The new bar comes into view about 218 ms in and rests at 240 ms. On the old session's
    // clock it would have rested at 140 ms.
    for frame in between(&frames, 0, 210) {
        assert_eq!(frame.leftmost[0], None, "at {} ms", frame.at);
    }
    for frame in between(&frames, 250, 300) {
        assert_eq!(frame.leftmost[0], Some(AT_REST), "at {} ms", frame.at);
    }
}
