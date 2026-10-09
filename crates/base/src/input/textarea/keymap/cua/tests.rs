//! CUA on each platform: what every key in the tables does, where the
//! platforms differ, and that the keys stay out of the other schemes.

use std::collections::HashSet;

use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, IntoElement, Keystroke, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};

use super::bindings;
use crate::input::keymap::test::{KeymapTest, parse};
use crate::input::keymap::{Keymap, KeymapPlatform};
use crate::input::{InputBaseState, InputModeKind, RopeExt as _, Undo};
use KeymapPlatform::{Linux, MacOS, Windows};

const ALL: &[KeymapPlatform] = &[MacOS, Windows, Linux];
const MAC: &[KeymapPlatform] = &[MacOS];
const WIN: &[KeymapPlatform] = &[Windows];
const LINUX: &[KeymapPlatform] = &[Linux];
const PC: &[KeymapPlatform] = &[Windows, Linux];

/// Keys pressed in a textarea holding `before`, on each platform in `on`,
/// and the text, selection and caret they leave. A key that is not a binding,
/// such as `x`, types itself.
struct Case {
    on: &'static [KeymapPlatform],
    before: &'static str,
    keys: &'static str,
    after: &'static str,
}

const fn case(
    on: &'static [KeymapPlatform],
    before: &'static str,
    keys: &'static str,
    after: &'static str,
) -> Case {
    Case {
        on,
        before,
        keys,
        after,
    }
}

