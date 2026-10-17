//! What to suggest at the caret: the calibrated n-gram scorer.
//!
//! The scorer estimates the probability that a word is the one the user is
//! about to write, from the one or two words before it in the same sentence
//! (the start of the sentence counting as a word), with interpolated absolute
//! discounting: each context's counts, less [`DISCOUNT`] each, plus the
//! discounted mass spread by the next shorter context, down to a word's own
//! frequency. Given a typed prefix, the probability is renormalized over the
//! words that fit it.
//!
//! Being a probability, it means the same for a word completed and a word
//! predicted, after one word or two, so one threshold serves every case. A
//! word shows when its probability is at least [`SHOW_THRESHOLD`]. Words
//! after it join the suggestion while the probability that all of them are
//! right stays at least [`ADD_WORD_THRESHOLD`], and the length offered is
//! the one that saves the most keystrokes on average. A suggestion of one
//! character never shows, since Tab is a keystroke too.

use super::local::LocalIndex;
use super::vocabulary::{Counts, Forms, SENTENCE_START, Successors, Vocabulary, fold};
use super::{
    CONTEXT_BYTES, EMPTY_PREDICTION_CONTINUATION_LIMIT, PREFIX_CONTINUATION_LIMIT,
    canonical_prediction_word, extract_prefix, has_closing_punctuation_before,
    has_text_after_cursor_on_line, is_clause_break, is_learnable_word, is_sentence_terminator,
    word_tokens,
};
use crate::classify::{TextContext, classify_context, is_prose_shaped};
use crate::text::floor_char_boundary;

/// How much of each count the interpolation holds back for the next shorter
/// context.
pub(super) const DISCOUNT: f64 = 0.75;

/// The probability a word needs to show.
pub(super) const SHOW_THRESHOLD: f64 = 0.5;

/// The probability that a suggestion is right in full, which each word added
/// after the first must keep it at.
pub(super) const ADD_WORD_THRESHOLD: f64 = 0.7;

/// The shortest suggestion shown, in characters.
pub(super) const MIN_GHOST_CHARS: usize = 2;

/// Where the counts come from. In prose: the shared vocabulary, with the
/// document's own words that it doesn't hold. In code: the document alone.
struct Evidence<'a> {
    shared: &'a Vocabulary,
    local: &'a LocalIndex,
    prose: bool,
}

impl<'a> Evidence<'a> {
    /// The counts n-gram contexts are looked up in.
    fn counts(&self) -> &'a Counts {
        if self.prose {
            self.shared.counts()
        } else {
            self.local.counts()
        }
    }

    /// A folded word's count.
    fn unigram(&self, folded: &str) -> f64 {
        let count = self.counts().word_count(folded);
        let extra = if self.prose {
            self.local.extra(folded)
        } else {
            0
        };
        f64::from(count) + f64::from(extra)
    }

    /// The sum of all word counts, plus one so that it is never zero.
    fn unigram_total(&self) -> f64 {
        let extra = if self.prose {
            self.local.extra_total()
        } else {
            0
        };
        (self.counts().total() + extra) as f64 + 1.0
    }

    /// The contexts that hold counts, the last word first, then the last two.
    /// Longer contexts after one never seen add nothing.
    fn levels(&self, context: &[String]) -> Vec<&'a Successors> {
        let counts = self.counts();
        (1..=context.len())
            .map_while(|length| counts.successors(&context[context.len() - length..]))
            .filter(|successors| successors.total() > 0)
            .collect()
    }

    /// Whether a word may be offered: in prose, a word shaped like prose and
    /// known well enough not to be a typo. Code offers any word of the
    /// document.
    fn offers(&self, form: &str, folded: &str) -> bool {
        !self.prose
            || (is_prose_shaped(form)
                && self.shared.is_trusted(
                    form,
                    self.shared.counts().word_count(folded),
                    self.local.counts().word_count(folded),
                ))
    }

    /// The form to show for `folded` after `levels`: the one it most often
    /// took after the longest context it followed.
    fn form(&self, levels: &[&Successors], folded: &str) -> String {
        if let Some(level) = levels.iter().rev().find(|level| level.count(folded) > 0) {
            return level.form(folded).to_string();
        }
        let forms = self
            .counts()
            .word(folded)
            .or(self.local.counts().word(folded));
        forms.map_or(folded, |forms| forms.best(folded)).to_string()
    }
}

