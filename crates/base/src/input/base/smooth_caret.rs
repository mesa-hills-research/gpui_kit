//! The smooth caret: the caret glides along its row to where it moves instead
//! of jumping there, and the text typed at it is uncovered as it passes.
//!
//! Only drawing is animated. The text, the selection, the undo history and
//! everything that reads them change at once, and the frame draws the
//! difference between where the caret is and where it is shown:
//!
//! - The handlers of the movements that glide (typing, Backspace, the arrow
//!   keys and the word motions of each keybinding scheme) run inside
//!   [`InputBaseState::caret_motion`], which records the caret before and
//!   after them and the edits they made, as a [`Step`].
//! - The next frame's prepaint turns the steps since the frame before into one
//!   [`Change`]. A change no recorded step explains, such as a click, a paste
//!   or an undo, puts the caret where it is at once and ends a glide in
//!   progress. So does a change of row: the caret never slides between rows.
//! - The glide is a critically damped spring along the row ([`Glide`]). A
//!   movement made before it arrives aims it at the new place and keeps its
//!   position and velocity.
//! - A [`CaretPlan`] says how the frame draws the caret's row: the caret's x,
//!   where the text before it stops (typed text the caret has not reached is
//!   not drawn), where the text after it starts (it follows the caret), and the
//!   glyphs Backspace deleted that the caret has not yet passed back over.

use std::{ops::Range, rc::Rc, time::Duration};

#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{App, Pixels, Window, px};

use super::{InputBaseState, display_map::LineLayout, kind::InputModeKind, layout::LastLayout};
use crate::input::{InputExtras as _, RopeExt as _};

/// A glide ends once the caret is this close to its target, in pixels.
const SETTLED: f32 = 0.5;
/// What a glide aims to be within at its settle time, a little inside
/// [`SETTLED`], so it has ended by then.
const SETTLE_AIM: f32 = 0.45;
/// The settle time of a move by one character with the arrow keys.
const GRAPHEME_DURATION: Duration = Duration::from_millis(80);
/// The settle time of a move by a word.
const WORD_DURATION: Duration = Duration::from_millis(120);
/// The settle time of typing, unless the application sets its own.
const TYPING_DURATION: Duration = Duration::from_millis(100);
/// The most movements kept for the next frame.
const MAX_STEPS: usize = 64;
/// How far behind its target the caret falls before its glide is hurried, at
/// least: the cap is this or two of the characters being typed, whichever is
/// wider. A glide further behind than the cap is hurried, so it catches up.
/// Hurrying sooner would stiffen the spring on most keystrokes of a fast
/// burst, and the caret would lurch with the rhythm of the keys.
const MIN_LAG_CAP: f32 = 24.;

/// How the caret moves when the smooth caret is on.
///
/// The defaults glide the caret to a typed character in 100 ms, glide it back
/// over a character Backspace deletes in the same time, and glide it along
/// with the arrow keys and the word motions. Every other movement is instant:
/// up and down, Home and End, a click, a paste, an undo, and any move to
/// another row.
///
/// ```
/// use std::time::Duration;
/// use gpui_base::input::SmoothCaretOptions;
///
/// let options = SmoothCaretOptions::default()
///     .typing_duration(Duration::from_millis(150))
///     .navigation(false);
/// assert_eq!(options.typing(), Duration::from_millis(150));
/// assert!(options.animates_backspace());
/// assert!(!options.animates_navigation());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmoothCaretOptions {
    typing: Duration,
    backspace: bool,
    navigation: bool,
}

impl Default for SmoothCaretOptions {
    fn default() -> Self {
        Self {
            typing: TYPING_DURATION,
            backspace: true,
            navigation: true,
        }
    }
}

impl SmoothCaretOptions {
    /// How long the caret takes to settle after a keystroke, and after a
    /// Backspace. 100 ms by default, and zero moves the caret at once.
    pub fn typing_duration(mut self, duration: Duration) -> Self {
        self.typing = duration;
        self
    }

    /// Whether Backspace glides the caret back over the character it deletes,
    /// covering it as it goes. On by default.
    pub fn backspace(mut self, animate: bool) -> Self {
        self.backspace = animate;
        self
    }

