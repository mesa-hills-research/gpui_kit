//! Spell checking for ordinary text.
//!
//! A [`TextareaState`] asks an application's [`SpellChecker`] which words are
//! misspelled and underlines them. The checker owns the dictionary, what
//! counts as a word and how suggestions are ranked. The textarea owns when to
//! check, the underlines, and the fixes the context menu offers.
//!
//! # When the text is checked
//!
//! All of the text is checked once the textarea first renders with a checker.
//! After that, an edit marks its lines, and once typing pauses for
//! [`SpellChecker::debounce`] only those lines are checked again. The
//! underlines are marks, so between checks they move with the text.
//!
//! A misspelling that ends at the caret right after typing is held back until
//! the caret leaves the word: a word still being typed is not marked.
//!
//! # Fixes
//!
//! The context menu of a misspelled word lists the checker's suggestions,
//! **Add to Dictionary** and **Ignore**. Each is an action carrying what it
//! does ([`ReplaceMisspelling`], [`AddToDictionary`], [`IgnoreMisspelling`]),
//! dispatched to the textarea, which applies it and reports a [`SpellEvent`].

use std::{
    collections::HashSet,
    fmt,
    ops::Range,
    rc::Rc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::Result;
use futures::FutureExt as _;
use gpui::{Action, App, Context, EventEmitter, Global, SharedString, Subscription, Task, Window};
use ropey::Rope;

use super::{
    InputBaseState, RopeExt as _, TextareaMode, TextareaState,
    decorations::{DecorationCollectionId, adjust_range_for_edit},
    marks::{Mark, MarkStyle, Marks},
    undo_manager::EditIntent,
};

/// Replace a misspelled word, as choosing a suggestion in its context menu
/// does.
///
/// The replacement is one step in the undo history. It applies only while the
/// text is still at `revision`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = input, no_json)]
pub struct ReplaceMisspelling {
    /// The misspelled word's byte range.
    pub range: Range<usize>,
    /// What replaces it.
    pub text: SharedString,
    /// The text revision `range` refers to, from [`TextareaState::revision`].
    pub revision: u64,
}

/// Add a word to the spell checker's dictionary, as **Add to Dictionary**
/// does.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = input, no_json)]
pub struct AddToDictionary {
    pub word: SharedString,
}

/// Stop marking a word in this textarea, as **Ignore** does.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = input, no_json)]
pub struct IgnoreMisspelling {
    pub word: SharedString,
}

/// Checks the spelling of a [`TextareaState`]'s text.
///
/// Every method is called on the main thread while the textarea is being
/// updated, so a checker must not read or update the textarea's entity: what
/// it needs arrives as arguments.
pub trait SpellChecker {
    /// Finds the misspelled words in the request's ranges.
    ///
    /// The ranges are whole lines: all of them the first time, then the lines
    /// edited since the last check. A task that is ready when returned is
    /// applied at once. A pending one is applied when it finishes, with its
    /// ranges moved along with any edits made meanwhile. An error is logged
    /// and leaves the underlines as they were.
    fn check(&self, request: &SpellCheckRequest, cx: &mut App) -> Task<Result<SpellCheck>>;

    /// Replacements for `word`, best first. The context menu lists the first
    /// five.
    fn suggestions(&self, word: &str, cx: &mut App) -> Vec<SharedString>;

    /// Accept `word` from now on, for example by adding it to the user's
    /// dictionary file.
    fn add_to_dictionary(&self, word: &str, cx: &mut App);

    /// The user chose to ignore `word`.
    ///
    /// The textarea stops marking the word by itself. A checker that shares
    /// the choice with other documents records it here. The default does
    /// nothing.
    fn ignore(&self, word: &str, cx: &mut App) {
        _ = (word, cx);
    }

    /// How long typing must pause before edited lines are checked again.
    ///
    /// Defaults to 300 milliseconds.
    fn debounce(&self) -> Duration {
        Duration::from_millis(300)
    }
}

/// What a [`SpellChecker`] is asked to check.
#[derive(Clone)]
pub struct SpellCheckRequest {
    text: Rope,
    ranges: Vec<Range<usize>>,
    revision: u64,
}

impl SpellCheckRequest {
    /// The text as it stands at [`Self::revision`].
    ///
    /// A rope shares its storage, so holding on to it costs nothing. Read the
    /// requested ranges of it rather than converting all of it to a string.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// The byte ranges to check: runs of whole lines, in order and apart from
    /// each other.
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    /// The text revision the request was made at, from
    /// [`TextareaState::revision`].
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl fmt::Debug for SpellCheckRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpellCheckRequest")
            .field("ranges", &self.ranges)
            .field("revision", &self.revision)
            .finish()
    }
}

/// What a [`SpellChecker`] found.
///
/// ```
/// use gpui_base::input::SpellCheck;
///
/// // "teh" at bytes 4..7 is misspelled, in the ranges that were asked for.
/// let check = SpellCheck {
///     misspelled: vec![4..7],
///     ..Default::default()
/// };
/// # _ = check;
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpellCheck {
    /// The byte ranges of the request's text the checker looked at. The
    /// spelling underlines in them are replaced by [`Self::misspelled`].
    ///
    /// Empty means the request's ranges. A checker that needs more context
    /// than it was given, such as all of a fenced code block, may cover more.
    pub checked: Vec<Range<usize>>,
    /// The misspelled words, as byte ranges of the request's text. Ranges
    /// outside the checked ones are ignored.
    pub misspelled: Vec<Range<usize>>,
}

