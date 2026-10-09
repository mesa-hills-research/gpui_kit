//! Insert and replace modes: the ways in, the keys they add, and the way
//! out, which repeats the inserted text for a count, makes the whole insert
//! one undo step and keeps it for `.`.
//!
//! The text typed in a session is read back from the undo history when it
//! ends: the edits since it began, followed from where typing started.
//! Whatever produced them, typing, an input method, a suggestion or a
//! closing bracket inserted for an opening one, the result is what `.`
//! types again.

use gpui::{Context, Window};

use super::{
    Change, Command, VimMode, VimState,
    operator::{self, edit},
    text,
};
use crate::input::TextareaState;

/// Where `i`, `a` and the other insert commands start typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertAt {
    /// `i`
    Before,
    /// `a`
    After,
    /// `I`
    FirstNonBlank,
    /// `gI`
    LineStart,
    /// `A`
    LineEnd,
    /// `o`
    LineBelow,
    /// `O`
    LineAbove,
}

/// How a count repeats the typed text when the insert ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InsertRepeat {
    /// Again at the caret, as `3ix` does.
    Inline,
    /// On new lines below, as `3ox` does.
    LineBelow,
    /// On new lines above.
    LineAbove,
}

/// One stay in insert or replace mode.
pub(super) struct InsertSession {
    /// The history before the command that started it, to undo as one step.
    undo_mark: usize,
    /// The history when typing began.
    typed_mark: usize,
    /// Where typing began.
    start: usize,
    count: usize,
    repeat: InsertRepeat,
    /// Several carets, as in visual block mode: not repeated with `.`.
    block: bool,
    /// The command opened the line with indentation, which goes again if
    /// nothing is typed.
    indented: bool,
    /// In replace mode, the character each typed one replaced, or none when
    /// it was added at a line's end.
    replaced: Option<Vec<Option<char>>>,
}

impl VimState {
    /// Start insert mode with the caret at `caret`.
    pub(super) fn start_insert(
        &mut self,
        caret: usize,
        count: usize,
        repeat: InsertRepeat,
        state: &mut TextareaState,
        _window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        self.mode = VimMode::Insert;
        self.visual = None;
        state.move_to(caret, None, cx);
        self.insert = Some(InsertSession {
            undo_mark: self.command_mark,
            typed_mark: state.undo_manager.mark(),
            start: state.cursor(),
            count: count.max(1),
            repeat,
            block: false,
            indented: false,
            replaced: None,
        });
    }

    /// Start insert mode with a caret at each of `carets`, typing into all
    /// of them, as visual block's `I`, `A` and `c` do.
    pub(super) fn start_block_insert(
        &mut self,
        carets: Vec<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(first) = carets.first().copied() else {
            self.enter_normal(state, cx);
            return;
        };
        self.start_insert(first, 1, InsertRepeat::Inline, state, window, cx);
        for caret in carets.into_iter().skip(1) {
            let id = state.selections.generate_id();
            state
                .selections
                .add(crate::input::cursor::CursorSelection::new(id, caret, caret));
        }
        if let Some(session) = &mut self.insert {
            session.block = true;
        }
        cx.notify();
    }

    /// `i`, `a`, `I`, `A`, `gI`, `o` and `O`.
    pub(super) fn insert(
        &mut self,
        at: InsertAt,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let count = count.unwrap_or(1);
        self.pending_change = Some(Change {
            command: Command::Insert(at),
            count: Some(count),
            register: None,
            text: None,
        });
        let text = state.text.clone();
        let caret = state.cursor();
        let row = text::row(&text, caret);
        let mut repeat = InsertRepeat::Inline;
        let mut indented = false;
        let caret = match at {
            InsertAt::Before => caret,
            InsertAt::After => {
                if caret < text::line_end(&text, row) {
                    text::next_offset(&text, caret)
                } else {
                    caret
                }
            }
            InsertAt::FirstNonBlank => text::first_non_blank(&text, row),
            InsertAt::LineStart => text::line_start(&text, row),
            InsertAt::LineEnd => text::line_end(&text, row),
            InsertAt::LineBelow | InsertAt::LineAbove => {
                let below = at == InsertAt::LineBelow;
                repeat = if below {
                    InsertRepeat::LineBelow
                } else {
                    InsertRepeat::LineAbove
                };
                let indent = text::indentation(&text, row);
                indented = !indent.is_empty();
                self.open_line(below, &indent, state, window, cx)
            }
        };
        self.start_insert(caret, count, repeat, state, window, cx);
        if let Some(session) = &mut self.insert {
            session.indented = indented;
        }
    }

