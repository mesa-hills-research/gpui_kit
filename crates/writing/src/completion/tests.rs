//! Suggestions in a textarea, typed a key at a time on GPUI's fake clock.

use super::pacing::{
    ESCAPE_WEIGHT, IGNORED_EXTRA_PAUSE, MAX_QUIET_PAUSE, QUIET_PAUSE, SUPPRESSION,
};
use super::vocabulary::Vocabulary;
use super::{
    LOCAL_COUNT_ON_UI_BYTES, QuietTiming, SuggestionMode, WordSuggestions, accept_word_bindings,
};
use crate::Writing;
use crate::spelling::Dictionary;
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, TestAppContext, VisualTestContext, Window, div, px, size,
};
use gpui_base::input::{Keymap, Rope, SuggestionOptions, TextareaState};
use std::rc::Rc;
use std::time::Duration;

/// A vocabulary that knows `hello` well, `help` and `helium` less, and that
/// `world` follows `hello`.
fn vocabulary() -> Vocabulary {
    let mut vocabulary = words(&[
        ("hello", 40),
        ("help", 3),
        ("helium", 1),
        ("world", 10),
        ("wonder", 3),
    ]);
    for _ in 0..8 {
        vocabulary.learn_bigram("hello", "world");
    }
    vocabulary
}

fn words(words: &[(&str, u32)]) -> Vocabulary {
    let mut vocabulary = Vocabulary::new();
    for &(word, count) in words {
        for _ in 0..count {
            vocabulary.learn_word(word);
        }
    }
    vocabulary
}

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Writing aids on `vocabulary`, without a dictionary.
fn writing(vocabulary: Vocabulary) -> Writing {
    Writing::from_parts(Rc::new(Dictionary::empty()), vocabulary)
}

/// Keys that differ on macOS, where Home and End scroll.
fn line_end() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-right"
    } else {
        "end"
    }
}

fn text_start() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-up"
    } else {
        "ctrl-home"
    }
}

fn text_end() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-down"
    } else {
        "ctrl-end"
    }
}

struct Root {
    textarea: Entity<TextareaState>,
    suggestions: Rc<WordSuggestions>,
    _pacing: Subscription,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.textarea.clone())
    }
}

/// A text editor in a window, with word suggestions shown as ghost text and
/// the focus in it.
struct Typist {
    cx: VisualTestContext,
    textarea: Entity<TextareaState>,
    suggestions: Rc<WordSuggestions>,
    writing: Writing,
}

impl Typist {
    fn new(cx: &mut TestAppContext, mode: SuggestionMode) -> Self {
        Self::with_vocabulary(cx, mode, vocabulary())
    }

    fn with_vocabulary(
        cx: &mut TestAppContext,
        mode: SuggestionMode,
        vocabulary: Vocabulary,
    ) -> Self {
        Self::open(cx, mode, writing(vocabulary), None)
    }

    /// A textarea opened with `content`, or empty, learning into `writing`.
    fn open(
        cx: &mut TestAppContext,
        mode: SuggestionMode,
        writing: Writing,
        content: Option<&str>,
    ) -> Self {
        Self::open_with_keymap(cx, mode, Keymap::Cua, writing, content)
    }

    fn open_with_keymap(
        cx: &mut TestAppContext,
        mode: SuggestionMode,
        keymap: Keymap,
        writing: Writing,
        content: Option<&str>,
    ) -> Self {
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.bind_keys(accept_word_bindings());
        });
        writing.set_suggestion_mode(mode);
        let content = content.map(str::to_string);
        let window = cx.open_window(size(px(600.), px(300.)), {
            let writing = writing.clone();
            move |window, cx| {
                let suggestions = writing.word_suggestions();
                if let Some(content) = &content {
                    suggestions.opened_with(content);
                }
                let textarea = cx.new(|cx| {
                    let state = TextareaState::new(window, cx)
                        .text_editor()
                        .soft_wrap(false)
                        .line_number(false)
                        .keymap(keymap)
                        .suggestion_provider(suggestions.clone())
                        .suggestion_options(SuggestionOptions::default().menu(false).inline(true));
                    match content {
                        Some(content) => state.default_value(content),
                        None => state,
                    }
                });
                let pacing = suggestions.follow(&textarea, cx);
                Root {
                    textarea,
                    suggestions,
                    _pacing: pacing,
                }
            }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let (textarea, suggestions) = window
            .read_with(&cx, |root, _| {
                (root.textarea.clone(), root.suggestions.clone())
            })
            .unwrap();
        cx.update(|window, cx| {
            textarea.update(cx, |state, cx| state.focus(window, cx));
            window.draw(cx).clear(cx);
        });
        Self {
            cx,
            textarea,
            suggestions,
            writing,
        }
    }

    /// The ghost text after the caret.
    fn ghost(&self) -> Option<String> {
        self.textarea.read_with(&self.cx, |state, _| {
            state
                .suggestions()
                .first()
                .map(|suggestion| suggestion.text().to_string())
        })
    }

    fn value(&self) -> String {
        self.textarea
            .read_with(&self.cx, |state, _| state.value().to_string())
    }

    /// Types `text` a key at a time, with no time between the keys.
    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
    }

    /// Types `text` a key at a time, and returns the ghost text after each.
    fn ghosts_while_typing(&mut self, text: &str) -> Vec<Option<String>> {
        text.chars()
            .map(|key| {
                self.cx.simulate_input(&key.to_string());
                self.ghost()
            })
            .collect()
    }

    fn wait(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
        self.cx.run_until_parked();
    }

    /// Waits until a suggestion shows, and returns how long that took, or
    /// `None` when none does within twice the longest pause.
    fn pause_before_a_suggestion(&mut self) -> Option<Duration> {
        let step = ms(50);
        let mut waited = Duration::ZERO;
        while self.ghost().is_none() {
            if waited > 2 * MAX_QUIET_PAUSE {
                return None;
            }
            self.wait(step);
            waited += step;
        }
        Some(waited)
    }

    /// Types `text`, then waits for a suggestion as long as Quiet ever waits.
    fn type_and_wait(&mut self, text: &str) -> Option<String> {
        self.type_text(text);
        self.wait(MAX_QUIET_PAUSE);
        self.ghost()
    }
}

fn ghosts(expected: &[Option<&str>]) -> Vec<Option<String>> {
    expected
        .iter()
        .map(|ghost| ghost.map(String::from))
        .collect()
}

/// In Eager, every keystroke brings the model's suggestion up at once, again
/// after Escape and after typing past it. A suggestion of one letter, which
/// Tab would save nothing on, doesn't show.
#[gpui::test]
fn eager_suggests_after_every_keystroke(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Eager);

    assert_eq!(
        typist.ghosts_while_typing("hello world he"),
        ghosts(&[
            Some("ello world"),
            Some("llo world"),
            Some("lo world"),
            Some("o world"),
            None,
            Some("world"),
            Some("orld"),
            Some("rld"),
            Some("ld"),
            None,
            None,
            None,
            Some("ello world"),
            Some("llo world"),
        ])
    );
    typist.cx.simulate_keystrokes("escape");
    assert_eq!(typist.ghost(), None);
    assert_eq!(
        typist.ghosts_while_typing("lp he"),
        ghosts(&[
            Some("lo world"),
            None,
            None,
            Some("ello world"),
            Some("llo world")
        ])
    );
    typist.cx.simulate_keystrokes("tab");
    assert_eq!(typist.value(), "hello world help hello world");
    assert_eq!(typist.ghost(), None);
}

