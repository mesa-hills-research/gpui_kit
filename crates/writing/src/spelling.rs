//! The spelling dictionary and the spell checker a textarea asks.
//!
//! # Dictionary
//!
//! A [`Dictionary`] holds the words of its [`WordList`]s and of the user's
//! own word list in the data folder. When the lists hold too few words to be
//! a dictionary it reports itself as not loaded, and every correctness query
//! answers permissively.
//!
//! Words may be added at runtime with [`Dictionary::add_word`], which also
//! appends them to the user's word list on a background thread, or ignored
//! until the application quits with [`Dictionary::ignore_word`]. Both bump a generation counter so
//! scanners know to drop caches computed against the old dictionary.
//!
//! # Scanner
//!
//! The scanner finds the misspelled words of a text, designed to run
//! debounced on the UI thread:
//!
//! - Results are cached per line, keyed by a hash of the line's content, so
//!   an edit re-tokenizes only the changed line.
//! - Code is skipped using the code/prose line classifier the word
//!   suggestions share, Markdown fence tracking, and neighborhood smoothing.
//! - Token rules follow a safe bias: identifiers, ALL-CAPS, camelCase,
//!   digit-adjacent tokens, paths, URLs, inline-code and tag-adjacent tokens
//!   are skipped rather than risk false squiggles.
//!
//! # Suggestion ranking
//!
//! Candidates are ranked by an edit distance weighted for typing mistakes:
//! doubling or undoubling a letter, swapping two neighbours (two vowels
//! least, as in "wierd") and confusing two vowels cost less than other edits,
//! so "mispeled" suggests "misspelled" first. Changing the first letter
//! (typos rarely do) and offering a proper noun for a lowercase word add to
//! the cost, and a longer shared beginning and a closer length break ties.
//! The word lists carry no word frequencies.
//!
//! # In a textarea
//!
//! [`DictionaryChecker`] is a textarea's [`SpellChecker`]: the textarea
//! decides when to check and draws the underlines and the fixes, and the
//! dictionary and scanner here decide what is misspelled and what to offer.

use crate::WordList;
use crate::classify::line_code_score;
use gpui::{App, SharedString, Task};
use gpui_base::input::{Rope, RopeExt as _, SpellCheck, SpellCheckRequest, SpellChecker};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{self, Write as _};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Idle time after the last edit before a document is re-checked.
const SPELL_CHECK_DEBOUNCE: Duration = Duration::from_millis(400);

/// Minimum number of entries for a wordlist to be considered a usable
/// dictionary. Guards against a lone user-dictionary flagging everything.
const MIN_USEFUL_DICTIONARY_SIZE: usize = 10_000;

/// Longest wordlist entry accepted at load time.
const MAX_DICTIONARY_WORD_LEN: usize = 48;

/// Most replacements offered for a misspelled word.
pub(crate) const MAX_SUGGESTIONS: usize = 5;

/// Largest weighted edit distance between a typo and a suggestion.
const MAX_SUGGESTION_COST: f32 = 2.0;

/// Largest difference in length, in characters, between a typo and a
/// suggestion.
const MAX_SUGGESTION_LENGTH_DIFFERENCE: usize = 3;

/// Cost of doubling or undoubling a letter: "untill", "ocured".
const DOUBLED_LETTER_COST: f32 = 0.4;

/// Cost of swapping two neighbouring letters: "teh", "hte".
const TRANSPOSITION_COST: f32 = 0.6;

/// Cost of swapping two neighbouring vowels, as in "i before e": "wierd",
/// "recieve".
const VOWEL_TRANSPOSITION_COST: f32 = 0.5;

/// Cost of writing one vowel for another: "seperate", "definately".
const VOWEL_COST: f32 = 0.75;

/// Cost of a missing or extra apostrophe: "dont".
const APOSTROPHE_COST: f32 = 0.5;

/// Added when a suggestion changes the first letter.
const FIRST_LETTER_COST: f32 = 0.5;

/// Added when a lowercase word would become a proper noun.
const CASE_COST: f32 = 0.5;

/// Cost of a suggestion that differs from the word only in case: "english".
const CASE_ONLY_COST: f32 = 0.3;

/// Documents larger than this are not spell checked.
const MAX_SPELL_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

/// Shortest token worth checking; also silences two-letter jargon (cx, px).
const MIN_CHECK_TOKEN_CHARS: usize = 3;

/// Longest token worth checking.
const MAX_CHECK_TOKEN_CHARS: usize = 48;

/// A line whose own code score exceeds this is never spell checked.
const LINE_CODE_SKIP_SCORE: f64 = 0.5;

/// A line whose neighborhood-smoothed code score exceeds this is skipped.
const SMOOTHED_CODE_SKIP_SCORE: f64 = 0.1;

/// A spelling dictionary: the words of some word lists, the words the user
/// added, and the words the user ignored this session.
///
/// It is shared behind an `Rc` by every textarea's [`DictionaryChecker`] and
/// by the word suggestions, which offer a word outside it only once the user
/// has typed it a few times.
pub struct Dictionary {
    words: RefCell<HashSet<Box<str>>>,
    /// The words again, by length and letter, for corrections.
    candidates: RefCell<Candidates>,
    /// Words accepted until the application quits, in lower case.
    ignored: RefCell<HashSet<Box<str>>>,
    loaded: Cell<bool>,
    has_apostrophe_entries: Cell<bool>,
    generation: Cell<u64>,
    /// The user's word list, which [`Self::add_word`] appends to.
    user_dictionary: Option<Arc<UserWordList>>,
}

/// The user's word list, which [`Dictionary::add_word`] appends to off the
/// UI thread.
struct UserWordList {
    path: PathBuf,
    /// Words added but not appended yet, in the order they were added.
    pending: Mutex<Vec<Box<str>>>,
    /// Held while appending, so words are appended in the order they were
    /// added.
    appending: Mutex<()>,
}

