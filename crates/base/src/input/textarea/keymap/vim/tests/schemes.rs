//! Vim among the other schemes and on each platform: the platform's
//! shortcuts that stay, the context menu, and the keys that apply only while
//! Vim does.

use std::{cell::RefCell, rc::Rc};

use gpui::{ClipboardItem, TestAppContext};

use super::*;

/// Whether `platform`'s Vim table binds `keys` in some context.
fn binds(platform: KeymapPlatform, keys: &str) -> bool {
    let keys = gpui::Keystroke::parse(keys).unwrap();
    Keymap::Vim
        .bindings(platform)
        .iter()
        .any(|binding| binding.keystrokes()[0].inner() == &keys)
}

#[test]
fn vims_keys_are_the_same_on_every_platform() {
    for platform in KeymapPlatform::ALL {
        for keys in [
            "ctrl-v", "ctrl-r", "ctrl-w", "ctrl-u", "ctrl-d", "ctrl-a", "ctrl-x",
        ] {
            assert!(binds(platform, keys), "{platform:?} binds {keys}");
        }
        for keys in ["ctrl-s", "ctrl-z", "ctrl-o", "cmd-s"] {
            assert!(
                !binds(platform, keys),
                "{platform:?} leaves {keys} to the app"
            );
        }
    }
    assert!(binds(KeymapPlatform::MacOS, "cmd-c"));
    assert!(binds(KeymapPlatform::MacOS, "cmd-z"));
    assert!(!binds(KeymapPlatform::MacOS, "ctrl-shift-c"));
    for platform in [KeymapPlatform::Windows, KeymapPlatform::Linux] {
        assert!(binds(platform, "ctrl-shift-c"));
        assert!(binds(platform, "ctrl-shift-v"));
        assert!(!binds(platform, "cmd-c"));
    }
}

#[gpui::test]
fn vims_keys_apply_only_while_vim_is_the_scheme(cx: &mut TestAppContext) {
    for keymap in [Keymap::Cua, Keymap::Emacs] {
        let mut test = KeymapTest::new(cx, keymap, KeymapPlatform::Linux, "ˇabc");
        test.keys("d d x");
        test.assert("ddxˇabc");
        test.keys("escape");
        test.assert("ddxˇabc");
        assert_eq!(test.mode_label(), None);
    }

    // Switching to Vim starts in normal mode, and away from it ends it.
    let mut test = KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, "abˇc");
    test.update(|state, _, cx| state.set_keymap(Keymap::Vim, cx));
    test.assert_mode(VimMode::Normal);
    test.keys("x");
    test.assert("aˇb");
    // A selection made in visual mode stays a selection.
    test.keys("v");
    test.update(|state, _, cx| state.set_keymap(Keymap::Cua, cx));
    test.keys("x");
    test.assert("axˇ");
}

#[gpui::test]
fn ctrl_v_starts_a_block_on_every_platform(cx: &mut TestAppContext) {
    for platform in KeymapPlatform::ALL {
        let mut test = vim_on(cx, platform, "ˇab\ncd");
        test.keys("ctrl-v j d");
        test.assert("ˇb\nd");
        test.keys("u ctrl-r");
        test.assert("ˇb\nd");
    }
}

#[gpui::test]
fn macos_keeps_the_cmd_shortcuts(cx: &mut TestAppContext) {
    let mut test = vim_on(cx, KeymapPlatform::MacOS, "ˇhello world");
    test.keys("v e cmd-c");
    test.assert_mode(VimMode::Normal);
    let copied = test.cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("hello"));
    test.keys("$ cmd-v");
    test.assert("hello worlhellˇod");
    test.keys("cmd-z");
    test.assert("hello worlˇd");
    test.assert_mode(VimMode::Normal);
    test.keys("cmd-a");
    test.assert_mode(VimMode::Visual);
    test.keys("cmd-x");
    test.assert("ˇ");

    // In insert mode they act as anywhere else.
    test.keys("i cmd-v");
    test.assert("hello worldˇ");
    test.keys("cmd-z");
    test.assert("ˇ");
}

#[gpui::test]
fn windows_and_linux_copy_and_paste_with_ctrl_shift(cx: &mut TestAppContext) {
    for platform in [KeymapPlatform::Windows, KeymapPlatform::Linux] {
        let mut test = vim_on(cx, platform, "ˇone two");
        test.keys("v e ctrl-shift-c");
        let copied = test.cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("one"));
        test.keys("A ctrl-shift-v");
        test.assert("one twooneˇ");
        test.cx
            .write_to_clipboard(ClipboardItem::new_string("!".into()));
        test.keys("escape 0 ctrl-shift-v");
        test.assert("ˇ!one twoone");
    }
}

#[gpui::test]
fn the_keyboard_opens_the_context_menu_in_normal_mode(cx: &mut TestAppContext) {
    let mut test = vim(cx, "abˇc");
    let opened = Rc::new(RefCell::new(Vec::new()));
    test.update(|state, _, _| {
        let opened = opened.clone();
        state.on_context_menu(Rc::new(move |_, capabilities, _, _, _| {
            opened.borrow_mut().push(capabilities.opened_at());
        }));
    });
    test.keys("shift-f10");
    assert_eq!(*opened.borrow(), [Some(2)]);
    test.keys("v h menu");
    assert_eq!(opened.borrow().len(), 2);
}
