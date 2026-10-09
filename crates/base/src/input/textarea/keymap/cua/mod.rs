//! CUA: the platform's usual shortcuts, and every input's default.
//!
//! Inputs and editors always follow it, so its own commands (in `commands`)
//! work in every input. The tables follow each platform's text views:
//!
//! - Windows and Linux carry the shortcuts on Ctrl, with the classic CUA
//!   clipboard keys (Shift-Delete, Ctrl-Insert, Shift-Insert) beside Ctrl-X,
//!   C and V. Windows moves by word to word starts and stops at line ends, as
//!   Word and Notepad do. Linux moves to word ends going right, as GTK does.
//!   Both redo with Ctrl-Y and Ctrl-Shift-Z.
//! - macOS carries them on Cmd and binds the Cocoa text system's keys: Option
//!   to move by word and line, Cmd to the ends of lines and the text, the Fn
//!   keys scrolling without moving the caret, and the Emacs-style Control keys
//!   with a kill buffer of their own.

mod commands;
#[cfg(test)]
mod tests;

use gpui::KeyBinding;

use super::{Keymap, KeymapPlatform, bind, common};
use crate::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use crate::input::{
    AddCursorAbove, AddCursorBelow, Backspace, Copy, Cut, Delete, DeleteToBeginningOfLine,
    DeleteToEndOfLine, DeleteToNextWordEnd, DeleteToPreviousWordStart, Indent, MoveDown, MoveEnd,
    MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight, MoveToEnd, MoveToNextWord,
    MoveToPreviousWord, MoveToStart, MoveUp, Outdent, Paste, Redo, Replace, Search, SelectAll,
    SelectToEnd, SelectToEndOfLine, SelectToNextWordEnd, SelectToPreviousWordStart, SelectToStart,
    SelectToStartOfLine, ShowCharacterPalette, ToggleCodeActions, TransposeCharacters, Undo,
};

pub(crate) use commands::register_actions;
pub use commands::{
    DeleteToWordStartLeft, DeleteToWordStartRight, KillToEndOfLine, MoveBackwardToLineStart,
    MoveForwardToLineEnd, MoveForwardToLineStart, MoveToWordStartLeft, MoveToWordStartRight,
    OpenLine, ScrollCaretToCenter, ScrollPageDown, ScrollPageUp, ScrollToEnd, ScrollToStart,
    SelectBackwardToLineStart, SelectForwardToLineEnd, SelectForwardToLineStart, SelectPageDown,
    SelectPageUp, SelectToWordStartLeft, SelectToWordStartRight, Yank,
};

/// CUA's bindings on `platform`.
pub(super) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let cx = Keymap::Cua.context();
    let mut bindings = common::bindings(cx);
    let primary = platform.primary();

    match platform {
        KeymapPlatform::MacOS => macos(&mut bindings, cx),
        KeymapPlatform::Windows | KeymapPlatform::Linux => {
            windows_and_linux(&mut bindings, platform, cx)
        }
    }

    // The same on every platform, on its own modifier. A menu shows an
    // action's latest binding, so these come after the platforms' second
    // keys for the same actions, such as Shift-Delete for Cut.
    bindings.extend([
        bind(&format!("{primary}-a"), SelectAll, cx),
        bind(&format!("{primary}-c"), Copy, cx),
        bind(&format!("{primary}-x"), Cut, cx),
        bind(&format!("{primary}-v"), Paste, cx),
        bind(&format!("{primary}-z"), Undo, cx),
        bind(&format!("{primary}-f"), Search, cx),
        bind(&format!("{primary}-."), ToggleCodeActions, cx),
        bind(&format!("{primary}-]"), Indent, cx),
        bind(&format!("{primary}-["), Outdent, cx),
        bind("shift-pageup", SelectPageUp, cx),
        bind("shift-pagedown", SelectPageDown, cx),
    ]);
    bindings
}

