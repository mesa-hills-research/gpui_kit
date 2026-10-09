//! Moving by character, line, word, sentence, paragraph and page, and to the
//! ends of the line and the text.
//!
//! While the mark is active a motion extends the region. Otherwise it first
//! collapses the selections, so it moves from the caret, and deactivates the
//! mark. A count repeats a motion, and a negative one reverses it.

use std::ops::Range;

use gpui::{Context, Window};

use super::{Fingerprint, Invocation, collapse_selections, emacs_mut, region_active};
use crate::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use crate::input::{
    MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight, MoveToEnd,
    MoveToStart, MoveUp, RopeExt as _, SelectToEnd, SelectToEndOfLine, SelectToStart,
    SelectToStartOfLine, TextareaState, smooth_caret::CaretMotion,
};

/// Where a motion goes. Named after its action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motion {
    ForwardChar,
    BackwardChar,
    NextLine,
    PreviousLine,
    ForwardWord,
    BackwardWord,
    BeginningOfLine,
    EndOfLine,
    ForwardSentence,
    BackwardSentence,
    ForwardParagraph,
    BackwardParagraph,
    BeginningOfBuffer,
    EndOfBuffer,
    NextPage,
    PreviousPage,
    BackToIndentation,
}

impl Motion {
    /// The motion the other way, for a negative count. The ends of the line
    /// and the text, and the indentation, have none.
    fn reversed(self) -> Self {
        use Motion::*;
        match self {
            ForwardChar => BackwardChar,
            BackwardChar => ForwardChar,
            NextLine => PreviousLine,
            PreviousLine => NextLine,
            ForwardWord => BackwardWord,
            BackwardWord => ForwardWord,
            ForwardSentence => BackwardSentence,
            BackwardSentence => ForwardSentence,
            ForwardParagraph => BackwardParagraph,
            BackwardParagraph => ForwardParagraph,
            NextPage => PreviousPage,
            PreviousPage => NextPage,
            BeginningOfLine | EndOfLine | BeginningOfBuffer | EndOfBuffer | BackToIndentation => {
                self
            }
        }
    }
}

pub(super) fn run(
    state: &mut TextareaState,
    invocation: Invocation,
    motion: Motion,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = invocation.count();
    let extend = region_active(state);
    if !extend {
        if let Some(emacs) = emacs_mut(state) {
            emacs.mark_active = false;
        }
        collapse_selections(state);
        // Leaving for an end of the text sets the mark where the caret was.
        if matches!(motion, Motion::BeginningOfBuffer | Motion::EndOfBuffer) {
            let caret = state.cursor();
            if let Some(emacs) = emacs_mut(state) {
                emacs.push_mark(caret);
            }
        }
    }

    match motion {
        // With a count, C-a and C-e first move down `count - 1` lines.
        Motion::BeginningOfLine | Motion::EndOfLine => {
            let lines = count - 1;
            let line = if lines < 0 {
                Motion::PreviousLine
            } else {
                Motion::NextLine
            };
            repeat(state, line, lines.unsigned_abs(), extend, window, cx);
            step(state, motion, extend, window, cx);
        }
        Motion::BeginningOfBuffer | Motion::EndOfBuffer | Motion::BackToIndentation => {
            step(state, motion, extend, window, cx);
        }
        _ => {
            let motion = if count < 0 { motion.reversed() } else { motion };
            repeat(state, motion, count.unsigned_abs(), extend, window, cx);
        }
    }
}

/// Take `motion` `times` times, stopping once it goes nowhere.
fn repeat(
    state: &mut TextareaState,
    motion: Motion,
    times: u64,
    extend: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    for _ in 0..times {
        let before = Fingerprint::of(state);
        step(state, motion, extend, window, cx);
        if Fingerprint::of(state) == before {
            break;
        }
    }
}

