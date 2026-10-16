//! The words of the open document, kept up to date edit by edit.
//!
//! In code they are all the suggestions draw on. In prose they add the
//! document's own words to the shared vocabulary, such as names in a pasted
//! email. A word's count there is its occurrences in the document that the
//! vocabulary can't already hold: occurrences this document taught it don't
//! count, and neither do occurrences the document opened with, up to the
//! vocabulary's own count of the word, since an earlier session may have
//! learned them. That way no text counts twice.
//!
//! Learnable words and their runs never cross a line break, so the counts
//! of a document are the sum of its lines' counts. An edit takes back the
//! counts of the lines it touches, as they were, and adds them as they are.
//! The word at the caret, being typed, isn't counted.

use super::vocabulary::{Counts, fold};
use super::{LOCAL_COUNT_CAP, canonical_prediction_word, extract_sentences_and_words};
use gpui_base::input::{Rope, RopeExt as _};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// What a document added to the shared vocabulary, or opened with: folded
/// words with counts.
pub(crate) type WordTally = HashMap<Box<str>, u32>;

/// How many occurrences of a folded word the shared vocabulary may hold
/// already.
pub(crate) type Held<'a> = &'a dyn Fn(&str) -> u32;

/// Folded learnable words of `text`, counted.
pub(crate) fn tally(text: &str) -> WordTally {
    let mut tally = WordTally::new();
    for run in extract_sentences_and_words(text, None) {
        for word in run {
            *tally
                .entry(fold(canonical_prediction_word(word)).as_ref().into())
                .or_default() += 1;
        }
    }
    tally
}

/// The counts of all of `text`, leaving out the word at `caret`. It can run
/// on any thread.
pub(crate) fn count(text: &Rope, caret: Option<usize>) -> Counts {
    let mut counts = Counts::default();
    let caret = caret.filter(|&caret| caret <= text.len());
    for run in extract_sentences_and_words(&text.to_string(), caret) {
        counts.add_run(false, &run);
    }
    counts
}

/// Counts the lines of `text` in `span`, whole lines, leaving out the word at
/// `caret`, and notes the words counted in `touched`.
fn add(
    counts: &mut Counts,
    text: &Rope,
    span: Range<usize>,
    caret: Option<usize>,
    touched: &mut HashSet<Box<str>>,
) {
    let lines = text.slice(span.clone()).to_string();
    let caret = caret
        .filter(|&caret| in_lines(&span, caret, text.len()))
        .map(|caret| caret - span.start);
    for run in extract_sentences_and_words(&lines, caret) {
        counts.add_run(false, &run);
        touched.extend(
            run.iter()
                .map(|word| fold(canonical_prediction_word(word)).as_ref().into()),
        );
    }
}

/// Takes back the counts [`add`] made of the same lines.
fn remove(
    counts: &mut Counts,
    text: &Rope,
    span: Range<usize>,
    caret: Option<usize>,
    touched: &mut HashSet<Box<str>>,
) {
    let lines = text.slice(span.clone()).to_string();
    let caret = caret
        .filter(|&caret| in_lines(&span, caret, text.len()))
        .map(|caret| caret - span.start);
    for run in extract_sentences_and_words(&lines, caret) {
        counts.remove_run(&run);
        touched.extend(
            run.iter()
                .map(|word| fold(canonical_prediction_word(word)).as_ref().into()),
        );
    }
}

/// Whether `caret` is in the whole lines `span` of a text `len` bytes long:
/// at the end of the last, too, when nothing follows it.
fn in_lines(span: &Range<usize>, caret: usize, len: usize) -> bool {
    span.contains(&caret) || (caret == span.end && caret == len)
}

/// The whole lines of `text` that `range` touches, the line break after the
/// last included.
fn whole_lines(text: &Rope, range: Range<usize>) -> Range<usize> {
    let first = text.offset_to_point(range.start).row;
    let last = text.offset_to_point(range.end).row;
    text.line_start_offset(first)..text.line_start_offset(last + 1)
}

