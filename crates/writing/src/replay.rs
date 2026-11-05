//! Typing into word suggestions without a window, the way a textarea does,
//! to measure them on written text. Built with the `replay` feature. It is
//! not part of the crate's stable API.
//!
//! A [`Writer`] holds one person's writing aids, which are their
//! vocabulary, the pacing their textareas share, and a clock that only
//! moves when told to. Each [`Field`] is one textarea. It applies keys as
//! GPUI Kit's textarea does. It reports every edit before the next request,
//! closes the ghost text at every edit, asks after a typed key that triggers, waits out
//! Quiet's pause on the clock, and inserts Tab's and Ctrl+Right's text with
//! the suggestion's origin.

use crate::completion::policy::{FrequencyOnly, SuggestionPolicy, Today};
use crate::completion::{Answer, Edit, SuggestionPacing, WordSuggestions};
use crate::spelling::Dictionary;
use crate::{WordList, Writing};
use gpui_base::input::{EditOrigin, Rope, SuggestionProvider as _};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub use crate::completion::SuggestionMode;
pub use crate::completion::pacing::Paced;

/// The bundled English word list, loaded on this thread.
pub fn english() -> Rc<Dictionary> {
    Rc::new(Dictionary::load(&[WordList::English], None))
}

/// How the suggestions decide what shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// As the crate does.
    Default,
    /// Word frequency alone. The most frequent known word that fits the
    /// prefix shows when it holds at least half the count of the words that
    /// fit, with no next word and no words after the first.
    FrequencyOnly,
    /// Word frequency alone, the most frequent fitting word whatever its
    /// share.
    FrequencyAlways,
}

impl Policy {
    fn make(self) -> Rc<dyn SuggestionPolicy> {
        match self {
            Self::Default => Rc::new(Today),
            Self::FrequencyOnly => Rc::new(FrequencyOnly { always: false }),
            Self::FrequencyAlways => Rc::new(FrequencyOnly { always: true }),
        }
    }
}

/// One person's writing aids and clock.
pub struct Writer {
    writing: Writing,
    policy: Rc<dyn SuggestionPolicy>,
    start: Instant,
    clock: Rc<Cell<Duration>>,
}

impl Writer {
    /// A writer with an empty vocabulary that trusts `dictionary`'s words,
    /// with suggestions in `mode`.
    pub fn new(mode: SuggestionMode, dictionary: Rc<Dictionary>) -> Self {
        Self::with_policy(mode, dictionary, Policy::Default)
    }

    /// A writer as [`Self::new`] makes, whose suggestions decide what shows
    /// by `policy`.
    pub fn with_policy(mode: SuggestionMode, dictionary: Rc<Dictionary>, policy: Policy) -> Self {
        let vocabulary =
            crate::completion::vocabulary::Vocabulary::with_dictionary(dictionary.clone());
        let writing = Writing::from_parts(dictionary, vocabulary);
        writing.set_suggestion_mode(mode);
        let mut writer = Self::with_writing(writing);
        writer.policy = policy.make();
        writer
    }

    pub(crate) fn with_writing(writing: Writing) -> Self {
        Self {
            writing,
            policy: Rc::new(Today),
            start: Instant::now(),
            clock: Rc::new(Cell::new(Duration::ZERO)),
        }
    }

    /// How long since the writer started, on their clock.
    pub fn elapsed(&self) -> Duration {
        self.clock.get()
    }

    /// Moves the clock on by `duration`, between pieces of writing. A
    /// field's [`Field::wait`] moves it while typing.
    pub fn wait(&self, duration: Duration) {
        self.clock.set(self.clock.get() + duration);
    }

    /// A new, empty textarea.
    pub fn field(&self) -> Field {
        Field {
            suggestions: self.writing.word_suggestions_with(self.policy.clone()),
            start: self.start,
            clock: self.clock.clone(),
            text: Rope::new(),
            caret: 0,
            offer: None,
            due: None,
            edits: Vec::new(),
            answers: Vec::new(),
        }
    }

    /// How long a pause Quiet waits for now.
    pub fn pause(&self) -> Duration {
        self.pacing().pause()
    }

    /// What the pacing did to offers since this was last called.
    pub fn take_paced(&self) -> Vec<Paced> {
        self.pacing().take_log()
    }

