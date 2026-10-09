//! The commands CUA binds beyond the engine's own, for keys whose meaning
//! differs between the platforms: Windows's word stops, macOS's Fn keys that
//! scroll without moving the caret, its kill buffer, and moving by line.
//!
//! Every input follows CUA, so these work in single-line inputs and editors
//! as well as textareas. Each acts on every cursor, and an edit is one step in
//! the undo history.

use std::ops::Range;

use gpui::{
    Context, Div, Entity, EntityId, Global, InteractiveElement as _, Stateful, Window, actions,
    point, px,
};
use sum_tree::Bias;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::input::{
    BufferPoint, InputBaseState, InputModeKind, RopeExt as _, movement::MoveDirection,
    smooth_caret::CaretMotion, undo_manager::EditIntent,
};

actions!(
    cua,
    [
        /// Move to the start of the word before the caret. A line's start and
        /// end are stops too, so from the start of a line this goes to the
        /// end of the one above. Ctrl-Left on Windows.
        MoveToWordStartLeft,
        /// Move to the start of the next word, or to the end of the line when
        /// no word follows on it. Ctrl-Right on Windows.
        MoveToWordStartRight,
        /// Select to where [`MoveToWordStartLeft`] goes.
        SelectToWordStartLeft,
        /// Select to where [`MoveToWordStartRight`] goes.
        SelectToWordStartRight,
        /// Delete to where [`MoveToWordStartLeft`] goes.
        DeleteToWordStartLeft,
        /// Delete to where [`MoveToWordStartRight`] goes: the rest of the
        /// word and the space after it.
        DeleteToWordStartRight,
        /// Move to the start of the caret's line, or of the line above when
        /// the caret is at a line's start already.
        MoveBackwardToLineStart,
        /// Move to the end of the caret's line, or of the line below when the
        /// caret is at a line's end already.
        MoveForwardToLineEnd,
        /// Move to the start of the next line, or the end of the text.
        MoveForwardToLineStart,
        /// Select to where [`MoveBackwardToLineStart`] goes.
        SelectBackwardToLineStart,
        /// Select to where [`MoveForwardToLineEnd`] goes.
        SelectForwardToLineEnd,
        /// Select to where [`MoveForwardToLineStart`] goes.
        SelectForwardToLineStart,
        /// Extend the selection a page up.
        SelectPageUp,
        /// Extend the selection a page down.
        SelectPageDown,
        /// Scroll to the start of the text, leaving the caret where it is.
        ScrollToStart,
        /// Scroll to the end of the text, leaving the caret where it is.
        ScrollToEnd,
        /// Scroll a page up, leaving the caret where it is.
        ScrollPageUp,
        /// Scroll a page down, leaving the caret where it is.
        ScrollPageDown,
        /// Scroll the caret's line to the middle of the view.
        ScrollCaretToCenter,
        /// Delete to the end of the line, or the line break at its end, into
        /// the kill buffer. The kill buffer is the app's, apart from the
        /// clipboard, and kills in a row add to it.
        KillToEndOfLine,
        /// Insert the kill buffer's text.
        Yank,
        /// Insert a line break after the caret, which stays where it is.
        OpenLine,
    ]
);

/// What macOS's Ctrl-K cut, for Ctrl-Y to put back: one for the app, apart
/// from the clipboard, as Cocoa keeps it.
#[derive(Default)]
struct KillBuffer {
    text: String,
    /// Where the last kill left the caret: the input, the revision of its
    /// text and the offset. A kill from there adds to the text.
    end: Option<(EntityId, u64, usize)>,
}

impl Global for KillBuffer {}

