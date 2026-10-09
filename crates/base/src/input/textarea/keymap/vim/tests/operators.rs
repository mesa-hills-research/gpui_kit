//! Operators with motions, doubled for lines, and with counts.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn delete_by_word(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone two three\nfour");
    test.keys("d w");
    test.assert("ˇtwo three\nfour");
    // The last word on a line stops at the line's end.
    test.keys("d 2 w");
    test.assert("ˇ\nfour");
    // On an empty line, `dw` deletes the line.
    test.keys("d w");
    test.assert("ˇfour");

    let mut test = vim(cx, "foo ˇbar\nbaz");
    test.keys("d w");
    test.assert("fooˇ \nbaz");

    let mut test = vim(cx, "ˇfoo bar");
    test.keys("d e");
    test.assert("ˇ bar");
    let mut test = vim(cx, "foo bˇar");
    test.keys("d b");
    test.assert("foo ˇar");

    // The operator's count and the motion's multiply.
    let mut test = vim(cx, "ˇa b c d e f g h");
    test.keys("2 d 3 w");
    test.assert("ˇg h");
}

#[gpui::test]
fn delete_by_line_and_to_line_ends(cx: &mut TestAppContext) {
    let mut test = vim(cx, "fooˇ bar");
    test.keys("D");
    test.assert("foˇo");

    let mut test = vim(cx, "foo ˇbar");
    test.keys("d 0");
    test.assert("ˇbar");

    let mut test = vim(cx, "  foo ˇbar");
    test.keys("d ^");
    test.assert("  ˇbar");

    let mut test = vim(cx, "oˇne\ntwo\nthree");
    test.keys("d j");
    test.assert("ˇthree");

    let mut test = vim(cx, "one\ntwo\nthrˇee");
    test.keys("d k");
    test.assert("ˇone");

    let mut test = vim(cx, "one\nˇtwo\nthree");
    test.keys("d G");
    test.assert("ˇone");

    let mut test = vim(cx, "one\ntwo\nthrˇee");
    test.keys("d g g");
    test.assert("ˇ");

    let mut test = vim(cx, "one\n  ˇtwo\nthree\nfour");
    test.keys("d d");
    test.assert("one\nˇthree\nfour");
    test.keys("2 d d");
    test.assert("ˇone");
    // More lines than there are: nothing happens on the last line.
    test.keys("3 d d");
    test.assert("ˇone");
    test.keys("d d");
    test.assert("ˇ");
}

#[gpui::test]
fn delete_to_paragraphs_finds_and_brackets(cx: &mut TestAppContext) {
    // From a line's start, `d}` takes whole lines.
    let mut test = vim(cx, "ˇa b\nc\n\nd");
    test.keys("d }");
    test.assert("ˇ\nd");
    // From within a line, up to the end of the paragraph's last line.
    let mut test = vim(cx, "a ˇb\nc\n\nd");
    test.keys("d }");
    test.assert("aˇ \n\nd");

    let mut test = vim(cx, "f(aˇbc)");
    test.keys("d t )");
    test.assert("f(aˇ)");
    let mut test = vim(cx, "ˇa, b");
    test.keys("d f ,");
    test.assert("ˇ b");
    let mut test = vim(cx, "ˇ(a) b");
    test.keys("d %");
    test.assert("ˇ b");
    let mut test = vim(cx, "ˇfoo bar foo");
    test.keys("d /");
    test.type_text("ar");
    test.keys("enter");
    test.assert("ˇar foo");
}

