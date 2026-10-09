//! Insert and replace modes.

use gpui::TestAppContext;

use super::*;
use crate::input::Suggestion;

#[gpui::test]
fn the_ways_into_insert_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "  aˇbc\nx");
    test.keys("i");
    test.assert("  aˇbc\nx");
    test.assert_mode(VimMode::Insert);
    assert_eq!(test.shape(), CursorShape::Bar);
    assert_eq!(test.label(), "INSERT");
    test.keys("escape");
    // Leaving moves the caret onto the character before it.
    test.assert("  ˇabc\nx");
    test.keys("a escape");
    test.assert("  ˇabc\nx");
    test.keys("A");
    test.assert("  abcˇ\nx");
    test.keys("escape I");
    test.assert("  ˇabc\nx");
    test.keys("escape g I");
    test.assert("ˇ  abc\nx");
    test.keys("escape o");
    test.assert("  abc\n  ˇ\nx");
    test.keys("escape");
    // Nothing typed: the new line's indentation goes again.
    test.assert("  abc\nˇ\nx");
    test.keys("O");
    test.type_text("y");
    test.keys("ctrl-[");
    test.assert("  abc\nˇy\n\nx");
    test.keys("A ctrl-c");
    test.assert_mode(VimMode::Normal);
}

#[gpui::test]
fn insert_mode_keys(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇ");
    test.keys("i");
    test.type_text("one two");
    test.keys("ctrl-w");
    test.assert("one ˇ");
    test.keys("ctrl-h");
    test.assert("oneˇ");
    test.keys("enter");
    test.type_text("  three");
    test.keys("ctrl-u");
    test.assert("one\n  ˇ");
    test.keys("ctrl-u");
    test.assert("one\nˇ");
    test.keys("ctrl-u");
    test.assert("oneˇ");
    test.keys("left left");
    test.type_text("-");
    test.assert("o-ˇne");
    // Ctrl-T and Ctrl-D shift the line and keep the caret on its character.
    test.keys("ctrl-t");
    test.assert("  o-ˇne");
    test.keys("ctrl-d");
    test.assert("o-ˇne");
    // Ctrl-V inserts the next key as it is, and Ctrl-R a register.
    test.keys("ctrl-v tab");
    test.assert("o-\tˇne");
    test.keys("escape y y A ctrl-r \"");
    test.assert("o-\tneo-\tne\nˇ");
}

#[gpui::test]
fn escape_closes_suggestions_first(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇ");
    test.keys("i");
    test.type_text("he");
    test.update(|state, _, cx| {
        state.present_suggestions(vec![Suggestion::new("hello"), Suggestion::new("help")], cx)
    });
    assert!(
        test.textarea
            .read_with(&test.cx, |state, _| state.is_suggestion_menu_open())
    );
    test.keys("escape");
    assert!(
        !test
            .textarea
            .read_with(&test.cx, |state, _| state.is_suggestion_menu_open())
    );
    test.assert_mode(VimMode::Insert);
    test.assert("heˇ");
    test.keys("escape");
    test.assert_mode(VimMode::Normal);
    test.assert("hˇe");

    // With the menu open, Ctrl-N and Ctrl-P choose and Enter accepts.
    test.keys("A");
    test.update(|state, _, cx| {
        state.present_suggestions(
            vec![
                Suggestion::new("hello").with_range(0..2),
                Suggestion::new("help").with_range(0..2),
            ],
            cx,
        )
    });
    test.keys("ctrl-n");
    assert_eq!(
        test.textarea
            .read_with(&test.cx, |state, _| state.selected_suggestion_ix()),
        Some(1)
    );
    test.keys("ctrl-p enter");
    test.assert("helloˇ");
}

#[gpui::test]
fn replace_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "aˇbcd\nx");
    test.keys("R");
    test.assert_mode(VimMode::Replace);
    assert_eq!(test.label(), "REPLACE");
    assert_eq!(test.shape(), CursorShape::Underline);
    test.type_text("XYZW");
    test.assert("aXYZWˇ\nx");
    // Backspace puts back what was replaced, and removes what was added.
    test.keys("backspace backspace");
    test.assert("aXYˇd\nx");
    test.keys("escape");
    test.assert("aXˇYd\nx");
    test.keys("u");
    test.assert("aˇbcd\nx");
    test.keys("0 .");
    test.assert("XˇYcd\nx");
}

#[gpui::test]
fn insert_count_and_switching_schemes_mid_insert(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇ!");
    test.keys("3 a");
    test.type_text("ab");
    test.keys("escape");
    test.assert("!ababaˇb");

    test.keys("i");
    test.update(|state, _, cx| state.set_keymap(Keymap::Cua, cx));
    test.type_text("z");
    test.assert("!ababazˇb");
}
