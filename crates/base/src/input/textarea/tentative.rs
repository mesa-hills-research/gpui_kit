//! Tentative text, the words a producer such as dictation shows in a textarea
//! before they settle.
//!
//! The words show after the caret, or at an offset, in the muted colour with a
//! hairline in the accent under them. They take room in their line and wrap
//! with it as typed words would. They aren't in the document, so the value,
//! copying, the undo history, search and suggestions never see them, and the
//! caret and the selection pass over them. Committing a prefix puts it in the
//! document as an edit of its own, drawn by the glyphs that already showed it.
//! Its colour then fills in from the muted colour to the text colour, left to
//! right on the smooth caret's glide, and the hairline's left end rides the
//! fill's edge, so the colour pushes the line along. While an input method
//! composes, the words hide and can't be committed.
//!
//! Each glide moves as it paints, so no view draws again while one runs.

use std::ops::Range;

#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AvailableSpace, BackgroundExecutor, Bounds,
    ContentMask, Context, ElementId, Font, Hsla, IntoElement as _, PaintEffect, Pixels, Point,
    ShapedLine, SharedString, Styled as _, TextAlign, TextRun, Window, canvas, fill, point, px,
    size,
};
use sum_tree::Bias;
use unicode_segmentation::UnicodeSegmentation as _;

use super::{
    InputBaseState, InputModeKind, RopeExt as _, TextareaState,
    cursor::CursorSelection,
    display_map::{InlineText, offset_after_edit},
    layout::LastLayout,
    smooth_caret::{Glide, WORD_DURATION},
    undo_manager::EditIntent,
};

/// How long a committed word's colour takes to fill in, and an end of the
/// hairline to move. It's the smooth caret's settle time for a move by a word.
const GLIDE: std::time::Duration = WORD_DURATION;
/// The hairline's thickness.
const THICKNESS: Pixels = px(1.);
/// How far above the bottom of its line box the hairline's top is.
const RISE: Pixels = px(2.);
/// The widest column of committed text that fills in at once, in pixels.
const COLUMN: f32 = 2.;
/// The most columns a row of committed text fills in by. A longer row fills in
/// by wider ones.
const MAX_COLUMNS: usize = 256;

/// A textarea's tentative text and how its hairline last moved.
#[derive(Default)]
pub(crate) struct TentativeText {
    text: SharedString,
    /// Where the text shows. `None` is after the caret, and an offset moves
    /// with the edits around it.
    offset: Option<usize>,
    /// The document bytes the last commit put in, whose colour fills in.
    settled: Option<Range<usize>>,
    /// Count the changes of the text and the commits, so a frame can tell new
    /// words and a commit from anything else that moves the hairline.
    revision: u64,
    commits: u64,
    motion: Motion,
    /// What the last frame laid out.
    #[cfg(test)]
    pub(crate) last_frame: Option<TentativeFrame>,
}

/// What a frame laid out of tentative text, for tests.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct TentativeFrame {
    pub(crate) underlines: Vec<Bounds<Pixels>>,
    pub(crate) moving: usize,
    pub(crate) filling: Vec<Bounds<Pixels>>,
    pub(crate) riding: bool,
}

