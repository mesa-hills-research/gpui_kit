//! Motions: where each one goes from the caret, and whether an operator
//! takes the text up to it, through it or by whole lines.
//!
//! The word motions follow Vim's own `fwd_word`, `bck_word`, `end_word` and
//! `bckend_word`, so the corner cases match: `w` stopping at an empty line,
//! `dw` on a line's last word keeping the line break, `e` from the end of a
//! word moving to the end of the next one.

use std::ops::Range;

use ropey::Rope;

use super::{
    VimState, search,
    text::{self, Cursor, Step},
};
use crate::input::{TextareaState, smooth_caret::CaretMotion};

/// Where the caret goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    /// `h`
    Left,
    /// `l`
    Right,
    /// `k`
    Up,
    /// `j`
    Down,
    /// Backspace: `h` that wraps to the line before.
    WrappingLeft,
    /// Space: `l` that wraps to the next line.
    WrappingRight,
    /// `gk`: up a row as displayed, when lines wrap.
    DisplayUp,
    /// `gj`
    DisplayDown,
    /// `w`, `W`
    NextWordStart { big: bool },
    /// `b`, `B`
    PreviousWordStart { big: bool },
    /// `e`, `E`
    NextWordEnd { big: bool },
    /// `ge`, `gE`
    PreviousWordEnd { big: bool },
    /// `0`
    LineStart,
    /// `^`
    FirstNonBlank,
    /// `$`
    LineEnd,
    /// `g_`
    LastNonBlank,
    /// `+` and Enter
    NextLineStart,
    /// `-`
    PreviousLineStart,
    /// `_`
    CurrentLineStart,
    /// `gg`
    FirstLine,
    /// `G`
    LastLine,
    /// `f`, `F`, `t`, `T` with their character.
    Find {
        character: char,
        backward: bool,
        till: bool,
    },
    /// `;` and `,`
    RepeatFind { reverse: bool },
    /// `%`: the matching bracket, or with a count, that percentage of the
    /// text.
    MatchBracket,
    /// `{`
    ParagraphBackward,
    /// `}`
    ParagraphForward,
    /// `H`
    WindowTop,
    /// `M`
    WindowMiddle,
    /// `L`
    WindowBottom,
    /// Ctrl-D
    HalfPageDown,
    /// Ctrl-U
    HalfPageUp,
    /// Ctrl-F
    PageDown,
    /// Ctrl-B
    PageUp,
    /// `n` and `N`, and the search typed after `/` or `?`.
    SearchNext { reverse: bool },
    /// `*` and `#`
    SearchWord { backward: bool },
    /// The caret's line, for an operator typed twice, such as `dd`.
    CurrentLine,
}

/// How an operator takes the text a motion moves over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MotionKind {
    /// Up to the target, not including it.
    Exclusive,
    /// Through the character at the target.
    Inclusive,
    /// Every line from the caret's to the target's.
    Linewise,
}

/// What the motion does to the column `j` and `k` aim for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Want {
    /// Keep aiming for the same column.
    Keep,
    /// Aim for the target's column.
    Reset,
    /// Aim for the ends of lines, after `$`.
    End,
}

/// Where a motion lands.
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub(super) offset: usize,
    pub(super) kind: MotionKind,
    pub(super) want: Want,
}

impl Target {
    fn new(offset: usize, kind: MotionKind) -> Self {
        Self {
            offset,
            kind,
            want: Want::Reset,
        }
    }

    fn keep_want(mut self) -> Self {
        self.want = Want::Keep;
        self
    }
}

/// What a motion reads besides the text.
pub(super) struct Env<'a> {
    pub(super) state: &'a TextareaState,
    pub(super) vim: &'a VimState,
    /// An operator takes the motion.
    pub(super) operator: bool,
    /// Visual mode extends the selection with it: the caret may rest on a
    /// line's end.
    pub(super) visual: bool,
}