    /// Whether the vocabulary holds `word`, in any case, and trusts it
    /// enough to offer it in prose.
    pub fn knows(&self, word: &str) -> bool {
        let vocabulary = self.writing.vocabulary().borrow();
        let folded = crate::completion::vocabulary::fold(word);
        let count = vocabulary.counts().word_count(&folded);
        count > 0 && vocabulary.is_trusted(word, count, 0)
    }

    /// How many words the vocabulary holds, and how many times it saw them.
    pub fn vocabulary_size(&self) -> (usize, u64) {
        let vocabulary = self.writing.vocabulary().borrow();
        let counts = vocabulary.counts();
        (counts.words_with_prefix("").count(), counts.total())
    }

    fn pacing(&self) -> &SuggestionPacing {
        self.writing.pacing()
    }

    #[cfg(test)]
    pub(crate) fn word_counts(&self) -> Vec<(String, u32)> {
        self.writing.vocabulary().borrow().word_counts()
    }
}

/// A suggestion on offer, with its text and where its range starts. Tab
/// replaces the range, from its start to the caret, with the text.
#[derive(Clone, Debug, PartialEq)]
struct Offer {
    text: String,
    start: usize,
}

/// One request's answer, at the caret it was made for.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub caret: usize,
    /// The model's suggestion.
    pub model: Option<String>,
    /// What pacing let show of it.
    pub shown: Option<String>,
}

/// One textarea of a [`Writer`].
pub struct Field {
    suggestions: Rc<WordSuggestions>,
    start: Instant,
    clock: Rc<Cell<Duration>>,
    text: Rope,
    caret: usize,
    offer: Option<Offer>,
    /// When a request waiting for typing to pause is sent.
    due: Option<Duration>,
    /// Edits the suggestions haven't heard of.
    edits: Vec<(Range<usize>, String, EditOrigin)>,
    answers: Vec<Asked>,
}

impl Field {
    /// The text.
    pub fn text(&self) -> String {
        self.text.to_string()
    }

    /// The caret, as a byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The ghost text after the caret, which Tab would add.
    pub fn ghost(&self) -> Option<String> {
        let offer = self.offer.as_ref()?;
        let typed = self.text.slice(offer.start..self.caret).to_string();
        offer
            .text
            .strip_prefix(typed.as_str())
            .filter(|rest| !rest.is_empty())
            .map(str::to_string)
    }

    /// The whole suggestion on offer, as the textarea lists it.
    pub fn suggestion(&self) -> Option<&str> {
        self.offer.as_ref().map(|offer| offer.text.as_str())
    }

    /// The requests answered since this was last called.
    pub fn take_answers(&mut self) -> Vec<Asked> {
        std::mem::take(&mut self.answers)
    }

    /// Moves the clock on by `duration` while this field has the focus,
    /// sending a request that was waiting for a pause once it is due.
    pub fn wait(&mut self, duration: Duration) {
        self.clock.set(self.clock.get() + duration);
        self.send_due();
    }

    /// Types `key`, one keystroke's text, at the caret.
    pub fn type_key(&mut self, key: &str) {
        self.send_due();
        let start = self.caret;
        self.apply(start..start, key, EditOrigin::Typed);
        if self.suggestions.typed_key(start, key, self.now()) {
            let debounce = self.suggestions.debounce();
            if debounce.is_zero() {
                self.ask();
            } else {
                self.due = Some(self.clock.get() + debounce);
            }
        }
        self.report();
    }

    /// Presses Enter, which inserts a line break.
    pub fn enter(&mut self) {
        self.send_due();
        let caret = self.caret;
        self.apply(caret..caret, "\n", EditOrigin::Program);
        self.report();
    }

    /// Presses Backspace, which deletes the character before the caret.
    pub fn backspace(&mut self) {
        self.send_due();
        let Some(before) = self.char_before() else {
            return;
        };
        let caret = self.caret;
        self.apply(caret - before.len_utf8()..caret, "", EditOrigin::Program);
        self.report();
    }

    /// Pastes `text` at the caret.
    pub fn paste(&mut self, text: &str) {
        self.send_due();
        let caret = self.caret;
        self.apply(caret..caret, text, EditOrigin::Pasted);
        self.report();
    }

