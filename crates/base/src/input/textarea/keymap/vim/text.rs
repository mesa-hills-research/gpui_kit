//! Reading the text the way Vim does: lines without their line breaks,
//! character classes for words, and a cursor that steps over characters and
//! line ends like Vim's.
//!
//! Offsets are byte offsets into the rope. A line's end is the offset of its
//! line break, `\n` or `\r\n`, which is where Vim's cursor sits "on the end of
//! the line".

use std::ops::Range;

use ropey::Rope;

use crate::input::RopeExt as _;

/// The row `offset` is on.
pub(super) fn row(text: &Rope, offset: usize) -> usize {
    text.offset_to_point(offset.min(text.len())).row
}

/// The last row.
pub(super) fn last_row(text: &Rope) -> usize {
    text.lines_len().saturating_sub(1)
}

pub(super) fn line_start(text: &Rope, row: usize) -> usize {
    text.line_start_offset(row)
}

/// The end of a row's text, before its line break.
pub(super) fn line_end(text: &Rope, row: usize) -> usize {
    let start = text.line_start_offset(row);
    let end = text.line_end_offset(row);
    if end > start && text.char_at(end - 1) == Some('\r') && text.char_at(end) == Some('\n') {
        end - 1
    } else {
        end
    }
}

/// The start of the next row, or the end of the text on the last row.
pub(super) fn next_line_start(text: &Rope, row: usize) -> usize {
    if row >= last_row(text) {
        text.len()
    } else {
        text.line_start_offset(row + 1)
    }
}

pub(super) fn is_empty_line(text: &Rope, row: usize) -> bool {
    line_start(text, row) == line_end(text, row)
}

/// A row of only spaces and tabs, or none.
pub(super) fn is_blank_line(text: &Rope, row: usize) -> bool {
    line_slice(text, row).chars().all(|c| c == ' ' || c == '\t')
}

pub(super) fn line_text(text: &Rope, row: usize) -> String {
    line_slice(text, row).to_string()
}

fn line_slice(text: &Rope, row: usize) -> ropey::RopeSlice<'_> {
    text.slice(line_start(text, row)..line_end(text, row))
}

/// The character at `offset`, if any.
pub(super) fn char_at(text: &Rope, offset: usize) -> Option<char> {
    if offset >= text.len() {
        return None;
    }
    text.chars_at(offset).next()
}

/// The character before `offset`, if any.
pub(super) fn char_before(text: &Rope, offset: usize) -> Option<char> {
    if offset == 0 {
        return None;
    }
    text.chars_at(offset).reversed().next()
}

/// The offset after the character at `offset`.
pub(super) fn next_offset(text: &Rope, offset: usize) -> usize {
    char_at(text, offset).map_or(text.len(), |c| offset + c.len_utf8())
}

/// The offset of the character before `offset`.
pub(super) fn prev_offset(text: &Rope, offset: usize) -> usize {
    char_before(text, offset).map_or(0, |c| offset - c.len_utf8())
}

/// The first character that is not a space or tab, or the line's end.
pub(super) fn first_non_blank(text: &Rope, row: usize) -> usize {
    let start = line_start(text, row);
    let end = line_end(text, row);
    let mut offset = start;
    for c in text.chars_at(start) {
        if offset >= end || (c != ' ' && c != '\t') {
            break;
        }
        offset += c.len_utf8();
    }
    offset.min(end)
}

/// The leading spaces and tabs of a row.
pub(super) fn indentation(text: &Rope, row: usize) -> String {
    let start = line_start(text, row);
    text.slice(start..first_non_blank(text, row)).to_string()
}

/// The start of the row's last character, or the row's start when it is
/// empty: where normal mode's caret stops.
pub(super) fn last_char(text: &Rope, row: usize) -> usize {
    let start = line_start(text, row);
    let end = line_end(text, row);
    if end > start {
        prev_offset(text, end)
    } else {
        start
    }
}

/// Where normal mode puts a caret at `offset`: on a character, never on a
/// line's end unless the line is empty.
pub(super) fn normal_offset(text: &Rope, offset: usize) -> usize {
    let offset = clip(text, offset);
    let row = row(text, offset);
    if offset >= line_end(text, row) {
        last_char(text, row)
    } else {
        offset
    }
}

/// `offset` on a character boundary within the text.
pub(super) fn clip(text: &Rope, offset: usize) -> usize {
    text.clip_offset(offset.min(text.len()), sum_tree::Bias::Left)
}

/// How many characters into its row `offset` is.
pub(super) fn column(text: &Rope, offset: usize) -> usize {
    let row = row(text, offset);
    text.slice(line_start(text, row)..offset).chars().count()
}

