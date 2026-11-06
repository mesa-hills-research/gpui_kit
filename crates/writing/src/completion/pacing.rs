//! When a word suggestion shows, as opposed to which one.
//!
//! The model in the parent module decides what would complete the text at the
//! caret. This module decides whether to show it now, in the mode the user
//! chose:
//!
//! - **Off** offers nothing.
//! - **Eager** offers the model's suggestion after every keystroke.
//! - **Quiet**, the default, waits for typing to pause and backs off from
//!   suggestions the user types past. Someone typing a word they know never
//!   sees a suggestion. Someone who hesitates finds one there.
//!
//! # Ignored suggestions
//!
//! A suggestion is *offered* when it shows as ghost text after the caret.
//! From then on it is *pending* until one of these settles it:
//!
//! - **Accepted.** Tab takes it. Ignored suggestions stop counting, and the
//!   next one comes after the plain pause again.
//! - **Typed along.** The user types the next letters of its first word. It
//!   stays pending, and on screen without waiting for a pause, so it doesn't
//!   blink away under keys that match it. It stays pending even when the
//!   model has nothing to show after the new letters.
//! - **Ignored.** The user types anything else at the caret: a letter the
//!   word doesn't continue with, a space or punctuation that ends the word
//!   short of it, Enter, or the word's last letter, which finishes it without
//!   Tab.
//! - **Refused.** Escape closes it. This counts as [`ESCAPE_WEIGHT`] ignored
//!   suggestions.
//! - **Left.** The user deletes, undoes, or moves the caret and types
//!   somewhere else. That says nothing about the suggestion, so it counts for
//!   nothing. A caret move is noticed when the next typing lands away from
//!   the suggestion, so moving away and back before typing counts as staying.
//!
//! A suggestion turned down by a key that leaves its word, or by Escape,
//! was wanted after all when the user goes on to write its word there
//! anyway, such as after a typo fixed with Backspace or an Escape pressed by
//! mistake. Once that word is finished, turning it down counts for nothing.
//! Finishing the word by typing its last letter isn't turning it down this
//! way, so it still counts as ignored.
//!
//! An ignored or refused word is not offered again for [`SUPPRESSION`] while
//! the user types the same word from the same start: the prefix it was
//! ignored after or a longer one, or, for a word predicted after a space, the
//! same previous word. When it is the model's best answer, nothing shows.
//! Each ignored suggestion also lengthens the pause before the next one by
//! [`IGNORED_EXTRA_PAUSE`], up to [`MAX_QUIET_PAUSE`], for the next
//! [`IGNORED_FADE_WORDS`] words typed or [`IGNORED_FADE`], whichever ends
//! first. So someone who types past a few suggestions sees fewer for a
//! while, and someone whose pauses are shorter than the longest pause still
//! finds suggestions once they have typed on.
//!
//! This state lives in memory for the session.
//!
//! # Timing by the typist's rhythm
//!
//! With [`QuietTiming::Rhythm`], Quiet asks the model at the key, as Eager
//! does, and holds its suggestion back for a share of the gap the typist's
//! own keys come at, shorter for a surer or longer word and for a typist
//! whose last keys came slowly, longer after ignored suggestions. The next
//! key drops a suggestion still held. A suggestion held in one textarea is
//! settled by time: when the next key or edit comes after it was due to
//! show, it showed, and counts as any shown suggestion does. [`Rhythm`]
//! learns the typist's gaps, and [`Hold`] is the unit that sets the wait.

use super::model::Suggested;
use super::vocabulary::fold;
use super::{Edit, extract_prefix, is_word_char, previous_two_words, word_end};
use gpui_base::input::{Rope, SuggestionEvent};
use serde::{Deserialize, Deserializer, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long typing must pause before Quiet offers a suggestion. Typing a
/// familiar word leaves well under this between keys, and a hesitation lasts
/// longer.
pub(crate) const QUIET_PAUSE: Duration = Duration::from_millis(300);

/// How much longer Quiet waits for each suggestion ignored in a row, so that
/// someone who keeps typing past them sees fewer.
pub(crate) const IGNORED_EXTRA_PAUSE: Duration = Duration::from_millis(150);

/// The longest Quiet waits, however many suggestions were ignored. A real
/// hesitation still brings one up.
pub(crate) const MAX_QUIET_PAUSE: Duration = Duration::from_millis(1000);

/// How many words typed an ignored suggestion lengthens the pause for.
pub(crate) const IGNORED_FADE_WORDS: u64 = 20;

/// How long an ignored suggestion lengthens the pause for, at most.
pub(crate) const IGNORED_FADE: Duration = Duration::from_secs(2 * 60);

/// How long an ignored word stays unoffered after the prefix it was ignored
/// at: the rest of a stretch of writing, after which it may be wanted again.
pub(crate) const SUPPRESSION: Duration = Duration::from_secs(10 * 60);

/// How many ignored suggestions Escape counts as. Escape is a clear no.
pub(crate) const ESCAPE_WEIGHT: u32 = 2;

/// How eagerly word suggestions show.
///
/// Serialized as `"off"`, `"quiet"` or `"eager"`. A boolean, as settings
/// saved when this was a switch hold it, reads `true` as Quiet and `false` as
/// Off, and a name this version doesn't know as the default, Quiet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionMode {
    /// No suggestions.
    Off,
    /// Suggestions after a pause in typing, fewer for words typed past.
    #[default]
    Quiet,
    /// A suggestion after every keystroke.
    Eager,
}

impl<'de> Deserialize<'de> for SuggestionMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Switch(bool),
            Name(String),
        }
        Ok(match Saved::deserialize(deserializer)? {
            Saved::Switch(true) => Self::Quiet,
            Saved::Switch(false) => Self::Off,
            Saved::Name(name) => match name.as_str() {
                "off" => Self::Off,
                "eager" => Self::Eager,
                // A mode this version doesn't know, from a later one.
                _ => Self::default(),
            },
        })
    }
}

