//! Marks: ranges of a textarea's text that move with edits and draw an
//! underline.
//!
//! Spelling underlines are marks the textarea keeps itself. An application
//! adds its own, such as grammar hints or comments, through a
//! [`MarkCollection`], and reads back what is under a position with
//! [`TextareaState::marks_at`].

use std::{any::Any, fmt, ops::Range, rc::Rc};

use gpui::{App, Context, HighlightStyle, Hsla, UnderlineStyle, WeakEntity, px};

use super::{
    InputEditorStyle, TextareaState,
    decorations::{DecorationCollectionId, DecorationCollections, TrackedDecoration, normalize},
};

/// How a [`Mark`] underlines its text.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkStyle {
    /// A wavy underline in the theme's error colour, for a misspelled word.
    Spelling,
    /// A wavy underline in the theme's info colour, for a grammar or style
    /// hint.
    Grammar,
    /// An underline in `color`, or in the text colour when it is `None`.
    Underline { color: Option<Hsla>, wavy: bool },
}

impl MarkStyle {
    fn highlight(self, style: &InputEditorStyle) -> HighlightStyle {
        let (color, wavy) = match self {
            Self::Spelling => (style.diagnostics.error, true),
            Self::Grammar => (style.diagnostics.info, true),
            Self::Underline { color, wavy } => (color.unwrap_or(style.foreground), wavy),
        };
        HighlightStyle {
            underline: Some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy,
            }),
            ..Default::default()
        }
    }
}

/// A range of a textarea's text that moves with edits and draws an
/// underline.
///
/// Typing at either edge of a mark leaves it as it is, and typing inside it
/// stretches it. An edit that replaces all of its text removes it.
///
/// ```
/// use gpui_base::input::{Mark, MarkStyle};
///
/// let mark = Mark::new(4..9, MarkStyle::Grammar).with_data("passive voice");
/// assert_eq!(mark.range(), 4..9);
/// assert_eq!(mark.data::<&str>(), Some(&"passive voice"));
/// ```
#[derive(Clone)]
pub struct Mark {
    range: Range<usize>,
    style: MarkStyle,
    data: Option<Rc<dyn Any>>,
}

impl Mark {
    /// A mark over the byte range `range`.
    pub fn new(range: Range<usize>, style: MarkStyle) -> Self {
        Self {
            range,
            style,
            data: None,
        }
    }

    /// Application data, read back with [`Self::data`], for example the
    /// message a context menu item shows.
    pub fn with_data(mut self, data: impl Any) -> Self {
        self.data = Some(Rc::new(data));
        self
    }

    /// The byte range the mark covers now.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// How the mark is drawn.
    pub fn style(&self) -> MarkStyle {
        self.style
    }

    /// The data set with [`Self::with_data`], if it is a `T`.
    pub fn data<T: 'static>(&self) -> Option<&T> {
        self.data.as_ref()?.downcast_ref()
    }
}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mark")
            .field("range", &self.range)
            .field("style", &self.style)
            .field("data", &self.data.is_some())
            .finish()
    }
}

impl PartialEq for Mark {
    /// Equal when they cover the same text in the same style; application data
    /// is compared by identity.
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range
            && self.style == other.style
            && match (&self.data, &other.data) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl TrackedDecoration for Mark {
    const DROP_WHEN_REPLACED: bool = true;

    fn range(&self) -> &Range<usize> {
        &self.range
    }

    fn range_mut(&mut self) -> &mut Range<usize> {
        &mut self.range
    }
}

/// A set of marks an application manages, created with
/// [`TextareaState::create_mark_collection`].
///
/// Clones address the same set. The marks follow edits, so a collection is
/// set again only when what it marks changes. Operations after the textarea
/// is dropped do nothing.
#[derive(Clone, Debug)]
pub struct MarkCollection {
    state: WeakEntity<TextareaState>,
    id: DecorationCollectionId,
}

impl MarkCollection {
    /// Replace every mark in this collection.
    pub fn set(&self, marks: Vec<Mark>, cx: &mut App) {
        _ = self.state.update(cx, |state, cx| {
            let marks = normalize(&state.text, marks);
            if state.extras.marks.collections.set(self.id, marks) {
                cx.notify();
            }
        });
    }