impl UserWordList {
    /// Appends the pending words, creating the file and its folder. Words it
    /// couldn't append stay pending, for the next word added or the next
    /// save.
    fn append_pending(&self) {
        let _appending = self
            .appending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let words = std::mem::take(&mut *self.pending());
        if words.is_empty() {
            return;
        }
        let mut lines = String::new();
        for word in &words {
            lines.push_str(word);
            lines.push('\n');
        }
        if let Err(error) = append(&self.path, &lines) {
            log::error!(
                "Couldn't add {} words to the user's word list {}: {error}. They are accepted \
                 until the application quits, and tried again with the next word added or \
                 the next save.",
                words.len(),
                self.path.display()
            );
            let mut pending = self.pending();
            let later = std::mem::replace(&mut *pending, words);
            pending.extend(later);
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Vec<Box<str>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Appends `text` to the file at `path`, creating it and its folder.
fn append(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

impl Dictionary {
    /// An empty, unloaded dictionary that accepts every word.
    pub fn empty() -> Self {
        Self::unloaded(None)
    }

    /// An empty dictionary that accepts every word until [`Self::install`]
    /// gives it its words. [`Self::add_word`] appends to the user's word
    /// list at `user_dictionary`.
    pub(crate) fn unloaded(user_dictionary: Option<PathBuf>) -> Self {
        Self {
            words: RefCell::new(HashSet::new()),
            candidates: RefCell::new(Candidates::default()),
            ignored: RefCell::new(HashSet::new()),
            loaded: Cell::new(false),
            has_apostrophe_entries: Cell::new(false),
            generation: Cell::new(0),
            user_dictionary: user_dictionary.map(|path| {
                Arc::new(UserWordList {
                    path,
                    pending: Mutex::default(),
                    appending: Mutex::default(),
                })
            }),
        }
    }

    /// Loads `word_lists` and the user's word list at `user_dictionary`, which
    /// [`Self::add_word`] then appends to, on this thread.
    #[cfg(test)]
    pub(crate) fn load(word_lists: &[WordList], user_dictionary: Option<PathBuf>) -> Self {
        let words = Words::read(word_lists, user_dictionary.as_deref());
        let dictionary = Self::unloaded(user_dictionary);
        dictionary.install(words);
        dictionary
    }

    /// Takes the words read for it, with any words added while they were
    /// read.
    pub(crate) fn install(&self, read: Words) {
        let count = {
            let mut words = self.words.borrow_mut();
            let added = std::mem::replace(&mut *words, read.words);
            let mut candidates = read.candidates;
            for word in added {
                if !words.contains(&word) {
                    candidates.add(&word);
                    words.insert(word);
                }
            }
            *self.candidates.borrow_mut() = candidates;
            words.len()
        };
        let loaded = count >= MIN_USEFUL_DICTIONARY_SIZE;
        if loaded {
            log::info!("Spell dictionary loaded ({count} words)");
        } else if count == 0 {
            log::info!("No spell dictionary found; spell checking is inactive");
        } else {
            log::warn!(
                "Wordlist too small to act as a dictionary ({count} words); spell checking is inactive"
            );
        }
        self.loaded.set(loaded);
        self.has_apostrophe_entries
            .set(read.has_apostrophe_entries || self.has_apostrophe_entries.get());
        self.generation.set(self.generation.get() + 1);
    }

    /// Whether a usable dictionary was loaded. An unloaded one accepts every
    /// word.
    pub fn is_loaded(&self) -> bool {
        self.loaded.get()
    }

    /// Number of entries in the dictionary.
    pub fn word_count(&self) -> usize {
        self.words.borrow().len()
    }

    /// Monotonic counter incremented whenever the dictionary content
    /// changes, letting scanners invalidate caches lazily.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Accepts `word` from now on, and appends it to the user's word list
    /// on a background thread. Bumps the generation so cached scan results
    /// that flagged this word are discarded on the next scan.
    ///
    /// A word the background thread hasn't appended by the time the
    /// application quits is appended by [`crate::Writing::save`].
    pub fn add_word(&self, word: &str, cx: &App) {
        let Some(word) = self.insert_word(word) else {
            return;
        };
        if let Some(list) = &self.user_dictionary {
            list.pending().push(Box::from(word));
            let list = list.clone();
            cx.background_executor()
                .spawn(async move { list.append_pending() })
                .detach();
        }
    }

    /// Accepts `word` from now on, in memory only. Returns the word as
    /// added, or `None` when it isn't a word or was known already.
    pub(crate) fn insert_word<'a>(&self, word: &'a str) -> Option<&'a str> {
        let word = word.trim();
        if word.is_empty() || word.len() > MAX_DICTIONARY_WORD_LEN {
            return None;
        }
        if !word
            .chars()
            .all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        {
            return None;
        }
        if !self.words.borrow_mut().insert(Box::from(word)) {
            return None;
        }
        self.candidates.borrow_mut().add(word);
        if word.contains('\'') {
            self.has_apostrophe_entries.set(true);
        }
        self.generation.set(self.generation.get() + 1);
        Some(word)
    }

    /// Appends the words [`Self::add_word`] added that the background thread
    /// hasn't yet, on this thread.
    pub(crate) fn save_added_words(&self) {
        if let Some(list) = &self.user_dictionary {
            list.append_pending();
        }
    }

    /// Accepts `word`, in any case, until the application quits, without
    /// saving it. Bumps the generation like [`Dictionary::add_word`].
    pub fn ignore_word(&self, word: &str) {
        let word = word.trim().to_lowercase();
        if word.is_empty() {
            return;
        }
        if self.ignored.borrow_mut().insert(Box::from(word)) {
            self.generation.set(self.generation.get() + 1);
        }
    }

    /// Checks whether a word is spelled correctly.
    ///
    /// Returns `true` when no dictionary is loaded so callers degrade
    /// gracefully. Curly apostrophes are normalized; if the wordlist has no
    /// apostrophe entries at all, contractions are treated as unknowable and
    /// pass. Hyphenated compounds are validated part by part.
    pub fn is_correct(&self, word: &str) -> bool {
        if !self.is_loaded() || word.is_empty() {
            return true;
        }
        {
            let ignored = self.ignored.borrow();
            if !ignored.is_empty() && ignored.contains(word.to_lowercase().as_str()) {
                return true;
            }
        }
        let normalized: Cow<'_, str> = if word.contains('\u{2019}') {
            Cow::Owned(word.replace('\u{2019}', "'"))
        } else {
            Cow::Borrowed(word)
        };
        let word = normalized.as_ref();
        if word.contains('\'') && !self.has_apostrophe_entries.get() {
            return true;
        }
        if word.contains('-') {
            return word
                .split('-')
                .all(|part| !part.is_empty() && self.lookup(part));
        }
        self.lookup(word)
    }

    /// Performs a case-relaxed dictionary lookup for a single token.
    fn lookup(&self, word: &str) -> bool {
        if word.chars().any(|c| c.is_ascii_digit() || c == '_') {
            return false;
        }
        let words = self.words.borrow();
        if words.contains(word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower != word && words.contains(lower.as_str()) {
            return true;
        }
        let is_all_caps = word.chars().all(|c| !c.is_lowercase());
        if is_all_caps && word.chars().count() > 1 {
            let title = capitalize(&lower);
            if words.contains(title.as_str()) {
                return true;
            }
        }
        false
    }

    /// Generates up to `limit` correction suggestions for a misspelled word,
    /// best first. See the module docs for the ranking.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        if !self.is_loaded() || word.is_empty() || limit == 0 {
            return Vec::new();
        }
        let target: Vec<char> = word.to_lowercase().chars().collect();
        if target.is_empty() || target.len() > MAX_DICTIONARY_WORD_LEN {
            return Vec::new();
        }
        let first = target.first().copied();
        let target_is_lowercase = word.chars().all(|c| !c.is_uppercase());

        let candidates = self.candidates.borrow();
        let mut matches: Vec<(f32, &str)> = Vec::new();
        let mut cand_chars: Vec<char> = Vec::new();
        let mut rows = Rows::default();
        for (cand, cand_len) in candidates.near(&target) {
            let cand_first = cand.chars().next().map(|c| c.to_ascii_lowercase());
            lowercase_into(cand, &mut cand_chars);
            let Some(mut cost) = rows.typo_distance(&target, &cand_chars, MAX_SUGGESTION_COST)
            else {
                continue;
            };
            if cost == 0.0 {
                if cand == word {
                    continue;
                }
                cost = CASE_ONLY_COST;
            }
            if cand_first != first {
                cost += FIRST_LETTER_COST;
            }
            if target_is_lowercase && cand.chars().any(|c| c.is_uppercase()) {
                cost += CASE_COST;
            }
            // Tie-breaks, each smaller than any edit: a longer shared
            // beginning and a closer length.
            let shared_prefix = target
                .iter()
                .zip(&cand_chars)
                .take_while(|(a, b)| a == b)
                .count()
                .min(4);
            cost -= 0.02 * shared_prefix as f32;
            cost += 0.03 * cand_len.abs_diff(target.len()) as f32;
            matches.push((cost, cand));
        }

        matches.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        let mut suggestions: Vec<String> = Vec::with_capacity(limit);
        for (_, candidate) in matches {
            let suggestion = match_case(word, candidate);
            if !suggestions.contains(&suggestion) {
                suggestions.push(suggestion);
            }
            if suggestions.len() == limit {
                break;
            }
        }
        suggestions
    }
}

