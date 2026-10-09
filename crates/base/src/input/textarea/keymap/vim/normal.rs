//! Normal mode: counts, operators waiting for their motion, the commands
//! that change text directly, undo, and `.`.
//!
//! Every command that changes text is a [`Command`], so `.` runs it again
//! with the count and register it had, and types again what was typed
//! after it.

use std::ops::Range;

use gpui::{Context, Window};

use super::{
    MAX_COUNT, VimMode, VimState,
    insert::{InsertAt, type_text},
    motion::{Env, Motion, MotionKind, Want, next_word_end},
    object::Object,
    operator::{Operator, Region, edit, toggle_case},
    register::RegisterKind,
    search::{self, LastSearch},
    text::{self, Cursor},
    visual::VisualExtent,
};
use crate::input::TextareaState;

/// A command that changes text, which `.` repeats.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Command {
    /// An operator and what it acts on.
    Operate { operator: Operator, target: Target },
    /// `r`
    ReplaceChars(char),
    /// `~`
    ToggleCaseChars,
    /// `J` with `spaces`, `gJ` without.
    Join { spaces: bool },
    /// `p`, `P`, `gp`, `gP`
    Put { before: bool, move_past: bool },
    /// `i`, `a` and the other ways into insert mode.
    Insert(InsertAt),
    /// `R`
    Replace,
    /// Ctrl-A and Ctrl-X
    Increment(i64),
}

/// What an operator acts on.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Motion(Motion),
    Object {
        object: Object,
        around: bool,
    },
    /// The caret's line, and more for a count: `dd`, `yy`, `>>`.
    Line,
    /// As much text as a visual selection held.
    Visual(VisualExtent),
}

/// The last change, for `.`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Change {
    pub(super) command: Command,
    pub(super) count: Option<usize>,
    pub(super) register: Option<char>,
    /// What was typed after the command started insert or replace mode.
    pub(super) text: Option<String>,
}

