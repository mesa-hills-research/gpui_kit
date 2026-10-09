//! Operators, the regions they act on, and what each leaves behind: the
//! registers it fills and where the caret goes.

use std::ops::Range;

use gpui::{Context, Window};
use ropey::Rope;

use super::{
    VimState,
    insert::InsertRepeat,
    motion::{MotionKind, Target},
    register::{self, Register, RegisterKind, Store},
    text,
};
use crate::input::TextareaState;

/// What an operator does to the text it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    /// `d`
    Delete,
    /// `c`
    Change,
    /// `y`
    Yank,
    /// `>`
    Indent,
    /// `<`
    Outdent,
    /// `g~`
    ToggleCase,
    /// `gu`
    Lowercase,
    /// `gU`
    Uppercase,
}

impl Operator {
    /// The value of `vim_operator` in the key context while the operator
    /// waits for its motion.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Change => "change",
            Self::Yank => "yank",
            Self::Indent => "indent",
            Self::Outdent => "outdent",
            Self::ToggleCase => "toggle_case",
            Self::Lowercase => "lowercase",
            Self::Uppercase => "uppercase",
        }
    }

    /// Whether the operator changes the text, so `.` repeats it.
    pub(super) fn changes_text(self) -> bool {
        self != Self::Yank
    }
}

/// The text an operator acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Region {
    /// The characters in a range.
    Chars(Range<usize>),
    /// Whole rows.
    Lines(Range<usize>),
    /// The same columns of several rows: `left` up to `right`, or to each
    /// line's end.
    Block {
        rows: Range<usize>,
        left: usize,
        right: Option<usize>,
    },
}

impl Region {
    /// The region a motion from `from` to `target` covers, with Vim's rules
    /// for exclusive motions that end at the start of a line: the end moves
    /// back to the end of the line before, and when the motion started in
    /// its line's indentation, it takes whole lines.
    pub(super) fn for_motion(text: &Rope, from: usize, target: &Target) -> Self {
        let (start, end) = (from.min(target.offset), from.max(target.offset));
        match target.kind {
            MotionKind::Linewise => Self::Lines(text::row(text, start)..text::row(text, end) + 1),
            MotionKind::Inclusive => {
                let end = if end < text::line_end(text, text::row(text, end)) {
                    text::next_offset(text, end)
                } else {
                    end
                };
                Self::Chars(start..end)
            }
            MotionKind::Exclusive => {
                let start_row = text::row(text, start);
                let end_row = text::row(text, end);
                if end_row > start_row && end == text::line_start(text, end_row) {
                    if start <= text::first_non_blank(text, start_row) {
                        return Self::Lines(start_row..end_row);
                    }
                    return Self::Chars(start..text::line_end(text, end_row - 1));
                }
                Self::Chars(start..end)
            }
        }
    }

    /// The rows the region touches.
    pub(super) fn rows(&self, text: &Rope) -> Range<usize> {
        match self {
            Self::Chars(range) => {
                let start = text::row(text, range.start);
                let end = if range.end > range.start {
                    text::row(text, text::prev_offset(text, range.end))
                } else {
                    start
                };
                start..end.max(start) + 1
            }
            Self::Lines(rows) | Self::Block { rows, .. } => rows.clone(),
        }
    }

    /// Each row's part of a block.
    pub(super) fn segments(
        text: &Rope,
        rows: &Range<usize>,
        left: usize,
        right: Option<usize>,
    ) -> Vec<Range<usize>> {
        rows.clone()
            .map(|row| {
                let start = text::offset_at_column(text, row, left);
                let end = match right {
                    Some(right) => text::offset_at_column(text, row, right),
                    None => text::line_end(text, row),
                };
                start..end.max(start)
            })
            .collect()
    }

    /// The text the region holds, as a register would keep it.
    pub(super) fn register(&self, text: &Rope) -> Register {
        match self {
            Self::Chars(range) => {
                Register::new(text.slice(range.clone()).to_string(), RegisterKind::Chars)
            }
            Self::Lines(rows) => Register::new(text::rows_text(text, rows), RegisterKind::Lines),
            Self::Block { rows, left, right } => {
                let lines: Vec<String> = Self::segments(text, rows, *left, *right)
                    .into_iter()
                    .map(|segment| text.slice(segment).to_string())
                    .collect();
                Register::new(lines.join("\n"), RegisterKind::Block)
            }
        }
    }

