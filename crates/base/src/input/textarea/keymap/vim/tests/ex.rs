//! The command line and Ex commands.

use std::{cell::RefCell, rc::Rc};

use gpui::TestAppContext;

use super::*;
use crate::input::{VimCommand, VimQuit, VimWrite};

/// What reached the application: `w`, `q` or the command of a
/// [`VimCommand`].
fn listen(test: &mut KeymapTest) -> Rc<RefCell<Vec<String>>> {
    let heard: Rc<RefCell<Vec<String>>> = Rc::default();
    test.cx.update(|_, cx| {
        let write = heard.clone();
        cx.on_action(move |_: &VimWrite, _| write.borrow_mut().push("w".into()));
        let quit = heard.clone();
        cx.on_action(move |_: &VimQuit, _| quit.borrow_mut().push("q".into()));
        let command = heard.clone();
        cx.on_action(move |action: &VimCommand, _| {
            command.borrow_mut().push(action.command.to_string())
        });
    });
    heard
}

#[gpui::test]
fn the_command_line_shows_as_the_mode_label(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone\ntwo\nthree");
    test.keys(":");
    assert_eq!(test.label(), ":");
    test.type_text("3x");
    assert_eq!(test.label(), ":3x");
    test.keys("backspace");
    assert_eq!(test.label(), ":3");
    test.keys("enter");
    assert_eq!(test.label(), "NORMAL");
    test.assert("one\ntwo\nˇthree");

    test.keys(": 1 enter");
    test.assert("ˇone\ntwo\nthree");
    test.keys(": $ enter");
    test.assert("one\ntwo\nˇthree");

    // Escape, or Backspace on an empty line, closes it.
    test.keys(": escape");
    assert_eq!(test.label(), "NORMAL");
    test.keys(": backspace");
    assert_eq!(test.label(), "NORMAL");
    test.keys(":");
    test.type_text("a b");
    test.keys("ctrl-w");
    assert_eq!(test.label(), ":a ");
    test.keys("ctrl-u");
    assert_eq!(test.label(), ":");
}

#[gpui::test]
fn substitute(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa a\na a\na a");
    test.keys(":");
    test.type_text("s/a/b/");
    test.keys("enter");
    test.assert("ˇb a\na a\na a");
    test.keys(":");
    test.type_text("s/a/c/g");
    test.keys("enter");
    test.assert("ˇb c\na a\na a");
    test.keys(":");
    test.type_text("%s/a/x/g");
    test.keys("enter");
    test.assert("b c\nx x\nˇx x");
    test.keys("u");
    test.assert("ˇb c\na a\na a");
    test.keys(":");
    test.type_text("2,3s#a#-#");
    test.keys("enter");
    test.assert("b c\n- a\nˇ- a");

    // Groups, `&`, and a line break in the replacement.
    let mut test = vim(cx, "ˇone two");
    test.keys(":");
    test.type_text("s/\\(\\w\\+\\) \\(\\w\\+\\)/\\2 \\1 [&]/");
    test.keys("enter");
    test.assert("ˇtwo one [one two]");
    test.keys(":");
    test.type_text("s/ /\\r/g");
    test.keys("enter");
    test.assert("two\none\n[one\nˇtwo]");
}

#[gpui::test]
fn substitute_in_the_visual_selection(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa\na\na\na");
    test.keys("j V j :");
    assert_eq!(test.label(), ":'<,'>");
    test.type_text("s/a/b/");
    test.keys("enter");
    test.assert("a\nb\nˇb\na");
    // A count before `:` is a range of that many lines.
    test.keys("g g 2 :");
    assert_eq!(test.label(), ":.,.+1");
    test.type_text("s/$/!/");
    test.keys("enter");
    test.assert("a!\nˇb!\nb\na");
}

#[gpui::test]
fn line_commands(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone\ntwo\nthree\nfour");
    test.keys(":");
    test.type_text("2,3d");
    test.keys("enter");
    test.assert("one\nˇfour");
    test.keys("p");
    test.assert("one\nfour\nˇtwo\nthree");
    test.keys(":");
    test.type_text("1y a");
    test.keys("enter \" a P");
    test.assert("one\nfour\nˇone\ntwo\nthree");

    // `:noh` hides the search highlight.
    test.keys("* : n o h enter");
    assert!(
        !test
            .textarea
            .read_with(&test.cx, |state, _| state.search_session().is_active())
    );
}

#[gpui::test]
fn write_and_quit_reach_the_application(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇtext");
    let heard = listen(&mut test);
    test.keys(": w enter");
    assert_eq!(*heard.borrow(), ["w"]);
    test.keys(": q enter");
    test.keys(": w q enter");
    test.keys(": x enter");
    test.keys("Z Z Z Q");
    assert_eq!(
        *heard.borrow(),
        ["w", "q", "w", "q", "w", "q", "w", "q", "q"]
    );
    heard.borrow_mut().clear();

    // Commands Vim doesn't have reach it as typed.
    test.keys(":");
    test.type_text("e notes.md");
    test.keys("enter");
    assert_eq!(*heard.borrow(), ["e notes.md"]);
    test.assert("ˇtext");
}