impl VimState {
    /// The count typed so far, the operator's multiplied by the motion's.
    pub(super) fn take_count(&mut self) -> Option<usize> {
        match (self.operator_count.take(), self.count.take()) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(1).saturating_mul(b.unwrap_or(1)).min(MAX_COUNT)),
        }
    }

    /// A digit of a count.
    pub(super) fn digit(&mut self, digit: usize) {
        let count = self
            .count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit);
        self.count = Some(count.min(MAX_COUNT));
    }

    /// Forget a half-typed command: its count, register and operator.
    pub(super) fn reset_pending(&mut self) {
        self.count = None;
        self.operator_count = None;
        self.operator = None;
        self.register = None;
        self.awaiting = None;
        if self.mode == VimMode::OperatorPending {
            self.mode = VimMode::Normal;
        }
    }

    /// Keep `change` for `.`. A change that went on into insert mode is
    /// kept when the insert ends, with what was typed.
    pub(super) fn record(&mut self, change: Change) {
        if matches!(self.mode, VimMode::Insert | VimMode::Replace) {
            self.pending_change = Some(change);
        } else {
            self.last_change = Some(change);
        }
    }

    /// Normal mode with the caret at `offset`, on a character.
    pub(super) fn enter_normal_at(
        &mut self,
        offset: usize,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        if self.visual.is_some() {
            self.leave_visual();
        }
        self.mode = VimMode::Normal;
        self.selected = None;
        self.reset_pending();
        let offset = text::normal_offset(&state.text, offset);
        state.move_to(offset, None, cx);
    }

    pub(super) fn enter_normal(
        &mut self,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let caret = state.cursor();
        self.enter_normal_at(caret, state, cx);
    }

    /// The column `j` and `k` aim for from `offset`.
    pub(super) fn want_column(&self, text: &ropey::Rope, offset: usize) -> usize {
        match self.want {
            Some((at, column)) if at == offset => column,
            _ => text::column(text, offset),
        }
    }

    fn remember_want(&mut self, want: Want, column: usize, offset: usize) {
        self.want = match want {
            Want::Keep => Some((offset, column)),
            Want::End => Some((offset, usize::MAX)),
            Want::Reset => None,
        };
    }

    /// A motion key: move the caret, extend the selection, or give an
    /// operator its text.
    pub(super) fn motion(
        &mut self,
        motion: Motion,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let count = self.take_count();
        match self.mode {
            VimMode::OperatorPending => {
                let Some(operator) = self.operator.take() else {
                    self.reset_pending();
                    return;
                };
                self.mode = VimMode::Normal;
                self.operate(operator, Target::Motion(motion), count, state, window, cx);
            }
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock => {
                let Some(visual) = self.visual else {
                    return;
                };
                let (motion, from) = self.prepare_motion(motion, visual.head, state, cx);
                let column = self.want_column(&state.text, from);
                let env = Env {
                    state,
                    vim: self,
                    operator: false,
                    visual: true,
                };
                if let Some(target) = motion.evaluate(&env, from, count) {
                    self.remember_want(target.want, column, target.offset);
                    // After `$`, a block reaches the ends of lines until a
                    // motion picks a column.
                    let to_line_end = match target.want {
                        Want::End => true,
                        Want::Keep => visual.to_line_end,
                        Want::Reset => false,
                    };
                    self.extend_visual(target.offset, to_line_end, state, cx);
                }
                self.register = None;
            }
            _ => {
                let (motion, from) = self.prepare_motion(motion, state.cursor(), state, cx);
                let column = self.want_column(&state.text, from);
                let env = Env {
                    state,
                    vim: self,
                    operator: false,
                    visual: false,
                };
                if let Some(target) = motion.evaluate(&env, from, count) {
                    let offset = text::normal_offset(&state.text, target.offset);
                    self.reset_pending();
                    self.mode = VimMode::Normal;
                    match motion.caret_motion() {
                        Some(caret_motion) => state
                            .caret_motion(caret_motion, |state| state.move_to(offset, None, cx)),
                        None => state.move_to(offset, None, cx),
                    }
                    self.remember_want(target.want, column, offset);
                } else {
                    self.reset_pending();
                }
            }
        }
    }

    /// Remember what a motion needs before it runs: the character `f`
    /// looks for, the word `*` searches. Returns the motion and where it
    /// starts.
    fn prepare_motion(
        &mut self,
        motion: Motion,
        from: usize,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) -> (Motion, usize) {
        match motion {
            Motion::Find { .. } => {
                self.last_find = Some(motion);
                (motion, from)
            }
            Motion::SearchWord { backward } => {
                let Some((range, pattern)) = search::word_pattern(&state.text, from) else {
                    return (motion, from);
                };
                search::highlight(&pattern, state, cx);
                self.last_search = Some(LastSearch { pattern, backward });
                // `#` starts from the word, so it finds the one before.
                (motion, if backward { range.start } else { from })
            }
            Motion::SearchNext { .. } => {
                if let Some(search) = &self.last_search {
                    let pattern = search.pattern.clone();
                    search::highlight(&pattern, state, cx);
                }
                (motion, from)
            }
            _ => (motion, from),
        }
    }

    /// An operator key: wait for the motion in normal mode, act on the
    /// selection in visual mode.
    pub(super) fn push_operator(
        &mut self,
        operator: Operator,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        match self.mode {
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock => {
                self.visual_operator(operator, state, window, cx);
            }
            VimMode::OperatorPending => self.reset_pending(),
            _ => {
                self.operator = Some(operator);
                self.operator_count = self.count.take();
                self.mode = VimMode::OperatorPending;
            }
        }
    }

    /// A text object: select it in visual mode, or give it to the operator.
    pub(super) fn object(
        &mut self,
        object: Object,
        around: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        if self.mode.is_visual() {
            self.visual_object(object, around, state, cx);
            return;
        }
        let count = self.take_count();
        let Some(operator) = self.operator.take() else {
            self.reset_pending();
            return;
        };
        self.mode = VimMode::Normal;
        self.operate(
            operator,
            Target::Object { object, around },
            count,
            state,
            window,
            cx,
        );
    }

    /// Run `operator` on `target` from the caret.
    pub(super) fn operate(
        &mut self,
        operator: Operator,
        target: Target,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let text = state.text.clone();
        let from = state.cursor();
        let register = self.register;
        let n = count.unwrap_or(1).max(1);
        let mut recorded = target.clone();
        let found = match &target {
            Target::Motion(motion) => {
                let (motion, start) = self.prepare_motion(*motion, from, state, cx);
                if let Motion::SearchWord { .. } = motion {
                    recorded = Target::Motion(Motion::SearchNext { reverse: false });
                }
                let on_word = Cursor::new(&text, from).class(false) != 0;
                let target = match motion {
                    // `cw` on a word changes to its end, as `ce` does.
                    Motion::NextWordStart { big } if operator == Operator::Change && on_word => {
                        let mut cursor = Cursor::new(&text, from);
                        next_word_end(&mut cursor, n, big, true);
                        Some(super::motion::Target {
                            offset: cursor.offset,
                            kind: MotionKind::Inclusive,
                            want: Want::Reset,
                        })
                    }
                    motion => {
                        let env = Env {
                            state,
                            vim: self,
                            operator: true,
                            visual: false,
                        };
                        motion.evaluate(&env, start, count)
                    }
                };
                target.map(|target| {
                    let region = Region::for_motion(&text, from, &target);
                    let cursor = match &region {
                        Region::Lines(rows) if text::row(&text, from) == rows.start => from,
                        _ => from.min(target.offset),
                    };
                    (region, cursor, motion.is_jump())
                })
            }
            Target::Object { object, around } => object
                .range(&text, from, *around, n, None)
                .filter(|found| !found.range.is_empty() || operator == Operator::Change)
                .map(|found| {
                    let start = found.range.start;
                    let region = if found.linewise {
                        let end = text::prev_offset(&text, found.range.end).max(start);
                        Region::Lines(text::row(&text, start)..text::row(&text, end) + 1)
                    } else {
                        Region::Chars(found.range)
                    };
                    (region, start, false)
                }),
            Target::Line => {
                let row = text::row(&text, from);
                let last_row = text::last_row(&text);
                (n == 1 || row < last_row)
                    .then(|| (Region::Lines(row..(row + n).min(last_row + 1)), from, false))
            }
            Target::Visual(extent) => Some((extent.region(&text, from), from, false)),
        };
        let Some((region, cursor, big)) = found else {
            self.reset_pending();
            return;
        };
        self.apply_operator(operator, region, cursor, big, 1, state, window, cx);
        if operator.changes_text() {
            self.record(Change {
                command: Command::Operate {
                    operator,
                    target: recorded,
                },
                count,
                register,
                text: None,
            });
        }
    }

    /// Run a command: from its key, or again for `.`.
    pub(super) fn execute(
        &mut self,
        command: Command,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        match command {
            Command::Operate { operator, target } => {
                self.operate(operator, target, count, state, window, cx)
            }
            Command::ReplaceChars(character) => {
                self.replace_chars(character, count, state, window, cx)
            }
            Command::ToggleCaseChars => self.toggle_case_chars(count, state, window, cx),
            Command::Join { spaces } => self.join(spaces, count, state, window, cx),
            Command::Put { before, move_past } => {
                self.put(before, move_past, count, state, window, cx)
            }
            Command::Insert(at) => self.insert(at, count, state, window, cx),
            Command::Replace => self.start_replace(count, state, window, cx),
            Command::Increment(step) => self.increment(step, count, state, window, cx),
        }
    }

    /// `.`: the last change again, with a new count if one was typed.
    pub(super) fn repeat(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let count = self.take_count();
        let Some(change) = self.last_change.clone() else {
            self.reset_pending();
            return;
        };
        let count = count.or(change.count);
        self.register = change.register;
        self.execute(change.command.clone(), count, state, window, cx);
        if let Some(typed) = &change.text
            && matches!(self.mode, VimMode::Insert | VimMode::Replace)
        {
            if self.mode == VimMode::Replace {
                self.replace_typed(typed, state, window, cx);
            } else {
                type_text(typed, state, window, cx);
            }
            self.leave_insert(state, window, cx);
        }
    }

    /// `u` and Ctrl-R.
    pub(super) fn undo_redo(
        &mut self,
        undo: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let count = self.take_count().unwrap_or(1);
        for _ in 0..count {
            if undo {
                state.undo(&crate::input::Undo, window, cx);
            } else {
                state.redo(&crate::input::Redo, window, cx);
            }
        }
        if self.visual.is_some() {
            self.leave_visual();
        }
        // Undo restores the caret from before the change. Redo puts it where
        // the change starts, as Vim does.
        let mut caret = state
            .selections
            .iter()
            .map(|selection| selection.start)
            .min()
            .unwrap_or_default();
        if !undo {
            let mark = state.undo_manager.mark();
            if let Some(start) = state
                .undo_manager
                .changes_since(mark.saturating_sub(1))
                .iter()
                .map(|change| change.new_range.start)
                .min()
            {
                caret = start;
            }
        }
        self.enter_normal_at(caret, state, cx);
    }

    /// `r`: replace `count` characters with `character`, or with a line
    /// break.
    fn replace_chars(
        &mut self,
        character: char,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let n = count.unwrap_or(1).max(1);
        let text = state.text.clone();
        let from = state.cursor();
        let end = text::line_end(&text, text::row(&text, from));
        let mut stop = from;
        for _ in 0..n {
            if stop >= end {
                self.reset_pending();
                return;
            }
            stop = text::next_offset(&text, stop);
        }
        let register = self.register;
        let (replacement, caret) = if character == '\n' || character == '\r' {
            ("\n".to_string(), from + 1)
        } else {
            let replacement = character.to_string().repeat(n);
            let caret = from + replacement.len() - character.len_utf8();
            (replacement, caret)
        };
        edit(state, vec![(from..stop, replacement)], window, cx);
        self.enter_normal_at(caret, state, cx);
        self.record(Change {
            command: Command::ReplaceChars(character),
            count,
            register,
            text: None,
        });
    }

    /// `~`: flip the case of `count` characters and move past them.
    fn toggle_case_chars(
        &mut self,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let n = count.unwrap_or(1).max(1);
        let text = state.text.clone();
        let from = state.cursor();
        let end = text::line_end(&text, text::row(&text, from));
        let mut stop = from;
        for _ in 0..n {
            if stop >= end {
                break;
            }
            stop = text::next_offset(&text, stop);
        }
        if stop == from {
            self.reset_pending();
            return;
        }
        let old = text.slice(from..stop).to_string();
        let new = toggle_case(&old);
        let caret = from + new.len();
        if new != old {
            edit(state, vec![(from..stop, new)], window, cx);
        }
        self.enter_normal_at(caret, state, cx);
        self.record(Change {
            command: Command::ToggleCaseChars,
            count,
            register: None,
            text: None,
        });
    }

    /// `J` and `gJ`: join `count` lines, at least two.
    pub(super) fn join(
        &mut self,
        spaces: bool,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let (rows, record) = match self.visual_region(state, true) {
            Some(region) => {
                self.leave_visual();
                let rows = region.rows(&state.text);
                (rows.start..rows.end.max(rows.start + 2), false)
            }
            None => {
                let row = text::row(&state.text, state.cursor());
                let n = count.unwrap_or(2).max(2);
                (row..row + n, true)
            }
        };
        let text = state.text.clone();
        let last_row = text::last_row(&text);
        if rows.start >= last_row {
            self.enter_normal(state, cx);
            return;
        }
        let rows = rows.start..rows.end.min(last_row + 1);
        let mut joined = text::line_text(&text, rows.start);
        let mut caret = joined.len();
        for row in rows.start + 1..rows.end {
            let next = text::line_text(&text, row);
            if spaces {
                let next = next.trim_start_matches([' ', '\t']);
                caret = joined.len();
                if !next.is_empty()
                    && !joined.is_empty()
                    && !joined.ends_with([' ', '\t'])
                    && !next.starts_with(')')
                {
                    joined.push(' ');
                } else if next.is_empty() || joined.is_empty() {
                    // Nothing joined after the line: the caret stays on its
                    // last character.
                    caret = joined.len().saturating_sub(1);
                }
                joined.push_str(next);
            } else {
                caret = joined.len();
                joined.push_str(&next);
            }
        }
        let start = text::line_start(&text, rows.start);
        let range = start..text::line_end(&text, rows.end - 1);
        edit(state, vec![(range, joined)], window, cx);
        self.enter_normal_at(start + caret, state, cx);
        if record {
            self.record(Change {
                command: Command::Join { spaces },
                count,
                register: None,
                text: None,
            });
        }
    }

    /// `p`, `P`, `gp` and `gP`.
    fn put(
        &mut self,
        before: bool,
        move_past: bool,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let name = self.register.take();
        let Some(register) = self.read_register(name, cx) else {
            self.reset_pending();
            return;
        };
        let n = count.unwrap_or(1).max(1);
        let text = state.text.clone();
        let caret = state.cursor();
        let row = text::row(&text, caret);
        let last_row = text::last_row(&text);
        let caret = match register.kind {
            RegisterKind::Chars => {
                let line_empty = text::is_empty_line(&text, row);
                let at = if before || line_empty || caret >= text::line_end(&text, row) {
                    caret
                } else {
                    text::next_offset(&text, caret)
                };
                let inserted = register.text.repeat(n);
                edit(state, vec![(at..at, inserted.clone())], window, cx);
                if move_past {
                    at + inserted.len()
                } else if inserted.contains('\n') {
                    at
                } else {
                    text::prev_offset(&state.text, at + inserted.len()).max(at)
                }
            }
            RegisterKind::Lines => {
                let mut lines = register.text.clone();
                if !lines.ends_with('\n') {
                    lines.push('\n');
                }
                let inserted = lines.repeat(n);
                let line_count = inserted.matches('\n').count();
                let first_row;
                if before {
                    let at = text::line_start(&text, row);
                    edit(state, vec![(at..at, inserted)], window, cx);
                    first_row = row;
                } else if row < last_row {
                    let at = text::line_start(&text, row + 1);
                    edit(state, vec![(at..at, inserted)], window, cx);
                    first_row = row + 1;
                } else {
                    let at = text.len();
                    let mut inserted = inserted;
                    inserted.pop();
                    edit(state, vec![(at..at, format!("\n{inserted}"))], window, cx);
                    first_row = row + 1;
                }
                if move_past {
                    let after = (first_row + line_count).min(text::last_row(&state.text));
                    text::line_start(&state.text, after)
                } else {
                    text::first_non_blank(&state.text, first_row)
                }
            }
            RegisterKind::Block => {
                let column = text::column(&text, caret)
                    + usize::from(!before && !text::is_empty_line(&text, row));
                let mut edits = Vec::new();
                let mut appended = String::new();
                for (ix, line) in register.text.split('\n').enumerate() {
                    let line = line.repeat(n);
                    let target = row + ix;
                    if target > last_row {
                        appended.push('\n');
                        appended.push_str(&" ".repeat(column));
                        appended.push_str(&line);
                        continue;
                    }
                    let chars = text::line_chars(&text, target);
                    let at = text::offset_at_column(&text, target, column);
                    let padding = " ".repeat(column.saturating_sub(chars));
                    edits.push((at..at, format!("{padding}{line}")));
                }
                if !appended.is_empty() {
                    let end = text.len();
                    edits.push((end..end, appended));
                }
                edit(state, edits, window, cx);
                text::offset_at_column(&state.text, row, column)
            }
        };
        self.enter_normal_at(caret, state, cx);
        self.record(Change {
            command: Command::Put { before, move_past },
            count,
            register: name,
            text: None,
        });
    }

    /// Ctrl-A and Ctrl-X: add `step` times the count to the number at or
    /// after the caret on its line.
    fn increment(
        &mut self,
        step: i64,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let text = state.text.clone();
        let caret = state.cursor();
        let row = text::row(&text, caret);
        let line_start = text::line_start(&text, row);
        let line = text::line_text(&text, row);
        let Some((range, replacement)) =
            increment_number(&line, caret - line_start, step * count.unwrap_or(1) as i64)
        else {
            self.reset_pending();
            return;
        };
        let start = line_start + range.start;
        let caret = start + replacement.len() - 1;
        edit(
            state,
            vec![(start..line_start + range.end, replacement)],
            window,
            cx,
        );
        self.enter_normal_at(caret, state, cx);
        self.record(Change {
            command: Command::Increment(step),
            count,
            register: None,
            text: None,
        });
    }
}