/// The counts of one document's words.
pub(crate) struct LocalIndex {
    counts: Counts,
    /// Per folded word, occurrences the shared vocabulary doesn't hold.
    extra: HashMap<Box<str>, u32>,
    extra_total: u64,
    /// The text counted, once it is.
    text: Option<Rope>,
    /// Where the caret was, whose word isn't counted.
    caret: Option<usize>,
}

impl LocalIndex {
    pub fn new() -> Self {
        Self {
            counts: Counts::default(),
            extra: HashMap::new(),
            extra_total: 0,
            text: None,
            caret: None,
        }
    }

    /// The document's own counts, every occurrence included.
    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// Occurrences of a folded word that the shared vocabulary doesn't hold.
    pub fn extra(&self, folded: &str) -> u32 {
        self.extra.get(folded).copied().unwrap_or(0)
    }

    pub fn extra_total(&self) -> u64 {
        self.extra_total
    }

    /// The text counted, once it is.
    pub fn text(&self) -> Option<&Rope> {
        self.text.as_ref()
    }

    /// Counts all of `text`, leaving out the word at `caret`.
    pub fn build(&mut self, text: &Rope, caret: Option<usize>, held: Held) {
        self.install(count(text, caret), text.clone(), caret, held);
    }

    /// Takes `counts`, which [`count`] made of `text` with `caret`, perhaps
    /// on another thread.
    pub fn install(&mut self, counts: Counts, text: Rope, caret: Option<usize>, held: Held) {
        self.counts = counts;
        self.text = Some(text);
        self.caret = caret;
        self.update_all_extra(held);
    }

    /// Works out again how many occurrences of every word the shared
    /// vocabulary doesn't hold, after what it may hold changed.
    pub fn update_all_extra(&mut self, held: Held) {
        self.extra.clear();
        self.extra_total = 0;
        let words: Vec<Box<str>> = self
            .counts
            .words_with_prefix("")
            .map(|(folded, _)| folded.into())
            .collect();
        self.update_extra(words.iter().map(|word| &**word), held);
    }

    /// Forgets the text, to count it again.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Follows an edit of the counted text: `range` of it replaced by
    /// `new_text`. The word at the caret counts from then until the next
    /// [`Self::move_caret`].
    pub fn edit(&mut self, range: Range<usize>, new_text: &str, held: Held) {
        let Some(mut text) = self.text.take() else {
            return;
        };
        if range.end > text.len() || !text.is_char_boundary(range.start) {
            self.clear();
            return;
        }
        let span = whole_lines(&text, range.clone());
        let caret_in_span = self
            .caret
            .is_some_and(|caret| in_lines(&span, caret, text.len()));
        let mut touched = HashSet::new();
        remove(
            &mut self.counts,
            &text,
            span.clone(),
            self.caret,
            &mut touched,
        );
        text.remove(range.clone());
        text.insert(range.start, new_text);
        let end = span.end + new_text.len() - range.len();
        add(&mut self.counts, &text, span.start..end, None, &mut touched);
        self.caret = self.caret.filter(|_| !caret_in_span).map(|caret| {
            if caret < span.start {
                caret
            } else {
                caret + new_text.len() - range.len()
            }
        });
        self.text = Some(text);
        self.update_extra(touched.iter().map(|word| &**word), held);
    }

    /// Leaves out the word at `caret`, and counts the one at the caret
    /// before.
    pub fn move_caret(&mut self, caret: Option<usize>, held: Held) {
        if caret == self.caret {
            return;
        }
        let Some(text) = self.text.clone() else {
            return;
        };
        let mut touched = HashSet::new();
        if let Some(old) = self.caret {
            let span = whole_lines(&text, old..old);
            remove(
                &mut self.counts,
                &text,
                span.clone(),
                Some(old),
                &mut touched,
            );
            add(&mut self.counts, &text, span, None, &mut touched);
        }
        let caret = caret.filter(|caret| *caret <= text.len() && text.is_char_boundary(*caret));
        if let Some(new) = caret {
            let span = whole_lines(&text, new..new);
            remove(&mut self.counts, &text, span.clone(), None, &mut touched);
            add(&mut self.counts, &text, span, Some(new), &mut touched);
        }
        self.caret = caret;
        self.update_extra(touched.iter().map(|word| &**word), held);
    }

