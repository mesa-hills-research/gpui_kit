//! Deleting characters, transposing, changing case and opening and joining
//! lines.
//!
//! These act from the caret and leave the region alone, as Emacs does, except
//! Backspace, which deletes a selection.

use gpui::{Context, Window};

use super::super::commands::Edit;
use super::motion::{backward_word, forward_word, words_from};
use super::{Fingerprint, Invocation, collapse_selections};
use crate::input::{
    Backspace, Delete, JoinLines, RopeExt as _, TextareaState, TransposeCharacters,
};

/// C-d and Backspace: delete `count` characters after the caret, or before
/// it for Backspace. Only Backspace deletes the selection.
pub(super) fn delete_char(
    state: &mut TextareaState,
    invocation: Invocation,
    forward: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = invocation.count();
    if forward {
        collapse_selections(state);
    }
    let forward = forward == (count >= 0);
    for _ in 0..count.unsigned_abs() {
        let before = Fingerprint::of(state);
        if forward {
            state.delete(&Delete, window, cx);
        } else {
            state.backspace(&Backspace, window, cx);
        }
        if Fingerprint::of(state) == before {
            break;
        }
    }
}

/// C-t: swap the characters around the caret and move past them, `count`
/// times.
pub(super) fn transpose_chars(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    collapse_selections(state);
    for _ in 0..invocation.count().max(1) {
        let before = Fingerprint::of(state);
        state.transpose_characters(&TransposeCharacters, window, cx);
        if Fingerprint::of(state) == before {
            break;
        }
    }
}

/// M-t: swap the word before the caret, or the one the caret is in, with the
/// word after it, and move past both, `count` times.
pub(super) fn transpose_words(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    collapse_selections(state);
    for _ in 0..invocation.count().max(1) {
        let edits: Vec<Edit> = state
            .selections
            .iter()
            .filter_map(|selection| {
                let first_start = backward_word(state, selection.cursor_offset());
                let first_end = forward_word(state, first_start);
                let second_end = forward_word(state, first_end);
                let second_start = backward_word(state, second_end);
                if second_start < first_end || second_end <= second_start {
                    return None;
                }
                let slice = |start, end| state.text.slice(start..end).to_string();
                let text = slice(second_start, second_end)
                    + &slice(first_end, second_start)
                    + &slice(first_start, first_end);
                Some(Edit {
                    range: first_start..second_end,
                    select: text.len()..text.len(),
                    text,
                })
            })
            .collect();
        if edits.is_empty() {
            break;
        }
        state.apply_edits(edits, window, cx);
    }
}

/// M-u, M-l and M-c: convert to the end of the word, or `count` words, and
/// move past them. A negative count converts the words before the caret and
/// leaves it where it is.
pub(super) fn convert_words(
    state: &mut TextareaState,
    invocation: Invocation,
    convert: fn(&str) -> String,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = invocation.count();
    collapse_selections(state);
    let edits = state
        .selections
        .iter()
        .filter_map(|selection| {
            let caret = selection.cursor_offset();
            let to = words_from(state, caret, count);
            let range = caret.min(to)..caret.max(to);
            if range.is_empty() {
                return None;
            }
            let text = convert(&state.text.slice(range.clone()).to_string());
            Some(Edit {
                range,
                select: text.len()..text.len(),
                text,
            })
        })
        .collect();
    state.apply_edits(edits, window, cx);
}

/// C-o: insert `count` line breaks after the caret, leaving it before them.
pub(super) fn open_line(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let Ok(count) = usize::try_from(invocation.count()) else {
        return;
    };
    let text = "\n".repeat(count);
    let edits = state
        .selections
        .iter()
        .map(|selection| {
            let caret = selection.cursor_offset();
            Edit {
                range: caret..caret,
                text: text.clone(),
                select: 0..0,
            }
        })
        .collect();
    state.apply_edits(edits, window, cx);
}

/// M-^: join the caret's line to the one before, with one space where the
/// line break and the indentation were, and leave the caret at the join.
pub(super) fn delete_indentation(
    state: &mut TextareaState,
    _: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let row = state.text.offset_to_point(state.cursor()).row;
    if row == 0 || !state.is_editable() {
        return;
    }
    let previous = state.text.line_start_offset(row - 1);
    state.move_to(previous, None, cx);
    state.join_lines(&JoinLines, window, cx);
}
