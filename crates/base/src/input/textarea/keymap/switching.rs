//! Switching a textarea's scheme while it is in use. Each switch starts the
//! new scheme afresh: Vim in normal mode, Emacs with no mark and no prefix
//! argument, and the caret back to a bar for CUA and Emacs. The selection and
//! the undo history stay with the text.

use gpui::{KeyContext, TestAppContext};

use super::test::KeymapTest;
use super::*;

/// The textarea's key context, as its element sets it.
fn key_context(test: &KeymapTest) -> KeyContext {
    test.textarea.read_with(&test.cx, |state, _| {
        let mut context = KeyContext::default();
        context.add("Input");
        state.extras.keymap.key_context(&mut context);
        context
    })
}

/// Where the scheme draws its own caret, if it does.
fn caret_offset(test: &KeymapTest) -> Option<usize> {
    test.textarea
        .read_with(&test.cx, |state, _| state.extras.keymap.caret_offset())
}

fn vim_mode(test: &KeymapTest) -> Option<VimMode> {
    test.textarea.read_with(&test.cx, |state, _| {
        state.keymap_state::<VimState>().map(VimState::mode)
    })
}

fn emacs_mark(test: &KeymapTest) -> Option<usize> {
    test.textarea.read_with(&test.cx, |state, _| {
        state
            .keymap_state::<EmacsState>()
            .and_then(EmacsState::mark)
    })
}

fn switch(test: &mut KeymapTest, keymap: Keymap) {
    test.update(|state, _, cx| state.set_keymap(keymap, cx));
}

/// After switching to CUA or Emacs: a bar caret at the selection's end, no
/// mode label, and none of Vim's key-context entries.
#[track_caller]
fn assert_modeless(test: &KeymapTest, keymap: Keymap) {
    let current = test
        .textarea
        .read_with(&test.cx, |state, _| state.current_keymap());
    assert_eq!(current, keymap);
    assert_eq!(test.mode_label(), None);
    assert_eq!(test.cursor_shape(), CursorShape::Bar);
    assert_eq!(caret_offset(test), None);
    assert_eq!(vim_mode(test), None);
    let context = key_context(test);
    assert_eq!(
        context.get("keymap").map(|name| name.as_ref()),
        Some(keymap.name())
    );
    for entry in ["vim_mode", "vim_visual", "vim_operator", "vim_count"] {
        assert!(!context.contains(entry), "{entry} in {context:?}");
    }
}

/// Text typed in CUA, then Vim: normal mode on the last character typed,
/// letters as commands, and the typing one step for `u`.
#[gpui::test]
fn cua_to_vim_mid_insert(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, "one\ntwoˇ");
    test.type_text(" th");
    test.type_text("ree");
    test.assert("one\ntwo threeˇ");

    switch(&mut test, Keymap::Vim);
    assert_eq!(vim_mode(&test), Some(VimMode::Normal));
    assert_eq!(test.mode_label().as_deref(), Some("NORMAL"));
    assert_eq!(test.cursor_shape(), CursorShape::Block);
    assert_eq!(
        key_context(&test).get("vim_mode").map(|mode| mode.as_ref()),
        Some("normal")
    );
    test.assert("one\ntwo threˇe");
    test.type_text("q");
    test.assert("one\ntwo threˇe");
    test.keys("x");
    test.assert("one\ntwo thrˇe");
    test.keys("u");
    test.assert("one\ntwo threˇe");
    test.keys("u");
    assert_eq!(test.marked().replace('ˇ', ""), "one\ntwo");
    test.keys("i");
    assert_eq!(test.cursor_shape(), CursorShape::Bar);

    // Back to CUA in the middle of Vim's insert.
    switch(&mut test, Keymap::Cua);
    assert_modeless(&test, Keymap::Cua);
    test.keys("ctrl-end");
    test.type_text("!");
    test.assert("one\ntwo!ˇ");
    test.keys("ctrl-z");
    test.assert("one\ntwoˇ");
}

