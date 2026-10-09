//! Editing commands any keybinding scheme can bind, beyond the engine's own.
//!
//! Each acts on every cursor. An edit is one step in the undo history.

use std::ops::Range;

use gpui::{Context, Div, Entity, InteractiveElement as _, Stateful, Window, actions};

use crate::input::{
    RopeExt as _, TextareaState, cursor::CursorSelection, movement::MoveDirection,
    undo_manager::EditIntent,
};

actions!(
    input,
    [
        /// Move to the blank line before the caret's paragraph, or the start
        /// of the text. Paragraphs are separated by blank lines.
        MoveToParagraphStart,
        /// Move to the blank line after the caret's paragraph, or the end of
        /// the text.
        MoveToParagraphEnd,
        /// Select to the blank line before the caret's paragraph.
        SelectToParagraphStart,
        /// Select to the blank line after the caret's paragraph.
        SelectToParagraphEnd,
        /// Move to the start of the next word, past the rest of this one and
        /// the space after it.
        MoveToNextWordStart,
        /// Select to the start of the next word.
        SelectToNextWordStart,
        /// Delete the lines the selection touches, with their line breaks.
        DeleteLine,
        /// Join the caret's line with the next, or the lines a selection
        /// touches, with one space where the line break and indentation were.
        JoinLines,
        /// Insert a line above the caret's line, indented like it, and move
        /// there.
        NewlineAbove,
        /// Insert a line below the caret's line, indented like it, and move
        /// there.
        NewlineBelow,
        /// Swap the characters on either side of the caret and move past
        /// them. At the end of a line, swap the two before the caret.
        TransposeCharacters,
        /// Upper-case the selection, or the rest of the word after the caret,
        /// moving past it.
        ConvertToUpperCase,
        /// Lower-case the selection, or the rest of the word after the caret.
        ConvertToLowerCase,
        /// Capitalize each word of the selection, or the word after the
        /// caret: its first letter upper-case, the rest lower-case.
        ConvertToTitleCase,
    ]
);

/// One edit of a batch, and where its cursor ends: a range relative to the
/// start of the replacement.
struct Edit {
    range: Range<usize>,
    text: String,
    select: Range<usize>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The characters' class for moving by word: 0 space, 1 word, 2 other.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if is_word_char(c) {
        1
    } else {
        2
    }
}

impl TextareaState {
    fn is_blank_row(&self, row: usize) -> bool {
        self.text.slice_line(row).chars().all(char::is_whitespace)
    }

    /// See [`MoveToParagraphStart`].
    fn paragraph_start_before(&self, offset: usize) -> usize {
        let mut row = self.text.offset_to_point(offset).row;
        while row > 0 && self.is_blank_row(row) {
            row -= 1;
        }
        while row > 0 && !self.is_blank_row(row) {
            row -= 1;
        }
        self.text.line_start_offset(row)
    }

    /// See [`MoveToParagraphEnd`].
    fn paragraph_end_after(&self, offset: usize) -> usize {
        let last = self.text.lines_len().saturating_sub(1);
        let mut row = self.text.offset_to_point(offset).row;
        while row < last && self.is_blank_row(row) {
            row += 1;
        }
        while row < last && !self.is_blank_row(row) {
            row += 1;
        }
        if self.is_blank_row(row) {
            self.text.line_start_offset(row)
        } else {
            self.text.len()
        }
    }

    /// See [`MoveToNextWordStart`].
    fn next_word_start_after(&self, offset: usize) -> usize {
        let mut chars = self.text.chars_at(offset).peekable();
        let mut offset = offset;
        if let Some(&first) = chars.peek() {
            let start_class = class(first);
            while let Some(&c) = chars.peek() {
                if start_class == 0 || class(c) != start_class {
                    break;
                }
                offset += c.len_utf8();
                chars.next();
            }
        }
        while let Some(&c) = chars.peek() {
            if !c.is_whitespace() {
                break;
            }
            offset += c.len_utf8();
            chars.next();
        }
        offset
    }

    /// The rows a selection touches: a selection that ends at the start of a
    /// line leaves that line out.
    fn selected_rows(&self, selection: &CursorSelection) -> Range<usize> {
        let start = self.text.offset_to_point(selection.start).row;
        let end = self.text.offset_to_point(selection.end);
        let end_row = if !selection.is_empty() && end.column == 0 && end.row > start {
            end.row - 1
        } else {
            end.row
        };
        start..end_row + 1
    }