/// Off offers nothing, however long typing pauses.
#[gpui::test]
fn off_offers_nothing(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Off);
    assert!(
        typist
            .ghosts_while_typing("hello world he")
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(typist.pause_before_a_suggestion(), None);
}

/// What is typed while suggestions are off is never learned, not even once
/// they are on again and the next key would learn the sentence before it.
#[gpui::test]
fn what_is_typed_while_off_is_never_learned(cx: &mut TestAppContext) {
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Off, Vocabulary::new());
    typist.type_text("Mosses grow fast.");
    typist.wait(MAX_QUIET_PAUSE);
    typist.writing.set_suggestion_mode(SuggestionMode::Quiet);
    typist.type_text(" T");
    typist.wait(MAX_QUIET_PAUSE);
    assert_eq!(learned(&typist), []);

    // What is typed once they are on is.
    typist.type_text("hen it rained. Yes");
    typist.wait(MAX_QUIET_PAUSE);
    assert_eq!(
        learned(&typist),
        counts(&[("Then", 1), ("it", 1), ("rained", 1)])
    );
}

/// Quiet shows nothing while keys come 60 ms apart, and the suggestion once
/// typing has paused for the pause.
#[gpui::test]
fn quiet_waits_for_typing_to_pause(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    for key in "hello wor".chars() {
        typist.type_text(&key.to_string());
        assert_eq!(typist.ghost(), None);
        typist.wait(ms(60));
        assert_eq!(typist.ghost(), None);
    }
    typist.wait(QUIET_PAUSE - ms(61));
    assert_eq!(typist.ghost(), None);
    typist.wait(ms(1));
    assert_eq!(typist.ghost().as_deref(), Some("ld"));
}

/// Timed by the typist's rhythm, Quiet asks at every key and holds the
/// suggestion back for a few of the typist's gaps between keys. Keys at a
/// steady 100 ms show nothing, the ghost shows once its hold is up, and a
/// key typed before then drops the suggestion held.
#[gpui::test]
fn quiet_timed_by_rhythm_holds_a_suggestion_back(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    typist.writing.set_quiet_timing(QuietTiming::Rhythm);
    // The same keys at the same times in a replay say how long each
    // suggestion is held.
    let replayed = writing(vocabulary());
    replayed.set_suggestion_mode(SuggestionMode::Quiet);
    let writer = crate::replay::Writer::with_writing(replayed);
    writer.set_quiet_timing(QuietTiming::Rhythm);
    let mut field = writer.field();
    let gap = ms(100);
    // Types `text` a key every `gap`, and returns how long the last key's
    // suggestion is held.
    let type_steadily = |typist: &mut Typist, field: &mut crate::replay::Field, text: &str| {
        let mut hold = Duration::ZERO;
        for key in text.chars() {
            typist.wait(gap);
            field.wait(gap);
            assert_eq!(typist.ghost(), None, "held before {key:?}");
            typist.type_text(&key.to_string());
            field.type_key(&key.to_string());
            assert_eq!(typist.ghost(), None, "held after {key:?}");
            hold = field
                .take_answers()
                .pop()
                .filter(|asked| asked.shown.is_some())
                .map_or(Duration::ZERO, |asked| asked.hold);
            assert!(hold.is_zero() || hold > gap, "{key:?} held for {hold:?}");
        }
        hold
    };

    let hold = type_steadily(&mut typist, &mut field, "hello wor");
    typist.wait(hold - ms(1));
    assert_eq!(typist.ghost(), None);
    typist.wait(ms(1));
    assert_eq!(typist.ghost().as_deref(), Some("ld"));
    typist.cx.simulate_keystrokes("tab");
    field.wait(hold);
    assert_eq!(field.tab().as_deref(), Some("ld"));

    let hold = type_steadily(&mut typist, &mut field, " hel");
    assert!(!hold.is_zero());
    typist.wait(hold - ms(1));
    assert_eq!(typist.ghost(), None);
    typist.type_text("x");
    assert_eq!(typist.value(), "hello world helx");
    typist.wait(2 * MAX_QUIET_PAUSE);
    assert_eq!(typist.ghost(), None, "dropped by the key");
}

/// A suggestion on screen stays there, without a pause, while the user types
/// the letters it has next.
#[gpui::test]
fn quiet_keeps_a_suggestion_typed_along(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.type_and_wait("hel").as_deref(), Some("lo world"));
    assert_eq!(typist.ghosts_while_typing("l"), ghosts(&[Some("o world")]));
    typist.cx.simulate_keystrokes("tab");
    assert_eq!(typist.value(), "hello world");
}

/// A word typed past is not offered again after the same prefix, or a longer
/// one, until the suppression time is over. A shorter prefix still offers it.
#[gpui::test]
fn a_word_typed_past_comes_back_after_the_suppression_time(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.type_and_wait("hel").as_deref(), Some("lo world"));
    typist.type_text("p");

    assert_eq!(typist.type_and_wait(" hel"), None);
    assert_eq!(typist.type_and_wait("l"), None);
    assert_eq!(typist.type_and_wait(" he").as_deref(), Some("llo world"));
    typist.cx.simulate_keystrokes("tab");

    typist.wait(SUPPRESSION / 2);
    assert_eq!(typist.type_and_wait(" hel"), None);
    typist.wait(SUPPRESSION / 2);
    assert_eq!(typist.type_and_wait(" hel").as_deref(), Some("lo world"));
}

/// Typing the rest of the word, Enter and Escape all leave the suggestion
/// behind: each suppresses it and lengthens the next pause, Escape by more.
#[gpui::test]
fn finishing_the_word_enter_and_escape_ignore_the_suggestion(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.type_and_wait("hel").as_deref(), Some("lo world"));
    assert_eq!(
        typist.ghosts_while_typing("lo"),
        ghosts(&[Some("o world"), None])
    );
    assert_eq!(typist.type_and_wait(" hel"), None, "finished without Tab");

    typist.type_text(" wo");
    assert_eq!(
        typist.pause_before_a_suggestion(),
        Some(QUIET_PAUSE + IGNORED_EXTRA_PAUSE)
    );
    typist.cx.simulate_keystrokes("enter");
    assert_eq!(typist.type_and_wait("wo"), None, "left with Enter");

    typist.type_text(" wond");
    assert_eq!(
        typist.pause_before_a_suggestion(),
        Some(QUIET_PAUSE + IGNORED_EXTRA_PAUSE * 2)
    );
    assert_eq!(typist.ghost().as_deref(), Some("er"));
    typist.cx.simulate_keystrokes("escape");
    assert_eq!(typist.type_and_wait(" wond"), None, "refused with Escape");

    typist.type_text(" he");
    assert_eq!(
        typist.pause_before_a_suggestion(),
        Some(QUIET_PAUSE + IGNORED_EXTRA_PAUSE * (2 + ESCAPE_WEIGHT))
    );
}

