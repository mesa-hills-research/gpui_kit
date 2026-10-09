//! The smooth caret, driven by keystrokes in a textarea under the test
//! scheduler's clock, with a frame drawn at each step of the clock.
//!
//! The test text system gives every character the same width, 9.6 px, and
//! an emoji twice that.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{
    App, AppContext as _, Bounds, EntityInputHandler as _, Modifiers, Pixels, SharedString, Task,
    TestAppContext, px,
};

use super::*;
use crate::input::{
    CursorShape, Keymap, KeymapPlatform, Redo, SelectAll, SpellCheck, SpellCheckRequest,
    SpellChecker, Undo, keymap::test::KeymapTest,
};

/// The width of a character in the test text system.
const CHAR: f32 = 9.6;
/// The step between frames.
const FRAME_MS: u64 = 5;

/// A focused textarea with the smooth caret on.
struct Glider {
    test: KeymapTest,
}

/// What one frame drew of the caret.
#[derive(Clone)]
struct Sample {
    plan: CaretPlan,
    /// The glide's window x, before it is put on a device pixel, and its
    /// velocity.
    x: f32,
    velocity: f32,
}

impl Glider {
    fn new(cx: &mut TestAppContext, marked: &str) -> Self {
        Self::with(cx, Keymap::Cua, KeymapPlatform::Linux, marked)
    }

    fn with(
        cx: &mut TestAppContext,
        keymap: Keymap,
        platform: KeymapPlatform,
        marked: &str,
    ) -> Self {
        let mut test = KeymapTest::new(cx, keymap, platform, marked);
        // The caret is drawn in an active window only.
        test.cx.update(|window, _| window.activate_window());
        test.update(|state, _, cx| state.set_smooth_caret(true, cx));
        let mut glider = Self { test };
        glider.frames_requested();
        glider
    }

    fn options(&mut self, options: SmoothCaretOptions) {
        self.test
            .update(|state, _, cx| state.set_smooth_caret_options(options, cx));
    }

    /// Moves the clock on and draws a frame.
    fn advance(&mut self, ms: u64) {
        self.test
            .cx
            .executor()
            .advance_clock(Duration::from_millis(ms));
        self.test.draw();
    }

    /// The last frame's glide, or `None` when it drew the caret where it is.
    fn sample(&self) -> Option<Sample> {
        self.test.textarea.read_with(&self.test.cx, |state, _| {
            state
                .smooth_caret
                .last_frame
                .clone()
                .map(|(plan, x, velocity)| Sample { plan, x, velocity })
        })
    }

    #[track_caller]
    fn gliding(&self) -> Sample {
        self.sample().expect("the caret glides")
    }

    #[track_caller]
    fn assert_still(&self, what: &str) {
        assert!(
            self.sample().is_none(),
            "{what}: the caret is drawn where it is, with nothing hidden or moved"
        );
    }

    /// The window x of the caret where it is, as the last frame laid it out.
    fn caret_x(&self) -> Pixels {
        self.test.textarea.read_with(&self.test.cx, |state, _| {
            state.cursor_layout().expect("a caret").0.origin.x
        })
    }

    /// The caret's top, which tells its rows apart.
    fn caret_y(&self) -> Pixels {
        self.test.textarea.read_with(&self.test.cx, |state, _| {
            state.cursor_layout().expect("a caret").0.origin.y
        })
    }

    /// The caret the last frame painted, if it painted one.
    fn painted_caret(&self) -> Option<Bounds<Pixels>> {
        self.test
            .textarea
            .read_with(&self.test.cx, |state, _| state.smooth_caret.painted_caret)
    }

    /// How many animation frames the frames drawn since the last call asked
    /// for.
    fn frames_requested(&mut self) -> usize {
        self.test
            .cx
            .update(|window, cx| window.simulate_next_frame(cx))
    }

    /// Draws frames until the glide ends, at most `ms` later, and returns
    /// them, the current one first.
    fn run(&mut self, ms: u64) -> Vec<Sample> {
        let mut samples = self.sample().into_iter().collect::<Vec<_>>();
        let mut elapsed = 0;
        while self.sample().is_some() && elapsed < ms {
            self.advance(FRAME_MS);
            elapsed += FRAME_MS;
            samples.extend(self.sample());
        }
        samples
    }
}

#[track_caller]
fn assert_close(actual: Pixels, expected: Pixels, what: &str) {
    assert!(
        (actual - expected).abs() <= px(0.5),
        "{what}: {actual:?}, expected {expected:?}"
    );
}

/// The reveal and the text after the caret, as one frame plans them for a
/// caret gliding right over typed text.
#[track_caller]
fn assert_uncovering(sample: &Sample) {
    let plan = &sample.plan;
    // Typed text is drawn up to the caret and no further.
    assert_eq!(plan.head_end, plan.caret_x.min(plan.target_x));
    // The text after the caret starts there, moved left by exactly how far
    // the caret is behind.
    assert_eq!(plan.tail_start, plan.head_end);
    let lag = plan.target_x - px(sample.x).min(plan.target_x);
    assert!(
        (plan.tail_shift + lag).abs() < px(1e-3),
        "moved {:?} with the caret {lag:?} behind",
        plan.tail_shift
    );
    assert!(plan.ghosts.is_empty());
}

