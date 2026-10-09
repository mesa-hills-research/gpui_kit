//! Normal mode's commands that change text directly.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn delete_characters(cx: &mut TestAppContext) {
    let mut test = vim(cx, "abcˇdef\nx");
    test.keys("x");
    test.assert("abcˇef\nx");
    // A count runs to the line's end, and no further.
    test.keys("5 x");
    test.assert("abˇc\nx");
    test.keys("X");
    test.assert("aˇc\nx");
    test.keys("0 X");
    test.assert("ˇac\nx");
    test.keys("j x x");
    test.assert("ac\nˇ");
    test.keys("x");
    test.assert("ac\nˇ");
}

#[gpui::test]
fn replace_characters(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇabcd");
    test.keys("r x");
    test.assert("ˇxbcd");
    assert_eq!(test.shape(), CursorShape::Block);
    test.keys("3 r y");
    test.assert("yyˇyd");
    // Too few characters left: nothing happens.
    test.keys("3 r z");
    test.assert("yyˇyd");
    test.keys("r enter");
    test.assert("yy\nˇd");
    // Escape cancels.
    test.keys("r");
    assert_eq!(test.shape(), CursorShape::Underline);
    test.keys("escape");
    test.assert("yy\nˇd");
}

#[gpui::test]
fn join_lines(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone\n  two\nthree\n)four");
    test.keys("J");
    test.assert("oneˇ two\nthree\n)four");
    test.keys("3 J");
    test.assert("one two threeˇ)four");

    let mut test = vim(cx, "ˇa \n  b\n\nc");
    test.keys("J");
    test.assert("a ˇb\n\nc");
    test.keys("J");
    test.assert("a ˇb\nc");
    test.keys("g J");
    test.assert("a bˇc");
    // The last line has nothing to join.
    test.keys("J");
    test.assert("a bˇc");
}

#[gpui::test]
fn toggle_case_moves_past(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇaBc d");
    test.keys("~");
    test.assert("AˇBc d");
    test.keys("2 ~");
    test.assert("AbCˇ d");
    test.keys("$ ~");
    test.assert("AbC ˇD");
}

#[gpui::test]
fn increment_numbers(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇitem 9 of 10");
    test.keys("ctrl-a");
    test.assert("item 1ˇ0 of 10");
    test.keys("5 ctrl-x");
    test.assert("item ˇ5 of 10");
    test.keys("w w ctrl-a");
    test.assert("item 5 of 1ˇ1");
    test.keys("1 2 ctrl-x");
    test.assert("item 5 of -ˇ1");
    test.keys(".");
    test.assert("item 5 of -1ˇ3");

    let mut test = vim(cx, "ˇx = 0xfe");
    test.keys("ctrl-a ctrl-a");
    test.assert("x = 0x10ˇ0");
}
