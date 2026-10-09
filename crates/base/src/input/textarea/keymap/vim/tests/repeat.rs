//! `.` and undo.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn dot_repeats_operators_with_their_counts(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa b c d e f");
    test.keys("d w .");
    test.assert("ˇc d e f");
    // A new count replaces the old one.
    test.keys("2 .");
    test.assert("ˇe f");

    let mut test = vim(cx, "ˇabcdef");
    test.keys("2 x .");
    test.assert("ˇef");

    let mut test = vim(cx, "ˇa\nb\nc\nd");
    test.keys("> > j .");
    test.assert("  a\n  ˇb\nc\nd");
    test.keys("j 2 d d .");
    test.assert("  a\n  ˇb");
}

#[gpui::test]
fn dot_repeats_changes_with_the_text_typed(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo foo foo");
    test.keys("c w");
    test.type_text("bar");
    test.keys("escape w .");
    test.assert("bar baˇr foo");
    test.keys("w .");
    test.assert("bar bar baˇr");

    let mut test = vim(cx, "ˇa\nb");
    test.keys("A");
    test.type_text("!");
    test.keys("escape j .");
    test.assert("a!\nbˇ!");

    let mut test = vim(cx, "x ˇfoo y foo");
    test.keys("c i w");
    test.type_text("z");
    test.keys("escape f f .");
    test.assert("x z y ˇz");
}

#[gpui::test]
fn dot_repeats_inserts_and_their_counts(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇ-");
    test.keys("3 i");
    test.type_text("ab");
    test.keys("escape");
    test.assert("ababaˇb-");
    test.keys(".");
    test.assert("ababaababaˇbb-");

    let mut test = vim(cx, "ˇx");
    test.keys("2 o");
    test.type_text("y");
    test.keys("escape");
    test.assert("x\ny\nˇy");
    test.keys("O");
    test.type_text("z");
    test.keys("escape .");
    test.assert("x\ny\nˇz\nz\ny");

    // `a` types after the caret's character again.
    let mut test = vim(cx, "ˇab");
    test.keys("a");
    test.type_text("1");
    test.keys("escape");
    test.keys("$ .");
    test.assert("a1bˇ1");
}

#[gpui::test]
fn dot_repeats_visual_operations_on_as_much_text(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇabcdef");
    test.keys("v l d");
    test.assert("ˇcdef");
    test.keys(".");
    test.assert("ˇef");

    let mut test = vim(cx, "ˇa\nb\nc\nd\ne");
    test.keys("V j >");
    test.assert("  ˇa\n  b\nc\nd\ne");
    test.keys("j j .");
    test.assert("  a\n  b\n  ˇc\n  d\ne");
}

#[gpui::test]
fn an_insert_is_one_undo_step(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("c w");
    test.type_text("ba");
    test.keys("left");
    test.type_text("z");
    test.keys("escape");
    test.assert("bˇza bar");
    test.keys("u");
    test.assert("ˇfoo bar");
    test.keys("ctrl-r");
    test.assert("ˇbza bar");

    let mut test = vim(cx, "ˇa");
    test.keys("o");
    test.type_text("one");
    test.keys("enter");
    test.type_text("two");
    test.keys("backspace escape");
    test.assert("a\none\ntˇw");
    test.keys("u");
    test.assert("ˇa");
}

#[gpui::test]
fn undo_takes_counts_and_leaves_normal_mode_on_a_character(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa b c d");
    test.keys("d w d w d w");
    test.assert("ˇd");
    test.keys("2 u");
    test.assert("ˇb c d");
    test.keys("u");
    test.assert("ˇa b c d");
    test.keys("u");
    test.assert("ˇa b c d");
    test.keys("3 ctrl-r");
    test.assert("ˇd");

    // Undoing a visual change leaves no selection behind.
    let mut test = vim(cx, "ˇabc def");
    test.keys("v e d u");
    test.assert("ˇabc def");
    test.assert_mode(VimMode::Normal);
}
