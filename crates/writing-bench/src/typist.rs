//! A typist replaying one writer's pieces, and what it counts.
//!
//! The typist knows the text it means to type. A ghost is right when that
//! text goes on with all of it, case included, and a word ends right after
//! it. The Tab typist takes a right ghost with Tab and otherwise types the
//! next character. The word typist also takes a ghost's right leading words
//! with the next-word key. The blind typist never takes one. The slip
//! typist is the Tab typist, except that it closes one right suggestion in
//! [`SLIP_EVERY`] with Escape by mistake and then types the word by hand.
//!
//! A typist takes a ghost only once it has been on screen for the reaction
//! time, so a ghost that comes and goes between two keys in flow is typed
//! over, and counted as a flicker.

use crate::corpus::Piece;
use gpui_kit_writing::Dictionary;
use gpui_kit_writing::replay::{Field, Paced, Policy, QuietTiming, SuggestionMode, Writer};
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
    /// Gaps spread as real typists' are, around this median between the
    /// letters of a word, in milliseconds. See [`natural_gap`].
    Natural(u32),
    /// The gaps the corpus recorded, one per character typed.
    Recorded,
}

/// The gap before the key at `offset` of `text` for a typist whose letters
/// come `median_ms` apart in the median. The rolls come from `seed` and the
/// offset, so a piece types the same way in every run.
///
/// The numbers, and where they come from. None was measured here: they are
/// read from the literature on typing, and the owner's own recordings will
/// replace them.
///
/// - Speed. Dhakal and others (CHI 2018, 168,000 volunteers) found a mean
///   of 52 words a minute with a standard deviation of 20, the fastest over
///   120. At five characters and a space a word, 250 ms between keys is 40
///   words a minute, 180 ms is 56, 120 ms is 83 and 90 ms is 111.
/// - Spread. Salthouse (Psychological Bulletin, 1986) reports gaps inside a
///   word varying with the letter pair by about a third of the mean either
///   way. A log-normal spread of 0.35 gives that, and makes one gap in a
///   hundred longer than 2.2 medians.
/// - Word start. The first letter of a word comes later in transcription
///   (Salthouse's word-initiation effect, 20 to 50%), and later still in
///   composition, where keystroke logging (Leijten and Van Waes; Chukharev-
///   Hudilainen, 2014) finds the gap before a word about one and a half to
///   two times the gap inside one. 1.6 medians, spread 0.5.
/// - Hitches. Salthouse finds a few per cent of gaps inside a word at twice
///   the median or more. One word in sixteen gets a hitch of 600 ms (spread
///   0.5) at one of its inner letters, one in ten of seven letters or more.
/// - Thinking. Composition pauses before one word in twenty, 1,500 ms
///   (spread 0.6), as the thinking typist has it.
/// - Punctuation. A sentence starts after 1,000 ms (spread 0.6), a clause
///   after a comma, colon or semicolon after 500 ms (spread 0.5). Wengelin
///   (2006) puts the median pause at a sentence boundary in composition
///   between one and two seconds, and shorter inside one.
/// - A piece starts after 1,000 ms.
fn natural_gap(
    median_ms: u32,
    text: &str,
    offset: usize,
    spans: &[Range<usize>],
    seed: u64,
) -> Duration {
    const SENTENCE_MS: f64 = 1000.0;
    const CLAUSE_MS: f64 = 500.0;
    const THINKING_MS: f64 = 1500.0;
    const HITCH_MS: f64 = 600.0;
    const IN_WORD_SPREAD: f64 = 0.35;
    const WORD_START: f64 = 1.6;
    const WORD_START_SPREAD: f64 = 0.5;
    const THINKING_ONE_IN: u64 = 20;
    const HITCH_PER_CENT: u64 = 6;
    const HITCH_PER_CENT_LONG: u64 = 10;
    const LONG_WORD: usize = 7;

    let median = f64::from(median_ms);
    let mut before = text[..offset].chars().rev();
    let previous = before.next();
    let earlier = before.next();
    let Some(current) = text[offset..].chars().next() else {
        return Duration::ZERO;
    };
    let in_word = is_word_char_at(text, offset, current);
    let gap = |salt, median_ms, spread| log_normal(seed, offset, salt, median_ms, spread);
    match (previous, earlier) {
        (None, _) => Duration::from_millis(SENTENCE_MS as u64),
        (Some('\n'), _) | (Some(' '), Some('.' | '!' | '?')) => gap(1, SENTENCE_MS, 0.6),
        (Some(' '), Some(',' | ';' | ':')) => gap(2, CLAUSE_MS, 0.5),
        (Some(' '), _) if in_word => {
            if roll(seed, offset, 3) % THINKING_ONE_IN == 0 {
                gap(4, THINKING_MS, 0.6)
            } else {
                gap(5, WORD_START * median, WORD_START_SPREAD)
            }
        }
        (Some(p), _) if in_word && is_word_char_at(text, offset - p.len_utf8(), p) => {
            let hitch = span_at(spans, offset).is_some_and(|span| {
                let span = &spans[span];
                let letters = text[span.clone()].chars().count();
                let per_cent = if letters >= LONG_WORD {
                    HITCH_PER_CENT_LONG
                } else {
                    HITCH_PER_CENT
                };
                let word = roll(seed, span.start, 6);
                letters > 1
                    && word % 100 < per_cent
                    && text[span.start..offset].chars().count()
                        == 1 + (word / 100) as usize % (letters - 1)
            });
            if hitch {
                gap(7, HITCH_MS, 0.5)
            } else {
                gap(8, median, IN_WORD_SPREAD)
            }
        }
        _ => gap(9, median, IN_WORD_SPREAD),
    }
}