/// Deleting, or moving the caret and typing elsewhere, leaves a suggestion
/// without counting it as ignored.
#[gpui::test]
fn deleting_or_typing_elsewhere_ignores_nothing(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.type_and_wait("hel").as_deref(), Some("lo world"));
    typist.cx.simulate_keystrokes("backspace");
    typist.type_text("l");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
    assert_eq!(typist.ghost().as_deref(), Some("lo world"));

    typist.cx.simulate_keystrokes("left");
    typist.type_text("x");
    typist.cx.simulate_keystrokes(line_end());
    assert_eq!(typist.value(), "hexl");
    typist.type_text(" hel");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
    assert_eq!(typist.ghost().as_deref(), Some("lo world"));
}

/// Each suggestion ignored in a row lengthens the pause, up to the cap, and
/// accepting one brings the plain pause back.
#[gpui::test]
fn ignored_suggestions_lengthen_the_pause_until_one_is_accepted(cx: &mut TestAppContext) {
    let names = [
        "alpha", "bravo", "charlie", "delta", "echoes", "foxtrot", "golfer", "hotel", "india",
    ];
    let vocabulary = words(&names.map(|name| (name, 10)));
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary);

    let mut pauses = Vec::new();
    for name in &names[..7] {
        typist.type_text(&name[..2]);
        pauses.push(typist.pause_before_a_suggestion().unwrap());
        typist.type_text(" ");
    }
    assert_eq!(
        pauses,
        [
            ms(300),
            ms(450),
            ms(600),
            ms(750),
            ms(900),
            ms(1000),
            ms(1000)
        ]
    );
    assert_eq!(MAX_QUIET_PAUSE, ms(1000));

    typist.type_text("ho");
    assert_eq!(typist.pause_before_a_suggestion(), Some(MAX_QUIET_PAUSE));
    typist.cx.simulate_keystrokes("tab");
    assert!(typist.value().ends_with(" hotel"), "{}", typist.value());
    typist.type_text(" in");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
}

/// Suggestions typed past lengthen the pause only for a stretch of writing,
/// so someone whose pauses are shorter than the longest pause still finds
/// suggestions once they have typed on for a while.
#[gpui::test]
fn the_pause_shortens_again_once_the_user_types_on(cx: &mut TestAppContext) {
    let names = ["alpha", "bravo", "charlie", "delta", "echoes", "foxtrot"];
    let vocabulary = words(&names.map(|name| (name, 10)));
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary);
    for name in &names[..5] {
        typist.type_text(&name[..2]);
        assert!(typist.pause_before_a_suggestion().is_some());
        typist.type_text(" ");
    }
    // After five ignored in a row, a pause of 700 ms no longer brings one up.
    typist.type_text("fo");
    typist.wait(ms(700));
    assert_eq!(typist.ghost(), None);
    typist.cx.simulate_keystrokes("backspace backspace");
    for _ in 0..super::pacing::IGNORED_FADE_WORDS {
        typist.type_text("zz ");
    }
    typist.type_text("fo");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
}

/// A suggestion turned down by Escape, or by a typo fixed at once, whose
/// word the user then writes anyway was wanted after all. Turning it down
/// counts for nothing, so the pause stays plain and the word is offered
/// again. A different word written in its place keeps it turned down.
#[gpui::test]
fn writing_a_turned_down_word_anyway_undoes_turning_it_down(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.type_and_wait("hel").as_deref(), Some("lo world"));
    typist.cx.simulate_keystrokes("escape");
    typist.type_text("lo ");
    typist.type_text("hel");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
    assert_eq!(typist.ghost().as_deref(), Some("lo world"));

    typist.type_text("x");
    typist.cx.simulate_keystrokes("backspace");
    typist.type_text("lo ");
    typist.type_text("hel");
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));

    typist.type_text("p ");
    assert_eq!(typist.type_and_wait("hel"), None, "help written instead");

    // A next word turned down and then written.
    typist.type_text("p hello ");
    let pause = QUIET_PAUSE + IGNORED_EXTRA_PAUSE;
    assert_eq!(typist.pause_before_a_suggestion(), Some(pause));
    assert_eq!(typist.ghost().as_deref(), Some("world"));
    typist.cx.simulate_keystrokes("escape");
    typist.type_text("world hello ");
    assert_eq!(typist.pause_before_a_suggestion(), Some(pause));
    assert_eq!(typist.ghost().as_deref(), Some("world"));
}

/// Quiet holds back a completion a young vocabulary has seen once, which is
/// as likely a word it hasn't seen, until the word has been written a few
/// times. Eager offers it at once, and so does Quiet after the words it
/// followed before.
#[gpui::test]
fn quiet_waits_for_a_young_vocabulary_to_know_a_word(cx: &mut TestAppContext) {
    let once = || {
        let mut vocabulary = Vocabulary::new();
        vocabulary.learn_sentence("We went to the bank");
        vocabulary
    };
    let mut eager = Typist::with_vocabulary(cx, SuggestionMode::Eager, once());
    assert_eq!(
        eager.type_and_wait("We went to a ba").as_deref(),
        Some("nk")
    );

    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, once());
    assert_eq!(quiet.type_and_wait("We went to a ba"), None);
    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, once());
    assert_eq!(
        quiet.type_and_wait("We went to the ba").as_deref(),
        Some("nk")
    );

    let mut often = Vocabulary::new();
    for _ in 0..4 {
        often.learn_sentence("We went to the bank");
    }
    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, often);
    assert_eq!(
        quiet.type_and_wait("We went to a ba").as_deref(),
        Some("nk")
    );
}

/// Quiet holds back a long completion of a word the document doesn't hold
/// until more of it is typed, since the more is left to type, the likelier
/// the user means another word. Eager offers it at once, and so does Quiet
/// once the document holds the word.
#[gpui::test]
fn quiet_waits_for_more_of_a_long_word_the_document_lacks(cx: &mut TestAppContext) {
    let vocabulary = || words(&[("photosynthesis", 5)]);
    let mut eager = Typist::with_vocabulary(cx, SuggestionMode::Eager, vocabulary());
    assert_eq!(
        eager.type_and_wait("Tomorrow ph").as_deref(),
        Some("otosynthesis")
    );

    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary());
    assert_eq!(quiet.type_and_wait("Tomorrow ph"), None);
    assert_eq!(quiet.type_and_wait("otosynth").as_deref(), Some("esis"));

    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary());
    assert_eq!(
        quiet
            .type_and_wait("Photosynthesis needs light. Tomorrow ph")
            .as_deref(),
        Some("otosynthesis")
    );
}

/// Quiet holds back a next word the document doesn't have unless the model
/// is surer of it, since it may come from writing on another subject. Eager
/// offers it, and so does Quiet once the document has the word.
#[gpui::test]
fn quiet_holds_back_a_next_word_the_document_lacks(cx: &mut TestAppContext) {
    let vocabulary = || {
        let mut vocabulary = Vocabulary::new();
        for _ in 0..4 {
            vocabulary.learn_sentence("We went to the bank");
        }
        for _ in 0..2 {
            vocabulary.learn_sentence("We went to the park");
        }
        vocabulary
    };
    let mut eager = Typist::with_vocabulary(cx, SuggestionMode::Eager, vocabulary());
    assert_eq!(
        eager.type_and_wait("We went to the ").as_deref(),
        Some("bank")
    );

    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary());
    assert_eq!(quiet.type_and_wait("We went to the "), None);
    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary());
    assert_eq!(
        quiet
            .type_and_wait("Our bank was shut. We went to the ")
            .as_deref(),
        Some("bank")
    );
}