impl Env<'_> {
    fn text(&self) -> &Rope {
        &self.state.text
    }

    /// Normal mode: the caret stays on a character.
    fn normal(&self) -> bool {
        !self.operator && !self.visual
    }

    /// The offset `j` and `k` land on in `row`.
    fn offset_for_want(&self, row: usize, want: usize) -> usize {
        let text = self.text();
        if want == usize::MAX {
            if self.visual {
                text::line_end(text, row)
            } else {
                text::last_char(text, row)
            }
        } else {
            let offset = text::offset_at_column(text, row, want);
            if self.normal() {
                text::normal_offset(text, offset)
            } else {
                offset
            }
        }
    }
}

impl Motion {
    /// How the smooth caret glides for this motion in normal mode: `h` and `l`
    /// move by a character, and the word motions by a word. Every other
    /// motion moves the caret at once.
    pub(super) fn caret_motion(self) -> Option<CaretMotion> {
        match self {
            Motion::Left | Motion::Right | Motion::WrappingLeft | Motion::WrappingRight => {
                Some(CaretMotion::Grapheme)
            }
            Motion::NextWordStart { .. }
            | Motion::PreviousWordStart { .. }
            | Motion::NextWordEnd { .. }
            | Motion::PreviousWordEnd { .. } => Some(CaretMotion::Word),
            _ => None,
        }
    }

    /// Motions that put deleted text in register 1 even within one line,
    /// and that `'` and `` ` `` would record as jumps.
    pub(super) fn is_jump(self) -> bool {
        matches!(
            self,
            Self::MatchBracket
                | Self::ParagraphBackward
                | Self::ParagraphForward
                | Self::SearchNext { .. }
                | Self::SearchWord { .. }
        )
    }