/// A visual selection carried into Emacs is a region Emacs's keys act on,
/// with Emacs's caret and Emacs's mark, and Vim forgets it when it comes
/// back.
#[gpui::test]
fn vim_visual_to_emacs(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::Linux, "ˇone two\nthree");
    test.keys("v e");
    assert_eq!(vim_mode(&test), Some(VimMode::Visual));
    assert_eq!(caret_offset(&test), Some(2));
    test.assert("«oneˇ» two\nthree");

    switch(&mut test, Keymap::Emacs);
    assert_modeless(&test, Keymap::Emacs);
    assert_eq!(emacs_mark(&test), None);
    test.assert("«oneˇ» two\nthree");
    test.keys("ctrl-w");
    test.assert("ˇ two\nthree");
    test.type_text("1");
    test.keys("ctrl-n ctrl-a");
    test.assert("1 two\nˇthree");
    // The mark follows an edit before it.
    test.keys("ctrl-space ctrl-g ctrl-p ctrl-a");
    test.type_text("0");
    assert_eq!(emacs_mark(&test), Some(7));
    test.keys("ctrl-x ctrl-x ctrl-g");
    test.assert("01 two\nˇthree");

    // Vim starts in normal mode, with no visual selection to bring back.
    switch(&mut test, Keymap::Vim);
    assert_eq!(vim_mode(&test), Some(VimMode::Normal));
    assert_eq!(caret_offset(&test), None);
    test.keys("g v");
    assert_eq!(vim_mode(&test), Some(VimMode::Normal));
    test.assert("01 two\nˇthree");
}

/// The region of an active mark stays a selection in CUA, and Emacs comes
/// back without the mark or the prefix argument.
#[gpui::test]
fn emacs_with_active_mark_to_cua(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::Linux, "ˇone two three");
    test.keys("ctrl-space alt-f ctrl-u");
    assert_eq!(emacs_mark(&test), Some(0));
    assert_eq!(test.mode_label().as_deref(), Some("C-u 4"));
    test.assert("«oneˇ» two three");

    switch(&mut test, Keymap::Cua);
    assert_modeless(&test, Keymap::Cua);
    test.assert("«oneˇ» two three");
    test.keys("shift-right");
    test.assert("«one ˇ»two three");
    test.keys("right ctrl-right");
    test.assert("one twoˇ three");

    switch(&mut test, Keymap::Emacs);
    assert_modeless(&test, Keymap::Emacs);
    assert_eq!(emacs_mark(&test), None);
    test.keys("ctrl-f");
    test.assert("one two ˇthree");
    test.keys("ctrl-x ctrl-x");
    test.assert("one two ˇthree");
}

/// CUA's own commands are registered on every input, but only CUA's
/// bindings reach them: with Emacs or Vim in any of its modes, no key of
/// CUA's table on any platform resolves to a CUA binding.
#[gpui::test]
fn no_cua_binding_resolves_in_emacs_or_vim(cx: &mut TestAppContext) {
    let mut contexts = Vec::new();
    let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::Linux, "ˇone");
    contexts.push(key_context(&test));
    test.keys("ctrl-space ctrl-u");
    contexts.push(key_context(&test));
    for keys in ["", "i", "R", "v", "shift-v", "ctrl-v", "d", "2", "f", ":"] {
        let mut test = KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::Linux, "ˇone two");
        if !keys.is_empty() {
            test.keys(keys);
        }
        contexts.push(key_context(&test));
    }

    for platform in KeymapPlatform::ALL {
        let all: Vec<KeyBinding> = Keymap::ALL
            .iter()
            .flat_map(|keymap| keymap.bindings(platform))
            .collect();
        let keymap = gpui::Keymap::new(all);
        for binding in Keymap::Cua.bindings(platform) {
            let keystrokes = binding.keystrokes();
            for context in &contexts {
                let (resolved, _) = keymap.bindings_for_input(keystrokes, &[context.clone()]);
                for found in resolved {
                    let predicate = found.predicate().map(|predicate| predicate.to_string());
                    assert!(
                        !predicate.is_some_and(|predicate| predicate.contains("keymap == cua"))
                            && !found.action().name().starts_with("cua::"),
                        "{platform:?}: {keystrokes:?} in {context:?} reaches {}",
                        found.action().name()
                    );
                }
            }
        }
    }

    // On macOS, where CUA binds Cocoa's Control keys, Emacs and Vim keep
    // theirs: C-k kills into Emacs's ring, and Ctrl-V starts a block.
    let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::MacOS, "heˇllo");
    test.keys("ctrl-k");
    test.assert("heˇ");
    let ring = test.textarea.read_with(&test.cx, |state, _| {
        state
            .keymap_state::<EmacsState>()
            .map(|emacs| emacs.kill_ring().map(String::from).collect::<Vec<_>>())
    });
    assert_eq!(ring, Some(vec!["llo".to_string()]));
    let mut test = KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::MacOS, "ˇab\ncd");
    test.keys("ctrl-v");
    assert_eq!(vim_mode(&test), Some(VimMode::VisualBlock));
}