/// Which letters a word in lower case has: a bit for each of `a` to `z`, one
/// for the apostrophe, one for the hyphen and one for every other character.
fn letters(word: &[char]) -> u32 {
    word.iter().fold(0, |letters, &c| {
        letters
            | match c {
                'a'..='z' => 1 << (c as u32 - 'a' as u32),
                '\'' | '\u{2019}' => 1 << 26,
                '-' => 1 << 27,
                _ => 1 << 28,
            }
    })
}

/// How many [`letters`] apart rule a word out as a correction of another.
/// Every edit changes the letters a word holds at a cost of at least 0.375
/// a letter (a substitution changes two for 0.75 or more, an insertion or a
/// deletion one for 0.4 or more), and each letter one word has and the other
/// lacks takes one such change at least, so six of them cost at least 2.25,
/// more than [`MAX_SUGGESTION_COST`].
const MAX_LETTERS_APART: u32 = 6;

/// `word` in lower case, as characters, in `chars`.
fn lowercase_into(word: &str, chars: &mut Vec<char>) {
    chars.clear();
    if word.is_ascii() {
        chars.extend(
            word.bytes()
                .map(|byte| char::from(byte.to_ascii_lowercase())),
        );
    } else {
        chars.extend(word.to_lowercase().chars());
    }
}

/// A dictionary's words by length and by first and last letter, so that
/// corrections look only at the words that could be one: about a
/// twentieth of an English dictionary for a word of average length.
#[derive(Default)]
struct Candidates {
    /// The words, one after another.
    text: String,
    /// Where each word is in `text`, and its [`letters`].
    words: Vec<(u32, u8, u32)>,
    /// Indices into `words` by their length in characters and their first
    /// letter, in ASCII lower case.
    by_first: HashMap<(usize, char), Vec<u32>>,
    /// The same by their last letter.
    by_last: HashMap<(usize, char), Vec<u32>>,
}

impl Candidates {
    fn add(&mut self, word: &str) {
        let Ok(length) = u8::try_from(word.len()) else {
            return;
        };
        let index = self.words.len() as u32;
        let mut lowercase = Vec::new();
        lowercase_into(word, &mut lowercase);
        self.words
            .push((self.text.len() as u32, length, letters(&lowercase)));
        self.text.push_str(word);
        let chars = word.chars().count();
        if let Some(first) = word.chars().next() {
            self.by_first
                .entry((chars, first.to_ascii_lowercase()))
                .or_default()
                .push(index);
        }
        if let Some(last) = word.chars().next_back() {
            self.by_last
                .entry((chars, last.to_ascii_lowercase()))
                .or_default()
                .push(index);
        }
    }

    /// Gives back the room its vectors grew into as the words were added:
    /// for the English list, 7.8 MB of index becomes 5 MB.
    fn shrink_to_fit(&mut self) {
        self.text.shrink_to_fit();
        self.words.shrink_to_fit();
        for indices in self.by_first.values_mut().chain(self.by_last.values_mut()) {
            indices.shrink_to_fit();
        }
        self.by_first.shrink_to_fit();
        self.by_last.shrink_to_fit();
    }

    /// The bytes the index takes on the heap, about.
    #[cfg(test)]
    fn heap_bytes(&self) -> usize {
        let map = |map: &HashMap<(usize, char), Vec<u32>>| {
            map.capacity() * 8 / 7 * (size_of::<(usize, char)>() + size_of::<Vec<u32>>() + 1)
                + map
                    .values()
                    .map(|indices| indices.capacity() * 4)
                    .sum::<usize>()
        };
        self.text.capacity()
            + self.words.capacity() * size_of::<(u32, u8, u32)>()
            + map(&self.by_first)
            + map(&self.by_last)
    }

    /// The word at `index`, and its [`letters`].
    fn word(&self, index: u32) -> (&str, u32) {
        let (start, length, letters) = self.words[index as usize];
        (
            &self.text[start as usize..start as usize + length as usize],
            letters,
        )
    }

    /// The words that could be within [`MAX_SUGGESTION_COST`] of `target`,
    /// with their length in characters, each once: those within
    /// [`MAX_SUGGESTION_LENGTH_DIFFERENCE`] characters of its length that
    /// begin with its first letter or end with its last, and that are fewer
    /// than [`MAX_LETTERS_APART`] [`letters`] apart from it.
    fn near<'a>(&'a self, target: &[char]) -> impl Iterator<Item = (&'a str, usize)> {
        let first = target.first().copied();
        let last = target.last().copied();
        let target_letters = letters(target);
        let close = move |(word, letters): (&'a str, u32)| {
            ((letters ^ target_letters).count_ones() < MAX_LETTERS_APART).then_some(word)
        };
        let length = target.len();
        let lengths = length.saturating_sub(MAX_SUGGESTION_LENGTH_DIFFERENCE)
            ..=length + MAX_SUGGESTION_LENGTH_DIFFERENCE;
        lengths.flat_map(move |chars| {
            let beginning = first
                .and_then(|first| self.by_first.get(&(chars, first)))
                .into_iter()
                .flatten()
                .filter_map(move |&index| close(self.word(index)));
            // Words that also begin with `first` came with those.
            let ending = last
                .and_then(|last| self.by_last.get(&(chars, last)))
                .into_iter()
                .flatten()
                .filter_map(move |&index| close(self.word(index)))
                .filter(move |word| word.chars().next().map(|c| c.to_ascii_lowercase()) != first);
            beginning.chain(ending).map(move |word| (word, chars))
        })
    }
}

