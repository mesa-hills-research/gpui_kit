//! The Emacs scheme, driven with keystrokes.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{ClipboardItem, TestAppContext};

use super::super::{Keymap, KeymapPlatform, common, test::KeymapTest};
use super::*;

fn emacs(cx: &mut TestAppContext, marked: &str) -> KeymapTest {
    KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::Linux, marked)
}

fn kill_ring(test: &KeymapTest) -> Vec<String> {
    test.textarea.read_with(&test.cx, |state, _| {
        state
            .keymap_state::<EmacsState>()
            .map(|emacs| emacs.kill_ring().map(String::from).collect())
            .unwrap_or_default()
    })
}

fn clipboard(test: &mut KeymapTest) -> Option<String> {
    test.cx
        .update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn caret_row(test: &KeymapTest) -> u32 {
    test.textarea
        .read_with(&test.cx, |state, _| state.cursor_position().line)
}

/// Listen for `A` at the application, and report whether it arrived.
fn listen<A: Action>(test: &mut KeymapTest) -> Rc<Cell<bool>> {
    let heard = Rc::new(Cell::new(false));
    let flag = heard.clone();
    test.cx.update(|_, cx| {
        cx.on_action(move |_: &A, _| flag.set(true));
    });
    heard
}

/// The keystrokes of each binding, written as in a keymap.
fn keys(bindings: &[KeyBinding]) -> Vec<String> {
    bindings
        .iter()
        .map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.inner().unparse())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn every_meta_key_also_follows_escape() {
    for platform in KeymapPlatform::ALL {
        let keys = keys(&bindings(platform));
        let meta: Vec<&String> = keys.iter().filter(|key| key.starts_with("alt-")).collect();
        assert!(meta.len() > 30, "{platform:?}");
        for key in meta {
            let escaped = format!("escape {}", &key["alt-".len()..]);
            assert!(
                keys.contains(&escaped),
                "{platform:?}: {key} without {escaped}"
            );
        }
    }
}

#[test]
fn the_tables_differ_only_in_macos_cmd_keys() {
    let linux = keys(&bindings(KeymapPlatform::Linux));
    assert_eq!(linux, keys(&bindings(KeymapPlatform::Windows)));
    let mac = keys(&bindings(KeymapPlatform::MacOS));
    let cmd: Vec<String> = mac.into_iter().filter(|key| !linux.contains(key)).collect();
    let expected = ["cmd-a", "cmd-c", "cmd-x", "cmd-v", "cmd-z", "cmd-shift-z"]
        .map(|key| gpui::Keystroke::parse(key).unwrap().unparse());
    assert_eq!(cmd, expected);

    // Past the shared keys it rebinds, Emacs binds each key once.
    let shared = common::bindings(Keymap::Emacs.context()).len();
    let mut own = keys(&bindings(KeymapPlatform::MacOS)[shared..]);
    own.sort();
    let count = own.len();
    own.dedup();
    assert_eq!(own.len(), count);
}

#[gpui::test]
fn meta_is_alt_or_escape_on_every_platform(cx: &mut TestAppContext) {
    for platform in KeymapPlatform::ALL {
        let mut test = KeymapTest::new(cx, Keymap::Emacs, platform, "ˇone two\none three");
        test.keys("alt-f");
        test.assert("oneˇ two\none three");
        test.keys("alt-f alt-f");
        test.assert("one two\noneˇ three");
        test.keys("alt-b");
        test.assert("one two\nˇone three");
        test.keys("escape f");
        test.assert("one two\noneˇ three");
        test.keys("escape <");
        test.assert("ˇone two\none three");
        test.keys("alt->");
        test.assert("one two\none threeˇ");
    }
}

#[gpui::test]
fn control_keys_and_arrows_move(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone two\none three");
    test.keys("ctrl-f");
    test.assert("oˇne two\none three");
    test.keys("ctrl-b");
    test.assert("ˇone two\none three");
    test.keys("ctrl-e");
    test.assert("one twoˇ\none three");
    test.keys("ctrl-a ctrl-f ctrl-f ctrl-f ctrl-f ctrl-n");
    test.assert("one two\none ˇthree");
    test.keys("ctrl-p");
    test.assert("one ˇtwo\none three");
    test.keys("down right");
    test.assert("one two\none tˇhree");
    test.keys("up left left");
    test.assert("oneˇ two\none three");
    test.keys("ctrl-right");
    test.assert("one twoˇ\none three");
    test.keys("ctrl-left home");
    test.assert("ˇone two\none three");
    test.keys("end");
    test.assert("one twoˇ\none three");
}

#[gpui::test]
fn words_are_letters_and_digits(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇdon't, x2 foo_bar");
    test.keys("alt-f");
    test.assert("don'tˇ, x2 foo_bar");
    test.keys("alt-f");
    test.assert("don't, x2ˇ foo_bar");
    test.keys("alt-f");
    test.assert("don't, x2 fooˇ_bar");
    test.keys("alt-b alt-b");
    test.assert("don't, ˇx2 foo_bar");
}

#[gpui::test]
fn sentences_end_at_punctuation_and_paragraphs(cx: &mut TestAppContext) {
    let text = "Hello there. How are you? Fine!\n\nNext one.";
    let at = |offset: usize| format!("{}ˇ{}", &text[..offset], &text[offset..]);
    let mut test = emacs(cx, &at(0));
    for offset in [12, 25, 31, 42, 42] {
        test.keys("alt-e");
        test.assert(&at(offset));
    }
    for offset in [33, 26, 13, 0, 0] {
        test.keys("alt-a");
        test.assert(&at(offset));
    }
}

#[gpui::test]
fn paragraphs_indentation_and_the_ends_of_the_text(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone\ntwo\n\n  three");
    test.keys("alt-}");
    test.assert("one\ntwo\nˇ\n  three");
    test.keys("ctrl-down");
    test.assert("one\ntwo\n\n  threeˇ");
    test.keys("alt-m");
    test.assert("one\ntwo\n\n  ˇthree");
    test.keys("alt-{");
    test.assert("one\ntwo\nˇ\n  three");
    test.keys("ctrl-up");
    test.assert("ˇone\ntwo\n\n  three");

    // Leaving for an end of the text sets the mark where the caret was.
    let mut test = emacs(cx, "one\ntwˇo");
    test.keys("alt-<");
    test.assert("ˇone\ntwo");
    test.keys("ctrl-x ctrl-x");
    test.assert("«one\ntwˇ»o");
}

#[gpui::test]
fn page_keys_move_by_the_height_of_the_textarea(cx: &mut TestAppContext) {
    let lines: Vec<String> = (0..80).map(|line| format!("line {line}")).collect();
    let mut test = emacs(cx, &format!("ˇ{}", lines.join("\n")));
    test.keys("ctrl-v");
    let page = caret_row(&test);
    assert!(page > 3, "{page}");
    test.keys("pagedown");
    assert_eq!(caret_row(&test), page * 2);
    test.keys("alt-v pageup");
    assert_eq!(caret_row(&test), 0);

    test.keys("ctrl-space ctrl-v");
    assert_eq!(caret_row(&test), page);
    let selected = test
        .textarea
        .read_with(&test.cx, |state, _| state.selected_range());
    assert_eq!(selected.start, 0);
}

#[gpui::test]
fn motions_extend_the_region_while_the_mark_is_active(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space ctrl-f ctrl-f");
    test.assert("«heˇ»llo world");
    test.keys("alt-f");
    test.assert("«helloˇ» world");
    test.keys("ctrl-b");
    test.assert("«hellˇ»o world");
    test.keys("right");
    test.assert("«helloˇ» world");
    test.keys("ctrl-g");
    test.assert("helloˇ world");
    test.keys("ctrl-f");
    test.assert("hello ˇworld");

    // The region goes either way from the mark.
    test.keys("ctrl-@ ctrl-a");
    test.assert("«ˇhello »world");
    test.keys("ctrl-e");
    test.assert("hello «worldˇ»");
}

#[gpui::test]
fn exchange_point_and_mark_and_select_all(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space alt-f");
    test.assert("«helloˇ» world");
    test.keys("ctrl-x ctrl-x");
    test.assert("«ˇhello» world");
    test.keys("ctrl-x ctrl-x");
    test.assert("«helloˇ» world");
    test.keys("ctrl-x h");
    test.assert("«ˇhello world»");
    test.keys("ctrl-x ctrl-x");
    test.assert("«hello worldˇ»");
}

#[gpui::test]
fn set_mark_twice_deactivates_and_c_u_jumps_back(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "heˇllo world");
    test.keys("ctrl-space ctrl-space ctrl-e");
    test.assert("hello worldˇ");
    test.keys("ctrl-u ctrl-space");
    test.assert("heˇllo world");
}

