//! Visual modes: characterwise, linewise and blockwise selections that
//! motions extend and operators act on.
//!
//! Vim's selection includes the character under the caret. The textarea's
//! selection holds that character too, and the caret is drawn on it through
//! [`crate::input::KeymapState::caret_offset`]. A block is one selection per
//! row.

use std::ops::Range;

use gpui::{Context, Window};
use ropey::Rope;

use super::{
    Change, Command, Target as RepeatTarget, VimMode, VimState,
    object::Object,
    operator::{Operator, Region, edit},
    register::{self, RegisterKind, Store},
    text,
};
use crate::input::{TextareaState, cursor::CursorSelection};

/// Which visual mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisualKind {
    /// `v`
    Char,
    /// `V`
    Line,
    /// Ctrl-V
    Block,
}

impl VisualKind {
    pub(super) fn mode(self) -> VimMode {
        match self {
            Self::Char => VimMode::Visual,
            Self::Line => VimMode::VisualLine,
            Self::Block => VimMode::VisualBlock,
        }
    }

    /// The value of `vim_visual` in the key context.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Char => "char",
            Self::Line => "line",
            Self::Block => "block",
        }
    }
}

/// A visual selection: where it started and where the caret is. Both are on
/// a character, or on a line's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Visual {
    pub(super) kind: VisualKind,
    pub(super) anchor: usize,
    pub(super) head: usize,
    /// A block reaches each line's end, after `$`.
    pub(super) to_line_end: bool,
}

/// The size of a visual selection, for `.` to act on as much text again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualExtent {
    kind: VisualKind,
    /// Rows, at least one.
    rows: usize,
    /// Characters: on the one row, or on the last of several. Columns for a
    /// block.
    columns: usize,
    to_line_end: bool,
}

/// The offset after the character at `offset`, taking a line break whole.
fn char_end(text: &Rope, offset: usize) -> usize {
    let row = text::row(text, offset);
    if offset >= text::line_end(text, row) {
        text::next_line_start(text, row)
    } else {
        text::next_offset(text, offset)
    }
}

impl Visual {
    fn rows(&self, text: &Rope) -> Range<usize> {
        let (a, b) = (text::row(text, self.anchor), text::row(text, self.head));
        a.min(b)..a.max(b) + 1
    }

    /// The block's columns: from the left one up to past the right one, or
    /// to each line's end.
    fn columns(&self, text: &Rope) -> (usize, Option<usize>) {
        let (a, b) = (
            text::column(text, self.anchor),
            text::column(text, self.head),
        );
        let right = (!self.to_line_end).then(|| a.max(b) + 1);
        (a.min(b), right)
    }

    /// What the selection covers, for an operator.
    pub(super) fn region(&self, text: &Rope) -> Region {
        match self.kind {
            VisualKind::Char => {
                let start = self.anchor.min(self.head);
                let end = char_end(text, self.anchor.max(self.head)).min(text.len());
                Region::Chars(start..end)
            }
            VisualKind::Line => Region::Lines(self.rows(text)),
            VisualKind::Block => {
                let (left, right) = self.columns(text);
                Region::Block {
                    rows: self.rows(text),
                    left,
                    right,
                }
            }
        }
    }

    /// The textarea's selections: the head's first. And whether the first is
    /// reversed.
    fn selections(&self, text: &Rope) -> (Vec<Range<usize>>, bool) {
        let reversed = self.head < self.anchor;
        match self.region(text) {
            Region::Chars(range) => (vec![range], reversed),
            Region::Lines(rows) => (vec![text::rows_range(text, &rows)], reversed),
            Region::Block { rows, left, right } => {
                let head_row = text::row(text, self.head);
                let mut segments = Region::segments(text, &rows, left, right);
                let head = head_row - rows.start;
                let first = segments.remove(head);
                segments.insert(0, first);
                (segments, self.head < self.anchor)
            }
        }
    }

    fn extent(&self, text: &Rope) -> VisualExtent {
        let rows = self.rows(text).len();
        let columns = match self.kind {
            VisualKind::Char => {
                let start = self.anchor.min(self.head);
                let end = self.anchor.max(self.head);
                if rows == 1 {
                    text.slice(start..end).chars().count() + 1
                } else {
                    text::column(text, end) + 1
                }
            }
            VisualKind::Line => 0,
            VisualKind::Block => {
                let (left, right) = self.columns(text);
                right.unwrap_or(left + 1) - left
            }
        };
        VisualExtent {
            kind: self.kind,
            rows,
            columns,
            to_line_end: self.to_line_end,
        }
    }
}