/// A completion Quiet showed stays while the user types along it, though
/// Quiet would hold back the word after the narrower prefix.
#[gpui::test]
fn quiet_keeps_a_completion_typed_along_that_it_would_hold_back(cx: &mut TestAppContext) {
    let mut vocabulary = Vocabulary::new();
    for _ in 0..20 {
        vocabulary.learn_sentence("but be");
    }
    for n in 0..40u8 {
        let word = format!("zz{}{}", (b'a' + n / 26) as char, (b'a' + n % 26) as char);
        vocabulary.learn_sentence(&format!("{word} {word}x {word}y"));
    }
    vocabulary.learn_sentence("We went to the bank");
    let mut quiet = Typist::with_vocabulary(cx, SuggestionMode::Quiet, vocabulary);
    assert_eq!(
        quiet.type_and_wait("We went to the b").as_deref(),
        Some("ank")
    );
    assert_eq!(quiet.ghosts_while_typing("a"), ghosts(&[Some("nk")]));
}

/// A new mode applies from the next keystroke.
#[gpui::test]
fn a_new_mode_applies_at_once(cx: &mut TestAppContext) {
    let mut typist = Typist::new(cx, SuggestionMode::Quiet);
    assert_eq!(typist.ghosts_while_typing("he"), ghosts(&[None, None]));

    typist.writing.set_suggestion_mode(SuggestionMode::Eager);
    assert_eq!(typist.ghosts_while_typing("l"), ghosts(&[Some("lo world")]));

    typist.writing.set_suggestion_mode(SuggestionMode::Off);
    assert_eq!(typist.type_and_wait(" he"), None);

    typist.writing.set_suggestion_mode(SuggestionMode::Quiet);
    assert_eq!(
        typist.ghosts_while_typing(" he"),
        ghosts(&[None, None, None])
    );
    assert_eq!(typist.pause_before_a_suggestion(), Some(QUIET_PAUSE));
}

/// The words the typist's vocabulary has learned, with their counts.
fn learned(typist: &Typist) -> Vec<(String, u32)> {
    typist.writing.vocabulary().borrow().word_counts()
}

fn counts(words: &[(&str, u32)]) -> Vec<(String, u32)> {
    let mut words: Vec<(String, u32)> = words
        .iter()
        .map(|&(word, count)| (word.to_string(), count))
        .collect();
    words.sort();
    words
}

/// Runs `script` on an empty vocabulary in Eager and then in Quiet, checks
/// that both learned the same words, and returns them.
fn learned_in_both_modes(
    cx: &mut TestAppContext,
    script: impl Fn(&mut Typist),
) -> Vec<(String, u32)> {
    let mut results = Vec::new();
    for mode in [SuggestionMode::Eager, SuggestionMode::Quiet] {
        let mut typist = Typist::with_vocabulary(cx, mode, Vocabulary::new());
        script(&mut typist);
        results.push(learned(&typist));
    }
    assert_eq!(results[1], results[0], "Quiet learns what Eager learns");
    results.remove(0)
}

fn paste(typist: &mut Typist, text: &str) {
    let item = ClipboardItem::new_string(text.to_string());
    typist.cx.update(|_, cx| cx.write_to_clipboard(item));
    typist.cx.simulate_keystrokes("secondary-v");
}

/// Each sentence is learned when its terminator is typed, though Quiet asks
/// for suggestions only once typing pauses.
#[gpui::test]
fn typing_sentences_learns_them(cx: &mut TestAppContext) {
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Ferns grow slowly. Mosses grow faster. ");
    });
    assert_eq!(
        learned,
        counts(&[
            ("Ferns", 1),
            ("Mosses", 1),
            ("faster", 1),
            ("grow", 2),
            ("slowly", 1)
        ])
    );
}

/// A paste teaches nothing, neither when the user types after it nor when
/// they finish a pasted sentence. What they type below it is learned.
#[gpui::test]
fn a_paste_learns_nothing(cx: &mut TestAppContext) {
    let learned = learned_in_both_modes(cx, |typist| {
        paste(
            typist,
            "Ferns grow slowly.\nMosses grow faster. Lichens wait",
        );
        assert_eq!(
            typist.value(),
            "Ferns grow slowly.\nMosses grow faster. Lichens wait"
        );
        assert_eq!(learned(typist), []);
        typist.type_text(" patiently. Then");
    });
    assert_eq!(learned, []);

    let learned = learned_in_both_modes(cx, |typist| {
        paste(typist, "Lichens wait patiently");
        typist.type_text(".");
        paste(typist, " Ferns grow slowly.\n");
        typist.type_text("Mosses grow faster. Then");
    });
    assert_eq!(
        learned,
        counts(&[("Mosses", 1), ("faster", 1), ("grow", 1)])
    );

    // Typing inside pasted text doesn't make it the user's.
    let learned = learned_in_both_modes(cx, |typist| {
        paste(typist, "Alpha beta gamma. Delta epsilon zeta.");
        typist.cx.simulate_keystrokes("left");
        typist.type_text(" eta");
        typist.cx.simulate_keystrokes(line_end());
        typist.type_text(" Theta");
    });
    assert_eq!(learned, []);
}

/// A message sent without a terminator still teaches its last sentence when
/// the composer learns the rest of it as it sends, once, and under the same
/// rules: pasted text teaches nothing.
#[gpui::test]
fn the_rest_of_a_sent_message_is_learned(cx: &mut TestAppContext) {
    let send = |typist: &mut Typist| {
        let text = typist
            .textarea
            .read_with(&typist.cx, |state, _| state.text().clone());
        typist.suggestions.learn_rest(&text);
        typist.suggestions.learn_rest(&text);
        typist.cx.update(|window, cx| {
            typist
                .textarea
                .update(cx, |state, cx| state.set_value("", window, cx))
        });
    };
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Thanks. Please fix the bug");
        send(typist);
    });
    assert_eq!(
        learned,
        counts(&[
            ("Please", 1),
            ("Thanks", 1),
            ("bug", 1),
            ("fix", 1),
            ("the", 1)
        ])
    );

    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("It works now.");
        send(typist);
    });
    assert_eq!(learned, counts(&[("It", 1), ("now", 1), ("works", 1)]));

    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("See ");
        paste(typist, "the attached log");
        send(typist);
    });
    assert_eq!(learned, []);
}

/// A long message without a terminator teaches what the same message ended
/// with a period would: only its last kilobyte or so, as a sentence is
/// never read further back than that.
#[gpui::test]
fn a_long_unfinished_message_learns_what_a_finished_one_would(cx: &mut TestAppContext) {
    let message = format!("Alpha {}omega", "lorem ipsum dolor ".repeat(70));
    let finished = learned_in_both_modes(cx, |typist| {
        typist.type_text(&message);
        typist.type_text(". ");
    });
    let rest = learned_in_both_modes(cx, |typist| {
        typist.type_text(&message);
        let text = typist
            .textarea
            .read_with(&typist.cx, |state, _| state.text().clone());
        typist.suggestions.learn_rest(&text);
    });
    assert_eq!(rest, finished);
    assert!(rest.iter().any(|(word, _)| word == "omega"), "{rest:?}");
    assert!(!rest.iter().any(|(word, _)| word == "Alpha"), "{rest:?}");
}