#[gpui::test]
fn a_click_or_a_shift_selection_ends_the_region(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space ctrl-f");
    test.assert("«hˇ»ello world");
    test.update(|state, _, cx| state.set_selected_range(8..8, cx));
    test.keys("ctrl-f");
    test.assert("hello worˇld");

    // Shift selects without the mark. A motion then moves from the caret.
    test.keys("ctrl-a shift-right shift-right");
    test.assert("«heˇ»llo world");
    test.keys("ctrl-f");
    test.assert("helˇlo world");
    test.keys("ctrl-a shift-right shift-right ctrl-w");
    test.assert("ˇllo world");
    assert_eq!(kill_ring(&test), ["he"]);
}

#[gpui::test]
fn the_mark_moves_with_edits(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "abˇc");
    test.keys("ctrl-space ctrl-g ctrl-a");
    test.type_text("XY");
    test.assert("XYˇabc");
    let mark = test.textarea.read_with(&test.cx, |state, _| {
        state
            .keymap_state::<EmacsState>()
            .and_then(EmacsState::mark)
    });
    assert_eq!(mark, Some(4));
    test.keys("ctrl-x ctrl-x");
    test.assert("XY«abˇ»c");
}

#[gpui::test]
fn kills_in_a_row_join_and_go_to_the_clipboard(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "heˇllo\nworld\nend");
    test.keys("ctrl-k");
    test.assert("heˇ\nworld\nend");
    test.keys("ctrl-k");
    test.assert("heˇworld\nend");
    test.keys("ctrl-k");
    test.assert("heˇ\nend");
    assert_eq!(kill_ring(&test), ["llo\nworld"]);
    assert_eq!(clipboard(&mut test).as_deref(), Some("llo\nworld"));
    test.keys("ctrl-y");
    test.assert("hello\nworldˇ\nend");

    // Spaces before the line break go with it.
    let mut test = emacs(cx, "oneˇ  \ntwo");
    test.keys("ctrl-k");
    test.assert("oneˇtwo");

    // A count kills whole lines.
    let mut test = emacs(cx, "aˇ\nb\nc\nd");
    test.keys("ctrl-u 2 ctrl-k");
    test.assert("aˇc\nd");
    assert_eq!(kill_ring(&test), ["\nb\n"]);
}