impl VisualExtent {
    /// The same amount of text from `from`.
    pub(super) fn region(&self, text: &Rope, from: usize) -> Region {
        let last_row = text::last_row(text);
        let row = text::row(text, from);
        let end_row = (row + self.rows - 1).min(last_row);
        match self.kind {
            VisualKind::Char => {
                let end = if self.rows == 1 {
                    let mut end = from;
                    for _ in 0..self.columns {
                        end = char_end(text, end);
                    }
                    end
                } else {
                    char_end(
                        text,
                        text::offset_at_column(text, end_row, self.columns - 1),
                    )
                };
                Region::Chars(from..end.min(text.len()))
            }
            VisualKind::Line => Region::Lines(row..end_row + 1),
            VisualKind::Block => {
                let left = text::column(text, from);
                Region::Block {
                    rows: row..end_row + 1,
                    left,
                    right: (!self.to_line_end).then_some(left + self.columns),
                }
            }
        }
    }
}

impl VimState {
    /// `v`, `V` and Ctrl-V: start the visual mode, switch to it from another,
    /// or leave it when already in it.
    pub(super) fn toggle_visual(
        &mut self,
        kind: VisualKind,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        match self.visual {
            Some(visual) if visual.kind == kind => {
                self.enter_normal_at(visual.head, state, cx);
            }
            Some(mut visual) => {
                visual.kind = kind;
                self.set_visual(visual, state, cx);
            }
            None => {
                let caret = state.cursor();
                self.set_visual(
                    Visual {
                        kind,
                        anchor: caret,
                        head: caret,
                        to_line_end: false,
                    },
                    state,
                    cx,
                );
            }
        }
    }

    /// Enter or update visual mode with `visual`, and select it.
    pub(super) fn set_visual(
        &mut self,
        visual: Visual,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let len = state.text.len();
        let visual = Visual {
            anchor: text::clip(&state.text, visual.anchor.min(len)),
            head: text::clip(&state.text, visual.head.min(len)),
            ..visual
        };
        self.mode = visual.kind.mode();
        self.visual = Some(visual);
        let (ranges, reversed) = visual.selections(&state.text);
        state.selections.remove_all_but_active();
        let mut ranges = ranges.into_iter();
        if let Some(first) = ranges.next() {
            state.set_selection(first.start, first.end);
            state.active_selection_mut().reversed = reversed;
        }
        for range in ranges {
            let id = state.selections.generate_id();
            state
                .selections
                .add(CursorSelection::new(id, range.start, range.end));
        }
        self.selected = Some(selection_snapshot(state));
        state.scroll_to(visual.head, None, cx);
        state.pause_blink_cursor(cx);
        cx.notify();
    }

    /// Whether the textarea still shows the selection visual mode made.
    pub(super) fn visual_is_current(&self, state: &TextareaState) -> bool {
        self.selected.as_ref() == Some(&selection_snapshot(state))
    }

    /// Move the head of the selection.
    pub(super) fn extend_visual(
        &mut self,
        head: usize,
        to_line_end: bool,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        if let Some(mut visual) = self.visual {
            visual.head = head;
            visual.to_line_end = to_line_end;
            self.set_visual(visual, state, cx);
        }
    }

    /// `o`: the caret to the other end of the selection. `O` in a block: to
    /// the other end of the same row.
    pub(super) fn swap_visual_ends(
        &mut self,
        same_row: bool,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(mut visual) = self.visual else {
            return;
        };
        let text = &state.text;
        if same_row && visual.kind == VisualKind::Block {
            let (anchor_row, head_row) =
                (text::row(text, visual.anchor), text::row(text, visual.head));
            let (anchor_column, head_column) = (
                text::column(text, visual.anchor),
                text::column(text, visual.head),
            );
            visual.anchor = text::offset_at_column(text, anchor_row, head_column);
            visual.head = text::offset_at_column(text, head_row, anchor_column);
        } else {
            std::mem::swap(&mut visual.anchor, &mut visual.head);
        }
        self.set_visual(visual, state, cx);
    }

    /// `gv`: select what was last selected, swapping with the current
    /// selection in visual mode.
    pub(super) fn reselect_visual(
        &mut self,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(previous) = self.last_visual else {
            return;
        };
        if let Some(current) = self.visual {
            self.last_visual = Some(current);
        }
        self.set_visual(previous, state, cx);
    }

    /// An operator typed in visual mode acts on the selection.
    pub(super) fn visual_operator(
        &mut self,
        operator: Operator,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(visual) = self.visual else {
            return;
        };
        let count = self.take_count();
        let region = visual.region(&state.text);
        let extent = visual.extent(&state.text);
        let start = match &region {
            Region::Chars(range) => range.start,
            Region::Lines(rows) => {
                let head_row = text::row(&state.text, visual.head);
                if head_row == rows.start {
                    visual.head
                } else {
                    text::offset_at_column(
                        &state.text,
                        rows.start,
                        text::column(&state.text, visual.anchor),
                    )
                }
            }
            Region::Block { rows, left, .. } => {
                text::offset_at_column(&state.text, rows.start, *left)
            }
        };
        self.leave_visual();
        let times = if matches!(operator, Operator::Indent | Operator::Outdent) {
            count.unwrap_or(1)
        } else {
            1
        };
        let register = self.register;
        self.apply_operator(operator, region, start, false, times, state, window, cx);
        if operator.changes_text() {
            let change = Change {
                command: Command::Operate {
                    operator,
                    target: RepeatTarget::Visual(extent),
                },
                count: (times > 1).then_some(times),
                register,
                text: None,
            };
            self.record(change);
        }
    }