/// The words of a dictionary's word lists, read on any thread.
pub(crate) struct Words {
    words: HashSet<Box<str>>,
    candidates: Candidates,
    has_apostrophe_entries: bool,
}

impl Words {
    /// Reads `word_lists` and the user's word list at `user_dictionary`.
    pub(crate) fn read(word_lists: &[WordList], user_dictionary: Option<&Path>) -> Self {
        let mut texts: Vec<Cow<'static, str>> =
            word_lists.iter().filter_map(WordList::read).collect();
        if let Some(path) = user_dictionary
            && let Ok(content) = std::fs::read_to_string(path)
        {
            texts.push(Cow::Owned(content));
        }
        let mut words: HashSet<Box<str>> = HashSet::new();
        for content in &texts {
            Self::insert_wordlist(content, &mut words);
        }
        let has_apostrophe_entries = words.iter().any(|word| word.contains('\''));
        let mut candidates = Candidates::default();
        for word in &words {
            candidates.add(word);
        }
        candidates.shrink_to_fit();
        Self {
            words,
            candidates,
            has_apostrophe_entries,
        }
    }

    /// Parses a wordlist, stripping Hunspell affix flags and skipping
    /// comments, blanks, and malformed entries.
    fn insert_wordlist(content: &str, into: &mut HashSet<Box<str>>) {
        for line in content.lines() {
            let entry = line.trim();
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            let word = entry.split('/').next().unwrap_or(entry).trim();
            if word.is_empty() || word.len() > MAX_DICTIONARY_WORD_LEN {
                continue;
            }
            if !word
                .chars()
                .all(|c| c.is_alphabetic() || c == '\'' || c == '-')
            {
                continue;
            }
            into.insert(Box::from(word));
        }
    }
}

/// One textarea's spell checker, against a shared [`Dictionary`].
///
/// Make one per textarea with [`crate::Writing::spell_checker`]: its line
/// cache follows that textarea's text.
pub struct DictionaryChecker {
    dictionary: Rc<Dictionary>,
    scanner: RefCell<SpellScanner>,
}

impl DictionaryChecker {
    pub(crate) fn new(dictionary: Rc<Dictionary>) -> Self {
        Self {
            dictionary,
            scanner: RefCell::new(SpellScanner::new()),
        }
    }

    /// Checks the lines `ranges` cover, and the lines whose code score the
    /// edits there may have changed. See [`SpellScanner::check`].
    pub(crate) fn check_text(&self, text: &Rope, ranges: &[Range<usize>]) -> SpellCheck {
        self.scanner
            .borrow_mut()
            .check(&self.dictionary, text, ranges)
    }
}

impl SpellChecker for DictionaryChecker {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<anyhow::Result<SpellCheck>> {
        Task::ready(Ok(self.check_text(request.text(), request.ranges())))
    }

    fn suggestions(&self, word: &str, _: &mut App) -> Vec<SharedString> {
        self.dictionary
            .suggest(word, MAX_SUGGESTIONS)
            .into_iter()
            .map(SharedString::from)
            .collect()
    }

    fn add_to_dictionary(&self, word: &str, cx: &mut App) {
        self.dictionary.add_word(word, cx);
    }

    /// Ignored words stay ignored in every textarea until the application
    /// quits.
    fn ignore(&self, word: &str, _: &mut App) {
        self.dictionary.ignore_word(word);
    }

    fn debounce(&self) -> Duration {
        SPELL_CHECK_DEBOUNCE
    }
}

/// Cached analysis for one unique line content.
struct CachedLine {
    code_score: f64,
    is_fence: bool,
    /// Byte ranges of the misspelled words within the line.
    misspellings: Option<Rc<Vec<Range<usize>>>>,
}

/// Per-row working record for one scan pass.
struct LineRecord {
    hash: u64,
    code_score: f64,
    is_fence: bool,
    content: String,
}

/// Incremental, cache-backed document spell scanner. One instance per
/// document so caches track that document's content.
pub(crate) struct SpellScanner {
    cache: HashMap<u64, CachedLine>,
    dictionary_generation: u64,
    /// How many fence lines the text had at the last check, once it was
    /// checked whole.
    fences: Option<usize>,
}

/// How many lines on each side of a line its smoothed code score reads.
const SMOOTHING_MARGIN: usize = 2;

impl SpellScanner {
    pub(crate) fn new() -> Self {
        Self {
            cache: HashMap::new(),
            dictionary_generation: 0,
            fences: None,
        }
    }

    /// Checks the lines `ranges` cover, and the [`SMOOTHING_MARGIN`] lines
    /// on each side, whose smoothed code score an edit in them changes.
    ///
    /// The textarea asks for the lines edited since the last check, so the
    /// others are as they were. Whether a line is inside a fenced code block
    /// depends on every fence above it, so the whole text is checked when
    /// the number of fences changed, when a requested line is a fence, when
    /// the dictionary changed, and at the first check. It is also checked
    /// whole when the request covers a quarter of it, and when the lines
    /// edited away fill the cache.
    pub(crate) fn check(
        &mut self,
        dictionary: &Dictionary,
        text: &Rope,
        ranges: &[Range<usize>],
    ) -> SpellCheck {
        let whole = |this: &mut Self| SpellCheck {
            checked: vec![0..text.len()],
            misspelled: this.scan(dictionary, text),
        };
        if !dictionary.is_loaded()
            || text.len() == 0
            || text.len() > MAX_SPELL_DOCUMENT_BYTES
            || dictionary.generation() != self.dictionary_generation
        {
            return whole(self);
        }
        let Some(fences) = self.fences else {
            return whole(self);
        };
        let lines = text.lines_len();
        // A check of the whole text also drops the lines edited away from
        // the cache.
        if fence_lines(text, 0..text.len()) != fences || self.cache.len() > 2 * lines + 1024 {
            return whole(self);
        }

        // The requested rows, and those around them, merged.
        let mut spans: Vec<Range<usize>> = Vec::new();
        for range in ranges {
            let first = text.offset_to_point(range.start).row;
            let last = text
                .offset_to_point(range.end.saturating_sub(1).max(range.start))
                .row;
            if (first..=last).any(|row| is_fence(text, row)) {
                return whole(self);
            }
            let span =
                first.saturating_sub(SMOOTHING_MARGIN)..(last + 1 + SMOOTHING_MARGIN).min(lines);
            match spans.last_mut() {
                Some(previous) if span.start <= previous.end => {
                    previous.end = previous.end.max(span.end);
                }
                _ => spans.push(span),
            }
        }
        let checked_rows: usize = spans.iter().map(|span| span.len()).sum();
        if checked_rows * 4 > lines {
            return whole(self);
        }

        let mut misspelled = Vec::new();
        let mut counted_to = 0;
        let mut fences_above = 0;
        for span in &spans {
            // Rows whose code scores the span's smoothing reads.
            let scored = span.start.saturating_sub(SMOOTHING_MARGIN)
                ..(span.end + SMOOTHING_MARGIN).min(lines);
            let scored_start = text.line_start_offset(scored.start);
            fences_above += fence_lines(text, counted_to..scored_start);
            counted_to = scored_start;
            let records: Vec<LineRecord> = scored
                .clone()
                .map(|row| self.record(text.slice_line(row).to_string()))
                .collect();
            let mut in_fence = fences_above % 2 == 1;
            for (index, record) in records.iter().enumerate() {
                let row = scored.start + index;
                if record.is_fence {
                    in_fence = !in_fence;
                    continue;
                }
                if !span.contains(&row)
                    || in_fence
                    || record.code_score > LINE_CODE_SKIP_SCORE
                    || smoothed_code_score(&records, index) > SMOOTHED_CODE_SKIP_SCORE
                {
                    continue;
                }
                let line_start = text.line_start_offset(row);
                misspelled.extend(
                    self.misspellings(dictionary, record)
                        .iter()
                        .map(|word| line_start + word.start..line_start + word.end),
                );
            }
        }
        SpellCheck {
            checked: spans
                .iter()
                .map(|span| text.line_start_offset(span.start)..text.line_start_offset(span.end))
                .collect(),
            misspelled,
        }
    }