/// The Cocoa text system's keys.
fn macos(bindings: &mut Vec<KeyBinding>, cx: &str) {
    // Cmd to the ends of the line and the text, Option by word and by line.
    bindings.extend([
        bind("cmd-left", MoveHome, cx),
        bind("cmd-right", MoveEnd, cx),
        bind("cmd-up", MoveToStart, cx),
        bind("cmd-down", MoveToEnd, cx),
        bind("alt-left", MoveToPreviousWord, cx),
        bind("alt-right", MoveToNextWord, cx),
        bind("alt-up", MoveBackwardToLineStart, cx),
        bind("alt-down", MoveForwardToLineEnd, cx),
        bind("cmd-shift-left", SelectToStartOfLine, cx),
        bind("cmd-shift-right", SelectToEndOfLine, cx),
        bind("cmd-shift-up", SelectToStart, cx),
        bind("cmd-shift-down", SelectToEnd, cx),
        bind("alt-shift-left", SelectToPreviousWordStart, cx),
        bind("alt-shift-right", SelectToNextWordEnd, cx),
        bind("alt-shift-up", SelectBackwardToLineStart, cx),
        bind("alt-shift-down", SelectForwardToLineEnd, cx),
    ]);

    // Home, End, Page Up and Page Down (Fn with the arrows) scroll and leave
    // the caret where it is. With Shift they select, and Option-Page Up and
    // Down move the caret.
    replace(
        bindings,
        [
            bind("home", ScrollToStart, cx),
            bind("end", ScrollToEnd, cx),
            bind("pageup", ScrollPageUp, cx),
            bind("pagedown", ScrollPageDown, cx),
            bind("shift-home", SelectToStart, cx),
            bind("shift-end", SelectToEnd, cx),
        ],
    );
    bindings.extend([
        bind("alt-pageup", MovePageUp, cx),
        bind("alt-pagedown", MovePageDown, cx),
    ]);

    // Deleting.
    bindings.extend([
        bind("cmd-backspace", DeleteToBeginningOfLine, cx),
        bind("cmd-delete", DeleteToEndOfLine, cx),
        bind("alt-backspace", DeleteToPreviousWordStart, cx),
        bind("alt-delete", DeleteToNextWordEnd, cx),
        bind("ctrl-backspace", Backspace, cx),
    ]);

    // The Emacs-style Control keys every Cocoa text field has, with Shift to
    // select where they move.
    bindings.extend([
        bind("ctrl-a", MoveHome, cx),
        bind("ctrl-e", MoveEnd, cx),
        bind("ctrl-f", MoveRight, cx),
        bind("ctrl-b", MoveLeft, cx),
        bind("ctrl-n", MoveDown, cx),
        bind("ctrl-p", MoveUp, cx),
        bind("ctrl-v", MovePageDown, cx),
        bind("ctrl-shift-a", SelectToStartOfLine, cx),
        bind("ctrl-shift-e", SelectToEndOfLine, cx),
        bind("ctrl-shift-f", SelectRight, cx),
        bind("ctrl-shift-b", SelectLeft, cx),
        bind("ctrl-shift-n", SelectDown, cx),
        bind("ctrl-shift-p", SelectUp, cx),
        bind("ctrl-shift-v", SelectPageDown, cx),
        bind("ctrl-d", Delete, cx),
        bind("ctrl-h", Backspace, cx),
        bind("ctrl-k", KillToEndOfLine, cx),
        bind("ctrl-y", Yank, cx),
        bind("ctrl-t", TransposeCharacters, cx),
        bind("ctrl-o", OpenLine, cx),
        bind("ctrl-l", ScrollCaretToCenter, cx),
    ]);

    // History, search, cursors and the character palette.
    bindings.extend([
        bind("cmd-shift-z", Redo, cx),
        bind("cmd-alt-f", Replace, cx),
        bind("cmd-shift-f", Replace, cx),
        bind("cmd-alt-up", AddCursorAbove, cx),
        bind("cmd-alt-down", AddCursorBelow, cx),
        bind("ctrl-cmd-space", ShowCharacterPalette, cx),
    ]);
}