#[gpui::test]
fn a_typed_character_is_uncovered_by_the_caret(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    let before = glider.caret_x();
    glider.test.type_text("W");
    glider.test.assert("abWˇcd");

    // The keystroke's frame: the caret has not moved yet, so nothing of the
    // new character is drawn, and "cd" is drawn where it was.
    let first = glider.gliding();
    let target = first.plan.target_x;
    assert_close(target, before + px(CHAR), "target");
    assert_close(first.plan.caret_x, before, "caret at the keystroke");
    assert_eq!(first.plan.head_end, first.plan.caret_x);
    assert_eq!(first.plan.tail_start, first.plan.caret_x);
    assert_close(
        first.plan.tail_start,
        before,
        "the text after the caret stays",
    );

    let mut previous = first;
    let mut middle = None;
    for step in 1..=40 {
        glider.advance(FRAME_MS);
        let Some(sample) = glider.sample() else {
            assert!(
                step * FRAME_MS > 150,
                "settled too early, at {} ms",
                step * FRAME_MS
            );
            break;
        };
        assert_uncovering(&sample);
        assert!(
            sample.x > previous.x,
            "the caret glides right without stopping"
        );
        assert!(sample.plan.caret_x >= previous.plan.caret_x);
        // Ease out: fastest at the start, slowing as it arrives.
        assert!(sample.velocity < previous.velocity);
        if step * FRAME_MS == 100 {
            middle = Some(sample.clone());
        }
        previous = sample;
    }
    let middle = middle.expect("still gliding at 100 ms");
    assert!(
        middle.plan.caret_x > before + px(0.5) && middle.plan.caret_x < target - px(0.5),
        "at 100 ms the caret is between where it was and where it goes: {:?}",
        middle.plan.caret_x
    );

    // After the settle time everything is drawn as without the smooth caret.
    glider.advance(200);
    glider.assert_still("after 200 ms");
    assert_eq!(glider.painted_caret().unwrap().origin.x, glider.caret_x());
}

#[gpui::test]
fn a_zero_typing_duration_types_at_once(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.options(SmoothCaretOptions::default().typing_duration(Duration::ZERO));
    glider.test.type_text("W");
    glider.assert_still("typing");
    glider.test.keys("backspace");
    glider.assert_still("Backspace");
    glider.test.keys("left");
    assert!(glider.sample().is_some(), "the arrow keys still glide");
}

#[gpui::test]
fn a_keystroke_settles_within_the_typing_duration(cx: &mut TestAppContext) {
    for (duration, typed) in [(200, "W"), (200, "a"), (120, "W"), (300, "WW")] {
        let mut glider = Glider::new(cx, "ˇ");
        glider.options(
            SmoothCaretOptions::default().typing_duration(Duration::from_millis(duration)),
        );
        glider.test.cx.simulate_input(typed);
        glider.test.draw();
        glider.advance(duration / 2);
        assert!(glider.sample().is_some(), "{duration} ms: gliding halfway");
        glider.advance(duration - duration / 2);
        glider.assert_still(&format!("{duration} ms after typing {typed:?}"));
    }
}

/// Types `count` characters `period_ms` apart with a frame every 5 ms, and
/// returns every frame's glide with its time.
fn type_steadily(glider: &mut Glider, count: usize, period_ms: u64) -> Vec<(u64, Sample)> {
    let mut frames = Vec::new();
    let mut now = 0;
    for key in 0..count {
        if key > 0 {
            // The frame at the keystroke's instant, drawn before it: the
            // glide aimed anew there keeps its position and velocity.
            let before = glider.gliding();
            glider.test.type_text("a");
            let after = glider.gliding();
            assert!(
                (after.x - before.x).abs() < 1e-3,
                "key {key}: the caret jumped from {} to {}",
                before.x,
                after.x
            );
            assert!(
                (after.velocity - before.velocity).abs() < 1e-2,
                "key {key}: the velocity jumped from {} to {}",
                before.velocity,
                after.velocity
            );
            assert!(after.plan.target_x > before.plan.target_x);
        } else {
            glider.test.type_text("a");
        }
        frames.push((now, glider.gliding()));
        for _ in 0..period_ms / FRAME_MS {
            glider.advance(FRAME_MS);
            now += FRAME_MS;
            if key + 1 < count {
                frames.push((now, glider.gliding()));
            }
        }
    }
    frames
}

#[gpui::test]
fn rapid_typing_aims_the_glide_anew_without_restarting_it(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "ˇ");
    let frames = type_steadily(&mut glider, 20, 30);
    let speed = CHAR / 0.030;

    for pair in frames.windows(2) {
        let ((_, a), (t, b)) = (&pair[0], &pair[1]);
        // Continuous: no frame moves the caret further than the typing
        // could, and the velocity never changes by half the typing speed
        // from one frame to the next.
        assert!(
            b.x - a.x >= -1e-3 && b.x - a.x < 3.,
            "{t} ms: the caret moved {} px in one frame",
            b.x - a.x
        );
        assert!(
            (b.velocity - a.velocity).abs() < speed / 2.,
            "{t} ms: the velocity went from {} to {}",
            a.velocity,
            b.velocity
        );
    }
    for (t, sample) in &frames {
        let lag = sample.plan.target_x - px(sample.x);
        // Bounded lag: within two characters, or 24 px.
        assert!(lag <= px(24.), "{t} ms: {lag:?} behind");
        assert_uncovering(sample);
        // No pause: once under way the caret keeps a steady pace.
        if *t >= 150 {
            assert!(
                sample.velocity > 0.5 * speed && sample.velocity < 1.5 * speed,
                "{t} ms: {} px/s while typing at {speed} px/s",
                sample.velocity
            );
        }
    }

    // When typing stops, the caret settles within the typing duration of the
    // last keystroke.
    glider.advance(200 - 30);
    glider.assert_still("200 ms after the last keystroke");
}