#[gpui::test]
fn change(cx: &mut TestAppContext) {
    // `cw` on a word changes to its end.
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("c w");
    test.assert_mode(VimMode::Insert);
    test.type_text("baz");
    test.keys("escape");
    test.assert("baˇz bar");
    test.assert_mode(VimMode::Normal);

    // On blanks, it changes them up to the next word.
    let mut test = vim(cx, "fooˇ   bar");
    test.keys("c w");
    test.type_text("-");
    test.keys("escape");
    test.assert("fooˇ-bar");

    let mut test = vim(cx, "fooˇ bar");
    test.keys("C");
    test.type_text("!");
    test.keys("escape");
    test.assert("fooˇ!");

    // `cc` and `S` keep the line's indentation.
    let mut test = vim(cx, "  ˇfoo\nbar");
    test.keys("c c");
    test.type_text("x");
    test.keys("escape");
    test.assert("  ˇx\nbar");
    test.keys("S");
    test.type_text("y");
    test.keys("escape");
    test.assert("  ˇy\nbar");

    let mut test = vim(cx, "ˇabc");
    test.keys("s");
    test.type_text("X");
    test.keys("escape");
    test.assert("ˇXbc");
    test.keys("2 s");
    test.type_text("Y");
    test.keys("escape");
    test.assert("ˇYc");

    // `cl` on an empty line still starts insert mode.
    let mut test = vim(cx, "a\nˇ\nb");
    test.keys("s");
    test.assert_mode(VimMode::Insert);
    test.type_text("x");
    test.keys("escape");
    test.assert("a\nˇx\nb");
}

#[gpui::test]
fn yank_and_put(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("y w");
    test.assert("ˇfoo bar");
    test.keys("P");
    test.assert("fooˇ foo bar");
    test.keys("u");
    test.assert("ˇfoo bar");
    test.keys("p");
    test.assert("ffooˇ oo bar");

    let mut test = vim(cx, "ˇone\ntwo");
    test.keys("y y p");
    test.assert("one\nˇone\ntwo");
    test.keys("j Y P");
    test.assert("one\none\nˇtwo\ntwo");
    test.keys("G 2 p");
    test.assert("one\none\ntwo\ntwo\nˇtwo\ntwo");

    // A yank leaves the caret at the start of the text it took.
    let mut test = vim(cx, "one tˇwo");
    test.keys("y b");
    test.assert("one ˇtwo");
    let mut test = vim(cx, "one\ntwˇo");
    test.keys("y k");
    test.assert("onˇe\ntwo");
}

#[gpui::test]
fn shift_lines(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa\nb\nc");
    test.keys("> >");
    test.assert("  ˇa\nb\nc");
    test.keys("< <");
    test.assert("ˇa\nb\nc");
    test.keys("> j");
    test.assert("  ˇa\n  b\nc");
    test.keys("3 > >");
    test.assert("    ˇa\n    b\n  c");
    test.keys("< G");
    test.assert("  ˇa\n  b\nc");
    // Empty lines stay empty.
    let mut test = vim(cx, "ˇa\n\nb");
    test.keys("> G");
    test.assert("  ˇa\n\n  b");
}

#[gpui::test]
fn change_case(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("g ~ w");
    test.assert("ˇFOO bar");
    test.keys("g u u");
    test.assert("ˇfoo bar");
    test.keys("w g U i w");
    test.assert("foo ˇBAR");
    test.keys("g U g U");
    test.assert("FOO ˇBAR");
    test.keys("g u $");
    test.assert("FOO ˇbar");
    test.keys("0 ~");
    test.assert("fˇOO bar");
    test.keys("5 ~");
    test.assert("foo BAˇr");
}

#[gpui::test]
fn operator_pending_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("d");
    test.assert_mode(VimMode::OperatorPending);
    assert_eq!(test.label(), "O-PENDING");
    assert_eq!(test.shape(), CursorShape::Underline);
    test.keys("escape");
    test.assert_mode(VimMode::Normal);
    test.keys("w");
    test.assert("foo ˇbar");
    // A key that means nothing cancels the operator.
    test.keys("d q");
    test.assert_mode(VimMode::Normal);
    test.assert("foo ˇbar");
    // An operator that cannot move does nothing.
    test.keys("d f z");
    test.assert("foo ˇbar");
    test.assert_mode(VimMode::Normal);
}
