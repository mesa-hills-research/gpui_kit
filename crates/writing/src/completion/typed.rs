//! Which text in a textarea the user typed.
//!
//! The persistent vocabulary learns only from what the user wrote. Every
//! change to the text arrives through `did_change`, each edit with its
//! origin. An inserted range is *untyped* unless the user typed it, took it
//! from a suggestion with Tab, or chose it as a spelling correction: a paste,
//! an undo or redo, and any text the program set are untyped, whatever their
//! length. These ranges move with later edits, and a sentence that overlaps
//! one is never learned.
//!
//! Insertions without a letter, such as Enter or an indent, can't add words
//! and are left out.

use gpui_base::input::{EditOrigin, TextChange};
use std::ops::Range;

#[derive(Default)]
pub(super) struct TypedText {
    /// Byte ranges of untyped insertions in the current text, sorted and
    /// apart.
    untyped: Vec<Range<usize>>,
}

impl TypedText {
    /// Follows `change`, edit by edit. Unless `learning`, as while
    /// suggestions are off, even what the user writes is untyped, so it is
    /// never learned.
    pub fn changed(&mut self, change: &TextChange, learning: bool) {
        for edit in change.edits() {
            let origin = if learning {
                edit.origin()
            } else {
                EditOrigin::Program
            };
            self.edit(edit.range(), edit.text(), origin);
        }
    }

    fn edit(&mut self, range: Range<usize>, inserted: &str, origin: EditOrigin) {
        let written = matches!(
            origin,
            EditOrigin::Typed | EditOrigin::Suggestion | EditOrigin::Correction
        );
        self.replace(range.clone(), inserted.len());
        if !written && inserted.chars().any(char::is_alphabetic) {
            self.insert(range.start..range.start + inserted.len());
        }
    }

    /// Moves the untyped ranges for `range` replaced with `new_len` bytes,
    /// dropping what it removed.
    fn replace(&mut self, range: Range<usize>, new_len: usize) {
        let shift = |offset: usize| offset - range.len() + new_len;
        let mut moved: Vec<Range<usize>> = Vec::with_capacity(self.untyped.len() + 1);
        let mut push = |piece: Range<usize>| match moved.last_mut() {
            // A deletion inside a range leaves its two ends touching.
            Some(last) if last.end >= piece.start => last.end = last.end.max(piece.end),
            _ => moved.push(piece),
        };
        for untyped in std::mem::take(&mut self.untyped) {
            if untyped.end <= range.start {
                push(untyped);
            } else if untyped.start >= range.end {
                push(shift(untyped.start)..shift(untyped.end));
            } else {
                if untyped.start < range.start {
                    push(untyped.start..range.start);
                }
                if untyped.end > range.end {
                    push(shift(range.end)..shift(untyped.end));
                }
            }
        }
        self.untyped = moved;
    }

    fn insert(&mut self, range: Range<usize>) {
        let index = self
            .untyped
            .partition_point(|untyped| untyped.end < range.start);
        let mut merged = range;
        while index < self.untyped.len() && self.untyped[index].start <= merged.end {
            let next = self.untyped.remove(index);
            merged = merged.start.min(next.start)..merged.end.max(next.end);
        }
        self.untyped.insert(index, merged);
    }

    /// Whether any of `range` was inserted without typing.
    pub fn overlaps_untyped(&self, range: Range<usize>) -> bool {
        let index = self
            .untyped
            .partition_point(|untyped| untyped.end <= range.start);
        self.untyped
            .get(index)
            .is_some_and(|untyped| untyped.start < range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn untyped(typed: &TypedText) -> Vec<(usize, usize)> {
        typed
            .untyped
            .iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    use EditOrigin::{Correction, History, Pasted, Program, Suggestion, Typed};

    #[test]
    fn pasted_ranges_move_with_later_edits() {
        let mut typed = TypedText::default();
        typed.edit(0..0, "Hello world", Pasted);
        assert_eq!(untyped(&typed), [(0, 11)]);

        // Typed text before it moves it, typed text inside splits it.
        typed.edit(0..0, "A", Typed);
        assert_eq!(untyped(&typed), [(1, 12)]);
        typed.edit(6..6, " ", Typed);
        assert_eq!(untyped(&typed), [(1, 6), (7, 13)]);

        // Deleting part of it shortens it, deleting all of it drops it.
        typed.edit(8..10, "", Typed);
        assert_eq!(untyped(&typed), [(1, 6), (7, 11)]);
        typed.edit(0..20, "", Typed);
        assert_eq!(untyped(&typed), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn neighbouring_pastes_merge() {
        let mut typed = TypedText::default();
        typed.edit(0..0, "one two", Pasted);
        typed.edit(7..7, " three", History);
        typed.edit(20..20, "four", Program);
        assert_eq!(untyped(&typed), [(0, 13), (20, 24)]);
        assert!(typed.overlaps_untyped(12..15));
        assert!(!typed.overlaps_untyped(13..20));
        assert!(typed.overlaps_untyped(15..21));
        assert!(!typed.overlaps_untyped(24..30));
    }

    /// Typing, a taken suggestion and a chosen correction are the user's
    /// writing; a one-word paste over a selection is not, though it looks
    /// like a correction.
    #[test]
    fn what_the_user_wrote_counts_as_typed() {
        let none = Vec::<(usize, usize)>::new();
        let mut typed = TypedText::default();
        typed.edit(0..0, "()", Typed);
        typed.edit(1..1, "llo world", Suggestion);
        typed.edit(0..3, "Hullo", Correction);
        assert_eq!(untyped(&typed), none);
        typed.edit(13..13, "\n    ", Program);
        assert_eq!(untyped(&typed), none, "no letters");
        typed.edit(6..11, "secret", Pasted);
        assert_eq!(untyped(&typed), [(6, 12)], "a one-word paste over a word");
    }
}