#[gpui::test]
fn holding_a_key_glides_steadily(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "ˇ");
    // Auto-repeat at about 30 characters a second, for two seconds.
    let frames = type_steadily(&mut glider, 60, 35);
    let lag = |from: u64, to: u64| {
        frames
            .iter()
            .filter(|(t, _)| (from..to).contains(t))
            .map(|(_, sample)| (sample.plan.target_x - px(sample.x)).as_f32())
            .fold(0f32, f32::max)
    };
    let early = lag(300, 700);
    let late = lag(1600, 2100);
    assert!(
        late <= early + 0.5,
        "the caret drifts behind: {early} px, then {late} px"
    );
    assert!(late <= 24., "{late} px behind");
}

#[gpui::test]
fn wide_characters_keep_the_lag_within_two_of_them(cx: &mut TestAppContext) {
    // An emoji, which the test text system draws twice as wide.
    let mut glider = Glider::new(cx, "ˇ");
    for key in 0..12 {
        glider.test.type_text("😀");
        for _ in 0..6 {
            glider.advance(FRAME_MS);
            let sample = glider.gliding();
            let lag = sample.plan.target_x - px(sample.x);
            assert!(lag <= px(4. * CHAR), "key {key}: {lag:?} behind");
            assert_uncovering(&sample);
        }
    }
}

#[gpui::test]
fn backspace_covers_the_deleted_character(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abcˇde");
    let before = glider.caret_x();
    glider.test.keys("backspace");
    glider.test.assert("abˇde");

    let first = glider.gliding();
    let target = first.plan.target_x;
    assert_close(target, before - px(CHAR), "target");
    assert_close(first.plan.caret_x, before, "caret at the keystroke");
    let mut previous: Option<Sample> = None;
    for sample in glider.run(300) {
        // The deleted "c" is drawn from the target up to the caret, and "de"
        // starts there.
        assert_covering(&sample, px(CHAR));
        if let Some(previous) = previous {
            assert!(sample.x < previous.x, "the caret glides left");
        }
        previous = Some(sample);
    }
    glider.assert_still("after the glide");

    // Rapid Backspace keeps covering, glyph after glyph.
    glider.test.keys("backspace");
    glider.advance(FRAME_MS * 4);
    glider.test.keys("backspace");
    glider.test.assert("ˇde");
    let sample = glider.gliding();
    assert_eq!(sample.plan.ghosts.len(), 2);
    let shown: Pixels = sample
        .plan
        .ghosts
        .iter()
        .map(|ghost| ghost.visible.end - ghost.visible.start)
        .sum();
    assert_eq!(
        sample.plan.target_x + shown,
        sample.plan.ghosts[1].visible.end
    );
    assert_covering(&sample, px(2. * CHAR));
}

/// The deleted text and the text after the caret, as one frame plans them
/// for a caret gliding left over `deleted` pixels of deleted text.
#[track_caller]
fn assert_covering(sample: &Sample, deleted: Pixels) {
    let plan = &sample.plan;
    let end = plan.target_x + deleted;
    let shown = plan
        .ghosts
        .last()
        .expect("deleted text is shown")
        .visible
        .end;
    // Deleted text is drawn from the target up to the caret.
    assert_eq!(plan.ghosts[0].visible.start, plan.target_x);
    assert_eq!(shown, plan.caret_x.max(plan.target_x).min(end));
    // The text after the caret starts there, moved right by exactly how far
    // the caret is from the target.
    assert_eq!(plan.tail_start, shown);
    let ahead = px(sample.x).max(plan.target_x).min(end) - plan.target_x;
    assert!(
        (plan.tail_shift - ahead).abs() < px(1e-3),
        "moved {:?} with the caret {ahead:?} ahead",
        plan.tail_shift
    );
    assert_eq!(plan.head_end, plan.target_x);
}

#[gpui::test]
fn backspace_can_be_instant(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abcˇde");
    glider.options(SmoothCaretOptions::default().backspace(false));
    glider.test.keys("backspace");
    glider.assert_still("Backspace with its glide off");
    glider.test.type_text("x");
    assert!(glider.sample().is_some(), "typing still glides");
}