#[rustfmt::skip]
const CASES: &[Case] = &[
    // The keys every scheme shares.
    case(ALL, "abˇc", "left", "aˇbc"),
    case(ALL, "aˇbc", "right", "abˇc"),
    case(ALL, "ab\ncˇd", "up", "aˇb\ncd"),
    case(ALL, "aˇb\ncd", "down", "ab\ncˇd"),
    case(ALL, "abˇc", "shift-left", "a«ˇb»c"),
    case(ALL, "aˇbc", "shift-right", "a«b»c"),
    case(ALL, "ab\ncˇd", "shift-up", "a«ˇb\nc»d"),
    case(ALL, "aˇb\ncd", "shift-down", "a«b\nc»d"),
    case(ALL, "abˇc", "backspace", "aˇc"),
    case(ALL, "abˇc", "shift-backspace", "aˇc"),
    case(ALL, "aˇbc", "delete", "aˇc"),
    case(MAC, "aˇbc", "shift-delete", "aˇc"),
    case(ALL, "abˇc", "enter", "ab\nˇc"),
    case(ALL, "abˇc", "shift-enter", "ab\nˇc"),
    case(ALL, "aˇb", "tab", "a  ˇb"),
    case(ALL, "  aˇb", "shift-tab", "aˇb"),
    case(PC, "ab\ncdˇe", "home", "ab\nˇcde"),
    case(PC, "ab\nˇcde", "end", "ab\ncdeˇ"),
    case(PC, "ab\ncdˇe", "shift-home", "ab\n«ˇcd»e"),
    case(PC, "ab\ncˇde", "shift-end", "ab\nc«de»"),
    // Select All, the clipboard and the history, on Cmd or Ctrl.
    case(MAC, "aˇb\ncd", "cmd-a", "«ab\ncd»"),
    case(PC, "aˇb\ncd", "ctrl-a", "«ab\ncd»"),
    case(MAC, "a«bc»d", "cmd-c right cmd-v", "abcbcˇd"),
    case(PC, "a«bc»d", "ctrl-c right ctrl-v", "abcbcˇd"),
    case(MAC, "a«bc»d", "cmd-x right cmd-v", "adbcˇ"),
    case(PC, "a«bc»d", "ctrl-x right ctrl-v", "adbcˇ"),
    case(MAC, "aˇ", "x cmd-z", "aˇ"),
    case(PC, "aˇ", "x ctrl-z", "aˇ"),
    case(MAC, "aˇ", "x cmd-z cmd-shift-z", "axˇ"),
    case(PC, "aˇ", "x ctrl-z ctrl-y", "axˇ"),
    case(PC, "aˇ", "x ctrl-z ctrl-shift-z", "axˇ"),
    case(WIN, "aˇ", "x alt-backspace", "aˇ"),
    case(MAC, "aˇb\ncd", "cmd-]", "  aˇb\ncd"),
    case(PC, "aˇb\ncd", "ctrl-]", "  aˇb\ncd"),
    case(MAC, "  aˇb\ncd", "cmd-[", "aˇb\ncd"),
    case(PC, "  aˇb\ncd", "ctrl-[", "aˇb\ncd"),
    // The classic CUA clipboard keys. Shift-Delete cuts nothing without a
    // selection.
    case(PC, "a«bc»d", "ctrl-insert right shift-insert", "abcbcˇd"),
    case(PC, "a«bc»d", "shift-delete right shift-insert", "adbcˇ"),
    case(PC, "aˇbc", "shift-delete", "aˇbc"),
    // macOS: Cmd to the ends of the line and the text.
    case(MAC, "ab\ncdˇe", "cmd-left", "ab\nˇcde"),
    case(MAC, "ab\nˇcde", "cmd-right", "ab\ncdeˇ"),
    case(MAC, "ab\ncˇd", "cmd-up", "ˇab\ncd"),
    case(MAC, "ab\ncˇd", "cmd-down", "ab\ncdˇ"),
    case(MAC, "ab\ncdˇe", "cmd-shift-left", "ab\n«ˇcd»e"),
    case(MAC, "ab\ncˇde", "cmd-shift-right", "ab\nc«de»"),
    case(MAC, "ab\ncˇd", "cmd-shift-up", "«ˇab\nc»d"),
    case(MAC, "ab\ncˇd", "cmd-shift-down", "ab\nc«d»"),
    case(MAC, "ab\ncˇd", "shift-home", "«ˇab\nc»d"),
    case(MAC, "ab\ncˇd", "shift-end", "ab\nc«d»"),
    // macOS: Option-Up and Down go to the start or end of the line, then of
    // the next one.
    case(MAC, "ab\ncdˇe\nf", "alt-up", "ab\nˇcde\nf"),
    case(MAC, "ab\ncdˇe\nf", "alt-up alt-up", "ˇab\ncde\nf"),
    case(MAC, "aˇb\ncde\nf", "alt-down", "abˇ\ncde\nf"),
    case(MAC, "aˇb\ncde\nf", "alt-down alt-down", "ab\ncdeˇ\nf"),
    case(MAC, "ab\ncdˇe", "alt-shift-up alt-shift-up", "«ˇab\ncd»e"),
    case(MAC, "aˇb\ncd", "alt-shift-down alt-shift-down", "a«b\ncd»"),
    // macOS: deleting.
    case(MAC, "ab\ncdˇe", "cmd-backspace", "ab\nˇe"),
    case(MAC, "aˇb cd\ne", "cmd-delete", "aˇ\ne"),
    case(MAC, "abˇc", "ctrl-backspace", "aˇc"),
    // macOS: the Control keys of every Cocoa text field.
    case(MAC, "ab\ncdˇe", "ctrl-a", "ab\nˇcde"),
    case(MAC, "ab\nˇcde", "ctrl-e", "ab\ncdeˇ"),
    case(MAC, "aˇbc", "ctrl-f", "abˇc"),
    case(MAC, "aˇbc", "ctrl-b", "ˇabc"),
    case(MAC, "aˇb\ncd", "ctrl-n", "ab\ncˇd"),
    case(MAC, "ab\ncˇd", "ctrl-p", "aˇb\ncd"),
    case(MAC, "ab\ncdˇe", "ctrl-shift-a", "ab\n«ˇcd»e"),
    case(MAC, "ab\ncˇde", "ctrl-shift-e", "ab\nc«de»"),
    case(MAC, "aˇbc", "ctrl-shift-f", "a«b»c"),
    case(MAC, "abˇc", "ctrl-shift-b", "a«ˇb»c"),
    case(MAC, "aˇb\ncd", "ctrl-shift-n", "a«b\nc»d"),
    case(MAC, "ab\ncˇd", "ctrl-shift-p", "a«ˇb\nc»d"),
    case(MAC, "aˇbc", "ctrl-d", "aˇc"),
    case(MAC, "abˇc", "ctrl-h", "aˇc"),
    case(MAC, "abˇcd", "ctrl-t", "acbˇd"),
    case(MAC, "abˇc", "ctrl-o", "abˇ\nc"),
    // Windows and Linux: Ctrl-Up goes to the start of the line, then of the
    // one above. Ctrl-Down goes to the next line's start on Windows and to
    // the line's end on Linux.
    case(PC, "ab\ncdˇe\nf", "ctrl-up", "ab\nˇcde\nf"),
    case(PC, "ab\ncdˇe\nf", "ctrl-up ctrl-up", "ˇab\ncde\nf"),
    case(WIN, "aˇb\ncde\nf", "ctrl-down", "ab\nˇcde\nf"),
    case(WIN, "aˇb\ncde\nf", "ctrl-down ctrl-down", "ab\ncde\nˇf"),
    case(LINUX, "aˇb\ncde\nf", "ctrl-down", "abˇ\ncde\nf"),
    case(LINUX, "aˇb\ncde\nf", "ctrl-down ctrl-down", "ab\ncdeˇ\nf"),
    case(PC, "ab\ncdˇe", "ctrl-shift-up", "ab\n«ˇcd»e"),
    case(WIN, "aˇb\ncd", "ctrl-shift-down", "a«b\n»cd"),
    case(LINUX, "aˇb\ncd", "ctrl-shift-down", "a«b»\ncd"),
    // Windows and Linux: Ctrl to the ends of the text.
    case(PC, "ab\ncˇd", "ctrl-home", "ˇab\ncd"),
    case(PC, "ab\ncˇd", "ctrl-end", "ab\ncdˇ"),
    case(PC, "ab\ncˇd", "ctrl-shift-home", "«ˇab\nc»d"),
    case(PC, "ab\ncˇd", "ctrl-shift-end", "ab\nc«d»"),
    // Selecting by character with Alt on Windows.
    case(WIN, "aˇbc", "shift-alt-right", "a«b»c"),
    case(WIN, "abˇc", "shift-alt-left", "a«ˇb»c"),
    // Adding cursors, then typing at each.
    case(MAC, "ab\naˇb", "cmd-alt-up x", "axb\naxb"),
    case(MAC, "aˇb\nab", "cmd-alt-down x", "axb\naxb"),
    case(WIN, "ab\naˇb", "ctrl-alt-up x", "axb\naxb"),
    case(WIN, "aˇb\nab", "ctrl-alt-down x", "axb\naxb"),
    case(LINUX, "ab\naˇb", "shift-alt-up x", "axb\naxb"),
    case(LINUX, "aˇb\nab", "shift-alt-down x", "axb\naxb"),
    // Moving by word on macOS and Linux: right to the end of a word, left to
    // its start, across line breaks. An underscore and letters beyond ASCII
    // join a word, and punctuation is a word of its own.
    case(MAC, "ˇis_ok, café\nnaïve end", "alt-right", "is_okˇ, café\nnaïve end"),
    case(MAC, "is_okˇ, café\nnaïve end", "alt-right", "is_ok,ˇ café\nnaïve end"),
    case(MAC, "is_ok,ˇ café\nnaïve end", "alt-right", "is_ok, caféˇ\nnaïve end"),
    case(MAC, "is_ok, caféˇ\nnaïve end", "alt-right", "is_ok, café\nnaïveˇ end"),
    case(MAC, "is_ok, café\nnaïve endˇ", "alt-left", "is_ok, café\nnaïve ˇend"),
    case(MAC, "is_ok, café\nnaïve ˇend", "alt-left", "is_ok, café\nˇnaïve end"),
    case(MAC, "is_ok, café\nˇnaïve end", "alt-left", "is_ok, ˇcafé\nnaïve end"),
    case(MAC, "is_ok, ˇcafé\nnaïve end", "alt-left", "is_okˇ, café\nnaïve end"),
    case(MAC, "is_okˇ, café\nnaïve end", "alt-left", "ˇis_ok, café\nnaïve end"),
    case(LINUX, "ˇis_ok, café\nnaïve end", "ctrl-right", "is_okˇ, café\nnaïve end"),
    case(LINUX, "is_okˇ, café\nnaïve end", "ctrl-right", "is_ok,ˇ café\nnaïve end"),
    case(LINUX, "is_ok,ˇ café\nnaïve end", "ctrl-right", "is_ok, caféˇ\nnaïve end"),
    case(LINUX, "is_ok, caféˇ\nnaïve end", "ctrl-right", "is_ok, café\nnaïveˇ end"),
    case(LINUX, "is_ok, café\nnaïve endˇ", "ctrl-left", "is_ok, café\nnaïve ˇend"),
    case(LINUX, "is_ok, café\nnaïve ˇend", "ctrl-left", "is_ok, café\nˇnaïve end"),
    case(LINUX, "is_ok, café\nˇnaïve end", "ctrl-left", "is_ok, ˇcafé\nnaïve end"),
    case(LINUX, "is_ok, ˇcafé\nnaïve end", "ctrl-left", "is_okˇ, café\nnaïve end"),
    case(LINUX, "is_okˇ, café\nnaïve end", "ctrl-left", "ˇis_ok, café\nnaïve end"),
    case(MAC, "is_ok,ˇ café\nnaïve end", "alt-shift-right", "is_ok,« café»\nnaïve end"),
    case(LINUX, "is_ok,ˇ café\nnaïve end", "ctrl-shift-right", "is_ok,« café»\nnaïve end"),
    case(LINUX, "is_ok,ˇ café\nnaïve end", "alt-shift-right", "is_ok,« café»\nnaïve end"),
    case(MAC, "is_ok, café\nˇnaïve end", "alt-shift-left", "is_ok, «ˇcafé\n»naïve end"),
    case(LINUX, "is_ok, café\nˇnaïve end", "ctrl-shift-left", "is_ok, «ˇcafé\n»naïve end"),
    case(LINUX, "is_ok, café\nˇnaïve end", "alt-shift-left", "is_ok, «ˇcafé\n»naïve end"),
    case(MAC, "is_ok, ˇcafé\nnaïve end", "alt-backspace", "is_okˇcafé\nnaïve end"),
    case(MAC, "is_ok, café\nˇnaïve end", "alt-backspace", "is_ok, ˇnaïve end"),
    case(LINUX, "is_ok, café\nˇnaïve end", "ctrl-backspace", "is_ok, ˇnaïve end"),
    case(MAC, "is_ok,ˇ café\nnaïve end", "alt-delete", "is_ok,ˇ\nnaïve end"),
    case(LINUX, "is_ok,ˇ café\nnaïve end", "ctrl-delete", "is_ok,ˇ\nnaïve end"),
    case(LINUX, "is_ok, caféˇ\nnaïve end", "ctrl-delete", "is_ok, caféˇ end"),
    // Moving by word on Windows: to the start of each word, punctuation
    // included, and to the start and end of each line.
    case(WIN, "ˇis_ok, café\nnaïve end", "ctrl-right", "is_okˇ, café\nnaïve end"),
    case(WIN, "is_okˇ, café\nnaïve end", "ctrl-right", "is_ok, ˇcafé\nnaïve end"),
    case(WIN, "is_ok, ˇcafé\nnaïve end", "ctrl-right", "is_ok, caféˇ\nnaïve end"),
    case(WIN, "is_ok, caféˇ\nnaïve end", "ctrl-right", "is_ok, café\nˇnaïve end"),
    case(WIN, "is_ok, café\nˇnaïve end", "ctrl-right", "is_ok, café\nnaïve ˇend"),
    case(WIN, "is_ok, café\nnaïve ˇend", "ctrl-right", "is_ok, café\nnaïve endˇ"),
    case(WIN, "is_ok, café\nnaïve endˇ", "ctrl-left", "is_ok, café\nnaïve ˇend"),
    case(WIN, "is_ok, café\nnaïve ˇend", "ctrl-left", "is_ok, café\nˇnaïve end"),
    case(WIN, "is_ok, café\nˇnaïve end", "ctrl-left", "is_ok, caféˇ\nnaïve end"),
    case(WIN, "is_ok, caféˇ\nnaïve end", "ctrl-left", "is_ok, ˇcafé\nnaïve end"),
    case(WIN, "is_ok, ˇcafé\nnaïve end", "ctrl-left", "is_okˇ, café\nnaïve end"),
    case(WIN, "is_okˇ, café\nnaïve end", "ctrl-left", "ˇis_ok, café\nnaïve end"),
    case(WIN, "  ˇindented", "ctrl-left", "ˇ  indented"),
    case(WIN, "ˇ  indented", "ctrl-right", "  ˇindented"),
    case(WIN, "is_ok,ˇ café\nnaïve end", "ctrl-shift-right", "is_ok,« »café\nnaïve end"),
    case(WIN, "is_ok, café\nˇnaïve end", "ctrl-shift-left", "is_ok, café«ˇ\n»naïve end"),
    case(WIN, "is_ok, ˇcafé\nnaïve end", "ctrl-backspace", "is_okˇcafé\nnaïve end"),
    case(WIN, "is_ok, café\nˇnaïve end", "ctrl-backspace", "is_ok, caféˇnaïve end"),
    case(WIN, "is_ok,ˇ café\nnaïve end", "ctrl-delete", "is_ok,ˇcafé\nnaïve end"),
    case(WIN, "is_ok, caféˇ\nnaïve end", "ctrl-delete", "is_ok, caféˇnaïve end"),
    case(WIN, "ˇab cd\nab cd", "ctrl-alt-down ctrl-right x", "ab xcd\nab xcd"),
    // A CRLF line break is one stop, before its \r.
    case(WIN, "abˇ\r\ncd", "ctrl-right", "ab\r\nˇcd"),
    case(WIN, "ab\r\nˇcd", "ctrl-left", "abˇ\r\ncd"),
    case(WIN, "ab\r\nˇcd", "ctrl-backspace", "abˇcd"),
    case(WIN, "aˇb\r\ncd", "ctrl-down", "ab\r\nˇcd"),
    case(LINUX, "aˇb\r\ncd", "ctrl-down ctrl-down", "ab\r\ncdˇ"),
    case(MAC, "ab\r\ncˇd", "alt-up alt-up", "ˇab\r\ncd"),
];