    /// Leave visual mode, remembering the selection for `gv`.
    pub(super) fn leave_visual(&mut self) {
        if let Some(visual) = self.visual.take() {
            self.last_visual = Some(visual);
        }
        self.selected = None;
        if self.mode.is_visual() {
            self.mode = VimMode::Normal;
        }
    }

    /// The selection's characters in a visual mode, or its lines after `V`
    /// or with `linewise`, in a register.
    pub(super) fn visual_region(&self, state: &TextareaState, linewise: bool) -> Option<Region> {
        let visual = self.visual?;
        let region = visual.region(&state.text);
        Some(match region {
            Region::Chars(_) | Region::Block { .. } if linewise => {
                Region::Lines(region.rows(&state.text))
            }
            region => region,
        })
    }

    /// `X`, `D`, `Y`, `S`, `R` and `C` in visual mode: whole lines. In a
    /// block, `D` and `C` act to the ends of its lines instead.
    pub(super) fn visual_line_operator(
        &mut self,
        operator: Operator,
        block_to_line_end: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(mut visual) = self.visual else {
            return;
        };
        match visual.kind {
            VisualKind::Block if block_to_line_end => {
                visual.to_line_end = true;
                self.visual = Some(visual);
            }
            _ => {
                visual.kind = VisualKind::Line;
                self.visual = Some(visual);
            }
        }
        self.visual_operator(operator, state, window, cx);
    }

    /// `p` and `P` in visual mode: replace the selection with a register.
    /// `p` keeps what it replaced in the unnamed register, `P` does not.
    pub(super) fn visual_put(
        &mut self,
        keep_replaced: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(visual) = self.visual else {
            return;
        };
        let count = self.take_count().unwrap_or(1);
        let name = self.register.take();
        let Some(put) = self.read_register(name, cx) else {
            self.leave_visual();
            self.enter_normal_at(visual.anchor.min(visual.head), state, cx);
            return;
        };
        let text = state.text.clone();
        let region = visual.region(&text);
        let replaced = region.register(&text);
        self.leave_visual();
        let repeated = put.text.repeat(count);
        let (range, inserted, caret_at_line) = match (&region, put.kind) {
            (Region::Lines(rows), RegisterKind::Lines) => (
                text::rows_range(&text, rows),
                with_line_break(&text, rows, repeated),
                true,
            ),
            (Region::Lines(rows), _) => (
                text::rows_range(&text, rows),
                with_line_break(&text, rows, format!("{repeated}\n")),
                false,
            ),
            (Region::Chars(range), RegisterKind::Lines) => {
                (range.clone(), format!("\n{repeated}"), true)
            }
            (Region::Chars(range), _) => (range.clone(), repeated, false),
            (Region::Block { rows, left, right }, _) => {
                // The block's text goes and the register goes in at its top.
                let segments = Region::segments(&text, rows, *left, *right);
                let mut edits: Vec<(Range<usize>, String)> = segments
                    .iter()
                    .skip(1)
                    .map(|segment| (segment.clone(), String::new()))
                    .collect();
                let first = segments[0].clone();
                edits.push((first.clone(), repeated.trim_end_matches('\n').to_string()));
                if keep_replaced {
                    register::store(None, Store::Delete { big: true }, replaced, cx);
                }
                edit(state, edits, window, cx);
                self.enter_normal_at(first.start, state, cx);
                return;
            }
        };
        if keep_replaced {
            register::store(
                None,
                Store::Delete {
                    big: replaced.kind != RegisterKind::Chars,
                },
                replaced,
                cx,
            );
        }
        let start = range.start;
        edit(state, vec![(range, inserted.clone())], window, cx);
        let caret = if caret_at_line {
            let row = text::row(&state.text, start) + usize::from(inserted.starts_with('\n'));
            text::first_non_blank(&state.text, row.min(text::last_row(&state.text)))
        } else if inserted.contains('\n') {
            start
        } else {
            text::prev_offset(&state.text, start + inserted.len()).max(start)
        };
        self.enter_normal_at(caret, state, cx);
    }