/// A composer is cleared at every send, so a message or a sentence the user
/// sends again is learned again, each time.
#[gpui::test]
fn a_message_sent_again_is_learned_again(cx: &mut TestAppContext) {
    let send = |typist: &mut Typist| {
        let text = typist
            .textarea
            .read_with(&typist.cx, |state, _| state.text().clone());
        typist.suggestions.learn_rest(&text);
        typist.cx.update(|window, cx| {
            typist
                .textarea
                .update(cx, |state, cx| state.set_value("", window, cx))
        });
    };
    let learned = learned_in_both_modes(cx, |typist| {
        for _ in 0..3 {
            typist.type_text("Please fix the tests");
            send(typist);
            typist.type_text("Looks good. ");
            send(typist);
        }
    });
    assert_eq!(
        learned,
        counts(&[
            ("Looks", 3),
            ("Please", 3),
            ("fix", 3),
            ("good", 3),
            ("tests", 3),
            ("the", 3)
        ])
    );

    // In one text, a sentence written once is learned once.
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Looks good. ");
        typist.type_text("More text. ");
        send(typist);
    });
    assert_eq!(
        learned,
        counts(&[("Looks", 1), ("More", 1), ("good", 1), ("text", 1)])
    );
}

/// Edits made while the textarea had no suggestions, or other ones, never
/// reach these, so they can't tell where a paste moved: the text from
/// before is treated as not typed, and the paste is still never learned.
/// So is text a textarea held before its suggestions were installed.
#[gpui::test]
fn text_changed_while_the_suggestions_were_out_is_not_learned(cx: &mut TestAppContext) {
    let provide = |typist: &mut Typist, on: bool| {
        let suggestions: Option<Rc<dyn gpui_base::input::SuggestionProvider>> =
            on.then(|| typist.suggestions.clone() as _);
        typist.cx.update(|_, cx| {
            typist.textarea.update(cx, |state, cx| {
                state.set_suggestion_provider(suggestions, cx)
            })
        });
    };
    let caret_at = |typist: &mut Typist, offset: usize| {
        typist.cx.update(|_, cx| {
            typist
                .textarea
                .update(cx, |state, cx| state.set_selected_range(offset..offset, cx))
        });
    };
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Hi ");
        paste(typist, "secret");
        provide(typist, false);
        caret_at(typist, 0);
        typist.type_text("Hello there, my friend. ");
        provide(typist, true);
        let end = typist.value().len();
        caret_at(typist, end);
        typist.type_text(". Next words. ");
    });
    assert_eq!(learned, counts(&[("Next", 1), ("words", 1)]));

    let learned = learned_in_both_modes(cx, |typist| {
        provide(typist, false);
        typist.type_text("Hi secret");
        provide(typist, true);
        typist.type_text(". Next words. ");
    });
    assert_eq!(learned, counts(&[("Next", 1), ("words", 1)]));
}

/// One word pasted over a selected word looks like a spelling correction,
/// but the user didn't write it: its sentence teaches nothing.
#[gpui::test]
fn a_word_pasted_over_a_selection_learns_nothing(cx: &mut TestAppContext) {
    let word_left = if cfg!(target_os = "macos") {
        "alt-shift-left"
    } else {
        "ctrl-shift-left"
    };
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("The token is placeholder");
        typist.cx.simulate_keystrokes(word_left);
        paste(typist, "hunter2secret");
        assert_eq!(typist.value(), "The token is hunter2secret");
        typist.type_text(". Next");
    });
    assert_eq!(learned, []);
}

/// A line is learned once the user moves on from it, after a blank line
/// too, as is the last item of a list before one.
#[gpui::test]
fn a_line_before_a_blank_line_is_learned(cx: &mut TestAppContext) {
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Let me know what you think");
        typist.cx.simulate_keystrokes("enter enter");
        typist.type_text("Sounds");
    });
    assert_eq!(
        learned,
        counts(&[
            ("Let", 1),
            ("know", 1),
            ("me", 1),
            ("think", 1),
            ("what", 1),
            ("you", 1)
        ])
    );

    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("- buy milk");
        typist.cx.simulate_keystrokes("enter");
        typist.type_text("- call the bank");
        typist.cx.simulate_keystrokes("enter enter");
        typist.type_text("Next");
    });
    assert_eq!(
        learned,
        counts(&[
            ("bank", 1),
            ("buy", 1),
            ("call", 1),
            ("milk", 1),
            ("the", 1)
        ])
    );
}

/// Undo and redo teach nothing: a sentence typed once is learned once.
#[gpui::test]
fn undo_and_redo_learn_nothing(cx: &mut TestAppContext) {
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("Ferns grow slowly.");
        let once = learned(typist);
        assert_eq!(once.len(), 3);
        typist.cx.simulate_keystrokes("secondary-z");
        assert_ne!(typist.value(), "Ferns grow slowly.");
        typist.cx.simulate_keystrokes("secondary-shift-z");
        assert_eq!(typist.value(), "Ferns grow slowly.");
        assert_eq!(learned(typist), once);
        typist.type_text(" Mosses grow.");
        assert_eq!(typist.value(), "Ferns grow slowly. Mosses grow.");
    });
    assert_eq!(
        learned,
        counts(&[("Ferns", 1), ("Mosses", 1), ("grow", 2), ("slowly", 1)])
    );

    // A sentence typed with suggestions off isn't learned from its redo.
    let learned = learned_in_both_modes(cx, |typist| {
        let mode = typist.writing.suggestion_mode();
        typist.writing.set_suggestion_mode(SuggestionMode::Off);
        typist.type_text("Ferns grow slowly.");
        typist.writing.set_suggestion_mode(mode);
        typist.cx.simulate_keystrokes("secondary-z");
        typist.cx.simulate_keystrokes("secondary-shift-z");
        assert_eq!(typist.value(), "Ferns grow slowly.");
    });
    assert_eq!(learned, []);
}

/// How many times the vocabulary has learned `word`.
fn count(typist: &Typist, word: &str) -> u32 {
    learned(typist)
        .into_iter()
        .find(|(learned, _)| learned == word)
        .map_or(0, |(_, count)| count)
}

/// A sentence is learned once, whatever happens around it: an edit above it
/// doesn't make it new again, and reopening the document doesn't either. A
/// second copy the user types is learned.
#[gpui::test]
fn a_sentence_is_learned_once(cx: &mut TestAppContext) {
    let learned = learned_in_both_modes(cx, |typist| {
        typist.type_text("First line here.\nLet me know what you think. ");
        typist.cx.simulate_keystrokes(text_start());
        typist.type_text("A");
        typist.cx.simulate_keystrokes(text_end());
        typist.type_text("So");
        assert_eq!(count(typist, "know"), 1);
    });
    assert_eq!(learned.len(), 9, "{learned:?}");

    for mode in [SuggestionMode::Eager, SuggestionMode::Quiet] {
        let writing = writing(Vocabulary::new());
        let mut typist = Typist::open(cx, mode, writing.clone(), None);
        typist.type_text("Let me know what you think. ");
        assert_eq!(count(&typist, "know"), 1);

        let content = typist.value();
        let mut reopened = Typist::open(cx, mode, writing.clone(), Some(&content));
        reopened.cx.simulate_keystrokes(text_end());
        reopened.type_text("So");
        assert_eq!(reopened.value(), "Let me know what you think. So");
        assert_eq!((count(&reopened, "know"), count(&reopened, "So")), (1, 0));
        reopened.type_text(". Let me know what you think. Then");
        assert_eq!((count(&reopened, "know"), count(&reopened, "So")), (2, 1));
    }
}