#[gpui::test]
fn backspace_while_typing_covers_what_was_typed(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "ˇ");
    let start = glider.caret_x();
    glider.test.type_text("ab");
    glider.advance(60);
    let typing = glider.gliding();
    // Part of "b" is uncovered, and the caret is still gliding right.
    assert!(typing.plan.caret_x > start + px(CHAR));
    assert!(typing.velocity > 0.);
    glider.test.keys("backspace");
    let covering = glider.gliding();
    assert!((covering.x - typing.x).abs() < 1e-3);
    assert!((covering.velocity - typing.velocity).abs() < 1e-2);
    // What showed of "b" still shows, as deleted text, up to the caret.
    assert_eq!(covering.plan.ghosts.len(), 1);
    assert_eq!(covering.plan.ghosts[0].visible.end, covering.plan.caret_x);
    assert_covering(&covering, px(CHAR));
    // The caret turns around and covers it.
    let frames = glider.run(400);
    assert!(frames.last().unwrap().x < covering.x);
    glider.assert_still("after covering");
    glider.test.assert("aˇ");
}

#[gpui::test]
fn typing_after_backspace_uncovers_from_the_insertion(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abcˇ");
    glider.test.keys("backspace");
    glider.advance(10);
    // The caret is still right of "ab", covering "c": typing ends that glide
    // and uncovers the new character from its own left edge.
    let insertion = glider.gliding().plan.target_x;
    glider.test.type_text("x");
    let sample = glider.gliding();
    assert!(sample.plan.ghosts.is_empty());
    assert_close(
        sample.plan.caret_x,
        insertion,
        "the caret starts at the insertion",
    );
    assert_uncovering(&sample);
}

#[gpui::test]
fn forward_delete_is_instant(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.keys("delete");
    glider.test.assert("abˇd");
    glider.assert_still("Delete");
}

#[gpui::test]
fn space_and_tab_are_typed_like_any_character(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.type_text(" ");
    assert_uncovering(&glider.gliding());
    glider.advance(300);

    let before = glider.caret_x();
    glider.test.keys("tab");
    let sample = glider.gliding();
    assert_uncovering(&sample);
    // A tab is wider than a character, and is uncovered progressively too.
    assert!(sample.plan.target_x - before > px(CHAR));
    glider.advance(50);
    let sample = glider.gliding();
    assert!(sample.plan.caret_x > before && sample.plan.caret_x < sample.plan.target_x);
    assert_eq!(sample.plan.head_end, sample.plan.caret_x);
}

#[gpui::test]
fn the_arrow_keys_glide_the_caret_alone(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    let before = glider.caret_x();
    glider.test.keys("right");
    let sample = glider.gliding();
    assert!(!sample.plan.splits_row(), "nothing is hidden or moved");
    assert_close(sample.plan.target_x, before + px(CHAR), "target");
    glider.advance(40);
    let sample = glider.gliding();
    assert!(sample.plan.caret_x > before && sample.plan.caret_x < sample.plan.target_x);
    glider.advance(40);
    glider.assert_still("80 ms after Right");

    glider.test.keys("left");
    assert!(!glider.gliding().plan.splits_row());
    glider.advance(80);
    glider.assert_still("80 ms after Left");

    glider.options(SmoothCaretOptions::default().navigation(false));
    glider.test.keys("right");
    glider.assert_still("Right with navigation off");
}

/// Word motions glide in 120 ms, in every scheme and on every platform.
#[gpui::test]
fn word_motions_glide(cx: &mut TestAppContext) {
    let cases = [
        (
            Keymap::Cua,
            KeymapPlatform::Linux,
            "ctrl-right",
            "ctrl-left",
        ),
        (
            Keymap::Cua,
            KeymapPlatform::Windows,
            "ctrl-right",
            "ctrl-left",
        ),
        (Keymap::Cua, KeymapPlatform::MacOS, "alt-right", "alt-left"),
        (Keymap::Emacs, KeymapPlatform::Linux, "alt-f", "alt-b"),
        (Keymap::Vim, KeymapPlatform::Linux, "w", "b"),
        (Keymap::Vim, KeymapPlatform::Linux, "e", "b"),
    ];
    for (keymap, platform, forward, backward) in cases {
        let mut glider = Glider::with(cx, keymap, platform, "ˇone two three");
        for key in [forward, backward] {
            let before = glider.caret_x();
            glider.test.keys(key);
            let sample = glider.gliding();
            assert!(!sample.plan.splits_row(), "{keymap:?} {key}");
            assert!((sample.plan.target_x - before).abs() > px(CHAR));
            glider.advance(60);
            assert!(
                glider.sample().is_some(),
                "{keymap:?} {key}: gliding at 60 ms"
            );
            glider.advance(60);
            glider.assert_still(&format!("{keymap:?} {key} after 120 ms"));
        }
    }
}

/// The character motions of the other schemes glide like the arrow keys.
#[gpui::test]
fn character_motions_of_every_scheme_glide(cx: &mut TestAppContext) {
    for (keymap, forward, backward) in
        [(Keymap::Emacs, "ctrl-f", "ctrl-b"), (Keymap::Vim, "l", "h")]
    {
        let mut glider = Glider::with(cx, keymap, KeymapPlatform::Linux, "aˇbcd");
        for key in [forward, backward] {
            glider.test.keys(key);
            assert!(!glider.gliding().plan.splits_row(), "{keymap:?} {key}");
            glider.advance(80);
            glider.assert_still(&format!("{keymap:?} {key} after 80 ms"));
        }
    }
}