/// How Quiet times a suggestion after the key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuietTiming {
    /// The model is asked once typing has paused for [`QUIET_PAUSE`], and
    /// longer after ignored suggestions, the same for every typist.
    #[default]
    FixedPause,
    /// The model is asked at the key, and its suggestion is held back for a
    /// share of the typist's own gap between keys. See [`Hold`].
    Rhythm,
}

/// The typist's rhythm: the gap their keys usually come at inside a word
/// and at the start of one, and whether their last few came slower.
///
/// Each median is a running mean of the logarithm of the gaps, so a pause
/// moves it little, and a gap enters it no further than [`Median::CLIP`]
/// from it. The first keys of a session count more, so a new typist's
/// rhythm is known within a few words. Only a key typed right after the
/// previous one, as one character, teaches it: a gap between two letters of
/// a word, or before the space that ends it, is inside the word, and a gap
/// after a space is at the start of one. Gaps after punctuation or a line
/// break are the writer's, not the typist's, and teach nothing.
pub(crate) struct Rhythm {
    /// The last key: the byte its text ended at, its last character, and
    /// when it came.
    last: Option<(usize, char, Instant)>,
    in_word: Median,
    word_start: Median,
    /// The mean of the last few gaps' logarithms over the rhythm's, halving
    /// in weight at each key. Above zero, the typist is slowing down.
    hesitation: f64,
}

/// A running median of gaps, as the mean of their logarithms.
struct Median {
    log_ms: f64,
    keys: u32,
}

impl Median {
    /// How far from the median a gap may enter, as a factor of three either
    /// way, so a pause counts as a slow key and no more.
    const CLIP: f64 = 1.0986;
    /// How many keys the median remembers once it is learned.
    const MEMORY: f64 = 32.0;

    fn new(prior_ms: f64) -> Self {
        Self {
            log_ms: prior_ms.ln(),
            keys: 0,
        }
    }

    fn learn(&mut self, gap_ms: f64) {
        let log = gap_ms
            .max(1.0)
            .ln()
            .clamp(self.log_ms - Self::CLIP, self.log_ms + Self::CLIP);
        let rate = (1.0 / (f64::from(self.keys) + 2.0)).max(1.0 / Self::MEMORY);
        self.log_ms += rate * (log - self.log_ms);
        self.keys += 1;
    }

    fn ms(&self) -> f64 {
        self.log_ms.exp()
    }
}

impl Rhythm {
    /// A rhythm before any key: a touch typist of about 65 words a minute,
    /// 180 ms between letters, and 300 ms before a word.
    const IN_WORD_PRIOR_MS: f64 = 180.0;
    const WORD_START_PRIOR_MS: f64 = 300.0;

    fn new() -> Self {
        Self {
            last: None,
            in_word: Median::new(Self::IN_WORD_PRIOR_MS),
            word_start: Median::new(Self::WORD_START_PRIOR_MS),
            hesitation: 0.0,
        }
    }

    /// Hears that `text` was typed at byte `offset` at `now`.
    fn key(&mut self, offset: usize, text: &str, now: Instant) {
        let mut chars = text.chars();
        let Some(first) = chars.next() else {
            return;
        };
        let single = chars.next().is_none();
        let last = self.last.replace((
            offset + text.len(),
            text.chars().last().unwrap_or(first),
            now,
        ));
        let Some((end, previous, at)) = last.filter(|_| single) else {
            return;
        };
        if end != offset {
            return;
        }
        let gap_ms = now.duration_since(at).as_secs_f64() * 1000.0;
        let median = if is_word_char(previous) && (is_word_char(first) || first == ' ') {
            &mut self.in_word
        } else if previous == ' ' && first.is_alphanumeric() {
            &mut self.word_start
        } else {
            return;
        };
        let ratio = (gap_ms.max(1.0) / median.ms()).ln().clamp(-1.0, 1.5);
        self.hesitation += 0.5 * (ratio - self.hesitation);
        median.learn(gap_ms);
    }

    /// The gap the typist's keys usually come at, inside a word or, with
    /// `at_word_start`, after the space before one.
    fn gap_ms(&self, at_word_start: bool) -> f64 {
        if at_word_start {
            self.word_start.ms()
        } else {
            self.in_word.ms()
        }
    }
}

/// How long Quiet holds a suggestion back after the key, as a multiple of
/// the typist's own gap at that place: one linear unit over what the
/// suggestion is worth and how the typist is going, in the logarithm of the
/// multiple, so each weight is a factor. Each number was set by reasoning.
///
/// The hold trades a suggestion that shows while the typist goes on
/// typing, which flickers and tells them nothing, against one that is there
/// when they pause. Gaps inside a word spread so that about one in a hundred
/// typed in flow is longer than 2.2 times their median, so that is what a
/// suggestion no surer than even waits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    /// The logarithm of the multiple a suggestion at a chance of one half,
    /// worth three keys, waits.
    pub base: f64,
    /// Added as the chance goes from one half to one.
    pub surer: f64,
    /// Per doubling of the keys a Tab would save.
    pub per_doubling: f64,
    /// Per ignored suggestion, by its weight.
    pub per_ignored: f64,
    /// Per unit of hesitation, the log ratio of the last gaps to the rhythm.
    pub per_hesitation: f64,
}

impl Hold {
    /// The weights set by reasoning.
    ///
    /// - A suggestion at an even chance worth three keys waits ln 2.2 =
    ///   0.788, that is 2.2 gaps.
    /// - A sure word flickers past a typist in flow without being wrong, so
    ///   it waits 0.61 of the base: −0.5.
    /// - A word worth twice as many keys shows 22% sooner: −0.25 a doubling.
    /// - Each ignored suggestion lengthens the wait by 35%, as the fixed
    ///   pause's first extra does by half: +0.3.
    /// - Keys coming at twice the rhythm, a log ratio of 0.69, shorten the
    ///   wait by 24%: −0.4.
    pub const HAND_WRITTEN: Self = Self {
        base: 0.788,
        surer: -0.5,
        per_doubling: -0.25,
        per_ignored: 0.3,
        per_hesitation: -0.4,
    };