fn step(
    state: &mut TextareaState,
    motion: Motion,
    extend: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    use Motion::*;
    match (motion, extend) {
        (ForwardChar, false) => state.right(&MoveRight, window, cx),
        (ForwardChar, true) => state.select_right(&SelectRight, window, cx),
        (BackwardChar, false) => state.left(&MoveLeft, window, cx),
        (BackwardChar, true) => state.select_left(&SelectLeft, window, cx),
        (NextLine, false) => state.down(&MoveDown, window, cx),
        (NextLine, true) => state.select_down(&SelectDown, window, cx),
        (PreviousLine, false) => state.up(&MoveUp, window, cx),
        (PreviousLine, true) => state.select_up(&SelectUp, window, cx),
        (BeginningOfLine, false) => state.home(&MoveHome, window, cx),
        (BeginningOfLine, true) => state.select_to_start_of_line(&SelectToStartOfLine, window, cx),
        (EndOfLine, false) => state.end(&MoveEnd, window, cx),
        (EndOfLine, true) => state.select_to_end_of_line(&SelectToEndOfLine, window, cx),
        (BeginningOfBuffer, false) => state.move_to_start(&MoveToStart, window, cx),
        (BeginningOfBuffer, true) => state.select_to_start(&SelectToStart, window, cx),
        (EndOfBuffer, false) => state.move_to_end(&MoveToEnd, window, cx),
        (EndOfBuffer, true) => state.select_to_end(&SelectToEnd, window, cx),
        (NextPage, false) => state.page_down(&MovePageDown, window, cx),
        (PreviousPage, false) => state.page_up(&MovePageUp, window, cx),
        (NextPage, true) => select_page(state, true, cx),
        (PreviousPage, true) => select_page(state, false, cx),
        (ForwardWord, _) => state.caret_motion(CaretMotion::Word, |state| {
            move_by(state, extend, forward_word, window, cx)
        }),
        (BackwardWord, _) => state.caret_motion(CaretMotion::Word, |state| {
            move_by(state, extend, backward_word, window, cx)
        }),
        (ForwardSentence, _) => move_by(state, extend, forward_sentence, window, cx),
        (BackwardSentence, _) => move_by(state, extend, backward_sentence, window, cx),
        (ForwardParagraph, _) => move_by(
            state,
            extend,
            |state, offset| state.paragraph_end_after(offset),
            window,
            cx,
        ),
        (BackwardParagraph, _) => move_by(
            state,
            extend,
            |state, offset| state.paragraph_start_before(offset),
            window,
            cx,
        ),
        (BackToIndentation, _) => move_by(state, extend, indentation, window, cx),
    }
}

/// Move every caret to `target(caret)`, or extend every selection there.
fn move_by(
    state: &mut TextareaState,
    extend: bool,
    target: fn(&TextareaState, usize) -> usize,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if extend {
        state.select_all_cursors_to(
            |state, selection| target(state, selection.cursor_offset()),
            cx,
        );
    } else {
        state.move_all_cursors(
            |state, selection| {
                let offset = target(state, selection.cursor_offset());
                (offset, state.preferred_column_for(offset), false)
            },
            None,
            window,
            cx,
        );
    }
}

/// Extend every selection by a page, the height of the textarea.
fn select_page(state: &mut TextareaState, down: bool, cx: &mut Context<TextareaState>) {
    let Some(line_height) = state.line_height() else {
        return;
    };
    let rows = (state.input_bounds.size.height / line_height) as isize;
    let rows = if down { rows } else { -rows };
    state.select_all_cursors_to(
        |state, selection| {
            state
                .vertical_target(
                    selection.cursor_offset(),
                    selection.column_anchor,
                    state.line_end_affinity_for(selection),
                    rows,
                )
                .0
        },
        cx,
    );
}

/// A letter or digit. Emacs's words are runs of them, and an apostrophe
/// between two keeps them one word, as in "don't".
fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

/// The end of the word at or after `offset`.
pub(super) fn forward_word(state: &TextareaState, offset: usize) -> usize {
    let mut chars = state.text.chars_at(offset).peekable();
    let mut offset = offset;
    while let Some(&c) = chars.peek().filter(|c| !is_word(**c)) {
        offset += c.len_utf8();
        chars.next();
    }
    while let Some(c) = chars.next() {
        if !is_word(c) && !(is_apostrophe(c) && chars.peek().is_some_and(|c| is_word(*c))) {
            break;
        }
        offset += c.len_utf8();
    }
    offset
}

/// The start of the word before `offset`.
pub(super) fn backward_word(state: &TextareaState, offset: usize) -> usize {
    let mut chars = state.text.chars_at(offset).reversed().peekable();
    let mut offset = offset;
    while let Some(&c) = chars.peek().filter(|c| !is_word(**c)) {
        offset -= c.len_utf8();
        chars.next();
    }
    while let Some(c) = chars.next() {
        if !is_word(c) && !(is_apostrophe(c) && chars.peek().is_some_and(|c| is_word(*c))) {
            break;
        }
        offset -= c.len_utf8();
    }
    offset
}

