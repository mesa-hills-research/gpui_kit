//! Personal, on-device word suggestions.
//!
//! # Design
//!
//! Suggestions come from an n-gram model learned online from the text the
//! user has typed, kept on their computer:
//!
//! 1. **A calibrated probability.** For the word being typed, or the next
//!    word after a space, the model estimates the probability that a word is
//!    the one coming, from the one or two words before it in the sentence and
//!    whether the sentence starts there. A suggestion shows when that
//!    probability is at least one half, so it is right more often than not.
//!    See [`model`].
//!
//! 2. **Words past the current one when they are likely too.** Words are
//!    added after the first while the chance that the whole suggestion is
//!    right stays high, and the length offered is the one that saves the
//!    most keystrokes on average. Tab takes it all, and the key
//!    [`accept_word_bindings`] binds takes its next word.
//!
//! 3. **Words in any case.** "Thanks" and "thanks" are one word. A
//!    suggestion keeps the letters typed and finishes the word in the form
//!    the user usually writes it. See [`vocabulary`].
//!
//! 4. **Context awareness.** In prose, code-shaped candidates are filtered
//!    out. In code, suggestions come from the local document only and are
//!    never extended. See [`crate::classify`].
//!
//! The local per-document index adds the document's own words, such as
//! pasted names and code identifiers. It follows each edit by counting the
//! lines the edit touched again, and counts only what the shared vocabulary
//! doesn't already hold, so no text counts twice (see [`local`]).
//!
//! # Learning
//!
//! The persistent vocabulary learns only from typed text. A sentence is
//! learned at the first typed key after its terminator, a line break
//! included, so a line ended by a blank line counts. It is skipped when any
//! of it was pasted, undone or redone, or inserted by the program rather than
//! typed (see [`typed`]), and when its context classifies as code. Each
//! sentence is learned once, however the text around it changes, and the
//! text a textarea opens with isn't learned again (see [`learned`]).
//!
//! An apostrophe between two letters stays inside a word, so contractions
//! such as "don't" are one word. One-character English words `a`, `A`, and
//! `I` are preserved. Other rejected tokens break the n-gram sequence so
//! words on either side never become falsely adjacent.
//!
//! # Spelling hygiene
//!
//! When a dictionary is available, words that are not dictionary-correct are
//! only suggested after repeated committed use, so one-off typos do not
//! surface while genuine jargon can earn trust.
//!
//! # Pacing
//!
//! When a suggestion shows, as opposed to which one, follows the user's
//! [`SuggestionMode`]: in Quiet, the default, after a pause in typing, and
//! less often for words the user types past. See [`pacing`].

mod learned;
pub(crate) mod local;
mod model;
mod pacing;
mod typed;
pub(crate) mod vocabulary;

pub use pacing::SuggestionMode;
pub(crate) use pacing::SuggestionPacing;

use crate::classify::{TextContext, classify_context};
use crate::text::floor_char_boundary;
use gpui::{App, Entity, KeyBinding, Subscription, Task, Window};
use gpui_base::input::{
    AcceptSuggestionWord, Rope, RopeExt as _, Suggestion, SuggestionEvent, SuggestionProvider,
    SuggestionRequest, TextChange, TextareaState,
};
use learned::{LearnedSentences, typed_copies};
use local::{Held, LocalIndex, WordTally, tally};
use model::generate_suggestion;
use pacing::Pacer;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use typed::TypedText;
use vocabulary::{Counts, Vocabulary};

/// Current persisted model schema and tokenization version. Version 3 adds
/// the clause ends; its n-grams are version 2's.
const MODEL_VERSION: u32 = 3;

/// The first version whose n-grams this one reads. Earlier files keep only
/// their words.
const FIRST_NGRAM_VERSION: u32 = 2;

/// Minimum length of an ordinary learnable word.
const MIN_WORD_LEN: usize = 2;

/// Maximum length of a learnable word.
const MAX_WORD_LEN: usize = 60;

/// Persisted vocabulary size cap.
const VOCAB_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The most occurrences of one word the open document adds to the
/// vocabulary's count, so one long document doesn't swamp it.
const LOCAL_COUNT_CAP: u32 = 50;

