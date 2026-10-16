//! Timings of the work the engines do on the UI thread, for performance
//! changes to quote. They are ignored tests that print their numbers; run
//! them in an optimized build, one at a time:
//!
//! ```sh
//! cargo test -p gpui-kit-writing --lib --release timings -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The text is made up from the bundled word list, with word frequencies
//! that follow Zipf's law, a misspelling now and then, and a fenced code
//! block every forty paragraphs.

use crate::completion::vocabulary::{LocalIndex, Vocabulary, WordTally};
use crate::spelling::tests::{Scratch, english};
use crate::spelling::{Dictionary, MAX_SUGGESTIONS};
use crate::{WordList, Writing};
use gpui_base::input::Rope;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Runs `work` once to warm up, then `runs` times, and prints the median and
/// the fastest run, divided by `per` for work that does `per` things.
fn time<T>(name: &str, runs: usize, per: u32, mut work: impl FnMut() -> T) -> Duration {
    black_box(work());
    let mut samples: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            black_box(work());
            start.elapsed() / per
        })
        .collect();
    samples.sort();
    let median = samples[runs / 2];
    println!(
        "{name:<52} median {median:>11.3?}   fastest {:>11.3?}   ({runs} runs)",
        samples[0]
    );
    median
}

/// Made-up prose from the bundled word list.
struct Prose {
    words: Vec<&'static str>,
    state: u64,
}

impl Prose {
    /// Every eighth word of the list, about 20,000.
    fn new() -> Self {
        Self::every(8)
    }

    /// Every `step`th word of the list.
    fn every(step: usize) -> Self {
        let list: &'static str = match WordList::English.read() {
            Some(std::borrow::Cow::Borrowed(list)) => list,
            _ => panic!("the English word list is bundled"),
        };
        let words = list
            .lines()
            .filter(|word| {
                (2..=12).contains(&word.len()) && word.chars().all(|c| c.is_ascii_lowercase())
            })
            .step_by(step)
            .collect();
        Self {
            words,
            state: 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// A word, the more frequent the earlier in the list.
    fn word(&mut self) -> &'static str {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        let rank = (self.words.len() as f64).powf(unit) as usize - 1;
        self.words[rank.min(self.words.len() - 1)]
    }

    fn sentence(&mut self) -> String {
        let length = 6 + self.next() % 15;
        let mut sentence = String::new();
        for index in 0..length {
            let word = self.word();
            if index == 0 {
                let mut chars = word.chars();
                let first = chars.next().unwrap().to_ascii_uppercase();
                sentence.push(first);
                sentence.push_str(chars.as_str());
            } else {
                sentence.push(if self.next() % 9 == 0 { ',' } else { ' ' });
                if sentence.ends_with(',') {
                    sentence.push(' ');
                }
                if self.next() % 200 == 0 && word.len() > 3 {
                    // A typo: two letters swapped.
                    let mut letters: Vec<char> = word.chars().collect();
                    letters.swap(1, 2);
                    sentence.extend(letters);
                    continue;
                }
                sentence.push_str(word);
            }
        }
        sentence.push('.');
        sentence
    }

    /// Paragraphs of a few sentences each, one per line with blank lines
    /// between, up to about `bytes`.
    fn document(&mut self, bytes: usize) -> String {
        let mut document = String::with_capacity(bytes + 1024);
        let mut paragraphs = 0;
        while document.len() < bytes {
            paragraphs += 1;
            if paragraphs % 40 == 0 {
                document.push_str("```rust\nfn shift(range: Range<usize>) -> usize {\n");
                document.push_str("    let end = range.end + 1;\n    end\n}\n```\n\n");
                continue;
            }
            let sentences = 3 + self.next() % 4;
            for index in 0..sentences {
                if index > 0 {
                    document.push(' ');
                }
                document.push_str(&self.sentence());
            }
            document.push_str("\n\n");
        }
        document
    }

    /// A vocabulary learned from `sentences` made-up sentences.
    fn vocabulary(&mut self, sentences: usize, dictionary: Rc<Dictionary>) -> Vocabulary {
        let mut vocabulary = Vocabulary::with_dictionary(dictionary);
        for _ in 0..sentences {
            vocabulary.learn_sentence(&self.sentence());
        }
        vocabulary
    }
}

/// A line in the middle of `text` and the text with a letter typed into it.
fn typed_into_the_middle(text: &str) -> (Rope, Rope, std::ops::Range<usize>) {
    let middle = text[..text.len() / 2].rfind("\n\n").unwrap() + 2;
    let end = middle + text[middle..].find('\n').unwrap();
    let before = Rope::from(text);
    let mut after = text.to_string();
    after.insert(middle + 3, 'x');
    (before, Rope::from(after.as_str()), middle..end + 1)
}

