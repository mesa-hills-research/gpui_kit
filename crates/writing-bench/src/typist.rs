//! A typist replaying one writer's pieces, and what it counts.
//!
//! The typist knows the text it means to type. A ghost is right when that
//! text goes on with all of it, case included, and a word ends right after
//! it. The Tab typist takes a right ghost with Tab and otherwise types the
//! next character. The word typist also takes a ghost's right leading words
//! with the next-word key. The blind typist never takes one. The slip
//! typist is the Tab typist, except that it closes one right suggestion in
//! [`SLIP_EVERY`] with Escape by mistake and then types the word by hand.

use crate::corpus::Piece;
use gpui_kit_writing::Dictionary;
use gpui_kit_writing::replay::{Field, Paced, Policy, SuggestionMode, Writer};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How often the slip typist closes a right suggestion with Escape.
const SLIP_EVERY: u64 = 10;

/// The writer's typing rhythm, which decides when Quiet's pauses come.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    /// 180 ms a key, 600 ms before a sentence, 350 ms after a comma.
    Fluent,
    /// Fluent, plus a 700 ms hesitation before 15% of the words of seven
    /// letters or more and after the second letter of another 15%.
    Hesitant,
    /// Hesitant, plus a 1,500 ms pause to think before one word in twenty.
    Thinking,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TypistKind {
    Tab,
    Word,
    Blind,
    Slip,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub mode: SuggestionMode,
    pub timing: Timing,
    pub policy: Policy,
    pub typist: TypistKind,
    /// Whether to time every key's work.
    pub latency: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: SuggestionMode::Eager,
            timing: Timing::Fluent,
            policy: Policy::Default,
            typist: TypistKind::Tab,
            latency: false,
        }
    }
}

impl Settings {
    pub fn describe(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": format!("{:?}", self.mode),
            "timing": format!("{:?}", self.timing),
            "policy": format!("{:?}", self.policy),
            "typist": format!("{:?}", self.typist),
            "latency": self.latency,
        })
    }
}