/// Maximum continuation words appended after completing a typed prefix.
const PREFIX_CONTINUATION_LIMIT: usize = 2;

/// Maximum continuation words appended after an empty-prefix prediction.
const EMPTY_PREDICTION_CONTINUATION_LIMIT: usize = 1;

/// Committed occurrences required before a non-dictionary word is suggested.
const NON_DICTIONARY_MIN_SHARED_FREQ: u32 = 3;

/// In-document occurrences required before a non-dictionary word is suggested.
const NON_DICTIONARY_MIN_LOCAL_FREQ: u32 = 2;

/// Backward scan bound when extracting a committed sentence.
const SENTENCE_SCAN_BYTES: usize = 1024;

/// Backward scan bound when locating the most recent terminator.
const TERMINATOR_SEARCH_BYTES: usize = 4096;

/// Backward scan bound when extracting previous-word context.
const CONTEXT_BYTES: usize = 512;

/// The largest text whose words are counted on the UI thread, in about
/// seven milliseconds. A larger one is counted on another thread, and its
/// words join the suggestions when that is done.
const LOCAL_COUNT_ON_UI_BYTES: usize = 64 * 1024;

/// The key that takes the next word of a suggestion, as in VS Code:
/// Cmd+Right on macOS and Ctrl+Right elsewhere, in every text input.
///
/// With no suggestion offered, [`AcceptSuggestionWord`] passes the key on to
/// the next binding, so it moves as the keymap has it move, such as to the
/// end of the word. Bind these after `gpui_kit::init`, so they are tried
/// first.
pub fn accept_word_bindings() -> Vec<KeyBinding> {
    let key = if cfg!(target_os = "macos") {
        "cmd-right"
    } else {
        "ctrl-right"
    };
    vec![KeyBinding::new(key, AcceptSuggestionWord, Some("Input"))]
}

/// Canonicalizes safe single-character prediction tokens.
fn canonical_prediction_word(word: &str) -> &str {
    if word.eq_ignore_ascii_case("a") {
        "a"
    } else if word.eq_ignore_ascii_case("i") {
        "I"
    } else {
        word
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

/// Whether the character `ch` at byte `index` of `text` belongs to a word: a
/// word character, or an apostrophe between two letters, so contractions and
/// possessives ("don't", "I'm", "Sam's") stay one word. The typographic
/// apostrophe counts too.
fn is_word_char_at(text: &str, index: usize, ch: char) -> bool {
    if is_word_char(ch) {
        return true;
    }
    (ch == '\'' || ch == '\u{2019}')
        && text[..index]
            .chars()
            .next_back()
            .is_some_and(char::is_alphabetic)
        && text[index + ch.len_utf8()..]
            .chars()
            .next()
            .is_some_and(char::is_alphabetic)
}

/// The end of the word that starts at byte `start` of `text`.
fn word_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|&(index, character)| !is_word_char_at(text, start + index, character))
        .map_or(text.len(), |(index, _)| start + index)
}

fn is_sentence_terminator(ch: char) -> bool {
    ch == '.' || ch == '!' || ch == '?' || ch == '\n'
}

/// Returns whether a token belongs in the learned word and n-gram stream.
fn is_learnable_word(word: &str) -> bool {
    let character_count = word.chars().count();

    if character_count == 1 {
        return matches!(word, "a" | "A" | "I");
    }

    (MIN_WORD_LEN..=MAX_WORD_LEN).contains(&character_count)
        && word.chars().any(|character| character.is_alphabetic())
}

/// Commits a token to the current n-gram sequence or breaks the sequence when
/// the token is not learnable.
fn commit_extracted_word<'a>(
    text: &'a str,
    start: usize,
    end: usize,
    sequences: &mut Vec<Vec<&'a str>>,
    current: &mut Vec<&'a str>,
) {
    let word = &text[start..end];

    if is_learnable_word(word) {
        current.push(word);
    } else if !current.is_empty() {
        sequences.push(std::mem::take(current));
    }
}