impl TentativeText {
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Moves the text's offset and the committed bytes with an edit of
    /// `range` to `new_len` bytes. Committed bytes the edit touches fill in
    /// no more.
    pub(crate) fn adjust_for_edit(&mut self, range: &Range<usize>, new_len: usize) {
        if let Some(offset) = &mut self.offset {
            *offset = offset_after_edit(*offset, range, new_len);
        }
        self.settled = self.settled.take().and_then(|settled| {
            if range.end <= settled.start {
                let start = settled.start + new_len - range.len();
                Some(start..start + settled.len())
            } else {
                (range.start >= settled.end).then_some(settled)
            }
        });
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Where tentative text at `offset` can be. That's a character boundary
    /// of the document outside any token, which shows as one piece, so an
    /// offset inside a token is at its end.
    fn tentative_anchor(&self, offset: usize) -> usize {
        let offset = self
            .text
            .clip_offset(offset.min(self.text.len()), Bias::Left);
        self.token_spans()
            .iter()
            .map(|span| span.range())
            .find(|range| range.start < offset && offset < range.end)
            .map_or(offset, |range| range.end)
    }

    /// Puts the tentative text's offset where it can be, for when the tokens
    /// changed since it was set. The offset that's kept is the one it shows
    /// at, which edits move and commits put text at.
    fn settle_tentative_anchor(&mut self) {
        if let Some(offset) = self.tentative.offset {
            self.tentative.offset = Some(self.tentative_anchor(offset));
        }
    }

    /// The tentative text as it shows now, and where, or `None` while there is
    /// none or an input method composes.
    pub(crate) fn tentative_inline_text(&self) -> Option<InlineText> {
        if self.tentative.text.is_empty() || self.ime_marked_range.is_some() {
            return None;
        }
        let offset = match self.tentative.offset {
            Some(offset) => offset,
            None => self.tentative_anchor(self.cursor()),
        };
        Some(InlineText {
            offset,
            text: self.tentative.text.clone(),
        })
    }

    /// Lays the tentative text out in its line, and makes room for it in an
    /// input that grows with its text.
    pub(crate) fn sync_tentative_text(&mut self, cx: &mut Context<Self>) {
        self.settle_tentative_anchor();
        let inline = self.tentative_inline_text();
        let rows = self.display_map.wrap_row_count();
        self.display_map.set_inline_text(inline, cx);
        if self.mode.is_auto_grow() && self.display_map.wrap_row_count() != rows {
            self.mode.update_auto_grow(&self.display_map);
            cx.notify();
        }
    }

    /// The rows of the line the tentative text is in, or the text its last
    /// commit put in while that fills in, as `layout` lays them out with the
    /// content's top-left at `origin`. `caret` is the active caret, which
    /// rides the fill's edge when it's drawn where the tentative text starts.
    pub(super) fn tentative_line(
        &self,
        layout: &LastLayout,
        origin: Point<Pixels>,
        caret: Option<Bounds<Pixels>>,
    ) -> Option<TentativeLine> {
        let shown = self.tentative_inline_text().is_some();
        let inline = self.display_map.inline_text().filter(|_| shown);
        let settled = self.tentative.settled.clone();
        let anchor = inline
            .map(|(inline, _)| inline.offset)
            .or(settled.as_ref().map(|settled| settled.end))?
            .min(self.text.len());
        let buffer_line = self.text.offset_to_point(anchor).row;
        let ix = layout
            .visible_buffer_lines
            .binary_search(&buffer_line)
            .ok()?;
        let line = layout.lines.get(ix)?;
        let line_start = *layout.visible_line_byte_offsets.get(ix)?;
        let line_height = layout.line_height;
        let top = origin.y
            + line_height * self.display_map.buffer_line_to_display_row(buffer_line) as f32;
        // The rows start where the text's own rows do.
        let left = origin.x + layout.line_number_width;

        let mut rows = Vec::with_capacity(line.wrapped_lines.len());
        let mut along = 0.;
        let mut start = 0;
        for (row_ix, row) in line.wrapped_lines.iter().enumerate() {
            let row_top = top + line_height * row_ix as f32;
            // On the bottom of the line box, below the glyphs' descent.
            let glyph_bottom = row.shaped_at(0).map_or(px(0.), |(shaped, _, _)| {
                row_top + (line_height + shaped.ascent + shaped.descent) / 2.
            });
            let underline_y = (row_top + line_height - RISE)
                .max(glyph_bottom)
                .min(row_top + line_height - THICKNESS);
            rows.push(TentativeRow {
                along,
                start,
                len: row.len,
                origin: point(left + line.row_x(row_ix, layout), row_top),
                width: row.width,
                underline_y,
                inserted: row.inserted_span(),
                words: row.inserted_words(),
                settled_words: None,
                shaped: None,
            });
            along += row.width.as_f32();
            start += row.len;
        }

        // The committed bytes, while they're in this line, and the shaped text
        // that draws them in each row.
        let settled = settled
            .filter(|settled| settled.start >= line_start && settled.end <= line_start + line.len())
            .map(|settled| settled.start - line_start..settled.end - line_start);
        if let Some(settled) = &settled {
            for (row, tentative_row) in line.wrapped_lines.iter().zip(&mut rows) {
                let (row_start, row_end) =
                    (tentative_row.start, tentative_row.start + tentative_row.len);
                if settled.start < row_end && settled.end > row_start {
                    let local = settled.start.max(row_start) - row_start;
                    tentative_row.shaped = row
                        .shaped_at(local)
                        .map(|(shaped, x, _)| (shaped.clone(), tentative_row.origin.x + x));
                    // The committed words in the row, without the spaces at
                    // either end.
                    let end = settled.end.min(row_end) - row_start;
                    let committed = row.text.get(local..end).unwrap_or_default();
                    let trimmed = committed.trim();
                    if !trimmed.is_empty() {
                        let start = local + committed.len() - committed.trim_start().len();
                        tentative_row.settled_words =
                            Some(row.x_for_index(start)..row.x_for_index(start + trimmed.len()));
                    }
                }
            }
        }

        // Where byte `local` of the line is along the rows, drawn as the caret
        // would be.
        let position = |local: usize| -> Option<f32> {
            let point = line.position_for_index(local, layout, false)?;
            let row = rows.get((point.y / line_height).round() as usize)?;
            Some(row.along + (left + point.x - row.origin.x).as_f32())
        };
        // The hairline runs under the words, from the first letter to the last.
        let text_rows = inline.map_or(&[][..], |(_, inline_rows)| {
            let first = inline_rows.first.min(rows.len());
            &rows[first..(first + inline_rows.slices.len()).min(rows.len())]
        });
        let words = |row: &TentativeRow| {
            let words = row.words.as_ref()?;
            Some((
                row.along + words.start.as_f32(),
                row.along + words.end.as_f32(),
            ))
        };
        let text = text_rows
            .iter()
            .find_map(words)
            .zip(text_rows.iter().rev().find_map(words))
            .map(|((start, _), (_, end))| (start, end));
        // The caret is in front of the tentative text, spaces and all.
        let text_start = text_rows
            .first()
            .and_then(|row| Some(row.along + row.inserted.as_ref()?.start.as_f32()));
        let settled =
            settled.and_then(|settled| Some((position(settled.start)?, position(settled.end)?)));
        let caret_at = text_start.or(settled.map(|(_, end)| end));
        let caret = caret.and_then(|caret| {
            let row = ((caret.top() - top) / line_height).floor().max(0.) as usize;
            let along = rows.get(row)?.along + (caret.left() - rows[row].origin.x).as_f32();
            caret_at
                .is_some_and(|at| (along - at).abs() < 0.5)
                .then_some((caret, along))
        });
        Some(TentativeLine {
            rows,
            text,
            settled,
            caret,
            line_height,
        })
    }
}

impl TextareaState {
    /// Shows `text` as tentative text after the caret, in place of what
    /// showed before, and keeps it after the caret as the caret moves.
    ///
    /// Tentative text is what a producer such as dictation hasn't settled
    /// yet. It shows in the theme's muted text colour with a hairline in the
    /// accent under it, and takes room in its line as typed text would. It
    /// isn't part of the document, so [`Self::value`], copying, the undo
    /// history, search and suggestions don't see it, and the caret and the
    /// selection can't enter it. A click on it puts the caret where it shows.
    ///
    /// It shows on one line, so a line break in `text` shows as a space. An
    /// empty `text` clears it. While it shows, no suggestions are offered,
    /// and those offered close. While an input method composes, it hides,
    /// and shows again once the composition ends.
    ///
    /// ```ignore
    /// // A recognizer's result, with the words that may still change.
    /// textarea.update(cx, |textarea, cx| textarea.set_tentative_text(" and the", cx));
    /// ```
    pub fn set_tentative_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.show_tentative_text(None, text.into(), cx);
    }

    /// Shows `text` as tentative text at byte `offset` of the document, as
    /// [`Self::set_tentative_text`] does after the caret. The offset moves with
    /// edits before it, and text put in at the offset goes before it. An
    /// offset past the end shows at the end.
    pub fn set_tentative_text_at(
        &mut self,
        offset: usize,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.show_tentative_text(Some(offset), text.into(), cx);
    }

    /// The tentative text, or an empty string when there is none.
    pub fn tentative_text(&self) -> &str {
        &self.tentative.text
    }

    /// Puts the first `len` bytes of the tentative text into the document
    /// where they show, as an edit of their own that Undo takes back, and
    /// keeps the rest as tentative text after them.
    ///
    /// Nothing moves. The same glyphs stay, and their colour fills in from
    /// the tentative colour to the text colour, left to right, with the
    /// smooth caret's glide, at once with reduced motion. A caret the text
    /// shows after moves to the end of what went in, as typing would move
    /// it, and other carets stay with the text around them.
    ///
    /// Returns whether anything went in. Nothing does while an input method
    /// composes, when `len` is 0, past the tentative text's end or inside a
    /// character, or when the document won't take the text.
    pub fn commit_tentative_text(
        &mut self,
        len: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.settle_tentative_anchor();
        let Some(inline) = self.tentative_inline_text() else {
            return false;
        };
        let text = self.tentative.text.clone();
        if len == 0 || !text.is_char_boundary(len) {
            return false;
        }
        let at = inline.offset;
        let committed = self.normalize_input(&text[..len]).into_owned();
        let inserted = committed.len();
        // A caret the tentative text shows after moves past what goes in, as
        // typing would move it. Other carets stay with the text around them,
        // and a selection doesn't grow over what goes in at its end.
        let follows = self.tentative.offset.is_none();
        let active = self.active_selection().id;
        let shift = |offset: usize, after: bool| {
            if offset > at || (offset == at && after) {
                offset + inserted
            } else {
                offset
            }
        };
        let selections: Vec<CursorSelection> = self
            .selections
            .iter()
            .map(|selection| {
                let mut selection = *selection;
                if follows && selection.id == active {
                    selection.place_at(at + inserted, None);
                } else if selection.is_empty() {
                    selection.place_at(shift(selection.start, true), selection.column_anchor);
                } else {
                    selection.start = shift(selection.start, true);
                    selection.end = shift(selection.end, false);
                }
                selection
            })
            .collect();
        let before = self.text.len();
        self.pause_blink_cursor(cx);
        self.with_edits_allowed(|this| {
            this.undo_manager.set_pending_intent(EditIntent::Atomic);
            this.replace_text_in_ranges_selecting(
                &[(at..at, committed)],
                Some(selections),
                window,
                cx,
            );
        });
        if inserted == 0 || self.text.len() != before + inserted {
            return false;
        }
        self.tentative.text = SharedString::new(&text[len..]);
        // The rest shows after what went in.
        self.tentative.offset = self
            .tentative
            .offset
            .filter(|_| !self.tentative.text.is_empty())
            .map(|_| at + inserted);
        self.tentative.settled = Some(at..at + inserted);
        self.tentative.commits += 1;
        self.sync_tentative_text(cx);
        cx.notify();
        true
    }