/// Keys tested on their own below: they need long text, the scroll position
/// or the kill buffer.
const ELSEWHERE: &[(&[KeymapPlatform], &str)] = &[
    (PC, "pageup pagedown shift-pageup shift-pagedown"),
    (
        MAC,
        "pageup pagedown shift-pageup shift-pagedown home end alt-pageup alt-pagedown \
         ctrl-v ctrl-shift-v ctrl-l ctrl-k ctrl-y",
    ),
];

/// Keys whose action works outside the text, such as opening the search
/// panel, with the action each binds.
const OUTSIDE: &[(&[KeymapPlatform], &str, &str)] = &[
    (ALL, "escape", "Escape"),
    (ALL, "secondary-enter", "Enter"),
    (ALL, "shift-f10", "ShowContextMenu"),
    (ALL, "menu", "ShowContextMenu"),
    (MAC, "cmd-f", "Search"),
    (PC, "ctrl-f", "Search"),
    (MAC, "cmd-alt-f", "Replace"),
    (MAC, "cmd-shift-f", "Replace"),
    (PC, "ctrl-h", "Replace"),
    (MAC, "cmd-.", "ToggleCodeActions"),
    (PC, "ctrl-.", "ToggleCodeActions"),
    (MAC, "ctrl-cmd-space", "ShowCharacterPalette"),
];

