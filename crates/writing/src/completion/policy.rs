//! The judgements behind what the suggestions offer, gathered behind one
//! trait so that a replay can try others. Eager makes them by [`Ungated`],
//! and Quiet by [`ConfidenceGate`], which also holds back a suggestion's
//! first word when it is more likely wrong than right.

use super::local::LocalIndex;
use super::model::{ADD_WORD_THRESHOLD, SHOW_THRESHOLD};
use super::vocabulary::{Successors, Vocabulary};
use super::{EMPTY_PREDICTION_CONTINUATION_LIMIT, PREFIX_CONTINUATION_LIMIT};
use crate::classify::{TextContext, classify_context, is_prose_shaped};

/// A word that may be offered, and how likely it is to be the one.
pub(crate) struct Candidate<'a> {
    /// The probability that it is the word coming.
    pub probability: f64,
    /// For a completion, the words that fit the prefix. None for a next
    /// word.
    pub fitting: Option<Fitting<'a>>,
    /// For a next word, what the words before the caret say about it. None
    /// for a completion.
    pub predicted: Option<Predicted<'a>>,
}

/// A completion's word, the known words that fit its prefix, and what the
/// writer's vocabulary and the document hold.
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
    /// What followed the longest run of the words before the caret that
    /// anything followed.
    pub context: Option<&'a Successors>,
    /// The document's words.
    pub local: &'a LocalIndex,
}

impl Fitting<'_> {
    /// How many characters the completion adds to the word.
    pub fn rest_chars(&self) -> usize {
        self.word.chars().count().saturating_sub(self.prefix_chars)
    }

    /// Whether the word followed the words before the caret before.
    pub fn followed_context(&self) -> bool {
        self.context
            .is_some_and(|context| context.count(self.word) > 0)
    }

    /// Whether the document holds the word.
    pub fn in_document(&self) -> bool {
        self.local.counts().word_count(self.word) > 0
    }
}

/// A predicted next word and what supports it.
pub(crate) struct Predicted<'a> {
    /// The word, folded.
    pub word: &'a str,
    /// What followed the longest run of the words before the caret that
    /// anything followed, and how many words long that run is.
    pub context: &'a Successors,
    pub context_words: usize,
    /// The document's words.
    pub local: &'a LocalIndex,
}

impl Predicted<'_> {
    /// How many times the word followed the words before the caret.
    pub fn context_count(&self) -> u32 {
        self.context.count(self.word)
    }

    /// Whether the document holds the word.
    pub fn in_document(&self) -> bool {
        self.local.counts().word_count(self.word) > 0
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

/// The rules without a gate, which Eager follows.
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
    #[cfg_attr(not(any(test, feature = "replay")), allow(dead_code))]
    pub long_bar: Option<(usize, f64)>,
}

impl CompletionGate {
    /// The gate whose chance [`CompletionConfidence`] starts from, its two
    /// numbers set by reasoning. The prior weighs two occurrences of the
    /// writer's own rate of new words, and each known word that fits counts
    /// once towards a new one, as Witten and Bell have it.
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

#[cfg(any(test, feature = "replay"))]
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

/// The chance that a completion in prose is right, from the completion
/// gate's chance in log-odds and a step for each sign the gate doesn't
/// weigh. Each number was set by reasoning from replays of real writing.
pub(crate) struct CompletionConfidence;

impl CompletionConfidence {
    /// The log-odds taken off every completion. The gate's chance is right
    /// on average only because the words with a step of their own below
    /// pull the average up.
    const BELOW_CHANCE: f64 = 0.4;
    /// The log-odds added for a word the document holds. The document's
    /// words are the writer's own recent vocabulary, which the gate's
    /// chance, read from the shared vocabulary, leaves out.
    const IN_DOCUMENT: f64 = 0.7;
    /// The log-odds taken off for each character of rest beyond
    /// `CALIBRATED_REST`, for a word the document doesn't hold. The more is
    /// left to type, the more room there is for another word. At a rest of
    /// two the gate's chance is right.
    const PER_CHAR: f64 = 0.17;
    const CALIBRATED_REST: usize = 2;
    /// The log-odds added for a word that followed the words before the
    /// caret before, which the gate's chance, read from word frequency
    /// alone, leaves out.
    const FOLLOWED_CONTEXT: f64 = 0.6;