/// Every movement the table makes instant, made while a glide runs: the
/// glide ends at once, and the caret is drawn where it is.
#[gpui::test]
fn large_and_vertical_moves_are_instant_and_end_a_glide(cx: &mut TestAppContext) {
    let instant: [(&str, fn(&mut Glider)); 14] = [
        ("up", |g| g.test.keys("up")),
        ("down", |g| g.test.keys("down")),
        ("page up", |g| g.test.keys("pageup")),
        ("page down", |g| g.test.keys("pagedown")),
        ("home", |g| g.test.keys("home")),
        ("end", |g| g.test.keys("end")),
        ("the start of the text", |g| g.test.keys("ctrl-home")),
        ("the end of the text", |g| g.test.keys("ctrl-end")),
        ("enter", |g| g.test.keys("enter")),
        ("a selection", |g| g.test.keys("shift-left")),
        ("select all", |g| g.test.dispatch(SelectAll)),
        ("undo", |g| g.test.dispatch(Undo)),
        ("redo", |g| {
            g.test.dispatch(Undo);
            g.test.dispatch(Redo);
        }),
        (
            "a replacement, as a spelling fix or find and replace makes",
            |g| {
                g.test.update(|state, window, cx| {
                    state.replace_text_in_range(Some(0..3), "The", window, cx)
                })
            },
        ),
    ];
    for (what, act) in instant {
        let mut glider = Glider::new(cx, "one two\nthree fourˇ five\nsix seven");
        glider.test.type_text("s");
        glider.advance(FRAME_MS);
        assert!(glider.sample().is_some(), "{what}: gliding before");
        act(&mut glider);
        glider.assert_still(what);
        assert_eq!(
            glider.painted_caret().unwrap().origin.x,
            glider.caret_x(),
            "{what}: the caret is drawn where it is"
        );
        assert!(glider.frames_requested() > 0, "{what}: the glide asked");
        glider.advance(FRAME_MS);
        assert_eq!(glider.frames_requested(), 0, "{what}: no more frames");
    }
}

#[gpui::test]
fn a_paste_is_instant(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider
        .test
        .cx
        .write_to_clipboard(gpui::ClipboardItem::new_string("x".into()));
    glider.test.keys("ctrl-v");
    glider.test.assert("abxˇcd");
    glider.assert_still("a one-character paste");
}

#[gpui::test]
fn a_click_ends_a_glide(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abcdefˇ");
    glider.test.type_text("g");
    glider.advance(FRAME_MS);
    assert!(glider.sample().is_some());
    let bounds = glider.test.textarea.read_with(&glider.test.cx, |state, _| {
        state.range_to_bounds(&(1..2)).unwrap()
    });
    glider
        .test
        .cx
        .simulate_click(bounds.center(), Modifiers::none());
    glider.test.draw();
    glider.assert_still("a click");
    let marked = glider.test.marked();
    assert!(
        marked.starts_with('a') && !marked.ends_with('ˇ'),
        "{marked}"
    );
    assert_eq!(glider.painted_caret().unwrap().origin.x, glider.caret_x());
}

#[gpui::test]
fn the_caret_never_slides_to_another_row(cx: &mut TestAppContext) {
    // Enter: the caret jumps to the next line.
    let mut glider = Glider::new(cx, "abcˇ");
    glider.test.type_text("d");
    glider.advance(FRAME_MS);
    let row = glider.caret_y();
    glider.test.keys("enter");
    glider.assert_still("Enter");
    assert!(glider.caret_y() > row);

    // A word that soft-wraps as it is typed: the caret jumps with it to the
    // next row, and the glide on the row it left ends, with everything typed
    // drawn.
    let line = format!("{} bbbb", "a".repeat(55));
    let mut glider = Glider::new(cx, &format!("{line}ˇ"));
    let row = glider.caret_y();
    let mut wrapped = false;
    for _ in 0..10 {
        glider.test.type_text("b");
        if glider.caret_y() > row {
            glider.assert_still("a word wrapping onto the next row");
            wrapped = true;
            break;
        }
        assert!(glider.sample().is_some(), "typing on the row glides");
    }
    assert!(wrapped, "the word wrapped");
    glider.test.type_text("b");
    assert!(
        glider.sample().is_some(),
        "typing on the new row glides again"
    );

    // Vim's space at the end of a line goes to the next one, at once.
    let mut glider = Glider::with(cx, Keymap::Vim, KeymapPlatform::Linux, "abˇc\ndef");
    glider.test.keys("space");
    glider.test.assert("abc\nˇdef");
    glider.assert_still("Vim's space onto the next line");
}

#[gpui::test]
fn input_method_composition_is_instant(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇ");
    for marked in ["n", "ni"] {
        glider.test.update(|state, window, cx| {
            state.replace_and_mark_text_in_range(None, marked, None, window, cx);
        });
        glider.assert_still("marked text");
    }
    // The commit arrives as typed text, in place of the marked text.
    glider.test.update(|state, window, cx| {
        state.caret_motion(CaretMotion::Typing, |state| {
            state.replace_text_in_range(None, "你", window, cx)
        })
    });
    glider.test.assert("ab你ˇ");
    glider.assert_still("the composition's commit");
}