/// Interpolated absolute-discounting probabilities of `candidates`, given
/// each one's probability from word frequency alone, after `levels`.
///
/// With `restricted`, the candidates are every word that fits a typed
/// prefix, and the probabilities are renormalized over them: `mass` starts
/// as their summed frequency probability.
fn interpolate(
    levels: &[&Successors],
    candidates: &[&str],
    probabilities: &mut [f64],
    restricted: bool,
    mut mass: f64,
) {
    for level in levels {
        let total = f64::from(level.total());
        let backoff = DISCOUNT * level.types() as f64 / total;
        let mut kept_sum = 0.0;
        if restricted && level.types() < candidates.len() {
            // Many words fit a short prefix, and few of them followed the
            // context: look those up among the candidates, which are in
            // order. Kept counts are multiples of a quarter, so they add up
            // to the same sum in any order.
            for probability in probabilities.iter_mut() {
                *probability *= backoff;
            }
            for (word, count) in level.counts() {
                if let Ok(index) = candidates.binary_search(&word) {
                    let kept = (f64::from(count) - DISCOUNT).max(0.0);
                    kept_sum += kept;
                    probabilities[index] += kept / total;
                }
            }
        } else {
            for (word, probability) in candidates.iter().zip(probabilities.iter_mut()) {
                let kept = (f64::from(level.count(word)) - DISCOUNT).max(0.0);
                kept_sum += kept;
                *probability = kept / total + backoff * *probability;
            }
        }
        mass = kept_sum / total + backoff * mass;
    }
    if restricted && mass > 0.0 {
        for probability in probabilities {
            *probability /= mass;
        }
    }
}

/// The probability that punctuation ends the clause after `levels`, such as
/// a comma, interpolated as a word's is. Word frequency alone gives it none.
fn clause_end_probability(levels: &[&Successors]) -> f64 {
    levels.iter().fold(0.0, |lower, level| {
        let total = f64::from(level.total());
        let backoff = DISCOUNT * level.types() as f64 / total;
        (f64::from(level.clause_ends()) - DISCOUNT).max(0.0) / total + backoff * lower
    })
}

/// Whether the clause may still end before the next word.
#[derive(Clone, Copy, PartialEq)]
enum Clause {
    /// Punctuation may still come: the word's probability counts the times
    /// it did, as for a word added to a suggestion.
    MayEnd,
    /// The user typed a space after the last word, so no punctuation comes
    /// between it and the next: the probability leaves out the times it did.
    Goes,
}

/// The likeliest next word after `context`, with its probability.
fn best_next(
    evidence: &Evidence,
    context: &[String],
    clause: Clause,
) -> Option<(String, f64, String)> {
    let levels = evidence.levels(context);
    let (shortest, longer) = levels.split_first()?;
    // What follows a longer context followed the shorter one too, as they
    // were learned together.
    let mut candidates: Vec<&str> = shortest.words().collect();
    for level in longer {
        candidates.extend(level.words().filter(|word| shortest.count(word) == 0));
    }
    let total = evidence.unigram_total();
    let mut probabilities: Vec<f64> = candidates
        .iter()
        .map(|word| evidence.unigram(word) / total)
        .collect();
    interpolate(&levels, &candidates, &mut probabilities, false, 1.0);
    if clause == Clause::Goes {
        let goes = 1.0 - clause_end_probability(&levels);
        if goes > 0.0 {
            for probability in &mut probabilities {
                *probability /= goes;
            }
        }
    }
    // The likeliest, the first in alphabetical order of equals.
    let (word, probability) = candidates
        .into_iter()
        .zip(probabilities)
        .reduce(|best, next| {
            if next.1 > best.1 || (next.1 == best.1 && next.0 < best.0) {
                next
            } else {
                best
            }
        })?;
    Some((word.to_string(), probability, evidence.form(&levels, word)))
}