/// A contraction is one word, with either apostrophe: "don't" is learned
/// whole, completed whole, and predicts what follows it.
#[gpui::test]
fn contractions_are_one_word(cx: &mut TestAppContext) {
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Eager, Vocabulary::new());
    for _ in 0..6 {
        typist.type_text("I don't know what to do. We’ll see. ");
    }
    assert_eq!((count(&typist, "don't"), count(&typist, "We’ll")), (6, 6));
    assert_eq!((count(&typist, "don"), count(&typist, "ll")), (0, 0));

    typist.type_text("I don't k");
    assert_eq!(typist.ghost().as_deref(), Some("now what to"));
    typist.type_text("now. I d");
    assert_eq!(typist.ghost().as_deref(), Some("on't know what"));
}

/// "Thanks" opening a sentence and "thanks" inside one are one word, so
/// what follows both is offered after either.
#[gpui::test]
fn a_word_in_another_case_is_the_same_word(cx: &mut TestAppContext) {
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Eager, Vocabulary::new());
    for _ in 0..4 {
        typist.type_text("Thanks for the update. ");
    }
    for _ in 0..3 {
        typist.type_text("Many thanks for the help. ");
    }
    typist.type_text("Tha");
    assert_eq!(typist.ghost().as_deref(), Some("nks for the"));
    typist.type_text("nks. Many tha");
    assert_eq!(typist.ghost().as_deref(), Some("nks for the"));
}

/// The vocabulary and the document's own words don't count the same text
/// twice: a document reopened with sentences it taught the vocabulary
/// suggests what a new document does.
#[gpui::test]
fn a_document_and_the_vocabulary_count_its_sentences_once(cx: &mut TestAppContext) {
    let writing = writing(Vocabulary::new());
    let mut first = Typist::open(cx, SuggestionMode::Eager, writing.clone(), None);
    first.type_text("We will circle back on the budget. We will circle back on the plan. ");
    let content = first.value();

    let mut new = Typist::open(cx, SuggestionMode::Eager, writing.clone(), None);
    new.type_text("Next week we will cir");
    let mut reopened = Typist::open(cx, SuggestionMode::Eager, writing, Some(&content));
    reopened.cx.simulate_keystrokes(text_end());
    reopened.type_text("Next week we will cir");
    assert_eq!(reopened.ghost(), new.ghost());
    assert_eq!(new.ghost().as_deref(), Some("cle back on"));
}

/// The ghost text the textarea shows: what taking the suggestion would add
/// after the caret.
fn shown_ghost(typist: &Typist) -> Option<String> {
    typist.textarea.read_with(&typist.cx, |state, _| {
        let suggestion = state.suggestions().first()?;
        let range = suggestion.range()?;
        let typed = state.value()[range.start..state.cursor()].to_string();
        let rest = suggestion.text().strip_prefix(typed.as_str())?;
        (!rest.is_empty()).then(|| rest.to_string())
    })
}

/// The next-word key takes the next word of a suggestion and leaves the
/// rest of it offered, in every keymap. With nothing offered it does what
/// the keymap binds it to. Ctrl+Right moves by a word in the standard
/// keymap and Emacs. Cmd+Right, on macOS, goes to the end of the line in the
/// standard keymap, while Emacs and Vim keep Cmd for the clipboard and
/// history. Vim's insert mode binds neither.
#[gpui::test]
fn the_next_word_key_takes_one_word(cx: &mut TestAppContext) {
    let key = if cfg!(target_os = "macos") {
        "cmd-right"
    } else {
        "ctrl-right"
    };
    for keymap in [Keymap::Cua, Keymap::Emacs, Keymap::Vim] {
        let mut typist = Typist::open_with_keymap(
            cx,
            SuggestionMode::Eager,
            keymap,
            writing(vocabulary()),
            None,
        );
        if keymap == Keymap::Vim {
            typist.cx.simulate_keystrokes("i");
        }
        typist.type_text("he");
        assert_eq!(
            shown_ghost(&typist).as_deref(),
            Some("llo world"),
            "{keymap:?}"
        );
        typist.cx.simulate_keystrokes(key);
        assert_eq!(typist.value(), "hello", "{keymap:?}");
        assert_eq!(
            shown_ghost(&typist).as_deref(),
            Some(" world"),
            "{keymap:?}"
        );
        typist.cx.simulate_keystrokes(key);
        assert_eq!(typist.value(), "hello world", "{keymap:?}");
        assert_eq!(shown_ghost(&typist), None, "{keymap:?}");
        // What it took counts as typed, so the sentence is learned.
        typist.type_text(". So");
        assert_eq!(count(&typist, "hello"), 41, "{keymap:?}");

        // At the start of the line, where nothing is offered. The keys to
        // get there differ from keymap to keymap.
        typist.cx.update(|_, cx| {
            typist
                .textarea
                .update(cx, |state, cx| state.set_selected_range(0..0, cx))
        });
        assert_eq!(shown_ghost(&typist), None, "{keymap:?}");
        typist.cx.simulate_keystrokes(key);
        let caret = typist
            .textarea
            .read_with(&typist.cx, |state, _| state.cursor());
        // On macOS, Cmd-Right goes to the line's end in every keymap.
        // Elsewhere Ctrl-Right moves a word in CUA and Emacs, and Vim
        // leaves it unbound.
        let moves = match keymap {
            Keymap::Cua | Keymap::Emacs => true,
            _ => cfg!(target_os = "macos"),
        };
        assert_eq!(caret > 0, moves, "{keymap:?}");
        assert_eq!(typist.value(), "hello world. So", "{keymap:?}");
    }
}

/// "UI/UX design" was typed before, and "UI/UX, " offers nothing after the
/// comma, where it offered "design". After "UI/UX " it does.
#[gpui::test]
fn a_comma_ends_what_a_suggestion_follows(cx: &mut TestAppContext) {
    let mut typist = Typist::with_vocabulary(cx, SuggestionMode::Eager, Vocabulary::new());
    typist.type_text(
        "We want better UI/UX design. Good UI/UX design matters. Their UI/UX design is clean. ",
    );
    typist.type_text("Improve the UI/UX, ");
    assert_eq!(typist.ghost(), None);
    typist.type_text("and the UI/UX ");
    assert_eq!(typist.ghost().as_deref(), Some("design"));
}

/// How often the document's words, as counted, hold `word`.
fn counted(suggestions: &WordSuggestions, word: &str) -> u32 {
    suggestions.local_index.borrow().counts().word_count(word)
}