/// Extracts learnable words grouped into contiguous n-gram sequences.
fn extract_sentences_and_words(text: &str, cursor_offset: Option<usize>) -> Vec<Vec<&str>> {
    let mut sequences = Vec::new();
    let mut current = Vec::new();
    let mut word_start = None;

    for (index, character) in text.char_indices() {
        if is_word_char_at(text, index, character) {
            if word_start.is_none() {
                word_start = Some(index);
            }
            continue;
        }

        if let Some(start) = word_start.take() {
            let skip = cursor_offset.is_some_and(|cursor| cursor >= start && cursor <= index);

            if skip {
                if !current.is_empty() {
                    sequences.push(std::mem::take(&mut current));
                }
            } else {
                commit_extracted_word(text, start, index, &mut sequences, &mut current);
            }
        }

        if is_sentence_terminator(character) && !current.is_empty() {
            sequences.push(std::mem::take(&mut current));
        }
    }

    if let Some(start) = word_start {
        let end = text.len();
        let skip = cursor_offset.is_some_and(|cursor| cursor >= start && cursor <= end);

        if skip {
            if !current.is_empty() {
                sequences.push(std::mem::take(&mut current));
            }
        } else {
            commit_extracted_word(text, start, end, &mut sequences, &mut current);
        }
    }

    if !current.is_empty() {
        sequences.push(current);
    }

    sequences
}

/// The words of `text`, as byte ranges.
fn word_tokens(text: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if is_word_char_at(text, index, character) {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            tokens.push(start..index);
        }
    }
    if let Some(start) = start {
        tokens.push(start..text.len());
    }
    tokens
}

/// Whether the text between two words, or after the last word of a
/// sentence, ends a clause: anything but whitespace, such as a comma, a
/// colon, a dash, a bracket, a quote or a slash.
fn is_clause_break(gap: &str) -> bool {
    gap.chars().any(|character| !character.is_whitespace())
}

/// Words written one after another in a sentence, with nothing but spaces
/// between them.
struct Run<'a> {
    /// Whether the run opens the sentence.
    opens: bool,
    words: Vec<&'a str>,
    /// Whether punctuation follows the run's last word, such as a comma.
    clause_end: bool,
}

/// The learnable words of a sentence, in runs. A token that isn't
/// learnable, such as a number, ends a run, and so does punctuation between
/// two words: "Apples, oranges" never teaches that "oranges" follows
/// "apples".
fn sentence_runs(sentence: &str) -> Vec<Run<'_>> {
    let tokens = word_tokens(sentence);
    let mut runs = Vec::new();
    let mut current = Vec::new();
    let mut current_opens = false;
    for (index, range) in tokens.iter().enumerate() {
        let word = &sentence[range.clone()];
        if is_learnable_word(word) {
            if current.is_empty() {
                current_opens = index == 0;
            }
            current.push(word);
        } else if !current.is_empty() {
            runs.push(Run {
                opens: current_opens,
                words: std::mem::take(&mut current),
                clause_end: false,
            });
        }
        let gap_end = tokens
            .get(index + 1)
            .map_or(sentence.len(), |next| next.start);
        if !current.is_empty() && is_clause_break(&sentence[range.end..gap_end]) {
            runs.push(Run {
                opens: current_opens,
                words: std::mem::take(&mut current),
                clause_end: true,
            });
        }
    }
    if !current.is_empty() {
        runs.push(Run {
            opens: current_opens,
            words: current,
            clause_end: false,
        });
    }
    runs
}

fn extract_prefix(text: &str, offset: usize) -> &str {
    if offset == 0 || offset > text.len() {
        return "";
    }

    let before = &text[..offset];
    let start = before
        .char_indices()
        .rev()
        .take_while(|&(index, character)| is_word_char_at(before, index, character))
        .last()
        .map(|(index, _)| index)
        .unwrap_or(offset);

    &text[start..offset]
}