    /// Works out again how many occurrences of `words` the shared vocabulary
    /// doesn't hold, after it learned them.
    pub fn update_extra<'a>(&mut self, words: impl IntoIterator<Item = &'a str>, held: Held) {
        for folded in words {
            let extra = self
                .counts
                .word_count(folded)
                .saturating_sub(held(folded))
                .min(LOCAL_COUNT_CAP);
            let before = if extra > 0 {
                self.extra.insert(folded.into(), extra)
            } else {
                self.extra.remove(folded)
            };
            self.extra_total = self.extra_total + u64::from(extra) - u64::from(before.unwrap_or(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_held(_: &str) -> u32 {
        0
    }

    /// Every word with its count, and every bigram, in order.
    fn summary(counts: &Counts) -> (Vec<(String, u32)>, Vec<(String, String, u32)>) {
        let words: Vec<(String, u32)> = counts
            .words_with_prefix("")
            .map(|(word, forms)| (word.to_string(), forms.count()))
            .collect();
        let mut bigrams = Vec::new();
        for (word, _) in &words {
            if let Some(successors) = counts.successors(std::slice::from_ref(word)) {
                for next in successors.words() {
                    bigrams.push((word.clone(), next.to_string(), successors.count(next)));
                }
            }
        }
        bigrams.sort();
        (words, bigrams)
    }

    /// After any edits, the counts are those of the text counted afresh.
    #[test]
    fn edits_keep_the_counts_of_the_text() {
        let mut text = String::from("Ferns grow slowly.\nMosses grow fast here.\n\nOld ferns");
        let mut index = LocalIndex::new();
        index.build(&Rope::from(text.as_str()), None, &nothing_held);
        let edits: [(Range<usize>, &str); 7] = [
            (0..0, "Tall "),
            (10..15, "spread"),
            (24..24, "\nNew line of words "),
            (5..30, ""),
            (14..14, "x"),
            (0..0, "Ferns ferns ferns"),
            (12..20, "Mosses\n\nMosses"),
        ];
        for (range, new_text) in edits {
            text.replace_range(range.clone(), new_text);
            index.edit(range, new_text, &nothing_held);
            let rope = Rope::from(text.as_str());
            assert_eq!(index.text().unwrap().to_string(), text);
            assert_eq!(
                summary(index.counts()),
                summary(&count(&rope, None)),
                "{text:?}"
            );
            assert_eq!(index.counts().total(), count(&rope, None).total());
        }
    }

    /// The word at the caret isn't counted, wherever the caret goes.
    #[test]
    fn the_word_at_the_caret_isnt_counted() {
        let text = Rope::from("Ferns grow slowly.\nMosses grow fast");
        let mut index = LocalIndex::new();
        index.build(&text, Some(text.len()), &nothing_held);
        assert_eq!(index.counts().word_count("fast"), 0);
        assert_eq!(
            summary(index.counts()),
            summary(&count(&text, Some(text.len())))
        );

        index.move_caret(Some(3), &nothing_held);
        assert_eq!(index.counts().word_count("fast"), 1);
        assert_eq!(index.counts().word_count("ferns"), 0);
        assert_eq!(summary(index.counts()), summary(&count(&text, Some(3))));

        index.move_caret(None, &nothing_held);
        assert_eq!(summary(index.counts()), summary(&count(&text, None)));
    }

    /// A word's extra count leaves out what the vocabulary may hold, up to
    /// the cap.
    #[test]
    fn extra_counts_leave_out_what_the_vocabulary_holds() {
        let text = Rope::from("ferns ferns ferns mosses");
        let mut index = LocalIndex::new();
        let held = |word: &str| u32::from(word == "ferns");
        index.build(&text, None, &held);
        assert_eq!((index.extra("ferns"), index.extra("mosses")), (2, 1));
        assert_eq!(index.extra_total(), 3);

        let held = |word: &str| if word == "ferns" { 3 } else { 0 };
        index.update_extra(["ferns"], &held);
        assert_eq!((index.extra("ferns"), index.extra_total()), (0, 1));

        index.edit(0..0, "mosses ", &held);
        assert_eq!((index.extra("mosses"), index.extra_total()), (2, 2));
    }
}