    /// Where the motion lands from `from`, or `None` when it cannot move.
    pub(super) fn evaluate(self, env: &Env, from: usize, count: Option<usize>) -> Option<Target> {
        let text = env.text();
        let n = count.unwrap_or(1).max(1);
        let row = text::row(text, from);
        let last_row = text::last_row(text);
        use MotionKind::*;
        match self {
            Self::Left => {
                let start = text::line_start(text, row);
                let mut offset = from;
                for _ in 0..n {
                    if offset <= start {
                        break;
                    }
                    offset = text::prev_offset(text, offset);
                }
                (offset != from).then(|| Target::new(offset, Exclusive))
            }
            Self::Right => {
                let end = text::line_end(text, row);
                let mut offset = from;
                let mut moved = 0;
                for _ in 0..n {
                    let next = text::next_offset(text, offset);
                    let can_move = if env.visual { offset < end } else { next < end };
                    if !can_move {
                        break;
                    }
                    offset = next;
                    moved += 1;
                }
                if moved == n {
                    Some(Target::new(offset, Exclusive))
                } else if env.operator {
                    // An operator still takes the character under the caret,
                    // as `x` on a line's last character does. On an empty
                    // line it takes nothing, and `cl` still inserts.
                    let kind = if end > text::line_start(text, row) {
                        Inclusive
                    } else {
                        Exclusive
                    };
                    Some(Target::new(offset, kind))
                } else if moved > 0 {
                    Some(Target::new(offset, Exclusive))
                } else {
                    None
                }
            }
            Self::Up | Self::Down => {
                let up = self == Self::Up;
                if (up && row == 0) || (!up && row >= last_row) {
                    return None;
                }
                let target_row = if up {
                    row.saturating_sub(n)
                } else {
                    (row + n).min(last_row)
                };
                let want = env.vim.want_column(text, from);
                Some(Target::new(env.offset_for_want(target_row, want), Linewise).keep_want())
            }
            Self::WrappingLeft => {
                let mut offset = from;
                for _ in 0..n {
                    let r = text::row(text, offset);
                    if offset > text::line_start(text, r) {
                        offset = text::prev_offset(text, offset);
                    } else if r > 0 {
                        offset = if env.operator {
                            text::line_end(text, r - 1)
                        } else {
                            text::last_char(text, r - 1)
                        };
                    } else {
                        break;
                    }
                }
                (offset != from).then(|| Target::new(offset, Exclusive))
            }
            Self::WrappingRight => {
                let mut offset = from;
                for _ in 0..n {
                    let r = text::row(text, offset);
                    let end = text::line_end(text, r);
                    let next = text::next_offset(text, offset);
                    if next < end || (!env.normal() && offset < end) {
                        offset = next.min(end);
                    } else if r < last_row {
                        offset = text::line_start(text, r + 1);
                    } else {
                        break;
                    }
                }
                (offset != from).then(|| Target::new(offset, Exclusive))
            }
            Self::DisplayUp | Self::DisplayDown => {
                let lines = if self == Self::DisplayUp {
                    -(n as isize)
                } else {
                    n as isize
                };
                let (offset, _) = env.state.vertical_target(from, None, false, lines);
                let offset = if env.normal() {
                    text::normal_offset(text, offset)
                } else {
                    offset
                };
                (offset != from).then(|| Target::new(offset, Exclusive))
            }
            Self::NextWordStart { big } => {
                let mut cursor = Cursor::new(text, from);
                next_word_start(&mut cursor, n, big, env.operator);
                Some(adjust_forward(env, from, cursor, Exclusive))
            }
            Self::PreviousWordStart { big } => {
                let mut cursor = Cursor::new(text, from);
                previous_word_start(&mut cursor, n, big, false);
                (cursor.offset != from).then(|| Target::new(cursor.offset, Exclusive))
            }
            Self::NextWordEnd { big } => {
                let mut cursor = Cursor::new(text, from);
                next_word_end(&mut cursor, n, big, false);
                Some(adjust_forward(env, from, cursor, Inclusive))
            }
            Self::PreviousWordEnd { big } => {
                let mut cursor = Cursor::new(text, from);
                previous_word_end(&mut cursor, n, big);
                (cursor.offset != from).then(|| Target::new(cursor.offset, Inclusive))
            }
            Self::LineStart => Some(Target::new(text::line_start(text, row), Exclusive)),
            Self::FirstNonBlank => {
                let offset = text::first_non_blank(text, row);
                let offset = if env.normal() {
                    text::normal_offset(text, offset)
                } else {
                    offset
                };
                Some(Target::new(offset, Exclusive))
            }
            Self::LineEnd => {
                let target_row = (row + n - 1).min(last_row);
                if n > 1 && row >= last_row {
                    return None;
                }
                let offset = if env.visual {
                    text::line_end(text, target_row)
                } else {
                    text::last_char(text, target_row)
                };
                Some(Target {
                    offset,
                    kind: Inclusive,
                    want: Want::End,
                })
            }
            Self::LastNonBlank => {
                let target_row = (row + n - 1).min(last_row);
                let start = text::line_start(text, target_row);
                let mut offset = text::line_end(text, target_row);
                while offset > start {
                    let before = text::prev_offset(text, offset);
                    if !matches!(text::char_at(text, before), Some(' ' | '\t')) {
                        break;
                    }
                    offset = before;
                }
                let offset = if offset > start {
                    text::prev_offset(text, offset)
                } else {
                    start
                };
                Some(Target::new(offset, Inclusive))
            }
            Self::NextLineStart => {
                (row < last_row).then(|| line_target(text, (row + n).min(last_row)))
            }
            Self::PreviousLineStart => (row > 0).then(|| line_target(text, row.saturating_sub(n))),
            Self::CurrentLineStart => {
                if n > 1 && row >= last_row {
                    return None;
                }
                Some(line_target(text, (row + n - 1).min(last_row)))
            }
            Self::FirstLine => {
                let target = count.map_or(0, |count| count.saturating_sub(1));
                Some(line_target(text, target.min(last_row)))
            }
            Self::LastLine => {
                let target = count.map_or(last_row, |count| count.saturating_sub(1));
                Some(line_target(text, target.min(last_row)))
            }
            Self::Find {
                character,
                backward,
                till,
            } => find(text, from, character, backward, till, n, false),
            Self::RepeatFind { reverse } => {
                let last = env.vim.last_find?;
                let Self::Find {
                    character,
                    backward,
                    till,
                } = last
                else {
                    return None;
                };
                find(text, from, character, backward != reverse, till, n, till)
            }
            Self::MatchBracket => match count {
                Some(percent) => {
                    if percent > 100 {
                        return None;
                    }
                    let lines = last_row + 1;
                    let target = (percent * lines).div_ceil(100).saturating_sub(1);
                    Some(line_target(text, target.min(last_row)))
                }
                None => match_bracket(text, from).map(|offset| Target::new(offset, Inclusive)),
            },
            Self::ParagraphBackward | Self::ParagraphForward => {
                paragraph(text, from, n, self == Self::ParagraphForward)
            }
            Self::WindowTop | Self::WindowMiddle | Self::WindowBottom => {
                let rows = visible_rows(env.state).unwrap_or(0..last_row + 1);
                let (top, bottom) = (rows.start, rows.end.saturating_sub(1).max(rows.start));
                let target = match self {
                    Self::WindowTop => (top + n - 1).min(bottom),
                    Self::WindowBottom => bottom.saturating_sub(n - 1).max(top),
                    _ => top + (bottom - top) / 2,
                };
                Some(line_target(text, target.min(last_row)))
            }
            Self::HalfPageDown | Self::HalfPageUp | Self::PageDown | Self::PageUp => {
                let visible = visible_rows(env.state).map_or(20, |rows| rows.len().max(1));
                let lines = match self {
                    Self::HalfPageDown | Self::HalfPageUp => count.unwrap_or(visible / 2).max(1),
                    _ => n * visible.saturating_sub(2).max(1),
                };
                let down = matches!(self, Self::HalfPageDown | Self::PageDown);
                if (down && row >= last_row) || (!down && row == 0) {
                    return None;
                }
                let target_row = if down {
                    (row + lines).min(last_row)
                } else {
                    row.saturating_sub(lines)
                };
                let want = env.vim.want_column(text, from);
                Some(Target::new(env.offset_for_want(target_row, want), Linewise).keep_want())
            }
            Self::SearchNext { reverse } => {
                let search = env.vim.last_search.as_ref()?;
                let backward = search.backward != reverse;
                search::find(text, &search.pattern, from, backward, n)
                    .map(|offset| Target::new(offset, Exclusive))
            }
            Self::SearchWord { .. } => {
                // `*` and `#` set the search first and then move as `n`.
                Self::SearchNext { reverse: false }.evaluate(env, from, count)
            }
            Self::CurrentLine => {
                if n > 1 && row >= last_row {
                    return None;
                }
                Some(Target::new(
                    text::first_non_blank(text, (row + n - 1).min(last_row)),
                    Linewise,
                ))
            }
        }
    }
}

