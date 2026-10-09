//! Text objects after operators.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn word_objects(cx: &mut TestAppContext) {
    let mut test = vim(cx, "foo bˇar baz");
    test.keys("d i w");
    test.assert("foo ˇ baz");

    let mut test = vim(cx, "foo bˇar baz");
    test.keys("d a w");
    test.assert("foo ˇbaz");

    // With no blanks after it, `aw` takes the ones before.
    let mut test = vim(cx, "foo bˇar");
    test.keys("d a w");
    test.assert("foˇo");

    let mut test = vim(cx, "foo bˇar baz");
    test.keys("c i w");
    test.type_text("X");
    test.keys("escape");
    test.assert("foo ˇX baz");

    let mut test = vim(cx, "a bˇ.c d");
    test.keys("d i W");
    test.assert("a ˇ d");
    let mut test = vim(cx, "a bˇ.c d");
    test.keys("d a W");
    test.assert("a ˇd");
    let mut test = vim(cx, "a bˇ.c d");
    test.keys("d i w");
    test.assert("a bˇc d");
}

#[gpui::test]
fn quote_objects(cx: &mut TestAppContext) {
    let mut test = vim(cx, "say \"heˇllo world\" now");
    test.keys("d i \"");
    test.assert("say \"ˇ\" now");

    let mut test = vim(cx, "say \"heˇllo world\" now");
    test.keys("d a \"");
    test.assert("say ˇnow");

    // Before the quotes, the pair after the caret.
    let mut test = vim(cx, "ˇsay 'hi'");
    test.keys("c i '");
    test.type_text("yo");
    test.keys("escape");
    test.assert("say 'yˇo'");

    let mut test = vim(cx, "a `bˇc` d");
    test.keys("y i ` P");
    test.assert("a `bˇcbc` d");
}

#[gpui::test]
fn bracket_objects(cx: &mut TestAppContext) {
    let mut test = vim(cx, "f(a, ˇb)");
    test.keys("d i (");
    test.assert("f(ˇ)");

    let mut test = vim(cx, "f(a, ˇb)");
    test.keys("d a b");
    test.assert("ˇf");

    // A count reaches the pairs further out.
    let mut test = vim(cx, "((a ˇb) c)");
    test.keys("d 2 i )");
    test.assert("(ˇ)");

    // The caret on a bracket uses its pair.
    let mut test = vim(cx, "x ˇ[1, [2]]");
    test.keys("d i ]");
    test.assert("x [ˇ]");

    let mut test = vim(cx, "<ˇdiv>");
    test.keys("d i <");
    test.assert("<ˇ>");

    // Braces on lines of their own: the lines between them.
    let mut test = vim(cx, "fn x() {\n    ˇa;\n    b;\n}");
    test.keys("d i {");
    test.assert("fn x() {\nˇ}");

    let mut test = vim(cx, "fn x() {\n    ˇa;\n    b;\n}");
    test.keys("c i B");
    test.type_text("z");
    test.keys("escape");
    test.assert("fn x() {\n    ˇz\n}");

    let mut test = vim(cx, "a {ˇ b } c");
    test.keys("d a }");
    test.assert("a ˇ c");

    // Outside any pair there is nothing to take.
    let mut test = vim(cx, "ˇno brackets");
    test.keys("d i (");
    test.assert("ˇno brackets");
}

#[gpui::test]
fn tag_objects(cx: &mut TestAppContext) {
    let mut test = vim(cx, "<a><b class=\"x\">ˇxy</b></a>");
    test.keys("d i t");
    test.assert("<a><b class=\"x\">ˇ</b></a>");

    let mut test = vim(cx, "<a><b>ˇxy</b></a>");
    test.keys("d a t");
    test.assert("<a>ˇ</a>");

    let mut test = vim(cx, "<a><b>ˇxy</b><br/></a>");
    test.keys("d 2 i t");
    test.assert("<a>ˇ</a>");
}

#[gpui::test]
fn paragraph_objects(cx: &mut TestAppContext) {
    let mut test = vim(cx, "a\nˇb\n\nc");
    test.keys("d i p");
    test.assert("ˇ\nc");

    let mut test = vim(cx, "a\nˇb\n\nc");
    test.keys("d a p");
    test.assert("ˇc");

    // The last paragraph takes the blank lines before it.
    let mut test = vim(cx, "a\n\nˇc\nd");
    test.keys("d a p");
    test.assert("ˇa");

    let mut test = vim(cx, "a\nˇb\n\nc");
    test.keys("y i p G p");
    test.assert("a\nb\n\nc\nˇa\nb");
}
