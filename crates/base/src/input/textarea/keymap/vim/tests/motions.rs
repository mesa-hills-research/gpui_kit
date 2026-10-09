//! Motions in normal mode: counts, line ends, empty lines and Unicode.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn h_and_l_stay_on_the_line(cx: &mut TestAppContext) {
    let mut test = vim(cx, "abˇc\ndef");
    test.keys("l");
    test.assert("abˇc\ndef");
    test.keys("h h h");
    test.assert("ˇabc\ndef");
    test.keys("2 l");
    test.assert("abˇc\ndef");
    test.keys("5 h");
    test.assert("ˇabc\ndef");
    // Space and Backspace wrap to the next and previous line.
    test.keys("$ space");
    test.assert("abc\nˇdef");
    test.keys("backspace");
    test.assert("abˇc\ndef");
}

#[gpui::test]
fn j_and_k_keep_the_column(cx: &mut TestAppContext) {
    let mut test = vim(cx, "abcˇdef\nxy\n\nabcdefgh");
    test.keys("j");
    test.assert("abcdef\nxˇy\n\nabcdefgh");
    test.keys("j");
    test.assert("abcdef\nxy\nˇ\nabcdefgh");
    test.keys("j");
    test.assert("abcdef\nxy\n\nabcˇdefgh");
    test.keys("3 k");
    test.assert("abcˇdef\nxy\n\nabcdefgh");
    // Past the last line, j moves as far as there is.
    test.keys("9 j");
    test.assert("abcdef\nxy\n\nabcˇdefgh");
    // After `$`, j and k aim for the end of each line.
    test.keys("g g $ j");
    test.assert("abcdef\nxˇy\n\nabcdefgh");
    test.keys("j j");
    test.assert("abcdef\nxy\n\nabcdefgˇh");
}

#[gpui::test]
fn word_motions_follow_vims_classes(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo.bar  baz\nqux");
    test.keys("w");
    test.assert("fooˇ.bar  baz\nqux");
    test.keys("w");
    test.assert("foo.ˇbar  baz\nqux");
    test.keys("w");
    test.assert("foo.bar  ˇbaz\nqux");
    test.keys("w");
    test.assert("foo.bar  baz\nˇqux");
    test.keys("b b");
    test.assert("foo.ˇbar  baz\nqux");
    test.keys("g g W");
    test.assert("foo.bar  ˇbaz\nqux");
    test.keys("B");
    test.assert("ˇfoo.bar  baz\nqux");
    test.keys("e");
    test.assert("foˇo.bar  baz\nqux");
    test.keys("e");
    test.assert("fooˇ.bar  baz\nqux");
    test.keys("e e");
    test.assert("foo.bar  baˇz\nqux");
    test.keys("g e");
    test.assert("foo.baˇr  baz\nqux");
    test.keys("g g E");
    test.assert("foo.baˇr  baz\nqux");
}

#[gpui::test]
fn word_motions_take_counts_and_stop_at_empty_lines(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone two three four");
    test.keys("3 w");
    test.assert("one two three ˇfour");
    test.keys("2 b");
    test.assert("one ˇtwo three four");
    // `w` on the last word goes to its last character.
    test.keys("$ w");
    test.assert("one two three fouˇr");

    let mut test = vim(cx, "ˇfoo\n\n  bar");
    test.keys("w");
    test.assert("foo\nˇ\n  bar");
    test.keys("w");
    test.assert("foo\n\n  ˇbar");
    test.keys("b");
    test.assert("foo\nˇ\n  bar");
}

#[gpui::test]
fn word_motions_handle_unicode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇhéllo wörld");
    test.keys("w");
    test.assert("héllo ˇwörld");
    test.keys("e");
    test.assert("héllo wörlˇd");
    test.keys("b");
    test.assert("héllo ˇwörld");
    test.keys("0 l l");
    test.assert("héˇllo wörld");

    // Japanese and Latin letters are words of their own.
    let mut test = vim(cx, "ˇ日本語text more");
    test.keys("w");
    test.assert("日本語ˇtext more");
    test.keys("w");
    test.assert("日本語text ˇmore");
    test.keys("0 e");
    test.assert("日本ˇ語text more");
}

#[gpui::test]
fn line_motions(cx: &mut TestAppContext) {
    let mut test = vim(cx, "  ˇfoo bar\nx");
    test.keys("0");
    test.assert("ˇ  foo bar\nx");
    test.keys("^");
    test.assert("  ˇfoo bar\nx");
    test.keys("$");
    test.assert("  foo baˇr\nx");
    test.keys("0 2 $");
    test.assert("  foo bar\nˇx");
    test.keys("-");
    test.assert("  ˇfoo bar\nx");
    test.keys("+");
    test.assert("  foo bar\nˇx");
    test.keys("k g _");
    test.assert("  foo baˇr\nx");
    test.keys("home");
    test.assert("ˇ  foo bar\nx");
    test.keys("end");
    test.assert("  foo baˇr\nx");
}