/// The words before `prefix_start` in its sentence that predict the next
/// one, folded: the last two at most, after the sentence start when the
/// sentence opens there. A number or another token that isn't learned right
/// before leaves none, since nothing before it helps.
///
/// In prose, with `clauses`, the words before punctuation, such as a comma,
/// predict nothing either, as learning never counts a word after them: none
/// after "UI/UX, ", and "UX" alone after "UI/UX ".
fn context_words(text: &str, prefix_start: usize, clauses: bool) -> Vec<String> {
    let before = &text[..prefix_start];
    let sentence_start = before
        .rfind(is_sentence_terminator)
        .map_or(0, |index| index + 1);
    let truncated = prefix_start - sentence_start > CONTEXT_BYTES;
    let area_start = if truncated {
        floor_char_boundary(text, prefix_start - CONTEXT_BYTES)
    } else {
        sentence_start
    };
    let area = &text[area_start..prefix_start];
    let tokens = word_tokens(area);
    if clauses
        && let Some(last) = tokens.last()
        && is_clause_break(&area[last.end..])
    {
        return Vec::new();
    }
    let words: Vec<&str> = tokens.iter().map(|range| &area[range.clone()]).collect();
    let mut run_start = words
        .iter()
        .rposition(|word| !is_learnable_word(word))
        .map_or(0, |index| index + 1);
    if clauses
        && let Some(index) = (1..tokens.len())
            .rev()
            .find(|&index| is_clause_break(&area[tokens[index - 1].end..tokens[index].start]))
    {
        run_start = run_start.max(index);
    }
    if run_start == words.len() && !words.is_empty() {
        return Vec::new();
    }
    let mut context = Vec::new();
    if run_start == 0 && !truncated {
        context.push(SENTENCE_START.to_string());
    }
    context.extend(
        words[run_start..]
            .iter()
            .map(|word| fold(canonical_prediction_word(word)).into_owned()),
    );
    if context.len() > 2 {
        context.drain(..context.len() - 2);
    }
    context
}

/// The rest of `form` after a typed prefix that folds to `folded_prefix`.
fn rest_after<'a>(form: &'a str, folded_prefix: &str) -> Option<&'a str> {
    let mut folded_len = 0;
    for (index, character) in form.char_indices() {
        if folded_len == folded_prefix.len() {
            return Some(&form[index..]);
        }
        folded_len += fold(&form[index..index + character.len_utf8()]).len();
    }
    (folded_len == folded_prefix.len()).then_some("")
}

/// The rest of a word to show after `prefix`: the stored form's letters, in
/// capitals after a prefix typed in capitals.
fn shown_rest(prefix: &str, form: &str, folded_prefix: &str) -> Option<String> {
    let rest = rest_after(form, folded_prefix)?;
    let letters: Vec<char> = prefix.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase()) {
        Some(rest.to_uppercase())
    } else {
        Some(rest.to_string())
    }
}

/// A candidate completion: its folded word, how often it was seen, and its
/// forms.
struct Completion<'a> {
    folded: &'a str,
    count: f64,
    forms: &'a Forms,
}