#[gpui::test]
fn a_motion_between_kills_starts_a_new_entry(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone two");
    test.keys("alt-d");
    test.assert("ˇ two");
    test.keys("ctrl-f alt-d");
    test.assert(" ˇ");
    assert_eq!(kill_ring(&test), ["two", "one"]);

    test.keys("ctrl-y");
    test.assert(" twoˇ");
    test.keys("alt-y");
    test.assert(" oneˇ");
    test.keys("alt-y");
    test.assert(" twoˇ");

    // Meta-Y only follows a yank.
    test.keys("ctrl-b alt-y");
    test.assert(" twˇo");
}

#[gpui::test]
fn backward_kills_join_at_the_start(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "one two threeˇ");
    test.keys("alt-backspace ctrl-backspace");
    test.assert("one ˇ");
    assert_eq!(kill_ring(&test), ["two three"]);
    test.keys("ctrl-y");
    test.assert("one two threeˇ");

    let mut test = emacs(cx, "ˇone two three");
    test.keys("alt-d ctrl-delete");
    test.assert("ˇ three");
    assert_eq!(kill_ring(&test), ["one two"]);
}

#[gpui::test]
fn the_region_is_killed_or_copied(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space alt-f ctrl-w");
    test.assert("ˇ world");
    assert_eq!(clipboard(&mut test).as_deref(), Some("hello"));
    test.keys("ctrl-e ctrl-y");
    test.assert(" worldhelloˇ");

    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space alt-f alt-w");
    test.assert("helloˇ world");
    test.keys("ctrl-e ctrl-y");
    test.assert("hello worldhelloˇ");

    // The region runs to the mark even when it is not active.
    let mut test = emacs(cx, "ˇhello world");
    test.keys("ctrl-space alt-f ctrl-g");
    test.assert("helloˇ world");
    test.keys("ctrl-w");
    test.assert("ˇ world");
}