/// Returns the previous word and the word before it without crossing a
/// sentence boundary.
fn previous_two_words(text: &str, from: usize) -> (Option<&str>, Option<&str>) {
    if from == 0 || from > text.len() {
        return (None, None);
    }

    let before = &text[..from];
    let sentence_start = before
        .rfind(|character: char| is_sentence_terminator(character))
        .map(|index| index + 1)
        .unwrap_or(0);

    let mut area = &before[sentence_start..];

    if area.len() > CONTEXT_BYTES {
        let mut boundary = area.len() - CONTEXT_BYTES;
        while !area.is_char_boundary(boundary) {
            boundary += 1;
        }
        area = &area[boundary..];
    }

    let mut last = None;
    let mut previous = None;
    let mut current = None;

    for (index, character) in area.char_indices() {
        if is_word_char_at(area, index, character) {
            if current.is_none() {
                current = Some(index);
            }
        } else if let Some(start) = current.take() {
            previous = last;
            last = Some((start, index));
        }
    }

    if let Some(start) = current {
        previous = last;
        last = Some((start, area.len()));
    }

    (
        last.map(|(start, end)| &area[start..end]),
        previous.map(|(start, end)| &area[start..end]),
    )
}

/// Checks whether any non-whitespace content exists after the cursor on the
/// current line.
fn has_text_after_cursor_on_line(text: &str, offset: usize) -> bool {
    if offset >= text.len() {
        return false;
    }

    text[offset..]
        .chars()
        .take_while(|&character| character != '\n')
        .any(|character| !character.is_whitespace())
}

fn is_closing_punctuation(character: char) -> bool {
    matches!(
        character,
        ')' | ']' | '}' | '\'' | '"' | '\u{2019}' | '\u{201D}'
    )
}

/// Checks whether a closing enclosure appears before the prefix start.
fn has_closing_punctuation_before(text: &str, prefix_start: usize) -> bool {
    if prefix_start == 0 {
        return false;
    }

    for character in text[..prefix_start].chars().rev() {
        if character.is_whitespace() {
            continue;
        }

        return is_closing_punctuation(character);
    }

    false
}

/// Word suggestions for one textarea, from the vocabulary its [`crate::Writing`]
/// shares, offered as ghost text after the caret.
///
/// Make one per textarea with [`crate::Writing::word_suggestions`], install
/// it with `TextareaState::suggestion_provider`, and keep the subscription
/// [`Self::follow`] returns, so Tab and Escape pace what comes next:
///
/// ```ignore
/// let suggestions = writing.word_suggestions();
/// let textarea = cx.new(|cx| {
///     TextareaState::new(window, cx)
///         .suggestion_provider(suggestions.clone())
///         .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
/// });
/// let _pacing = suggestions.follow(&textarea, cx);
/// ```
pub struct WordSuggestions {
    vocabulary: Rc<RefCell<Vocabulary>>,
    local_index: RefCell<LocalIndex>,
    /// The document's words being counted on another thread.
    counting: RefCell<Option<Counting>>,
    /// The vocabulary's generation the document's extra counts were worked
    /// out at.
    vocabulary_generation: Cell<u64>,
    learned: RefCell<LearnedSentences>,
    /// The words this document taught the shared vocabulary.
    taught: RefCell<WordTally>,
    /// The words the document opened with.
    opened: RefCell<WordTally>,
    typed: RefCell<TypedText>,
    pacer: Pacer,
}

impl WordSuggestions {
    pub(crate) fn new(vocabulary: Rc<RefCell<Vocabulary>>, pacing: Rc<SuggestionPacing>) -> Self {
        Self {
            vocabulary,
            local_index: RefCell::new(LocalIndex::new()),
            counting: RefCell::new(None),
            vocabulary_generation: Cell::new(0),
            learned: RefCell::new(LearnedSentences::default()),
            taught: RefCell::new(WordTally::new()),
            opened: RefCell::new(WordTally::new()),
            typed: RefCell::new(TypedText::default()),
            pacer: Pacer::new(pacing),
        }
    }

    /// Notes the text the textarea opened with, such as a document read from
    /// disk. It isn't the user's typing in this session, so none of it is
    /// learned.
    pub fn opened_with(&self, text: &str) {
        self.learned.borrow_mut().opened_with(text);
        *self.opened.borrow_mut() = tally(text);
        self.with_held(|held| self.local_index.borrow_mut().update_all_extra(held));
    }

    /// Calls `f` with how many occurrences of a folded word the shared
    /// vocabulary may hold already: those this document taught it, and those
    /// the document opened with, up to the vocabulary's count of the word.
    fn with_held<R>(&self, f: impl FnOnce(Held) -> R) -> R {
        let vocabulary = self.vocabulary.borrow();
        let taught = self.taught.borrow();
        let opened = self.opened.borrow();
        f(&|folded: &str| {
            let opened = opened.get(folded).copied().unwrap_or(0);
            taught.get(folded).copied().unwrap_or(0)
                + opened.min(vocabulary.counts().word_count(folded))
        })
    }