    /// Takes the tentative text away, without putting it in the document.
    pub fn clear_tentative_text(&mut self, cx: &mut Context<Self>) {
        self.show_tentative_text(None, SharedString::default(), cx);
    }

    fn show_tentative_text(
        &mut self,
        offset: Option<usize>,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        let text = if text.contains(['\n', '\r']) {
            SharedString::from(text.replace("\r\n", " ").replace(['\n', '\r'], " "))
        } else {
            text
        };
        let offset = offset
            .filter(|_| !text.is_empty())
            .map(|offset| self.tentative_anchor(offset));
        if text == self.tentative.text && offset == self.tentative.offset {
            return;
        }
        self.tentative.text = text;
        self.tentative.offset = offset;
        self.tentative.revision += 1;
        if !self.tentative.text.is_empty() {
            self.close_suggestions(cx);
        }
        self.sync_tentative_text(cx);
        cx.notify();
    }
}

/// The rows of the line tentative text is in, as a frame lays them out, in
/// window coordinates. A place "along" the line is its distance from the start
/// of the first row with the rows laid end to end.
pub(crate) struct TentativeLine {
    rows: Vec<TentativeRow>,
    /// Where the tentative text starts and ends along the line.
    text: Option<(f32, f32)>,
    /// Where the text the last commit put in starts and ends along the line.
    settled: Option<(f32, f32)>,
    /// The active caret and where it is along the line, when it's drawn
    /// where the tentative text starts.
    caret: Option<(Bounds<Pixels>, f32)>,
    line_height: Pixels,
}

struct TentativeRow {
    along: f32,
    /// The row's first byte in its line, and how many it has.
    start: usize,
    len: usize,
    /// The window x of the row's left edge and its top.
    origin: Point<Pixels>,
    width: Pixels,
    /// The top of the hairline under this row.
    underline_y: Pixels,
    /// Where the row's tentative text is in the row, and its words.
    inserted: Option<Range<Pixels>>,
    words: Option<Range<Pixels>>,
    /// Where the words the last commit put in are in the row.
    settled_words: Option<Range<Pixels>>,
    /// The shaped text drawing the row's committed bytes, and the window x
    /// it's drawn at.
    shaped: Option<(ShapedLine, Pixels)>,
}

/// The part of a row between two places along the line.
struct Segment {
    row: usize,
    /// Where it starts and ends along the line, and in the window.
    along: Range<f32>,
    x: Range<Pixels>,
}

impl TentativeLine {
    /// The parts of the rows from `from` to `to` along the line.
    fn segments(&self, from: f32, to: f32) -> impl Iterator<Item = Segment> + '_ {
        self.rows.iter().enumerate().filter_map(move |(ix, row)| {
            let start = from.max(row.along);
            let end = to.min(row.along + row.width.as_f32());
            (end > start).then(|| Segment {
                row: ix,
                along: start..end,
                x: row.origin.x + px(start - row.along)..row.origin.x + px(end - row.along),
            })
        })
    }

    /// The hairline under `segment`.
    fn underline(&self, segment: &Segment) -> Bounds<Pixels> {
        let row = &self.rows[segment.row];
        Bounds::from_corners(
            point(segment.x.start, row.underline_y),
            point(segment.x.end, row.underline_y + THICKNESS),
        )
    }

    /// The part of the hairline piece `rect` under the words the last commit
    /// put in row `row`.
    fn under_settled_words(&self, rect: Bounds<Pixels>, row: usize) -> Option<Bounds<Pixels>> {
        let row = &self.rows[row];
        let words = row.settled_words.as_ref()?;
        let left = rect.left().max(row.origin.x + words.start);
        let right = rect.right().min(row.origin.x + words.end);
        (right > left)
            .then(|| Bounds::from_corners(point(left, rect.top()), point(right, rect.bottom())))
    }

    /// The part of the hairline piece `rect` under the words of row `row`,
    /// when the row shows tentative text, and `rect` when it doesn't.
    fn under_words(&self, rect: Bounds<Pixels>, row: usize) -> Option<Bounds<Pixels>> {
        let row = &self.rows[row];
        if row.inserted.is_none() {
            return Some(rect);
        }
        let words = row.words.as_ref()?;
        let left = rect.left().max(row.origin.x + words.start);
        let right = rect.right().min(row.origin.x + words.end);
        (right > left)
            .then(|| Bounds::from_corners(point(left, rect.top()), point(right, rect.bottom())))
    }

    /// The row with the caret, which is where `along` is unless it's at the
    /// start of a row.
    fn caret_row(&self, caret: Bounds<Pixels>) -> Option<&TentativeRow> {
        let top = self.rows.first()?.origin.y;
        self.rows
            .get(((caret.top() - top) / self.line_height).floor().max(0.) as usize)
    }
}

/// The hairline's ends as the last frame aimed them, and their glides.
#[derive(Default)]
struct Motion {
    revision: u64,
    commits: u64,
    /// Where the left and right ends were aimed along the line.
    targets: Option<(f32, f32)>,
    left: Option<End>,
    right: Option<End>,
    /// Counts the glides, to name their paint animations.
    glides: u64,
}

/// An end of the hairline on its way.
#[derive(Clone, Copy)]
struct End {
    glide: Glide,
    /// Where this glide started.
    from: f32,
    ends: Instant,
    id: u64,
}

impl End {
    /// Where the end is at `now`, while it's still on its way.
    fn at(&self, now: Instant) -> Option<f32> {
        (now < self.ends).then(|| self.glide.at(now).0)
    }
}

/// Where the hairline's ends are aimed, and the glides of those on their way.
struct Ends {
    left: f32,
    right: f32,
    left_glide: Option<End>,
    right_glide: Option<End>,
}