    /// Whether the region holds more than part of one line.
    pub(super) fn spans_lines(&self, text: &Rope) -> bool {
        match self {
            Self::Chars(range) => text::row(text, range.start) != text::row(text, range.end),
            _ => true,
        }
    }
}

/// The range deleting whole `rows` removes: their text and a line break,
/// the one before them on the last line.
pub(super) fn delete_rows_range(text: &Rope, rows: &Range<usize>) -> Range<usize> {
    let last_row = text::last_row(text);
    if rows.end <= last_row {
        text::line_start(text, rows.start)..text::line_start(text, rows.end)
    } else if rows.start > 0 {
        text::line_end(text, rows.start - 1)..text.len()
    } else {
        0..text.len()
    }
}

impl VimState {
    /// Apply `operator` to `region`. `cursor` is where a yank leaves the
    /// caret, `big` puts a delete in register 1, and `times` is how far `>`
    /// and `<` shift.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_operator(
        &mut self,
        operator: Operator,
        region: Region,
        cursor: usize,
        big: bool,
        times: usize,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let register_name = self.register.take();
        let text = state.text.clone();
        match operator {
            Operator::Yank => {
                if !matches!(&region, Region::Chars(range) if range.is_empty()) {
                    register::store(register_name, Store::Yank, region.register(&text), cx);
                }
                let caret = match &region {
                    Region::Block { rows, left, .. } => {
                        text::offset_at_column(&text, rows.start, *left)
                    }
                    _ => cursor,
                };
                self.enter_normal_at(caret, state, cx);
            }
            Operator::Delete => {
                let big = big || region.spans_lines(&text);
                let contents = region.register(&text);
                match region {
                    Region::Chars(range) => {
                        if range.is_empty() {
                            self.enter_normal_at(range.start, state, cx);
                            return;
                        }
                        register::store(register_name, Store::Delete { big }, contents, cx);
                        edit(state, vec![(range.clone(), String::new())], window, cx);
                        self.enter_normal_at(range.start, state, cx);
                    }
                    Region::Lines(rows) => {
                        register::store(register_name, Store::Delete { big: true }, contents, cx);
                        edit(
                            state,
                            vec![(delete_rows_range(&text, &rows), String::new())],
                            window,
                            cx,
                        );
                        let row = rows.start.min(text::last_row(&state.text));
                        let caret = text::first_non_blank(&state.text, row);
                        self.enter_normal_at(caret, state, cx);
                    }
                    Region::Block { rows, left, right } => {
                        register::store(register_name, Store::Delete { big: true }, contents, cx);
                        let edits = Region::segments(&text, &rows, left, right)
                            .into_iter()
                            .filter(|segment| !segment.is_empty())
                            .map(|segment| (segment, String::new()))
                            .collect();
                        edit(state, edits, window, cx);
                        let caret = text::offset_at_column(&state.text, rows.start, left);
                        self.enter_normal_at(caret, state, cx);
                    }
                }
            }
            Operator::Change => {
                let big = big || region.spans_lines(&text);
                let contents = region.register(&text);
                match region {
                    Region::Chars(range) => {
                        if !range.is_empty() {
                            register::store(register_name, Store::Delete { big }, contents, cx);
                        }
                        let start = range.start;
                        edit(state, vec![(range, String::new())], window, cx);
                        self.start_insert(start, 1, InsertRepeat::Inline, state, window, cx);
                    }
                    Region::Lines(rows) => {
                        register::store(register_name, Store::Delete { big: true }, contents, cx);
                        let indent = text::indentation(&text, rows.start);
                        let range = text::line_start(&text, rows.start)
                            ..text::line_end(&text, rows.end - 1);
                        let caret = range.start + indent.len();
                        edit(state, vec![(range, indent)], window, cx);
                        self.start_insert(caret, 1, InsertRepeat::Inline, state, window, cx);
                    }
                    Region::Block { rows, left, right } => {
                        register::store(register_name, Store::Delete { big: true }, contents, cx);
                        let segments = Region::segments(&text, &rows, left, right);
                        let edits = segments
                            .iter()
                            .filter(|segment| !segment.is_empty())
                            .map(|segment| (segment.clone(), String::new()))
                            .collect();
                        edit(state, edits, window, cx);
                        // Type into every row that reaches the block.
                        let carets: Vec<usize> = rows
                            .clone()
                            .filter(|row| text::line_chars(&state.text, *row) >= left)
                            .map(|row| text::offset_at_column(&state.text, row, left))
                            .collect();
                        self.start_block_insert(carets, state, window, cx);
                    }
                }
            }
            Operator::Indent | Operator::Outdent => {
                let rows = region.rows(&text);
                let edits = shift_edits(state, &rows, operator == Operator::Indent, times.max(1));
                edit(state, edits, window, cx);
                let caret = text::first_non_blank(&state.text, rows.start);
                self.enter_normal_at(caret, state, cx);
            }
            Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase => {
                let convert = |s: &str| -> String {
                    match operator {
                        Operator::Lowercase => s.to_lowercase(),
                        Operator::Uppercase => s.to_uppercase(),
                        _ => toggle_case(s),
                    }
                };
                let (ranges, caret) = match &region {
                    Region::Chars(range) => (vec![range.clone()], range.start),
                    Region::Lines(rows) => (
                        rows.clone()
                            .map(|row| text::line_start(&text, row)..text::line_end(&text, row))
                            .collect(),
                        cursor
                            .min(text::line_start(&text, rows.start))
                            .max(text::line_start(&text, rows.start)),
                    ),
                    Region::Block { rows, left, right } => {
                        let segments = Region::segments(&text, rows, *left, *right);
                        let caret = segments.first().map_or(cursor, |segment| segment.start);
                        (segments, caret)
                    }
                };
                let edits = ranges
                    .into_iter()
                    .filter(|range| !range.is_empty())
                    .filter_map(|range| {
                        let old = text.slice(range.clone()).to_string();
                        let new = convert(&old);
                        (new != old).then_some((range, new))
                    })
                    .collect();
                edit(state, edits, window, cx);
                let caret = if matches!(region, Region::Lines(_)) {
                    cursor
                } else {
                    caret
                };
                self.enter_normal_at(caret, state, cx);
            }
        }
    }
}