    fn leading_whitespace(&self, row: usize) -> String {
        self.text
            .slice_line(row)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Apply `edits` as one undo step and place a cursor, or a selection, for
    /// each. Overlapping edits after the first are dropped.
    fn apply_edits(&mut self, mut edits: Vec<Edit>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_editable() || edits.is_empty() {
            return;
        }
        edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
        let mut kept: Vec<Edit> = Vec::with_capacity(edits.len());
        for edit in edits {
            if kept
                .last()
                .is_some_and(|last| edit.range.start < last.range.end || edit.range == last.range)
            {
                continue;
            }
            kept.push(edit);
        }

        let batch: Vec<(Range<usize>, String)> = kept
            .iter()
            .map(|edit| (edit.range.clone(), edit.text.clone()))
            .collect();
        self.undo_manager.break_transaction_coalescing();
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        self.replace_text_in_ranges(&batch, window, cx);
        self.undo_manager.break_transaction_coalescing();

        let mut delta = 0isize;
        let ranges: Vec<Range<usize>> = kept
            .iter()
            .map(|edit| {
                let start = (edit.range.start as isize + delta) as usize;
                delta += edit.text.len() as isize - edit.range.len() as isize;
                start + edit.select.start..start + edit.select.end
            })
            .collect();
        self.select_ranges(ranges, cx);
    }

    /// Replace the selections with `ranges`, in order, the first active.
    fn select_ranges(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        let Some(first) = ranges.first().cloned() else {
            return;
        };
        self.selections.remove_all_but_active();
        self.set_selection(first.start, first.end);
        for range in ranges.into_iter().skip(1) {
            let id = self.selections.generate_id();
            self.selections
                .add(CursorSelection::new(id, range.start, range.end));
        }
        self.selections.merge_overlapping();
        self.scroll_to(self.cursor(), None, cx);
        cx.notify();
    }

    fn move_to_paragraph_start(
        &mut self,
        _: &MoveToParagraphStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_all_cursors(
            |s, sel| (s.paragraph_start_before(sel.cursor_offset()), None, false),
            Some(MoveDirection::Up),
            window,
            cx,
        );
    }

    fn move_to_paragraph_end(
        &mut self,
        _: &MoveToParagraphEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_all_cursors(
            |s, sel| (s.paragraph_end_after(sel.cursor_offset()), None, false),
            Some(MoveDirection::Down),
            window,
            cx,
        );
    }

    fn select_to_paragraph_start(
        &mut self,
        _: &SelectToParagraphStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all_cursors_to(|s, sel| s.paragraph_start_before(sel.cursor_offset()), cx);
    }

    fn select_to_paragraph_end(
        &mut self,
        _: &SelectToParagraphEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all_cursors_to(|s, sel| s.paragraph_end_after(sel.cursor_offset()), cx);
    }

    fn move_to_next_word_start(
        &mut self,
        _: &MoveToNextWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_all_cursors(
            |s, sel| (s.next_word_start_after(sel.cursor_offset()), None, false),
            None,
            window,
            cx,
        );
    }

    fn select_to_next_word_start(
        &mut self,
        _: &SelectToNextWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all_cursors_to(|s, sel| s.next_word_start_after(sel.cursor_offset()), cx);
    }

    fn delete_line(&mut self, _: &DeleteLine, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.text.lines_len().saturating_sub(1);
        let edits = self
            .selections
            .iter()
            .map(|selection| {
                let rows = self.selected_rows(selection);
                let range = if rows.end <= last {
                    self.text.line_start_offset(rows.start)..self.text.line_start_offset(rows.end)
                } else if rows.start > 0 {
                    // The last line has no line break of its own: take the one
                    // before it.
                    self.text.line_end_offset(rows.start - 1)..self.text.len()
                } else {
                    0..self.text.len()
                };
                Edit {
                    range,
                    text: String::new(),
                    select: 0..0,
                }
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    fn join_lines(&mut self, _: &JoinLines, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.text.lines_len().saturating_sub(1);
        let edits = self
            .selections
            .iter()
            .filter_map(|selection| {
                let rows = self.selected_rows(selection);
                let rows = rows.start..rows.end.max(rows.start + 2).min(last + 1);
                if rows.len() < 2 {
                    return None;
                }
                let mut joined = self.text.slice_line(rows.start).to_string();
                let mut caret = joined.len();
                for row in rows.start + 1..rows.end {
                    let next = self.text.slice_line(row).to_string();
                    let next = next.trim_start_matches([' ', '\t']);
                    let trimmed_len = joined.trim_end_matches([' ', '\t']).len();
                    joined.truncate(trimmed_len);
                    caret = joined.len();
                    if !joined.is_empty() && !next.is_empty() {
                        joined.push(' ');
                    }
                    joined.push_str(next);
                }
                Some(Edit {
                    range: self.text.line_start_offset(rows.start)
                        ..self.text.line_end_offset(rows.end - 1),
                    text: joined,
                    select: caret..caret,
                })
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    fn newline_above(&mut self, _: &NewlineAbove, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self
            .selections
            .iter()
            .map(|selection| {
                let row = self.text.offset_to_point(selection.cursor_offset()).row;
                let indent = self.leading_whitespace(row);
                let at = self.text.line_start_offset(row);
                Edit {
                    range: at..at,
                    select: indent.len()..indent.len(),
                    text: indent + "\n",
                }
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    fn newline_below(&mut self, _: &NewlineBelow, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self
            .selections
            .iter()
            .map(|selection| {
                let row = self.text.offset_to_point(selection.cursor_offset()).row;
                let indent = self.leading_whitespace(row);
                let at = self.text.line_end_offset(row);
                let caret = 1 + indent.len();
                Edit {
                    range: at..at,
                    text: format!("\n{indent}"),
                    select: caret..caret,
                }
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    fn transpose_characters(
        &mut self,
        _: &TransposeCharacters,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edits = self
            .selections
            .iter()
            .filter(|selection| selection.is_empty())
            .filter_map(|selection| {
                let caret = selection.cursor_offset();
                let row = self.text.offset_to_point(caret).row;
                let line_start = self.text.line_start_offset(row);
                let line_end = self.text.line_end_offset(row);
                // At the end of a line, the two characters before the caret.
                let middle = if caret == line_end {
                    caret.checked_sub(self.text.chars_at(caret).reversed().next()?.len_utf8())?
                } else {
                    caret
                };
                let before = self.text.chars_at(middle).reversed().next()?;
                let after = self.text.chars_at(middle).next()?;
                let start = middle.checked_sub(before.len_utf8())?;
                if start < line_start || after == '\n' {
                    return None;
                }
                let end = middle + after.len_utf8();
                let text = format!("{after}{before}");
                Some(Edit {
                    range: start..end,
                    select: text.len()..text.len(),
                    text,
                })
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    /// Convert the selections, or the rest of the word after each cursor,
    /// with `convert`.
    fn convert_case(
        &mut self,
        convert: impl Fn(&str) -> String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edits = self
            .selections
            .iter()
            .filter_map(|selection| {
                let (range, keep_selection) = if selection.is_empty() {
                    let caret = selection.cursor_offset();
                    (caret..self.next_end_of_word_at(caret), false)
                } else {
                    (selection.start..selection.end, true)
                };
                if range.is_empty() {
                    return None;
                }
                let text = convert(&self.text.slice(range.clone()).to_string());
                let select = if keep_selection {
                    0..text.len()
                } else {
                    text.len()..text.len()
                };
                Some(Edit {
                    range,
                    text,
                    select,
                })
            })
            .collect();
        self.apply_edits(edits, window, cx);
    }

    fn convert_to_upper_case(
        &mut self,
        _: &ConvertToUpperCase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.convert_case(str::to_uppercase, window, cx);
    }

    fn convert_to_lower_case(
        &mut self,
        _: &ConvertToLowerCase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.convert_case(str::to_lowercase, window, cx);
    }

    fn convert_to_title_case(
        &mut self,
        _: &ConvertToTitleCase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.convert_case(title_case, window, cx);
    }
}

/// Each word's first letter upper-case and the rest lower-case. An
/// apostrophe inside a word keeps it one word: "don't" becomes "Don't".
fn title_case(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_word = false;
    for c in text.chars() {
        if is_word_char(c) {
            if in_word {
                result.extend(c.to_lowercase());
            } else {
                result.extend(c.to_uppercase());
            }
            in_word = true;
        } else {
            in_word = in_word && matches!(c, '\'' | '’');
            result.push(c);
        }
    }
    result
}

/// Registers the commands on a textarea's root element.
pub(super) fn register_actions(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
) -> Stateful<Div> {
    element
        .on_action(window.listener_for(entity, TextareaState::move_to_paragraph_start))
        .on_action(window.listener_for(entity, TextareaState::move_to_paragraph_end))
        .on_action(window.listener_for(entity, TextareaState::select_to_paragraph_start))
        .on_action(window.listener_for(entity, TextareaState::select_to_paragraph_end))
        .on_action(window.listener_for(entity, TextareaState::move_to_next_word_start))
        .on_action(window.listener_for(entity, TextareaState::select_to_next_word_start))
        .on_action(window.listener_for(entity, TextareaState::delete_line))
        .on_action(window.listener_for(entity, TextareaState::join_lines))
        .on_action(window.listener_for(entity, TextareaState::newline_above))
        .on_action(window.listener_for(entity, TextareaState::newline_below))
        .on_action(window.listener_for(entity, TextareaState::transpose_characters))
        .on_action(window.listener_for(entity, TextareaState::convert_to_upper_case))
        .on_action(window.listener_for(entity, TextareaState::convert_to_lower_case))
        .on_action(window.listener_for(entity, TextareaState::convert_to_title_case))
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::super::{Keymap, KeymapPlatform, test::KeymapTest};
    use super::*;

    #[test]
    fn title_case_capitalizes_each_word() {
        assert_eq!(title_case("hello wORLD"), "Hello World");
        assert_eq!(title_case("don't stop"), "Don't Stop");
        assert_eq!(title_case("  x-ray"), "  X-Ray");
    }

    fn editor(cx: &mut TestAppContext, marked: &str) -> KeymapTest {
        KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, marked)
    }

    #[gpui::test]
    fn paragraph_motions_stop_at_blank_lines(cx: &mut TestAppContext) {
        let mut test = editor(cx, "one\ntwo\n\nthrˇee\nfour\n\n\nfive");
        test.dispatch(MoveToParagraphStart);
        test.assert("one\ntwo\nˇ\nthree\nfour\n\n\nfive");
        test.dispatch(MoveToParagraphStart);
        test.assert("ˇone\ntwo\n\nthree\nfour\n\n\nfive");
        test.dispatch(MoveToParagraphEnd);
        test.assert("one\ntwo\nˇ\nthree\nfour\n\n\nfive");
        test.dispatch(MoveToParagraphEnd);
        test.assert("one\ntwo\n\nthree\nfour\nˇ\n\nfive");
        test.dispatch(MoveToParagraphEnd);
        test.assert("one\ntwo\n\nthree\nfour\n\n\nfiveˇ");
        test.dispatch(SelectToParagraphStart);
        test.assert("one\ntwo\n\nthree\nfour\n\n«ˇ\nfive»");
    }

    #[gpui::test]
    fn next_word_start_skips_the_word_and_its_space(cx: &mut TestAppContext) {
        let mut test = editor(cx, "ˇhello, world  again");
        test.dispatch(MoveToNextWordStart);
        test.assert("hello, world  again".replacen(',', "ˇ,", 1).as_str());
        test.dispatch(MoveToNextWordStart);
        test.assert("hello, ˇworld  again");
        test.dispatch(MoveToNextWordStart);
        test.assert("hello, world  ˇagain");
        test.dispatch(SelectToNextWordStart);
        test.assert("hello, world  «again»");
    }

    #[gpui::test]
    fn line_commands_delete_join_and_open_lines(cx: &mut TestAppContext) {
        let mut test = editor(cx, "one\n  twˇo\nthree");
        test.dispatch(NewlineBelow);
        test.assert("one\n  two\n  ˇ\nthree");
        test.dispatch(DeleteLine);
        test.assert("one\n  two\nˇthree");
        test.dispatch(NewlineAbove);
        test.assert("one\n  two\nˇ\nthree");
        test.type_text("x");
        test.dispatch(JoinLines);
        test.assert("one\n  two\nxˇ three");
        test.dispatch(DeleteLine);
        test.assert("one\n  twoˇ");
        test.update(|state, _, cx| state.set_selected_range(0..5, cx));
        test.dispatch(JoinLines);
        test.assert("oneˇ two");
        test.keys("ctrl-z");
        test.assert("one\n  two");
    }

    #[gpui::test]
    fn transpose_swaps_around_the_caret(cx: &mut TestAppContext) {
        let mut test = editor(cx, "abˇcd\nef");
        test.dispatch(TransposeCharacters);
        test.assert("acbˇd\nef");
        test.dispatch(TransposeCharacters);
        test.assert("acdbˇ\nef");
        // Nothing before the caret on its line.
        test.update(|state, _, cx| state.set_selected_range(5..5, cx));
        test.dispatch(TransposeCharacters);
        test.assert("acdb\nˇef");
    }

    #[gpui::test]
    fn case_commands_convert_the_selection_or_the_next_word(cx: &mut TestAppContext) {
        let mut test = editor(cx, "ˇhello wide world");
        test.dispatch(ConvertToUpperCase);
        test.assert("HELLOˇ wide world");
        test.dispatch(ConvertToTitleCase);
        test.assert("HELLO Wideˇ world");
        test.update(|state, _, cx| state.set_selected_range(0..5, cx));
        test.dispatch(ConvertToLowerCase);
        test.assert("«helloˇ» Wide world");
    }
}