/// The keys whose meaning differs between the platforms, and what each binds
/// on macOS, Windows and Linux. `None` leaves the key free.
#[rustfmt::skip]
const DIFFERENCES: &[(&str, [Option<&str>; 3])] = &[
    ("cmd-z", [Some("Undo"), None, None]),
    ("ctrl-z", [None, Some("Undo"), Some("Undo")]),
    ("ctrl-a", [Some("MoveHome"), Some("SelectAll"), Some("SelectAll")]),
    ("ctrl-f", [Some("MoveRight"), Some("Search"), Some("Search")]),
    ("ctrl-h", [Some("Backspace"), Some("Replace"), Some("Replace")]),
    ("ctrl-v", [Some("MovePageDown"), Some("Paste"), Some("Paste")]),
    ("ctrl-y", [Some("Yank"), Some("Redo"), Some("Redo")]),
    ("ctrl-k", [Some("KillToEndOfLine"), None, None]),
    ("ctrl-t", [Some("TransposeCharacters"), None, None]),
    ("cmd-shift-z", [Some("Redo"), None, None]),
    ("ctrl-shift-z", [None, Some("Redo"), Some("Redo")]),
    ("home", [Some("ScrollToStart"), Some("MoveHome"), Some("MoveHome")]),
    ("end", [Some("ScrollToEnd"), Some("MoveEnd"), Some("MoveEnd")]),
    ("pageup", [Some("ScrollPageUp"), Some("MovePageUp"), Some("MovePageUp")]),
    ("pagedown", [Some("ScrollPageDown"), Some("MovePageDown"), Some("MovePageDown")]),
    ("shift-home", [Some("SelectToStart"), Some("SelectToStartOfLine"), Some("SelectToStartOfLine")]),
    ("ctrl-home", [None, Some("MoveToStart"), Some("MoveToStart")]),
    ("cmd-up", [Some("MoveToStart"), None, None]),
    ("cmd-left", [Some("MoveHome"), None, None]),
    ("alt-left", [Some("MoveToPreviousWord"), None, None]),
    ("ctrl-left", [None, Some("MoveToWordStartLeft"), Some("MoveToPreviousWord")]),
    ("ctrl-right", [None, Some("MoveToWordStartRight"), Some("MoveToNextWord")]),
    ("ctrl-backspace", [Some("Backspace"), Some("DeleteToWordStartLeft"), Some("DeleteToPreviousWordStart")]),
    ("ctrl-delete", [None, Some("DeleteToWordStartRight"), Some("DeleteToNextWordEnd")]),
    ("alt-backspace", [Some("DeleteToPreviousWordStart"), Some("Undo"), None]),
    ("alt-up", [Some("MoveBackwardToLineStart"), None, None]),
    ("ctrl-up", [None, Some("MoveBackwardToLineStart"), Some("MoveBackwardToLineStart")]),
    ("ctrl-down", [None, Some("MoveForwardToLineStart"), Some("MoveForwardToLineEnd")]),
    ("shift-delete", [Some("Delete"), Some("Cut"), Some("Cut")]),
    ("ctrl-insert", [None, Some("Copy"), Some("Copy")]),
    ("shift-insert", [None, Some("Paste"), Some("Paste")]),
    ("insert", [None, None, None]),
    ("alt-shift-left", [Some("SelectToPreviousWordStart"), Some("SelectLeft"), Some("SelectToPreviousWordStart")]),
    ("cmd-alt-up", [Some("AddCursorAbove"), None, None]),
    ("ctrl-alt-up", [None, Some("AddCursorAbove"), None]),
    ("shift-alt-up", [Some("SelectBackwardToLineStart"), None, Some("AddCursorAbove")]),
    ("cmd-alt-f", [Some("Replace"), None, None]),
];