/// Whether a word-boundary segment is a word: anything but white space,
/// punctuation included.
fn is_word(segment: &str) -> bool {
    !segment.trim_start().is_empty()
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Where Windows stops when moving by word on `row`, in order: the line's
    /// start, the start of each word and the line's end, before a CRLF's
    /// `\r`. Words are split as the engine's other word commands split them.
    fn word_stops(&self, row: usize) -> Vec<usize> {
        let start = self.text.line_start_offset(row);
        let line = self.text.slice_line(row).to_string();
        let line = line.trim_end_matches('\r');
        let mut stops = vec![start];
        stops.extend(
            line.split_word_bound_indices()
                .filter(|(_, segment)| is_word(segment))
                .map(|(i, _)| start + i),
        );
        stops.push(start + line.len());
        stops
    }

    /// See [`MoveToWordStartRight`].
    fn word_start_right(&self, offset: usize) -> usize {
        if self.masked {
            return self.text.len();
        }
        let rows = self.text.lines_len();
        let mut row = self.text.offset_to_point(offset).row;
        while row < rows {
            if let Some(stop) = self.word_stops(row).into_iter().find(|stop| *stop > offset) {
                return self.token_boundary(stop, Bias::Right);
            }
            row += 1;
        }
        self.text.len()
    }

    /// See [`MoveToWordStartLeft`].
    fn word_start_left(&self, offset: usize) -> usize {
        if self.masked {
            return 0;
        }
        let mut row = self.text.offset_to_point(offset).row;
        loop {
            let stops = self.word_stops(row);
            if let Some(stop) = stops.into_iter().rev().find(|stop| *stop < offset) {
                return self.token_boundary(stop, Bias::Left);
            }
            if row == 0 {
                return 0;
            }
            row -= 1;
        }
    }

    /// See [`MoveBackwardToLineStart`].
    fn line_start_before(&self, offset: usize) -> usize {
        if offset == 0 {
            return 0;
        }
        let row = self
            .text
            .offset_to_point(self.previous_boundary(offset))
            .row;
        self.text.line_start_offset(row)
    }

    /// See [`MoveForwardToLineEnd`].
    fn line_end_after(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        let row = self.text.offset_to_point(self.next_boundary(offset)).row;
        self.text.line_end_offset(row)
    }

    /// See [`MoveForwardToLineStart`].
    fn line_start_after(&self, offset: usize) -> usize {
        let row = self.text.offset_to_point(offset).row;
        if row + 1 < self.text.lines_len() {
            self.text.line_start_offset(row + 1)
        } else {
            self.text.len()
        }
    }

    /// What [`KillToEndOfLine`] deletes from a caret at `offset`.
    fn kill_range(&self, offset: usize) -> Range<usize> {
        let end = self.end_of_line_at(offset, self.line_end_affinity_at(offset));
        if end == offset {
            offset..self.next_boundary(offset)
        } else {
            offset..end
        }
    }

    /// Move every cursor to `target` of its caret.
    fn move_each_to(
        &mut self,
        target: impl Fn(&Self, usize) -> usize,
        direction: Option<MoveDirection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_all_cursors(
            |s, sel| {
                let offset = target(s, sel.cursor_offset());
                (offset, s.preferred_column_for(offset), false)
            },
            direction,
            window,
            cx,
        );
    }

    /// Extend every selection to `target` of its caret.
    fn select_each_to(&mut self, target: impl Fn(&Self, usize) -> usize, cx: &mut Context<Self>) {
        self.select_all_cursors_to(|s, sel| target(s, sel.cursor_offset()), cx);
    }

    /// Delete from every caret to `target` of it, or the selections.
    fn delete_each_to(
        &mut self,
        target: impl Fn(&Self, usize) -> usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_selections(
            true,
            EditIntent::Atomic,
            |s, offset| {
                let target = target(s, offset);
                target.min(offset)..target.max(offset)
            },
            window,
            cx,
        );
    }

    /// The rows a page holds, for a laid-out multi-line input.
    fn rows_per_page(&self) -> Option<isize> {
        if self.is_single_line() {
            return None;
        }
        let layout = self.last_layout.as_ref()?;
        Some((self.input_bounds.size.height / layout.line_height) as isize)
    }

    fn select_page(&mut self, pages: isize, cx: &mut Context<Self>) {
        let Some(rows) = self.rows_per_page() else {
            cx.propagate();
            return;
        };
        self.select_all_cursors_to_with_affinity(
            |s, sel| {
                s.vertical_selection_target(
                    sel.cursor_offset(),
                    sel.column_anchor,
                    s.line_end_affinity_for(sel),
                    rows * pages,
                )
            },
            true,
            cx,
        );
    }

    /// Scroll by `pages`, up when negative, keeping a line of the last page
    /// in view.
    fn scroll_pages(&mut self, pages: f32, cx: &mut Context<Self>) {
        let Some(line_height) = self
            .last_layout
            .as_ref()
            .filter(|_| self.is_multi_line())
            .map(|layout| layout.line_height)
        else {
            cx.propagate();
            return;
        };
        let page = (self.input_bounds.size.height - line_height).max(line_height);
        let offset = self.scroll_handle.offset();
        self.update_scroll_offset(Some(point(offset.x, offset.y - page * pages)), cx);
    }

    fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        let target = if self.is_single_line() {
            point(-self.scroll_size.width, px(0.))
        } else {
            point(px(0.), -self.scroll_size.height)
        };
        self.update_scroll_offset(Some(target), cx);
    }

    fn scroll_caret_to_center(&mut self, cx: &mut Context<Self>) {
        let Some(line_height) = self
            .last_layout
            .as_ref()
            .filter(|_| self.is_multi_line())
            .map(|layout| layout.line_height)
        else {
            cx.propagate();
            return;
        };
        let caret = self.text.offset_to_point(self.cursor());
        let row = self
            .display_map
            .buffer_pos_to_display_pos(BufferPoint::new(caret.row, caret.column))
            .row;
        let top = (self.input_bounds.size.height - line_height) / 2. - line_height * row;
        let x = self.scroll_handle.offset().x;
        self.update_scroll_offset(Some(point(x, top)), cx);
    }

    fn kill_to_end_of_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut ranges: Vec<Range<usize>> = self
            .selections
            .iter()
            .map(|sel| {
                if sel.is_empty() {
                    self.kill_range(sel.cursor_offset())
                } else {
                    sel.start..sel.end
                }
            })
            .collect();
        ranges.sort_by_key(|range| range.start);
        let killed = ranges
            .into_iter()
            .map(|range| self.text.slice(range).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        if killed.is_empty() {
            return;
        }

        let input = cx.entity_id();
        let follows = cx.try_global::<KillBuffer>().and_then(|buffer| buffer.end)
            == Some((input, self.document_revision, self.cursor()));
        self.delete_selections(
            true,
            EditIntent::Atomic,
            |s, offset| s.kill_range(offset),
            window,
            cx,
        );
        let end = Some((input, self.document_revision, self.cursor()));
        let buffer = cx.default_global::<KillBuffer>();
        if follows {
            buffer.text.push_str(&killed);
        } else {
            buffer.text = killed;
        }
        buffer.end = end;
    }

    fn yank(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut text) = cx
            .try_global::<KillBuffer>()
            .map(|buffer| buffer.text.clone())
            .filter(|text| !text.is_empty())
        else {
            return;
        };
        if self.is_single_line() {
            text = text.replace(['\r', '\n'], "");
        }
        self.undo_manager.break_transaction_coalescing();
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        self.replace_text_in_range_silent(None, &text, window, cx);
        self.scroll_to(self.cursor(), None, cx);
    }

    fn open_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.undo_manager.break_transaction_coalescing();
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        self.replace_text_in_range_silent(None, "\n", window, cx);
        self.move_each_to(|s, offset| s.previous_boundary(offset), None, window, cx);
    }
}