/// The first non-blank of `row`, taken linewise.
fn line_target(text: &Rope, row: usize) -> Target {
    Target::new(text::first_non_blank(text, row), MotionKind::Linewise)
}

/// After moving forward by words, the caret may rest on a line's end: back
/// onto the line's last character, as Vim does, which makes an operator take
/// that character.
fn adjust_forward(env: &Env, from: usize, mut cursor: Cursor, kind: MotionKind) -> Target {
    let mut kind = kind;
    if cursor.offset > from && !env.visual && cursor.back_onto_line() {
        kind = MotionKind::Inclusive;
    }
    Target::new(cursor.offset, kind)
}

/// Vim's `fwd_word`. With `eol`, as for an operator, the last word stops at
/// the end of its line instead of moving on to the next.
pub(super) fn next_word_start(cursor: &mut Cursor, count: usize, big: bool, eol: bool) {
    for remaining in (0..count).rev() {
        let last = remaining == 0;
        let start_class = cursor.class(big);
        let last_line = cursor.is_last_row();
        let step = cursor.inc();
        if step == Step::Edge || (step != Step::Char && last_line) {
            return;
        }
        if step != Step::Char && eol && last {
            return;
        }
        if start_class != 0 {
            while cursor.class(big) == start_class {
                let step = cursor.inc();
                if step == Step::Edge || (step != Step::Char && eol && last) {
                    return;
                }
            }
        }
        while cursor.class(big) == 0 {
            if cursor.at_line_start() && cursor.line_is_empty() {
                break;
            }
            let step = cursor.inc();
            if step == Step::Edge || (step != Step::Char && eol && last) {
                return;
            }
        }
    }
}