/// Each character's case flipped, as `~` does.
pub(super) fn toggle_case(s: &str) -> String {
    s.chars()
        .flat_map(|c| -> Box<dyn Iterator<Item = char>> {
            if c.is_lowercase() {
                Box::new(c.to_uppercase())
            } else if c.is_uppercase() {
                Box::new(c.to_lowercase())
            } else {
                Box::new(std::iter::once(c))
            }
        })
        .collect()
}

/// The edits that shift `rows` one indentation step right or left, `times`
/// over. Empty lines stay empty.
pub(super) fn shift_edits(
    state: &TextareaState,
    rows: &Range<usize>,
    right: bool,
    times: usize,
) -> Vec<(Range<usize>, String)> {
    let text = &state.text;
    let tab = state.mode.tab_size();
    let width = tab.tab_size.max(1);
    let unit = if tab.hard_tabs {
        "\t".to_string()
    } else {
        " ".repeat(width)
    };
    rows.clone()
        .filter_map(|row| {
            let start = text::line_start(text, row);
            if right {
                if text::is_empty_line(text, row) {
                    return None;
                }
                return Some((start..start, unit.repeat(times)));
            }
            // Remove up to `times` steps of leading blanks: a tab, or as
            // many spaces as a step is wide.
            let indent = text::indentation(text, row);
            let mut removed = 0;
            let mut steps = 0;
            let bytes = indent.as_bytes();
            while steps < times && removed < bytes.len() {
                if bytes[removed] == b'\t' {
                    removed += 1;
                } else {
                    let spaces = bytes[removed..]
                        .iter()
                        .take(width)
                        .take_while(|b| **b == b' ')
                        .count();
                    removed += spaces;
                    if removed < bytes.len() && bytes[removed] == b'\t' && spaces < width {
                        removed += 1;
                    }
                }
                steps += 1;
            }
            (removed > 0).then(|| (start..start + removed, String::new()))
        })
        .collect()
}

/// Apply `edits` to the text as one undo step.
pub(super) fn edit(
    state: &mut TextareaState,
    edits: Vec<(Range<usize>, String)>,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if edits.is_empty() {
        return;
    }
    state.undo_manager.break_transaction_coalescing();
    state
        .undo_manager
        .set_pending_intent(crate::input::undo_manager::EditIntent::Atomic);
    state.replace_text_in_ranges(&edits, window, cx);
    state.undo_manager.break_transaction_coalescing();
}
