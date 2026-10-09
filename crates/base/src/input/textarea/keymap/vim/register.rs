//! Registers, shared by every textarea in the app as Vim's are by its
//! buffers.
//!
//! - `"` the unnamed register: what the last yank, delete or change stored.
//! - `a`–`z`, appended to with `A`–`Z`.
//! - `0` the last yank, `1`–`9` the last deletes of a line or more, `-` the
//!   last smaller delete.
//! - `_` the black hole: nothing is stored.
//! - `+` and `*` the system clipboard.
//! - `.` the last inserted text, `/` the last search and `:` the last command
//!   line, read only.

use std::collections::HashMap;

use gpui::{App, ClipboardItem, Global};

/// How a register's text goes back in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RegisterKind {
    /// Within a line, at the caret.
    #[default]
    Chars,
    /// Whole lines, above or below the caret's. The text ends with a line
    /// break.
    Lines,
    /// A block of columns, one line of the text per row.
    Block,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Register {
    pub(super) text: String,
    pub(super) kind: RegisterKind,
}

impl Register {
    pub(super) fn new(text: impl Into<String>, kind: RegisterKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }
}

/// What put text in a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Store {
    Yank,
    /// A delete or change. `big` when it spans lines or used a jump motion,
    /// which puts it in register 1.
    Delete {
        big: bool,
    },
}

#[derive(Default)]
struct Registers {
    slots: HashMap<char, Register>,
}

impl Global for Registers {}

/// Whether `name` names a register a command can use.
pub(super) fn is_valid(name: char) -> bool {
    name.is_ascii_alphanumeric() || "\"-_+*./:".contains(name)
}

/// Store `register` as `store` does, in the register `name` names, or as
/// Vim does without one.
pub(super) fn store(name: Option<char>, store: Store, register: Register, cx: &mut App) {
    let name = name.unwrap_or('"');
    if name == '_' {
        return;
    }
    if matches!(name, '+' | '*') {
        cx.write_to_clipboard(ClipboardItem::new_string(register.text.clone()));
    }
    let registers = cx.default_global::<Registers>();
    let slots = &mut registers.slots;
    let unnamed = match name {
        'a'..='z' | '0'..='9' | '-' => {
            slots.insert(name, register.clone());
            register.clone()
        }
        'A'..='Z' => {
            let lower = name.to_ascii_lowercase();
            let appended = match slots.remove(&lower) {
                Some(existing) => append(existing, register.clone()),
                None => register.clone(),
            };
            slots.insert(lower, appended.clone());
            appended
        }
        _ => register.clone(),
    };
    let named = name != '"';
    match store {
        Store::Yank if !named => {
            slots.insert('0', register.clone());
        }
        Store::Yank => {}
        Store::Delete { big } => {
            if big {
                for ix in (1..9).rev() {
                    let from = char::from(b'0' + ix);
                    if let Some(moved) = slots.remove(&from) {
                        slots.insert(char::from(b'0' + ix + 1), moved);
                    }
                }
                slots.insert('1', register.clone());
            } else if !named {
                slots.insert('-', register.clone());
            }
        }
    }
    slots.insert('"', unnamed);
}

/// `existing` with `added` appended, as `"A` does. A linewise part makes the
/// whole register linewise.
fn append(existing: Register, added: Register) -> Register {
    let lines = existing.kind == RegisterKind::Lines || added.kind == RegisterKind::Lines;
    if !lines {
        return Register::new(existing.text + &added.text, existing.kind);
    }
    let mut text = existing.text;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&added.text);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Register::new(text, RegisterKind::Lines)
}

/// The register `name` names, or the unnamed one. `+` and `*` read the
/// clipboard, linewise when the text ends with a line break.
pub(super) fn read(name: Option<char>, cx: &mut App) -> Option<Register> {
    let name = name.unwrap_or('"').to_ascii_lowercase();
    if name == '_' {
        return None;
    }
    if matches!(name, '+' | '*') {
        let text = cx.read_from_clipboard()?.text()?;
        let kind = if text.ends_with('\n') {
            RegisterKind::Lines
        } else {
            RegisterKind::Chars
        };
        return Some(Register::new(text, kind));
    }
    cx.default_global::<Registers>().slots.get(&name).cloned()
}