/// Vim's `bck_word`. With `stop`, a caret already at a word's start moves
/// one word less.
pub(super) fn previous_word_start(cursor: &mut Cursor, count: usize, big: bool, stop: bool) {
    let mut stop = stop;
    'words: for _ in 0..count {
        let start_class = cursor.class(big);
        if cursor.dec() == Step::Edge {
            return;
        }
        if !stop || start_class == cursor.class(big) || start_class == 0 {
            // Skip the blanks before the word, stopping at an empty line.
            while cursor.class(big) == 0 {
                if cursor.at_line_start() && cursor.line_is_empty() {
                    stop = false;
                    continue 'words;
                }
                if cursor.dec() == Step::Edge {
                    return;
                }
            }
            // Back to the word's start.
            let class = cursor.class(big);
            while cursor.class(big) == class {
                if cursor.dec() == Step::Edge {
                    return;
                }
            }
        }
        // One too far.
        cursor.inc();
        stop = false;
    }
}

/// Vim's `end_word`. With `stop`, a caret already at a word's end moves one
/// word less, as `cw` does. With `empty`, an empty line stops the motion.
pub(super) fn next_word_end(cursor: &mut Cursor, count: usize, big: bool, stop: bool) {
    let mut stop = stop;
    'words: for _ in 0..count {
        let start_class = cursor.class(big);
        if cursor.inc() == Step::Edge {
            return;
        }
        if cursor.class(big) == start_class && start_class != 0 {
            // In a word: to its end.
            while cursor.class(big) == start_class {
                if cursor.inc() == Step::Edge {
                    return;
                }
            }
        } else if !stop || start_class == 0 {
            // At a word's end: past the blanks to the end of the next word.
            while cursor.class(big) == 0 {
                if cursor.inc() == Step::Edge {
                    return;
                }
            }
            let class = cursor.class(big);
            while cursor.class(big) == class {
                if cursor.inc() == Step::Edge {
                    return;
                }
            }
        } else {
            // Already at the end and asked to stop there.
            cursor.dec();
            stop = false;
            continue 'words;
        }
        // One too far.
        cursor.dec();
        stop = false;
    }
}

/// Vim's `bckend_word`.
pub(super) fn previous_word_end(cursor: &mut Cursor, count: usize, big: bool) {
    for _ in 0..count {
        let start_class = cursor.class(big);
        if cursor.dec() == Step::Edge {
            return;
        }
        if start_class != 0 {
            while cursor.class(big) == start_class {
                if cursor.dec() == Step::Edge {
                    return;
                }
            }
        }
        while cursor.class(big) == 0 {
            if cursor.at_line_start() && cursor.line_is_empty() {
                break;
            }
            if cursor.dec() == Step::Edge {
                return;
            }
        }
    }
}