    /// The longest hold, however many suggestions were ignored or however
    /// slow the typist.
    const MAX: Duration = Duration::from_millis(1500);

    /// How long to hold a suggestion whose first word is right with `chance`
    /// and which saves `gain` keys, for a typist with `rhythm` who ignored
    /// suggestions weighing `ignored`.
    fn wait(
        &self,
        rhythm: &Rhythm,
        at_word_start: bool,
        chance: f64,
        gain: usize,
        ignored: u32,
    ) -> Duration {
        let log_multiple = self.base
            + self.surer * (2.0 * chance - 1.0).clamp(0.0, 1.0)
            + self.per_doubling * (gain.max(1) as f64 / 3.0).log2()
            + self.per_ignored * f64::from(ignored)
            + self.per_hesitation * rhythm.hesitation.max(0.0);
        let ms = rhythm.gap_ms(at_word_start) * log_multiple.exp();
        Duration::from_secs_f64(ms / 1000.0).min(Self::MAX)
    }
}

/// What every textarea's suggestions share for the session: the mode, and
/// how the user has been answering suggestions.
pub(crate) struct SuggestionPacing {
    mode: Cell<SuggestionMode>,
    timing: Cell<QuietTiming>,
    hold_weights: Cell<Hold>,
    rhythm: RefCell<Rhythm>,
    /// Suggestions ignored since the last accepted one that still lengthen
    /// the pause.
    ignores: RefCell<Vec<Ignore>>,
    /// Words typed in the session, by the spaces typed. Enter comes as an
    /// edit, not a typed key, so a run of short lines fades by time.
    words: Cell<u64>,
    /// The number the next ignored suggestion gets.
    next_ignore: Cell<u64>,
    /// Lowercased ignored words, each with the lowercased leads it was
    /// ignored after and when that ends.
    suppressed: RefCell<HashMap<String, Vec<(String, Instant)>>>,
    /// What happened to the offers, for a replay to read.
    #[cfg(any(test, feature = "replay"))]
    log: RefCell<Vec<Paced>>,
}

/// Something the pacing did to an offer, as a replay sees it.
#[cfg(any(test, feature = "replay"))]
#[derive(Clone, Debug, PartialEq)]
pub enum Paced {
    /// The offer of `word` after `lead` was ignored, counting `weight`.
    Ignored {
        word: String,
        lead: String,
        weight: u32,
    },
    /// The model's `word` after `lead` wasn't shown, as it was ignored
    /// there before.
    Suppressed { word: String, lead: String },
    /// Turning `word` down counted for nothing, as the user wrote it anyway.
    Restored { word: String },
}

/// An ignored suggestion that lengthens the pause, with its weight, Escape
/// counting [`ESCAPE_WEIGHT`], and when it was ignored, in time and in words.
struct Ignore {
    id: u64,
    weight: u32,
    at: Instant,
    words: u64,
}

/// What ignoring an offer did, so that it can be undone. That is the ignore
/// that lengthens the pause, and the suppression of the lowercased word
/// after the lowercased lead until a time.
#[derive(Clone, Debug, PartialEq)]
struct Ignored {
    id: u64,
    word: String,
    lead: String,
    until: Instant,
}

impl SuggestionPacing {
    pub fn new(mode: SuggestionMode) -> Self {
        Self {
            mode: Cell::new(mode),
            timing: Cell::new(QuietTiming::default()),
            hold_weights: Cell::new(Hold::HAND_WRITTEN),
            rhythm: RefCell::new(Rhythm::new()),
            ignores: RefCell::new(Vec::new()),
            words: Cell::new(0),
            next_ignore: Cell::new(0),
            suppressed: RefCell::new(HashMap::new()),
            #[cfg(any(test, feature = "replay"))]
            log: RefCell::new(Vec::new()),
        }
    }

    /// What the pacing did since this was last called.
    #[cfg(any(test, feature = "replay"))]
    pub fn take_log(&self) -> Vec<Paced> {
        std::mem::take(&mut self.log.borrow_mut())
    }

    pub fn mode(&self) -> SuggestionMode {
        self.mode.get()
    }

    /// Switches every textarea to `mode` from its next keystroke.
    pub fn set_mode(&self, mode: SuggestionMode) {
        self.mode.set(mode);
    }

    pub fn timing(&self) -> QuietTiming {
        self.timing.get()
    }

    /// Switches how Quiet times its suggestions from the next keystroke.
    pub fn set_timing(&self, timing: QuietTiming) {
        self.timing.set(timing);
    }

    /// Sets the weights of the hold, for a replay to try others.
    #[cfg(any(test, feature = "replay"))]
    pub fn set_hold_weights(&self, weights: Hold) {
        self.hold_weights.set(weights);
    }

    /// The weight of the ignored suggestions that still lengthen the wait.
    fn ignored_weight(&self) -> u32 {
        self.ignores
            .borrow()
            .iter()
            .map(|ignore| ignore.weight)
            .sum()
    }

    /// How long typing must pause before Quiet offers a suggestion now,
    /// with the fixed pause.
    pub fn pause(&self) -> Duration {
        let extra = IGNORED_EXTRA_PAUSE.saturating_mul(self.ignored_weight());
        (QUIET_PAUSE + extra).min(MAX_QUIET_PAUSE)
    }

    /// How long Quiet holds back a suggestion whose first word is right with
    /// `chance` and which saves `gain` keys, by the typist's rhythm.
    pub fn hold(&self, at_word_start: bool, chance: f64, gain: usize) -> Duration {
        self.hold_weights.get().wait(
            &self.rhythm.borrow(),
            at_word_start,
            chance,
            gain,
            self.ignored_weight(),
        )
    }