    /// Presses Tab, which takes the whole suggestion. Returns what it
    /// inserted, or `None` with no suggestion on offer.
    pub fn tab(&mut self) -> Option<String> {
        self.send_due();
        let ghost = self.ghost()?;
        let offer = self.offer.clone()?;
        let caret = self.caret;
        self.apply(offer.start..caret, &offer.text, EditOrigin::Suggestion);
        self.report();
        self.suggestions.accepted();
        Some(ghost)
    }

    /// Presses Ctrl+Right, which takes the suggestion's next word and
    /// leaves the rest on offer, or the whole of it when one word is left.
    /// Returns what it inserted.
    pub fn accept_word(&mut self) -> Option<String> {
        self.send_due();
        let remainder = self.ghost()?;
        let word = next_word(self.char_before(), &remainder).to_string();
        if word.len() == remainder.len() {
            return self.tab();
        }
        let offer = self.offer.clone()?;
        let caret = self.caret;
        self.apply(caret..caret, &word, EditOrigin::Suggestion);
        self.offer = Some(offer);
        self.report();
        self.suggestions.accepted();
        Some(word)
    }

    /// Presses Escape, which closes the suggestion. Returns whether one was
    /// on offer.
    pub fn escape(&mut self) -> bool {
        self.send_due();
        if self.offer.take().is_none() {
            return false;
        }
        self.suggestions.dismissed(self.now());
        true
    }

    /// Sends the text, as a chat composer does. The suggestions learn its
    /// last sentence, and the field empties.
    pub fn send(&mut self) {
        self.send_due();
        self.suggestions.learn_rest(&self.text);
        let len = self.text.len();
        self.apply(0..len, "", EditOrigin::Program);
        self.report();
    }

    fn now(&self) -> Instant {
        self.start + self.clock.get()
    }

    fn char_before(&self) -> Option<char> {
        (self.caret > 0)
            .then(|| self.text.chars_at(self.caret).reversed().next())
            .flatten()
    }

    /// Replaces `range` with `text`, as one edit from `origin`. Like the
    /// textarea's, an edit closes the suggestion and drops a request that
    /// was waiting.
    fn apply(&mut self, range: Range<usize>, text: &str, origin: EditOrigin) {
        self.text.remove(range.clone());
        self.text.insert(range.start, text);
        self.caret = range.start + text.len();
        self.offer = None;
        self.due = None;
        self.edits.push((range, text.to_string(), origin));
    }

    /// Tells the suggestions about the edits they haven't heard of, as the
    /// textarea does at the end of an update and before each request.
    fn report(&mut self) {
        if self.edits.is_empty() {
            return;
        }
        let edits = std::mem::take(&mut self.edits);
        let edits: Vec<Edit> = edits
            .iter()
            .map(|(range, text, origin)| Edit {
                range: range.clone(),
                text,
                origin: *origin,
            })
            .collect();
        self.suggestions.heard(&edits, &self.text, self.now());
    }

    fn send_due(&mut self) {
        if self.due.is_some_and(|due| due <= self.clock.get()) {
            self.due = None;
            self.ask();
        }
    }

    fn ask(&mut self) {
        self.report();
        let Answer { model, shown } =
            self.suggestions
                .request(&self.text, self.caret, self.now(), None);
        self.offer = shown
            .clone()
            .filter(|shown| !shown.is_empty())
            .map(|text| Offer {
                text,
                start: self.caret,
            });
        self.answers.push(Asked {
            caret: self.caret,
            model,
            shown,
        });
    }
}

/// The part of `remainder` that Ctrl+Right takes, as the textarea has it,
/// which is leading whitespace, then one word or one other character.
/// `before` is the character before the caret.
fn next_word(before: Option<char>, remainder: &str) -> &str {
    let start = remainder.len() - remainder.trim_start().len();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let is_apostrophe = |c: char| matches!(c, '\'' | '\u{2019}');
    let mut prev = if start == 0 { before } else { None };
    let mut chars = remainder[start..].char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let next = chars.peek().map(|&(_, next)| next);
        let joined = is_apostrophe(c)
            && prev.is_some_and(char::is_alphabetic)
            && next.is_some_and(char::is_alphabetic);
        if !is_word(c) && !joined {
            let end = if index == 0 { c.len_utf8() } else { index };
            return &remainder[..start + end];
        }
        prev = Some(c);
    }
    remainder
}