    /// Whether the arrow keys and the word motions glide the caret, in
    /// 80 ms for a character and 120 ms for a word. On by default.
    pub fn navigation(mut self, animate: bool) -> Self {
        self.navigation = animate;
        self
    }

    /// See [`Self::typing_duration`].
    pub fn typing(&self) -> Duration {
        self.typing
    }

    /// See [`Self::backspace`].
    pub fn animates_backspace(&self) -> bool {
        self.backspace
    }

    /// See [`Self::navigation`].
    pub fn animates_navigation(&self) -> bool {
        self.navigation
    }
}

/// A movement that may glide, named by the handler that makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretMotion {
    /// Text typed at the caret, a tab included.
    Typing,
    /// Backspace over one character.
    Backspace,
    /// Left or right by one character.
    Grapheme,
    /// Left or right by a word.
    Word,
}

impl CaretMotion {
    fn duration(self, options: &SmoothCaretOptions) -> Duration {
        match self {
            Self::Typing | Self::Backspace => options.typing,
            Self::Grapheme => GRAPHEME_DURATION,
            Self::Word => WORD_DURATION,
        }
    }
}

/// The caret and the document as one movement found and left them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretStamp {
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    /// One caret, nothing selected and no input-method composition: the only
    /// state a glide starts from or ends in.
    pub(crate) simple: bool,
    pub(crate) text_len: usize,
}

/// One movement recorded by [`InputBaseState::caret_motion`].
#[derive(Clone, Debug)]
pub(crate) struct Step {
    motion: CaretMotion,
    before: CaretStamp,
    after: CaretStamp,
    /// The edits the movement made: a range of the text before the edit, and
    /// the length of the text put there.
    edits: Vec<(Range<usize>, usize)>,
}

/// What happened to the caret between two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Nothing.
    None,
    /// Anything that does not glide: the caret is drawn where it is.
    Instant,
    /// Text was typed at `at`, and the caret is now at `caret`.
    Insert { at: usize, caret: usize },
    /// Backspace deleted `from..to` of the text the last frame drew, and the
    /// caret is now at `from`.
    Delete { from: usize, to: usize },
    /// The caret moved without an edit.
    Move(CaretMotion),
}

impl Step {
    fn change(&self) -> Change {
        let (before, after) = (self.before, self.after);
        if !before.simple || !after.simple {
            return Change::Instant;
        }
        match self.motion {
            CaretMotion::Typing => match self.edits.as_slice() {
                [(range, inserted)]
                    if *range == (before.caret..before.caret)
                        && after.caret > before.caret
                        && after.caret <= before.caret + inserted
                        && after.text_len == before.text_len + inserted =>
                {
                    Change::Insert {
                        at: before.caret,
                        caret: after.caret,
                    }
                }
                // A typed closer that steps over the one already there.
                [] if after.revision == before.revision => Change::Move(CaretMotion::Grapheme),
                _ => Change::Instant,
            },
            CaretMotion::Backspace => match self.edits.as_slice() {
                [(range, 0)]
                    if *range == (after.caret..before.caret)
                        && after.caret < before.caret
                        && before.text_len - after.text_len == before.caret - after.caret =>
                {
                    Change::Delete {
                        from: after.caret,
                        to: before.caret,
                    }
                }
                _ => Change::Instant,
            },
            CaretMotion::Grapheme | CaretMotion::Word => {
                if self.edits.is_empty() && after.revision == before.revision {
                    Change::Move(self.motion)
                } else {
                    Change::Instant
                }
            }
        }
    }
}

/// Joins the change of a movement onto the changes before it in the same frame.
fn combine(earlier: Change, later: Change) -> Change {
    match (earlier, later) {
        (Change::None, change) => change,
        (
            Change::Insert { at, caret },
            Change::Insert {
                at: next,
                caret: end,
            },
        ) if next == caret => Change::Insert { at, caret: end },
        (
            Change::Delete { from, to },
            Change::Delete {
                from: next,
                to: end,
            },
        ) if end == from => Change::Delete { from: next, to },
        (Change::Move(a), Change::Move(b)) => {
            Change::Move(if a == CaretMotion::Word || b == CaretMotion::Word {
                CaretMotion::Word
            } else {
                CaretMotion::Grapheme
            })
        }
        _ => Change::Instant,
    }
}

