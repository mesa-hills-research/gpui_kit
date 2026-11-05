//! The judgements behind what the suggestions offer, gathered behind one
//! trait so that a replay can try others. Eager makes them by [`Ungated`],
//! and Quiet by [`CompletionGate::QUIET`], which also holds back
//! completions a young vocabulary is unsure of.

use super::model::{ADD_WORD_THRESHOLD, SHOW_THRESHOLD};
use super::vocabulary::Vocabulary;
use super::{EMPTY_PREDICTION_CONTINUATION_LIMIT, PREFIX_CONTINUATION_LIMIT};
use crate::classify::{TextContext, classify_context, is_prose_shaped};

/// A word that may be offered, and how likely it is to be the one.
pub(crate) struct Candidate<'a> {
    /// The probability that it is the word coming.
    pub probability: f64,
    /// For a completion, the words that fit the prefix. None for a next
    /// word.
    pub fitting: Option<Fitting<'a>>,
}

/// A completion's word, the known words that fit its prefix, and what the
/// writer's vocabulary holds.
pub(crate) struct Fitting<'a> {
    /// The word, folded.
    pub word: &'a str,
    /// How many known words fit the prefix.
    pub words: usize,
    /// How many times they were seen, all together.
    pub count: f64,
    /// How many different words the vocabulary holds.
    pub known: usize,
    /// How many times it saw them, all together.
    pub seen: f64,
    /// How many characters the prefix has.
    pub prefix_chars: usize,
    /// Whether the text around the caret is prose.
    pub prose: bool,
}

impl Fitting<'_> {
    /// How many characters the completion adds to the word.
    pub fn rest_chars(&self) -> usize {
        self.word.chars().count().saturating_sub(self.prefix_chars)
    }
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
    fn shows_completion(&self, candidate: &Candidate<'_>) -> bool {
        candidate.probability >= SHOW_THRESHOLD
    }

    /// Whether a next word predicted after a space may show.
    fn shows_prediction(&self, candidate: &Candidate<'_>) -> bool {
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

/// The rules without the completion gate, which Eager follows.
pub(crate) struct Ungated;

impl SuggestionPolicy for Ungated {}

/// Word frequency alone. The most frequent known word that fits the prefix
/// shows when it holds at least half the count of the words that fit, or
/// whatever its share with `always`, with no next word and no words after
/// it.
#[cfg(any(test, feature = "replay"))]
pub(crate) struct FrequencyOnly {
    pub always: bool,
}

/// The completion gate. A completion shows in prose when the chance that it
/// is the word coming, allowing for a word the vocabulary hasn't seen, is at
/// least one half, or `long_bar`'s lower bar for a completion that adds at
/// least that many characters.
///
/// A completion's probability is spread over the known words that fit the
/// prefix and leaves nothing for a new word, so a young vocabulary's
/// completions are overconfident. The gate estimates how often a word that
/// starts this way is new as Witten and Bell estimate an unseen
/// continuation. That is the number of known words that fit, times
/// `scale`, over their count plus that number, with `prior` occurrences of
/// the writer's overall rate of new words added to both. The writer's rate
/// is the number of different words in the vocabulary over the number of
/// times it saw them.
pub(crate) struct CompletionGate {
    pub prior: f64,
    pub scale: f64,
    pub long_bar: Option<(usize, f64)>,
}

impl CompletionGate {
    /// The gate Quiet uses, its two numbers set by reasoning. The prior
    /// weighs two occurrences of the writer's own rate of new words, and
    /// each known word that fits counts once towards a new one, as Witten
    /// and Bell have it.
    pub const QUIET: Self = Self {
        prior: 2.0,
        scale: 1.0,
        long_bar: None,
    };

    /// The probability that `candidate` is the word coming, allowing for a
    /// new word.
    pub fn chance(&self, candidate: &Candidate<'_>, fitting: &Fitting<'_>) -> f64 {
        let rate = if fitting.seen > 0.0 {
            (fitting.known as f64 / fitting.seen).min(1.0)
        } else {
            1.0
        };
        let words = self.scale * fitting.words as f64;
        let new = (words + self.prior * rate) / (fitting.count + words + self.prior);
        candidate.probability * (1.0 - new)
    }
}

impl SuggestionPolicy for CompletionGate {
    fn shows_completion(&self, candidate: &Candidate<'_>) -> bool {
        let Some(fitting) = candidate.fitting.as_ref().filter(|fitting| fitting.prose) else {
            return candidate.probability >= SHOW_THRESHOLD;
        };
        let bar = match self.long_bar {
            Some((chars, bar)) if fitting.rest_chars() >= chars => bar,
            _ => SHOW_THRESHOLD,
        };
        self.chance(candidate, fitting) >= bar
    }
}

#[cfg(any(test, feature = "replay"))]
impl SuggestionPolicy for FrequencyOnly {
    fn uses_context(&self) -> bool {
        false
    }

    fn shows_completion(&self, candidate: &Candidate<'_>) -> bool {
        self.always || candidate.probability >= SHOW_THRESHOLD
    }

    fn shows_prediction(&self, _: &Candidate<'_>) -> bool {
        false
    }

    fn words_after(&self, _: bool) -> usize {
        0
    }
}