/// A gap drawn from a log-normal spread around `median_ms`, by the rolls
/// of `seed`, `offset` and `salt`, to the millisecond.
fn log_normal(seed: u64, offset: usize, salt: u64, median_ms: f64, spread: f64) -> Duration {
    let uniform = |salt| (roll(seed, offset, salt) >> 11) as f64 / (1u64 << 53) as f64;
    let u1 = uniform(salt).max(1e-12);
    let u2 = uniform(salt + 100);
    let normal = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    Duration::from_millis((median_ms * (spread * normal).exp()).round() as u64)
}

/// A roll that depends on `seed`, `offset` and `salt` alone.
fn roll(seed: u64, offset: usize, salt: u64) -> u64 {
    let mut z = seed
        ^ (offset as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ salt.wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The seed of a piece's rolls, from the writer's id and the piece's
/// place in their stream.
fn piece_seed(writer: &str, index: usize) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in writer.bytes().chain(index.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
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
    /// How Quiet times its suggestions.
    pub quiet_timing: QuietTiming,
    /// How long a ghost must have been on screen before the typist can
    /// take it.
    pub reaction: Duration,
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
            quiet_timing: QuietTiming::FixedPause,
            reaction: Duration::ZERO,
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
            "quiet_timing": format!("{:?}", self.quiet_timing),
            "reaction_ms": self.reaction.as_millis(),
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
    /// Offers typed over before they had been on screen for the reaction
    /// time: ghosts that came and went while the typist kept typing.
    flickers,
    /// Requests whose suggestion was held back by the typist's rhythm.
    held,
    /// Their holds, added up, in milliseconds.
    hold_ms_sum,
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
/// types along it, whether it was right when it appeared, and when on the
/// clock it appeared.
struct Track {
    expected: String,
    right: bool,
    /// Whether the slip typist has made its choice about this one.
    decided: bool,
    shown_at: Duration,
}

struct State<'a> {
    settings: &'a Settings,
    writer: &'a Writer,
    histogram: Vec<u64>,
    right_offers: u64,
    /// Where the latest offer appeared, in the current text.
    last_offer_at: usize,
}

/// A typed segment's gaps, as the corpus recorded them, with the gap
/// standing in for a character the writer didn't type.
struct Recorded<'a> {
    gaps_ms: &'a [Option<u32>],
    fallback: Duration,
}

impl<'a> Recorded<'a> {
    /// The segment's recorded gaps, if it has them. A character without one
    /// gets the median of those it has, or the fluent typist's key.
    fn of(gaps_ms: Option<&'a [Option<u32>]>) -> Option<Self> {
        let gaps_ms = gaps_ms?;
        let mut known: Vec<u32> = gaps_ms.iter().flatten().copied().collect();
        known.sort_unstable();
        let fallback = known
            .get(known.len() / 2)
            .map_or(Duration::from_millis(180), |ms| {
                Duration::from_millis(u64::from(*ms))
            });
        Some(Self { gaps_ms, fallback })
    }

    fn gap(&self, char_index: usize) -> Duration {
        self.gaps_ms
            .get(char_index)
            .copied()
            .flatten()
            .map_or(self.fallback, |ms| Duration::from_millis(u64::from(ms)))
    }
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
    writer.set_quiet_timing(settings.quiet_timing);
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
    for (index, piece) in pieces.iter().enumerate() {
        writer.wait(Duration::from_secs(piece.gap_s));
        let mut field = writer.field();
        let mut tally = Tally::default();
        let seed = piece_seed(id, index);
        for segment in &piece.segments {
            if segment.pasted {
                state.timed(|| field.paste(&segment.text));
                state.drain(&mut field, "", 0, &[], &mut tally);
            } else {
                let recorded = Recorded::of(segment.gaps_ms.as_deref());
                state.type_text(&mut field, &segment.text, seed, recorded, &mut tally);
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

    fn type_text(
        &mut self,
        field: &mut Field,
        text: &str,
        seed: u64,
        recorded: Option<Recorded<'_>>,
        tally: &mut Tally,
    ) {
        let spans = word_spans(text);
        tally.chars += text.chars().count() as u64;
        tally.words += spans.len() as u64;
        let mut covered = vec![false; spans.len()];
        let mut reached = 0;
        let mut track: Option<Track> = None;
        let mut offset = 0;
        let mut char_index = 0;
        while offset < text.len() {
            let gap = match (self.settings.timing, &recorded) {
                (Timing::Recorded, Some(recorded)) => recorded.gap(char_index),
                (Timing::Natural(median_ms), _) => {
                    natural_gap(median_ms, text, offset, &spans, seed)
                }
                (timing, _) => gap_before(timing, text, offset, &spans),
            };
            // A key's time is the wait before it, which may send a request
            // Quiet held back, and the key itself.
            let started = self.settings.latency.then(Instant::now);
            field.wait(gap);
            let now = self.writer.elapsed();
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
            let action = self.decide(field.ghost().as_deref(), rest, next, &mut track, now);
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
                    char_index += 1;
                    typed = Some(character);
                    if let Some(track) = track.as_mut() {
                        match track.expected.strip_prefix(character) {
                            Some(rest) => track.expected = rest.to_string(),
                            None => {
                                track.expected.clear();
                                if now.saturating_sub(track.shown_at) < self.settings.reaction {
                                    tally.flickers += 1;
                                }
                            }
                        }
                    }
                }
                Action::Tab => {
                    let inserted = field.tab().unwrap_or_default();
                    tally.keys += 1;
                    tally.accepts += 1;
                    self.count_saved(&inserted, tally);
                    offset += inserted.len();
                    char_index += inserted.chars().count();
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
                    char_index += inserted.chars().count();
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
        now: Duration,
    ) -> Action {
        let kind = self.settings.typist;
        let Some(ghost) = ghost.filter(|_| kind != TypistKind::Blind) else {
            return Action::Char(next);
        };
        // A ghost that hasn't been on screen for the reaction time hasn't
        // been read yet.
        let shown_for = track
            .as_ref()
            .map_or(Duration::ZERO, |track| now.saturating_sub(track.shown_at));
        if shown_for < self.settings.reaction {
            return Action::Char(next);
        }
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
            shown_at: field.shown_at().unwrap_or_default(),
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
            if !asked.hold.is_zero() {
                tally.held += 1;
                tally.hold_ms_sum += asked.hold.as_millis() as u64;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The natural typist's gaps are the same in every run, spread around
    /// the median inside a word, longer before a word, and a few words get
    /// a hitch.
    #[test]
    fn natural_gaps_spread_around_the_median_and_repeat() {
        let text = "the quick brown fox jumps over the lazy dog. Nothing happened, apparently. "
            .repeat(300);
        let spans = word_spans(&text);
        let gaps = |seed| -> Vec<u64> {
            (0..text.len())
                .map(|offset| natural_gap(150, &text, offset, &spans, seed).as_millis() as u64)
                .collect()
        };
        let first = gaps(7);
        assert_eq!(first, gaps(7));
        assert_ne!(first, gaps(8));
        assert_eq!(&first[..4], &[1000, 56, 298, 135]);

        let bytes: Vec<(usize, char)> = text.char_indices().collect();
        let mut in_word = Vec::new();
        let mut word_start = Vec::new();
        for window in bytes.windows(2) {
            let ((_, previous), (offset, current)) = (window[0], window[1]);
            if previous.is_alphabetic() && current.is_alphabetic() {
                in_word.push(first[offset]);
            } else if previous == ' ' && current.is_alphabetic() {
                let earlier = text[..offset - 1].chars().next_back();
                if earlier.is_some_and(char::is_alphabetic) {
                    word_start.push(first[offset]);
                }
            }
        }
        in_word.sort_unstable();
        word_start.sort_unstable();
        let median = |gaps: &[u64]| gaps[gaps.len() / 2];
        assert!(
            (140..=160).contains(&median(&in_word)),
            "{}",
            median(&in_word)
        );
        assert!(
            (215..=265).contains(&median(&word_start)),
            "{}",
            median(&word_start)
        );
        // Hitches and the spread's tail: a few per cent of gaps inside a
        // word are over twice the median.
        let slow = in_word.iter().filter(|gap| **gap > 330).count();
        let share = slow as f64 / in_word.len() as f64;
        assert!((0.01..0.06).contains(&share), "{share}");
        let hitched = spans
            .iter()
            .filter(|span| (span.start + 1..span.end).any(|offset| first[offset] > 400))
            .count();
        let share = hitched as f64 / spans.len() as f64;
        assert!((0.04..0.12).contains(&share), "{share}");
    }

    /// Recorded gaps replay as they are, and a character the writer didn't
    /// type gets the median of those they did.
    #[test]
    fn recorded_gaps_replay_with_a_fallback() {
        let gaps = [Some(100), None, Some(300), Some(120)];
        let recorded = Recorded::of(Some(&gaps[..])).unwrap();
        let ms = |index| recorded.gap(index).as_millis();
        assert_eq!(
            (ms(0), ms(1), ms(2), ms(3), ms(9)),
            (100, 120, 300, 120, 120)
        );
        assert_eq!(
            Recorded::of(Some(&[None][..])).unwrap().gap(0).as_millis(),
            180
        );
        assert!(Recorded::of(None).is_none());
    }
}