/// The name of the action `keys` binds on `platform`, without its namespace.
fn action(platform: KeymapPlatform, keys: &str) -> Option<&'static str> {
    let keys = Keystroke::parse(keys).unwrap();
    bindings(platform)
        .iter()
        .find(|binding| binding.keystrokes()[0].inner() == &keys)
        .map(|binding| binding.action().name().rsplit("::").next().unwrap())
}

/// `keys` written the way a binding's keystroke unparses.
fn canonical(keys: &str) -> String {
    Keystroke::parse(keys).unwrap().unparse()
}

/// A menu shows an action's latest binding, which is the platform's usual
/// shortcut where a second key does the same.
#[test]
fn menus_show_each_platforms_usual_shortcut() {
    let shown = |platform: KeymapPlatform, action: &str| {
        bindings(platform)
            .iter()
            .rev()
            .find(|binding| binding.action().name().rsplit("::").next() == Some(action))
            .map(|binding| binding.keystrokes()[0].inner().unparse())
    };
    let actions = ["Cut", "Copy", "Paste", "Undo", "Redo", "SelectAll"];
    for (platform, expected) in [
        (MacOS, "cmd-x cmd-c cmd-v cmd-z cmd-shift-z cmd-a"),
        (Windows, "ctrl-x ctrl-c ctrl-v ctrl-z ctrl-y ctrl-a"),
        (Linux, "ctrl-x ctrl-c ctrl-v ctrl-z ctrl-shift-z ctrl-a"),
    ] {
        for (action, keys) in actions.into_iter().zip(expected.split(' ')) {
            assert_eq!(
                shown(platform, action),
                Some(canonical(keys)),
                "{action} on {platform:?}"
            );
        }
    }
}