    /// `r` in visual mode: every selected character becomes `character`.
    pub(super) fn visual_replace(
        &mut self,
        character: char,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(visual) = self.visual else {
            return;
        };
        let text = state.text.clone();
        let region = visual.region(&text);
        let ranges: Vec<Range<usize>> = match &region {
            Region::Chars(range) => vec![range.clone()],
            Region::Lines(rows) => rows
                .clone()
                .map(|row| text::line_start(&text, row)..text::line_end(&text, row))
                .collect(),
            Region::Block { rows, left, right } => Region::segments(&text, rows, *left, *right),
        };
        let start = ranges.first().map_or(0, |range| range.start);
        let edits = ranges
            .into_iter()
            .filter(|range| !range.is_empty())
            .map(|range| {
                let replaced: String = text
                    .slice(range.clone())
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' { c } else { character })
                    .collect();
                (range, replaced)
            })
            .collect();
        self.leave_visual();
        edit(state, edits, window, cx);
        self.enter_normal_at(start, state, cx);
    }

    /// `I` and `A` in a block: type before or after it on every row.
    pub(super) fn visual_block_insert(
        &mut self,
        after: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(visual) = self.visual else {
            return;
        };
        let text = state.text.clone();
        let rows = visual.rows(&text);
        let (left, right) = visual.columns(&text);
        self.leave_visual();
        let mut padding = Vec::new();
        let mut carets = Vec::new();
        for row in rows {
            let chars = text::line_chars(&text, row);
            if !after {
                if chars >= left {
                    carets.push(text::offset_at_column(&text, row, left));
                }
                continue;
            }
            match right {
                None => carets.push(text::line_end(&text, row)),
                Some(right) => {
                    let end = text::line_end(&text, row);
                    if chars < right {
                        // Short lines are padded to the block's edge.
                        padding.push((end..end, " ".repeat(right - chars)));
                    }
                    carets.push(text::offset_at_column(&text, row, right));
                }
            }
        }
        if !padding.is_empty() {
            edit(state, padding.clone(), window, cx);
            // Every padded row moves the carets after it.
            let mut shift = 0;
            let mut pads = padding.iter().peekable();
            for caret in carets.iter_mut() {
                while let Some((range, pad)) = pads.peek() {
                    if range.start <= *caret {
                        shift += pad.len();
                        pads.next();
                    } else {
                        break;
                    }
                }
                *caret += shift;
            }
        }
        self.start_block_insert(carets, state, window, cx);
    }

    /// A text object typed in visual mode selects it.
    pub(super) fn visual_object(
        &mut self,
        object: Object,
        around: bool,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(visual) = self.visual else {
            return;
        };
        let count = self.take_count().unwrap_or(1);
        let text = state.text.clone();
        let selection = visual.region(&text);
        let selection = match selection {
            Region::Chars(range) => Some(range),
            _ => None,
        };
        let Some(found) = object.range(&text, visual.head, around, count, selection) else {
            return;
        };
        if found.range.is_empty() {
            return;
        }
        let kind = if found.linewise {
            VisualKind::Line
        } else if visual.kind == VisualKind::Line {
            VisualKind::Char
        } else {
            visual.kind
        };
        let head = text::prev_offset(&text, found.range.end).max(found.range.start);
        self.set_visual(
            Visual {
                kind,
                anchor: found.range.start,
                head,
                to_line_end: false,
            },
            state,
            cx,
        );
    }

    /// Select all the text, as Cmd-A does on macOS.
    pub(super) fn select_all(
        &mut self,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let text = &state.text;
        let head = text::prev_offset(text, text.len());
        self.set_visual(
            Visual {
                kind: VisualKind::Char,
                anchor: 0,
                head,
                to_line_end: false,
            },
            state,
            cx,
        );
    }

    /// Start visual mode from a selection made some other way, such as with
    /// the mouse.
    pub(super) fn visual_from_selection(
        &mut self,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let selection = *state.active_selection();
        let text = &state.text;
        let last = text::prev_offset(text, selection.end).max(selection.start);
        let (anchor, head) = if selection.reversed {
            (last, selection.start)
        } else {
            (selection.start, last)
        };
        self.set_visual(
            Visual {
                kind: VisualKind::Char,
                anchor,
                head,
                to_line_end: false,
            },
            state,
            cx,
        );
    }
}

/// `inserted` replacing whole `rows`, with a line break kept after it unless
/// the rows end the text.
fn with_line_break(text: &Rope, rows: &Range<usize>, mut inserted: String) -> String {
    if rows.end > text::last_row(text) && inserted.ends_with('\n') {
        inserted.pop();
    }
    inserted
}

/// The selections as they stand, to notice a change made outside Vim.
pub(super) fn selection_snapshot(state: &TextareaState) -> Vec<(usize, usize, bool)> {
    state
        .selections
        .iter()
        .map(|selection| (selection.start, selection.end, selection.reversed))
        .collect()
}