#[gpui::test]
fn file_motions(cx: &mut TestAppContext) {
    let mut test = vim(cx, "one\ntwo\nthrˇee\n  four");
    test.keys("g g");
    test.assert("ˇone\ntwo\nthree\n  four");
    test.keys("G");
    test.assert("one\ntwo\nthree\n  ˇfour");
    test.keys("2 G");
    test.assert("one\nˇtwo\nthree\n  four");
    test.keys("3 g g");
    test.assert("one\ntwo\nˇthree\n  four");
    test.keys("5 0 %");
    test.assert("one\nˇtwo\nthree\n  four");
}

#[gpui::test]
fn find_motions_and_their_repeats(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇa,b,c,d");
    test.keys("f ,");
    test.assert("aˇ,b,c,d");
    test.keys(";");
    test.assert("a,bˇ,c,d");
    test.keys(",");
    test.assert("aˇ,b,c,d");
    test.keys("2 f ,");
    test.assert("a,b,cˇ,d");
    test.keys("F b");
    test.assert("a,ˇb,c,d");
    test.keys("f z");
    test.assert("a,ˇb,c,d");

    // `t` stops before the character, and `;` moves on past a match right
    // after the caret.
    let mut test = vim(cx, "ˇa,b,c,b");
    test.keys("t b");
    test.assert("aˇ,b,c,b");
    test.keys(";");
    test.assert("a,b,cˇ,b");
    test.keys("T a");
    test.assert("aˇ,b,c,b");
}

#[gpui::test]
fn bracket_and_paragraph_motions(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇif (a[1]) {x}");
    test.keys("%");
    test.assert("if (a[1]ˇ) {x}");
    test.keys("%");
    test.assert("if ˇ(a[1]) {x}");
    test.keys("l %");
    test.assert("if (a[1ˇ]) {x}");

    let mut test = vim(cx, "ˇa\nb\n\nc\nd\n\ne");
    test.keys("}");
    test.assert("a\nb\nˇ\nc\nd\n\ne");
    test.keys("}");
    test.assert("a\nb\n\nc\nd\nˇ\ne");
    test.keys("}");
    test.assert("a\nb\n\nc\nd\n\nˇe");
    test.keys("{");
    test.assert("a\nb\n\nc\nd\nˇ\ne");
    test.keys("2 {");
    test.assert("ˇa\nb\n\nc\nd\n\ne");
}

#[gpui::test]
fn search_motions(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇfoo bar foo baz foo");
    test.keys("/");
    assert_eq!(test.label(), "/");
    test.type_text("foo");
    assert_eq!(test.label(), "/foo");
    test.keys("enter");
    test.assert("foo bar ˇfoo baz foo");
    assert_eq!(test.label(), "NORMAL");
    test.keys("n");
    test.assert("foo bar foo baz ˇfoo");
    test.keys("n");
    test.assert("ˇfoo bar foo baz foo");
    test.keys("N");
    test.assert("foo bar foo baz ˇfoo");
    test.keys("?");
    test.type_text("ba");
    test.keys("enter");
    test.assert("foo bar foo ˇbaz foo");
    test.keys("n");
    test.assert("foo ˇbar foo baz foo");
    // An empty search repeats the last pattern.
    test.keys("/ enter");
    test.assert("foo bar foo ˇbaz foo");

    // `*` and `#` search for the word under the caret, whole.
    let mut test = vim(cx, "ˇfoo food foo");
    test.keys("*");
    test.assert("foo food ˇfoo");
    test.keys("#");
    test.assert("ˇfoo food foo");
    test.keys("w *");
    test.assert("foo ˇfood foo");

    // Matches of a plain pattern are highlighted with the textarea's search.
    let query = test.textarea.read_with(&test.cx, |state, _| {
        (
            state.search_session().is_active(),
            state.search_session().query.clone(),
        )
    });
    assert_eq!(query, (true, "food".to_string()));
}

#[gpui::test]
fn search_patterns_are_vims(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇone Two three tWo");
    test.keys("/");
    test.type_text("t\\(wo\\|hree\\)");
    test.keys("enter");
    test.assert("one Two ˇthree tWo");
    test.keys("/");
    test.type_text("\\ctwo");
    test.keys("enter");
    test.assert("one Two three ˇtWo");
    test.keys("n");
    test.assert("one ˇTwo three tWo");
}

#[gpui::test]
fn window_motions_use_the_visible_lines(cx: &mut TestAppContext) {
    let text: String = (0..60).map(|ix| format!("line {ix}\n")).collect();
    let mut test = vim(cx, &format!("ˇ{text}"));
    test.update(|state, _, cx| state.set_rows(10, cx));
    let row = |test: &KeymapTest| {
        test.textarea
            .read_with(&test.cx, |state, _| state.cursor_position().line)
    };
    test.keys("L");
    let bottom = row(&test);
    assert!(bottom >= 5 && bottom < 59, "L went to line {bottom}");
    test.keys("M");
    let middle = row(&test);
    assert!(middle > 0 && middle < bottom, "M went to line {middle}");
    test.keys("H");
    assert_eq!(row(&test), 0);
    test.keys("ctrl-d");
    let half = row(&test);
    assert!(half > 0 && half <= bottom, "Ctrl-D went to line {half}");
    test.keys("ctrl-u");
    assert_eq!(row(&test), 0);
}
