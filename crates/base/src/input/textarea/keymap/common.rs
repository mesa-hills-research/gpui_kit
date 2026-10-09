//! Keys with the same meaning in every scheme while it edits text: the
//! arrows, Home and End, Page Up and Down, Backspace, Delete, Enter, Tab and
//! Escape, with Shift to select.

use gpui::KeyBinding;

use super::bind;
use crate::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use crate::input::{
    Backspace, Delete, Enter, Escape, IndentInline, MoveDown, MoveEnd, MoveHome, MoveLeft,
    MovePageDown, MovePageUp, MoveRight, MoveUp, OutdentInline, SelectToEndOfLine,
    SelectToStartOfLine, ShowContextMenu,
};

/// The shared keys, bound in `context`. They are the same on every platform.
///
/// A scheme puts them in its table with its own context, or in a narrower one
/// such as Vim's insert mode.
pub(super) fn bindings(context: &str) -> Vec<KeyBinding> {
    vec![
        bind("backspace", Backspace, context),
        bind("shift-backspace", Backspace, context),
        bind("delete", Delete, context),
        bind("shift-delete", Delete, context),
        bind(
            "enter",
            Enter {
                secondary: false,
                shift: false,
            },
            context,
        ),
        bind(
            "shift-enter",
            Enter {
                secondary: false,
                shift: true,
            },
            context,
        ),
        bind(
            "secondary-enter",
            Enter {
                secondary: true,
                shift: false,
            },
            context,
        ),
        bind("escape", Escape, context),
        bind("shift-f10", ShowContextMenu, context),
        bind("menu", ShowContextMenu, context),
        bind("up", MoveUp, context),
        bind("down", MoveDown, context),
        bind("left", MoveLeft, context),
        bind("right", MoveRight, context),
        bind("pageup", MovePageUp, context),
        bind("pagedown", MovePageDown, context),
        bind("tab", IndentInline, context),
        bind("shift-tab", OutdentInline, context),
        bind("shift-left", SelectLeft, context),
        bind("shift-right", SelectRight, context),
        bind("shift-up", SelectUp, context),
        bind("shift-down", SelectDown, context),
        bind("home", MoveHome, context),
        bind("end", MoveEnd, context),
        bind("shift-home", SelectToStartOfLine, context),
        bind("shift-end", SelectToEndOfLine, context),
    ]
}