    /// The chance that `candidate` is the word coming.
    pub fn chance(candidate: &Candidate<'_>, fitting: &Fitting<'_>) -> f64 {
        let mut odds = logit(CompletionGate::QUIET.chance(candidate, fitting)) - Self::BELOW_CHANCE;
        if fitting.in_document() {
            odds += Self::IN_DOCUMENT;
        } else {
            let beyond = fitting.rest_chars().saturating_sub(Self::CALIBRATED_REST);
            odds -= Self::PER_CHAR * beyond as f64;
        }
        if fitting.followed_context() {
            odds += Self::FOLLOWED_CONTEXT;
        }
        sigmoid(odds)
    }
}

/// The chance that a predicted next word is right, from the model's
/// probability in log-odds and a step for each sign the model doesn't
/// weigh. Each number was set by reasoning from replays of real writing.
pub(crate) struct PredictionConfidence;

impl PredictionConfidence {
    /// The log-odds taken off a word whose count after its context is two
    /// or more, and more again when the last word alone is its context, as
    /// when nothing followed the two words before. At a count of one
    /// the model is already backing off to shorter contexts, and its
    /// probability is right.
    const OVERCONFIDENCE: f64 = 0.5;
    const BIGRAM_ONLY: f64 = 0.35;
    /// The log-odds taken off a word the document doesn't hold, which comes
    /// from an earlier document, where the writer was on another subject.
    const NOT_IN_DOCUMENT: f64 = 0.7;

    /// The chance that `candidate` is the word coming.
    pub fn chance(candidate: &Candidate<'_>, predicted: &Predicted<'_>) -> f64 {
        let mut odds = logit(candidate.probability);
        if predicted.context_count() >= 2 {
            odds -= Self::OVERCONFIDENCE;
            if predicted.context_words < 2 {
                odds -= Self::BIGRAM_ONLY;
            }
        }
        if !predicted.in_document() {
            odds -= Self::NOT_IN_DOCUMENT;
        }
        sigmoid(odds)
    }
}

fn logit(p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    (p / (1.0 - p)).ln()
}

fn sigmoid(odds: f64) -> f64 {
    1.0 / (1.0 + (-odds).exp())
}

/// The rules Quiet follows when a suggestion first shows. A completion in
/// prose shows when [`CompletionConfidence`] puts its chance at one half or
/// more, and a predicted next word when [`PredictionConfidence`] does. The
/// bar is one half whatever the word's length, because a word held back
/// can still show at the next key, so showing it now weighs one key saved
/// against one wrong offer.
pub(crate) struct ConfidenceGate;

impl SuggestionPolicy for ConfidenceGate {
    fn shows_completion(&self, candidate: &Candidate<'_>) -> bool {
        // Only what the model's own bar shows is judged. The steps could lift
        // a word under that bar, but the replays they were set from hold
        // nothing there.
        candidate.probability >= SHOW_THRESHOLD
            && candidate
                .fitting
                .as_ref()
                .filter(|fitting| fitting.prose)
                .is_none_or(|fitting| {
                    CompletionConfidence::chance(candidate, fitting) >= SHOW_THRESHOLD
                })
    }

