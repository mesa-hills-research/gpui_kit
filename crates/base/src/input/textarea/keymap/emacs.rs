//! Emacs keys.
//!
//! Control and Meta carry the commands. Meta is Alt on Windows and Linux and
//! Option on macOS, where Cmd keeps the system's shortcuts for the clipboard,
//! history and Select All.
//!
//! This module holds the scheme's table and, as it grows, the state it keeps
//! per textarea (the mark, the kill ring) and the actions only Emacs has.

use gpui::{Context, Div, Entity, KeyBinding, Stateful, Window};

use super::{Keymap, KeymapPlatform, KeymapState, bind, common};
use crate::input::{
    Copy, Cut, Delete, MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToNextWord,
    MoveToPreviousWord, MoveUp, Paste, Redo, SelectAll, TextareaState, Undo,
};

/// Emacs's bindings on `platform`.
pub(super) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let cx = Keymap::Emacs.context();
    let mut bindings = common::bindings(cx);
    bindings.extend([
        bind("ctrl-f", MoveRight, cx),
        bind("ctrl-b", MoveLeft, cx),
        bind("ctrl-n", MoveDown, cx),
        bind("ctrl-p", MoveUp, cx),
        bind("ctrl-a", MoveHome, cx),
        bind("ctrl-e", MoveEnd, cx),
        bind("ctrl-d", Delete, cx),
        bind("alt-f", MoveToNextWord, cx),
        bind("alt-b", MoveToPreviousWord, cx),
    ]);
    if platform.is_macos() {
        bindings.extend([
            bind("cmd-a", SelectAll, cx),
            bind("cmd-c", Copy, cx),
            bind("cmd-x", Cut, cx),
            bind("cmd-v", Paste, cx),
            bind("cmd-z", Undo, cx),
            bind("cmd-shift-z", Redo, cx),
        ]);
    }
    bindings
}

/// The state Emacs keeps for one textarea. None yet.
pub(super) fn new_state() -> Option<Box<dyn KeymapState>> {
    None
}

/// Registers the actions only Emacs has. None yet.
pub(super) fn register_actions(
    element: Stateful<Div>,
    _entity: &Entity<TextareaState>,
    _window: &mut Window,
) -> Stateful<Div> {
    element
}

/// Text typed while Emacs is active, before it is inserted. Returns whether
/// Emacs took it. It never does: Emacs inserts what is typed.
pub(super) fn typed_text(
    _state: &mut TextareaState,
    _text: &str,
    _window: &mut Window,
    _cx: &mut Context<TextareaState>,
) -> bool {
    false
}
