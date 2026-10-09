//! The kill ring: killing, copying and yanking, and the system clipboard.
//!
//! Every kill goes to the front of the ring and to the clipboard. Kills in a
//! row join into one entry, a forward kill at its end and a backward one at
//! its start. A yank first takes the clipboard into the ring when it holds
//! text copied elsewhere, so C-y pastes what another application copied.

use std::collections::VecDeque;
use std::ops::Range;

use gpui::{ClipboardItem, Context, Window};

use super::super::commands::Edit;
use super::motion::words_from;
use super::{Invocation, Last, deactivate_mark, emacs_mut, finish, region};
use crate::input::{RopeExt as _, TextareaState};

/// How many kills the ring keeps, as in Emacs.
const KILL_RING_MAX: usize = 120;

#[derive(Debug, Default)]
pub(super) struct KillRing {
    /// The latest kill first.
    entries: VecDeque<String>,
    /// The entry the next yank inserts. Meta-Y moves it back.
    yank: usize,
    /// What the ring last put on the clipboard or took from it, to tell when
    /// another application copied something since.
    clipboard: Option<String>,
}

impl KillRing {
    pub(super) fn iter(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    fn push(&mut self, text: String) {
        self.entries.push_front(text);
        self.entries.truncate(KILL_RING_MAX);
        self.yank = 0;
    }

    /// Join `text` to the latest kill: at its end for a forward kill, at its
    /// start for a backward one.
    fn append(&mut self, text: &str, forward: bool) {
        match self.entries.front_mut() {
            Some(latest) if forward => latest.push_str(text),
            Some(latest) => latest.insert_str(0, text),
            None => self.entries.push_front(text.to_string()),
        }
        self.yank = 0;
    }

    fn current(&self) -> Option<&str> {
        self.entries.get(self.yank).map(String::as_str)
    }

    /// Move the yank `by` entries back in the ring, wrapping around.
    fn rotate(&mut self, by: i64) {
        if !self.entries.is_empty() {
            let len = self.entries.len() as i64;
            self.yank = (self.yank as i64 + by).rem_euclid(len) as usize;
        }
    }
}

/// Put `text` in the kill ring, joined to the latest kill when `append`
/// gives the direction, and on the clipboard.
fn save(
    state: &mut TextareaState,
    text: String,
    append: Option<bool>,
    cx: &mut Context<TextareaState>,
) {
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    let ring = &mut emacs.kill_ring;
    match append {
        Some(forward) => ring.append(&text, forward),
        None => ring.push(text),
    }
    if let Some(latest) = ring.entries.front().cloned() {
        cx.write_to_clipboard(ClipboardItem::new_string(latest.clone()));
        ring.clipboard = Some(latest);
    }
}

/// The text of `ranges`, one per line.
fn texts(state: &TextareaState, ranges: &[Range<usize>]) -> String {
    ranges
        .iter()
        .map(|range| state.text.slice(range.clone()).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Kill `ranges`, one per cursor, joining the kill to the last one when the
/// command before was a kill too.
fn kill(
    state: &mut TextareaState,
    invocation: &Invocation,
    ranges: Vec<Range<usize>>,
    forward: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let mut ranges: Vec<Range<usize>> = ranges.into_iter().filter(|r| !r.is_empty()).collect();
    let appending = matches!(invocation.last, Some(Last::Kill));
    if ranges.is_empty() || !state.is_editable() {
        // Nothing to kill keeps a run of kills going.
        if appending {
            finish(state, Last::Kill);
        }
        return;
    }
    ranges.sort_by_key(|range| range.start);
    let text = texts(state, &ranges);
    save(state, text, appending.then_some(forward), cx);
    let edits = ranges
        .into_iter()
        .map(|range| Edit {
            range,
            text: String::new(),
            select: 0..0,
        })
        .collect();
    state.apply_edits(edits, window, cx);
    finish(state, Last::Kill);
}

/// C-k: to the end of the line, or through the line break when only spaces
/// and tabs are left. With a count, that many whole lines, or back to the
/// start of the line for zero and a negative count.
pub(super) fn kill_line(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = invocation.prefix.as_ref().map(|_| invocation.count());
    let text = &state.text;
    let last_row = text.lines_len().saturating_sub(1);
    let line_start = |row: usize| {
        if row > last_row {
            text.len()
        } else {
            text.line_start_offset(row)
        }
    };
    let ranges = state
        .selections
        .iter()
        .map(|selection| {
            let caret = selection.cursor_offset();
            let row = text.offset_to_point(caret).row;
            match count {
                None => {
                    let end = text.line_end_offset(row);
                    let rest = text.slice(caret..end);
                    if rest.chars().all(|c| matches!(c, ' ' | '\t' | '\r')) {
                        caret..line_start(row + 1)
                    } else {
                        caret..end
                    }
                }
                Some(count) if count > 0 => caret..line_start(row + count as usize),
                Some(count) => line_start(row.saturating_sub(count.unsigned_abs() as usize))..caret,
            }
        })
        .collect();
    let forward = count.is_none_or(|count| count > 0);
    kill(state, &invocation, ranges, forward, window, cx);
}

/// M-d and M-Backspace: to the end of the word, or the start of the one
/// before.
pub(super) fn kill_word(
    state: &mut TextareaState,
    invocation: Invocation,
    forward: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = if forward {
        invocation.count()
    } else {
        -invocation.count()
    };
    let ranges = state
        .selections
        .iter()
        .map(|selection| {
            let caret = selection.cursor_offset();
            let to = words_from(state, caret, count);
            caret.min(to)..caret.max(to)
        })
        .collect();
    kill(state, &invocation, ranges, count >= 0, window, cx);
}

/// C-w: the region.
pub(super) fn kill_region(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let ranges = region(state);
    kill(state, &invocation, ranges, true, window, cx);
}

/// M-w: copy the region and deactivate the mark.
pub(super) fn kill_ring_save(
    state: &mut TextareaState,
    invocation: Invocation,
    _: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let ranges = region(state);
    if ranges.is_empty() {
        return;
    }
    let text = texts(state, &ranges);
    let appending = matches!(invocation.last, Some(Last::Kill));
    save(state, text, appending.then_some(true), cx);
    deactivate_mark(state, cx);
}

/// Take the clipboard into the kill ring when another application copied
/// text since the ring last saw it.
fn take_clipboard(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
        return;
    };
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    let ring = &mut emacs.kill_ring;
    if text.is_empty() || ring.clipboard.as_deref() == Some(text.as_str()) {
        return;
    }
    ring.push(text.clone());
    ring.clipboard = Some(text);
}

/// C-y: insert the latest kill, leaving the mark at its start. After C-u the
/// caret goes to the start and the mark to the end. After a count N, the
/// Nth latest kill.
pub(super) fn yank(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if !state.is_editable() {
        return;
    }
    take_clipboard(state, cx);
    let caret_at_start = invocation.is_plain_prefix();
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    if invocation.prefix.is_some() && !caret_at_start {
        emacs.kill_ring.rotate(invocation.count() - 1);
    }
    let Some(text) = emacs.kill_ring.current().map(str::to_owned) else {
        return;
    };
    let ranges = state
        .selections
        .iter()
        .map(|selection| selection.start..selection.end)
        .collect();
    insert(state, ranges, text, caret_at_start, true, window, cx);
}

/// M-y: replace the text just yanked with the kill before it in the ring.
/// It only follows C-y or another M-y.
pub(super) fn yank_pop(
    state: &mut TextareaState,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let Some(Last::Yank {
        ranges,
        caret_at_start,
    }) = invocation.last.clone()
    else {
        return;
    };
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    emacs.kill_ring.rotate(invocation.count());
    let Some(text) = emacs.kill_ring.current().map(str::to_owned) else {
        return;
    };
    insert(state, ranges, text, caret_at_start, false, window, cx);
}

/// Replace `ranges` with `text` and remember them for M-y. The mark goes to
/// the other end of the active cursor's text, as a new mark when `push`.
fn insert(
    state: &mut TextareaState,
    ranges: Vec<Range<usize>>,
    text: String,
    caret_at_start: bool,
    push: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let len = text.len();
    let select = if caret_at_start { 0..0 } else { len..len };
    let edits = ranges
        .into_iter()
        .map(|range| Edit {
            range,
            text: text.clone(),
            select: select.clone(),
        })
        .collect();
    state.apply_edits(edits, window, cx);

    let other_end = |caret: usize| {
        if caret_at_start {
            caret + len
        } else {
            caret.saturating_sub(len)
        }
    };
    let yanked = state
        .selections
        .iter()
        .map(|selection| {
            let caret = selection.cursor_offset();
            caret.min(other_end(caret))..caret.max(other_end(caret))
        })
        .collect();
    let mark = other_end(state.cursor());
    if let Some(emacs) = emacs_mut(state) {
        if push {
            emacs.push_mark(mark);
        } else {
            emacs.mark = Some(mark);
        }
        emacs.mark_active = false;
    }
    finish(
        state,
        Last::Yank {
            ranges: yanked,
            caret_at_start,
        },
    );
}