/// The number at or after `column` in `line` with `delta` added: its range
/// in the line and its new text. Decimal numbers, negative after a `-`, and
/// hexadecimal ones after `0x`.
pub(super) fn increment_number(
    line: &str,
    column: usize,
    delta: i64,
) -> Option<(Range<usize>, String)> {
    let bytes = line.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        let hex = bytes[ix] == b'0'
            && matches!(bytes.get(ix + 1), Some(b'x' | b'X'))
            && bytes.get(ix + 2).is_some_and(u8::is_ascii_hexdigit);
        if hex {
            let digits = ix + 2;
            let mut end = digits;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            if end > column {
                let old = &line[digits..end];
                let value = i128::from_str_radix(old, 16).ok()?;
                let new = (value + i128::from(delta)).max(0);
                let mut formatted = if old.chars().any(|c| c.is_ascii_uppercase()) {
                    format!("{new:X}")
                } else {
                    format!("{new:x}")
                };
                while formatted.len() < old.len() {
                    formatted.insert(0, '0');
                }
                return Some((digits..end, formatted));
            }
            ix = end;
        } else if bytes[ix].is_ascii_digit() {
            let mut end = ix;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > column {
                let start = if ix > 0 && bytes[ix - 1] == b'-' {
                    ix - 1
                } else {
                    ix
                };
                let value: i128 = line[start..end].parse().ok()?;
                return Some((start..end, (value + i128::from(delta)).to_string()));
            }
            ix = end;
        } else {
            ix += 1;
        }
    }
    None
}