/// A large document's words are counted on another thread, and join the
/// suggestions once that is done, with the edits made meanwhile. A small
/// one's are counted at once.
#[gpui::test]
fn a_large_document_is_counted_on_another_thread(cx: &mut TestAppContext) {
    let mut content = "Ferns grow slowly in the shade.\n".repeat(2500);
    content.push_str(&"Ask Wojciech about the ferns.\n".repeat(3));
    assert!(content.len() > LOCAL_COUNT_ON_UI_BYTES);
    let suggestions = writing(Vocabulary::new()).word_suggestions();
    let text = Rope::from(content.as_str());
    cx.update(|cx| suggestions.count_words(&text, text.len(), Some(cx.background_executor())));
    assert_eq!(counted(&suggestions, "wojciech"), 0);

    // Typed while it counts.
    let end = text.len();
    suggestions.count_edits([(end..end, "Wojciech"), (0..5, "")], end + 8 - 5);
    content.push_str("Wojciech");
    content.replace_range(0..5, "");
    let text = Rope::from(content.as_str());
    cx.run_until_parked();
    cx.update(|cx| suggestions.count_words(&text, 0, Some(cx.background_executor())));
    assert_eq!(counted(&suggestions, "wojciech"), 4);
    assert_eq!(counted(&suggestions, "ferns"), 2500 - 1 + 3);

    let small = writing(Vocabulary::new()).word_suggestions();
    let text = Rope::from("Ask Wojciech about the ferns.");
    cx.update(|cx| small.count_words(&text, 0, Some(cx.background_executor())));
    assert_eq!(counted(&small, "wojciech"), 1);
}

/// On a line too long to copy at each keystroke, such as minified code
/// pasted in, the excerpt holds only the part around the caret, cut on
/// character boundaries, and the suggestion and the sentence to learn come
/// out as from all of the text.
#[test]
fn a_very_long_line_is_cut_around_the_caret() {
    use super::model::generate_suggestion;
    use super::{EXCERPT_MAX_AFTER_BYTES, EXCERPT_MAX_BEFORE_BYTES, Excerpt, preceding_sentence};

    let text = "Größe für die Straße. Thanks for the update. ".repeat(20_000);
    let vocabulary = vocabulary();
    let rope = Rope::from(text.as_str());
    let mut local = super::LocalIndex::new();
    local.build(&rope, None, &|_| 0);
    let mut offset = 0;
    while offset <= text.len() {
        let excerpt = Excerpt::around(&rope, offset);
        let inside = excerpt.offset_in(offset);
        assert_eq!(excerpt.start + inside, offset);
        assert!(
            excerpt.text.len() <= EXCERPT_MAX_BEFORE_BYTES + EXCERPT_MAX_AFTER_BYTES + 3,
            "{} bytes at {offset}",
            excerpt.text.len()
        );
        let part = excerpt.text.as_str();
        assert_eq!(
            generate_suggestion(&vocabulary, &local, part, inside, &super::policy::Ungated),
            generate_suggestion(&vocabulary, &local, &text, offset, &super::policy::Ungated),
            "at {offset}"
        );
        let sentence = preceding_sentence(part, inside).map(|(range, end)| {
            (
                excerpt.start + range.start..excerpt.start + range.end,
                excerpt.start + end,
            )
        });
        assert_eq!(sentence, preceding_sentence(&text, offset), "at {offset}");
        offset += 30_011;
        while offset < text.len() && !text.is_char_boundary(offset) {
            offset += 1;
        }
    }
}

/// What a suggestion reads of the text, the excerpt around the caret holds:
/// the suggestion, the sentence to learn and whether it is prose come out
/// the same from it as from all of the text, wherever the caret is.
#[test]
fn the_text_around_the_caret_is_enough() {
    use super::model::generate_suggestion;
    use super::{Excerpt, preceding_sentence};
    use crate::classify::classify_context;

    let mut text = String::new();
    for paragraph in 0..300 {
        match paragraph % 7 {
            0 => text.push_str("```\nfn shift(range: Range<usize>) -> usize {\n    range.end\n}\n"),
            1 => text.push_str("```\n\n"),
            2 => text.push_str(&"A very long line of prose that goes on. ".repeat(60)),
            _ => text.push_str("Thanks for the update. We will circle back on the plan.\n"),
        }
        text.push_str("let total = compute_total(items);\nThanks for the help\n\n");
    }
    let vocabulary = vocabulary();
    let rope = Rope::from(text.as_str());
    let mut local = super::LocalIndex::new();
    local.build(&rope, None, &|_| 0);
    let mut offset = 0;
    while offset < text.len() {
        let excerpt = Excerpt::around(&rope, offset);
        let inside = excerpt.offset_in(offset);
        assert_eq!(excerpt.start + inside, offset);
        let part = excerpt.text.as_str();
        assert_eq!(
            generate_suggestion(&vocabulary, &local, part, inside, &super::policy::Ungated),
            generate_suggestion(&vocabulary, &local, &text, offset, &super::policy::Ungated),
            "at {offset}"
        );
        let sentence = preceding_sentence(part, inside).map(|(range, end)| {
            (
                excerpt.start + range.start..excerpt.start + range.end,
                excerpt.start + end,
            )
        });
        assert_eq!(sentence, preceding_sentence(&text, offset), "at {offset}");
        if let Some((_, end)) = sentence {
            assert_eq!(
                classify_context(part, end - excerpt.start),
                classify_context(&text, end),
                "at {offset}"
            );
        }
        offset += 97;
        while !text.is_char_boundary(offset) {
            offset += 1;
        }
    }
}

/// Made-up writing for the replay tests, sentences built from a few
/// phrases so that suggestions come up, with greetings, lists, code, pastes,
/// numbers, brackets and contractions among them.
struct MadeUp {
    state: u64,
}

/// A stretch of made-up writing, typed or pasted.
enum Segment {
    Typed(String),
    Pasted(String),
}

impl MadeUp {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }

    fn sentence(&mut self) -> String {
        const SUBJECTS: &[&str] = &["I", "We", "The team", "Maya", "Our client", "Everyone"];
        const VERBS: &[&str] = &[
            "will send",
            "needs",
            "should review",
            "can share",
            "wants to discuss",
            "finished",
        ];
        const OBJECTS: &[&str] = &[
            "the report",
            "the budget",
            "the new design",
            "the meeting notes",
            "the draft",
            "the onboarding guide",
        ];
        const TAILS: &[&str] = &[
            " by Friday",
            " tomorrow morning",
            " before the call",
            " next week",
            "",
            ", if that works",
        ];
        const FIXED: &[&str] = &[
            "Thanks for the update.",
            "Let me know if you have any questions.",
            "Sorry for the delay.",
            "I don't think we're ready yet.",
            "It's on the shared drive (see the folder).",
            "We have 3 drafts and 12 notes.",
            "The API is down again!",
            "Can you take a look at \"the draft\"?",
        ];
        match self.next() % 10 {
            0..=3 => FIXED[(self.next() % FIXED.len() as u64) as usize].to_string(),
            4 => format!("Hi {},", self.pick(&["Maya", "Sam", "Priya", "Tomasz"])),
            5 => format!("- {}{}", self.pick(OBJECTS), self.pick(TAILS)),
            _ => {
                let end = self.pick(&[".", ".", "!", "?"]);
                format!(
                    "{} {} {}{}{end}",
                    self.pick(SUBJECTS),
                    self.pick(VERBS),
                    self.pick(OBJECTS),
                    self.pick(TAILS)
                )
            }
        }
    }

    /// About `sentences` sentences, with line breaks, code and pastes.
    fn segments(&mut self, sentences: usize) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut typed = String::new();
        for index in 0..sentences {
            match self.next() % 40 {
                0 => {
                    typed.push_str("```\nlet total = compute_total(items);\nreturn total;\n```\n");
                }
                1 => {
                    segments.push(Segment::Typed(std::mem::take(&mut typed)));
                    segments.push(Segment::Pasted(
                        "Error: the build failed at step 4.\n".into(),
                    ));
                    continue;
                }
                _ => {}
            }
            typed.push_str(&self.sentence());
            typed.push_str(match self.next() % 6 {
                0 => "\n",
                1 => "\n\n",
                _ => " ",
            });
            if index % 25 == 24 {
                segments.push(Segment::Typed(std::mem::take(&mut typed)));
            }
        }
        segments.push(Segment::Typed(typed));
        segments
    }
}

