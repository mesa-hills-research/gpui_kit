//! Vim's binding table.
//!
//! Vim's keys are the same on every platform: Ctrl-V starts visual block
//! mode and inserts the next character literally in insert mode, as Vim
//! does on Unix. The platform's clipboard and history shortcuts stay where
//! they don't clash with Vim's keys: Cmd on macOS, and Ctrl-Shift-C and
//! Ctrl-Shift-V on Windows and Linux.

use gpui::KeyBinding;

use super::{
    ChangeToEnd, CommandLineBackspace, CommandLineClear, CommandLineDeleteWord, CommandLineExecute,
    CopyToClipboard, CutToClipboard, DeleteChar, DeleteCharBefore, DeleteToEnd, DeleteToLineStart,
    Digit, FindChar, IncrementNumber, IndentLine, InputChar, InsertAfter, InsertEnd,
    InsertFirstNonBlank, InsertLineAbove, InsertLineBelow, InsertLineStart, InsertLiteral,
    InsertRegister, Join, JoinWithoutSpaces, Move, OpenCommandLine, OutdentLine,
    PasteFromClipboard, PushOperator, Put, Redo, Repeat, ReplaceBackspace, ReplaceChar,
    ReplaceMode, ReselectVisual, SelectAll, SelectObject, SelectRegister, StartSearch, Substitute,
    SubstituteLine, SwapVisualCorners, SwapVisualEnds, SwitchToInsertMode, SwitchToNormalMode,
    ToggleCaseChar, ToggleVisual, ToggleVisualBlock, ToggleVisualLine, Undo, VimQuit, VisualAppend,
    VisualChangeLines, VisualChangeToLineEnd, VisualDeleteLines, VisualDeleteToLineEnd,
    VisualInsert, VisualYankLines, WriteAndQuit, YankLine, motion::Motion, object::Object,
    operator::Operator,
};
use crate::input::{
    Backspace, Copy, Cut, DeleteToPreviousWordStart, Paste, Search, SelectNextSuggestion,
    SelectPreviousSuggestion, ShowContextMenu,
    keymap::{Keymap, KeymapPlatform, bind, common},
};