#[test]
fn no_platform_binds_a_key_twice() {
    for platform in KeymapPlatform::ALL {
        let mut seen = HashSet::new();
        for binding in bindings(platform) {
            let keys = binding.keystrokes()[0].inner().unparse();
            assert!(seen.insert(keys.clone()), "{platform:?} binds {keys} twice");
        }
    }
}

#[test]
fn the_platforms_differ_where_their_text_views_do() {
    for (keys, actions) in DIFFERENCES {
        for (platform, expected) in KeymapPlatform::ALL.into_iter().zip(actions) {
            assert_eq!(action(platform, keys), *expected, "{keys} on {platform:?}");
        }
    }
}

/// Every key in each platform's table is in a case, tested on its own, or
/// works outside the text and binds the action it should.
#[test]
fn every_binding_is_tested() {
    for platform in KeymapPlatform::ALL {
        let mut tested: HashSet<String> = HashSet::new();
        let pressed = CASES
            .iter()
            .filter(|case| case.on.contains(&platform))
            .map(|case| case.keys)
            .chain(
                ELSEWHERE
                    .iter()
                    .filter(|(on, _)| on.contains(&platform))
                    .map(|(_, keys)| *keys),
            );
        for keys in pressed {
            tested.extend(keys.split_whitespace().map(canonical));
        }
        for (on, keys, name) in OUTSIDE {
            if on.contains(&platform) {
                assert_eq!(
                    action(platform, keys),
                    Some(*name),
                    "{keys} on {platform:?}"
                );
                tested.insert(canonical(keys));
            }
        }

        let untested: Vec<String> = bindings(platform)
            .iter()
            .map(|binding| binding.keystrokes()[0].inner().unparse())
            .filter(|keys| !tested.contains(keys))
            .collect();
        assert!(
            untested.is_empty(),
            "untested on {platform:?}: {untested:?}"
        );
    }
}