#[gpui::test]
fn yank_takes_text_copied_elsewhere(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇmine");
    test.keys("ctrl-k");
    test.assert("ˇ");
    test.cx
        .update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("theirs".into())));
    test.keys("ctrl-y");
    test.assert("theirsˇ");
    test.keys("alt-y");
    test.assert("mineˇ");
    assert_eq!(kill_ring(&test), ["theirs", "mine"]);

    // What the ring put on the clipboard is not taken again.
    test.keys("ctrl-a ctrl-k ctrl-y");
    test.assert("mineˇ");
    assert_eq!(kill_ring(&test), ["mine", "theirs", "mine"]);
}

#[gpui::test]
fn yank_with_a_prefix(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone two");
    test.keys("alt-d ctrl-f alt-d");
    assert_eq!(kill_ring(&test), ["two", "one"]);

    // C-u leaves the caret before the text and the mark after it.
    test.keys("ctrl-u ctrl-y");
    test.assert(" ˇtwo");
    test.keys("ctrl-x ctrl-x");
    test.assert(" «twoˇ»");

    // A count yanks an older kill.
    test.keys("ctrl-g ctrl-e alt-2 ctrl-y");
    test.assert(" twooneˇ");
}

#[gpui::test]
fn deleting_and_transposing(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "aˇbcdefgh");
    test.keys("ctrl-d");
    test.assert("aˇcdefgh");
    test.keys("ctrl-u 2 ctrl-d");
    test.assert("aˇefgh");
    test.keys("ctrl-e backspace");
    test.assert("aefgˇ");
    test.keys("alt-2 backspace");
    test.assert("aeˇ");

    let mut test = emacs(cx, "abˇcd");
    test.keys("ctrl-t");
    test.assert("acbˇd");

    let mut test = emacs(cx, "oneˇ two three");
    test.keys("alt-t");
    test.assert("two oneˇ three");
    test.keys("alt-t");
    test.assert("two three oneˇ");
    test.keys("alt-t");
    test.assert("two three oneˇ");
}

#[gpui::test]
fn case_follows_words(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇhello WIDE World");
    test.keys("alt-u");
    test.assert("HELLOˇ WIDE World");
    test.keys("alt-l");
    test.assert("HELLO wideˇ World");
    test.keys("alt-- alt-c");
    test.assert("HELLO Wideˇ World");
    test.keys("ctrl-a alt-2 alt-l");
    test.assert("hello wideˇ World");

    let mut test = emacs(cx, "ˇdon't stop");
    test.keys("alt-c");
    test.assert("Don'tˇ stop");
}

#[gpui::test]
fn opening_and_joining_lines(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "abˇcd");
    test.keys("ctrl-o");
    test.assert("abˇ\ncd");
    test.keys("ctrl-j");
    test.assert("ab\nˇ\ncd");

    let mut test = emacs(cx, "one\n   twˇo");
    test.keys("alt-^");
    test.assert("oneˇ two");
}

#[gpui::test]
fn undo_keys(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇabc def");
    for undo in ["ctrl-/", "ctrl-_", "ctrl-x u"] {
        test.keys("ctrl-a alt-d");
        test.assert(" def");
        test.keys(undo);
        test.assert("abc def");
    }
}

#[gpui::test]
fn prefix_arguments_repeat_commands(cx: &mut TestAppContext) {
    let text = "abcdefghijklmnopqrstuvwxyz";
    let at = |offset: usize| format!("{}ˇ{}", &text[..offset], &text[offset..]);
    let mut test = emacs(cx, &at(0));
    test.keys("ctrl-u ctrl-f");
    test.assert(&at(4));
    test.keys("ctrl-u 1 2 ctrl-f");
    test.assert(&at(16));
    test.keys("alt-3 ctrl-b");
    test.assert(&at(13));
    test.keys("escape 1 escape 0 ctrl-f");
    test.assert(&at(23));
    test.keys("alt-- ctrl-f");
    test.assert(&at(22));
    test.keys("ctrl-u ctrl-u ctrl-u ctrl-b");
    test.assert(&at(0));

    // C-g drops the prefix.
    test.keys("ctrl-u ctrl-g ctrl-f");
    test.assert(&at(1));
}