    /// Insert a line below or above the caret's, indented by `indent`.
    /// Returns where its text starts.
    fn open_line(
        &mut self,
        below: bool,
        indent: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) -> usize {
        let text = state.text.clone();
        let row = text::row(&text, state.cursor());
        if below {
            let at = text::line_end(&text, row);
            edit(state, vec![(at..at, format!("\n{indent}"))], window, cx);
            at + 1 + indent.len()
        } else {
            let at = text::line_start(&text, row);
            edit(state, vec![(at..at, format!("{indent}\n"))], window, cx);
            at + indent.len()
        }
    }

    /// `R`: replace mode.
    pub(super) fn start_replace(
        &mut self,
        count: Option<usize>,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        self.pending_change = Some(Change {
            command: Command::Replace,
            count,
            register: None,
            text: None,
        });
        let caret = state.cursor();
        self.start_insert(
            caret,
            count.unwrap_or(1),
            InsertRepeat::Inline,
            state,
            window,
            cx,
        );
        self.mode = VimMode::Replace;
        if let Some(session) = &mut self.insert {
            session.replaced = Some(Vec::new());
        }
    }

    /// Text typed in replace mode: each character replaces the one under
    /// the caret, or is added at a line's end.
    pub(super) fn replace_typed(
        &mut self,
        typed: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        for c in typed.chars() {
            let text = state.text.clone();
            let caret = state.cursor();
            let end = text::line_end(&text, text::row(&text, caret));
            let (range, old) = if caret < end && c != '\n' {
                (
                    caret..text::next_offset(&text, caret),
                    text::char_at(&text, caret),
                )
            } else {
                (caret..caret, None)
            };
            edit(state, vec![(range.clone(), c.to_string())], window, cx);
            state.move_to(range.start + c.len_utf8(), None, cx);
            if let Some(Some(replaced)) = self.insert.as_mut().map(|session| &mut session.replaced)
            {
                replaced.push(old);
            }
        }
    }

    /// Backspace in replace mode: put back what the last typed character
    /// replaced.
    pub(super) fn replace_backspace(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let text = state.text.clone();
        let caret = state.cursor();
        let row = text::row(&text, caret);
        if caret <= text::line_start(&text, row) {
            return;
        }
        let before = text::prev_offset(&text, caret);
        let restored = self
            .insert
            .as_mut()
            .and_then(|session| session.replaced.as_mut())
            .and_then(|replaced| replaced.pop());
        match restored {
            Some(Some(original)) => {
                edit(
                    state,
                    vec![(before..caret, original.to_string())],
                    window,
                    cx,
                );
            }
            Some(None) => edit(state, vec![(before..caret, String::new())], window, cx),
            None => {}
        }
        state.move_to(before, None, cx);
    }

    /// Escape in insert or replace mode.
    pub(super) fn leave_insert(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let session = self.insert.take();
        self.mode = VimMode::Normal;
        state.selections.remove_all_but_active();
        let Some(session) = session else {
            self.enter_normal(state, cx);
            return;
        };
        let typed = (!session.block).then(|| typed_text(&session, state));
        if let Some(typed) = &typed {
            if typed.is_empty() && session.indented {
                // Nothing typed on an indented new line: drop the indentation.
                let row = text::row(&state.text, state.cursor());
                if text::is_blank_line(&state.text, row) {
                    let range =
                        text::line_start(&state.text, row)..text::line_end(&state.text, row);
                    edit(state, vec![(range.clone(), String::new())], window, cx);
                    state.move_to(range.start, None, cx);
                }
            }
            if session.count > 1 && !typed.is_empty() {
                for _ in 1..session.count {
                    match session.repeat {
                        InsertRepeat::Inline => {
                            if session.replaced.is_some() {
                                self.replace_typed(typed, state, window, cx);
                            } else {
                                type_text(typed, state, window, cx);
                            }
                        }
                        InsertRepeat::LineBelow | InsertRepeat::LineAbove => {
                            let row = text::row(&state.text, state.cursor());
                            let indent = text::indentation(&state.text, row);
                            let below = session.repeat == InsertRepeat::LineBelow;
                            let caret = self.open_line(below, &indent, state, window, cx);
                            state.move_to(caret, None, cx);
                            type_text(typed, state, window, cx);
                        }
                    }
                }
            }
        }
        state.undo_manager.group_since(session.undo_mark);
        if let Some(typed) = typed {
            self.last_inserted = Some(typed.clone());
            if let Some(mut change) = self.pending_change.take() {
                change.text = Some(typed);
                self.last_change = Some(change);
            }
        }
        self.pending_change = None;
        // Vim leaves the caret on the last character typed, or after a block
        // insert, where the block starts.
        let caret = state.cursor();
        let row = text::row(&state.text, caret);
        let caret = if session.block {
            session.start
        } else if caret > text::line_start(&state.text, row) {
            text::prev_offset(&state.text, caret)
        } else {
            caret
        };
        self.enter_normal_at(caret, state, cx);
    }