    /// A line's record, from the cache when its content was seen before.
    fn record(&mut self, content: String) -> LineRecord {
        let hash = hash_line(&content);
        let entry = self.cache.entry(hash).or_insert_with(|| CachedLine {
            code_score: line_code_score(&content),
            is_fence: content.trim_start().starts_with("```"),
            misspellings: None,
        });
        LineRecord {
            hash,
            code_score: entry.code_score,
            is_fence: entry.is_fence,
            content,
        }
    }

    /// The misspellings of a line, from the cache when it was checked before.
    fn misspellings(
        &mut self,
        dictionary: &Dictionary,
        record: &LineRecord,
    ) -> Rc<Vec<Range<usize>>> {
        let entry = self
            .cache
            .get_mut(&record.hash)
            .expect("the line's record is cached");
        entry
            .misspellings
            .get_or_insert_with(|| Rc::new(check_line(dictionary, &record.content)))
            .clone()
    }

    /// Scans the document and returns the byte ranges of all misspellings
    /// outside code regions, in order.
    pub(crate) fn scan(&mut self, dictionary: &Dictionary, text: &Rope) -> Vec<Range<usize>> {
        if !dictionary.is_loaded() || text.len() == 0 || text.len() > MAX_SPELL_DOCUMENT_BYTES {
            self.cache.clear();
            self.fences = None;
            return Vec::new();
        }

        if dictionary.generation() != self.dictionary_generation {
            self.cache.clear();
            self.dictionary_generation = dictionary.generation();
        }

        let mut rows: Vec<LineRecord> = Vec::with_capacity(text.lines_len());
        let mut new_cache: HashMap<u64, CachedLine> = HashMap::with_capacity(text.lines_len());

        for line in text.iter_lines() {
            let content = line.to_string();
            let hash = hash_line(&content);
            if let std::collections::hash_map::Entry::Vacant(e) = new_cache.entry(hash) {
                let entry = self.cache.remove(&hash).unwrap_or_else(|| CachedLine {
                    code_score: line_code_score(&content),
                    is_fence: content.trim_start().starts_with("```"),
                    misspellings: None,
                });
                e.insert(entry);
            }
            let entry = &new_cache[&hash];
            rows.push(LineRecord {
                hash,
                code_score: entry.code_score,
                is_fence: entry.is_fence,
                content,
            });
        }

        let mut issues = Vec::new();
        let mut in_fence = false;

        for (row, record) in rows.iter().enumerate() {
            if record.is_fence {
                in_fence = !in_fence;
                continue;
            }
            if in_fence || record.code_score > LINE_CODE_SKIP_SCORE {
                continue;
            }
            if smoothed_code_score(&rows, row) > SMOOTHED_CODE_SKIP_SCORE {
                continue;
            }

            let entry = new_cache
                .get_mut(&record.hash)
                .expect("entry inserted in first pass");
            let misspellings = match &entry.misspellings {
                Some(m) => m.clone(),
                None => {
                    let computed = Rc::new(check_line(dictionary, &record.content));
                    entry.misspellings = Some(computed.clone());
                    computed
                }
            };

            let line_start = text.line_start_offset(row);
            issues.extend(
                misspellings
                    .iter()
                    .map(|word| line_start + word.start..line_start + word.end),
            );
        }

        self.cache = new_cache;
        self.fences = Some(rows.iter().filter(|record| record.is_fence).count());
        issues
    }
}

/// Whether the line of `text` at `row` opens or closes a fenced code block.
fn is_fence(text: &Rope, row: usize) -> bool {
    let mut chars = text
        .slice_line(row)
        .chars()
        .skip_while(|c| c.is_whitespace());
    (0..3).all(|_| chars.next() == Some('`'))
}

/// How many fence lines start in `range` of `text`, which starts a line:
/// a search for backticks, which most lines lack, and a look at the start
/// of each line that has one.
fn fence_lines(text: &Rope, range: Range<usize>) -> usize {
    let mut count = 0;
    let mut offset = range.start;
    let mut last_row = None;
    for chunk in text.slice(range).chunks() {
        for (index, _) in chunk.match_indices('`') {
            let row = text.offset_to_point(offset + index).row;
            // The first backtick of a fence line starts it.
            if last_row.replace(row) != Some(row) && is_fence(text, row) {
                count += 1;
            }
        }
        offset += chunk.len();
    }
    count
}

/// Weighted average of code scores over a five-line neighborhood, so lines
/// inside code blocks inherit their surroundings' classification.
fn smoothed_code_score(rows: &[LineRecord], row: usize) -> f64 {
    const WEIGHTS: [(isize, f64); 5] = [(-2, 1.0), (-1, 2.0), (0, 4.0), (1, 2.0), (2, 1.0)];
    let mut total = 0.0;
    let mut weight = 0.0;
    for (delta, w) in WEIGHTS {
        let Some(ix) = row.checked_add_signed(delta) else {
            continue;
        };
        let Some(record) = rows.get(ix) else {
            continue;
        };
        total += w * record.code_score;
        weight += w;
    }
    if weight == 0.0 { 0.0 } else { total / weight }
}

fn hash_line(line: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

fn is_token_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '\u{2019}' || c == '-'
}

/// Tokenizes one line and returns the byte ranges of its misspellings.
fn check_line(dictionary: &Dictionary, line: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    if line.len() < MIN_CHECK_TOKEN_CHARS {
        return result;
    }
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut ix = 0;
    while ix < chars.len() {
        if !is_token_char(chars[ix].1) {
            ix += 1;
            continue;
        }
        let raw_start = ix;
        while ix < chars.len() && is_token_char(chars[ix].1) {
            ix += 1;
        }
        examine_token(dictionary, &chars, raw_start, ix, &mut result);
    }
    result
}

