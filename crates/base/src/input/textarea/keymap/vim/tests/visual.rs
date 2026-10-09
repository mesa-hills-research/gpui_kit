//! The visual modes.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn characterwise_selection(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇhello world");
    test.keys("v");
    test.assert_mode(VimMode::Visual);
    assert_eq!(test.label(), "VISUAL");
    assert_eq!(test.shape(), CursorShape::Block);
    // The selection holds the character under the caret.
    test.assert("«hˇ»ello world");
    test.keys("e");
    test.assert("«helloˇ» world");
    test.keys("w");
    test.assert("«hello wˇ»orld");
    test.keys("o");
    test.assert("«ˇhello w»orld");
    test.keys("l");
    test.assert("h«ˇello w»orld");
    test.keys("d");
    test.assert("hˇorld");
    test.assert_mode(VimMode::Normal);
    assert_eq!(test.shape(), CursorShape::Block);

    // `$` takes the line break.
    let mut test = vim(cx, "aˇb\ncd");
    test.keys("v $ d");
    test.assert("aˇcd");

    // Backward from the anchor, the anchor's character stays selected.
    let mut test = vim(cx, "abcˇd");
    test.keys("v h h y");
    test.assert("aˇbcd");
    test.keys("P");
    test.assert("abcˇdbcd");
}

#[gpui::test]
fn linewise_selection(cx: &mut TestAppContext) {
    let mut test = vim(cx, "one\ntˇwo\nthree\nfour");
    test.keys("V");
    test.assert_mode(VimMode::VisualLine);
    assert_eq!(test.label(), "VISUAL LINE");
    test.assert("one\n«two\nˇ»three\nfour");
    test.keys("j");
    test.assert("one\n«two\nthree\nˇ»four");
    test.keys("d");
    test.assert("one\nˇfour");
    test.keys("u");
    test.assert("one\nˇtwo\nthree\nfour");

    let mut test = vim(cx, "ˇa\nb\nc");
    test.keys("V j 2 >");
    test.assert("    ˇa\n    b\nc");
    test.keys("V y G p");
    test.assert("    a\n    b\nc\n    ˇa");
}

#[gpui::test]
fn blockwise_selection(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇabcd\nefgh\nijkl");
    test.keys("ctrl-v");
    test.assert_mode(VimMode::VisualBlock);
    assert_eq!(test.label(), "VISUAL BLOCK");
    test.keys("j l");
    // One selection per row, the caret's first.
    let selections = test.textarea.read_with(&test.cx, |state, _| {
        state
            .selections
            .iter()
            .map(|selection| selection.start..selection.end)
            .collect::<Vec<_>>()
    });
    assert_eq!(selections, vec![5..7, 0..2]);
    test.keys("d");
    test.assert("ˇcd\ngh\nijkl");
    test.keys("u l ctrl-v j y");
    test.assert("aˇbcd\nefgh\nijkl");
    // A block puts back as a block.
    test.keys("$ p");
    test.assert("abcdˇb\nefghf\nijkl");
}

#[gpui::test]
fn block_insert_and_append(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇabc\nde\nfghi");
    test.keys("l ctrl-v j j I");
    test.assert_mode(VimMode::Insert);
    test.type_text("--");
    test.keys("escape");
    test.assert("aˇ--bc\nd--e\nf--ghi");

    // `A` pads the short lines to the block's edge.
    let mut test = vim(cx, "ˇabc\nd\nfghi");
    test.keys("l ctrl-v j j l A");
    test.type_text("|");
    test.keys("escape");
    test.assert("abcˇ|\nd  |\nfgh|i");

    // After `$`, `A` appends at each line's end.
    let mut test = vim(cx, "ˇab\nc\ndef");
    test.keys("ctrl-v j j $ A");
    test.type_text(";");
    test.keys("escape");
    test.assert("abˇ;\nc;\ndef;");

    let mut test = vim(cx, "ˇabc\ndef");
    test.keys("ctrl-v j l c");
    test.type_text("X");
    test.keys("escape");
    test.assert("ˇXc\nXf");
}

#[gpui::test]
fn switching_modes_and_reselecting(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇabc\ndef");
    test.keys("v l");
    test.assert("«abˇ»c\ndef");
    test.keys("V");
    test.assert_mode(VimMode::VisualLine);
    test.assert("«abc\nˇ»def");
    test.keys("v");
    test.assert_mode(VimMode::Visual);
    test.keys("escape");
    test.assert_mode(VimMode::Normal);
    test.assert("aˇbc\ndef");
    test.keys("g v");
    test.assert("«abˇ»c\ndef");
    test.keys("v");
    test.assert_mode(VimMode::Normal);
}

#[gpui::test]
fn visual_operators(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇhello world");
    test.keys("v e U");
    test.assert("ˇHELLO world");
    test.keys("v e ~");
    test.assert("ˇhello world");
    test.keys("w v $ u");
    test.assert("hello ˇworld");
    test.keys("v l l r x");
    test.assert("hello ˇxxxld");
    test.keys("0 v e c");
    test.type_text("bye");
    test.keys("escape");
    test.assert("byˇe xxxld");

    // `p` replaces the selection, and what it replaced goes to the unnamed
    // register.
    let mut test = vim(cx, "ˇfoo bar");
    test.keys("y w w v e p");
    test.assert("foo fooˇ ");
    test.keys("0 v l p");
    test.assert("baˇro foo ");

    let mut test = vim(cx, "ˇa\nb\nc");
    test.keys("V j J");
    test.assert("aˇ b\nc");
}

#[gpui::test]
fn text_objects_select_in_visual_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "foo bˇar baz");
    test.keys("v i w");
    test.assert("foo «barˇ» baz");
    test.keys("escape");

    let mut test = vim(cx, "f((a ˇb))");
    test.keys("v i (");
    test.assert("f((«a bˇ»))");
    // Again, it grows to the next pair out.
    test.keys("i (");
    test.assert("f(«(a b)ˇ»)");

    let mut test = vim(cx, "a\nˇb\n\nc");
    test.keys("v i p");
    test.assert_mode(VimMode::VisualLine);
    test.keys("d");
    test.assert("ˇ\nc");
}

#[gpui::test]
fn a_mouse_selection_starts_visual_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "ˇhello world");
    test.update(|state, _, cx| state.set_selected_range(0..5, cx));
    test.keys("d");
    test.assert("ˇ world");
    test.assert_mode(VimMode::Normal);

    // A click while in visual mode leaves it.
    let mut test = vim(cx, "ˇhello world");
    test.keys("v l");
    test.update(|state, _, cx| state.set_selected_range(8..8, cx));
    test.keys("x");
    test.assert_mode(VimMode::Normal);
    test.assert("hello woˇld");
}