#[gpui::test]
fn a_prefix_repeats_typed_text_and_shows_as_the_mode_label(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇ");
    assert_eq!(test.mode_label(), None);
    test.keys("ctrl-u");
    assert_eq!(test.mode_label().as_deref(), Some("C-u 4"));
    test.keys("ctrl-u");
    assert_eq!(test.mode_label().as_deref(), Some("C-u 16"));
    test.keys("3");
    assert_eq!(test.mode_label().as_deref(), Some("C-u 3"));
    test.keys("x");
    test.assert("xxxˇ");
    assert_eq!(test.mode_label(), None);
    test.keys("alt-2 y");
    test.assert("xxxyyˇ");
    test.keys("ctrl-u -");
    assert_eq!(test.mode_label().as_deref(), Some("C-u -"));
}

#[gpui::test]
fn c_x_waits_for_its_second_key_and_c_g_cancels_it(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇabc");
    test.keys("ctrl-x ctrl-g h");
    test.assert("hˇabc");
    test.keys("ctrl-x h");
    test.assert("«ˇhabc»");
    test.keys("escape ctrl-g");
    test.assert("ˇhabc");
}

#[gpui::test]
fn escape_alone_still_reaches_the_textarea(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "one twoˇ");
    test.update(|state, _, _| state.set_clean_on_escape(true));
    test.keys("escape");
    test.assert("one twoˇ");
    test.cx.executor().advance_clock(Duration::from_secs(2));
    test.draw();
    test.assert("ˇ");
}

#[gpui::test]
fn search_keys_open_the_panel(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone two one");
    test.update(|state, _, _| state.searchable = true);
    test.keys("ctrl-s");
    assert!(
        test.textarea
            .read_with(&test.cx, |state, _| state.search_session().open)
    );

    test.update(|state, _, cx| {
        state.close_search(cx);
        state.set_search_query("one", true, cx);
    });
    test.keys("ctrl-r");
    let current = test.textarea.read_with(&test.cx, |state, _| {
        state.search_session().matcher.current_match_index()
    });
    assert_eq!(current, 1, "the previous match wraps to the last");

    // A textarea without the panel leaves search to the application.
    let mut test = emacs(cx, "ˇone");
    let searched = listen::<Search>(&mut test);
    test.keys("ctrl-s");
    assert!(searched.get());
}

#[gpui::test]
fn save_keys_reach_the_application(cx: &mut TestAppContext) {
    let mut test = emacs(cx, "ˇone");
    let saved = listen::<SaveBuffer>(&mut test);
    let saved_as = listen::<WriteFile>(&mut test);
    test.keys("ctrl-x ctrl-s");
    assert!(saved.get());
    assert!(!saved_as.get());
    test.keys("ctrl-x ctrl-w");
    assert!(saved_as.get());
    test.assert("ˇone");
}

#[gpui::test]
fn emacs_keys_do_nothing_in_other_schemes(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, "heˇllo world");
    test.keys("ctrl-k ctrl-space ctrl-g");
    test.assert("heˇllo world");
    test.dispatch(KillLine);
    test.assert("heˇllo world");

    let mut test = KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::Linux, "heˇllo world");
    test.keys("ctrl-k ctrl-y ctrl-space");
    test.assert("heˇllo world");
    // In Vim, C-x waits for nothing and h moves left.
    test.keys("ctrl-x h");
    test.assert("hˇello world");

    // Switching to Emacs starts with an empty kill ring.
    test.update(|state, _, cx| state.set_keymap(Keymap::Emacs, cx));
    assert!(kill_ring(&test).is_empty());
    test.keys("ctrl-k");
    test.assert("hˇ");
}

#[gpui::test]
fn macos_keeps_cmd_for_the_clipboard_and_history(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::MacOS, "ˇhello world");
    test.keys("alt-d");
    test.assert(" world");
    test.keys("cmd-z");
    test.assert("hello world");

    // Text copied with Cmd-C goes into the kill ring at the next yank.
    test.keys("ctrl-e ctrl-space alt-b cmd-c ctrl-g");
    test.assert("hello ˇworld");
    test.keys("ctrl-e ctrl-y");
    test.assert("hello worldworldˇ");
    test.keys("cmd-v");
    test.assert("hello worldworldworldˇ");
    assert_eq!(kill_ring(&test), ["world", "hello"]);
}
