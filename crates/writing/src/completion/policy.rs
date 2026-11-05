//! The judgements behind what the suggestions offer, gathered behind one
//! trait so that a replay can try others. [`Today`] makes them as this
//! crate does.

use super::model::{ADD_WORD_THRESHOLD, SHOW_THRESHOLD};
use super::vocabulary::Vocabulary;
use super::{EMPTY_PREDICTION_CONTINUATION_LIMIT, PREFIX_CONTINUATION_LIMIT};
use crate::classify::{TextContext, classify_context, is_prose_shaped};

/// A word that may be offered, and how likely it is to be the one.
pub(crate) struct Candidate {
    /// The probability that it is the word coming.
    pub probability: f64,
}

/// A word that may be offered in prose, with how often it was seen.
pub(crate) struct Word<'a> {
    /// The form it would show in.
    pub form: &'a str,
    /// Its count in the shared vocabulary.
    pub shared_count: u32,
    /// Its count in the document.
    pub local_count: u32,
    vocabulary: &'a Vocabulary,
}

impl<'a> Word<'a> {
    pub fn new(
        form: &'a str,
        shared_count: u32,
        local_count: u32,
        vocabulary: &'a Vocabulary,
    ) -> Self {
        Self {
            form,
            shared_count,
            local_count,
            vocabulary,
        }
    }

    /// Whether it is a dictionary word, or one written often enough not to
    /// be a one-off typo.
    pub fn is_trusted(&self) -> bool {
        self.vocabulary
            .is_trusted(self.form, self.shared_count, self.local_count)
    }
}

/// What decides which suggestion shows. Each method has today's rule as
/// its default.
pub(crate) trait SuggestionPolicy {
    /// Whether the words before the caret weigh in, or word frequency alone.
    fn uses_context(&self) -> bool {
        true
    }

    /// Whether a completion of the word being typed may show.
    fn shows_completion(&self, candidate: &Candidate) -> bool {
        candidate.probability >= SHOW_THRESHOLD
    }

    /// Whether a next word predicted after a space may show.
    fn shows_prediction(&self, candidate: &Candidate) -> bool {
        candidate.probability >= SHOW_THRESHOLD
    }

    /// How many words may follow the first, after a completion or after a
    /// prediction.
    fn words_after(&self, completing: bool) -> usize {
        if completing {
            PREFIX_CONTINUATION_LIMIT
        } else {
            EMPTY_PREDICTION_CONTINUATION_LIMIT
        }
    }

    /// Whether a word joins the suggestion, given the probability that the
    /// whole suggestion with it is right.
    fn adds_word(&self, probability: f64) -> bool {
        probability >= ADD_WORD_THRESHOLD
    }

    /// Whether `word` may be offered in prose.
    fn admits(&self, word: &Word) -> bool {
        is_prose_shaped(word.form) && word.is_trusted()
    }

    /// Whether the text around byte `offset` of `text` is code.
    fn is_code(&self, text: &str, offset: usize) -> bool {
        classify_context(text, offset) == TextContext::Code
    }
}

/// The judgements as the crate makes them.
pub(crate) struct Today;

impl SuggestionPolicy for Today {}

/// Word frequency alone. The most frequent known word that fits the prefix
/// shows when it holds at least half the count of the words that fit, or
/// whatever its share with `always`, with no next word and no words after
/// it.
#[cfg(any(test, feature = "replay"))]
pub(crate) struct FrequencyOnly {
    pub always: bool,
}

#[cfg(any(test, feature = "replay"))]
impl SuggestionPolicy for FrequencyOnly {
    fn uses_context(&self) -> bool {
        false
    }

    fn shows_completion(&self, candidate: &Candidate) -> bool {
        self.always || candidate.probability >= SHOW_THRESHOLD
    }

    fn shows_prediction(&self, _: &Candidate) -> bool {
        false
    }

    fn words_after(&self, _: bool) -> usize {
        0
    }
}
