//! How a spinner turns, measured on the frames a window draws. Each frame is rendered by
//! Mesa's software Vulkan driver (see `screenshots.rs`) and the angle of an icon that has a
//! gap in it is read from where its ink lies, so a spinner that speeds up and slows down, or
//! jumps where a turn ends, shows as a number.
#![cfg(target_os = "linux")]

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
    assets::Assets,
    component::{IconName, Sizable as _, Size, Theme, ThemeMode, spinner::Spinner},
    div, hsla, px, size,
};
use gpui_screenshot::{Screenshot, ScreenshotApp};

/// One turn of a spinner, in milliseconds.
const TURN_MS: f64 = 800.;

/// The most a frame's angular speed may differ from the median, in percent. A spinner that
/// turns evenly differs by a few percent from the pixels it is read from. One eased
/// in and out differs by about 100.
const MOST_UNEVEN: f64 = 10.;

struct Host {
    draws: Rc<Cell<usize>>,
    icon: IconName,
    size: Size,
    max_fps: Option<f32>,
    /// The icon's place in the window, in logical pixels.
    at: f32,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.draws.set(self.draws.get() + 1);
        let spinner = Spinner::new()
            .icon(self.icon.clone())
            .with_size(self.size)
            .color(hsla(0., 0., 1., 1.));
        let spinner = match self.max_fps {
            Some(max_fps) => spinner.max_fps(max_fps),
            None => spinner,
        };
        div().size_full().bg(hsla(0., 0., 0., 1.)).child(
            div()
                .absolute()
                .left(px(self.at))
                .top(px(self.at))
                .child(spinner),
        )
    }
}

struct Turn {
    icon: IconName,
    size: Size,
    max_fps: Option<f32>,
    scale: f32,
    /// How long it runs for, in turns.
    turns: f64,
}

impl Turn {
    fn new(icon: IconName, size: Size) -> Self {
        Self {
            icon,
            size,
            max_fps: None,
            scale: 1.,
            turns: 2.,
        }
    }

    fn icon_px(&self) -> f32 {
        match self.size {
            Size::XSmall => 12.,
            Size::Small => 14.,
            Size::Medium => 16.,
            Size::Large => 24.,
            Size::Size(px) => f32::from(px),
        }
    }
}

/// What a window showed in one frame the spinner drew.
struct Frame {
    /// Milliseconds since the window opened.
    at: f64,
    /// Degrees, unwrapped so that a turn goes on past 360.
    angle: f64,
    /// Where the ink is centred, in device pixels from the icon's centre.
    offset: (f64, f64),
}

/// Runs a window with a spinner under a display that shows a frame every 1/60 s and the test
/// clock, and reads each frame the spinner draws.
fn turn(turn: &Turn) -> Vec<Frame> {
    let mut app = ScreenshotApp::with_assets(Assets).unwrap();
    app.update(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.set_reduce_motion(false);
    });
    let draws = Rc::new(Cell::new(0));
    let icon = f64::from(turn.icon_px());
    let at = 16.;
    let side = icon as f32 + 2. * at;
    let window = app
        .open_window_with(size(px(side), px(side)), turn.scale, |options, cx| {
            gpui_kit::open_window(options, cx, |_, cx| {
                cx.new(|_| Host {
                    draws: draws.clone(),
                    icon: turn.icon.clone(),
                    size: turn.size,
                    max_fps: turn.max_fps,
                    at,
                })
            })
            .map(|(window, _)| window)
        })
        .unwrap();
    app.run_until_parked();

    let started = app.background_executor().now();
    // The icon's centre is on the whole pixels the layout rounds to.
    let centre = (
        (f64::from(at) + icon / 2.) * f64::from(turn.scale),
        (f64::from(at) + icon / 2.) * f64::from(turn.scale),
    );
    let vsync = Duration::from_micros(16_667);
    let step = Duration::from_micros(100);
    let steps = (Duration::from_secs_f64(TURN_MS * turn.turns / 1000.).as_micros()
        / step.as_micros()) as u32;
    let mut next_vsync = vsync;
    let mut seen = 0;
    let mut frames: Vec<Frame> = Vec::new();
    for tick in 0..=steps {
        if tick > 0 {
            app.advance_clock(step);
        }
        app.run_until_parked();
        if step * tick >= next_vsync {
            next_vsync += vsync;
            app.update_window(window, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            app.run_until_parked();
        }
        if draws.get() == seen {
            continue;
        }
        seen = draws.get();
        let shot = Screenshot {
            image: app.capture_screenshot(window).unwrap(),
            source: String::new(),
        };
        let (offset, angle) = ink_centre(&shot, centre, frames.last().map(|f| f.angle));
        frames.push(Frame {
            at: app
                .background_executor()
                .now()
                .duration_since(started)
                .as_secs_f64()
                * 1000.,
            angle,
            offset,
        });
    }
    frames
}