/// The offset `column` characters into `row`, stopping at the line's end.
pub(super) fn offset_at_column(text: &Rope, row: usize, column: usize) -> usize {
    let start = line_start(text, row);
    let end = line_end(text, row);
    let mut offset = start;
    for (ix, c) in text.chars_at(start).enumerate() {
        if ix >= column || offset >= end {
            break;
        }
        offset += c.len_utf8();
    }
    offset.min(end)
}

/// How many characters a row holds.
pub(super) fn line_chars(text: &Rope, row: usize) -> usize {
    line_slice(text, row).chars().count()
}

/// A range covering whole rows, with the line break after the last one.
pub(super) fn rows_range(text: &Rope, rows: &Range<usize>) -> Range<usize> {
    line_start(text, rows.start)..next_line_start(text, rows.end - 1)
}

/// The text of whole rows, ending in a line break, as a linewise register
/// holds it.
pub(super) fn rows_text(text: &Rope, rows: &Range<usize>) -> String {
    let mut lines = String::new();
    for row in rows.clone() {
        lines.push_str(&line_text(text, row));
        lines.push('\n');
    }
    lines
}

/// Vim's character classes for moving by word: 0 for blanks, 1 for
/// punctuation, 2 for word characters and further classes for scripts that
/// form words of their own. Every non-blank is one class for WORDs.
pub(super) fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        return 0;
    }
    if big {
        return 1;
    }
    match c as u32 {
        0x3040..=0x309F => 4,
        0x30A0..=0x30FF => 5,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3FFFF => 6,
        0xAC00..=0xD7A3 => 7,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF => 3,
        _ if c.is_alphanumeric() || c == '_' => 2,
        _ => 1,
    }
}

/// Whether `c` is part of a keyword, for `*` and `#`.
pub(super) fn is_word_char(c: char) -> bool {
    class(c, false) >= 2
}

/// A cursor that moves like Vim's: over each character of a line, onto the
/// line's end, then to the start of the next line.
#[derive(Clone)]
pub(super) struct Cursor<'a> {
    text: &'a Rope,
    pub(super) offset: usize,
    row: usize,
    start: usize,
    end: usize,
    last_row: usize,
}

/// What [`Cursor::inc`] and [`Cursor::dec`] crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Moved within the line.
    Char,
    /// Moved onto the line's end.
    LineEnd,
    /// Moved to another line.
    Line,
    /// Could not move: the start or end of the text.
    Edge,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(text: &'a Rope, offset: usize) -> Self {
        let offset = clip(text, offset);
        let row = row(text, offset);
        let start = line_start(text, row);
        let end = line_end(text, row);
        Self {
            text,
            offset: offset.min(end),
            row,
            start,
            end,
            last_row: last_row(text),
        }
    }

    fn set_row(&mut self, row: usize) {
        self.row = row;
        self.start = line_start(self.text, row);
        self.end = line_end(self.text, row);
    }

    /// The character under the cursor, or none on a line's end.
    pub(super) fn char(&self) -> Option<char> {
        if self.offset >= self.end {
            None
        } else {
            char_at(self.text, self.offset)
        }
    }

    pub(super) fn class(&self, big: bool) -> u8 {
        self.char().map_or(0, |c| class(c, big))
    }

    pub(super) fn line_is_empty(&self) -> bool {
        self.start == self.end
    }

    pub(super) fn at_line_start(&self) -> bool {
        self.offset == self.start
    }

    pub(super) fn is_last_row(&self) -> bool {
        self.row == self.last_row
    }

    /// Step forward, as Vim's `inc()`.
    pub(super) fn inc(&mut self) -> Step {
        if self.offset < self.end {
            self.offset = next_offset(self.text, self.offset).min(self.end);
            return if self.offset < self.end {
                Step::Char
            } else {
                Step::LineEnd
            };
        }
        if self.row < self.last_row {
            self.set_row(self.row + 1);
            self.offset = self.start;
            return Step::Line;
        }
        Step::Edge
    }

    /// Step back, as Vim's `dec()`: from a line's start onto the end of the
    /// line before.
    pub(super) fn dec(&mut self) -> Step {
        if self.offset > self.start {
            self.offset = prev_offset(self.text, self.offset).max(self.start);
            return Step::Char;
        }
        if self.row > 0 {
            self.set_row(self.row - 1);
            self.offset = self.end;
            return Step::Line;
        }
        Step::Edge
    }

    /// Step back onto the line's last character, as Vim does after moving
    /// past the end of a line.
    pub(super) fn back_onto_line(&mut self) -> bool {
        if self.offset >= self.end && self.end > self.start {
            self.offset = prev_offset(self.text, self.end);
            true
        } else {
            false
        }
    }
}