#[test]
#[ignore = "a timing, run with --release --ignored"]
fn timings_dictionary() {
    time("load the English dictionary", 10, 1, || {
        Dictionary::load(&[WordList::English], None)
    });
    let dictionary = english();
    let typos = [
        "mispeled",
        "recieve",
        "seperate",
        "definately",
        "becuase",
        "tommorow",
        "langauge",
        "wether",
    ];
    time(
        "suggest corrections, per misspelled word",
        10,
        typos.len() as u32,
        || {
            for typo in typos {
                black_box(dictionary.suggest(typo, MAX_SUGGESTIONS));
            }
        },
    );
}

#[test]
#[ignore = "a timing, run with --release --ignored"]
fn timings_spell_check() {
    let writing = Writing::from_parts(Rc::new(english()), Vocabulary::new());
    for bytes in [100_000, 1_000_000] {
        let text = Prose::new().document(bytes);
        let (before, after, line) = typed_into_the_middle(&text);
        let checker = writing.spell_checker();
        time(
            &format!("check all of a {} KB document", bytes / 1000),
            5,
            1,
            || {
                let checker = writing.spell_checker();
                checker.check_text(&before, &[0..before.len()])
            },
        );
        checker.check_text(&before, &[0..before.len()]);
        let mut flip = false;
        time(
            &format!("check a typed line of a {} KB document", bytes / 1000),
            20,
            1,
            || {
                flip = !flip;
                let text = if flip { &after } else { &before };
                checker.check_text(text, std::slice::from_ref(&line))
            },
        );
    }
}

#[test]
#[ignore = "a timing, run with --release --ignored"]
fn timings_word_suggestions() {
    let dictionary = Rc::new(english());
    let mut prose = Prose::new();
    let vocabulary = prose.vocabulary(100_000, dictionary.clone());
    let writing = Writing::from_parts(dictionary, vocabulary);
    let now = Instant::now();

    for bytes in [10_000, 1_000_000] {
        let mut text = prose.document(bytes);
        text.push_str("We went to the t");
        let rope = Rope::from(text.as_str());
        let suggestions = writing.word_suggestions();
        suggestions.suggest_in(&rope, rope.len(), now);
        time(
            &format!("suggest after a keystroke, {} KB document", bytes / 1000),
            20,
            1,
            || suggestions.suggest_in(&rope, rope.len(), now),
        );
        time(
            &format!("hear a keystroke, {} KB document", bytes / 1000),
            20,
            1,
            || suggestions.learn_before(&rope, rope.len()),
        );

        let shared = writing.vocabulary().borrow();
        let mut index = LocalIndex::new();
        time(
            &format!("rebuild the document's index, {} KB document", bytes / 1000),
            5,
            1,
            || {
                index.mark_stale();
                index.rebuild_if_stale(
                    &text,
                    text.len(),
                    now,
                    shared.counts(),
                    &WordTally::new(),
                    &WordTally::new(),
                );
            },
        );
    }

    let suggestions = writing.word_suggestions();
    for typed in ["We went to the t", "We went to the th", "We went to the "] {
        let rope = Rope::from(typed);
        time(&format!("suggest after {typed:?}"), 20, 1, || {
            suggestions.suggest_in(&rope, rope.len(), now)
        });
    }

    // Every word of the list, so many more of them start with each letter.
    let dictionary = writing.dictionary().clone();
    let vocabulary = Prose::every(1).vocabulary(200_000, dictionary.clone());
    let words = vocabulary.counts().words_with_prefix("t").count();
    let suggestions = Writing::from_parts(dictionary, vocabulary).word_suggestions();
    let rope = Rope::from("We went to the t");
    time(
        &format!("suggest after \"t\", {words} words start with it"),
        20,
        1,
        || suggestions.suggest_in(&rope, rope.len(), now),
    );
}

#[test]
#[ignore = "a timing, run with --release --ignored"]
fn timings_vocabulary_files() {
    let scratch = Scratch::new("timings");
    let path = scratch.0.join("vocabulary.json");
    let dictionary = Rc::new(english());
    let mut prose = Prose::new();
    let mut vocabulary = Vocabulary::load_from(path.clone(), dictionary.clone());
    for _ in 0..100_000 {
        vocabulary.learn_sentence(&prose.sentence());
    }
    let start = Instant::now();
    vocabulary.save();
    println!(
        "{:<52} {:>11.3?}   ({} KB)",
        "save a new vocabulary",
        start.elapsed(),
        std::fs::metadata(&path).unwrap().len() / 1000
    );
    time("load the vocabulary", 5, 1, || {
        Vocabulary::load_from(path.clone(), dictionary.clone())
    });
    time("save one more sentence", 5, 1, || {
        vocabulary.learn_sentence(&prose.sentence());
        vocabulary.save();
    });
}