    /// The gap the typist's keys usually come at, inside a word or at the
    /// start of one, as the rhythm has learned it.
    #[cfg(any(test, feature = "replay"))]
    pub fn rhythm_ms(&self, at_word_start: bool) -> f64 {
        self.rhythm.borrow().gap_ms(at_word_start)
    }

    fn accepted(&self) {
        self.ignores.borrow_mut().clear();
    }

    /// Hears that the user typed `text` at byte `offset` at `now`, which
    /// teaches the typist's rhythm, counts the words it ends and lets
    /// ignored suggestions stop lengthening the pause once their stretch of
    /// words or time is over.
    fn typed(&self, offset: usize, text: &str, now: Instant) {
        self.rhythm.borrow_mut().key(offset, text, now);
        let ended = text
            .chars()
            .filter(|character| character.is_whitespace())
            .count();
        let words = self.words.get() + ended as u64;
        self.words.set(words);
        self.ignores.borrow_mut().retain(|ignore| {
            words - ignore.words < IGNORED_FADE_WORDS
                && now.duration_since(ignore.at) < IGNORED_FADE
        });
    }

    fn ignored(&self, offer: &Offer, weight: u32, now: Instant) -> Ignored {
        #[cfg(any(test, feature = "replay"))]
        self.log.borrow_mut().push(Paced::Ignored {
            word: offer.word.clone(),
            lead: offer.lead.clone(),
            weight,
        });
        let id = self.next_ignore.get();
        self.next_ignore.set(id + 1);
        self.ignores.borrow_mut().push(Ignore {
            id,
            weight,
            at: now,
            words: self.words.get(),
        });
        let mut suppressed = self.suppressed.borrow_mut();
        suppressed.retain(|_, leads| {
            leads.retain(|(_, until)| *until > now);
            !leads.is_empty()
        });
        let ignored = Ignored {
            id,
            word: offer.word.to_lowercase(),
            lead: offer.lead.to_lowercase(),
            until: now + SUPPRESSION,
        };
        suppressed
            .entry(ignored.word.clone())
            .or_default()
            .push((ignored.lead.clone(), ignored.until));
        ignored
    }

    /// Undoes `ignored`, so it no longer lengthens the pause or keeps its word
    /// from being offered.
    fn restore(&self, ignored: &Ignored) {
        #[cfg(any(test, feature = "replay"))]
        self.log.borrow_mut().push(Paced::Restored {
            word: ignored.word.clone(),
        });
        self.ignores
            .borrow_mut()
            .retain(|ignore| ignore.id != ignored.id);
        let mut suppressed = self.suppressed.borrow_mut();
        if let Some(leads) = suppressed.get_mut(&ignored.word) {
            if let Some(index) = leads
                .iter()
                .position(|(lead, until)| *lead == ignored.lead && *until == ignored.until)
            {
                leads.remove(index);
            }
            if leads.is_empty() {
                suppressed.remove(&ignored.word);
            }
        }
    }

    /// Whether `word` was ignored after `lead`, or after the start of it,
    /// recently enough that it isn't offered.
    fn is_suppressed(&self, lead: &str, word: &str, now: Instant) -> bool {
        let lead = lead.to_lowercase();
        self.suppressed
            .borrow()
            .get(&word.to_lowercase())
            .is_some_and(|leads| {
                leads
                    .iter()
                    .any(|(ignored_after, until)| *until > now && lead.starts_with(ignored_after))
            })
    }
}

/// A suggestion on screen, or being typed along.
#[derive(Clone, Debug, PartialEq)]
struct Offer {
    /// What it continued when it first showed: the typed start of the word,
    /// or for a next-word prediction, the previous word and a space.
    lead: String,
    /// Its first word, whole.
    word: String,
    /// The caret: where typing continues it.
    caret: usize,
    /// What is left to type of `word`.
    rest: String,
    /// What is left of the whole suggestion, which Tab would insert.
    ghost: String,
}

impl Offer {
    /// The offer that `suggestion`, shown at `offset` in `text`, makes.
    fn new(text: &str, offset: usize, suggestion: &str) -> Option<Self> {
        let prefix = extract_prefix(text, offset);
        let word = format!("{prefix}{suggestion}");
        let rest = word[prefix.len()..word_end(&word, 0)].to_string();
        if rest.is_empty() {
            return None;
        }
        let lead = if prefix.is_empty() {
            format!("{} ", previous_two_words(text, offset).0?)
        } else {
            prefix.to_string()
        };
        Some(Self {
            lead,
            word: format!("{prefix}{rest}"),
            caret: offset,
            rest,
            ghost: suggestion.to_string(),
        })
    }

    /// Whether typing `text` at the caret continues the word without
    /// finishing it.
    fn is_continued_by(&self, text: &str) -> bool {
        !text.is_empty() && text.len() < self.rest.len() && self.rest.starts_with(text)
    }

    fn advance(&mut self, text: &str) {
        self.caret += text.len();
        self.rest.drain(..text.len());
        if self.ghost.starts_with(text) {
            self.ghost.drain(..text.len());
        } else {
            self.ghost.clear();
        }
    }
}

/// One textarea's view of its suggestions: what it offered, and what the
/// user did next.
///
/// The textarea reports typing through [`Self::typed`], then asks for
/// [`Self::debounce`] for the same keystroke. Every edit arrives later
/// through [`Self::changed`], Tab and Escape through [`Self::event`].
pub(super) struct Pacer {
    shared: Rc<SuggestionPacing>,
    offer: RefCell<Option<Offer>>,
    /// A suggestion held back by the typist's rhythm, and when it shows
    /// unless a key comes first.
    held: RefCell<Option<(Offer, Instant)>>,
    /// The suggestion turned down last, followed until its word is finished.
    rejected: RefCell<Option<Rejection>>,
    /// The keystroke just typed, until the change report for it arrives.
    typed: RefCell<Option<Keystroke>>,
    /// Whether the last keystroke typed along the pending suggestion.
    typing_along: Cell<bool>,
}