#[gpui::test]
fn vim_glides_with_the_block_caret(cx: &mut TestAppContext) {
    let mut glider = Glider::with(cx, Keymap::Vim, KeymapPlatform::Linux, "aˇbcd efgh\nij");
    assert_eq!(glider.test.cursor_shape(), CursorShape::Block);
    let block = glider.painted_caret().unwrap();
    glider.test.keys("l");
    let sample = glider.gliding();
    let painted = glider.painted_caret().unwrap();
    assert_eq!(painted.origin.x, sample.plan.caret_x);
    assert_eq!(painted.size, block.size, "the block keeps its size");
    glider.advance(80);
    glider.assert_still("l");

    // Down a line, back up, to the start and to the end: instant.
    for key in ["j", "k", "0", "$"] {
        glider.test.keys(key);
        glider.assert_still(key);
    }

    // Insert mode draws a bar, and typing uncovers.
    glider.test.keys("0 i");
    assert_eq!(glider.test.cursor_shape(), CursorShape::Bar);
    glider.assert_still("i");
    glider.test.type_text("x");
    assert_uncovering(&glider.gliding());
    // Escape changes the shape and steps back at once.
    glider.test.keys("escape");
    assert_eq!(glider.test.cursor_shape(), CursorShape::Block);
    glider.assert_still("escape");
}

#[gpui::test]
fn the_underline_caret_glides_too(cx: &mut TestAppContext) {
    let mut glider = Glider::with(cx, Keymap::Vim, KeymapPlatform::Linux, "ˇabcd");
    // Replace mode draws an underline.
    glider.test.keys("shift-r");
    assert_eq!(glider.test.cursor_shape(), CursorShape::Underline);
    glider.test.keys("right");
    let sample = glider.gliding();
    assert_eq!(
        glider.painted_caret().unwrap().origin.x,
        sample.plan.caret_x
    );
}

#[gpui::test]
fn the_caret_and_the_reveal_share_a_device_pixel(cx: &mut TestAppContext) {
    for scale in [2., 2.625, 1.] {
        let mut glider = Glider::new(cx, "abˇcd");
        glider.test.cx.simulate_scale_factor_change(scale);
        glider.test.draw();
        glider.test.type_text("W");
        let mut frames = glider.run(300);
        assert!(frames.len() > 10);
        glider.test.keys("backspace");
        frames.extend(glider.run(300));
        for sample in frames {
            let plan = &sample.plan;
            let device = plan.caret_x.as_f32() * scale;
            assert!(
                (device - device.round()).abs() < 1e-3,
                "scale {scale}: the caret is at {device} device pixels"
            );
            // While text is hidden or deleted text shown, its edge is the
            // caret's edge, exactly.
            if plan.head_end < plan.target_x {
                assert_eq!(plan.head_end, plan.caret_x);
                assert_eq!(plan.tail_start, plan.caret_x);
            }
            // One character was deleted.
            if let Some(ghost) = plan.ghosts.last()
                && plan.caret_x <= plan.target_x + px(CHAR)
            {
                assert_eq!(ghost.visible.end, plan.caret_x);
                assert_eq!(plan.tail_start, plan.caret_x);
            }
        }
    }
}

#[gpui::test]
fn the_painted_caret_is_the_glide(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.type_text("W");
    for _ in 0..10 {
        glider.advance(FRAME_MS);
        let sample = glider.gliding();
        let caret = glider.painted_caret().expect("the caret is painted");
        assert_eq!(caret.origin.x, sample.plan.caret_x);
        assert_eq!(caret.origin.x, sample.plan.head_end);
    }
}

#[gpui::test]
fn the_caret_is_solid_while_it_glides(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    // Longer than the blink's pause after a keystroke.
    glider.options(SmoothCaretOptions::default().typing_duration(Duration::from_millis(1000)));
    glider.test.type_text("W");
    let blink_visible = |glider: &Glider| {
        glider
            .test
            .textarea
            .read_with(&glider.test.cx, |state, cx| {
                state.blink_cursor.read(cx).visible()
            })
    };
    let mut hidden_by_blink = false;
    for _ in 0..250 {
        glider.advance(FRAME_MS);
        if glider.sample().is_none() {
            break;
        }
        hidden_by_blink |= !blink_visible(&glider);
        assert!(
            glider.painted_caret().is_some(),
            "the caret blinked while gliding"
        );
    }
    assert!(hidden_by_blink, "the blink would have hidden the caret");
    glider.assert_still("after the glide");

    // The blink starts over when the glide ends: the caret stays for the
    // blink's pause, then blinks.
    assert!(blink_visible(&glider));
    assert!(glider.painted_caret().is_some());
    glider.advance(290);
    assert!(blink_visible(&glider), "solid for the blink's pause");
    glider.advance(20);
    assert!(!blink_visible(&glider), "blinking again");
}

#[gpui::test]
fn frames_stop_once_the_caret_arrives(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.type_text("W");
    assert!(glider.frames_requested() > 0);
    while glider.sample().is_some() {
        glider.advance(FRAME_MS);
        if glider.sample().is_some() {
            assert!(
                glider.frames_requested() > 0,
                "a gliding frame asks for the next"
            );
        }
    }
    glider.frames_requested();
    glider.advance(FRAME_MS);
    assert_eq!(
        glider.frames_requested(),
        0,
        "a settled caret asks for no frame"
    );
    glider.advance(2000);
    assert_eq!(glider.frames_requested(), 0, "nor does an idle one");
}