    /// Ctrl-U in insert mode: delete back to the line's text, or to its start
    /// from there, or join with the line before from the start.
    pub(super) fn delete_to_line_start(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let text = state.text.clone();
        let caret = state.cursor();
        let row = text::row(&text, caret);
        let line_start = text::line_start(&text, row);
        let first = text::first_non_blank(&text, row);
        let start = if caret > first {
            first
        } else if caret > line_start {
            line_start
        } else if row > 0 {
            text::line_end(&text, row - 1)
        } else {
            return;
        };
        edit(state, vec![(start..caret, String::new())], window, cx);
        state.move_to(start, None, cx);
    }

    /// Ctrl-T and Ctrl-D in insert mode: shift the caret's line, keeping the
    /// caret on the same character.
    pub(super) fn shift_insert_line(
        &mut self,
        right: bool,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let caret = state.cursor();
        let row = text::row(&state.text, caret);
        let from_end = state.text.len() - caret;
        let edits = operator::shift_edits(state, &(row..row + 1), right, 1);
        if edits.is_empty() {
            return;
        }
        edit(state, edits, window, cx);
        let line_start = text::line_start(&state.text, row);
        let caret = (state.text.len() - from_end).max(line_start);
        state.move_to(caret, None, cx);
    }

    /// Insert `character` at the caret as typed, for Ctrl-V and Ctrl-R in
    /// insert mode.
    pub(super) fn insert_literal(
        &mut self,
        inserted: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        if self.mode == VimMode::Replace {
            self.replace_typed(inserted, state, window, cx);
        } else {
            type_text(inserted, state, window, cx);
        }
    }
}

/// Insert `typed` at the caret without the help typing gets: no closing
/// brackets and no suggestions.
pub(super) fn type_text(
    typed: &str,
    state: &mut TextareaState,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if typed.is_empty() {
        return;
    }
    state.replace_text_in_range_silent(None, typed, window, cx);
}

/// The text typed in `session`: its edits followed from where typing began.
/// Replace mode types from left to right, and Backspace takes back, so what
/// it typed runs up to the caret.
fn typed_text(session: &InsertSession, state: &TextareaState) -> String {
    if session.replaced.is_some() {
        let start = text::clip(&state.text, session.start);
        let end = text::clip(&state.text, state.cursor()).max(start);
        return state.text.slice(start..end).to_string();
    }
    let mut region = session.start..session.start;
    for change in state.undo_manager.changes_since(session.typed_mark) {
        let old = change.old_range.start..change.old_range.end;
        let delta = change.new_range.len() as isize - old.len() as isize;
        let shift = |offset: usize| (offset as isize + delta).max(0) as usize;
        if old.end < region.start {
            region = shift(region.start)..shift(region.end);
        } else if old.start > region.end {
            continue;
        } else if old.start >= region.start && old.end <= region.end {
            region.end = shift(region.end);
        } else {
            region.start = region.start.min(old.start);
            region.end = shift(region.end.max(old.end));
        }
    }
    let start = text::clip(&state.text, region.start);
    let end = text::clip(&state.text, region.end.max(start));
    state.text.slice(start..end).to_string()
}