/// A suggestion turned down by a key that leaves its word, or by Escape,
/// with where its word starts in the text, the word, folded, and what
/// turning it down did.
struct Rejection {
    start: usize,
    word: String,
    ignored: Ignored,
}

/// Text the user typed, as the textarea reported it.
struct Keystroke {
    offset: usize,
    text: String,
    /// Whether Eager would ask for a suggestion after it.
    triggers: bool,
}

impl Pacer {
    pub fn new(shared: Rc<SuggestionPacing>) -> Self {
        Self {
            shared,
            offer: RefCell::new(None),
            held: RefCell::new(None),
            rejected: RefCell::new(None),
            typed: RefCell::new(None),
            typing_along: Cell::new(false),
        }
    }

    pub fn mode(&self) -> SuggestionMode {
        self.shared.mode()
    }

    /// Whether the last keystroke typed along the suggestion on screen.
    pub fn is_typing_along(&self) -> bool {
        self.typing_along.get()
    }

    /// Whether the mode is Quiet. In another mode it forgets what it was
    /// following, so that Quiet starts afresh when it comes back.
    fn is_quiet(&self) -> bool {
        if self.mode() == SuggestionMode::Quiet {
            return true;
        }
        self.offer.take();
        self.held.take();
        self.rejected.take();
        self.typed.take();
        self.typing_along.set(false);
        false
    }

    /// Settles the suggestion held back, at `now`: one due to show by then
    /// showed, and is pending from here on, and one still held is dropped.
    fn settle_held(&self, now: Instant) {
        if let Some((offer, shows_at)) = self.held.take()
            && shows_at <= now
        {
            *self.offer.borrow_mut() = Some(offer);
        }
    }

    /// The user typed `text` at byte `offset`. `triggers` when Eager would
    /// ask for a suggestion after it.
    pub fn typed(&self, offset: usize, text: &str, triggers: bool, now: Instant) {
        self.typing_along.set(false);
        if !self.is_quiet() {
            return;
        }
        self.settle_held(now);
        self.shared.typed(offset, text, now);
        *self.typed.borrow_mut() = Some(Keystroke {
            offset,
            text: text.to_string(),
            triggers,
        });
        let Some(mut offer) = self.offer.take() else {
            return;
        };
        if offset != offer.caret {
            // Left: the caret moved before this.
            return;
        }
        if offer.is_continued_by(text) {
            offer.advance(text);
            *self.offer.borrow_mut() = Some(offer);
            self.typing_along.set(true);
        } else {
            let ignored = self.shared.ignored(&offer, 1, now);
            // Typing the word's last letters finishes it rather than
            // leaving it.
            if !text.starts_with(offer.rest.as_str()) {
                self.reject(&offer, ignored);
            }
        }
    }

    /// How long typing must pause before suggestions are requested, for the
    /// keystroke [`Self::typed`] just reported. Timed by the typist's
    /// rhythm, the request comes at once, and [`Self::offer`] says how long
    /// its suggestion is held back.
    pub fn debounce(&self) -> Duration {
        if self.is_quiet()
            && !self.typing_along.get()
            && self.shared.timing() == QuietTiming::FixedPause
        {
            self.shared.pause()
        } else {
            Duration::ZERO
        }
    }

    /// Settles the pending suggestion by an edit that wasn't typing: Enter,
    /// Tab, deleting or undoing.
    ///
    /// In Quiet, returns where the caret went when `edits` hold a keystroke
    /// after which Eager would have asked for a suggestion, and so learned
    /// the sentence before the caret.
    pub fn changed(&self, edits: &[Edit], now: Instant) -> Option<usize> {
        let mut typed = self.typed.take();
        if !self.is_quiet() {
            return None;
        }
        self.settle_held(now);
        let mut learn_at = None;
        for edit in edits {
            let range = edit.range.clone();
            // Edits after the keystroke move the caret it left.
            if let Some(caret) = learn_at.as_mut()
                && range.end <= *caret
            {
                *caret = *caret - range.len() + edit.text.len();
            }
            if let Some(keystroke) = typed.take_if(|keystroke| {
                range.start == keystroke.offset && edit.text.starts_with(&keystroke.text)
            }) {
                if keystroke.triggers {
                    learn_at = Some(keystroke.offset + keystroke.text.len());
                }
                continue;
            }
            let Some(offer) = self.offer.take() else {
                continue;
            };
            let inserted_at_caret = range == (offer.caret..offer.caret) && !edit.text.is_empty();
            // Tab taking the suggestion inserts it, and its event follows.
            if inserted_at_caret && !offer.ghost.starts_with(edit.text) {
                let ignored = self.shared.ignored(&offer, 1, now);
                self.reject(&offer, ignored);
            }
        }
        learn_at
    }

    /// Settles the pending suggestion by Tab or Escape.
    pub fn event(&self, event: &SuggestionEvent, now: Instant) {
        match event {
            SuggestionEvent::Accepted { .. } => self.accepted(),
            SuggestionEvent::Dismissed { .. } => self.dismissed(now),
            _ => {
                self.is_quiet();
            }
        }
    }

    /// Settles the pending suggestion as taken with Tab, whole or a word.
    /// Only a suggestion on screen can be taken, so one held back showed.
    pub fn accepted(&self) {
        if !self.is_quiet() {
            return;
        }
        self.held.take();
        self.offer.take();
        self.shared.accepted();
    }

    /// Settles the pending suggestion as closed with Escape. Only a
    /// suggestion on screen can be closed, so one held back showed.
    pub fn dismissed(&self, now: Instant) {
        if !self.is_quiet() {
            return;
        }
        if let Some((offer, _)) = self.held.take() {
            *self.offer.borrow_mut() = Some(offer);
        }
        if let Some(offer) = self.offer.take() {
            let ignored = self.shared.ignored(&offer, ESCAPE_WEIGHT, now);
            self.reject(&offer, ignored);
        }
    }