/// `f`, `F`, `t` and `T` within the caret's line. `skip_adjacent` makes a
/// repeated `t` move past a match right next to the caret.
fn find(
    text: &Rope,
    from: usize,
    character: char,
    backward: bool,
    till: bool,
    count: usize,
    skip_adjacent: bool,
) -> Option<Target> {
    let row = text::row(text, from);
    let start = text::line_start(text, row);
    let end = text::line_end(text, row);
    let mut offset = from;
    if !backward {
        if till
            && skip_adjacent
            && text::char_at(text, text::next_offset(text, offset)) == Some(character)
        {
            offset = text::next_offset(text, offset);
        }
        let mut found = 0;
        let mut position = offset;
        loop {
            position = text::next_offset(text, position);
            if position >= end {
                return None;
            }
            if text::char_at(text, position) == Some(character) {
                found += 1;
                if found == count {
                    break;
                }
            }
        }
        let target = if till {
            text::prev_offset(text, position)
        } else {
            position
        };
        Some(Target::new(target, MotionKind::Inclusive))
    } else {
        if till
            && skip_adjacent
            && offset > start
            && text::char_before(text, offset) == Some(character)
        {
            offset = text::prev_offset(text, offset);
        }
        let mut found = 0;
        let mut position = offset;
        loop {
            if position <= start {
                return None;
            }
            position = text::prev_offset(text, position);
            if text::char_at(text, position) == Some(character) {
                found += 1;
                if found == count {
                    break;
                }
            }
        }
        let target = if till {
            text::next_offset(text, position)
        } else {
            position
        };
        (target != from || !till).then(|| Target::new(target, MotionKind::Exclusive))
    }
}

/// `%` without a count: the bracket at or after the caret on its line, and
/// the one that matches it.
pub(super) fn match_bracket(text: &Rope, from: usize) -> Option<usize> {
    let row = text::row(text, from);
    let end = text::line_end(text, row);
    let mut offset = from;
    let (bracket, position) = loop {
        if offset >= end {
            return None;
        }
        let c = text::char_at(text, offset)?;
        if "()[]{}".contains(c) {
            break (c, offset);
        }
        offset = text::next_offset(text, offset);
    };
    let (open, close, forward) = match bracket {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        _ => ('{', '}', false),
    };
    let mut depth = 0usize;
    if forward {
        let mut offset = position;
        for c in text.chars_at(position) {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            offset += c.len_utf8();
        }
    } else {
        let mut offset = position + close.len_utf8();
        for c in text.chars_at(offset).reversed() {
            offset -= c.len_utf8();
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
        }
    }
    None
}

/// `{` and `}`: Vim's `findpar`. Paragraphs end at empty lines.
fn paragraph(text: &Rope, from: usize, count: usize, forward: bool) -> Option<Target> {
    let last_row = text::last_row(text);
    let mut row = text::row(text, from);
    for remaining in (0..count).rev() {
        let mut did_skip = false;
        let mut first = true;
        loop {
            if !text::is_empty_line(text, row) {
                did_skip = true;
            }
            if !first && did_skip && text::is_empty_line(text, row) {
                break;
            }
            let next = if forward {
                (row < last_row).then(|| row + 1)
            } else {
                row.checked_sub(1)
            };
            match next {
                Some(next) => row = next,
                None => {
                    if remaining > 0 {
                        return None;
                    }
                    break;
                }
            }
            first = false;
        }
    }
    if forward && row == last_row && !text::is_empty_line(text, row) {
        let offset = text::last_char(text, row);
        return Some(Target::new(offset, MotionKind::Inclusive));
    }
    let offset = text::line_start(text, row);
    (offset != from).then(|| Target::new(offset, MotionKind::Exclusive))
}

/// The buffer rows shown in full, from the last layout.
pub(super) fn visible_rows(state: &TextareaState) -> Option<Range<usize>> {
    let layout = state.last_layout.as_ref()?;
    let bounds = state.last_bounds?;
    let line_height = f32::from(layout.line_height);
    if line_height <= 0. {
        return None;
    }
    let scroll = state
        .deferred_scroll_offset
        .unwrap_or_else(|| state.scroll_handle.offset());
    let top = (-f32::from(scroll.y)).max(0.);
    let height = f32::from(bounds.size.height);
    let display_rows = state.display_map.display_row_count();
    if display_rows == 0 {
        return None;
    }
    let first = ((top / line_height).ceil() as usize).min(display_rows - 1);
    let last = (((top + height) / line_height).floor() as usize)
        .saturating_sub(1)
        .clamp(first, display_rows - 1);
    let first = state.display_map.display_row_to_buffer_line(first);
    let last = state.display_map.display_row_to_buffer_line(last);
    Some(first..last.max(first) + 1)
}