/// The change from what the last frame drew to `current`, explained by
/// `steps`, the movements recorded since. A change the steps do not explain
/// from end to end is instant.
pub(crate) fn classify(drawn: CaretStamp, current: CaretStamp, steps: &[Step]) -> Change {
    if drawn == current {
        return Change::None;
    }
    let mut at = drawn;
    let mut change = Change::None;
    for step in steps.iter().filter(|step| step.before != step.after) {
        if step.before != at {
            return Change::Instant;
        }
        change = combine(change, step.change());
        at = step.after;
    }
    if at != current || change == Change::None {
        return Change::Instant;
    }
    change
}

/// One stretch of a glide: a critically damped spring along the row, from
/// the moment it was last aimed at a target.
///
/// The position is in closed form, so a frame samples it at any time and the
/// frame rate only decides how many samples are drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glide {
    start: Instant,
    /// Where the caret was at `start`, less the target, in pixels.
    offset: f64,
    /// Its velocity at `start`, in pixels per second.
    velocity: f64,
    /// The spring's natural frequency, per second.
    omega: f64,
    pub(crate) target: f32,
}

/// The offset from the target and the velocity of a critically damped spring
/// `t` seconds after it was at `offset` moving at `velocity`.
fn spring(offset: f64, velocity: f64, omega: f64, t: f64) -> (f64, f64) {
    let b = velocity + omega * offset;
    let decay = (-omega * t).exp();
    ((offset + b * t) * decay, (velocity - omega * b * t) * decay)
}

/// The settle time of a glide over `distance`, shortened when that is further
/// than `cap` so the caret catches up.
fn hurried(duration: Duration, distance: f64, cap: f64) -> f64 {
    let duration = duration.as_secs_f64().max(0.001);
    if distance > cap {
        duration * cap / distance
    } else {
        duration
    }
}

impl Glide {
    /// A glide from rest at `from` to `target`, easing out: fastest at the
    /// start, slowing to a stop. `None` when the two are already within
    /// [`SETTLED`].
    pub(crate) fn from_rest(
        now: Instant,
        from: f32,
        target: f32,
        duration: Duration,
        cap: f32,
    ) -> Option<Self> {
        let offset = f64::from(from - target);
        let distance = offset.abs();
        if distance <= f64::from(SETTLED) {
            return None;
        }
        let duration = hurried(duration, distance, f64::from(cap));
        let omega = (distance / f64::from(SETTLE_AIM)).ln() / duration;
        // Moving at `omega` times the distance cancels the spring's own
        // acceleration from rest, so the distance decays exponentially.
        Some(Self {
            start: now,
            offset,
            velocity: -omega * offset,
            omega,
            target,
        })
    }

    /// The position and velocity at `now`.
    pub(crate) fn at(&self, now: Instant) -> (f32, f32) {
        let t = now.saturating_duration_since(self.start).as_secs_f64();
        let (offset, velocity) = spring(self.offset, self.velocity, self.omega, t);
        (self.target + offset as f32, velocity as f32)
    }

    /// The same glide aimed at `target` from `now`, keeping the position and
    /// the velocity it has then. The spring is stiffened as far as it must be
    /// to settle within `duration`, and further when it is behind by more
    /// than `cap`.
    pub(crate) fn retarget(&self, now: Instant, target: f32, duration: Duration, cap: f32) -> Self {
        let t = now.saturating_duration_since(self.start).as_secs_f64();
        let (offset, velocity) = spring(self.offset, self.velocity, self.omega, t);
        let offset = offset + f64::from(self.target - target);
        let duration = hurried(duration, offset.abs(), f64::from(cap));
        Self {
            start: now,
            offset,
            velocity,
            omega: settling_omega(offset, velocity, duration),
            target,
        }
    }
}