impl Motion {
    /// A glide of an end to `to`, from where `running` is if it's on its way
    /// and from `from` if not, or `None` when it's there already.
    fn glide(&mut self, running: Option<End>, from: f32, to: f32, now: Instant) -> Option<End> {
        let (glide, from) = match running.and_then(|end| Some((end, end.at(now)?))) {
            Some((end, at)) => (end.glide.retarget(now, to, GLIDE, f32::INFINITY), at),
            None => (Glide::from_rest(now, from, to, GLIDE, f32::INFINITY)?, from),
        };
        self.glides += 1;
        Some(End {
            glide,
            from,
            ends: now + GLIDE,
            id: self.glides,
        })
    }

    /// Moves on to this frame's `line`, made after `revision` changes of the
    /// text and `commits` commits. Returns where the ends are aimed, or
    /// `None` when there is no hairline.
    fn follow(
        &mut self,
        revision: u64,
        commits: u64,
        line: &TentativeLine,
        reduce_motion: bool,
        now: Instant,
    ) -> Option<Ends> {
        let new_words = revision != self.revision;
        let committed = commits != self.commits;
        (self.revision, self.commits) = (revision, commits);
        let pushing = self.left.is_some_and(|end| now < end.ends);
        let targets = match (line.text, line.settled) {
            (Some(text), _) => Some(text),
            // The last words went in, and the fill pushes the line out.
            (None, Some((_, end))) if !reduce_motion && (committed || pushing) => Some((end, end)),
            _ => None,
        };
        let Some((left, right)) = targets else {
            (self.targets, self.left, self.right) = (None, None, None);
            return None;
        };
        let previous = self.targets.replace((left, right));
        if reduce_motion {
            (self.left, self.right) = (None, None);
        } else {
            // The fill and the left end glide from where the fill is to the
            // end of what a commit put in.
            if committed {
                let from = self
                    .left
                    .and_then(|end| end.at(now))
                    .or(line.settled.map(|(start, _)| start))
                    .unwrap_or(left);
                self.left = self.glide(self.left, from, left, now);
            } else if previous.is_none_or(|(before, _)| before != left) {
                self.left = None;
            }
            // The right end glides to new words from where it was, or in from
            // the left under the first ones.
            let right_from = match previous {
                _ if !new_words || line.text.is_none() => None,
                Some((before_left, before_right)) if before_left == left || committed => Some(
                    self.right
                        .and_then(|end| end.at(now))
                        .unwrap_or(before_right),
                ),
                None => Some(left),
                Some(_) => None,
            };
            match right_from {
                Some(from) => self.right = self.glide(self.right, from, right, now),
                None if previous.is_none_or(|(_, before)| before != right) => self.right = None,
                None => {}
            }
        }
        Some(Ends {
            left,
            right,
            left_glide: self.left.filter(|end| now < end.ends),
            right_glide: self.right.filter(|end| now < end.ends),
        })
    }
}

/// The colours and font tentative text paints with.
pub(crate) struct TentativeStyle {
    pub(crate) underline: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) caret: Hsla,
    pub(crate) font: Font,
    pub(crate) font_size: Pixels,
}

/// What a frame paints of tentative text beside its glyphs.
#[derive(Default)]
pub(crate) struct TentativePrepaint {
    color: Hsla,
    /// The parts of the hairline that stay where they are.
    underlines: Vec<Bounds<Pixels>>,
    /// What moves as it paints, each with the clip it paints under. That's
    /// the hairline's gliding ends and the fill's columns.
    moving: Vec<(AnyElement, Bounds<Pixels>)>,
    /// The parts of rows the fill paints, which the text leaves out.
    pub(crate) filling: Vec<Bounds<Pixels>>,
    /// The caret, while it rides the fill's edge.
    pub(crate) caret: Option<AnyElement>,
}

impl TentativePrepaint {
    /// Moves the hairline right of `from` in the row from `top` to `bottom`
    /// by `shift`, with the text the smooth caret moves while it glides.
    pub(crate) fn follow_caret_glide(
        &mut self,
        top: Pixels,
        bottom: Pixels,
        from: Pixels,
        shift: Pixels,
    ) {
        for underline in &mut self.underlines {
            if underline.top() >= top && underline.top() < bottom && underline.left() >= from {
                underline.origin.x += shift;
            }
        }
    }

    pub(crate) fn paint(&mut self, window: &mut Window, cx: &mut App) {
        for underline in &self.underlines {
            window.paint_quad(fill(*underline, self.color));
        }
        for (element, clip) in &mut self.moving {
            window.with_content_mask(Some(ContentMask { bounds: *clip }), |window| {
                element.paint(window, cx)
            });
        }
    }
}

/// Lays out `element` in `bounds`, for a frame that paints it later.
fn place(
    mut element: AnyElement,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    element.layout_as_root(
        size(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::Definite(bounds.size.height),
        ),
        window,
        cx,
    );
    element.prepaint_at(bounds.origin, window, cx);
    element
}

/// The id of a paint animation of the glide numbered `glide`, a child of it
/// for each index of `path`, such as a row and a column of it.
fn glide_id(part: &'static str, glide: u64, path: &[usize]) -> ElementId {
    path.iter()
        .fold(ElementId::NamedInteger(part.into(), glide), |id, ix| {
            ElementId::NamedChild(std::sync::Arc::new(id), ix.to_string().into())
        })
}

/// Where `end` is at the moment a paint animation's frame is drawn, or its
/// target once the animation is over.
fn end_at(end: End, clock: &BackgroundExecutor, progress: f32) -> f32 {
    if progress >= 1. {
        end.glide.target
    } else {
        end.at(clock.now()).unwrap_or(end.glide.target)
    }
}

/// The glyphs of `line` that draw something between `left` and `right`,
/// with a character on either side for ink that reaches past its advance, and
/// their x in the line.
fn glyphs_within(line: &ShapedLine, left: Pixels, right: Pixels) -> Option<(ShapedLine, Pixels)> {
    let text: &str = line.text.as_ref();
    let start = line.closest_index_for_x(left);
    let start = text[..start]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(ix, _)| ix);
    let end = line.closest_index_for_x(right).min(text.len());
    let end = text[end..]
        .graphemes(true)
        .next()
        .map_or(end, |grapheme| end + grapheme.len());
    if end <= start {
        return None;
    }
    let (_, rest) = line.split_at(start);
    let (glyphs, _) = rest.split_at(end - start);
    Some((glyphs, line.x_for_index(start)))
}

impl TentativeText {
    /// Moves the hairline on to this frame's `line` and lays out what the
    /// frame paints.
    pub(crate) fn prepaint(
        &mut self,
        line: Option<TentativeLine>,
        style: TentativeStyle,
        window: &mut Window,
        cx: &mut App,
    ) -> TentativePrepaint {
        let prepaint = self.prepaint_frame(line, style, window, cx);
        #[cfg(test)]
        {
            self.last_frame = (!prepaint.underlines.is_empty() || !prepaint.moving.is_empty())
                .then(|| TentativeFrame {
                    underlines: prepaint.underlines.clone(),
                    moving: prepaint.moving.len(),
                    filling: prepaint.filling.clone(),
                    riding: prepaint.caret.is_some(),
                });
        }
        prepaint
    }