/// A misspelled word in a textarea, as its context menu offers it.
#[derive(Clone)]
pub struct Misspelling {
    range: Range<usize>,
    word: SharedString,
    revision: u64,
    checker: Rc<dyn SpellChecker>,
}

impl Misspelling {
    /// The word's byte range.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The misspelled word.
    pub fn word(&self) -> &SharedString {
        &self.word
    }

    /// The text revision [`Self::range`] refers to.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The checker's replacements for the word, best first.
    pub fn suggestions(&self, cx: &mut App) -> Vec<SharedString> {
        self.checker.suggestions(&self.word, cx)
    }

    /// The action that replaces the word with `text`.
    pub fn replace_with(&self, text: impl Into<SharedString>) -> ReplaceMisspelling {
        ReplaceMisspelling {
            range: self.range.clone(),
            text: text.into(),
            revision: self.revision,
        }
    }

    /// The action that adds the word to the dictionary.
    pub fn add_to_dictionary(&self) -> AddToDictionary {
        AddToDictionary {
            word: self.word.clone(),
        }
    }

    /// The action that ignores the word in this textarea.
    pub fn ignore(&self) -> IgnoreMisspelling {
        IgnoreMisspelling {
            word: self.word.clone(),
        }
    }
}

impl fmt::Debug for Misspelling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Misspelling")
            .field("range", &self.range)
            .field("word", &self.word)
            .field("revision", &self.revision)
            .finish()
    }
}

/// What the user did about a misspelled word.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpellEvent {
    /// `word` was replaced with `with`.
    Replaced {
        word: SharedString,
        with: SharedString,
    },
    /// `word` was added to the dictionary.
    AddedToDictionary { word: SharedString },
    /// `word` is no longer marked in this textarea.
    Ignored { word: SharedString },
}

impl EventEmitter<SpellEvent> for InputBaseState<TextareaMode> {}

/// Counts dictionary changes. Every textarea with a spell checker observes
/// it and checks all of its text again when it changes.
#[derive(Default)]
struct SpellingRevision(u64);

impl Global for SpellingRevision {}

/// Check the text of every textarea with a spell checker again, after the
/// dictionary changed outside them, for example in a settings page.
///
/// **Add to Dictionary** and **Ignore** in any textarea do this by themselves.
pub fn refresh_spelling(cx: &mut App) {
    cx.default_global::<SpellingRevision>().0 += 1;
}

/// A textarea's spell checking: the checker, the lines waiting for a check
/// and the check in flight.
pub(crate) struct Spelling {
    checker: Option<Rc<dyn SpellChecker>>,
    enabled: bool,
    /// The marks collection the underlines live in, created on first use.
    layer: Option<DecorationCollectionId>,
    /// Whether all of the text waits for a check: nothing was checked yet, or
    /// the dictionary changed.
    full: bool,
    /// Byte ranges edited since they were last checked, in the current text.
    dirty: Vec<Range<usize>>,
    /// A check is scheduled to run.
    scheduled: bool,
    timer: Task<()>,
    /// Edits made since the check in flight was requested, to move its answer
    /// along with them. `None` while nothing is in flight.
    in_flight: Option<Vec<(Range<usize>, usize)>>,
    check: Task<()>,
    /// Lines were edited while a check was in flight.
    rerun: bool,
    /// Words the user chose to ignore here.
    ignored: HashSet<String>,
    /// A misspelling held back because its word is still being typed.
    held: Option<Range<usize>>,
    /// Where the last edit ended, while the caret has not moved from there.
    typed_to: Option<usize>,
    observing: Option<Subscription>,
}

impl Default for Spelling {
    fn default() -> Self {
        Self {
            checker: None,
            enabled: true,
            layer: None,
            full: true,
            dirty: Vec::new(),
            scheduled: false,
            timer: Task::ready(()),
            in_flight: None,
            check: Task::ready(()),
            rerun: false,
            ignored: HashSet::new(),
            held: None,
            typed_to: None,
            observing: None,
        }
    }
}

impl Spelling {
    fn active_checker(&self) -> Option<Rc<dyn SpellChecker>> {
        self.checker.clone().filter(|_| self.enabled)
    }

    fn layer(&mut self, marks: &mut Marks) -> DecorationCollectionId {
        *self.layer.get_or_insert_with(|| marks.create())
    }

    /// Forget the underlines and everything waiting, so that the next check
    /// covers all of the text.
    fn reset(&mut self, marks: &mut Marks) {
        if let Some(layer) = self.layer {
            marks.set(layer, Vec::new());
        }
        self.full = true;
        self.dirty.clear();
        self.scheduled = false;
        self.timer = Task::ready(());
        self.in_flight = None;
        self.check = Task::ready(());
        self.rerun = false;
        self.held = None;
        self.typed_to = None;
    }
}