/// The completions of `folded_prefix`: the vocabulary's words, and in prose
/// the document's words it doesn't hold, merged in order.
fn completions<'a>(evidence: &Evidence<'a>, folded_prefix: &'a str) -> Vec<Completion<'a>> {
    let mut completions: Vec<Completion<'a>> = Vec::new();
    if !evidence.prose {
        for (folded, forms) in evidence.local.counts().words_with_prefix(folded_prefix) {
            completions.push(Completion {
                folded,
                count: f64::from(forms.count()),
                forms,
            });
        }
        return completions;
    }
    let mut shared = evidence
        .shared
        .counts()
        .words_with_prefix(folded_prefix)
        .peekable();
    // The document's words the vocabulary doesn't hold all of, with the
    // occurrences it doesn't hold.
    let mut local = evidence
        .local
        .counts()
        .words_with_prefix(folded_prefix)
        .filter_map(|(folded, forms)| {
            let extra = evidence.local.extra(folded);
            (extra > 0).then_some((folded, forms, extra))
        })
        .peekable();
    loop {
        let take_shared = match (shared.peek(), local.peek()) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some((a, _)), Some((b, _, _))) => a <= b,
        };
        if take_shared {
            let (folded, forms) = shared.next().unwrap();
            let extra = local
                .next_if(|(next, _, _)| *next == folded)
                .map_or(0, |(_, _, extra)| extra);
            completions.push(Completion {
                folded,
                count: f64::from(forms.count()) + f64::from(extra),
                forms,
            });
        } else {
            let (folded, forms, extra) = local.next().unwrap();
            completions.push(Completion {
                folded,
                count: f64::from(extra),
                forms,
            });
        }
    }
    completions
}