    /// Brings the count of the document's words to `text`, with the word at
    /// `caret` left out. The first count of a large text runs on another
    /// thread, and the words join once it is done.
    pub(crate) fn count_words(&self, text: &Rope, caret: usize, cx: &App) {
        let mut counting = self.counting.borrow_mut();
        let counted = counting
            .as_ref()
            .and_then(|counting| counting.result.try_recv().ok());
        if let Some(counts) = counted
            && let Some(finished) = counting.take()
        {
            finished.install(counts, &mut self.local_index.borrow_mut(), self);
        }
        if counting.is_some() {
            return;
        }
        let mut index = self.local_index.borrow_mut();
        let caret = Some(caret);
        let generation = self.vocabulary.borrow().generation();
        if self.vocabulary_generation.replace(generation) != generation {
            // What the vocabulary holds of the document's words may have
            // changed.
            self.with_held(|held| index.update_all_extra(held));
        }
        if index
            .text()
            .is_some_and(|counted| counted.len() == text.len())
        {
            self.with_held(|held| index.move_caret(caret, held));
        } else if text.len() <= LOCAL_COUNT_ON_UI_BYTES {
            self.with_held(|held| index.build(text, caret, held));
        } else {
            index.clear();
            *counting = Some(Counting::start(text.clone(), caret, cx));
        }
    }

    /// Hears that the user took a suggestion with Tab or closed it with
    /// Escape. [`Self::follow`] forwards a textarea's events here.
    pub fn suggestion_event(&self, event: &SuggestionEvent, cx: &App) {
        self.pacer.event(event, cx.background_executor().now());
    }

    /// Forwards `textarea`'s [`SuggestionEvent`]s to
    /// [`Self::suggestion_event`] for as long as the subscription is kept.
    pub fn follow(self: &Rc<Self>, textarea: &Entity<TextareaState>, cx: &mut App) -> Subscription {
        let this = self.clone();
        cx.subscribe(textarea, move |_, event: &SuggestionEvent, cx| {
            this.suggestion_event(event, cx)
        })
    }

    /// What a request for the suggestions at `offset` in `text` offers.
    pub(crate) fn suggest_in(&self, text: &Rope, offset: usize, now: Instant) -> Option<String> {
        let excerpt = Excerpt::around(text, offset);
        self.suggest(text, &excerpt, excerpt.offset_in(offset), now)
    }

    /// Follows `edits`, ranges and the text that replaced them, in the
    /// count of the document's words. The text is `len` bytes long after
    /// them.
    pub(crate) fn count_edits<'a>(
        &self,
        edits: impl IntoIterator<Item = (Range<usize>, &'a str)>,
        len: usize,
    ) {
        if let Some(counting) = self.counting.borrow_mut().as_mut() {
            counting.edits.extend(
                edits
                    .into_iter()
                    .map(|(range, text)| (range, text.to_string())),
            );
            return;
        }
        let mut index = self.local_index.borrow_mut();
        if index.text().is_none() {
            return;
        }
        self.with_held(|held| {
            for (range, text) in edits {
                index.edit(range, text, held);
            }
        });
        // An edit it didn't hear of: count again at the next request.
        if index.text().map(Rope::len) != Some(len) {
            index.clear();
        }
    }

    /// Learns the sentence before `caret` in `text`, after a typed key.
    pub(crate) fn learn_before(&self, text: &Rope, caret: usize) {
        let excerpt = Excerpt::around(text, caret);
        self.learn_preceding_sentence(text, &excerpt, excerpt.offset_in(caret));
    }

    /// The suggestion to offer at `offset` in `excerpt` of `whole`, if any:
    /// learns the sentence the caret just left, asks the model, and lets
    /// pacing decide whether it shows now.
    fn suggest(
        &self,
        whole: &Rope,
        excerpt: &Excerpt,
        offset: usize,
        now: Instant,
    ) -> Option<String> {
        self.learn_preceding_sentence(whole, excerpt, offset);

        let text = excerpt.text.as_str();
        let suggestion = {
            let vocabulary = self.vocabulary.borrow();
            let local = self.local_index.borrow();

            generate_suggestion(&vocabulary, &local, text, offset)
        };
        let caret = excerpt.start + offset;
        let suggestion = self.pacer.offer(text, offset, caret, suggestion, now);
        self.typed
            .borrow_mut()
            .offered(caret, suggestion.as_deref());
        suggestion
    }