/// Applies shape, adjacency, and dictionary checks to a single raw token.
///
/// Every skip rule errs toward silence: a missed check is a soft failure,
/// while a false squiggle on jargon or code is the failure mode that
/// actually annoys.
fn examine_token(
    dictionary: &Dictionary,
    chars: &[(usize, char)],
    raw_start: usize,
    raw_end: usize,
    out: &mut Vec<Range<usize>>,
) {
    let mut start = raw_start;
    let mut end = raw_end;
    while start < end && !chars[start].1.is_alphabetic() {
        start += 1;
    }
    while end > start && !chars[end - 1].1.is_alphabetic() {
        end -= 1;
    }
    let len = end - start;
    if !(MIN_CHECK_TOKEN_CHARS..=MAX_CHECK_TOKEN_CHARS).contains(&len) {
        return;
    }

    let mut has_lower = false;
    let mut has_upper = false;
    let mut internal_upper = false;
    for (i, &(_, c)) in chars[start..end].iter().enumerate() {
        if !c.is_ascii() && c != '\u{2019}' {
            return;
        }
        if c.is_ascii_uppercase() {
            has_upper = true;
            if i > 0 {
                internal_upper = true;
            }
        } else if c.is_ascii_lowercase() {
            has_lower = true;
        }
    }
    if has_upper && !has_lower {
        return;
    }
    if internal_upper {
        return;
    }

    let prev = (raw_start > 0).then(|| chars[raw_start - 1].1);
    let next = chars.get(raw_end).map(|&(_, c)| c);
    let next2 = chars.get(raw_end + 1).map(|&(_, c)| c);

    if let Some(p) = prev
        && (p.is_ascii_digit()
            || matches!(
                p,
                '_' | '.' | '@' | '/' | '\\' | '#' | '$' | '&' | '=' | '~' | '`' | '<' | ':'
            ))
    {
        return;
    }
    if let Some(n) = next {
        if n.is_ascii_digit() || matches!(n, '_' | '@' | '(' | '=' | '`' | '>' | '/' | '\\') {
            return;
        }
        if n == '.' && next2.is_some_and(|c| c.is_alphanumeric()) {
            return;
        }
        if n == ':' && next2.is_some_and(|c| !c.is_whitespace()) {
            return;
        }
    }

    let word: String = chars[start..end].iter().map(|&(_, c)| c).collect();
    if dictionary.is_correct(&word) {
        return;
    }
    if let Some(base) = word
        .strip_suffix("'s")
        .or_else(|| word.strip_suffix("\u{2019}s"))
        && dictionary.is_correct(base)
    {
        return;
    }
    if has_upper && !starts_sentence(chars, raw_start) {
        return;
    }

    let end_byte = chars.get(end).map_or_else(
        || chars[end - 1].0 + chars[end - 1].1.len_utf8(),
        |&(offset, _)| offset,
    );
    out.push(chars[start].0..end_byte);
}

/// Determines whether a token sits at a sentence start, scanning back over
/// whitespace and opening quotes/brackets. Capitalized unknown words that do
/// not start a sentence are treated as proper nouns and skipped.
fn starts_sentence(chars: &[(usize, char)], raw_start: usize) -> bool {
    let mut ix = raw_start;
    while ix > 0 {
        let c = chars[ix - 1].1;
        if c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '(' | '[' | '{' | '\u{2018}' | '\u{201C}' | '*'
            )
        {
            ix -= 1;
            continue;
        }
        return matches!(c, '.' | '!' | '?' | ':');
    }
    true
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Cost of the character at `ix` (1-based) of `word` being missing from the
/// other word: cheap when it doubles the letter before it, or is an
/// apostrophe.
fn gap_cost(word: &[char], ix: usize) -> f32 {
    let c = word[ix - 1];
    if c == '\'' || c == '\u{2019}' {
        APOSTROPHE_COST
    } else if ix >= 2 && word[ix - 2] == c {
        DOUBLED_LETTER_COST
    } else {
        1.0
    }
}

fn substitution_cost(a: char, b: char) -> f32 {
    if a == b {
        0.0
    } else if is_vowel(a) && is_vowel(b) {
        VOWEL_COST
    } else {
        1.0
    }
}

/// Optimal string alignment (restricted Damerau-Levenshtein) distance with
/// costs weighted for typing mistakes, from typed `a` to candidate `b`.
/// Returns `None` as soon as the distance provably exceeds `max`.
#[cfg(test)]
fn typo_distance(a: &[char], b: &[char], max: f32) -> Option<f32> {
    Rows::default().typo_distance(a, b, max)
}

/// The rows [`Rows::typo_distance`] works in, kept from one word to the
/// next.
#[derive(Default)]
struct Rows {
    prev2: Vec<f32>,
    prev: Vec<f32>,
    cur: Vec<f32>,
}

impl Rows {
    /// See [`typo_distance`].
    fn typo_distance(&mut self, a: &[char], b: &[char], max: f32) -> Option<f32> {
        let n = b.len();
        let Self { prev2, prev, cur } = self;
        for row in [&mut *prev2, &mut *prev, &mut *cur] {
            row.clear();
            row.resize(n + 1, 0.0);
        }
        for j in 1..=n {
            prev[j] = prev[j - 1] + gap_cost(b, j);
        }

        for i in 1..=a.len() {
            cur[0] = prev[0] + gap_cost(a, i);
            let mut row_min = cur[0];
            for j in 1..=n {
                let mut v = (prev[j] + gap_cost(a, i))
                    .min(cur[j - 1] + gap_cost(b, j))
                    .min(prev[j - 1] + substitution_cost(a[i - 1], b[j - 1]));
                if i > 1
                    && j > 1
                    && a[i - 1] != a[i - 2]
                    && a[i - 1] == b[j - 2]
                    && a[i - 2] == b[j - 1]
                {
                    let cost = if is_vowel(a[i - 1]) && is_vowel(a[i - 2]) {
                        VOWEL_TRANSPOSITION_COST
                    } else {
                        TRANSPOSITION_COST
                    };
                    v = v.min(prev2[j - 2] + cost);
                }
                cur[j] = v;
                row_min = row_min.min(v);
            }
            if row_min > max {
                return None;
            }
            std::mem::swap(prev2, prev);
            std::mem::swap(prev, cur);
        }

        let d = prev[n];
        (d <= max).then_some(d)
    }
}

/// Adjusts a suggestion's casing to match the shape of the source word.
fn match_case(source: &str, suggestion: &str) -> String {
    match source.chars().next() {
        Some(f) if f.is_uppercase() => {
            let all_caps = source.chars().all(|c| !c.is_lowercase()) && source.chars().count() > 1;
            if all_caps {
                suggestion.to_uppercase()
            } else {
                capitalize(suggestion)
            }
        }
        _ => suggestion.to_string(),
    }
}