    fn prepaint_frame(
        &mut self,
        line: Option<TentativeLine>,
        style: TentativeStyle,
        window: &mut Window,
        cx: &mut App,
    ) -> TentativePrepaint {
        let clock = cx.background_executor().clone();
        let now = clock.now();
        let Some(line) = line else {
            self.motion = Motion {
                revision: self.revision,
                commits: self.commits,
                glides: self.motion.glides,
                ..Motion::default()
            };
            return TentativePrepaint::default();
        };
        let Some(ends) =
            self.motion
                .follow(self.revision, self.commits, &line, cx.reduce_motion(), now)
        else {
            return TentativePrepaint::default();
        };
        let mut prepaint = TentativePrepaint {
            color: style.underline,
            ..TentativePrepaint::default()
        };
        let (left, right) = (ends.left, ends.right);

        // The hairline from the left end to where the right end's glide
        // starts stays where it is.
        let still_end = ends
            .right_glide
            .map_or(right, |end| end.from.min(right))
            .max(left);
        prepaint.underlines.extend(
            line.segments(left, still_end)
                .filter_map(|segment| line.under_words(line.underline(&segment), segment.row)),
        );

        if let Some(end) = ends.right_glide {
            // The hairline over the span the right end glides across is drawn
            // whole and moved, under a clip that cuts it at the right end.
            let (lo, hi) = (end.from.min(right).max(left), end.from.max(right));
            let growing = end.from < right;
            for (ix, segment) in line.segments(lo, hi).enumerate() {
                let rect = line.underline(&segment);
                // A line drawn in under new words stops where they do. One
                // going back passes over where words were.
                let clip = if growing {
                    match line.under_words(rect, segment.row) {
                        Some(clip) => clip,
                        None => continue,
                    }
                } else {
                    rect
                };
                let span = segment.along.clone();
                let clock = clock.clone();
                let color = style.underline;
                let element = canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| window.paint_quad(fill(bounds, color)),
                )
                .w(rect.size.width)
                .h(rect.size.height)
                .with_paint_animation(
                    glide_id("tentative-right", end.id, &[ix]),
                    Animation::new(GLIDE),
                    move |progress| {
                        let at = end_at(end, &clock, progress);
                        let dx = (at - span.end).clamp(span.start - span.end, 0.);
                        PaintEffect::translation(point(px(dx), px(0.)))
                    },
                )
                .into_any_element();
                prepaint
                    .moving
                    .push((place(element, rect, window, cx), clip));
            }
        }