    fn shows_prediction(&self, candidate: &Candidate<'_>) -> bool {
        // The steps only lower the chance, so a word under the model's own
        // bar needs no judging.
        candidate.probability >= SHOW_THRESHOLD
            && candidate.predicted.as_ref().is_none_or(|predicted| {
                PredictionConfidence::chance(candidate, predicted) >= SHOW_THRESHOLD
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::vocabulary::Counts;
    use gpui_base::input::Rope;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.0005
    }

    /// Cases worked by hand, with the arithmetic beside each. "building",
    /// seen three times and the only word that fits its prefix, in a
    /// vocabulary of 100 words seen 500 times. New words are
    /// (1 + 2 × 0.2) / (3 + 1 + 2) = 0.233 of the words that start so.
    #[test]
    fn completion_confidence_weighs_each_completion() {
        let outside = LocalIndex::new();
        let mut holding = LocalIndex::new();
        holding.build(&Rope::from("the building is here"), None, &|_| 0);
        let mut counts = Counts::default();
        counts.add_run(false, &["in", "building"]);
        let followed = counts.successors(&["in".to_string()]);
        let candidate = |probability, prefix_chars, local, context| Candidate {
            probability,
            fitting: Some(Fitting {
                word: "building",
                words: 1,
                count: 3.0,
                known: 100,
                seen: 500.0,
                prefix_chars,
                prose: true,
                context,
                local,
            }),
            predicted: None,
        };
        let chance = |probability, prefix_chars, local, context| {
            let candidate = candidate(probability, prefix_chars, local, context);
            CompletionConfidence::chance(&candidate, candidate.fitting.as_ref().unwrap())
        };

        // At p 1 the gate's chance is 0.767, log-odds 1.19, less 0.4 = 0.79.
        // After four letters, a rest of four outside the document takes off
        // 0.17 × 2, leaving 0.45, a chance of 0.611. After one letter, a rest
        // of seven takes off 0.85, leaving -0.06, a chance of 0.485.
        assert!(close(chance(1.0, 4, &outside, None), 0.6105));
        assert!(close(chance(1.0, 1, &outside, None), 0.4849));
        // At p 0.6 the gate's chance is 0.46, log-odds -0.16, less 0.4 =
        // -0.56. A rest of two outside the document stays there, a chance of
        // 0.363. In the document it gains 0.7, to 0.14, a chance of 0.535.
        assert!(close(chance(0.6, 6, &outside, None), 0.3635));
        assert!(close(chance(0.6, 6, &holding, None), 0.5349));
        // At p 0.8 the gate's chance is 0.613, log-odds 0.46, less 0.4 =
        // 0.06. A rest of four outside the document leaves -0.28, a chance of
        // 0.431. Having followed the word before gains 0.6, to 0.32, a chance
        // of 0.580.
        assert!(close(chance(0.8, 4, &outside, None), 0.4308));
        assert!(close(chance(0.8, 4, &outside, followed), 0.5797));

        // Quiet shows a completion at a chance of one half, and only one the
        // model's own bar shows. At p 0.45 the document and the words before
        // lift the chance to 0.564, but it stays hidden.
        let shows = |probability, prefix_chars, local, context| {
            ConfidenceGate.shows_completion(&candidate(probability, prefix_chars, local, context))
        };
        assert!(shows(1.0, 4, &outside, None));
        assert!(!shows(1.0, 1, &outside, None));
        assert!(close(chance(0.45, 6, &holding, followed), 0.5644));
        assert!(!shows(0.45, 6, &holding, followed));
    }

    /// Cases worked by hand, with the arithmetic beside each.
    #[test]
    fn prediction_confidence_weighs_each_prediction() {
        let outside = LocalIndex::new();
        let mut holding = LocalIndex::new();
        holding.build(&Rope::from("the building is here"), None, &|_| 0);
        let mut counts = Counts::default();
        counts.add_run(false, &["in", "the"]);
        for _ in 0..3 {
            counts.add_run(false, &["on", "the"]);
            counts.add_run(false, &["at", "building"]);
        }
        let after = |word: &str| counts.successors(&[word.to_string()]).unwrap();
        let candidate = |probability, word, context, context_words, local| Candidate {
            probability,
            fitting: None,
            predicted: Some(Predicted {
                word,
                context,
                context_words,
                local,
            }),
        };
        let chance = |probability, word, context, context_words, local| {
            let candidate = candidate(probability, word, context, context_words, local);
            PredictionConfidence::chance(&candidate, candidate.predicted.as_ref().unwrap())
        };

        // "the" at p 0.6 (log-odds 0.41), after a context of two words it
        // followed once, in the document, keeps its chance of 0.6.
        assert!(close(chance(0.6, "the", after("in"), 2, &holding), 0.6));
        // Followed three times, it falls to 0.41 - 0.5 = -0.09, a chance of
        // 0.476. At p 0.7 (0.85) it falls to 0.35, a chance of 0.586.
        assert!(close(chance(0.6, "the", after("on"), 2, &holding), 0.4764));
        assert!(close(chance(0.7, "the", after("on"), 2, &holding), 0.5860));
        // "building" after a one-word context it followed three times,
        // outside the document, is at 0.41 - 0.5 - 0.35 - 0.7 = -1.14, a
        // chance of 0.241.
        assert!(close(
            chance(0.6, "building", after("at"), 1, &outside),
            0.2415
        ));
        // At p 0.9 (2.20) after two words it followed three times, outside
        // the document, it is at 2.20 - 0.5 - 0.7 = 1.0, a chance of 0.731.
        assert!(close(chance(0.9, "the", after("on"), 2, &outside), 0.7305));

        let shows = |probability, word, context, context_words, local| {
            ConfidenceGate.shows_prediction(&candidate(
                probability,
                word,
                context,
                context_words,
                local,
            ))
        };
        assert!(shows(0.9, "the", after("on"), 2, &outside));
        assert!(!shows(0.6, "the", after("on"), 2, &holding));
    }
}