/// Where `count` words forward from `offset` end, or for a negative count,
/// where `-count` words back start.
pub(super) fn words_from(state: &TextareaState, offset: usize, count: i64) -> usize {
    let step = if count < 0 {
        backward_word
    } else {
        forward_word
    };
    (0..count.unsigned_abs()).fold(offset, |offset, _| step(state, offset))
}

/// The first character of the line that is not a space or tab.
fn indentation(state: &TextareaState, offset: usize) -> usize {
    let row = state.text.offset_to_point(offset).row;
    let start = state.text.line_start_offset(row);
    let indent: usize = state
        .text
        .slice_line(row)
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .map(char::len_utf8)
        .sum();
    start + indent
}

fn is_blank_row(state: &TextareaState, row: usize) -> bool {
    state.text.slice_line(row).chars().all(char::is_whitespace)
}

/// The text of the paragraph at `offset`, from the start of its first line to
/// the end of its last. On a blank line, the next paragraph's, or the
/// previous one's when `forward` is false. `None` when there is none that
/// way.
fn paragraph_text(state: &TextareaState, offset: usize, forward: bool) -> Option<Range<usize>> {
    let last = state.text.lines_len().saturating_sub(1);
    let mut row = state.text.offset_to_point(offset).row;
    while is_blank_row(state, row) {
        if forward && row < last {
            row += 1;
        } else if !forward && row > 0 {
            row -= 1;
        } else {
            return None;
        }
    }
    let (mut first, mut end) = (row, row);
    while first > 0 && !is_blank_row(state, first - 1) {
        first -= 1;
    }
    while end < last && !is_blank_row(state, end + 1) {
        end += 1;
    }
    Some(state.text.line_start_offset(first)..state.text.line_end_offset(end))
}

fn is_sentence_end(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '…')
}

/// Closing quotes and brackets, which belong to the sentence they follow.
fn is_closing(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '»')
}

/// The sentences of `paragraph`, as `(end, next)`: where a sentence's
/// punctuation and closing quotes end, and where the next sentence starts,
/// past the whitespace. A sentence ends at a period, question mark or
/// exclamation mark followed by whitespace or the end of the paragraph.
fn sentences(state: &TextareaState, paragraph: &Range<usize>) -> Vec<(usize, usize)> {
    let text = state.text.slice(paragraph.clone()).to_string();
    let mut sentences = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_sentence_end(c) {
            continue;
        }
        let mut end = index + c.len_utf8();
        while let Some(&(index, c)) = chars
            .peek()
            .filter(|(_, c)| is_sentence_end(*c) || is_closing(*c))
        {
            end = index + c.len_utf8();
            chars.next();
        }
        let mut next = end;
        while let Some(&(index, c)) = chars.peek().filter(|(_, c)| c.is_whitespace()) {
            next = index + c.len_utf8();
            chars.next();
        }
        if next > end || end == text.len() {
            sentences.push((paragraph.start + end, paragraph.start + next));
        }
    }
    sentences
}

/// The end of the sentence at or after `offset`: past its punctuation, or
/// the end of the paragraph.
fn forward_sentence(state: &TextareaState, offset: usize) -> usize {
    let len = state.text.len();
    let Some(mut paragraph) = paragraph_text(state, offset, true) else {
        return len;
    };
    if paragraph.end <= offset {
        // At the end of a paragraph, the next one.
        match paragraph_text(state, (offset + 1).min(len), true) {
            Some(next) if next.end > offset => paragraph = next,
            _ => return len,
        }
    }
    sentences(state, &paragraph)
        .into_iter()
        .map(|(end, _)| end)
        .find(|end| *end > offset)
        .unwrap_or(paragraph.end)
}

/// The start of the sentence before `offset`, or of the paragraph's text.
fn backward_sentence(state: &TextareaState, offset: usize) -> usize {
    let Some(mut paragraph) = paragraph_text(state, offset, false) else {
        return 0;
    };
    let text_start = |paragraph: &Range<usize>| indentation(state, paragraph.start);
    if text_start(&paragraph) >= offset {
        // At the start of a paragraph, the one before.
        match paragraph
            .start
            .checked_sub(1)
            .and_then(|before| paragraph_text(state, before, false))
        {
            Some(previous) => paragraph = previous,
            None => return text_start(&paragraph).min(offset),
        }
    }
    sentences(state, &paragraph)
        .into_iter()
        .map(|(_, next)| next)
        .rfind(|next| *next < offset && *next < paragraph.end)
        .unwrap_or_else(|| text_start(&paragraph))
}