        if let Some(end) = ends.left_glide {
            self.prepaint_fill(&line, end, left, &style, &clock, &mut prepaint, window, cx);
        }
        prepaint
    }

    /// Lays out the fill of the committed text from where `end` started to
    /// `left`, the hairline's left end on its edge, and the caret riding it.
    #[allow(clippy::too_many_arguments)]
    fn prepaint_fill(
        &self,
        line: &TentativeLine,
        end: End,
        left: f32,
        style: &TentativeStyle,
        clock: &BackgroundExecutor,
        prepaint: &mut TentativePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scale = window.scale_factor();
        let snap = |x: Pixels| px((x.as_f32() * scale).round() / scale);
        let line_height = line.line_height;
        for (ix, segment) in line.segments(end.from, left).enumerate() {
            let row = &line.rows[segment.row];

            // The hairline under the fill, drawn whole and moved, under a clip
            // that cuts it at the fill's edge.
            let rect = line.underline(&segment);
            let span = segment.along.clone();
            // Under the committed words only, as it was while they were
            // tentative.
            if let Some(clip) = line.under_settled_words(rect, segment.row) {
                let color = style.underline;
                let element = canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| window.paint_quad(fill(bounds, color)),
                )
                .w(rect.size.width)
                .h(rect.size.height)
                .with_paint_animation(
                    glide_id("tentative-left", end.id, &[ix]),
                    Animation::new(GLIDE),
                    {
                        let clock = clock.clone();
                        move |progress| {
                            let at = end_at(end, &clock, progress);
                            let dx = (at - span.start).clamp(0., span.end - span.start);
                            PaintEffect::translation(point(px(dx), px(0.)))
                        }
                    },
                )
                .into_any_element();
                prepaint
                    .moving
                    .push((place(element, rect, window, cx), clip));
            }

            // The committed glyphs, in columns that each turn from the muted
            // colour to their own as the fill's edge crosses them.
            let Some((shaped, line_x)) = &row.shaped else {
                continue;
            };
            let muted = window.text_system().shape_line(
                shaped.text.clone(),
                style.font_size,
                &[TextRun {
                    len: shaped.text.len(),
                    font: style.font.clone(),
                    color: style.muted,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );
            let (x0, x1) = (snap(segment.x.start), snap(segment.x.end));
            if x1 <= x0 {
                continue;
            }
            prepaint.filling.push(Bounds::from_corners(
                point(x0, row.origin.y),
                point(x1, row.origin.y + line_height),
            ));
            let columns = (((x1 - x0).as_f32() / COLUMN).ceil() as usize).clamp(1, MAX_COLUMNS);
            let (top, bottom) = (row.origin.y - line_height, row.origin.y + line_height * 2.);
            for column in 0..columns {
                let c0 = snap(x0 + (x1 - x0) * (column as f32 / columns as f32));
                let c1 = snap(x0 + (x1 - x0) * ((column + 1) as f32 / columns as f32));
                if c1 <= c0 {
                    continue;
                }
                let a0 = segment.along.start + (c0 - segment.x.start).as_f32();
                let a1 = segment.along.start + (c1 - segment.x.start).as_f32();
                let clip = Bounds::from_corners(point(c0, top), point(c1, bottom));
                for (copy, filled) in [(shaped, true), (&muted, false)] {
                    let Some((glyphs, x)) = glyphs_within(copy, c0 - *line_x, c1 - *line_x) else {
                        continue;
                    };
                    let origin = point(*line_x + x, row.origin.y);
                    let clock = clock.clone();
                    let element = canvas(
                        |_, _, _| {},
                        move |_, _, window, cx| {
                            glyphs
                                .paint(origin, line_height, TextAlign::Left, None, window, cx)
                                .ok();
                        },
                    )
                    .w(clip.size.width)
                    .h(clip.size.height)
                    .with_paint_animation(
                        glide_id(
                            if filled {
                                "tentative-fill"
                            } else {
                                "tentative-muted"
                            },
                            end.id,
                            &[ix, column],
                        ),
                        Animation::new(GLIDE),
                        move |progress| {
                            let at = end_at(end, &clock, progress);
                            let filled_in = ((at - a0) / (a1 - a0)).clamp(0., 1.);
                            PaintEffect::opacity(if filled { filled_in } else { 1. - filled_in })
                        },
                    )
                    .into_any_element();
                    prepaint
                        .moving
                        .push((place(element, clip, window, cx), clip));
                }
            }
        }

        // The caret rides the fill's edge on its own row, as far as its own
        // place.
        if let Some((caret, caret_at)) = line.caret
            && let Some(row) = line.caret_row(caret)
        {
            let row_start = row.along;
            let color = style.caret;
            let clock = clock.clone();
            let element = canvas(
                |_, _, _| {},
                move |bounds, _, window, _| window.paint_quad(fill(bounds, color)),
            )
            .w(caret.size.width)
            .h(caret.size.height)
            .with_paint_animation(
                glide_id("tentative-caret", end.id, &[]),
                Animation::new(GLIDE),
                move |progress| {
                    let at = end_at(end, &clock, progress);
                    PaintEffect::translation(point(
                        px(at.clamp(row_start, caret_at) - caret_at),
                        px(0.),
                    ))
                },
            )
            .into_any_element();
            prepaint.caret = Some(place(element, caret, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::{
        AppContext as _, Bounds, Context, Entity, EntityInputHandler as _, IntoElement,
        ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window,
        div, px, size,
    };

    use super::TentativeFrame;
    use crate::input::{Copy, SelectAll, Suggestion, TextareaState, Undo};

    /// The width of a character in the test text system.
    const CHAR: f32 = 9.6;

    /// Whether `span` runs from `start` to `end` characters, to the hundredth
    /// of a pixel.
    #[track_caller]
    fn assert_span(span: Option<std::ops::Range<Pixels>>, start: f32, end: f32) {
        let span = span.expect("tentative text in the row");
        assert!(
            (span.start.as_f32() - start * CHAR).abs() < 0.01
                && (span.end.as_f32() - end * CHAR).abs() < 0.01,
            "{span:?} for characters {start} to {end}"
        );
    }

    struct Host {
        textarea: Entity<TextareaState>,
        width: Pixels,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().w(self.width).h(px(200.)).child(self.textarea.clone()))
        }
    }

    /// A focused textarea `width` wide holding `text`, with the caret at
    /// the end.
    struct Test {
        cx: VisualTestContext,
        textarea: Entity<TextareaState>,
    }

    impl Test {
        fn new(cx: &mut TestAppContext, width: f32, text: &'static str) -> Self {
            cx.update(crate::init);
            let window = cx.open_window(size(px(600.), px(400.)), move |window, cx| Host {
                textarea: cx.new(|cx| TextareaState::new(window, cx).default_value(text)),
                width: px(width),
            });
            let cx = VisualTestContext::from_window(window.into(), cx);
            let textarea = window
                .read_with(&cx, |host, _| host.textarea.clone())
                .unwrap();
            let mut test = Self { cx, textarea };
            test.cx.update(|window, _| window.activate_window());
            test.update(|state, window, cx| {
                state.focus(window, cx);
                state.set_selected_range(text.len()..text.len(), cx);
            });
            test
        }

        fn draw(&mut self) {
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
            self.cx.run_until_parked();
        }

        fn update<R>(
            &mut self,
            f: impl FnOnce(&mut TextareaState, &mut Window, &mut Context<TextareaState>) -> R,
        ) -> R {
            let textarea = self.textarea.clone();
            let result = self
                .cx
                .update(|window, cx| textarea.update(cx, |state, cx| f(state, window, cx)));
            self.draw();
            result
        }

        fn read<R>(&self, f: impl FnOnce(&TextareaState) -> R) -> R {
            self.textarea.read_with(&self.cx, |state, _| f(state))
        }

        fn value(&self) -> String {
            self.read(|state| state.value().to_string())
        }

        fn tentative(&self) -> String {
            self.read(|state| state.tentative_text().to_string())
        }

        fn commit(&mut self, len: usize) -> bool {
            self.update(|state, window, cx| state.commit_tentative_text(len, window, cx))
        }

        /// What the last frame laid out of the tentative text.
        fn frame(&self) -> Option<TentativeFrame> {
            self.read(|state| state.tentative.last_frame.clone())
        }

        /// The first line's rows, each with its width and where its
        /// tentative text is.
        fn rows(&self) -> Vec<(Pixels, Option<std::ops::Range<Pixels>>)> {
            self.read(|state| {
                state.last_layout.as_ref().unwrap().lines[0]
                    .wrapped_lines
                    .iter()
                    .map(|row| (row.width, row.inserted_span()))
                    .collect()
            })
        }

        /// The caret's bounds as the last frame laid it out.
        fn caret(&self) -> Bounds<Pixels> {
            self.read(|state| state.cursor_layout().unwrap().0)
        }

        fn advance(&mut self, ms: u64) {
            self.cx.executor().advance_clock(Duration::from_millis(ms));
            self.draw();
        }
    }

    /// Tentative text shows after the caret in room of its own, with the
    /// hairline under it from the caret on, and isn't in the document. A
    /// committed prefix goes into the document where it showed, the caret
    /// follows it, and nothing moves. Clearing takes the rest away.
    #[gpui::test]
    fn set_commit_and_clear(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" big world", cx));
        assert_eq!(test.value(), "Hello");
        assert_eq!(test.tentative(), " big world");
        let rows = test.rows();
        assert_eq!(rows.len(), 1);
        assert_span(rows[0].1.clone(), 5., 15.);
        let width = rows[0].0;
        assert!((width.as_f32() - 15. * CHAR).abs() < 0.01);
        // The hairline draws in from the left, then stays.
        assert!(test.frame().is_some_and(|frame| frame.moving > 0));
        test.advance(200);
        let caret = test.caret();
        let frame = test.frame().expect("a hairline");
        assert_eq!(frame.underlines.len(), 1);
        assert_eq!(frame.moving, 0);
        // From the first letter to the last, past the space before them.
        let underline = frame.underlines[0];
        assert!((underline.left() - caret.left() - px(CHAR)).abs() < px(0.01));
        assert!((underline.size.width - px(9. * CHAR)).abs() < px(0.01));
        assert_eq!(underline.size.height, px(1.));

        assert!(test.commit(4));
        assert_eq!(test.value(), "Hello big");
        assert_eq!(test.tentative(), " world");
        assert_eq!(test.read(|state| state.cursor()), 9);
        let rows = test.rows();
        assert_eq!(rows[0].0, width, "nothing moved");
        assert_span(rows[0].1.clone(), 9., 15.);
        // The committed word fills in, and the caret rides the fill's edge.
        let frame = test.frame().expect("a hairline");
        assert_eq!(frame.filling.len(), 1);
        assert!(frame.moving > 0 && frame.riding);

        test.advance(200);
        let frame = test.frame().expect("a hairline");
        assert!(frame.filling.is_empty() && frame.moving == 0 && !frame.riding);
        assert!((frame.underlines[0].left() - test.caret().left() - px(CHAR)).abs() < px(0.01));

        test.update(|state, _, cx| state.clear_tentative_text(cx));
        assert_eq!(test.tentative(), "");
        assert_eq!(test.value(), "Hello big");
        assert_eq!(test.rows()[0].1, None);
        assert!((test.rows()[0].0.as_f32() - 9. * CHAR).abs() < 0.01);
        assert!(test.frame().is_none());
    }

    /// Committing the whole run pushes the hairline out, and it's gone once
    /// the fill is done.
    #[gpui::test]
    fn committing_the_whole_run_pushes_the_line_out(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" world", cx));
        assert!(test.commit(6));
        assert_eq!(test.value(), "Hello world");
        assert_eq!(test.tentative(), "");
        assert!(test.frame().is_some_and(|frame| frame.moving > 0));
        test.advance(200);
        assert!(test.frame().is_none());
    }

    /// Text set at an offset stays there as the text around it changes, and
    /// committing it leaves the caret where it was.
    #[gpui::test]
    fn tentative_text_at_an_offset(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text_at(0, "Well, ", cx));
        assert_span(test.rows()[0].1.clone(), 0., 6.);
        test.cx.simulate_input("!");
        test.draw();
        assert_eq!(test.value(), "Hello!");
        assert!(test.commit(6));
        assert_eq!(test.value(), "Well, Hello!");
        assert_eq!(test.read(|state| state.cursor()), 12);
    }

    /// Tentative text set inside a token shows after it, and what's committed
    /// of it goes in there, so the rest follows what went in. The same holds
    /// when the text it's in becomes a token after it was set.
    #[gpui::test]
    fn tentative_text_stays_out_of_tokens(cx: &mut TestAppContext) {
        use crate::input::InlineToken;

        let token = || InlineToken::new("note", "@note");
        for token_first in [true, false] {
            let mut test = Test::new(cx, 400., "hello world");
            let make_token = |test: &mut Test| {
                test.update(|state, window, cx| {
                    state
                        .replace_range_with_token(0..5, token(), window, cx)
                        .unwrap()
                });
            };
            if token_first {
                make_token(&mut test);
            }
            test.update(|state, _, cx| state.set_tentative_text_at(2, "xy", cx));
            if !token_first {
                make_token(&mut test);
            }
            let shown = |test: &Test| {
                test.read(|state| {
                    state
                        .display_map
                        .inline_text()
                        .map(|(inline, _)| inline.offset)
                })
            };
            assert_eq!(
                shown(&test),
                Some(5),
                "after the token, token first {token_first}"
            );
            assert!(test.commit(1));
            assert_eq!(test.value(), "@notex world");
            assert_eq!(test.tentative(), "y");
            assert_eq!(
                shown(&test),
                Some(6),
                "after what went in, token first {token_first}"
            );
            assert!(test.commit(1));
            assert_eq!(test.value(), "@notexy world");
        }
    }

    /// An input method's composition hides the tentative text, which can't be
    /// committed until the composition ends.
    #[gpui::test]
    fn a_composition_takes_precedence(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" world", cx));
        test.update(|state, window, cx| {
            state.replace_and_mark_text_in_range(None, "ni", None, window, cx)
        });
        assert_eq!(test.rows()[0].1, None);
        assert!(test.frame().is_none());
        assert!(!test.commit(6));
        assert_eq!(test.value(), "Helloni");
        test.update(|state, window, cx| state.replace_text_in_range(None, "你", window, cx));
        assert_eq!(test.value(), "Hello你");
        assert_eq!(test.tentative(), " world");
        // It shows again, after the caret.
        let caret_x = test.read(|state| {
            state.last_layout.as_ref().unwrap().lines[0].wrapped_lines[0].x_for_index(8)
        });
        assert_eq!(
            test.rows()[0].1.clone().map(|span| span.start),
            Some(caret_x)
        );
        assert!(test.commit(6));
        assert_eq!(test.value(), "Hello你 world");
    }

    /// Undo never brings tentative text into the document. A commit is an
    /// edit of its own, which Undo takes back.
    #[gpui::test]
    fn undo_leaves_tentative_text_out(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" big world", cx));
        test.cx.dispatch_action(Undo);
        test.draw();
        assert_eq!(test.value(), "Hello");
        test.cx.simulate_input("!");
        test.draw();
        assert!(test.commit(4));
        assert_eq!(test.value(), "Hello! big");
        test.cx.dispatch_action(Undo);
        test.draw();
        assert_eq!(test.value(), "Hello!");
        assert_eq!(test.tentative(), " world");
        test.cx.dispatch_action(Undo);
        test.draw();
        assert_eq!(test.value(), "Hello");
    }

    /// Undo and redo of a commit bring back every caret where it was, the
    /// ones elsewhere included.
    #[gpui::test]
    fn undo_and_redo_of_a_commit_keep_every_caret(cx: &mut TestAppContext) {
        use crate::input::Redo;

        let mut test = Test::new(cx, 400., "Hello there");
        test.update(|state, _, cx| {
            state.set_selected_range(2..2, cx);
            state.add_cursor_at(11, cx);
            state.set_tentative_text_at(5, " big", cx);
        });
        let carets = |test: &Test| {
            test.read(|state| {
                let mut carets: Vec<_> = state.selections.iter().map(|s| s.start..s.end).collect();
                carets.sort_by_key(|range| range.start);
                carets
            })
        };
        assert_eq!(carets(&test), [2..2, 11..11]);
        assert!(test.commit(4));
        assert_eq!(test.value(), "Hello big there");
        assert_eq!(carets(&test), [2..2, 15..15]);
        test.cx.dispatch_action(Undo);
        test.draw();
        assert_eq!(test.value(), "Hello there");
        assert_eq!(carets(&test), [2..2, 11..11]);
        test.cx.dispatch_action(Redo);
        test.draw();
        assert_eq!(test.value(), "Hello big there");
        assert_eq!(carets(&test), [2..2, 15..15]);
    }

    /// Selecting everything and copying leaves the tentative text out.
    #[gpui::test]
    fn copying_leaves_tentative_text_out(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" big world", cx));
        assert!(test.commit(4));
        test.cx.dispatch_action(SelectAll);
        test.cx.dispatch_action(Copy);
        test.draw();
        let copied = test.cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("Hello big"));
    }

    /// The fill's edge glides from the start of the committed text to its
    /// end on the smooth caret's spring, fastest at the start, and arrives in
    /// the smooth caret's settle time for a word.
    #[gpui::test]
    fn the_fill_glides_on_the_smooth_caret_spring(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.update(|state, _, cx| state.set_tentative_text(" big world", cx));
        assert!(test.commit(4));
        let end = test
            .read(|state| state.tentative.motion.left)
            .expect("the fill glides");
        // From the committed space to the first letter of the words after it,
        // where the hairline starts.
        assert!((end.from - 5. * CHAR).abs() < 0.01);
        assert!((end.glide.target - 10. * CHAR).abs() < 0.01);
        let start = test.cx.executor().now();
        assert_eq!(end.ends - start, super::GLIDE);
        let at = |ms: u64| end.glide.at(start + Duration::from_millis(ms)).0;
        let (from, to) = (end.from, end.glide.target);
        assert!(at(30) - from > (to - from) / 2., "{} after 30 ms", at(30));
        assert!(at(60) < to - 0.5 && at(60) > at(30));
        assert!((at(120) - to).abs() <= 0.5);
        test.advance(60);
        assert!(test.frame().is_some_and(|frame| frame.moving > 0));
        test.advance(60);
        assert!(test.frame().is_some_and(|frame| frame.moving == 0));
    }

    /// With reduced motion a commit fills in at once and the hairline steps.
    #[gpui::test]
    fn reduced_motion_steps_at_once(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hello");
        test.cx.update(|_, cx| cx.set_reduce_motion(true));
        test.update(|state, _, cx| state.set_tentative_text(" big world", cx));
        assert!(test.frame().is_some_and(|frame| frame.moving == 0));
        assert!(test.commit(4));
        let frame = test.frame().expect("a hairline");
        assert!(frame.moving == 0 && frame.filling.is_empty() && !frame.riding);
        assert!((frame.underlines[0].left() - test.caret().left() - px(CHAR)).abs() < px(0.01));
        assert!(test.commit(6));
        assert!(test.frame().is_none());
    }

    /// Tentative text wraps with its line, the hairline is drawn under it in
    /// each row it's in, and committing some of it moves no row.
    #[gpui::test]
    fn tentative_text_wraps_with_its_line(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 160., "Hello");
        test.cx.update(|_, cx| cx.set_reduce_motion(true));
        test.update(|state, _, cx| state.set_tentative_text(" one two three four five", cx));
        let rows = test.rows();
        assert!(rows.len() >= 2, "{rows:?}");
        assert!(rows.iter().all(|(_, span)| span.is_some()), "{rows:?}");
        let frame = test.frame().expect("a hairline");
        assert_eq!(frame.underlines.len(), rows.len());
        let line_height = test.read(|state| state.last_layout.as_ref().unwrap().line_height);
        for pair in frame.underlines.windows(2) {
            assert_eq!(pair[1].top() - pair[0].top(), line_height);
        }
        let first_row_top = test.caret().top() - (line_height - test.caret().size.height) / 2.;
        let bottom = first_row_top + line_height;
        assert!(frame.underlines[0].top() <= bottom - px(1.));
        assert!(frame.underlines[0].top() >= bottom - px(2.) - px(0.01));
        assert_eq!(test.value(), "Hello");

        assert!(test.commit(8));
        assert_eq!(test.value(), "Hello one two");
        let after = test.rows();
        assert_eq!(
            after.iter().map(|(width, _)| *width).collect::<Vec<_>>(),
            rows.iter().map(|(width, _)| *width).collect::<Vec<_>>(),
            "no row moved"
        );
    }

    /// A pane that holds a textarea, draws again when the textarea notifies,
    /// as a cached view has to, and counts its draws.
    struct Pane {
        textarea: Entity<TextareaState>,
        renders: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().child(self.textarea.clone())
        }
    }

    struct PaneWindow {
        pane: Entity<Pane>,
    }

    impl Render for PaneWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
        }
    }

    /// The fill and the hairline's ends move as they paint. The frames of a
    /// glide draw no view, the pane around the textarea included, and the
    /// glide asks for frames until it's done.
    #[gpui::test]
    fn a_glide_draws_no_view(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let window = cx.open_window(size(px(400.), px(100.)), {
            let renders = renders.clone();
            move |window, cx| PaneWindow {
                pane: cx.new(|cx| Pane {
                    textarea: cx.new(|cx| TextareaState::new(window, cx).default_value("Hello")),
                    renders,
                }),
            }
        });
        let textarea = window
            .update(cx, |root, _, cx| root.pane.read(cx).textarea.clone())
            .unwrap();
        let next_frame = |cx: &mut TestAppContext| {
            cx.executor().advance_clock(Duration::from_micros(16_667));
            let asked = window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            cx.run_until_parked();
            asked > 0
        };
        window
            .update(cx, |_, _, cx| {
                textarea.update(cx, |state, cx| {
                    state.set_selected_range(5..5, cx);
                    state.set_tentative_text(" big world", cx);
                })
            })
            .unwrap();
        cx.run_until_parked();
        for _ in 0..12 {
            next_frame(cx);
        }
        window
            .update(cx, |_, window, cx| {
                textarea.update(cx, |state, cx| {
                    assert!(state.commit_tentative_text(4, window, cx));
                })
            })
            .unwrap();
        cx.run_until_parked();
        next_frame(cx);
        let before = renders.get();
        let asked = (0..12).filter(|_| next_frame(cx)).count();
        assert_eq!(renders.get(), before, "a view drew while the fill glided");
        assert!(
            (5..12).contains(&asked),
            "the glide asked for {asked} frames"
        );
    }

    /// Unwrapped text scrolls as far as its tentative text reaches, and the
    /// hairline under a wrapped row starts where the row's indent puts it.
    #[gpui::test]
    fn tentative_text_takes_the_row_geometry_of_the_text(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 200., "  Hello");
        test.update(|state, window, cx| state.set_soft_wrap(false, window, cx));
        let width = |test: &Test| test.read(|state| state.scroll_size.width);
        let before = width(&test);
        test.update(|state, _, cx| {
            state.set_tentative_text(" and a long run of words after it", cx)
        });
        // The seven characters and the tentative text's 33.
        assert!(
            width(&test) >= px(40. * CHAR) && before < px(8. * CHAR),
            "{:?} from {before:?}",
            width(&test)
        );

        let mut test = Test::new(cx, 160., "  Hello");
        test.cx.update(|_, cx| cx.set_reduce_motion(true));
        test.update(|state, _, cx| state.set_tentative_text(" one two three four five", cx));
        let frame = test.frame().expect("a hairline");
        assert!(frame.underlines.len() >= 2);
        let content_left = test.read(|state| state.range_to_bounds(&(0..0)).unwrap().left());
        assert!(
            (frame.underlines[1].left() - content_left - px(2. * CHAR)).abs() < px(0.01),
            "{:?} from {content_left:?}",
            frame.underlines[1]
        );
    }

    /// While tentative text shows, no suggestions are offered, and those
    /// offered close when it comes.
    #[gpui::test]
    fn suggestions_wait_for_tentative_text(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, 400., "Hel");
        let offered = |test: &Test| test.read(|state| state.is_suggestion_menu_open());
        test.update(|state, _, cx| state.present_suggestions(vec![Suggestion::new("Hello")], cx));
        assert!(offered(&test));
        test.update(|state, _, cx| state.set_tentative_text(" there", cx));
        assert!(!offered(&test));
        test.update(|state, _, cx| state.present_suggestions(vec![Suggestion::new("Hello")], cx));
        assert!(!offered(&test));
        test.update(|state, _, cx| state.clear_tentative_text(cx));
        test.update(|state, _, cx| state.present_suggestions(vec![Suggestion::new("Hello")], cx));
        assert!(offered(&test));
    }
}