/// Registers CUA's commands on an input's root element. The engine calls it
/// for every input, since every input follows CUA.
pub(crate) fn register_actions<M: InputModeKind>(
    element: Stateful<Div>,
    entity: &Entity<InputBaseState<M>>,
    window: &mut Window,
) -> Stateful<Div> {
    use MoveDirection::{Down, Up};

    /// Edits pass the key on in an input that can't be edited, as the
    /// engine's own do.
    fn editable<M: InputModeKind>(
        state: &InputBaseState<M>,
        cx: &mut Context<InputBaseState<M>>,
    ) -> bool {
        let editable = state.is_editable();
        if !editable {
            cx.propagate();
        }
        editable
    }

    /// Moving by line passes the key on in a single-line input, as Up and
    /// Down do.
    fn multi_line<M: InputModeKind>(
        state: &InputBaseState<M>,
        cx: &mut Context<InputBaseState<M>>,
    ) -> bool {
        let multi_line = state.is_multi_line();
        if !multi_line {
            cx.propagate();
        }
        multi_line
    }

    element
        .on_action(
            window.listener_for(entity, |state, _: &MoveToWordStartLeft, window, cx| {
                state.caret_motion(CaretMotion::Word, |state| {
                    state.move_each_to(InputBaseState::word_start_left, None, window, cx)
                })
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &MoveToWordStartRight, window, cx| {
                state.caret_motion(CaretMotion::Word, |state| {
                    state.move_each_to(InputBaseState::word_start_right, None, window, cx)
                })
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectToWordStartLeft, _, cx| {
                state.select_each_to(InputBaseState::word_start_left, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectToWordStartRight, _, cx| {
                state.select_each_to(InputBaseState::word_start_right, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &DeleteToWordStartLeft, window, cx| {
                if editable(state, cx) {
                    state.delete_each_to(InputBaseState::word_start_left, window, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &DeleteToWordStartRight, window, cx| {
                if editable(state, cx) {
                    state.delete_each_to(InputBaseState::word_start_right, window, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &MoveBackwardToLineStart, window, cx| {
                if multi_line(state, cx) {
                    state.move_each_to(InputBaseState::line_start_before, Some(Up), window, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &MoveForwardToLineEnd, window, cx| {
                if multi_line(state, cx) {
                    state.move_each_to(InputBaseState::line_end_after, Some(Down), window, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &MoveForwardToLineStart, window, cx| {
                if multi_line(state, cx) {
                    state.move_each_to(InputBaseState::line_start_after, Some(Down), window, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectBackwardToLineStart, _, cx| {
                if multi_line(state, cx) {
                    state.select_each_to(InputBaseState::line_start_before, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectForwardToLineEnd, _, cx| {
                if multi_line(state, cx) {
                    state.select_each_to(InputBaseState::line_end_after, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectForwardToLineStart, _, cx| {
                if multi_line(state, cx) {
                    state.select_each_to(InputBaseState::line_start_after, cx)
                }
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectPageUp, _, cx| {
                state.select_page(-1, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SelectPageDown, _, cx| {
                state.select_page(1, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &ScrollToStart, _, cx| {
                state.update_scroll_offset(Some(point(px(0.), px(0.))), cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &ScrollToEnd, _, cx| {
                state.scroll_to_end(cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &ScrollPageUp, _, cx| {
                state.scroll_pages(-1., cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &ScrollPageDown, _, cx| {
                state.scroll_pages(1., cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &ScrollCaretToCenter, _, cx| {
                state.scroll_caret_to_center(cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &KillToEndOfLine, window, cx| {
                if editable(state, cx) {
                    state.kill_to_end_of_line(window, cx)
                }
            }),
        )
        .on_action(window.listener_for(entity, |state, _: &Yank, window, cx| {
            if editable(state, cx) {
                state.yank(window, cx)
            }
        }))
        .on_action(
            window.listener_for(entity, |state, _: &OpenLine, window, cx| {
                if editable(state, cx) && multi_line(state, cx) {
                    state.open_line(window, cx)
                }
            }),
        )
}