/// What a typist does next.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Word,
    Escape,
}

/// Whether `ghost` is right where `rest` is what the writer means to type
/// next, all of it, case included, up to the end of a word.
fn is_right(ghost: &str, rest: &str) -> bool {
    rest.starts_with(ghost)
        && rest[ghost.len()..]
            .chars()
            .next()
            .is_none_or(|next| !super::is_word_char_at(rest, ghost.len(), next))
}

/// The key a typist presses next, at step `step`, with `ghost` on screen
/// and `rest` left to type. It takes a right suggestion with Tab, or now
/// and then with the next-word key, closes a wrong one with Escape now and
/// then, and now and then makes a typo it fixes at once.
fn next_key(ghost: Option<&str>, rest: &str, step: u64, typo: &mut bool) -> Key {
    if *typo {
        *typo = false;
        return Key::Backspace;
    }
    let next = rest.chars().next().expect("text left to type");
    match ghost {
        Some(ghost) if is_right(ghost, rest) => {
            return if step % 4 == 0 { Key::Word } else { Key::Tab };
        }
        Some(_) if step % 7 == 0 => return Key::Escape,
        _ => {}
    }
    if step % 53 == 0 && next.is_alphabetic() {
        *typo = true;
        return Key::Char('q');
    }
    if next == '\n' {
        Key::Enter
    } else {
        Key::Char(next)
    }
}

/// The pause before the key at `offset` of `text`. It is short within
/// words, and longer at some word starts and at sentence starts, so that
/// Quiet has pauses to show suggestions in.
fn gap_before(text: &str, offset: usize, step: u64) -> Duration {
    let before = text[..offset].chars().next_back();
    match before {
        None | Some('\n' | '.' | '!' | '?') => ms(700),
        Some(' ') if step % 3 == 0 => ms(450),
        _ if step % 11 == 0 => ms(350),
        _ => ms(90),
    }
}

/// Types `segments` into a replay field, as [`next_key`] decides, and calls
/// `seen` after every key with the key and the field.
fn type_segments(
    field: &mut crate::replay::Field,
    segments: &[Segment],
    mut seen: impl FnMut(Option<Key>, &str, Duration, &crate::replay::Field),
) {
    let mut step = 0u64;
    let mut typo = false;
    for segment in segments {
        let text = match segment {
            Segment::Pasted(text) => {
                field.paste(text);
                seen(None, text, Duration::ZERO, field);
                continue;
            }
            Segment::Typed(text) => text,
        };
        let mut offset = 0;
        while offset < text.len() || typo {
            step += 1;
            let gap = gap_before(text, offset, step);
            field.wait(gap);
            let key = next_key(field.ghost().as_deref(), &text[offset..], step, &mut typo);
            let inserted = match key {
                Key::Char(character) => {
                    field.type_key(&character.to_string());
                    if typo { 0 } else { character.len_utf8() }
                }
                Key::Enter => {
                    field.enter();
                    1
                }
                Key::Backspace => {
                    field.backspace();
                    0
                }
                Key::Tab => field.tab().map_or(0, |ghost| ghost.len()),
                Key::Word => field.accept_word().map_or(0, |word| word.len()),
                Key::Escape => {
                    field.escape();
                    0
                }
            };
            offset += inserted;
            seen(Some(key), "", gap, field);
        }
    }
}

/// The replay offers what a real textarea offers, key for key. The text,
/// the suggestion after every key, in Eager and in Quiet with
/// pauses, and the words learned are the same, through Tab, the next-word
/// key, Escape, typos, Enter and pastes.
#[gpui::test]
fn the_replay_offers_what_a_textarea_offers(cx: &mut TestAppContext) {
    for mode in [SuggestionMode::Eager, SuggestionMode::Quiet] {
        let segments = MadeUp::new(7).segments(160);
        let mut typist = Typist::with_vocabulary(cx, mode, Vocabulary::new());
        let replayed = writing(Vocabulary::new());
        replayed.set_suggestion_mode(mode);
        let writer = crate::replay::Writer::with_writing(replayed);
        let mut field = writer.field();
        let mut keys = 0;
        type_segments(&mut field, &segments, |key, pasted, gap, field| {
            match key {
                None => paste(&mut typist, pasted),
                Some(key) => {
                    typist.wait(gap);
                    match key {
                        Key::Char(character) => typist.type_text(&character.to_string()),
                        Key::Enter => typist.cx.simulate_keystrokes("enter"),
                        Key::Backspace => typist.cx.simulate_keystrokes("backspace"),
                        Key::Tab => typist.cx.simulate_keystrokes("tab"),
                        Key::Word => typist.cx.simulate_keystrokes(if cfg!(target_os = "macos") {
                            "cmd-right"
                        } else {
                            "ctrl-right"
                        }),
                        Key::Escape => typist.cx.simulate_keystrokes("escape"),
                    }
                }
            }
            keys += 1;
            assert_eq!(typist.value(), field.text(), "{mode:?}, key {keys} {key:?}");
            assert_eq!(
                typist.ghost().as_deref(),
                field.suggestion(),
                "{mode:?}, key {keys} {key:?}, after {:?}",
                &field.text()[field.text().len().saturating_sub(40)..]
            );
        });
        assert!(keys > 3000, "{keys}");
        assert_eq!(learned(&typist), writer.word_counts(), "{mode:?}");
    }
}

/// FNV-1a, a hash that stays the same from one Rust release to the next.
fn fnv(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

/// The offers a fixed stretch of made-up writing gets, replayed in Eager
/// and in Quiet, stay what they were, so a change meant to leave behaviour
/// alone must leave this hash alone. A change meant to alter the offers
/// updates it, and says what changed in its message.
#[test]
fn offers_on_fixed_writing_stay_the_same() {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut tabs = 0;
    let mut shown = 0;
    for mode in [SuggestionMode::Eager, SuggestionMode::Quiet] {
        let segments = MadeUp::new(11).segments(800);
        let replayed = writing(Vocabulary::new());
        replayed.set_suggestion_mode(mode);
        let writer = crate::replay::Writer::with_writing(replayed);
        let mut field = writer.field();
        type_segments(&mut field, &segments, |key, _, _, field| {
            tabs += usize::from(matches!(key, Some(Key::Tab | Key::Word)));
            let ghost = field.ghost();
            shown += usize::from(ghost.is_some());
            fnv(&mut hash, ghost.as_deref().unwrap_or("-").as_bytes());
            fnv(&mut hash, b"\0");
        });
        for (word, count) in writer.word_counts() {
            fnv(&mut hash, format!("{word} {count}\n").as_bytes());
        }
        // A composer's send learns the last sentence and empties it.
        field.type_key("A");
        field.send();
        assert_eq!(field.text(), "");
    }
    assert!(tabs > 1000 && shown > 3000, "{tabs} {shown}");
    assert_eq!(
        hash, 0x5b19_fc22_1bad_281b,
        "offers hash {hash:#x}, {tabs} taken, {shown} shown"
    );
}
