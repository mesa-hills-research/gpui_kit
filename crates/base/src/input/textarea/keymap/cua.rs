//! CUA: the platform's usual shortcuts, and every input's default.
//!
//! Inputs and editors always follow it. Ctrl carries the shortcuts on Windows
//! and Linux and Cmd on macOS, where Ctrl and Option keep the Cocoa text keys
//! (Ctrl-A and Ctrl-E, Option to move by word).

use gpui::KeyBinding;

use super::{Keymap, KeymapPlatform, bind, common};
use crate::actions::{SelectLeft, SelectRight};
use crate::input::{
    AddCursorAbove, AddCursorBelow, Backspace, Copy, Cut, DeleteToBeginningOfLine,
    DeleteToEndOfLine, DeleteToNextWordEnd, DeleteToPreviousWordStart, Indent, MoveEnd, MoveHome,
    MoveToEnd, MoveToNextWord, MoveToPreviousWord, MoveToStart, Outdent, Paste, Redo, Replace,
    Search, SelectAll, SelectToEnd, SelectToEndOfLine, SelectToNextWordEnd,
    SelectToPreviousWordStart, SelectToStart, SelectToStartOfLine, ShowCharacterPalette,
    ToggleCodeActions, Undo,
};

/// CUA's bindings on `platform`.
pub(super) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let cx = Keymap::Cua.context();
    let mut bindings = common::bindings(cx);
    let mac = platform.is_macos();
    let primary = platform.primary();

    // Deleting by word and to the line's ends.
    if mac {
        bindings.extend([
            bind("ctrl-backspace", Backspace, cx),
            bind("cmd-backspace", DeleteToBeginningOfLine, cx),
            bind("cmd-delete", DeleteToEndOfLine, cx),
            bind("alt-backspace", DeleteToPreviousWordStart, cx),
            bind("alt-delete", DeleteToNextWordEnd, cx),
        ]);
    } else {
        bindings.extend([
            bind("ctrl-backspace", DeleteToPreviousWordStart, cx),
            bind("ctrl-delete", DeleteToNextWordEnd, cx),
        ]);
    }

    // Indenting the lines a selection touches.
    bindings.extend([
        bind(&format!("{primary}-]"), Indent, cx),
        bind(&format!("{primary}-["), Outdent, cx),
    ]);

    // Selecting by character with Alt, and adding cursors. Ctrl-Alt-arrows are
    // left alone on Linux, where desktops may reserve them.
    match platform {
        KeymapPlatform::MacOS => bindings.extend([
            bind("cmd-alt-up", AddCursorAbove, cx),
            bind("cmd-alt-down", AddCursorBelow, cx),
        ]),
        KeymapPlatform::Windows => bindings.extend([
            bind("shift-alt-left", SelectLeft, cx),
            bind("shift-alt-right", SelectRight, cx),
            bind("ctrl-alt-up", AddCursorAbove, cx),
            bind("ctrl-alt-down", AddCursorBelow, cx),
        ]),
        KeymapPlatform::Linux => bindings.extend([
            bind("shift-alt-up", AddCursorAbove, cx),
            bind("shift-alt-down", AddCursorBelow, cx),
        ]),
    }

    // Moving and selecting to the ends of the text, the line and the word.
    if mac {
        bindings.extend([
            bind("ctrl-shift-a", SelectToStartOfLine, cx),
            bind("ctrl-shift-e", SelectToEndOfLine, cx),
            bind("shift-cmd-left", SelectToStartOfLine, cx),
            bind("shift-cmd-right", SelectToEndOfLine, cx),
            bind("alt-shift-left", SelectToPreviousWordStart, cx),
            bind("alt-shift-right", SelectToNextWordEnd, cx),
            bind("ctrl-a", MoveHome, cx),
            bind("cmd-left", MoveHome, cx),
            bind("ctrl-e", MoveEnd, cx),
            bind("cmd-right", MoveEnd, cx),
            bind("cmd-up", MoveToStart, cx),
            bind("cmd-down", MoveToEnd, cx),
            bind("alt-left", MoveToPreviousWord, cx),
            bind("alt-right", MoveToNextWord, cx),
            bind("cmd-shift-up", SelectToStart, cx),
            bind("cmd-shift-down", SelectToEnd, cx),
            bind("ctrl-cmd-space", ShowCharacterPalette, cx),
        ]);
    } else {
        bindings.extend([
            bind("ctrl-home", MoveToStart, cx),
            bind("ctrl-end", MoveToEnd, cx),
            bind("ctrl-shift-home", SelectToStart, cx),
            bind("ctrl-shift-end", SelectToEnd, cx),
            bind("ctrl-shift-left", SelectToPreviousWordStart, cx),
            bind("ctrl-shift-right", SelectToNextWordEnd, cx),
            bind("ctrl-left", MoveToPreviousWord, cx),
            bind("ctrl-right", MoveToNextWord, cx),
        ]);
        if platform == KeymapPlatform::Linux {
            bindings.extend([
                bind("alt-shift-left", SelectToPreviousWordStart, cx),
                bind("alt-shift-right", SelectToNextWordEnd, cx),
            ]);
        }
    }

    // The clipboard, history, search and code actions.
    bindings.extend([
        bind(&format!("{primary}-a"), SelectAll, cx),
        bind(&format!("{primary}-c"), Copy, cx),
        bind(&format!("{primary}-x"), Cut, cx),
        bind(&format!("{primary}-v"), Paste, cx),
        bind(&format!("{primary}-z"), Undo, cx),
        bind(&format!("{primary}-."), ToggleCodeActions, cx),
        bind(&format!("{primary}-f"), Search, cx),
    ]);
    if mac {
        bindings.extend([
            bind("cmd-shift-z", Redo, cx),
            bind("cmd-shift-f", Replace, cx),
        ]);
    } else {
        bindings.extend([bind("ctrl-y", Redo, cx), bind("ctrl-h", Replace, cx)]);
    }

    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(platform: KeymapPlatform) -> Vec<(gpui::Keystroke, &'static str)> {
        bindings(platform)
            .iter()
            .map(|binding| {
                (
                    binding.keystrokes()[0].inner().clone(),
                    binding.action().name(),
                )
            })
            .collect()
    }

    fn has(platform: KeymapPlatform, keys: &str, action: &str) -> bool {
        let keys = gpui::Keystroke::parse(keys).unwrap();
        bound(platform)
            .iter()
            .any(|(k, a)| *k == keys && a.ends_with(action))
    }

    /// The tables hold the bindings inputs had before schemes existed: the
    /// same count on each platform, and the keys that differ between them.
    #[test]
    fn the_tables_keep_every_platforms_shortcuts() {
        assert_eq!(bindings(KeymapPlatform::MacOS).len(), 61);
        assert_eq!(bindings(KeymapPlatform::Windows).len(), 51);
        assert_eq!(bindings(KeymapPlatform::Linux).len(), 51);

        use KeymapPlatform::*;
        assert!(has(MacOS, "cmd-a", "SelectAll"));
        assert!(has(MacOS, "ctrl-a", "MoveHome"));
        assert!(has(MacOS, "alt-backspace", "DeleteToPreviousWordStart"));
        assert!(has(MacOS, "cmd-shift-z", "Redo"));
        assert!(!has(MacOS, "ctrl-home", "MoveToStart"));
        for platform in [Windows, Linux] {
            assert!(has(platform, "ctrl-a", "SelectAll"));
            assert!(has(platform, "ctrl-y", "Redo"));
            assert!(has(platform, "ctrl-h", "Replace"));
            assert!(has(platform, "ctrl-backspace", "DeleteToPreviousWordStart"));
        }
        assert!(has(Windows, "shift-alt-left", "SelectLeft"));
        assert!(has(Windows, "ctrl-alt-up", "AddCursorAbove"));
        assert!(has(Linux, "shift-alt-up", "AddCursorAbove"));
        assert!(has(Linux, "alt-shift-left", "SelectToPreviousWordStart"));
        assert!(!has(Linux, "ctrl-alt-up", "AddCursorAbove"));

        // No platform binds one key twice.
        for platform in KeymapPlatform::ALL {
            let mut keys: Vec<String> = bound(platform)
                .into_iter()
                .map(|(k, _)| k.unparse())
                .collect();
            keys.sort();
            let count = keys.len();
            keys.dedup();
            assert_eq!(keys.len(), count, "{platform:?} binds a key twice");
        }
    }
}