/// Records the text of each spelling request.
struct Recorder(Rc<RefCell<Vec<String>>>);

impl SpellChecker for Recorder {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<anyhow::Result<SpellCheck>> {
        self.0.borrow_mut().push(request.text().to_string());
        Task::ready(Ok(SpellCheck::default()))
    }

    fn suggestions(&self, _: &str, _: &mut App) -> Vec<SharedString> {
        Vec::new()
    }

    fn add_to_dictionary(&self, _: &str, _: &mut App) {}

    fn debounce(&self) -> Duration {
        Duration::ZERO
    }
}

#[gpui::test]
fn the_text_changes_at_once(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "The ˇ");
    let checked = Rc::new(RefCell::new(Vec::new()));
    let checker = Rc::new(Recorder(checked.clone()));
    glider
        .test
        .update(|state, _, cx| state.set_spell_checker(Some(checker), cx));

    // No time passes, so every glide is still to come.
    glider.test.type_text("teh");
    assert!(glider.sample().is_some());
    glider.test.assert("The tehˇ");
    assert_eq!(
        checked.borrow().last().map(String::as_str),
        Some("The teh"),
        "the spell checker sees the text"
    );
    glider.test.dispatch(Undo);
    glider.test.assert("The ˇ");
    glider.test.dispatch(Redo);
    glider.test.assert("The tehˇ");
}

#[gpui::test]
fn reduced_motion_moves_the_caret_at_once(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.cx.update(|_, cx| cx.set_reduce_motion(true));
    glider.test.type_text("W");
    glider.assert_still("typing");
    for key in ["backspace", "left", "ctrl-left"] {
        glider.test.keys(key);
        glider.assert_still(key);
    }
    assert_eq!(glider.frames_requested(), 0);

    glider.test.cx.update(|_, cx| cx.set_reduce_motion(false));
    glider.test.type_text("W");
    assert!(glider.sample().is_some());
}

#[gpui::test]
fn the_smooth_caret_is_off_by_default(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, "abˇcd");
    assert!(
        !test
            .textarea
            .read_with(&test.cx, |state, _| state.has_smooth_caret())
    );
    test.type_text("W");
    test.keys("backspace left ctrl-left");
    assert!(
        test.textarea
            .read_with(&test.cx, |state, _| state.smooth_caret.last_frame.is_none())
    );
}

#[gpui::test]
fn turning_the_smooth_caret_off_ends_a_glide(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    glider.test.type_text("W");
    assert!(glider.sample().is_some());
    glider
        .test
        .update(|state, _, cx| state.set_smooth_caret(false, cx));
    glider.assert_still("smooth caret turned off");
    assert_eq!(glider.painted_caret().unwrap().origin.x, glider.caret_x());
}

#[gpui::test]
fn right_to_left_text_moves_the_caret_at_once(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "שלוםˇ");
    glider.test.type_text("ש");
    glider.assert_still("Hebrew");
    glider.test.keys("left");
    glider.assert_still("Hebrew");
}

#[gpui::test]
fn several_carets_move_at_once(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇc\ndef");
    glider
        .test
        .update(|state, _, cx| state.add_cursor_at(6, cx));
    glider.test.type_text("x");
    glider.assert_still("two carets");
}

#[gpui::test]
fn movements_made_within_one_frame_join(cx: &mut TestAppContext) {
    let mut glider = Glider::new(cx, "abˇcd");
    let before = glider.caret_x();
    // Two characters typed before the next frame are uncovered together.
    glider.test.cx.simulate_input("xy");
    glider.test.draw();
    let sample = glider.gliding();
    assert_close(sample.plan.target_x, before + px(2. * CHAR), "target");
    assert_uncovering(&sample);

    // Typing and then a click before the next frame: instant.
    glider.advance(300);
    glider.test.cx.simulate_input("z");
    glider
        .test
        .update(|state, _, cx| state.set_selected_range(0..0, cx));
    glider.assert_still("typing then a click");
}

#[test]
fn a_change_the_steps_do_not_explain_is_instant() {
    let stamp = |revision, caret, text_len| CaretStamp {
        revision,
        caret,
        simple: true,
        text_len,
    };
    let typed = Step {
        motion: CaretMotion::Typing,
        before: stamp(0, 2, 4),
        after: stamp(1, 3, 5),
        edits: vec![(2..2, 1)],
    };
    assert_eq!(
        classify(stamp(0, 2, 4), stamp(1, 3, 5), std::slice::from_ref(&typed)),
        Change::Insert { at: 2, caret: 3 }
    );
    // Something moved the caret after the typing.
    assert_eq!(
        classify(stamp(0, 2, 4), stamp(1, 0, 5), std::slice::from_ref(&typed)),
        Change::Instant
    );
    // Something edited before the typing.
    assert_eq!(
        classify(stamp(0, 2, 3), stamp(1, 3, 5), std::slice::from_ref(&typed)),
        Change::Instant
    );
    // Typing that replaced text, as an input method's commit does.
    let replaced = Step {
        edits: vec![(1..2, 2)],
        ..typed.clone()
    };
    assert_eq!(
        classify(stamp(0, 2, 4), stamp(1, 3, 5), &[replaced]),
        Change::Instant
    );
    // An auto-closed pair: the caret lands between the two.
    let pair = Step {
        after: stamp(1, 3, 6),
        edits: vec![(2..2, 2)],
        ..typed
    };
    assert_eq!(
        classify(stamp(0, 2, 4), stamp(1, 3, 6), &[pair]),
        Change::Insert { at: 2, caret: 3 }
    );
    // A selection is never glided from.
    let selected = CaretStamp {
        simple: false,
        ..stamp(0, 2, 4)
    };
    let moved = Step {
        motion: CaretMotion::Grapheme,
        before: selected,
        after: stamp(0, 4, 4),
        edits: Vec::new(),
    };
    assert_eq!(
        classify(selected, stamp(0, 4, 4), &[moved]),
        Change::Instant
    );
}