macro_rules! tally {
    ($($(#[$doc:meta])* $field:ident),* $(,)?) => {
        /// Counts over some stretch of typing.
        #[derive(Clone, Debug, Default, Serialize)]
        pub struct Tally {
            $($(#[$doc])* pub $field: u64,)*
        }

        impl Tally {
            fn add(&mut self, other: &Tally) {
                $(self.$field += other.$field;)*
            }
        }
    };
}

tally! {
    /// Characters typed without suggestions, pasted text left out.
    chars,
    /// Keys pressed with suggestions, counting characters, Tab, the
    /// next-word key and Escape.
    keys,
    /// Words typed.
    words,
    /// Words the vocabulary knew and trusted when the typist reached them.
    known_words,
    /// Words whose right completion or prediction showed before they were
    /// finished.
    covered_words,
    /// Ghosts shown, from when one appears until it goes.
    offers,
    /// Offers right in full.
    right,
    /// Offers whose first word is right and later words aren't.
    partly,
    /// Offers whose first word is wrong.
    wrong,
    shown_chars,
    /// Characters of wrong ghosts, and of the wrong part of partly right ones.
    wrong_chars,
    /// Right offers of two characters or fewer.
    late,
    /// A right ghost replaced by another while the typist typed along it.
    changes_of_mind,
    /// Suggestions taken, with Tab or the next-word key.
    accepts,
    word_keys,
    /// Characters inserted by accepted suggestions.
    saved_chars,
    /// Of them, those from accepts of two words or more.
    phrase_saved_chars,
    /// Right suggestions closed with Escape by mistake.
    slips,
    requests,
    /// Requests where the model had a suggestion.
    model_some,
    /// Requests where one showed.
    shown_some,
    /// Offers that pacing counted as ignored or refused.
    ignored,
    ignored_weight,
    /// Of them, offers of the very word the typist then wrote.
    wrongful_ignores,
    /// Requests where pacing withheld the model's word as ignored before.
    suppressed,
    /// Of them, those where the withheld word was right.
    suppressed_right,
    /// Turned-down suggestions whose word the typist wrote anyway, which
    /// pacing then counted for nothing.
    restored,
    /// Quiet's pause at each request, added up, in milliseconds.
    pause_ms_sum,
}

#[derive(Serialize)]
pub struct WriterResult {
    pub writer: String,
    pub split: String,
    pub total: Tally,
    pub by_category: BTreeMap<String, Tally>,
    /// By words written before each piece, under 1,000, to 5,000, to
    /// 20,000, and beyond.
    pub curve: [Tally; 4],
    /// Microseconds per key, as (bucket floor, count), when timed.
    pub latency: Option<Vec<(u64, u64)>>,
    pub vocabulary_words: usize,
    pub elapsed_ms: u128,
}

/// A ghost being followed, with what it should be next if the typist
/// types along it, and whether it was right when it appeared.
struct Track {
    expected: String,
    right: bool,
    /// Whether the slip typist has made its choice about this one.
    decided: bool,
}

struct State<'a> {
    settings: &'a Settings,
    writer: &'a Writer,
    histogram: Vec<u64>,
    right_offers: u64,
    /// Where the latest offer appeared, in the current text.
    last_offer_at: usize,
}

/// Replays `pieces`, one writer's, in order, and counts.
pub fn replay(
    id: &str,
    split: &str,
    pieces: &[Piece],
    settings: &Settings,
    dictionary: &Rc<Dictionary>,
) -> WriterResult {
    let started = Instant::now();
    let writer = Writer::with_policy(settings.mode, dictionary.clone(), settings.policy);
    let mut state = State {
        settings,
        writer: &writer,
        histogram: vec![0; HISTOGRAM_BUCKETS],
        right_offers: 0,
        last_offer_at: 0,
    };
    let mut total = Tally::default();
    let mut by_category: BTreeMap<String, Tally> = BTreeMap::new();
    let mut curve: [Tally; 4] = Default::default();
    let mut written = 0u64;
    for piece in pieces {
        writer.wait(Duration::from_secs(piece.gap_s));
        let mut field = writer.field();
        let mut tally = Tally::default();
        for segment in &piece.segments {
            if segment.pasted {
                state.timed(|| field.paste(&segment.text));
                state.drain(&mut field, "", 0, &[], &mut tally);
            } else {
                state.type_text(&mut field, &segment.text, &mut tally);
            }
        }
        if piece.surface == "composer" {
            state.timed(|| field.send());
            state.drain(&mut field, "", 0, &[], &mut tally);
        }
        let window = match written {
            0..1_000 => 0,
            1_000..5_000 => 1,
            5_000..20_000 => 2,
            _ => 3,
        };
        curve[window].add(&tally);
        by_category
            .entry(piece.category.clone())
            .or_default()
            .add(&tally);
        total.add(&tally);
        written += tally.words;
    }
    let latency = settings.latency.then(|| {
        state
            .histogram
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(bucket, count)| (bucket_floor(bucket), *count))
            .collect()
    });
    WriterResult {
        writer: id.to_string(),
        split: split.to_string(),
        total,
        by_category,
        curve,
        latency,
        vocabulary_words: writer.vocabulary_size().0,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

/// Microsecond buckets, one for each below 1 ms, then 10 µs wide to 100 ms.
const HISTOGRAM_BUCKETS: usize = 1000 + 9900;

fn bucket(micros: u64) -> usize {
    if micros < 1000 {
        micros as usize
    } else {
        (1000 + (micros - 1000) / 10).min(HISTOGRAM_BUCKETS as u64 - 1) as usize
    }
}

fn bucket_floor(bucket: usize) -> u64 {
    if bucket < 1000 {
        bucket as u64
    } else {
        1000 + (bucket as u64 - 1000) * 10
    }
}

/// What the typist does next.
enum Action {
    Char(char),
    Tab,
    Words(usize),
    Escape,
}

impl State<'_> {
    fn timed<R>(&mut self, work: impl FnOnce() -> R) -> R {
        if !self.settings.latency {
            return work();
        }
        let start = Instant::now();
        let result = work();
        self.record(start.elapsed());
        result
    }

    fn record(&mut self, elapsed: Duration) {
        self.histogram[bucket(elapsed.as_micros() as u64)] += 1;
    }

    fn type_text(&mut self, field: &mut Field, text: &str, tally: &mut Tally) {
        let spans = word_spans(text);
        tally.chars += text.chars().count() as u64;
        tally.words += spans.len() as u64;
        let mut covered = vec![false; spans.len()];
        let mut reached = 0;
        let mut track: Option<Track> = None;
        let mut offset = 0;
        while offset < text.len() {
            let gap = gap_before(self.settings.timing, text, offset, &spans);
            // A key's time is the wait before it, which may send a request
            // Quiet held back, and the key itself.
            let started = self.settings.latency.then(Instant::now);
            field.wait(gap);
            self.observe(
                field,
                text,
                offset,
                &spans,
                &mut covered,
                &mut track,
                None,
                tally,
            );
            self.drain(field, text, offset, &spans, tally);
            while reached < spans.len() && spans[reached].start <= offset {
                if self.writer.knows(&text[spans[reached].clone()]) {
                    tally.known_words += 1;
                }
                reached += 1;
            }
            let rest = &text[offset..];
            let next = rest.chars().next().unwrap();
            let action = self.decide(field.ghost().as_deref(), rest, next, &mut track);
            let mut typed = None;
            match action {
                Action::Char(character) => {
                    if character == '\n' {
                        field.enter();
                    } else {
                        let key = character.to_string();
                        field.type_key(&key);
                    }
                    tally.keys += 1;
                    offset += character.len_utf8();
                    typed = Some(character);
                    if let Some(track) = track.as_mut() {
                        match track.expected.strip_prefix(character) {
                            Some(rest) => track.expected = rest.to_string(),
                            None => track.expected.clear(),
                        }
                    }
                }
                Action::Tab => {
                    let inserted = field.tab().unwrap_or_default();
                    tally.keys += 1;
                    tally.accepts += 1;
                    self.count_saved(&inserted, tally);
                    offset += inserted.len();
                    track = None;
                }
                Action::Words(count) => {
                    let mut inserted = String::new();
                    for _ in 0..count {
                        if let Some(word) = field.accept_word() {
                            tally.keys += 1;
                            tally.word_keys += 1;
                            inserted.push_str(&word);
                        }
                    }
                    tally.accepts += 1;
                    self.count_saved(&inserted, tally);
                    offset += inserted.len();
                    if let Some(track) = track.as_mut() {
                        match track.expected.strip_prefix(inserted.as_str()) {
                            Some(rest) => track.expected = rest.to_string(),
                            None => track.expected.clear(),
                        }
                    }
                }
                Action::Escape => {
                    field.escape();
                    tally.keys += 1;
                    tally.slips += 1;
                    track = None;
                }
            }
            if let Some(started) = started {
                self.record(started.elapsed());
            }
            self.observe(
                field,
                text,
                offset,
                &spans,
                &mut covered,
                &mut track,
                typed,
                tally,
            );
            self.drain(field, text, offset, &spans, tally);
        }
        tally.covered_words += covered.iter().filter(|covered| **covered).count() as u64;
    }

    fn count_saved(&self, inserted: &str, tally: &mut Tally) {
        let chars = inserted.chars().count() as u64;
        tally.saved_chars += chars;
        if inserted.trim().contains(' ') {
            tally.phrase_saved_chars += chars;
        }
    }

    fn decide(
        &mut self,
        ghost: Option<&str>,
        rest: &str,
        next: char,
        track: &mut Option<Track>,
    ) -> Action {
        let kind = self.settings.typist;
        let Some(ghost) = ghost.filter(|_| kind != TypistKind::Blind) else {
            return Action::Char(next);
        };
        if is_right(ghost, rest) {
            if kind == TypistKind::Slip
                && let Some(track) = track.as_mut()
                && !track.decided
            {
                track.decided = true;
                self.right_offers += 1;
                if self.right_offers % SLIP_EVERY == 0 {
                    return Action::Escape;
                }
            }
            return Action::Tab;
        }
        if kind == TypistKind::Word {
            let ends = word_ends(ghost);
            if let Some((count, end)) = ends
                .iter()
                .enumerate()
                .rev()
                .find(|(_, end)| is_right(&ghost[..**end], rest))
                && ghost[..*end].chars().count() > count + 1
            {
                return Action::Words(count + 1);
            }
        }
        Action::Char(next)
    }

    /// Follows the ghost after an action, the same offer typed along or a
    /// new one, which it judges against the text.
    #[allow(clippy::too_many_arguments)]
    fn observe(
        &mut self,
        field: &Field,
        text: &str,
        offset: usize,
        spans: &[Range<usize>],
        covered: &mut [bool],
        track: &mut Option<Track>,
        typed: Option<char>,
        tally: &mut Tally,
    ) {
        let ghost = field.ghost();
        if let (Some(current), Some(ghost)) = (track.as_ref(), ghost.as_ref())
            && !current.expected.is_empty()
            && *ghost == current.expected
        {
            return;
        }
        let previous = track.take();
        let Some(ghost) = ghost else {
            return;
        };
        if let Some(previous) = previous
            && previous.right
            && typed.is_some()
            && !previous.expected.is_empty()
        {
            tally.changes_of_mind += 1;
        }
        let rest = &text[offset..];
        let chars = ghost.chars().count() as u64;
        tally.offers += 1;
        tally.shown_chars += chars;
        let right = is_right(&ghost, rest);
        let first = &ghost[..word_ends(&ghost).first().copied().unwrap_or(ghost.len())];
        let first_right = is_right(first, rest);
        if right {
            tally.right += 1;
            if chars <= 2 {
                tally.late += 1;
            }
        } else if first_right {
            tally.partly += 1;
            tally.wrong_chars += chars - first.chars().count() as u64;
        } else {
            tally.wrong += 1;
            tally.wrong_chars += chars;
        }
        if first_right && let Some(span) = span_at(spans, offset) {
            covered[span] = true;
        }
        self.last_offer_at = offset;
        *track = Some(Track {
            expected: ghost,
            right,
            decided: false,
        });
    }

    /// Takes in the requests answered and what pacing did since the last
    /// call.
    fn drain(
        &mut self,
        field: &mut Field,
        text: &str,
        offset: usize,
        spans: &[Range<usize>],
        tally: &mut Tally,
    ) {
        for asked in field.take_answers() {
            tally.requests += 1;
            tally.model_some += u64::from(asked.model.is_some());
            tally.shown_some += u64::from(asked.shown.is_some());
            if self.settings.mode == SuggestionMode::Quiet {
                tally.pause_ms_sum += self.writer.pause().as_millis() as u64;
            }
        }
        let written =
            |at: usize| span_at(spans, at).map(|span| text[spans[span].clone()].to_lowercase());
        for paced in self.writer.take_paced() {
            match paced {
                Paced::Ignored { word, weight, .. } => {
                    tally.ignored += 1;
                    tally.ignored_weight += u64::from(weight);
                    if written(self.last_offer_at)
                        .is_some_and(|written| written == word.to_lowercase())
                    {
                        tally.wrongful_ignores += 1;
                    }
                }
                Paced::Restored { .. } => tally.restored += 1,
                Paced::Suppressed { word, .. } => {
                    tally.suppressed += 1;
                    if written(offset).is_some_and(|written| written == word.to_lowercase()) {
                        tally.suppressed_right += 1;
                    }
                }
            }
        }
    }
}

/// The pause before the key at `offset` of `text`.
fn gap_before(timing: Timing, text: &str, offset: usize, spans: &[Range<usize>]) -> Duration {
    let mut before = text[..offset].chars().rev();
    let previous = before.next();
    let earlier = before.next();
    let ms = match (previous, earlier) {
        (None, _) | (Some('\n'), _) | (Some(' '), Some('.' | '!' | '?')) => 600,
        (Some(' '), Some(',')) => 350,
        _ => 180,
    };
    if timing == Timing::Thinking
        && ms == 180
        && let Some(span) = spans.iter().find(|span| span.start == offset)
        && ((span.start as u64).wrapping_mul(0xD1B5_4A32_D192_ED03) >> 40) % 20 == 0
    {
        return Duration::from_millis(1500);
    }
    if matches!(timing, Timing::Hesitant | Timing::Thinking)
        && ms == 180
        && let Some(span) = spans
            .iter()
            .find(|span| span.start <= offset && offset < span.end)
        && text[span.clone()].chars().count() >= 7
    {
        let roll = (span.start as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
        let roll = roll % 100;
        let into = text[span.start..offset].chars().count();
        if (into == 0 && roll < 15) || (into == 2 && (15..30).contains(&roll)) {
            return Duration::from_millis(700);
        }
    }
    Duration::from_millis(ms)
}

/// Whether `character` at byte `index` of `text` belongs to a word, as the
/// suggestions see it. That is a letter, a digit, `_` or `-`, or an
/// apostrophe between two letters.
fn is_word_char_at(text: &str, index: usize, character: char) -> bool {
    if character.is_alphanumeric() || character == '_' || character == '-' {
        return true;
    }
    (character == '\'' || character == '\u{2019}')
        && text[..index]
            .chars()
            .next_back()
            .is_some_and(char::is_alphabetic)
        && text[index + character.len_utf8()..]
            .chars()
            .next()
            .is_some_and(char::is_alphabetic)
}

/// Whether `ghost` is right where `rest` is what the typist means to type
/// next, all of it, case included, with a word ending right after it.
fn is_right(ghost: &str, rest: &str) -> bool {
    !ghost.is_empty()
        && rest.starts_with(ghost)
        && rest[ghost.len()..]
            .chars()
            .next()
            .is_none_or(|next| !is_word_char_at(rest, ghost.len(), next))
}

/// The ends of the words of `ghost`, as the next-word key takes them, each
/// leading spaces and then a word or one other character.
fn word_ends(ghost: &str) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut start = 0;
    while start < ghost.len() {
        let rest = &ghost[start..];
        let lead = rest.len() - rest.trim_start().len();
        let word = &rest[lead..];
        let mut end = word.len();
        for (index, character) in word.char_indices() {
            let word_char = character.is_alphanumeric()
                || character == '_'
                || ((character == '\'' || character == '\u{2019}')
                    && is_word_char_at(word, index, character));
            if !word_char {
                end = if index == 0 {
                    character.len_utf8()
                } else {
                    index
                };
                break;
            }
        }
        if lead + end == 0 {
            break;
        }
        start += lead + end;
        ends.push(start);
    }
    ends
}

/// The words of `text` with a letter in them, as byte ranges.
fn word_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if is_word_char_at(text, index, character) {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            spans.push(start..index);
        }
    }
    if let Some(start) = start {
        spans.push(start..text.len());
    }
    spans.retain(|span| text[span.clone()].chars().any(char::is_alphabetic));
    spans
}

/// The word an offer at `offset` is about, the one the caret is in or the
/// one that starts there.
fn span_at(spans: &[Range<usize>], offset: usize) -> Option<usize> {
    let index = spans.partition_point(|span| span.end < offset);
    let span = spans.get(index)?;
    (span.start <= offset).then_some(index)
}