/// The centre of the ink of `shot` from `centre`, and its direction in degrees, unwrapped
/// against `previous`.
fn ink_centre(shot: &Screenshot, centre: (f64, f64), previous: Option<f64>) -> ((f64, f64), f64) {
    let (mut ink, mut sum_x, mut sum_y) = (0., 0., 0.);
    for (x, y, pixel) in shot.image.enumerate_pixels() {
        let weight = f64::from(pixel.0[0]) / 255.;
        ink += weight;
        sum_x += weight * (f64::from(x) + 0.5);
        sum_y += weight * (f64::from(y) + 0.5);
    }
    let offset = (sum_x / ink - centre.0, sum_y / ink - centre.1);
    let mut angle = offset.1.atan2(offset.0).to_degrees();
    if let Some(previous) = previous {
        while angle - previous > 180. {
            angle -= 360.;
        }
        while angle - previous < -180. {
            angle += 360.;
        }
    }
    (offset, angle)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

/// The angle gained between each two frames, in degrees, and the median speed in degrees a
/// second, and how far the fastest and the slowest frame are from it in percent.
struct Motion {
    steps: Vec<f64>,
    median_speed: f64,
    uneven: f64,
}

fn motion(frames: &[Frame]) -> Motion {
    let mut steps = Vec::new();
    let mut speeds = Vec::new();
    for pair in frames.windows(2) {
        let seconds = (pair[1].at - pair[0].at) / 1000.;
        steps.push(pair[1].angle - pair[0].angle);
        speeds.push((pair[1].angle - pair[0].angle) / seconds);
    }
    let median_speed = median(speeds.clone());
    let uneven = speeds
        .iter()
        .map(|speed| ((speed - median_speed) / median_speed).abs() * 100.)
        .fold(0., f64::max);
    Motion {
        steps,
        median_speed,
        uneven,
    }
}

/// A big icon with a gap, so that its angle is read to a fraction of a degree.
fn arc() -> Turn {
    Turn::new(IconName::LoaderCircle, Size::Size(px(96.)))
}

/// A spinner turns at the speed it was given, for every frame of a turn, where a turn ends
/// and the next starts included. Eased in and out, it stood nearly still at the start of a
/// turn and went round at twice the speed halfway.
#[test]
fn a_spinner_turns_at_a_constant_speed() {
    let frames = turn(&arc());
    let motion = motion(&frames);
    assert!(
        motion.uneven < MOST_UNEVEN,
        "the speed differs from the median by {:.1} %",
        motion.uneven
    );
    assert!(
        (motion.median_speed - 360. / (TURN_MS / 1000.)).abs() < 15.,
        "{} degrees a second",
        motion.median_speed
    );
    assert!(
        (94..=98).contains(&frames.len()),
        "{} frames in two turns at 60 Hz",
        frames.len()
    );
}

/// A spinner with a frame rate cap turns evenly too, with a step for each of its frames.
#[test]
fn a_capped_spinner_turns_at_a_constant_speed() {
    for max_fps in [60., 30., 12.] {
        let frames = turn(&Turn {
            max_fps: Some(max_fps),
            ..arc()
        });
        let motion = motion(&frames);
        assert!(
            motion.uneven < MOST_UNEVEN,
            "at {max_fps} fps the speed differs from the median by {:.1} %",
            motion.uneven
        );
        let expected = (TURN_MS * 2. / 1000. * f64::from(max_fps)).round() as usize;
        assert!(
            frames.len().abs_diff(expected) <= 2,
            "{} frames in two turns at {max_fps} fps",
            frames.len()
        );
    }
}

/// The default icon has eight spokes alike, so it looks the same after a step of 45 degrees.
/// A step of more than half of that looks like a step the other way, as the spokes of a
/// wheel do on film. A spinner steps by less at the rate it draws by default.
#[test]
fn a_spinner_does_not_step_far_enough_to_look_reversed() {
    let motion = motion(&turn(&arc()));
    let widest = motion.steps.iter().copied().fold(0., f64::max);
    assert!(widest < 22.5, "a step of {widest:.1} degrees");
}

/// An icon turns about its centre, whatever its size and the display's scale, so it does not
/// wobble.
#[test]
fn a_spinner_turns_about_its_centre() {
    for scale in [1., 1.25] {
        for size in [Size::XSmall, Size::Medium] {
            let frames = turn(&Turn {
                scale,
                turns: 1.,
                ..Turn::new(IconName::Loader, size)
            });
            let n = frames.len() as f64;
            let mean = (
                frames.iter().map(|f| f.offset.0).sum::<f64>() / n,
                frames.iter().map(|f| f.offset.1).sum::<f64>() / n,
            );
            let orbit = frames
                .iter()
                .map(|f| ((f.offset.0 - mean.0).powi(2) + (f.offset.1 - mean.1).powi(2)).sqrt())
                .fold(0., f64::max);
            assert!(
                orbit < 0.1,
                "the ink of a spinner drifts {orbit:.2} device pixels at scale {scale}"
            );
        }
    }
}