    /// Replace the marks that overlap `range` with `marks`, for a check that
    /// covered only part of the text.
    pub fn splice(&self, range: Range<usize>, marks: Vec<Mark>, cx: &mut App) {
        _ = self.state.update(cx, |state, cx| {
            let marks = normalize(&state.text, marks);
            if state
                .extras
                .marks
                .splice(self.id, std::slice::from_ref(&range), marks)
            {
                cx.notify();
            }
        });
    }

    /// Remove every mark in this collection.
    pub fn clear(&self, cx: &mut App) {
        self.set(Vec::new(), cx);
    }

    /// The marks in this collection, at their current ranges.
    pub fn marks(&self, cx: &App) -> Vec<Mark> {
        self.state
            .read_with(cx, |state, _| {
                state
                    .extras
                    .marks
                    .collections
                    .get(self.id)
                    .map(<[Mark]>::to_vec)
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

/// Every mark a textarea holds, the spelling layer's among them.
#[derive(Default)]
pub(crate) struct Marks {
    collections: DecorationCollections<Mark>,
}

impl Marks {
    pub(super) fn create(&mut self) -> DecorationCollectionId {
        self.collections.create(Vec::new())
    }

    pub(super) fn get(&self, id: DecorationCollectionId) -> &[Mark] {
        self.collections.get(id).unwrap_or_default()
    }

    pub(super) fn set(&mut self, id: DecorationCollectionId, marks: Vec<Mark>) -> bool {
        self.collections.set(id, marks)
    }

    /// Replace the marks of collection `id` that overlap any of `ranges`.
    /// Returns whether anything changed.
    pub(super) fn splice(
        &mut self,
        id: DecorationCollectionId,
        ranges: &[Range<usize>],
        added: Vec<Mark>,
    ) -> bool {
        let Some(current) = self.collections.get(id) else {
            return false;
        };
        let overlaps = |mark: &Mark| {
            ranges.iter().any(|range| {
                mark.range.start < range.end && range.start < mark.range.end
                    || (range.is_empty() && mark.range.contains(&range.start))
            })
        };
        let kept: Vec<Mark> = current
            .iter()
            .filter(|mark| !overlaps(mark))
            .cloned()
            .collect();
        if kept.len() == current.len() && added.is_empty() {
            return false;
        }
        let mut marks = kept;
        marks.extend(added);
        marks.sort_by_key(|mark| (mark.range.start, mark.range.end));
        self.collections.set(id, marks)
    }

    pub(super) fn adjust_for_edit(&mut self, range: &Range<usize>, new_len: usize) {
        self.collections.adjust_for_edit(range, new_len);
    }

    /// The marks at `offset`: those containing it, or ending or starting
    /// there.
    pub(super) fn at(&self, offset: usize) -> Vec<&Mark> {
        self.collections
            .intersecting(&[offset.saturating_sub(1)..offset + 1])
            .into_iter()
            .filter(|mark| mark.range.start <= offset && offset <= mark.range.end)
            .collect()
    }

    /// The underlines to paint within `range`.
    pub(super) fn underlines(
        &self,
        range: &Range<usize>,
        style: &InputEditorStyle,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.collections
            .intersecting(std::slice::from_ref(range))
            .into_iter()
            .map(|mark| (mark.range.clone(), mark.style.highlight(style)))
            .collect()
    }
}

/// Methods for a textarea's marks. See [`Mark`].
impl TextareaState {
    /// Create a collection the application fills with its own marks.
    ///
    /// Collections paint in the order they were created, so where two
    /// overlap, the later one's underline shows. The textarea keeps them until
    /// it is dropped.
    pub fn create_mark_collection(&mut self, cx: &mut Context<Self>) -> MarkCollection {
        MarkCollection {
            state: cx.entity().downgrade(),
            id: self.extras.marks.create(),
        }
    }

    /// The marks at the byte `offset`, from every collection and the spelling
    /// underlines: those that contain it, start there or end there.
    pub fn marks_at(&self, offset: usize) -> Vec<Mark> {
        self.extras.marks.at(offset).into_iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(marks: &Marks, id: DecorationCollectionId) -> Vec<Range<usize>> {
        marks.get(id).iter().map(Mark::range).collect()
    }

    #[test]
    fn marks_move_with_edits() {
        let mut marks = Marks::default();
        let id = marks.create();
        marks.set(
            id,
            vec![
                Mark::new(4..7, MarkStyle::Spelling),
                Mark::new(12..15, MarkStyle::Spelling),
            ],
        );

        // Typing before a mark moves it, typing after leaves it.
        marks.adjust_for_edit(&(0..0), 2);
        assert_eq!(ranges(&marks, id), vec![6..9, 14..17]);
        marks.adjust_for_edit(&(20..20), 5);
        assert_eq!(ranges(&marks, id), vec![6..9, 14..17]);

        // Typing at an edge leaves the mark as it is, typing inside stretches it.
        marks.adjust_for_edit(&(9..9), 1);
        assert_eq!(ranges(&marks, id), vec![6..9, 15..18]);
        marks.adjust_for_edit(&(7..7), 1);
        assert_eq!(ranges(&marks, id), vec![6..10, 16..19]);

        // Deleting text before a mark pulls it back.
        marks.adjust_for_edit(&(0..3), 0);
        assert_eq!(ranges(&marks, id), vec![3..7, 13..16]);
    }

    #[test]
    fn replacing_all_of_a_mark_removes_it() {
        let mut marks = Marks::default();
        let id = marks.create();
        marks.set(
            id,
            vec![
                Mark::new(4..7, MarkStyle::Spelling),
                Mark::new(10..13, MarkStyle::Spelling),
            ],
        );

        // A replacement word goes in where the misspelled one was.
        marks.adjust_for_edit(&(4..7), 3);
        assert_eq!(ranges(&marks, id), vec![10..13]);

        // Replacing the whole text drops every mark rather than stretching one
        // over all of it.
        marks.adjust_for_edit(&(0..20), 30);
        assert!(ranges(&marks, id).is_empty());
    }

    #[test]
    fn a_replacement_overlapping_a_mark_clips_it() {
        let mut marks = Marks::default();
        let id = marks.create();
        marks.set(id, vec![Mark::new(4..10, MarkStyle::Grammar)]);
        marks.adjust_for_edit(&(8..12), 1);
        assert_eq!(ranges(&marks, id), vec![4..9]);
    }

    #[test]
    fn splice_replaces_only_overlapping_marks() {
        let mut marks = Marks::default();
        let id = marks.create();
        let other = marks.create();
        marks.set(
            id,
            vec![
                Mark::new(0..3, MarkStyle::Spelling),
                Mark::new(10..13, MarkStyle::Spelling),
                Mark::new(20..23, MarkStyle::Spelling),
            ],
        );
        marks.set(other, vec![Mark::new(10..13, MarkStyle::Grammar)]);

        assert!(marks.splice(id, &[8..16], vec![Mark::new(14..15, MarkStyle::Spelling)]));
        assert_eq!(ranges(&marks, id), vec![0..3, 14..15, 20..23]);
        assert_eq!(ranges(&marks, other), vec![10..13]);
        assert!(!marks.splice(id, &[30..40], Vec::new()));
    }

    #[test]
    fn marks_at_include_both_edges() {
        let mut marks = Marks::default();
        let id = marks.create();
        marks.set(
            id,
            vec![
                Mark::new(4..7, MarkStyle::Spelling),
                Mark::new(9..12, MarkStyle::Spelling),
            ],
        );
        let at = |offset| {
            marks
                .at(offset)
                .into_iter()
                .map(Mark::range)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(3), Vec::<Range<usize>>::new());
        assert_eq!(at(4), vec![4..7]);
        assert_eq!(at(5), vec![4..7]);
        assert_eq!(at(7), vec![4..7]);
        assert_eq!(at(8), Vec::<Range<usize>>::new());
        assert_eq!(at(12), vec![9..12]);
    }
}