    /// Follows `offer`, just turned down, until its word is finished.
    fn reject(&self, offer: &Offer, ignored: Ignored) {
        let start = offer.caret - (offer.word.len() - offer.rest.len());
        *self.rejected.borrow_mut() = Some(Rejection {
            start,
            word: fold(&offer.word).into_owned(),
            ignored,
        });
    }

    /// Follows the suggestion turned down last through `edits`, which made
    /// `text`. Once a word is finished at its place, turning it down counts
    /// for nothing if that word is the one it offered. An edit before the
    /// word that reaches into it ends the following.
    pub fn follow(&self, edits: &[Edit], text: &Rope) {
        if !self.is_quiet() {
            return;
        }
        let mut slot = self.rejected.borrow_mut();
        let Some(rejection) = slot.as_mut() else {
            return;
        };
        for edit in edits {
            let range = &edit.range;
            // Text typed at the word's start is the word's own first letters.
            let before = range.end < rejection.start
                || (range.end == rejection.start && range.start < rejection.start);
            if before {
                rejection.start = rejection.start - range.len() + edit.text.len();
            } else if range.start < rejection.start {
                *slot = None;
                return;
            }
        }
        if rejection.start > text.len() {
            *slot = None;
            return;
        }
        let mut written = String::new();
        let mut characters = text.chars_at(rejection.start).peekable();
        loop {
            let Some(character) = characters.next() else {
                // The word may still be being written.
                return;
            };
            let in_word = character.is_alphanumeric() || character == '_' || character == '-';
            let apostrophe = (character == '\'' || character == '\u{2019}')
                && written.chars().next_back().is_some_and(char::is_alphabetic);
            if in_word || (apostrophe && characters.peek().is_some_and(|next| next.is_alphabetic()))
            {
                written.push(character);
                if written.len() > rejection.word.len() + 64 {
                    break;
                }
            } else if apostrophe && characters.peek().is_none() {
                return;
            } else {
                break;
            }
        }
        let Some(rejection) = slot.take() else {
            return;
        };
        if fold(&written) == rejection.word {
            self.shared.restore(&rejection.ignored);
        }
    }