    /// Learns the most recently finished prose sentence before `offset` in
    /// `excerpt` of `whole`, when the user typed all of it and it isn't
    /// learned already.
    fn learn_preceding_sentence(&self, whole: &Rope, excerpt: &Excerpt, offset: usize) {
        let text = excerpt.text.as_str();
        let Some((range, terminator_offset)) = preceding_sentence(text, offset) else {
            return;
        };
        let sentence = &text[range.clone()];
        let in_whole = excerpt.start + range.start..excerpt.start + range.end;

        {
            let typed = self.typed.borrow();
            if typed.overlaps_untyped(in_whole.clone())
                || self
                    .learned
                    .borrow_mut()
                    .is_learned(sentence, in_whole.end, |limit| {
                        // Only when a sentence learned before turns up
                        // somewhere else, which needs all of the text.
                        typed_copies(&whole.to_string(), sentence, &typed, limit)
                    })
            {
                return;
            }
        }

        if classify_context(text, terminator_offset) == TextContext::Code {
            return;
        }

        let words = self.vocabulary.borrow_mut().learn_sentence(sentence);
        if words.is_empty() {
            return;
        }

        {
            let mut taught = self.taught.borrow_mut();
            for word in &words {
                *taught.entry(word.as_str().into()).or_default() += 1;
            }
        }
        self.learned.borrow_mut().learned(sentence, in_whole.end);
        // The index counted the sentence as the document's own.
        self.with_held(|held| {
            self.local_index
                .borrow_mut()
                .update_extra(words.iter().map(String::as_str), held)
        });
    }
}

/// How much of the text before the caret's line an [`Excerpt`] holds: more
/// than anything a suggestion reads back from the caret, which is at most
/// [`TERMINATOR_SEARCH_BYTES`] and [`SENTENCE_SCAN_BYTES`] to the start of the
/// sentence learned, and a few kilobytes above a line to classify it as
/// prose or code.
const EXCERPT_BEFORE_BYTES: usize = 16 * 1024;

/// How much of the text after the caret's line an [`Excerpt`] holds, for
/// the lines below that classifying reads.
const EXCERPT_AFTER_BYTES: usize = 2 * 1024;

/// The text around the caret, copied out of the textarea's rope: whole
/// lines from [`EXCERPT_BEFORE_BYTES`] before the caret's line to
/// [`EXCERPT_AFTER_BYTES`] after it. A suggestion reads no further, and
/// starting at a line keeps the start of the excerpt the start of a
/// sentence, as the line break before it ends one.
struct Excerpt {
    text: String,
    /// Where `text` starts in the whole text.
    start: usize,
}

impl Excerpt {
    fn around(text: &Rope, offset: usize) -> Self {
        let offset = offset.min(text.len());
        let row = text.offset_to_point(offset).row;
        let line_start = text.line_start_offset(row);
        let line_end = text.line_end_offset(row);
        let start = text.line_start_offset(
            text.offset_to_point(line_start.saturating_sub(EXCERPT_BEFORE_BYTES))
                .row,
        );
        let end = text.line_end_offset(
            text.offset_to_point((line_end + EXCERPT_AFTER_BYTES).min(text.len()))
                .row,
        );
        Self {
            text: text.slice(start..end).to_string(),
            start,
        }
    }

    /// Where byte `offset` of the whole text is in the excerpt, on a
    /// character boundary.
    fn offset_in(&self, offset: usize) -> usize {
        let offset = offset.saturating_sub(self.start).min(self.text.len());
        floor_char_boundary(&self.text, offset)
    }
}

/// A count of the document's words on another thread, and the edits made
/// since it began.
struct Counting {
    text: Rope,
    caret: Option<usize>,
    result: mpsc::Receiver<Counts>,
    /// The edits since, as ranges and the text that replaced them.
    edits: Vec<(Range<usize>, String)>,
    _task: Task<()>,
}