#[gpui::test]
fn every_key_does_what_its_platform_does(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    for case in CASES {
        for &platform in case.on {
            let mut test = KeymapTest::new(cx, Keymap::Cua, platform, case.before);
            test.keys(case.keys);
            let actual = test.marked();
            let expected = parse(case.after);
            let matches = if expected.selection.is_some() {
                parse(&actual) == expected
            } else {
                parse(&actual).text == expected.text
            };
            if !matches {
                failures.push(format!(
                    "{platform:?} {:?} in {:?}\n    actual: {actual:?}\n  expected: {:?}",
                    case.keys, case.before, case.after
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A hundred numbered lines, for keys that move by the page.
fn long_text() -> String {
    (0..100)
        .map(|row| format!("line {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The caret's row, the selection's start row and the scroll offset.
fn position(test: &mut KeymapTest) -> (usize, usize, f32) {
    test.update(|state, _, _| {
        let row = |offset| state.text.offset_to_point(offset).row;
        let range = state.selected_range();
        (
            row(state.cursor()),
            row(range.start),
            f32::from(state.scroll_offset().y),
        )
    })
}

#[gpui::test]
fn page_keys_move_the_caret_on_windows_and_linux(cx: &mut TestAppContext) {
    for platform in PC {
        let text = long_text();
        let mut test = KeymapTest::new(cx, Keymap::Cua, *platform, &format!("ˇ{text}"));
        test.keys("pagedown");
        let (page, _, scroll) = position(&mut test);
        assert!(page > 5, "{platform:?}: a page is more than a few rows");
        assert!(scroll < 0., "{platform:?}: the caret stays in view");
        test.keys("pagedown pageup");
        assert_eq!(position(&mut test).0, page, "{platform:?}");
        test.keys("pageup");
        assert_eq!(position(&mut test).0, 0, "{platform:?}");

        test.keys("shift-pagedown");
        assert_eq!(position(&mut test).0, page, "{platform:?}");
        assert_eq!(
            position(&mut test).1,
            0,
            "{platform:?}: the selection keeps its start"
        );
        test.keys("shift-pagedown shift-pageup");
        assert_eq!(position(&mut test).0, page, "{platform:?}");
        assert_eq!(position(&mut test).1, 0, "{platform:?}");
    }
}

/// On macOS, Home, End, Page Up and Page Down scroll and leave the caret
/// where it is. Option or Control move it a page, and Shift selects.
#[gpui::test]
fn macos_page_keys_scroll_without_moving_the_caret(cx: &mut TestAppContext) {
    let text = long_text();
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, &format!("ˇ{text}"));
    test.keys("pagedown");
    let (row, _, page) = position(&mut test);
    assert_eq!(row, 0, "the caret stays");
    assert!(page < 0., "the text scrolled");
    test.keys("pagedown");
    assert!(position(&mut test).2 < page, "a second page");
    test.keys("pageup pageup");
    assert_eq!(position(&mut test), (0, 0, 0.));

    test.keys("end");
    let (row, _, bottom) = position(&mut test);
    assert_eq!(row, 0, "the caret stays");
    test.keys("pagedown");
    assert_eq!(position(&mut test).2, bottom, "End scrolled to the bottom");
    test.keys("home");
    assert_eq!(position(&mut test), (0, 0, 0.));

    test.keys("alt-pagedown");
    let (page_rows, _, _) = position(&mut test);
    assert!(page_rows > 5, "Option-Page Down moves the caret a page");
    test.keys("alt-pageup");
    assert_eq!(position(&mut test).0, 0);
    test.keys("ctrl-v");
    assert_eq!(position(&mut test).0, page_rows, "Ctrl-V moves a page down");

    test.update(|state, _, cx| state.set_selected_range(0..0, cx));
    test.keys("shift-pagedown");
    assert_eq!(position(&mut test).0, page_rows);
    assert_eq!(position(&mut test).1, 0);
    test.keys("ctrl-shift-v");
    assert_eq!(position(&mut test).0, 2 * page_rows);
    test.keys("shift-pageup");
    assert_eq!(position(&mut test).0, page_rows);
    assert_eq!(position(&mut test).1, 0);
}

/// Ctrl-L on macOS scrolls the caret's line to the middle of the view.
#[gpui::test]
fn ctrl_l_centers_the_caret(cx: &mut TestAppContext) {
    let text = long_text();
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, &format!("ˇ{text}"));
    let line_50 = text.find("line 50").unwrap();
    test.update(|state, _, cx| state.set_selected_range(line_50..line_50, cx));
    test.keys("home ctrl-l");
    let (caret_y, height, line_height) = test.update(|state, _, _| {
        let line_height = state.last_layout.as_ref().unwrap().line_height;
        (
            f32::from(line_height * 50usize + state.scroll_offset().y),
            f32::from(state.input_bounds.size.height),
            f32::from(line_height),
        )
    });
    let middle = (height - line_height) / 2.;
    assert!(
        (caret_y - middle).abs() < line_height,
        "the caret's line is at {caret_y}, the middle at {middle}"
    );
}

/// Ctrl-K on macOS cuts to the end of the line into the kill buffer, and
/// kills in a row add to it. Ctrl-Y puts it back. The clipboard keeps what
/// it had.
#[gpui::test]
fn ctrl_k_kills_and_ctrl_y_yanks(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, "ˇone two\nthree");
    test.cx
        .write_to_clipboard(ClipboardItem::new_string("clipboard".into()));
    test.keys("ctrl-k");
    test.assert("ˇ\nthree");
    test.keys("ctrl-k");
    test.assert("ˇthree");
    test.keys("ctrl-y");
    test.assert("one two\nˇthree");
    test.keys("ctrl-e ctrl-y");
    test.assert("one two\nthreeone two\nˇ");
    assert_eq!(
        test.cx.read_from_clipboard().and_then(|item| item.text()),
        Some("clipboard".into())
    );

    // A kill after moving starts the buffer afresh. A selection is killed
    // whole.
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, "aˇb cd");
    test.keys("ctrl-k left ctrl-k");
    test.assert("ˇ");
    test.keys("ctrl-y");
    test.assert("aˇ");
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, "a«bc»d");
    test.keys("ctrl-k");
    test.assert("aˇd");
    test.keys("ctrl-y ctrl-y");
    test.assert("abcbcˇd");

    // Nothing to kill at the end of the text: the buffer keeps its text.
    test.keys("cmd-down ctrl-k ctrl-y");
    test.assert("abcbcdbcˇ");

    // Undo takes back a kill and a yank one at a time.
    test.keys("cmd-z");
    test.assert("abcbcdˇ");
}

/// The find and replace keys open a searchable textarea's search panel,
/// Replace with its replace field.
#[gpui::test]
fn find_and_replace_keys_open_the_search_panel(cx: &mut TestAppContext) {
    for platform in KeymapPlatform::ALL {
        let (find, replace): (&str, &[&str]) = if platform.is_macos() {
            ("cmd-f", &["cmd-alt-f", "cmd-shift-f"])
        } else {
            ("ctrl-f", &["ctrl-h"])
        };
        for keys in std::iter::once(find).chain(replace.iter().copied()) {
            let mut test = KeymapTest::new(cx, Keymap::Cua, platform, "ˇfind me");
            test.update(|state, _, cx| state.set_searchable(true, cx));
            test.keys(keys);
            let (open, replace_mode) = test.update(|state, _, _| {
                let session = state.search_session();
                (session.open, session.replace_mode)
            });
            assert!(open, "{keys} on {platform:?}");
            assert_eq!(replace_mode, keys != find, "{keys} on {platform:?}");
        }
    }
}

/// A CUA key does nothing of CUA's while the textarea follows another
/// scheme, and works again once it follows CUA.
#[gpui::test]
fn cua_keys_stay_out_of_the_other_schemes(cx: &mut TestAppContext) {
    for keymap in [Keymap::Emacs, Keymap::Vim] {
        for platform in KeymapPlatform::ALL {
            let (keys, before, after) = if platform.is_macos() {
                ("alt-up", "ab\ncdˇ", "ab\nˇcd")
            } else {
                // The caret starts on a character, where Vim's normal mode
                // leaves it.
                ("ctrl-shift-z", "aˇb", "axˇb")
            };
            let mut test = KeymapTest::new(cx, keymap, platform, before);
            test.update(|state, window, cx| state.insert("x", window, cx));
            test.dispatch(Undo);
            test.keys(keys);
            // Ctrl-Shift-Z would redo the edit, Option-Up move the caret.
            let (actual, cua) = (parse(&test.marked()), parse(after));
            let fired = if platform.is_macos() {
                actual.selection == cua.selection
            } else {
                actual.text == cua.text
            };
            assert!(!fired, "{keys} in {keymap:?} on {platform:?}");

            test.update(|state, _, cx| state.set_keymap(Keymap::Cua, cx));
            test.keys(keys);
            assert_eq!(
                parse(&test.marked()),
                parse(after),
                "{keys} in CUA on {platform:?}"
            );
        }
    }
}

struct InputHarness<M: InputModeKind> {
    input: Entity<InputBaseState<M>>,
}

impl<M: InputModeKind> Render for InputHarness<M> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().h(px(300.)).child(self.input.clone()))
    }
}

/// A focused input of mode `M` holding `text` with the caret at `caret`, and
/// `platform`'s tables bound.
fn open_input<M: InputModeKind>(
    cx: &mut TestAppContext,
    platform: KeymapPlatform,
    text: &str,
    caret: usize,
    new: fn(&mut Window, &mut Context<InputBaseState<M>>) -> InputBaseState<M>,
) -> (VisualTestContext, Entity<InputBaseState<M>>) {
    cx.update(|cx| {
        crate::init(cx);
        cx.clear_key_bindings();
        for keymap in Keymap::ALL {
            cx.bind_keys(keymap.bindings(platform));
        }
    });
    let text = text.to_string();
    let window = cx.open_window(size(px(600.), px(400.)), move |window, cx| InputHarness {
        input: cx.new(|cx| new(window, cx).default_value(text)),
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let input = window
        .read_with(&cx, |harness, _| harness.input.clone())
        .unwrap();
    cx.update(|window, cx| {
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.set_selected_range(caret..caret, cx);
        })
    });
    cx.run_until_parked();
    (cx, input)
}

/// Single-line inputs and code editors follow CUA too, and its commands
/// work in them. The kill buffer is the app's, shared by every input.
#[gpui::test]
fn cua_commands_work_in_inputs_and_editors(cx: &mut TestAppContext) {
    let (mut input_cx, input) = open_input(cx, Windows, "hello, world", 0, |window, cx| {
        crate::input::InputState::new(window, cx)
    });
    input_cx.simulate_keystrokes("ctrl-right ctrl-right");
    input.read_with(&input_cx, |state, _| assert_eq!(state.cursor(), 7));
    input_cx.simulate_keystrokes("ctrl-delete");
    input.read_with(&input_cx, |state, _| {
        assert_eq!(state.value().as_ref(), "hello, ")
    });

    let (mut editor_cx, editor) =
        open_input(cx, Windows, "fn main() {\n    body\n}", 0, |window, cx| {
            crate::input::EditorState::new(window, cx)
        });
    editor_cx.simulate_keystrokes("ctrl-down ctrl-right");
    editor.read_with(&editor_cx, |state, _| assert_eq!(state.cursor(), 16));

    let (mut input_cx, input) = open_input(cx, MacOS, "kill me", 5, |window, cx| {
        crate::input::InputState::new(window, cx)
    });
    input_cx.simulate_keystrokes("ctrl-k");
    input.read_with(&input_cx, |state, _| {
        assert_eq!(state.value().as_ref(), "kill ")
    });
    let mut test = KeymapTest::new(cx, Keymap::Cua, MacOS, "ˇ!");
    test.keys("ctrl-y");
    test.assert("meˇ!");
}