    /// What to offer of the model's `suggestion` for the caret at `offset`
    /// in `text`, which is at `caret` in the whole text, and how long after
    /// `now` it shows. Timed by the typist's rhythm, a suggestion is held
    /// back for that long, and shows unless a key comes first.
    pub fn offer(
        &self,
        text: &str,
        offset: usize,
        caret: usize,
        suggestion: Option<Suggested>,
        now: Instant,
    ) -> (Option<String>, Duration) {
        if !self.is_quiet() {
            return (suggestion.map(|suggested| suggested.ghost), Duration::ZERO);
        }
        // A request replaces whatever was held back for the key before.
        self.held.take();
        let typing_along = self.typing_along.replace(false);
        let mut pending = self.offer.borrow_mut();
        let Some(Suggested { ghost, chance }) = suggestion else {
            // While the user types along a suggestion it stays pending, even
            // when the model has nothing to add at this letter.
            if !typing_along {
                *pending = None;
            }
            return (None, Duration::ZERO);
        };
        let Some(mut offer) = Offer::new(text, offset, &ghost) else {
            *pending = None;
            return (Some(ghost), Duration::ZERO);
        };
        offer.caret = caret;
        if typing_along {
            // Keep showing the suggestion being typed along. Should the model
            // change its mind mid-word, the new word waits for a pause.
            let shown = match pending.as_mut() {
                Some(typed_along)
                    if typed_along.caret == caret && typed_along.word == offer.word =>
                {
                    typed_along.ghost = offer.ghost;
                    Some(ghost)
                }
                _ => None,
            };
            return (shown, Duration::ZERO);
        }
        if self.shared.is_suppressed(&offer.lead, &offer.word, now) {
            #[cfg(any(test, feature = "replay"))]
            self.shared.log.borrow_mut().push(Paced::Suppressed {
                word: offer.word.clone(),
                lead: offer.lead.clone(),
            });
            *pending = None;
            return (None, Duration::ZERO);
        }
        let hold = match self.shared.timing() {
            QuietTiming::FixedPause => Duration::ZERO,
            QuietTiming::Rhythm => {
                let at_word_start = offer.lead.ends_with(' ');
                let gain = ghost.chars().count().saturating_sub(1);
                self.shared.hold(at_word_start, chance, gain)
            }
        };
        if hold.is_zero() {
            *pending = Some(offer);
        } else {
            *pending = None;
            *self.held.borrow_mut() = Some((offer, now + hold));
        }
        (Some(ghost), hold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ignored_suggestion_lengthens_the_pause_up_to_the_cap() {
        let pacing = SuggestionPacing::new(SuggestionMode::Quiet);
        let offer = Offer::new("he", 2, "llo").unwrap();
        let now = Instant::now();
        let mut pauses = vec![pacing.pause()];
        for _ in 0..7 {
            pacing.ignored(&offer, 1, now);
            pauses.push(pacing.pause());
        }
        let ms = |ms| Duration::from_millis(ms);
        assert_eq!(
            pauses,
            [
                ms(300),
                ms(450),
                ms(600),
                ms(750),
                ms(900),
                ms(1000),
                ms(1000),
                ms(1000)
            ]
        );
        pacing.accepted();
        assert_eq!(pacing.pause(), QUIET_PAUSE);
        pacing.ignored(&offer, ESCAPE_WEIGHT, now);
        assert_eq!(pacing.pause(), ms(600));
    }

    /// Ignored suggestions lengthen the pause for the next twenty words or
    /// two minutes, whichever ends first.
    #[test]
    fn ignored_suggestions_lengthen_the_pause_for_a_stretch_of_words_or_time() {
        let shared = Rc::new(SuggestionPacing::new(SuggestionMode::Quiet));
        let pacer = Pacer::new(shared.clone());
        let offer = Offer::new("he", 2, "llo").unwrap();
        let ms = |ms| Duration::from_millis(ms);
        let now = Instant::now();
        shared.ignored(&offer, 1, now);
        pacer.typed(0, "a b ", true, now);
        shared.ignored(&offer, 1, now);
        shared.ignored(&offer, 1, now);
        assert_eq!(shared.pause(), ms(750));
        for _ in 0..IGNORED_FADE_WORDS - 3 {
            pacer.typed(0, " ", true, now);
        }
        assert_eq!(shared.pause(), ms(750), "nineteen words on from the first");
        pacer.typed(0, " ", true, now);
        assert_eq!(shared.pause(), ms(600), "twenty words on from the first");
        pacer.typed(0, "  ", true, now);
        assert_eq!(shared.pause(), QUIET_PAUSE);

        shared.ignored(&offer, ESCAPE_WEIGHT, now);
        pacer.typed(0, "a", true, now + IGNORED_FADE - Duration::from_secs(1));
        assert_eq!(shared.pause(), ms(600));
        pacer.typed(0, "a", true, now + IGNORED_FADE);
        assert_eq!(shared.pause(), QUIET_PAUSE);
    }

    /// The rhythm learns the gaps inside a word and before one apart, from
    /// keys typed one after another, and notices the typist slowing down.
    #[test]
    fn the_rhythm_learns_the_typists_gaps() {
        let mut rhythm = Rhythm::new();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut clock = 0;
        // "hello world", 100 ms between letters, 400 ms before "world".
        for (index, character) in "hello world".char_indices() {
            clock += if index == 6 { 400 } else { 100 };
            rhythm.key(index, &character.to_string(), at(clock));
        }
        // Ten gaps of 100 ms inside words, the one before the space
        // included, against a prior of 180 ms weighing one key.
        let in_word = rhythm.gap_ms(false);
        assert!((in_word - 105.5).abs() < 1.0, "{in_word}");
        // One gap of 400 ms before a word, against a prior of 300 ms.
        let word_start = rhythm.gap_ms(true);
        assert!((word_start - 346.4).abs() < 1.0, "{word_start}");
        assert!(rhythm.hesitation < 0.1, "{}", rhythm.hesitation);

        // Two keys at three times the rhythm: the typist is slowing down.
        clock += 300;
        rhythm.key(11, "s", at(clock));
        clock += 300;
        rhythm.key(12, "l", at(clock));
        assert!(rhythm.hesitation > 0.7, "{}", rhythm.hesitation);
        // A pause enters the median as a slow key and no more: the median
        // moves, but far less than a 30 s gap would move a plain mean.
        clock += 30_000;
        rhythm.key(13, "o", at(clock));
        assert!(rhythm.gap_ms(false) < 160.0, "{}", rhythm.gap_ms(false));

        // A key typed somewhere else, or after punctuation, teaches nothing.
        let before = rhythm.gap_ms(false);
        rhythm.key(40, "a", at(clock + 5000));
        rhythm.key(41, ".", at(clock + 5100));
        rhythm.key(42, "b", at(clock + 9000));
        assert_eq!(rhythm.gap_ms(false), before);
    }

    /// Cases worked by hand, with the arithmetic beside each, for a rhythm
    /// still at its priors of 180 ms inside a word and 300 ms before one.
    #[test]
    fn the_hold_weighs_the_suggestion_and_the_typist() {
        let mut rhythm = Rhythm::new();
        let hold = |rhythm: &Rhythm, at_word_start, chance, gain, ignored| {
            Hold::HAND_WRITTEN
                .wait(rhythm, at_word_start, chance, gain, ignored)
                .as_millis()
        };
        // Even chance, three keys: e^0.788 = 2.2 gaps, 396 ms.
        assert_eq!(hold(&rhythm, false, 0.5, 3, 0), 395);
        // Sure: e^(0.788 - 0.5) = 1.33 gaps, 240 ms. Twelve keys at an even
        // chance, two doublings: the same.
        assert_eq!(hold(&rhythm, false, 1.0, 3, 0), 240);
        assert_eq!(hold(&rhythm, false, 0.5, 12, 0), 240);
        // Two ignored: e^(0.788 + 0.6) = 4.0 gaps, 721 ms.
        assert_eq!(hold(&rhythm, false, 0.5, 3, 2), 721);
        // Before a word, 2.2 of 300 ms: 660 ms.
        assert_eq!(hold(&rhythm, true, 0.5, 3, 0), 659);
        // Six ignored would be 13 gaps: the cap.
        assert_eq!(hold(&rhythm, false, 0.5, 3, 6), 1500);
        // Slowing down to twice the rhythm, a log ratio of 0.69:
        // e^(0.788 - 0.277) = 1.67 gaps, 300 ms.
        rhythm.hesitation = 0.693;
        assert_eq!(hold(&rhythm, false, 0.5, 3, 0), 299);
    }

    /// Timed by the rhythm, a suggestion is held back after the key. A key
    /// before it was due to show drops it, and it counts for nothing. A key
    /// after that time finds it shown, and types along it or past it as
    /// with any shown suggestion.
    #[test]
    fn a_held_suggestion_shows_by_time_or_is_dropped_by_the_next_key() {
        let shared = Rc::new(SuggestionPacing::new(SuggestionMode::Quiet));
        shared.set_timing(QuietTiming::Rhythm);
        let pacer = Pacer::new(shared.clone());
        let suggested = |ghost: &str| {
            Some(Suggested {
                ghost: ghost.to_string(),
                chance: 0.5,
            })
        };
        let start = Instant::now();
        pacer.typed(1, "e", true, start);
        assert_eq!(pacer.debounce(), Duration::ZERO);
        let (shown, hold) = pacer.offer("he", 2, 2, suggested("llo world"), start);
        assert_eq!(shown.as_deref(), Some("llo world"));
        assert_eq!(hold, shared.hold(false, 0.5, 8));
        assert!(hold > Duration::ZERO);

        // Typed past before it showed: nothing was ignored.
        let now = start + hold / 2;
        pacer.typed(2, "x", true, now);
        assert_eq!(shared.take_log(), vec![]);
        assert_eq!(shared.pause(), QUIET_PAUSE);

        // Shown, then typed along: it stays, without a hold.
        let (_, hold) = pacer.offer("hex", 3, 3, suggested("agon"), now);
        let now = now + hold;
        pacer.typed(3, "a", true, now);
        assert!(pacer.is_typing_along());
        assert_eq!(pacer.debounce(), Duration::ZERO);
        assert_eq!(
            pacer.offer("hexa", 4, 4, suggested("gon"), now),
            (Some("gon".to_string()), Duration::ZERO)
        );

        // Shown, then typed past: ignored, as with the fixed pause.
        pacer.typed(4, "x", true, now);
        assert_eq!(
            shared.take_log(),
            vec![Paced::Ignored {
                word: "hexagon".to_string(),
                lead: "hex".to_string(),
                weight: 1,
            }]
        );
        let (_, hold) = pacer.offer("hexax", 5, 5, suggested("yz"), now);
        assert!(
            hold > shared.hold(false, 0.5, 2),
            "ignoring lengthens the hold"
        );
        let now = now + hold + Duration::from_millis(1);
        pacer.typed(5, "q", true, now);
        assert_eq!(shared.take_log().len(), 1);

        // With the fixed pause, nothing is held back.
        shared.set_timing(QuietTiming::FixedPause);
        pacer.typed(6, "r", true, now);
        assert_eq!(pacer.debounce(), shared.pause());
        let (shown, hold) = pacer.offer("hexaxqr", 7, 7, suggested("st"), now);
        assert_eq!((shown.as_deref(), hold), (Some("st"), Duration::ZERO));
    }

    #[test]
    fn a_word_is_suppressed_after_its_prefix_and_longer_ones_until_it_expires() {
        let pacing = SuggestionPacing::new(SuggestionMode::Quiet);
        let now = Instant::now();
        let offer = Offer::new("say hel", 7, "lo world").unwrap();
        assert_eq!(offer.lead, "hel");
        assert_eq!(offer.word, "hello");
        pacing.ignored(&offer, 1, now);

        assert!(pacing.is_suppressed("hel", "hello", now));
        assert!(pacing.is_suppressed("hell", "hello", now));
        assert!(pacing.is_suppressed("Hel", "Hello", now), "in any case");
        assert!(
            !pacing.is_suppressed("he", "hello", now),
            "a shorter prefix"
        );
        assert!(!pacing.is_suppressed("hel", "help", now), "another word");
        let later = now + SUPPRESSION;
        assert!(pacing.is_suppressed("hel", "hello", later - Duration::from_secs(1)));
        assert!(!pacing.is_suppressed("hel", "hello", later));
    }

    #[test]
    fn a_prediction_is_suppressed_after_the_same_previous_word() {
        let pacing = SuggestionPacing::new(SuggestionMode::Quiet);
        let now = Instant::now();
        let offer = Offer::new("Say hello ", 10, "world again").unwrap();
        assert_eq!(offer.lead, "hello ");
        assert_eq!(offer.word, "world");
        assert_eq!(offer.rest, "world");
        assert_eq!(offer.ghost, "world again");
        pacing.ignored(&offer, 1, now);

        assert!(pacing.is_suppressed("hello ", "world", now));
        assert!(!pacing.is_suppressed("goodbye ", "world", now));
        assert!(!pacing.is_suppressed("w", "world", now), "completing it");
    }

    #[test]
    fn typing_continues_an_offer_until_the_word_is_finished() {
        let mut offer = Offer::new("hel", 3, "lo world").unwrap();
        assert!(offer.is_continued_by("l"));
        assert!(!offer.is_continued_by("lo"), "that finishes the word");
        assert!(!offer.is_continued_by("x"));
        assert!(!offer.is_continued_by(" "));
        assert!(!offer.is_continued_by(""));
        offer.advance("l");
        assert_eq!((offer.caret, offer.rest.as_str()), (4, "o"));
        assert_eq!(offer.ghost, "o world");
        assert!(!offer.is_continued_by("o"));
    }

    #[test]
    fn an_offer_continues_through_a_contraction() {
        let mut offer = Offer::new("I do", 4, "n't know").unwrap();
        assert_eq!((offer.word.as_str(), offer.rest.as_str()), ("don't", "n't"));
        assert!(offer.is_continued_by("n"));
        offer.advance("n");
        assert!(offer.is_continued_by("'"));
        assert!(!offer.is_continued_by("'t"), "that finishes the word");
    }

    #[test]
    fn modes_load_from_names_and_from_the_old_switch() {
        fn load(json: &str) -> SuggestionMode {
            serde_json::from_str(json).unwrap()
        }
        assert_eq!(load("true"), SuggestionMode::Quiet);
        assert_eq!(load("false"), SuggestionMode::Off);
        assert_eq!(load(r#""off""#), SuggestionMode::Off);
        assert_eq!(load(r#""quiet""#), SuggestionMode::Quiet);
        assert_eq!(load(r#""eager""#), SuggestionMode::Eager);
        assert_eq!(load(r#""someday""#), SuggestionMode::Quiet);
        for mode in [
            SuggestionMode::Off,
            SuggestionMode::Quiet,
            SuggestionMode::Eager,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(load(&json), mode, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&SuggestionMode::Quiet).unwrap(),
            r#""quiet""#
        );
    }
}