/// Windows's and Linux's keys, which differ in moving by word and paragraph
/// and in the keys for multiple cursors.
fn windows_and_linux(bindings: &mut Vec<KeyBinding>, platform: KeymapPlatform, cx: &str) {
    let windows = platform == KeymapPlatform::Windows;

    // By word: to word starts and line ends on Windows, to word ends going
    // right on Linux.
    if windows {
        bindings.extend([
            bind("ctrl-left", MoveToWordStartLeft, cx),
            bind("ctrl-right", MoveToWordStartRight, cx),
            bind("ctrl-shift-left", SelectToWordStartLeft, cx),
            bind("ctrl-shift-right", SelectToWordStartRight, cx),
            bind("ctrl-backspace", DeleteToWordStartLeft, cx),
            bind("ctrl-delete", DeleteToWordStartRight, cx),
        ]);
    } else {
        bindings.extend([
            bind("ctrl-left", MoveToPreviousWord, cx),
            bind("ctrl-right", MoveToNextWord, cx),
            bind("ctrl-shift-left", SelectToPreviousWordStart, cx),
            bind("ctrl-shift-right", SelectToNextWordEnd, cx),
            bind("ctrl-backspace", DeleteToPreviousWordStart, cx),
            bind("ctrl-delete", DeleteToNextWordEnd, cx),
        ]);
    }

    // By paragraph, which is a line: to its start going up on both, and down
    // to the next line's start on Windows (Word, WordPad) or to the line's
    // end on Linux (GTK).
    bindings.extend([
        bind("ctrl-up", MoveBackwardToLineStart, cx),
        bind("ctrl-shift-up", SelectBackwardToLineStart, cx),
    ]);
    if windows {
        bindings.extend([
            bind("ctrl-down", MoveForwardToLineStart, cx),
            bind("ctrl-shift-down", SelectForwardToLineStart, cx),
        ]);
    } else {
        bindings.extend([
            bind("ctrl-down", MoveForwardToLineEnd, cx),
            bind("ctrl-shift-down", SelectForwardToLineEnd, cx),
        ]);
    }

    // To the ends of the text.
    bindings.extend([
        bind("ctrl-home", MoveToStart, cx),
        bind("ctrl-end", MoveToEnd, cx),
        bind("ctrl-shift-home", SelectToStart, cx),
        bind("ctrl-shift-end", SelectToEnd, cx),
    ]);

    // Redo, the classic CUA clipboard keys and Replace. Shift-Delete cuts
    // here, where the shared keys have it delete. Menus show Ctrl-Y for Redo
    // on Windows and Ctrl-Shift-Z on Linux, the later of the two.
    let (redo, menu_redo) = if windows {
        ("ctrl-shift-z", "ctrl-y")
    } else {
        ("ctrl-y", "ctrl-shift-z")
    };
    bindings.extend([
        bind(redo, Redo, cx),
        bind(menu_redo, Redo, cx),
        bind("ctrl-insert", Copy, cx),
        bind("shift-insert", Paste, cx),
        bind("ctrl-h", Replace, cx),
    ]);
    replace(bindings, [bind("shift-delete", Cut, cx)]);

    // Undo on Alt-Backspace, selecting by character with Alt, and adding
    // cursors. Ctrl-Alt-arrows are left alone on Linux, where desktops may
    // reserve them.
    if windows {
        bindings.extend([
            bind("alt-backspace", Undo, cx),
            bind("shift-alt-left", SelectLeft, cx),
            bind("shift-alt-right", SelectRight, cx),
            bind("ctrl-alt-up", AddCursorAbove, cx),
            bind("ctrl-alt-down", AddCursorBelow, cx),
        ]);
    } else {
        bindings.extend([
            bind("alt-shift-left", SelectToPreviousWordStart, cx),
            bind("alt-shift-right", SelectToNextWordEnd, cx),
            bind("shift-alt-up", AddCursorAbove, cx),
            bind("shift-alt-down", AddCursorBelow, cx),
        ]);
    }
}

/// Add `new`, each in place of the shared binding of the same keys.
fn replace(bindings: &mut Vec<KeyBinding>, new: impl IntoIterator<Item = KeyBinding>) {
    for binding in new {
        bindings.retain(|old| old.keystrokes() != binding.keystrokes());
        bindings.push(binding);
    }
}