#[test]
fn a_glide_aimed_anew_keeps_its_position_and_velocity() {
    let start = Instant::now();
    let glide = Glide::from_rest(start, 0., 10., Duration::from_millis(200), 12.).unwrap();
    let (x0, v0) = glide.at(start);
    assert_eq!(x0, 0.);
    // Ease out: fastest at the start.
    assert!(v0 > glide.at(start + Duration::from_millis(10)).1);

    let later = start + Duration::from_millis(30);
    let (x, v) = glide.at(later);
    let aimed = glide.retarget(later, 20., Duration::from_millis(200), 12.);
    let (x1, v1) = aimed.at(later);
    assert!((x1 - x).abs() < 1e-4 && (v1 - v).abs() < 1e-3);

    // It never passes its target, and it is there after the duration.
    let mut previous = x1;
    for ms in (35..=230).step_by(5) {
        let (x, _) = aimed.at(start + Duration::from_millis(ms));
        assert!(x >= previous - 1e-4 && x <= 20. + 1e-4, "{ms} ms: {x}");
        previous = x;
    }
    assert!(20. - aimed.at(start + Duration::from_millis(230)).0 <= SETTLED);
}

#[test]
fn a_glide_turned_around_comes_back_without_passing_its_target() {
    let start = Instant::now();
    let glide = Glide::from_rest(start, 0., 30., Duration::from_millis(200), 15.).unwrap();
    let later = start + Duration::from_millis(20);
    let (x, v) = glide.at(later);
    assert!(v > 0.);
    // Aimed behind itself while it moves right.
    let back = glide.retarget(later, x - 5., Duration::from_millis(200), 12.);
    let mut furthest = x;
    for ms in (20..=220).step_by(2) {
        let (position, _) = back.at(start + Duration::from_millis(ms));
        furthest = furthest.max(position);
        assert!(
            position >= x - 5. - 1e-3,
            "{ms} ms: passed the target at {position}"
        );
    }
    assert!(furthest > x, "it keeps its velocity and turns around");
    assert!((back.at(start + Duration::from_millis(220)).0 - (x - 5.)).abs() <= SETTLED);
}

struct EditorWindow {
    editor: gpui::Entity<crate::input::EditorState>,
}

impl gpui::Render for EditorWindow {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement as _, Styled as _};
        gpui::div()
            .size_full()
            .child(gpui::div().h(px(300.)).child(self.editor.clone()))
    }
}

#[gpui::test]
fn the_code_editor_glides_too(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let window = cx.open_window(gpui::size(px(600.), px(400.)), |window, cx| EditorWindow {
        editor: cx.new(|cx| {
            crate::input::EditorState::new(window, cx)
                .language("sql")
                .smooth_caret(true)
                .default_value("select ")
        }),
    });
    let mut cx = gpui::VisualTestContext::from_window(window.into(), cx);
    let editor = window
        .read_with(&cx, |window, _| window.editor.clone())
        .unwrap();
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |state, cx| {
            state.focus(window, cx);
            state.set_selected_range(7..7, cx);
        });
    });
    draw(&mut cx);
    let sample = |cx: &gpui::VisualTestContext| {
        editor.read_with(cx, |state, _| {
            state
                .smooth_caret
                .last_frame
                .clone()
                .map(|(plan, x, velocity)| Sample { plan, x, velocity })
        })
    };

    // Typed text is uncovered, right of the line numbers.
    cx.simulate_input("x");
    draw(&mut cx);
    let typed = sample(&cx).expect("typing glides");
    assert_uncovering(&typed);
    assert!(typed.plan.target_x > px(7. * CHAR));
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(&mut cx);
    assert!(sample(&cx).is_none());

    // An opening bracket closes itself: the closer follows the caret.
    cx.simulate_input("(");
    draw(&mut cx);
    editor.read_with(&cx, |state, _| assert_eq!(state.value(), "select x()"));
    assert_uncovering(&sample(&cx).expect("typing glides"));
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(&mut cx);

    // Typing the closer steps over it: the caret glides, nothing is hidden.
    cx.simulate_input(")");
    draw(&mut cx);
    editor.read_with(&cx, |state, _| {
        assert_eq!(state.value(), "select x()");
        assert_eq!(state.cursor(), 10);
    });
    assert!(!sample(&cx).expect("the caret glides").plan.splits_row());
}