/// The softest spring that brings a caret `offset` from its target, moving at
/// `velocity`, to within [`SETTLE_AIM`] of it after `duration` seconds and
/// keeps it there, without passing the target on the way.
fn settling_omega(offset: f64, velocity: f64, duration: f64) -> f64 {
    let distance = offset.abs();
    if distance <= f64::from(SETTLE_AIM) && velocity == 0. {
        return 1. / duration;
    }
    // The velocity toward the target. A spring passes the target when this is
    // more than `omega` times the distance, so `omega` stays above that.
    let toward = -velocity * offset.signum();
    let floor = if toward > 0. && distance > 0. {
        toward / distance * 1.000_1
    } else {
        1e-3
    };
    let remaining = |omega: f64| {
        // The offset grows while the caret still moves away, and shrinks
        // from its peak on: the furthest it is after `duration`.
        let b = velocity + omega * offset;
        let peak = if b != 0. { velocity / (omega * b) } else { 0. };
        let (offset, _) = spring(offset, velocity, omega, duration.max(peak));
        offset.abs()
    };
    let aim = f64::from(SETTLE_AIM);
    if remaining(floor) <= aim {
        return floor;
    }
    let (mut low, mut high) = (floor, floor.max(1.) * 2.);
    while remaining(high) > aim && high < 1e5 {
        low = high;
        high *= 2.;
    }
    for _ in 0..48 {
        let middle = (low + high) / 2.;
        if remaining(middle) > aim {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

/// A visual row of a visible line: the line, which of its wrapped rows, and
/// the byte the row starts at within the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowKey {
    pub(crate) buffer_line: usize,
    pub(crate) row: usize,
    pub(crate) start: usize,
}

/// A row found in one frame's layout.
#[derive(Clone, Copy, Debug)]
struct RowSpot {
    /// Index of the line among the layout's visible lines.
    line: usize,
    key: RowKey,
    len: usize,
    /// The x in the content of the row's first byte.
    x: Pixels,
    /// The byte the line starts at in the text the layout was made from.
    line_start: usize,
}

impl RowSpot {
    /// The row of `layout` that draws the caret at byte `local` of
    /// `buffer_line`, the way the caret is placed when it is drawn.
    fn find(layout: &LastLayout, buffer_line: usize, local: usize, affinity: bool) -> Option<Self> {
        let line_ix = layout
            .visible_buffer_lines
            .binary_search(&buffer_line)
            .ok()?;
        let line = layout.lines.get(line_ix)?;
        let rows = &line.wrapped_lines;
        let mut start = 0;
        for (row, shaped) in rows.iter().enumerate() {
            let is_last = row + 1 == rows.len();
            let matches = if shaped.len == 0 {
                local == start
            } else if is_last || affinity {
                local >= start && local <= start + shaped.len
            } else {
                local >= start && local < start + shaped.len
            };
            if matches {
                return Self::at(layout, buffer_line, line_ix, row, start);
            }
            start += shaped.len;
        }
        None
    }

    /// The row `key` names, if `layout` has it.
    fn of(layout: &LastLayout, key: RowKey) -> Option<Self> {
        let line_ix = layout
            .visible_buffer_lines
            .binary_search(&key.buffer_line)
            .ok()?;
        let line = layout.lines.get(line_ix)?;
        let start: usize = line
            .wrapped_lines
            .iter()
            .take(key.row)
            .map(|row| row.len)
            .sum();
        (start == key.start && key.row < line.wrapped_lines.len())
            .then(|| Self::at(layout, key.buffer_line, line_ix, key.row, start))
            .flatten()
    }

    fn at(
        layout: &LastLayout,
        buffer_line: usize,
        line_ix: usize,
        row: usize,
        start: usize,
    ) -> Option<Self> {
        let line = layout.lines.get(line_ix)?;
        let indent = if row == 0 { px(0.) } else { line.wrap_indent };
        Some(Self {
            line: line_ix,
            key: RowKey {
                buffer_line,
                row,
                start,
            },
            len: line.wrapped_lines.get(row)?.len,
            x: layout.alignment_offset(line.longest_width) + indent,
            line_start: *layout.visible_line_byte_offsets.get(line_ix)?,
        })
    }

    /// The x in the content of the caret at `offset`, an offset into the
    /// text the layout was made from, or `None` when it is not on this row.
    fn x_of(&self, layout: &LastLayout, offset: usize) -> Option<Pixels> {
        let local = offset.checked_sub(self.line_start)?;
        let in_row = local.checked_sub(self.key.start)?;
        if in_row > self.len {
            return None;
        }
        let row = layout
            .lines
            .get(self.line)?
            .wrapped_lines
            .get(self.key.row)?;
        Some(self.x + row.x_for_index(in_row))
    }

    /// Whether the row holds right-to-left text, which this does not animate.
    fn is_right_to_left(&self, layout: &LastLayout) -> bool {
        layout
            .lines
            .get(self.line)
            .and_then(|line| line.wrapped_lines.get(self.key.row))
            .is_some_and(|row| row.text.chars().any(is_right_to_left))
    }
}

/// Whether `c` is a strongly right-to-left letter: Hebrew, Arabic and the
/// other scripts written right to left.
fn is_right_to_left(c: char) -> bool {
    matches!(
        c,
        '\u{0590}'..='\u{08FF}'
            | '\u{FB1D}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}'
            | '\u{10800}'..='\u{10FFF}'
            | '\u{1E800}'..='\u{1EFFF}'
    )
}

/// Deleted text still drawn while the caret passes back over it: glyphs of a
/// row as an earlier frame laid it out.
#[derive(Clone)]
struct Ghost {
    lines: Rc<Vec<LineLayout>>,
    line: usize,
    row: usize,
    /// The x in that layout's content where the deleted text starts.
    left: Pixels,
    width: Pixels,
}

/// A glide in progress.
#[derive(Clone)]
struct Animation {
    row: RowKey,
    glide: Glide,
    /// Where in the content the typed text the caret has not yet reached
    /// starts. The row's text from here to the target is hidden right of the
    /// caret.
    reveal_from: Option<Pixels>,
    /// Deleted text, left to right, laid out from the target on.
    ghosts: Vec<Ghost>,
}

/// What the last frame drew.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    stamp: CaretStamp,
    row: RowKey,
    /// The caret's target x in the content.
    x: Pixels,
}

/// The smooth caret of one input: its options and the glide in progress.
#[derive(Default)]
pub(crate) struct SmoothCaret {
    pub(crate) enabled: bool,
    pub(crate) options: SmoothCaretOptions,
    steps: Vec<Step>,
    /// Edits since the open step began, or `None` outside a step.
    recording: Option<Vec<(Range<usize>, usize)>>,
    drawn: Option<Drawn>,
    animation: Option<Animation>,
    /// The last frame's plan, and the caret's window x before it is put on
    /// a device pixel and its velocity.
    #[cfg(test)]
    pub(crate) last_frame: Option<(CaretPlan, f32, f32)>,
    /// The active caret the last frame painted, if it painted one.
    #[cfg(test)]
    pub(crate) painted_caret: Option<gpui::Bounds<Pixels>>,
}

impl SmoothCaret {
    /// Called for every edit, with its range in the text before it and the
    /// length of what it put there.
    pub(crate) fn note_edit(&mut self, range: Range<usize>, len: usize) {
        if let Some(edits) = &mut self.recording {
            edits.push((range, len));
        }
    }

    /// Stops any glide, for a setting that turns the smooth caret off.
    pub(crate) fn stop(&mut self) {
        self.animation = None;
        self.drawn = None;
        self.steps.clear();
    }
}

/// How a frame draws the caret's row while the caret glides. Every x is in
/// window coordinates.
#[derive(Clone)]
pub(crate) struct CaretPlan {
    pub(crate) buffer_line: usize,
    pub(crate) row: usize,
    /// The left edge of the caret, on a device pixel.
    pub(crate) caret_x: Pixels,
    /// Where the caret is going.
    pub(crate) target_x: Pixels,
    /// The text before the caret is drawn up to here and no further: typed
    /// text from here on is hidden until the caret reaches it.
    pub(crate) head_end: Pixels,
    /// The text after the caret is drawn from here on.
    pub(crate) tail_start: Pixels,
    /// How far the text after the caret is moved from where it is laid out:
    /// left by the typed text still hidden, right by the deleted text still
    /// shown.
    pub(crate) tail_shift: Pixels,
    /// Deleted glyphs still drawn.
    pub(crate) ghosts: Vec<GhostPlan>,
}

impl CaretPlan {
    /// Whether the row is drawn in parts: something on it is hidden or
    /// moved. A glide made with the arrow keys moves only the caret.
    pub(crate) fn splits_row(&self) -> bool {
        self.head_end < self.target_x || self.tail_shift != px(0.) || !self.ghosts.is_empty()
    }
}

/// Deleted glyphs to draw: a row of an earlier frame's layout, moved by
/// `shift` and shown only across `visible`.
#[derive(Clone)]
pub(crate) struct GhostPlan {
    pub(crate) lines: Rc<Vec<LineLayout>>,
    pub(crate) line: usize,
    pub(crate) row: usize,
    pub(crate) shift: Pixels,
    pub(crate) visible: Range<Pixels>,
}

/// One frame of the smooth caret.
pub(crate) struct CaretFrame {
    /// How to draw the caret's row, while a glide is in progress.
    pub(crate) plan: Option<CaretPlan>,
    /// A glide ended with this frame, so the caret's blink starts over.
    pub(crate) ended: bool,
}

impl<M: InputModeKind> InputBaseState<M> {
    /// The caret and the document now.
    pub(crate) fn caret_stamp(&self) -> CaretStamp {
        CaretStamp {
            revision: self.document_revision,
            caret: self.cursor(),
            simple: self.selections.is_single()
                && self.active_selection().is_empty()
                && self.ime_marked_range.is_none(),
            text_len: self.text.len(),
        }
    }

    /// Runs `f`, a handler that moves the caret as `motion`, and records what
    /// it did for the smooth caret to glide. Without the smooth caret this is
    /// only `f`. A movement no handler records this way is instant.
    pub(crate) fn caret_motion<R>(
        &mut self,
        motion: CaretMotion,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if !self.smooth_caret.enabled || self.smooth_caret.recording.is_some() {
            return f(self);
        }
        let before = self.caret_stamp();
        self.smooth_caret.recording = Some(Vec::new());
        let result = f(self);
        let edits = self.smooth_caret.recording.take().unwrap_or_default();
        let after = self.caret_stamp();
        // Movements no frame drew, such as in an input that is not on
        // screen, are not kept for long: the next frame moves the caret at
        // once.
        if self.smooth_caret.steps.len() >= MAX_STEPS {
            self.smooth_caret.steps.clear();
            self.smooth_caret.drawn = None;
        }
        if before != after {
            self.smooth_caret.steps.push(Step {
                motion,
                before,
                after,
                edits,
            });
        }
        result
    }

    /// Advances the smooth caret to this frame, whose layout is `layout`.
    /// `text_x` is the window x of the content's left edge.
    pub(super) fn smooth_caret_frame(
        &mut self,
        layout: &LastLayout,
        text_x: Pixels,
        window: &mut Window,
        cx: &App,
    ) -> CaretFrame {
        let steps = std::mem::take(&mut self.smooth_caret.steps);
        let was_animating = self.smooth_caret.animation.is_some();
        let stamp = self.caret_stamp();

        // Only a caret that is drawn glides.
        let target = (self.smooth_caret.enabled
            && !cx.reduce_motion()
            && self.focus_handle.is_focused(window)
            && window.is_window_active()
            && !self.disabled
            && self.is_multi_line()
            && stamp.simple
            && !self.tokens_visible()
            && self.extras.caret_offset().is_none())
        .then(|| {
            let point = self.text.offset_to_point(stamp.caret);
            let spot = RowSpot::find(
                layout,
                point.row,
                stamp.caret - self.text.line_start_offset(point.row),
                self.cursor_line_end_affinity,
            )?;
            let x = spot.x_of(layout, stamp.caret)?;
            (!spot.is_right_to_left(layout)).then_some((spot, x))
        })
        .flatten();

        let Some((spot, target_x)) = target else {
            self.smooth_caret.animation = None;
            self.smooth_caret.drawn = None;
            #[cfg(test)]
            {
                self.smooth_caret.last_frame = None;
            }
            return CaretFrame {
                plan: None,
                ended: was_animating,
            };
        };

        let change = match self.smooth_caret.drawn {
            Some(drawn) => classify(drawn.stamp, stamp, &steps),
            None => Change::Instant,
        };
        let now = cx.background_executor().now();
        let animation = self.apply_caret_change(change, layout, spot, target_x, now);
        self.smooth_caret.drawn = Some(Drawn {
            stamp,
            row: spot.key,
            x: target_x,
        });

        // Sample the glide for this frame, and end it once it has arrived.
        let sample = animation.and_then(|animation| {
            let (x, velocity) = animation.glide.at(now);
            ((x - animation.glide.target).abs() > SETTLED).then_some((animation, x, velocity))
        });
        let Some((animation, x, velocity)) = sample else {
            self.smooth_caret.animation = None;
            #[cfg(test)]
            {
                self.smooth_caret.last_frame = None;
            }
            return CaretFrame {
                plan: None,
                ended: was_animating,
            };
        };

        window.request_animation_frame();
        let plan = caret_plan(&animation, x, text_x, window.scale_factor());
        #[cfg(test)]
        {
            self.smooth_caret.last_frame =
                Some((plan.clone(), (text_x + px(x)).as_f32(), velocity));
        }
        #[cfg(not(test))]
        let _ = velocity;
        self.smooth_caret.animation = Some(animation);
        CaretFrame {
            plan: Some(plan),
            ended: false,
        }
    }

    /// The glide after `change`: the one in progress aimed anew, a new one,
    /// or none.
    fn apply_caret_change(
        &mut self,
        change: Change,
        layout: &LastLayout,
        spot: RowSpot,
        target_x: Pixels,
        now: Instant,
    ) -> Option<Animation> {
        let options = self.smooth_caret.options;
        // A glide never leaves its row.
        let running = self
            .smooth_caret
            .animation
            .take()
            .filter(|animation| animation.row == spot.key);
        let drawn = self
            .smooth_caret
            .drawn
            .filter(|drawn| drawn.row == spot.key);
        let target = target_x.as_f32();
        // How far behind the caret falls before it is hurried: two of the
        // characters this movement crossed, or the minimum.
        let lag_cap = |width: f32| MIN_LAG_CAP.max(2. * width.abs());

        // A zero duration moves the caret at once.
        let duration = match change {
            Change::Insert { .. } => CaretMotion::Typing.duration(&options),
            Change::Delete { .. } => CaretMotion::Backspace.duration(&options),
            Change::Move(motion) => motion.duration(&options),
            Change::None | Change::Instant => Duration::MAX,
        };
        if duration.is_zero() {
            return None;
        }

        match change {
            Change::None => running.filter(|animation| {
                (animation.glide.target - target).abs() < 0.01 && drawn.is_some()
            }),
            Change::Instant => None,
            Change::Insert { at, .. } => {
                let drawn = drawn?;
                let reveal_from = spot.x_of(layout, at)?;
                // The text before the insertion stayed where it was.
                if (reveal_from - drawn.x).abs() > px(SETTLED) {
                    return None;
                }
                let cap = lag_cap((target_x - reveal_from).as_f32());
                // A glide that is right of where the text went in, such as
                // one still covering deleted text, ends first: a typed
                // character is always uncovered from its own left edge.
                let running = running.filter(|animation| {
                    animation.glide.at(now).0 <= reveal_from.as_f32() + SETTLED
                });
                match running {
                    Some(mut animation) => {
                        animation.glide = animation.glide.retarget(now, target, duration, cap);
                        animation.reveal_from = Some(
                            animation
                                .reveal_from
                                .map_or(reveal_from, |from| from.min(reveal_from)),
                        );
                        animation.ghosts.clear();
                        Some(animation)
                    }
                    None => Some(Animation {
                        row: spot.key,
                        glide: Glide::from_rest(now, reveal_from.as_f32(), target, duration, cap)?,
                        reveal_from: Some(reveal_from),
                        ghosts: Vec::new(),
                    }),
                }
            }
            Change::Delete { from, to } => {
                if !options.backspace {
                    return None;
                }
                let drawn = drawn?;
                // Where the deleted text was, in the last frame's layout.
                let old = self.last_layout.as_ref()?;
                let old_spot = RowSpot::of(old, drawn.row)?;
                let left = old_spot.x_of(old, from)?;
                let right = old_spot.x_of(old, to)?;
                if right <= left || (left - target_x).abs() > px(SETTLED) {
                    return None;
                }
                let ghost = Ghost {
                    lines: old.lines.clone(),
                    line: old_spot.line,
                    row: old_spot.key.row,
                    left,
                    width: right - left,
                };
                let cap = lag_cap((right - left).as_f32());
                match running {
                    Some(mut animation) => {
                        animation.glide = animation.glide.retarget(now, target, duration, cap);
                        animation.reveal_from =
                            animation.reveal_from.filter(|from| *from < target_x);
                        animation.ghosts.insert(0, ghost);
                        Some(animation)
                    }
                    None => Some(Animation {
                        row: spot.key,
                        glide: Glide::from_rest(now, drawn.x.as_f32(), target, duration, cap)?,
                        reveal_from: None,
                        ghosts: vec![ghost],
                    }),
                }
            }
            Change::Move(_) => {
                if !options.navigation {
                    return None;
                }
                let drawn = drawn?;
                let cap = lag_cap((target_x - drawn.x).as_f32());
                match running {
                    // Text still hidden or deleted text still shown settles
                    // at once: only typing and Backspace uncover and cover.
                    Some(mut animation) => {
                        animation.glide = animation.glide.retarget(now, target, duration, cap);
                        animation.reveal_from = None;
                        animation.ghosts.clear();
                        Some(animation)
                    }
                    None => Some(Animation {
                        row: spot.key,
                        glide: Glide::from_rest(now, drawn.x.as_f32(), target, duration, cap)?,
                        reveal_from: None,
                        ghosts: Vec::new(),
                    }),
                }
            }
        }
    }
}

/// How to draw the caret's row with the caret at `x` in the content, for a
/// content whose left edge is at window x `text_x`.
///
/// The caret is drawn on a device pixel, and the edges that hide typed text
/// and deleted text are that same value, so the caret and the reveal never
/// disagree by a fraction of a pixel. The text after the caret moves with the
/// glide itself, unrounded, so it moves smoothly and lands where it is laid
/// out.
fn caret_plan(animation: &Animation, x: f32, text_x: Pixels, scale_factor: f32) -> CaretPlan {
    let snap = |x: Pixels| px((x.as_f32() * scale_factor).round() / scale_factor);
    let shown = text_x + px(x);
    let caret_x = snap(shown);
    let target_x = text_x + px(animation.glide.target);

    // Typed text the caret has not reached is hidden, and the text after it
    // moves left by as much.
    let (head_end, uncovered) = match animation.reveal_from.map(|from| text_x + from) {
        Some(from) if from < target_x => (
            caret_x.max(snap(from)).min(target_x),
            shown.max(from).min(target_x),
        ),
        _ => (target_x, target_x),
    };
    // Deleted text the caret has not passed back over is shown, and the text
    // after it stays right of it.
    let deleted: Pixels = animation.ghosts.iter().map(|ghost| ghost.width).sum();
    let ghost_end = caret_x.max(target_x).min(target_x + deleted);
    let covered = shown.max(target_x).min(target_x + deleted);

    let (tail_start, tail_shift) = if covered > target_x {
        (ghost_end, covered - target_x)
    } else {
        (head_end, uncovered - target_x)
    };

    let mut ghosts = Vec::new();
    let mut left = target_x;
    for ghost in &animation.ghosts {
        let right = left + ghost.width;
        let visible = left..right.min(ghost_end);
        if visible.start < visible.end {
            ghosts.push(GhostPlan {
                lines: ghost.lines.clone(),
                line: ghost.line,
                row: ghost.row,
                shift: left - (text_x + ghost.left),
                visible,
            });
        }
        left = right;
    }

    CaretPlan {
        buffer_line: animation.row.buffer_line,
        row: animation.row.row,
        caret_x,
        target_x,
        head_end,
        tail_start,
        tail_shift,
        ghosts,
    }
}

#[cfg(test)]
#[path = "smooth_caret_tests.rs"]
mod tests;