/// Uppercases the first character of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The bundled English dictionary.
    pub(crate) fn english() -> Dictionary {
        let dictionary = Dictionary::load(&[WordList::English], None);
        assert!(dictionary.is_loaded());
        dictionary
    }

    #[test]
    fn common_typos_suggest_the_intended_word_first() {
        let dictionary = english();
        for (typo, intended) in [
            ("mispeled", "misspelled"),
            ("teh", "the"),
            ("recieve", "receive"),
            ("seperate", "separate"),
            ("occured", "occurred"),
            ("definately", "definitely"),
            ("accomodate", "accommodate"),
            ("untill", "until"),
            ("wierd", "weird"),
            ("becuase", "because"),
            ("adress", "address"),
            ("tommorow", "tomorrow"),
            ("Teh", "The"),
        ] {
            let suggestions = dictionary.suggest(typo, MAX_SUGGESTIONS);
            assert_eq!(
                suggestions.first().map(String::as_str),
                Some(intended),
                "{typo}: {suggestions:?}"
            );
        }
    }

    /// Corrections as `Dictionary::suggest` found them by trying every word
    /// of the dictionary.
    fn suggest_by_scan(dictionary: &Dictionary, word: &str, limit: usize) -> Vec<String> {
        let target: Vec<char> = word.to_lowercase().chars().collect();
        let first = target.first().copied();
        let last = target.last().copied();
        let target_is_lowercase = word.chars().all(|c| !c.is_uppercase());
        let words = dictionary.words.borrow();
        let mut matches: Vec<(f32, &str)> = Vec::new();
        for cand in words.iter() {
            let cand_len = cand.chars().count();
            if cand_len.abs_diff(target.len()) > MAX_SUGGESTION_LENGTH_DIFFERENCE {
                continue;
            }
            let cand_first = cand.chars().next().map(|c| c.to_ascii_lowercase());
            let cand_last = cand.chars().last().map(|c| c.to_ascii_lowercase());
            if cand_first != first && cand_last != last {
                continue;
            }
            let cand_chars: Vec<char> = cand.to_lowercase().chars().collect();
            let Some(mut cost) = typo_distance(&target, &cand_chars, MAX_SUGGESTION_COST) else {
                continue;
            };
            if cost == 0.0 {
                if cand.as_ref() == word {
                    continue;
                }
                cost = CASE_ONLY_COST;
            }
            if cand_first != first {
                cost += FIRST_LETTER_COST;
            }
            if target_is_lowercase && cand.chars().any(|c| c.is_uppercase()) {
                cost += CASE_COST;
            }
            let shared_prefix = target
                .iter()
                .zip(&cand_chars)
                .take_while(|(a, b)| a == b)
                .count()
                .min(4);
            cost -= 0.02 * shared_prefix as f32;
            cost += 0.03 * cand_len.abs_diff(target.len()) as f32;
            matches.push((cost, cand.as_ref()));
        }
        matches.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        let mut suggestions: Vec<String> = Vec::new();
        for (_, candidate) in matches {
            let suggestion = match_case(word, candidate);
            if !suggestions.contains(&suggestion) {
                suggestions.push(suggestion);
            }
            if suggestions.len() == limit {
                break;
            }
        }
        suggestions
    }

    /// A prose line among lines of code reads as code, until an edit two
    /// lines away turns one of them into prose: checking that line checks
    /// the prose line again.
    #[test]
    fn checking_a_line_checks_its_neighbours() {
        let dictionary = english();
        let prose = "A line of plain prose.\n".repeat(20);
        let mut text = prose.clone();
        text.push_str("a = b(c);\nd = e(f);\nThis line has a mispeled word.\n");
        text.push_str("g = h(i);\nj = k(l);\n");
        text.push_str(&prose);
        let mut scanner = SpellScanner::new();
        let rope = Rope::from(text.as_str());
        let check = scanner.check(&dictionary, &rope, &[0..rope.len()]);
        assert_eq!(check.misspelled, Vec::<Range<usize>>::new());

        let code = text.find("j = k(l);").unwrap();
        text.replace_range(code..code + "j = k(l);".len(), "Just some plain prose.");
        let rope = Rope::from(text.as_str());
        let line = code..code + "Just some plain prose.\n".len();
        let check = scanner.check(&dictionary, &rope, std::slice::from_ref(&line));
        let misspelled = text.find("mispeled").unwrap();
        assert!(check.checked.iter().all(|checked| checked.end < rope.len()));
        assert!(
            check
                .checked
                .iter()
                .any(|checked| checked.contains(&misspelled))
        );
        assert_eq!(
            check.misspelled,
            [misspelled..misspelled + "mispeled".len()]
        );
        assert_eq!(
            SpellScanner::new().scan(&dictionary, &rope),
            check.misspelled
        );
    }

    /// After any edits, checking the edited lines as the textarea asks for
    /// them, and keeping the underlines elsewhere, underlines what checking
    /// the whole text does.
    #[test]
    fn checking_the_edited_lines_finds_what_checking_everything_does() {
        let dictionary = english();
        let mut text = String::new();
        for paragraph in 0..40 {
            text.push_str("This paragraph has a mispeled word and recieve too.\n");
            text.push_str("It goes on for a while, as paragraphs do.\n\n");
            if paragraph % 10 == 5 {
                text.push_str("```\nlet mispeled = recieve(teh);\n```\n\n");
            }
        }
        let mut scanner = SpellScanner::new();
        let rope = Rope::from(text.as_str());
        let mut marks = scanner
            .check(&dictionary, &rope, &[0..rope.len()])
            .misspelled;

        // Each edit, at a place found in the text as it is then.
        fn paragraph(text: &str) -> usize {
            let middle = text.len() / 2;
            middle + text[middle..].find("This paragraph").unwrap()
        }
        type Edit = Box<dyn Fn(&str) -> (Range<usize>, &'static str)>;
        let edits: Vec<Edit> = vec![
            // A misspelling typed into prose.
            Box::new(|text| (paragraph(text)..paragraph(text), "Wierd ")),
            // A line of code above prose, which then reads as code too, and
            // taken away again.
            Box::new(|text| {
                let at = paragraph(text);
                (at..at, "fn main() { let x = foo(bar); }\n")
            }),
            Box::new(|text| {
                let at = text.find("fn main").unwrap();
                (at..at + "fn main() { let x = foo(bar); }\n".len(), "")
            }),
            // Code typed below prose.
            Box::new(|text| {
                let at = paragraph(text) + text[paragraph(text)..].find("\n\n").unwrap() + 1;
                (at..at, "x = y(z);")
            }),
            // A fence added and taken away.
            Box::new(|text| (paragraph(text)..paragraph(text), "```\n")),
            Box::new(|text| {
                let at = paragraph(text) - 4;
                (at..at + 4, "")
            }),
            // Inside a fenced block.
            Box::new(|text| {
                let at = text.find("let mispeled").unwrap();
                (at..at, "teh ")
            }),
            // The first and the last line.
            Box::new(|_| (0..4, "Thsi")),
            Box::new(|text| (text.len()..text.len(), "Last lien")),
            // A line split in two.
            Box::new(|text| {
                let at = paragraph(text) + 20;
                (at..at, "\n")
            }),
        ];
        let mut partial = 0;
        for edit in edits {
            let (range, new_text) = edit(&text);
            text.replace_range(range.clone(), new_text);
            let rope = Rope::from(text.as_str());
            // The marks move with the edit, as the textarea's do.
            let grown = new_text.len() as isize - range.len() as isize;
            marks.retain(|mark| mark.end <= range.start || mark.start >= range.end);
            for mark in &mut marks {
                if mark.start >= range.end {
                    mark.start = (mark.start as isize + grown) as usize;
                    mark.end = (mark.end as isize + grown) as usize;
                }
            }
            let first = rope.line_start_offset(rope.offset_to_point(range.start).row);
            let edited_end = range.start + new_text.len();
            let last = rope.offset_to_point(edited_end).row;
            let lines = first..rope.line_start_offset(last + 1);

            let check = scanner.check(&dictionary, &rope, std::slice::from_ref(&lines));
            if check.checked != [0..rope.len()] {
                partial += 1;
            }
            assert!(
                check
                    .checked
                    .iter()
                    .any(|checked| { checked.start <= lines.start && lines.end <= checked.end })
            );
            marks.retain(|mark| {
                !check
                    .checked
                    .iter()
                    .any(|checked| checked.contains(&mark.start))
            });
            marks.extend(check.misspelled);
            marks.sort_by_key(|mark| mark.start);

            let everything = SpellScanner::new().scan(&dictionary, &rope);
            assert_eq!(marks, everything, "after {new_text:?} at {range:?}");
        }
        // All but the fences' edits checked only some lines.
        assert_eq!(partial, 8);
    }

    /// Looking only at words of about the right length that share the first
    /// or the last letter finds what trying every word finds, in the same
    /// order, words added later included.
    #[test]
    fn corrections_are_those_of_trying_every_word() {
        let dictionary = english();
        dictionary.insert_word("Wojciech");
        dictionary.insert_word("Ångström");
        for word in [
            "mispeled",
            "teh",
            "Teh",
            "TEH",
            "recieve",
            "seperate",
            "occured",
            "definately",
            "accomodate",
            "untill",
            "wierd",
            "becuase",
            "adress",
            "tommorow",
            "dont",
            "im",
            "wont",
            "Wojceich",
            "Angstrom",
            "ångstrom",
            "a",
            "ab",
            "xq",
            "hte",
            "yuo",
            "qwzx",
            "photograhpy",
            "Mississipi",
            "zzzzzzzzzzzz",
            "english",
            "Paris",
            "paris",
            "e-mial",
            "cafe",
            "naïve",
            "Straße",
            "colour",
            "organisation",
        ] {
            for limit in [1, MAX_SUGGESTIONS, 50] {
                assert_eq!(
                    dictionary.suggest(word, limit),
                    suggest_by_scan(&dictionary, word, limit),
                    "{word} {limit}"
                );
            }
        }
    }

    #[test]
    fn doubled_letters_and_swaps_cost_less_than_other_edits() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        let cost = |a: &str, b: &str| typo_distance(&chars(a), &chars(b), 3.0).unwrap();
        assert_eq!(cost("untill", "until"), DOUBLED_LETTER_COST);
        assert_eq!(cost("teh", "the"), TRANSPOSITION_COST);
        assert_eq!(cost("wierd", "weird"), VOWEL_TRANSPOSITION_COST);
        assert_eq!(cost("seperate", "separate"), VOWEL_COST);
        assert_eq!(cost("cat", "cut"), VOWEL_COST);
        assert_eq!(cost("cat", "cot"), VOWEL_COST);
        assert_eq!(cost("cat", "bat"), 1.0);
        assert_eq!(cost("dont", "don't"), APOSTROPHE_COST);
        assert!(typo_distance(&chars("abc"), &chars("xyz"), 2.0).is_none());
    }

    #[test]
    fn misspellings_are_byte_ranges_of_the_document() {
        let dictionary = english();
        let mut scanner = SpellScanner::new();
        let text = Rope::from("Café notes.\nThis line is mispeled here.\n");
        let found = scanner.scan(&dictionary, &text);
        let words: Vec<String> = found
            .iter()
            .map(|range| text.slice(range.clone()).to_string())
            .collect();
        assert_eq!(words, ["mispeled"]);
    }

    /// A folder for one test's files, removed afterwards.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(name: &str) -> Self {
            let folder = std::env::temp_dir()
                .join(format!("gpui-kit-writing-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The user's word list is read with the others, and added words are
    /// appended to it.
    #[gpui::test]
    fn added_words_are_kept_in_the_users_word_list(cx: &mut gpui::TestAppContext) {
        let scratch = Scratch::new("user-dictionary");
        let user = scratch.0.join("user-dictionary.txt");
        std::fs::write(&user, "Wojciech\n").unwrap();
        let dictionary = Dictionary::load(&[WordList::English], Some(user.clone()));
        assert!(dictionary.is_correct("Wojciech"));
        assert!(!dictionary.is_correct("Zbigniewski"));

        cx.update(|cx| dictionary.add_word("Zbigniewski", cx));
        cx.run_until_parked();
        assert!(dictionary.is_correct("Zbigniewski"));
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            "Wojciech\nZbigniewski\n"
        );
        let reloaded = Dictionary::load(&[WordList::English], Some(user));
        assert!(reloaded.is_correct("Zbigniewski"));
    }

    /// The index of corrections keeps no room to grow once it is read: for
    /// the English list it takes about 5 MB, not the 7.8 MB it grew into.
    #[test]
    fn the_correction_index_keeps_no_spare_room() {
        let candidates = Words::read(&[WordList::English], None).candidates;
        assert_eq!(candidates.text.capacity(), candidates.text.len());
        assert_eq!(candidates.words.capacity(), candidates.words.len());
        for indices in candidates
            .by_first
            .values()
            .chain(candidates.by_last.values())
        {
            assert_eq!(indices.capacity(), indices.len());
        }
        let bytes = candidates.heap_bytes();
        assert!(bytes < 5_500_000, "{bytes} bytes");
    }

    /// Too few words to be a dictionary, such as a user's word list alone,
    /// accept every word.
    #[test]
    fn a_few_words_are_no_dictionary() {
        let dictionary = Dictionary::load(&[WordList::Text("alpha\nbeta\n".into())], None);
        assert!(!dictionary.is_loaded());
        assert!(dictionary.is_correct("mispeled"));
        assert_eq!(dictionary.suggest("mispeled", 5), Vec::<String>::new());
    }
}