/// The whole lines `ranges` touch, merged into runs of consecutive lines.
fn line_ranges(text: &Rope, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut rows: Vec<Range<usize>> = ranges
        .iter()
        .map(|range| {
            let start = text.offset_to_point(range.start.min(text.len())).row;
            let end = text.offset_to_point(range.end.min(text.len())).row;
            start..end + 1
        })
        .collect();
    rows.sort_by_key(|rows| rows.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(rows.len());
    for rows in rows {
        match merged.last_mut() {
            Some(last) if rows.start <= last.end => last.end = last.end.max(rows.end),
            _ => merged.push(rows),
        }
    }
    merged
        .into_iter()
        .map(|rows| text.line_start_offset(rows.start)..text.line_end_offset(rows.end - 1))
        .collect()
}

/// The text a mark covers, when its range still falls on the text.
fn marked_text(text: &Rope, range: &Range<usize>) -> Option<String> {
    (range.end <= text.len()).then(|| text.slice(range.clone()).to_string())
}

/// Whether an edit of `edit` touches `range`: overlaps it or reaches either
/// edge.
fn touches(edit: &Range<usize>, range: &Range<usize>) -> bool {
    edit.start <= range.end && range.start <= edit.end
}

/// Methods for a textarea's spell checking. See [`SpellChecker`].
impl TextareaState {
    /// Check spelling with `checker`.
    pub fn spell_checker(mut self, checker: Rc<dyn SpellChecker>) -> Self {
        self.extras.spelling.checker = Some(checker);
        self
    }

    /// Replace the spell checker, or remove it with `None`. The underlines are
    /// cleared, and a new checker checks all of the text.
    pub fn set_spell_checker(
        &mut self,
        checker: Option<Rc<dyn SpellChecker>>,
        cx: &mut Context<Self>,
    ) {
        let spelling = &mut self.extras.spelling;
        spelling.reset(&mut self.extras.marks);
        spelling.checker = checker;
        cx.notify();
    }

    /// Turn spell checking off and on, keeping the checker. Off clears the
    /// underlines, and on checks all of the text again. On by default.
    pub fn set_spell_checking(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let spelling = &mut self.extras.spelling;
        if spelling.enabled == enabled {
            return;
        }
        spelling.reset(&mut self.extras.marks);
        spelling.enabled = enabled;
        cx.notify();
    }

    /// Whether spelling is checked: a checker is installed and checking is
    /// on.
    pub fn is_spell_checking(&self) -> bool {
        self.extras.spelling.active_checker().is_some()
    }

    /// Check all of the text again now.
    pub fn check_spelling(&mut self, cx: &mut Context<Self>) {
        let spelling = &mut self.extras.spelling;
        if spelling.active_checker().is_none() {
            return;
        }
        spelling.full = true;
        self.schedule_spell_check(Duration::ZERO, cx);
    }

    /// The byte ranges of the words marked as misspelled, in order.
    pub fn misspellings(&self) -> Vec<Range<usize>> {
        let spelling = &self.extras.spelling;
        let mut ranges: Vec<Range<usize>> = spelling
            .layer
            .map(|layer| {
                self.extras
                    .marks
                    .get(layer)
                    .iter()
                    .map(Mark::range)
                    .collect()
            })
            .unwrap_or_default();
        ranges.sort_by_key(|range| range.start);
        ranges
    }

    /// The misspelled word at the byte `offset`, including its edges.
    pub fn misspelling_at(&self, offset: usize) -> Option<Misspelling> {
        let spelling = &self.extras.spelling;
        let checker = spelling.active_checker()?;
        let contains = |range: &Range<usize>| range.start <= offset && offset <= range.end;
        let range = spelling
            .layer
            .and_then(|layer| {
                self.extras
                    .marks
                    .get(layer)
                    .iter()
                    .map(Mark::range)
                    .find(|range| contains(range))
            })
            .or_else(|| spelling.held.clone().filter(|range| contains(range)))?;
        let word = marked_text(&self.text, &range)?;
        Some(Misspelling {
            range,
            word: word.into(),
            revision: self.document_revision,
            checker,
        })
    }

    /// Record an edit for the next check. See
    /// [`crate::input::InputModeKind::did_edit`].
    pub(crate) fn record_spelling_edit(
        &mut self,
        range: &Range<usize>,
        new_len: usize,
        cx: &mut Context<Self>,
    ) {
        let spelling = &mut self.extras.spelling;
        let Some(checker) = spelling.active_checker() else {
            return;
        };
        for dirty in &mut spelling.dirty {
            *dirty = adjust_range_for_edit(dirty, range, new_len);
        }
        spelling.dirty.push(range.start..range.start + new_len);
        if let Some(held) = spelling.held.take() {
            let replaced = !range.is_empty() && range.start <= held.start && held.end <= range.end;
            let held = adjust_range_for_edit(&held, range, new_len);
            spelling.held = (!replaced && !held.is_empty()).then_some(held);
        }
        if let Some(edits) = &mut spelling.in_flight {
            edits.push((range.clone(), new_len));
        }
        spelling.typed_to = (new_len > 0).then_some(range.start + new_len);
        self.schedule_spell_check(checker.debounce(), cx);
    }

    /// Start checking, hold back or release a misspelling at the caret, and
    /// listen for dictionary changes. Runs on every render.
    pub(crate) fn spelling_on_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.extras.spelling.active_checker().is_none() {
            return;
        }
        if self.extras.spelling.observing.is_none() {
            self.extras.spelling.observing = Some(cx.observe_global::<SpellingRevision>(
                |state: &mut Self, cx| state.check_spelling(cx),
            ));
        }
        if self.extras.spelling.full && !self.extras.spelling.scheduled {
            self.schedule_spell_check(Duration::ZERO, cx);
        }

        let caret = self
            .active_selection()
            .is_empty()
            .then(|| self.cursor())
            .filter(|_| self.selections.is_single() && self.focus_handle.is_focused(window));
        let spelling = &mut self.extras.spelling;
        if spelling.typed_to != caret {
            spelling.typed_to = None;
        }
        if let Some(held) = spelling.held.clone()
            && caret != Some(held.end)
        {
            spelling.held = None;
            self.mark_misspelled(vec![held], cx);
        }
    }

    fn schedule_spell_check(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let spelling = &mut self.extras.spelling;
        spelling.scheduled = true;
        let timer = (!delay.is_zero()).then(|| cx.background_executor().timer(delay));
        spelling.timer = cx.spawn(async move |state, cx| {
            if let Some(timer) = timer {
                timer.await;
            }
            _ = state.update(cx, |state, cx| state.run_spell_check(cx));
        });
    }

    fn run_spell_check(&mut self, cx: &mut Context<Self>) {
        let spelling = &mut self.extras.spelling;
        spelling.scheduled = false;
        let Some(checker) = spelling.active_checker() else {
            return;
        };
        if spelling.in_flight.is_some() {
            spelling.rerun = true;
            return;
        }
        let ranges = if spelling.full {
            vec![0..self.text.len()]
        } else {
            line_ranges(&self.text, &spelling.dirty)
        };
        spelling.full = false;
        spelling.dirty.clear();
        if ranges.is_empty() {
            return;
        }
        spelling.in_flight = Some(Vec::new());
        let request = SpellCheckRequest {
            text: self.text.clone(),
            ranges,
            revision: self.document_revision,
        };
        let mut task = checker.check(&request, cx);

        // A synchronous answer is applied in this update, before the next
        // frame is drawn.
        let mut poll_cx = TaskContext::from_waker(futures::task::noop_waker_ref());
        if let Poll::Ready(answer) = task.poll_unpin(&mut poll_cx) {
            self.receive_spell_check(request, answer, cx);
            return;
        }
        self.extras.spelling.check = cx.spawn(async move |state, cx| {
            let answer = task.await;
            _ = state.update(cx, |state, cx| {
                state.receive_spell_check(request, answer, cx)
            });
        });
    }

    fn receive_spell_check(
        &mut self,
        request: SpellCheckRequest,
        answer: Result<SpellCheck>,
        cx: &mut Context<Self>,
    ) {
        let spelling = &mut self.extras.spelling;
        // Dropped when the checker changed while it was checking.
        let Some(edits) = spelling.in_flight.take() else {
            return;
        };
        let rerun = std::mem::take(&mut spelling.rerun);
        match answer {
            Ok(check) => {
                let checked = if check.checked.is_empty() {
                    request.ranges
                } else {
                    check.checked
                };
                // Follow the edits made while the checker worked. A word an
                // edit touched is on an edited line, which is checked again.
                let checked: Vec<Range<usize>> = checked
                    .into_iter()
                    .map(|range| {
                        edits.iter().fold(range, |range, (edit, new_len)| {
                            adjust_range_for_edit(&range, edit, *new_len)
                        })
                    })
                    .collect();
                let misspelled: Vec<Range<usize>> = check
                    .misspelled
                    .into_iter()
                    .filter_map(|word| {
                        edits.iter().try_fold(word, |word, (edit, new_len)| {
                            (!touches(edit, &word))
                                .then(|| adjust_range_for_edit(&word, edit, *new_len))
                        })
                    })
                    .filter(|word| {
                        checked
                            .iter()
                            .any(|range| range.start <= word.start && word.end <= range.end)
                    })
                    .collect();
                self.apply_spell_check(&checked, misspelled, cx);
            }
            Err(error) => tracing::warn!("spell checker failed: {error:#}"),
        }
        if rerun {
            self.schedule_spell_check(Duration::ZERO, cx);
        }
    }

    /// Whether `word` can be marked: a nonempty range on character
    /// boundaries, and a word the user did not ignore.
    fn is_markable(&self, word: &Range<usize>) -> bool {
        word.start < word.end
            && word.end <= self.text.len()
            && self.text.is_char_boundary(word.start)
            && self.text.is_char_boundary(word.end)
            && marked_text(&self.text, word)
                .is_some_and(|text| !self.extras.spelling.ignored.contains(&text))
    }

    /// Replace the underlines in `checked` with `misspelled`, holding back
    /// a word that is still being typed.
    fn apply_spell_check(
        &mut self,
        checked: &[Range<usize>],
        misspelled: Vec<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let misspelled: Vec<Range<usize>> = misspelled
            .into_iter()
            .filter(|word| self.is_markable(word))
            .collect();
        let spelling = &mut self.extras.spelling;
        let typed_to = spelling.typed_to;
        let (held, marks): (Vec<_>, Vec<_>) = misspelled
            .into_iter()
            .partition(|word| Some(word.end) == typed_to);
        let within_checked = |word: &Range<usize>| {
            checked
                .iter()
                .any(|range| range.start <= word.start && word.end <= range.end)
        };
        if spelling.held.as_ref().is_some_and(within_checked) {
            spelling.held = None;
        }
        if let Some(held) = held.into_iter().next() {
            spelling.held = Some(held);
        }
        let marks = marks
            .into_iter()
            .map(|word| Mark::new(word, MarkStyle::Spelling))
            .collect();
        let layer = spelling.layer(&mut self.extras.marks);
        if self.extras.marks.splice(layer, checked, marks) {
            cx.notify();
        }
    }

    /// Underline `words`, keeping the underlines already there.
    fn mark_misspelled(&mut self, words: Vec<Range<usize>>, cx: &mut Context<Self>) {
        let marks: Vec<Mark> = words
            .into_iter()
            .filter(|word| self.is_markable(word))
            .map(|word| Mark::new(word, MarkStyle::Spelling))
            .collect();
        if marks.is_empty() {
            return;
        }
        let layer = self.extras.spelling.layer(&mut self.extras.marks);
        if self.extras.marks.splice(layer, &[], marks) {
            cx.notify();
        }
    }

    /// Remove the underlines of every occurrence of `word`.
    fn unmark_word(&mut self, word: &str, cx: &mut Context<Self>) {
        let text = &self.text;
        let spelling = &mut self.extras.spelling;
        if spelling
            .held
            .as_ref()
            .is_some_and(|held| marked_text(text, held).as_deref() == Some(word))
        {
            spelling.held = None;
        }
        let Some(layer) = spelling.layer else {
            return;
        };
        let marks = self.extras.marks.get(layer);
        let kept: Vec<Mark> = marks
            .iter()
            .filter(|mark| marked_text(text, &mark.range()).as_deref() != Some(word))
            .cloned()
            .collect();
        if kept.len() != marks.len() {
            self.extras.marks.set(layer, kept);
            cx.notify();
        }
    }

    fn on_action_replace_misspelling(
        &mut self,
        action: &ReplaceMisspelling,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = action.range.clone();
        let applies = self.is_editable()
            && action.revision == self.document_revision
            && range.start <= range.end
            && range.end <= self.text.len()
            && self.text.is_char_boundary(range.start)
            && self.text.is_char_boundary(range.end);
        if !applies {
            cx.propagate();
            return;
        }
        let word = SharedString::new(self.text.slice(range.clone()).to_string());

        // One step of its own in the history, apart from the typing before it.
        self.undo_manager.break_transaction_coalescing();
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        let range_utf16 = self.range_to_utf16(&range);
        self.replace_text_in_range_silent(Some(range_utf16), &action.text, window, cx);
        self.undo_manager.break_transaction_coalescing();
        // The caret ends after a word that was chosen, not typed.
        self.extras.spelling.typed_to = None;

        cx.emit(SpellEvent::Replaced {
            word,
            with: action.text.clone(),
        });
    }

    fn on_action_add_to_dictionary(
        &mut self,
        action: &AddToDictionary,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(checker) = self.extras.spelling.active_checker() else {
            cx.propagate();
            return;
        };
        checker.add_to_dictionary(&action.word, cx);
        self.unmark_word(&action.word, cx);
        cx.emit(SpellEvent::AddedToDictionary {
            word: action.word.clone(),
        });
        refresh_spelling(cx);
    }

    fn on_action_ignore_misspelling(
        &mut self,
        action: &IgnoreMisspelling,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(checker) = self.extras.spelling.active_checker() else {
            cx.propagate();
            return;
        };
        self.extras.spelling.ignored.insert(action.word.to_string());
        checker.ignore(&action.word, cx);
        self.unmark_word(&action.word, cx);
        cx.emit(SpellEvent::Ignored {
            word: action.word.clone(),
        });
        refresh_spelling(cx);
    }

    /// Registers the spelling actions on the textarea's root element.
    pub(crate) fn register_spelling_actions(
        element: gpui::Stateful<gpui::Div>,
        entity: &gpui::Entity<Self>,
        window: &mut Window,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        element
            .on_action(window.listener_for(entity, Self::on_action_replace_misspelling))
            .on_action(window.listener_for(entity, Self::on_action_add_to_dictionary))
            .on_action(window.listener_for(entity, Self::on_action_ignore_misspelling))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use futures::channel::oneshot;
    use gpui::{
        AppContext as _, Entity, EntityInputHandler as _, IntoElement, ParentElement as _, Render,
        Styled as _, Subscription, TestAppContext, VisualTestContext, div, px, size,
    };

    use super::*;
    use crate::input::{Redo, Undo};

    /// Knows a list of words, case-insensitively, and the words added to it
    /// as they were spelled. A word is a run of letters and apostrophes.
    #[derive(Default)]
    struct WordList {
        words: RefCell<HashSet<String>>,
        learned: RefCell<HashSet<String>>,
        /// Hold every answer until the test sends it.
        held: bool,
        answers: RefCell<Vec<(SpellCheck, oneshot::Sender<SpellCheck>)>>,
        requests: RefCell<Vec<SpellCheckRequest>>,
        added: RefCell<Vec<String>>,
        ignored: RefCell<Vec<String>>,
    }

    impl WordList {
        fn new(words: &[&str]) -> Self {
            Self {
                words: RefCell::new(words.iter().map(|word| word.to_string()).collect()),
                ..Default::default()
            }
        }

        fn knows(&self, word: &str) -> bool {
            self.words.borrow().contains(&word.to_lowercase())
                || self.learned.borrow().contains(word)
        }

        /// The requested ranges of each request, as text.
        fn requested(&self) -> Vec<Vec<String>> {
            self.requests
                .borrow()
                .iter()
                .map(|request| {
                    request
                        .ranges()
                        .iter()
                        .map(|range| request.text().slice(range.clone()).to_string())
                        .collect()
                })
                .collect()
        }

        /// Send the held answer to request `ix`.
        fn answer(&self, ix: usize) {
            let (check, sender) = self.answers.borrow_mut().remove(ix);
            _ = sender.send(check);
        }
    }

    impl SpellChecker for WordList {
        fn check(&self, request: &SpellCheckRequest, cx: &mut App) -> Task<Result<SpellCheck>> {
            self.requests.borrow_mut().push(request.clone());
            let mut misspelled = Vec::new();
            for range in request.ranges() {
                let text = request.text().slice(range.clone()).to_string();
                let mut start = None;
                for (ix, c) in text.char_indices().chain([(text.len(), ' ')]) {
                    match (start, c.is_alphabetic() || c == '\'') {
                        (None, true) => start = Some(ix),
                        (Some(word_start), false) => {
                            if !self.knows(&text[word_start..ix]) {
                                misspelled.push(range.start + word_start..range.start + ix);
                            }
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
            let check = SpellCheck {
                misspelled,
                ..Default::default()
            };
            if !self.held {
                return Task::ready(Ok(check));
            }
            let (sender, receiver) = oneshot::channel();
            self.answers.borrow_mut().push((check, sender));
            cx.spawn(async move |_| Ok(receiver.await.unwrap_or_default()))
        }

        fn suggestions(&self, word: &str, _: &mut App) -> Vec<SharedString> {
            let mut words: Vec<String> = self
                .words
                .borrow()
                .iter()
                .filter(|known| {
                    let mut a: Vec<char> = known.chars().collect();
                    let mut b: Vec<char> = word.to_lowercase().chars().collect();
                    a.sort_unstable();
                    b.sort_unstable();
                    a == b
                })
                .cloned()
                .collect();
            words.sort();
            words.into_iter().map(SharedString::from).collect()
        }

        fn add_to_dictionary(&self, word: &str, _: &mut App) {
            self.learned.borrow_mut().insert(word.to_string());
            self.added.borrow_mut().push(word.to_string());
        }

        fn ignore(&self, word: &str, _: &mut App) {
            self.ignored.borrow_mut().push(word.to_string());
        }
    }

    struct Harness {
        textarea: Entity<TextareaState>,
        _subscription: Subscription,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().h(px(200.)).child(self.textarea.clone()))
        }
    }

    struct Test {
        cx: VisualTestContext,
        textarea: Entity<TextareaState>,
        events: Rc<RefCell<Vec<SpellEvent>>>,
    }

    impl Test {
        fn new(cx: &mut TestAppContext, checker: Rc<WordList>, text: &str) -> Self {
            cx.update(crate::init);
            let events: Rc<RefCell<Vec<SpellEvent>>> = Default::default();
            let text = text.to_string();
            let window = cx.open_window(size(px(400.), px(240.)), {
                let events = events.clone();
                move |window, cx| {
                    let textarea = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .spell_checker(checker)
                            .default_value(text)
                    });
                    let _subscription = cx.subscribe(&textarea, {
                        let events = events.clone();
                        move |_, _, event: &SpellEvent, _| events.borrow_mut().push(event.clone())
                    });
                    Harness {
                        textarea,
                        _subscription,
                    }
                }
            });
            let mut cx = VisualTestContext::from_window(window.into(), cx);
            let textarea = window
                .read_with(&cx, |harness, _| harness.textarea.clone())
                .unwrap();
            cx.update(|window, cx| {
                textarea.update(cx, |state, cx| state.focus(window, cx));
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
            Self {
                cx,
                textarea,
                events,
            }
        }

        fn draw(&mut self) {
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
            self.cx.run_until_parked();
        }

        /// Let typing pause long enough for the edited lines to be checked.
        fn pause(&mut self) {
            self.cx.executor().advance_clock(Duration::from_millis(300));
            self.cx.run_until_parked();
        }

        fn update<R>(
            &mut self,
            f: impl FnOnce(&mut TextareaState, &mut Window, &mut Context<TextareaState>) -> R,
        ) -> R {
            let textarea = self.textarea.clone();
            self.cx
                .update(|window, cx| textarea.update(cx, |state, cx| f(state, window, cx)))
        }

        /// Type `text` at byte `offset`, as the keyboard does.
        fn type_at(&mut self, offset: usize, text: &str) {
            self.update(|state, window, cx| {
                state.set_selected_range(offset..offset, cx);
                state.replace_text_in_range(None, text, window, cx);
            });
            self.draw();
        }

        fn value(&self) -> String {
            self.textarea
                .read_with(&self.cx, |state, _| state.value().to_string())
        }

        /// The marked words.
        fn marked(&self) -> Vec<String> {
            self.textarea.read_with(&self.cx, |state, _| {
                state
                    .misspellings()
                    .into_iter()
                    .map(|range| state.text().slice(range).to_string())
                    .collect()
            })
        }

        fn misspellings(&self) -> Vec<Range<usize>> {
            self.textarea
                .read_with(&self.cx, |state, _| state.misspellings())
        }

        fn take_events(&self) -> Vec<SpellEvent> {
            std::mem::take(&mut self.events.borrow_mut())
        }
    }

    const WORDS: &[&str] = &[
        "a", "the", "cat", "sat", "on", "mat", "i", "saw", "today", "one", "two", "three", "four",
        "ship", "now", "more", "and", "later",
    ];

    /// The first render checks all of the text, and the underlines then move
    /// with edits before any check runs again.
    #[gpui::test]
    fn underlines_move_with_edits(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker.clone(), "Teh cat sat\non teh mat");
        assert_eq!(checker.requested(), [["Teh cat sat\non teh mat"]]);
        assert_eq!(test.misspellings(), [0..3, 15..18]);

        test.type_at(0, "A ");
        assert_eq!(test.misspellings(), [2..5, 17..20]);
        assert_eq!(test.marked(), ["Teh", "teh"]);
        assert_eq!(checker.requests.borrow().len(), 1, "typing has not paused");

        // Deleting the line break joins the lines, and the second underline
        // comes along.
        test.update(|state, window, cx| {
            state.replace_text_in_range(Some(13..14), " ", window, cx);
        });
        assert_eq!(test.marked(), ["Teh", "teh"]);
        test.pause();
        assert_eq!(test.value(), "A Teh cat sat on teh mat");
        assert_eq!(test.marked(), ["Teh", "teh"]);
    }

    /// After the first check, only the lines edited since the last check are
    /// checked, once typing pauses.
    #[gpui::test]
    fn only_edited_lines_are_checked_again(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker.clone(), "one\ntwo\nthree\nfour");
        assert_eq!(checker.requested().len(), 1);

        test.type_at(13, "s");
        test.type_at(14, " one");
        assert_eq!(checker.requested().len(), 1, "typing has not paused");
        test.pause();
        assert_eq!(checker.requested()[1], ["threes one"]);

        // Lines apart are separate ranges, lines next to each other one.
        test.type_at(0, "a ");
        test.type_at(test.value().len(), " a");
        test.type_at(6, "a ");
        test.pause();
        assert_eq!(checker.requested()[2], ["a one\na two", "four a"]);
        assert_eq!(checker.requested().len(), 3);
    }

    /// A misspelling that ends at the caret right after typing is not marked
    /// until the caret leaves the word.
    #[gpui::test]
    fn a_word_being_typed_is_marked_once_the_caret_leaves_it(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker, "");

        for ch in ["t", "e", "h"] {
            let end = test.value().len();
            test.type_at(end, ch);
        }
        test.pause();
        assert!(test.marked().is_empty(), "the word is still being typed");
        test.textarea.read_with(&test.cx, |state, _| {
            assert_eq!(state.misspelling_at(2).unwrap().word(), "teh");
        });

        test.type_at(3, " ");
        assert_eq!(test.marked(), ["teh"]);

        // Moving the caret away releases a held word too.
        test.type_at(4, "cta");
        test.pause();
        assert_eq!(test.marked(), ["teh"]);
        test.update(|state, _, cx| state.set_selected_range(0..0, cx));
        test.draw();
        assert_eq!(test.marked(), ["teh", "cta"]);
    }

    /// The misspelling under a position, with the checker's suggestions and
    /// the actions that fix it.
    #[gpui::test]
    fn the_misspelling_at_a_position(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker, "I saw teh cat");

        test.textarea.read_with(&test.cx, |state, _| {
            assert!(state.misspelling_at(5).is_none());
            let misspelling = state.misspelling_at(6).unwrap();
            assert_eq!(misspelling.range(), 6..9);
            assert_eq!(misspelling.word(), "teh");
            assert_eq!(misspelling.revision(), state.revision());
            assert_eq!(
                misspelling.replace_with("the"),
                ReplaceMisspelling {
                    range: 6..9,
                    text: "the".into(),
                    revision: state.revision(),
                }
            );
            assert_eq!(
                misspelling.add_to_dictionary(),
                AddToDictionary { word: "teh".into() }
            );
            assert_eq!(state.misspelling_at(9).unwrap().range(), 6..9);
            assert!(state.misspelling_at(10).is_none());
        });
        let suggestions = test.cx.update(|_, cx| {
            let misspelling = test.textarea.read(cx).misspelling_at(7).unwrap();
            misspelling.suggestions(cx)
        });
        assert_eq!(suggestions, ["the"]);
    }

    /// Choosing a suggestion replaces the word in one step of the history,
    /// apart from the typing before it, and only while the text is unchanged.
    #[gpui::test]
    fn replacing_a_misspelling_is_one_undo_step(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker, "I saw teh cat");
        test.type_at(13, " today");
        let revision = test.update(|state, _, _| state.revision());

        test.cx.dispatch_action(ReplaceMisspelling {
            range: 6..9,
            text: "the".into(),
            revision: revision - 1,
        });
        assert_eq!(
            test.value(),
            "I saw teh cat today",
            "the text has changed since"
        );

        test.cx.dispatch_action(ReplaceMisspelling {
            range: 6..9,
            text: "the".into(),
            revision,
        });
        assert_eq!(test.value(), "I saw the cat today");
        assert!(test.marked().is_empty());
        assert_eq!(
            test.take_events(),
            [SpellEvent::Replaced {
                word: "teh".into(),
                with: "the".into(),
            }]
        );

        test.update(|state, window, cx| state.undo(&Undo, window, cx));
        assert_eq!(test.value(), "I saw teh cat today");
        test.update(|state, window, cx| state.undo(&Undo, window, cx));
        assert_eq!(test.value(), "I saw teh cat");
        test.update(|state, window, cx| state.redo(&Redo, window, cx));
        test.update(|state, window, cx| state.redo(&Redo, window, cx));
        assert_eq!(test.value(), "I saw the cat today");
    }

    /// Adding a word to the dictionary clears every underline of it, then
    /// checks all of the text again with the new dictionary.
    #[gpui::test]
    fn adding_a_word_clears_all_its_underlines(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(
            cx,
            checker.clone(),
            "Ship gpui now.\nMore gpui and Gpui later, and teh.",
        );
        assert_eq!(test.marked(), ["gpui", "gpui", "Gpui", "teh"]);
        assert_eq!(checker.requested().len(), 1);

        test.cx.dispatch_action(AddToDictionary {
            word: "gpui".into(),
        });
        test.draw();
        // This checker learns words as they are spelled.
        assert_eq!(test.marked(), ["Gpui", "teh"]);
        assert_eq!(checker.added.borrow().as_slice(), ["gpui"]);
        assert_eq!(
            test.take_events(),
            [SpellEvent::AddedToDictionary {
                word: "gpui".into()
            }]
        );
        assert_eq!(
            checker.requested()[1..],
            [["Ship gpui now.\nMore gpui and Gpui later, and teh."]]
        );
    }

    /// The underlines of an added word go at once, before the check that
    /// follows has answered.
    #[gpui::test]
    fn an_added_word_is_unmarked_before_the_next_check(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker.clone(), "gpui teh gpui");
        assert_eq!(test.marked(), ["gpui", "teh", "gpui"]);
        let unmarked = test.update(|state, window, cx| {
            state.on_action_add_to_dictionary(
                &AddToDictionary {
                    word: "gpui".into(),
                },
                window,
                cx,
            );
            state.misspellings()
        });
        assert_eq!(unmarked, [5..8]);
        assert_eq!(checker.requested().len(), 1);
    }

    /// An ignored word stays unmarked in this textarea, also after its line
    /// is checked again, and the checker hears about it.
    #[gpui::test]
    fn an_ignored_word_stays_unmarked(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker.clone(), "Ship gpui now, teh gpui.");
        assert_eq!(test.marked(), ["gpui", "teh", "gpui"]);

        test.cx.dispatch_action(IgnoreMisspelling {
            word: "gpui".into(),
        });
        assert_eq!(test.marked(), ["teh"]);
        assert_eq!(checker.ignored.borrow().as_slice(), ["gpui"]);
        assert_eq!(
            test.take_events(),
            [SpellEvent::Ignored {
                word: "gpui".into()
            }]
        );

        test.type_at(0, "gpui ");
        test.pause();
        assert_eq!(test.marked(), ["teh"]);
    }

    /// An answer that arrives after the text changed is moved along with the
    /// edits. A word an edit touched waits for the next check of its line.
    #[gpui::test]
    fn a_slow_answer_follows_the_edits_made_meanwhile(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList {
            held: true,
            ..WordList::new(WORDS)
        });
        let mut test = Test::new(cx, checker.clone(), "cat teh\nsat on teh mat");
        assert!(test.marked().is_empty(), "the answer is still on its way");

        test.type_at(0, "a ");
        test.type_at(20, "x,");
        assert_eq!(test.value(), "a cat teh\nsat on tehx, mat");
        checker.answer(0);
        test.cx.run_until_parked();
        assert_eq!(test.misspellings(), [6..9]);

        test.pause();
        assert_eq!(checker.requested()[1], ["a cat teh\nsat on tehx, mat"]);
        checker.answer(0);
        test.cx.run_until_parked();
        assert_eq!(test.marked(), ["teh", "tehx"]);
    }

    /// Turning spell checking off clears the underlines, and back on checks
    /// the text again. Replacing the checker does the same with the new one.
    #[gpui::test]
    fn spell_checking_turns_off_and_on(cx: &mut TestAppContext) {
        let checker = Rc::new(WordList::new(WORDS));
        let mut test = Test::new(cx, checker.clone(), "teh cat");
        assert_eq!(test.marked(), ["teh"]);

        test.update(|state, _, cx| state.set_spell_checking(false, cx));
        test.draw();
        assert!(test.marked().is_empty());
        test.type_at(0, "zzz ");
        test.pause();
        assert!(test.marked().is_empty());
        assert_eq!(checker.requested().len(), 1);

        test.update(|state, _, cx| state.set_spell_checking(true, cx));
        test.draw();
        assert_eq!(test.marked(), ["zzz", "teh"]);

        let other = Rc::new(WordList::new(&["teh", "cat", "zzz"]));
        test.update(|state, _, cx| state.set_spell_checker(Some(other.clone()), cx));
        test.draw();
        assert!(test.marked().is_empty());
        assert_eq!(other.requested().len(), 1);
    }

    #[test]
    fn line_ranges_cover_whole_lines_and_merge_neighbours() {
        let text = Rope::from("one\ntwo\nthree\nfour");
        assert_eq!(line_ranges(&text, &[5..5]), [4..7]);
        assert_eq!(line_ranges(&text, &[0..0, 9..10]), [0..3, 8..13]);
        assert_eq!(line_ranges(&text, &[0..1, 5..6]), [0..7]);
        assert_eq!(line_ranges(&text, &[2..9, 15..15]), [0..18]);
        assert_eq!(line_ranges(&text, &[18..18]), [14..18]);
    }
}