impl Counting {
    fn start(text: Rope, caret: Option<usize>, cx: &App) -> Self {
        let (sender, result) = mpsc::channel();
        let counted = text.clone();
        let task = cx.background_executor().spawn(async move {
            let _ = sender.send(local::count(&counted, caret));
        });
        Self {
            text,
            caret,
            result,
            edits: Vec::new(),
            _task: task,
        }
    }

    /// Takes `counts`, its result, into `index`, and the edits made since.
    fn install(self, counts: Counts, index: &mut LocalIndex, suggestions: &WordSuggestions) {
        suggestions.with_held(|held| {
            index.install(counts, self.text, self.caret, held);
            for (range, text) in self.edits {
                index.edit(range, &text, held);
            }
        });
    }
}

/// The last finished sentence before `offset`, as its trimmed byte range and
/// the offset of the terminator after it.
///
/// A sentence ends at a terminator: `.`, `!`, `?` or a line break. Empty
/// stretches between terminators are passed over, so the line before a blank
/// line counts as finished.
fn preceding_sentence(text: &str, offset: usize) -> Option<(Range<usize>, usize)> {
    let search_floor = floor_char_boundary(text, offset.saturating_sub(TERMINATOR_SEARCH_BYTES));
    let mut end = offset;
    loop {
        let terminator = search_floor + text[search_floor..end].rfind(is_sentence_terminator)?;
        let scan_floor = floor_char_boundary(text, terminator.saturating_sub(SENTENCE_SCAN_BYTES));
        let start = text[scan_floor..terminator]
            .rfind(is_sentence_terminator)
            .map_or(scan_floor, |index| scan_floor + index + 1);
        let segment = &text[start..terminator];
        let sentence = segment.trim();
        if !sentence.is_empty() {
            let sentence_start = start + segment.len() - segment.trim_start().len();
            return Some((sentence_start..sentence_start + sentence.len(), terminator));
        }
        end = terminator;
    }
}

/// Offers one word, or a short run of words, to continue the text at the
/// caret. The textarea shows it as ghost text that Tab accepts.
impl SuggestionProvider for WordSuggestions {
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<Suggestion>>> {
        if self.pacer.mode() == SuggestionMode::Off {
            return Task::ready(Ok(Vec::new()));
        }

        self.count_words(request.text(), request.offset(), cx);
        let suggestion = self.suggest_in(
            request.text(),
            request.offset(),
            cx.background_executor().now(),
        );
        Task::ready(Ok(suggestion.into_iter().map(Suggestion::new).collect()))
    }

    fn is_trigger(&self, offset: usize, text: &str, cx: &mut App) -> bool {
        self.typed.borrow_mut().typed(offset, text);
        let triggers = self.pacer.mode() != SuggestionMode::Off
            && text.chars().any(|character| {
                character.is_alphanumeric()
                    || character == '_'
                    || character == '-'
                    || character == ' '
                    || is_sentence_terminator(character)
            });
        self.pacer
            .typed(offset, text, triggers, cx.background_executor().now());
        triggers
    }

    /// Quiet waits for typing to pause, longer after ignored suggestions,
    /// but not while the user types along the suggestion on screen. Eager
    /// never waits.
    fn debounce(&self) -> Duration {
        self.pacer.debounce()
    }

    /// Notes which inserted text the user didn't type, in every mode.
    ///
    /// A request learns the sentence before the caret, and Eager makes one
    /// after every keystroke that triggers. Quiet makes one only once typing
    /// pauses, so it learns after those keystrokes here instead. It learns
    /// what Eager would, and at the same moments: after a typed key, never
    /// at a paste, an undo or another edit the user didn't type.
    fn did_change(&self, change: &TextChange, cx: &mut App) {
        self.typed.borrow_mut().changed(change);
        self.count_edits(
            change
                .edits()
                .iter()
                .map(|edit| (edit.range(), edit.text())),
            change.text().len(),
        );
        let now = cx.background_executor().now();
        if let Some(caret) = self.pacer.changed(change, now) {
            self.learn_before(change.text(), caret);
        }
    }
}

#[cfg(test)]
mod tests;
