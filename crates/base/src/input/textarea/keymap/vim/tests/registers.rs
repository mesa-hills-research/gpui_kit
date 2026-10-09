//! Registers: named, appended, the yank and delete registers, the black
//! hole and the clipboard.

use gpui::{ClipboardItem, TestAppContext};

use super::*;
use crate::input::keymap::vim::register;

/// The text in register `name`.
fn register(test: &mut KeymapTest, name: char) -> Option<String> {
    test.cx
        .update(|_, cx| register::read(Some(name), cx).map(|register| register.text))
}

#[gpui::test]
fn named_registers(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone\ntwo");
    test.keys("\" a y y j");
    assert_eq!(register(&mut test, 'a').as_deref(), Some("one\n"));
    test.keys("\" a p");
    test.assert("one\ntwo\nˇone");

    // An upper-case name appends.
    let mut test = vim(cx, "ˇab cd");
    test.keys("\" a y w w \" A y w");
    assert_eq!(register(&mut test, 'a').as_deref(), Some("ab cd"));
    test.keys("$ \" a p");
    test.assert("ab cdab cˇd");
    // Appending lines makes the register linewise.
    test.keys("\" A y y");
    assert_eq!(
        register(&mut test, 'a').as_deref(),
        Some("ab cd\nab cdab cd\n")
    );
    // A name selects for one command only.
    test.keys("y w");
    assert_eq!(
        register(&mut test, 'a').as_deref(),
        Some("ab cd\nab cdab cd\n")
    );
}

#[gpui::test]
fn yank_and_delete_registers(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar\na\nb\nc");
    test.keys("y w");
    assert_eq!(register(&mut test, '0').as_deref(), Some("foo "));
    assert_eq!(register(&mut test, '"').as_deref(), Some("foo "));
    // A delete within a line goes to `-`, and the yank stays in `0`.
    test.keys("w d w");
    assert_eq!(register(&mut test, '-').as_deref(), Some("bar"));
    assert_eq!(register(&mut test, '"').as_deref(), Some("bar"));
    assert_eq!(register(&mut test, '0').as_deref(), Some("foo "));
    assert_eq!(register(&mut test, '1'), None);
    // Deleted lines shift through 1 to 9.
    test.keys("j d d d d d d");
    assert_eq!(register(&mut test, '1').as_deref(), Some("c\n"));
    assert_eq!(register(&mut test, '2').as_deref(), Some("b\n"));
    assert_eq!(register(&mut test, '3').as_deref(), Some("a\n"));
    test.keys("\" 3 p");
    test.assert("foo \nˇa");
    test.keys("\" 0 P");
    test.assert("foo \nfooˇ a");
}

#[gpui::test]
fn the_black_hole_keeps_the_unnamed_register(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇkeep\ndrop\nend");
    test.keys("y y j \" _ d d");
    test.assert("keep\nˇend");
    assert_eq!(register(&mut test, '"').as_deref(), Some("keep\n"));
    test.keys("p");
    test.assert("keep\nend\nˇkeep");
}

#[gpui::test]
fn clipboard_registers(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone\ntwo");
    test.keys("\" + y y");
    let clipboard = test.cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(clipboard.as_deref(), Some("one\n"));
    // Linewise text comes back linewise.
    test.keys("j \" + p");
    test.assert("one\ntwo\nˇone");

    test.cx
        .write_to_clipboard(ClipboardItem::new_string("xy".into()));
    test.keys("g g \" * P");
    test.assert("xˇyone\ntwo\none");
    assert_eq!(register(&mut test, '+').as_deref(), Some("xy"));
}

#[gpui::test]
fn puts_take_counts_and_keep_the_register_kind(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇx-");
    test.keys("y l 3 p");
    test.assert("xxxˇx-");

    let mut test = vim(cx, "ˇa\nb");
    test.keys("y y j 2 P");
    test.assert("a\nˇa\na\nb");
    test.keys("g p");
    test.assert("a\na\na\nˇa\nb");
}

#[gpui::test]
fn read_only_registers(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇ");
    test.keys("i");
    test.type_text("hey");
    test.keys("escape \" . p");
    test.assert("heyheˇy");
    test.keys("/");
    test.type_text("ey");
    test.keys("enter 0 \" / P");
    test.assert("eˇyheyhey");
}