/// Generates an inline suggestion at the cursor.
pub(super) fn generate_suggestion(
    shared: &Vocabulary,
    local: &LocalIndex,
    text: &str,
    offset: usize,
) -> Option<String> {
    if has_text_after_cursor_on_line(text, offset) {
        return None;
    }

    let prefix = extract_prefix(text, offset);
    let prefix_start = offset - prefix.len();

    if has_closing_punctuation_before(text, prefix_start) {
        return None;
    }

    let evidence = Evidence {
        shared,
        local,
        prose: classify_context(text, offset) == TextContext::Prose,
    };
    let mut context = context_words(text, prefix_start, evidence.prose);
    let at_sentence_start = context.len() == 1 && context[0] == SENTENCE_START;

    let (first, first_probability, ghost) = if prefix.is_empty() {
        // A next word, after a word and a space, in prose.
        if !evidence.prose
            || !text[..offset].ends_with(' ')
            || context.is_empty()
            || at_sentence_start
        {
            return None;
        }
        let (word, probability, form) = best_next(&evidence, &context, Clause::Goes)?;
        if probability < SHOW_THRESHOLD || !evidence.offers(&form, &word) {
            return None;
        }
        (word, probability, form)
    } else {
        // The rest of the word being typed.
        let folded_prefix = fold(prefix);
        let completions = completions(&evidence, &folded_prefix);
        if completions.is_empty() {
            return None;
        }
        let total = evidence.unigram_total();
        let candidates: Vec<&str> = completions.iter().map(|c| c.folded).collect();
        let mut probabilities: Vec<f64> = completions.iter().map(|c| c.count / total).collect();
        let mass = probabilities.iter().sum();
        let levels = evidence.levels(&context);
        interpolate(&levels, &candidates, &mut probabilities, true, mass);

        // The likeliest word that may be offered. Only a word at the show
        // threshold can show, and as the probabilities add up to one, at
        // most two words reach it.
        let mut likely: Vec<usize> = (0..completions.len())
            .filter(|&index| probabilities[index] >= SHOW_THRESHOLD)
            .collect();
        likely.sort_by(|&a, &b| {
            probabilities[b]
                .total_cmp(&probabilities[a])
                .then(a.cmp(&b))
        });
        let (index, rest) = likely.into_iter().find_map(|index| {
            let completion = &completions[index];
            let rest = shown_rest(
                prefix,
                completion.forms.best(completion.folded),
                &folded_prefix,
            )?;
            evidence
                .offers(&format!("{prefix}{rest}"), completion.folded)
                .then_some((index, rest))
        })?;
        let completion = &completions[index];
        if completion.folded == folded_prefix {
            // Most likely the word is already complete.
            return None;
        }
        (completion.folded.to_string(), probabilities[index], rest)
    };

    let mut options = vec![(ghost.clone(), first_probability)];
    if evidence.prose {
        let limit = if prefix.is_empty() {
            EMPTY_PREDICTION_CONTINUATION_LIMIT
        } else {
            PREFIX_CONTINUATION_LIMIT
        };
        let mut ghost = ghost;
        let mut cumulative = first_probability;
        let mut previous = first;
        context.push(previous.clone());
        for _ in 0..limit {
            if context.len() > 2 {
                context.drain(..context.len() - 2);
            }
            let Some((word, probability, form)) = best_next(&evidence, &context, Clause::MayEnd)
            else {
                break;
            };
            cumulative *= probability;
            if cumulative < ADD_WORD_THRESHOLD || word == previous || !evidence.offers(&form, &word)
            {
                break;
            }
            ghost.push(' ');
            ghost.push_str(&form);
            options.push((ghost.clone(), cumulative));
            context.push(word.clone());
            previous = word;
        }
    }

    // The length that saves the most keystrokes on average: the characters
    // past the Tab that takes it, times the chance it is right.
    let (ghost, _) = options.into_iter().max_by(|a, b| {
        let value = |(ghost, probability): &(String, f64)| {
            probability * (ghost.chars().count() as f64 - 1.0)
        };
        value(a).total_cmp(&value(b))
    })?;
    (ghost.chars().count() >= MIN_GHOST_CHARS).then_some(ghost)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The probabilities of a small, known set of counts, worked by hand.
    #[test]
    fn probabilities_interpolate_the_contexts() {
        let mut counts = Counts::default();
        // "thanks for the" three times, "thanks for your" once.
        for last in ["the", "the", "the", "your"] {
            counts.add_run(false, &["thanks", "for", last]);
        }
        let levels: Vec<&Successors> =
            [vec!["for".to_string()], vec!["thanks".into(), "for".into()]]
                .iter()
                .map(|context| counts.successors(context).unwrap())
                .collect();
        // Words seen 12 times: thanks 4, for 4, the 3, your 1. Plus one.
        let total = 13.0;
        let candidates = ["the", "your", "thanks"];
        let mut probabilities: Vec<f64> = [3.0, 1.0, 4.0].iter().map(|c| c / total).collect();
        interpolate(&levels, &candidates, &mut probabilities, false, 1.0);
        // After "for": the 3, your 1, of 4, with 2 types: backoff 0.75 * 2 / 4.
        let bigram = |count: f64, unigram: f64| (count - 0.75f64).max(0.0) / 4.0 + 0.375 * unigram;
        // After "thanks for": the same counts.
        let the = bigram(3.0, bigram(3.0, 3.0 / total));
        let your = bigram(1.0, bigram(1.0, 1.0 / total));
        let thanks = bigram(0.0, bigram(0.0, 4.0 / total));
        assert!(close(probabilities[0], the), "{probabilities:?}");
        assert!(close(probabilities[1], your));
        assert!(close(probabilities[2], thanks));
        assert!(the >= SHOW_THRESHOLD && your < SHOW_THRESHOLD);

        // Renormalized over the words that fit a prefix: "t" fits "the" and
        // "thanks".
        let candidates = ["thanks", "the"];
        let mut probabilities: Vec<f64> = [4.0, 3.0].iter().map(|c| c / total).collect();
        let mass = probabilities.iter().sum();
        interpolate(&levels, &candidates, &mut probabilities, true, mass);
        let mass = bigram(0.0, bigram(0.0, 4.0 / total)) + bigram(3.0, bigram(3.0, 3.0 / total));
        assert!(close(probabilities[1], the / mass));
        assert!(close(probabilities[0] + probabilities[1], 1.0));
    }

    fn vocabulary(sentences: &[&str]) -> Vocabulary {
        let mut vocabulary = Vocabulary::new();
        for sentence in sentences {
            vocabulary.learn_sentence(sentence);
        }
        vocabulary
    }

    fn suggest(vocabulary: &Vocabulary, text: &str) -> Option<String> {
        generate_suggestion(vocabulary, &LocalIndex::new(), text, text.len())
    }

    #[test]
    fn thresholds_decide_what_shows_and_how_far() {
        let vocabulary = vocabulary(&["Thanks for the update"; 6]);
        // At the start of a sentence, "T" is "Thanks" every time, and after
        // "Thanks", "for the" follows every time.
        assert_eq!(suggest(&vocabulary, "T").as_deref(), Some("hanks for the"));
        assert_eq!(
            suggest(&vocabulary, "Thanks f").as_deref(),
            Some("or the update")
        );

        // An even split between three words shows none.
        let vocabulary = vocabulary_split();
        assert_eq!(suggest(&vocabulary, "Let me k"), None);
        assert_eq!(suggest(&vocabulary, "Let me kn").as_deref(), Some("ow"));
    }

    fn vocabulary_split() -> Vocabulary {
        vocabulary(&["Let me know", "Let me keep", "Let me kick"])
    }

    #[test]
    fn a_one_letter_suggestion_never_shows() {
        let vocabulary = vocabulary(&["We went to the bank"; 4]);
        assert_eq!(suggest(&vocabulary, "We went to the ban"), None);
        assert_eq!(
            suggest(&vocabulary, "We went to the ba").as_deref(),
            Some("nk")
        );
    }

    /// A word the dictionary doesn't know, perhaps a typo, needs to be typed
    /// three times before it is offered.
    #[test]
    fn a_word_outside_the_dictionary_needs_repeated_use() {
        let dictionary = std::rc::Rc::new(crate::spelling::tests::english());
        assert!(dictionary.is_loaded());
        let mut vocabulary = Vocabulary::with_dictionary(dictionary);
        vocabulary.learn_sentence("Ask Wojciech");
        vocabulary.learn_sentence("Ask Wojciech");
        assert_eq!(suggest(&vocabulary, "Ask Woj"), None);
        vocabulary.learn_sentence("Ask Wojciech");
        assert_eq!(suggest(&vocabulary, "Ask Woj").as_deref(), Some("ciech"));
    }

    /// In code, suggestions come from the document alone and stop at the end
    /// of the word.
    #[test]
    fn code_draws_on_the_document_alone() {
        let vocabulary = vocabulary(&["The compass points north"; 5]);
        let text = "let total = compute_total(items);\nreturn comp";
        let mut local = LocalIndex::new();
        local.build(
            &gpui_base::input::Rope::from(text),
            Some(text.len()),
            &|_| 0,
        );
        assert_eq!(
            generate_suggestion(&vocabulary, &local, text, text.len()).as_deref(),
            Some("ute_total")
        );
    }

    /// "Thanks" opening a sentence and "thanks" inside one are one word: the
    /// casing typed stays, and what follows either one follows both.
    #[test]
    fn words_match_in_any_case() {
        let mut sentences = vec!["Thanks for the update"; 4];
        sentences.extend(["Many thanks for the help"; 3]);
        let vocabulary = vocabulary(&sentences);
        assert_eq!(suggest(&vocabulary, "Tha").as_deref(), Some("nks for the"));
        assert_eq!(
            suggest(&vocabulary, "Many tha").as_deref(),
            Some("nks for the")
        );
        assert_eq!(suggest(&vocabulary, "THA").as_deref(), Some("NKS for the"));
    }

    /// Folding the context's few successors into many candidates gives the
    /// probabilities that weighing every candidate gives, to the bit.
    #[test]
    fn interpolating_sparsely_changes_nothing() {
        let mut sentences = vec!["Thanks for the work".to_string(); 3];
        sentences.extend(["Thanks for today".to_string(), "We then went".to_string()]);
        let words = [
            "tall", "trees", "tower", "toward", "twilight", "then", "there", "these", "theme",
            "this", "that", "to", "tomorrow", "team", "test", "tell", "time", "tree", "thanks",
        ];
        for (index, word) in words.iter().enumerate() {
            for other in words.iter().skip(index) {
                sentences.push(format!("Some {word} or {other}"));
            }
        }
        let sentences: Vec<&str> = sentences.iter().map(String::as_str).collect();
        let vocabulary = vocabulary(&sentences);
        let local = LocalIndex::new();
        let evidence = Evidence {
            shared: &vocabulary,
            local: &local,
            prose: true,
        };
        let mut sparse_cases = 0;
        for context in [
            vec!["for"],
            vec!["the"],
            vec!["thanks", "for"],
            vec!["we", "then"],
        ] {
            let context: Vec<String> = context.iter().map(|word| word.to_string()).collect();
            let levels = evidence.levels(&context);
            assert!(!levels.is_empty());
            for prefix in ["t", "th", "the", "to"] {
                let completions = completions(&evidence, prefix);
                let candidates: Vec<&str> = completions.iter().map(|c| c.folded).collect();
                let total = evidence.unigram_total();
                let start: Vec<f64> = completions.iter().map(|c| c.count / total).collect();
                let mass: f64 = start.iter().sum();

                let mut sparse = start.clone();
                interpolate(&levels, &candidates, &mut sparse, true, mass);

                // Every candidate looked up in every level.
                let mut dense = start;
                let mut dense_mass = mass;
                for level in &levels {
                    let total = f64::from(level.total());
                    let backoff = DISCOUNT * level.types() as f64 / total;
                    let mut kept_sum = 0.0;
                    for (word, probability) in candidates.iter().zip(dense.iter_mut()) {
                        let kept = (f64::from(level.count(word)) - DISCOUNT).max(0.0);
                        kept_sum += kept;
                        *probability = kept / total + backoff * *probability;
                    }
                    dense_mass = kept_sum / total + backoff * dense_mass;
                }
                for probability in &mut dense {
                    *probability /= dense_mass;
                }
                if levels.iter().any(|level| level.types() < candidates.len()) {
                    sparse_cases += 1;
                }
                let bits = |values: &[f64]| values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&sparse), bits(&dense), "{context:?} {prefix}");
            }
        }
        assert!(sparse_cases >= 8, "{sparse_cases}");
    }

    /// "UI/UX design" was written before. After "UI/UX, " the comma ended
    /// the clause, so nothing learned after "UI/UX" is offered. After
    /// "UI/UX " it is.
    #[test]
    fn punctuation_ends_the_words_a_prediction_follows() {
        let vocabulary = vocabulary(&["We want better UI/UX design"; 4]);
        assert_eq!(
            suggest(&vocabulary, "Improve the UI/UX ").as_deref(),
            Some("design")
        );
        for punctuation in [",", ";", ":", " —", "/"] {
            let text = format!("Improve the UI/UX{punctuation} ");
            assert_eq!(suggest(&vocabulary, &text), None, "{text:?}");
        }
        // A word typed after the comma completes from word frequency alone.
        assert_eq!(
            suggest(&vocabulary, "Improve the UI/UX, des").as_deref(),
            Some("ign")
        );
    }

    /// A word often followed by a comma makes a word after it less likely,
    /// unless the user typed a space instead.
    #[test]
    fn a_frequent_comma_keeps_a_suggestion_to_the_word() {
        let mut sentences = vec!["Check the analytics dashboard"; 4];
        let before = vocabulary(&sentences);
        assert_eq!(
            suggest(&before, "Open the analyt").as_deref(),
            Some("ics dashboard")
        );

        sentences.extend(["The analytics, sadly, lag behind"; 4]);
        let after = vocabulary(&sentences);
        assert_eq!(suggest(&after, "Open the analyt").as_deref(), Some("ics"));
        assert_eq!(
            suggest(&after, "Open the analytics ").as_deref(),
            Some("dashboard")
        );
        assert_eq!(suggest(&after, "Open the analytics, "), None);
    }
}