/// Vim's bindings on `platform`.
pub(in crate::input::keymap) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let vim = Keymap::Vim.context();
    let mode = |modes: &[&str]| {
        let modes: Vec<String> = modes
            .iter()
            .map(|mode| format!("vim_mode == {mode}"))
            .collect();
        format!("{vim} && ({})", modes.join(" || "))
    };
    let normal = &mode(&["normal"]);
    let visual = &mode(&["visual"]);
    let operator = &mode(&["operator"]);
    let waiting = &mode(&["waiting"]);
    let command = &mode(&["command"]);
    let replace = &mode(&["replace"]);
    let typing = &mode(&["insert", "replace"]);
    // Normal and visual mode, where commands are typed.
    let commands = &mode(&["normal", "visual"]);
    // And operator-pending mode, where motions are typed too.
    let moving = &mode(&["normal", "visual", "operator"]);
    let counting = &format!("{moving} && vim_count");
    let selecting = &mode(&["visual", "operator"]);
    let block = &format!("{visual} && vim_visual == block");
    let pending = |pending: Operator| format!("{operator} && vim_operator == {}", pending.name());

    // Typing: the keys every scheme shares, then Vim's.
    let mut bindings = common::bindings(typing);
    for escape in ["escape", "ctrl-[", "ctrl-c"] {
        for context in [typing, moving, waiting, command] {
            bindings.push(bind(escape, SwitchToNormalMode, context));
        }
    }
    bindings.extend([
        bind("ctrl-w", DeleteToPreviousWordStart, typing),
        bind("ctrl-u", DeleteToLineStart, typing),
        bind("ctrl-h", Backspace, typing),
        bind("ctrl-r", InsertRegister, typing),
        bind("ctrl-v", InsertLiteral, typing),
        bind("ctrl-t", IndentLine, typing),
        bind("ctrl-d", OutdentLine, typing),
        bind("ctrl-n", SelectNextSuggestion, typing),
        bind("ctrl-p", SelectPreviousSuggestion, typing),
        bind("backspace", ReplaceBackspace, replace),
        bind("ctrl-h", ReplaceBackspace, replace),
    ]);

    // A character for `f`, `r`, `"` and the like that has a key of its own.
    bindings.extend([
        bind("enter", InputChar { character: '\n' }, waiting),
        bind("tab", InputChar { character: '\t' }, waiting),
    ]);

    // The command line.
    bindings.extend([
        bind("enter", CommandLineExecute, command),
        bind("backspace", CommandLineBackspace, command),
        bind("ctrl-h", CommandLineBackspace, command),
        bind("ctrl-u", CommandLineClear, command),
        bind("ctrl-w", CommandLineDeleteWord, command),
    ]);

    // Motions.
    let motions: &[(&str, Motion)] = &[
        ("h", Motion::Left),
        ("left", Motion::Left),
        ("backspace", Motion::WrappingLeft),
        ("l", Motion::Right),
        ("right", Motion::Right),
        ("space", Motion::WrappingRight),
        ("k", Motion::Up),
        ("up", Motion::Up),
        ("j", Motion::Down),
        ("down", Motion::Down),
        ("g k", Motion::DisplayUp),
        ("g up", Motion::DisplayUp),
        ("g j", Motion::DisplayDown),
        ("g down", Motion::DisplayDown),
        ("w", Motion::NextWordStart { big: false }),
        ("W", Motion::NextWordStart { big: true }),
        ("b", Motion::PreviousWordStart { big: false }),
        ("B", Motion::PreviousWordStart { big: true }),
        ("e", Motion::NextWordEnd { big: false }),
        ("E", Motion::NextWordEnd { big: true }),
        ("g e", Motion::PreviousWordEnd { big: false }),
        ("g E", Motion::PreviousWordEnd { big: true }),
        ("0", Motion::LineStart),
        ("home", Motion::LineStart),
        ("^", Motion::FirstNonBlank),
        ("$", Motion::LineEnd),
        ("end", Motion::LineEnd),
        ("g _", Motion::LastNonBlank),
        ("+", Motion::NextLineStart),
        ("enter", Motion::NextLineStart),
        ("-", Motion::PreviousLineStart),
        ("_", Motion::CurrentLineStart),
        ("g g", Motion::FirstLine),
        ("G", Motion::LastLine),
        (";", Motion::RepeatFind { reverse: false }),
        (",", Motion::RepeatFind { reverse: true }),
        ("%", Motion::MatchBracket),
        ("{", Motion::ParagraphBackward),
        ("}", Motion::ParagraphForward),
        ("H", Motion::WindowTop),
        ("M", Motion::WindowMiddle),
        ("L", Motion::WindowBottom),
        ("n", Motion::SearchNext { reverse: false }),
        ("N", Motion::SearchNext { reverse: true }),
        ("*", Motion::SearchWord { backward: false }),
        ("#", Motion::SearchWord { backward: true }),
    ];
    for (keys, motion) in motions {
        bindings.push(bind(keys, Move { motion: *motion }, moving));
    }
    let pages: &[(&str, Motion)] = &[
        ("ctrl-d", Motion::HalfPageDown),
        ("ctrl-u", Motion::HalfPageUp),
        ("ctrl-f", Motion::PageDown),
        ("ctrl-b", Motion::PageUp),
        ("pagedown", Motion::PageDown),
        ("pageup", Motion::PageUp),
    ];
    for (keys, motion) in pages {
        bindings.push(bind(keys, Move { motion: *motion }, commands));
    }
    for (keys, backward, till) in [
        ("f", false, false),
        ("F", true, false),
        ("t", false, true),
        ("T", true, true),
    ] {
        bindings.push(bind(keys, FindChar { backward, till }, moving));
    }
    for (keys, backward) in [("/", false), ("?", true)] {
        bindings.push(bind(keys, StartSearch { backward }, moving));
    }

    // Counts. `0` moves to the line's start unless it continues a count.
    for digit in 1..=9 {
        bindings.push(bind(&digit.to_string(), Digit { digit }, moving));
    }
    bindings.push(bind("0", Digit { digit: 0 }, counting));

    // Operators, and the same key again for the caret's line.
    let operators: &[(&str, Operator)] = &[
        ("d", Operator::Delete),
        ("c", Operator::Change),
        ("y", Operator::Yank),
        (">", Operator::Indent),
        ("<", Operator::Outdent),
        ("g ~", Operator::ToggleCase),
        ("g u", Operator::Lowercase),
        ("g U", Operator::Uppercase),
    ];
    for (keys, operator) in operators {
        bindings.push(bind(
            keys,
            PushOperator {
                operator: *operator,
            },
            commands,
        ));
        bindings.push(bind(
            keys,
            Move {
                motion: Motion::CurrentLine,
            },
            &pending(*operator),
        ));
    }
    for (keys, operator) in [
        ("~", Operator::ToggleCase),
        ("u", Operator::Lowercase),
        ("U", Operator::Uppercase),
    ] {
        bindings.push(bind(
            keys,
            Move {
                motion: Motion::CurrentLine,
            },
            &pending(operator),
        ));
    }

    // Text objects, after an operator or in visual mode.
    let objects: &[(&str, Object)] = &[
        ("w", Object::Word { big: false }),
        ("W", Object::Word { big: true }),
        ("\"", Object::Quote('"')),
        ("'", Object::Quote('\'')),
        ("`", Object::Quote('`')),
        (
            "(",
            Object::Bracket {
                open: '(',
                close: ')',
            },
        ),
        (
            ")",
            Object::Bracket {
                open: '(',
                close: ')',
            },
        ),
        (
            "b",
            Object::Bracket {
                open: '(',
                close: ')',
            },
        ),
        (
            "[",
            Object::Bracket {
                open: '[',
                close: ']',
            },
        ),
        (
            "]",
            Object::Bracket {
                open: '[',
                close: ']',
            },
        ),
        (
            "{",
            Object::Bracket {
                open: '{',
                close: '}',
            },
        ),
        (
            "}",
            Object::Bracket {
                open: '{',
                close: '}',
            },
        ),
        (
            "B",
            Object::Bracket {
                open: '{',
                close: '}',
            },
        ),
        (
            "<",
            Object::Bracket {
                open: '<',
                close: '>',
            },
        ),
        (
            ">",
            Object::Bracket {
                open: '<',
                close: '>',
            },
        ),
        ("t", Object::Tag),
        ("p", Object::Paragraph),
    ];
    for (key, object) in objects {
        for (prefix, around) in [("i", false), ("a", true)] {
            bindings.push(bind(
                &format!("{prefix} {key}"),
                SelectObject {
                    object: *object,
                    around,
                },
                selecting,
            ));
        }
    }

    // Normal mode's commands.
    bindings.extend([
        bind("i", SwitchToInsertMode, normal),
        bind("insert", SwitchToInsertMode, normal),
        bind("a", InsertAfter, normal),
        bind("I", InsertFirstNonBlank, normal),
        bind("g I", InsertLineStart, normal),
        bind("A", InsertEnd, normal),
        bind("o", InsertLineBelow, normal),
        bind("O", InsertLineAbove, normal),
        bind("R", ReplaceMode, normal),
        bind("x", DeleteChar, normal),
        bind("delete", DeleteChar, normal),
        bind("X", DeleteCharBefore, normal),
        bind("D", DeleteToEnd, normal),
        bind("C", ChangeToEnd, normal),
        bind("Y", YankLine, normal),
        bind("s", Substitute, normal),
        bind("S", SubstituteLine, normal),
        bind("r", ReplaceChar, normal),
        bind("~", ToggleCaseChar, normal),
        bind("J", Join, normal),
        bind("g J", JoinWithoutSpaces, normal),
        bind(
            "p",
            Put {
                before: false,
                move_past: false,
            },
            normal,
        ),
        bind(
            "P",
            Put {
                before: true,
                move_past: false,
            },
            normal,
        ),
        bind(
            "g p",
            Put {
                before: false,
                move_past: true,
            },
            normal,
        ),
        bind(
            "g P",
            Put {
                before: true,
                move_past: true,
            },
            normal,
        ),
        bind("u", Undo, normal),
        bind("ctrl-r", Redo, normal),
        bind(".", Repeat, normal),
        bind("ctrl-a", IncrementNumber { step: 1 }, normal),
        bind("ctrl-x", IncrementNumber { step: -1 }, normal),
        bind("Z Z", WriteAndQuit, normal),
        bind("Z Q", VimQuit, normal),
    ]);

    // Shared by normal and visual mode.
    bindings.extend([
        bind("v", ToggleVisual, commands),
        bind("V", ToggleVisualLine, commands),
        bind("ctrl-v", ToggleVisualBlock, commands),
        bind("g v", ReselectVisual, commands),
        bind("\"", SelectRegister, commands),
        bind(":", OpenCommandLine, commands),
        bind("shift-f10", ShowContextMenu, commands),
        bind("menu", ShowContextMenu, commands),
    ]);

    // Visual mode's own keys.
    bindings.extend([
        bind("o", SwapVisualEnds, visual),
        bind("O", SwapVisualEnds, visual),
        bind("O", SwapVisualCorners, block),
        bind(
            "x",
            PushOperator {
                operator: Operator::Delete,
            },
            visual,
        ),
        bind(
            "delete",
            PushOperator {
                operator: Operator::Delete,
            },
            visual,
        ),
        bind(
            "s",
            PushOperator {
                operator: Operator::Change,
            },
            visual,
        ),
        bind(
            "~",
            PushOperator {
                operator: Operator::ToggleCase,
            },
            visual,
        ),
        bind(
            "u",
            PushOperator {
                operator: Operator::Lowercase,
            },
            visual,
        ),
        bind(
            "U",
            PushOperator {
                operator: Operator::Uppercase,
            },
            visual,
        ),
        bind("X", VisualDeleteLines, visual),
        bind("D", VisualDeleteLines, visual),
        bind("D", VisualDeleteToLineEnd, block),
        bind("Y", VisualYankLines, visual),
        bind("S", VisualChangeLines, visual),
        bind("R", VisualChangeLines, visual),
        bind("C", VisualChangeLines, visual),
        bind("C", VisualChangeToLineEnd, block),
        bind("r", ReplaceChar, visual),
        bind("J", Join, visual),
        bind("g J", JoinWithoutSpaces, visual),
        bind(
            "p",
            Put {
                before: false,
                move_past: false,
            },
            visual,
        ),
        bind(
            "P",
            Put {
                before: true,
                move_past: false,
            },
            visual,
        ),
        bind("I", VisualInsert, block),
        bind("A", VisualAppend, block),
    ]);

    // The platform's own shortcuts, where they don't clash with Vim's.
    let (copy, cut, paste) = if platform.is_macos() {
        ("cmd-c", Some("cmd-x"), "cmd-v")
    } else {
        ("ctrl-shift-c", None, "ctrl-shift-v")
    };
    bindings.extend([
        bind(copy, Copy, typing),
        bind(paste, Paste, typing),
        bind(copy, CopyToClipboard, commands),
        bind(paste, PasteFromClipboard, commands),
    ]);
    if let Some(cut) = cut {
        bindings.extend([bind(cut, Cut, typing), bind(cut, CutToClipboard, commands)]);
    }
    if platform.is_macos() {
        bindings.extend([
            bind("cmd-a", crate::input::SelectAll, typing),
            bind("cmd-a", SelectAll, commands),
            bind("cmd-z", crate::input::Undo, typing),
            bind("cmd-shift-z", crate::input::Redo, typing),
            bind("cmd-z", Undo, commands),
            bind("cmd-shift-z", Redo, commands),
            bind("cmd-f", Search, vim),
        ]);
    }

    bindings
}
