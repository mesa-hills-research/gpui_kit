//! The command line: `:` for Ex commands, `/` and `?` for searches.
//!
//! What is typed shows as the mode label, such as `:s/a/b/`, so an
//! application that shows the label shows the command line too.
//!
//! Ex commands handled here: `:{n}` and `:$` go to a line, `:s` substitutes,
//! `:d` and `:y` delete and yank lines, `:noh` clears the search highlight,
//! `:u` and `:red` undo and redo. `:w`, `:q`, `:wq` and `:x` dispatch
//! [`VimWrite`] and [`VimQuit`] for the application, and any other command
//! is dispatched as [`VimCommand`].

use std::ops::Range;

use gpui::{Action, Context, SharedString, Window};

use super::{
    VimState,
    motion::Motion,
    operator::{Operator, Region, edit},
    search::{self, LastSearch},
    text,
};
use crate::input::TextareaState;

/// `:w` in Vim: the user asked to save. Dispatched from the textarea, so it
/// reaches the application's handler on any element around it.
#[derive(Action, Clone, Debug, Default, PartialEq, Eq)]
#[action(namespace = vim, name = "Write")]
pub struct VimWrite;

/// `:q` in Vim: the user asked to close what they are editing. `:wq` and
/// `:x` dispatch [`VimWrite`] and then this.
#[derive(Action, Clone, Debug, Default, PartialEq, Eq)]
#[action(namespace = vim, name = "Quit")]
pub struct VimQuit;

/// An Ex command Vim does not handle itself, such as `:e notes.md`, for the
/// application to handle.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, name = "Command", no_json)]
pub struct VimCommand {
    /// The command as typed, without the `:`.
    pub command: SharedString,
}

/// What the command line is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandKind {
    Ex,
    Search { backward: bool },
}

/// The command line being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandLine {
    pub(super) kind: CommandKind,
    pub(super) text: String,
}

impl CommandLine {
    /// The line as shown, with its `:`, `/` or `?`.
    pub(super) fn label(&self) -> String {
        let prefix = match self.kind {
            CommandKind::Ex => ':',
            CommandKind::Search { backward: false } => '/',
            CommandKind::Search { backward: true } => '?',
        };
        format!("{prefix}{}", self.text)
    }
}

impl VimState {
    /// `:`, `/` and `?`.
    pub(super) fn open_command_line(
        &mut self,
        kind: CommandKind,
        state: &mut TextareaState,
        cx: &mut Context<TextareaState>,
    ) {
        let mut text = String::new();
        if kind == CommandKind::Ex {
            if self.visual.is_some() {
                // The command acts on the lines selected.
                self.leave_visual();
                self.enter_normal(state, cx);
                text.push_str("'<,'>");
            } else if let Some(count) = self.take_count() {
                text = if count > 1 {
                    format!(".,.+{}", count - 1)
                } else {
                    ".".into()
                };
            }
        }
        self.command_line = Some(CommandLine { kind, text });
    }

    pub(super) fn command_line_backspace(&mut self) {
        let Some(line) = &mut self.command_line else {
            return;
        };
        if line.text.pop().is_none() {
            self.cancel_command_line();
        }
    }

    /// Ctrl-U on the command line.
    pub(super) fn command_line_clear(&mut self) {
        if let Some(line) = &mut self.command_line {
            line.text.clear();
        }
    }

    /// Ctrl-W on the command line.
    pub(super) fn command_line_delete_word(&mut self) {
        if let Some(line) = &mut self.command_line {
            let trimmed = line.text.trim_end().len();
            line.text.truncate(trimmed);
            let keep = line
                .text
                .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                .map_or(0, |ix| ix + 1);
            let keep = if keep == line.text.len() {
                keep.saturating_sub(1)
            } else {
                keep
            };
            line.text.truncate(keep);
        }
    }

    /// Escape on the command line: forget it and what it was for.
    pub(super) fn cancel_command_line(&mut self) {
        self.command_line = None;
        self.reset_pending();
    }

    /// Enter on the command line.
    pub(super) fn execute_command_line(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let Some(line) = self.command_line.take() else {
            return;
        };
        match line.kind {
            CommandKind::Search { backward } => {
                let pattern = if line.text.is_empty() {
                    match &self.last_search {
                        Some(search) => search.pattern.clone(),
                        None => {
                            self.reset_pending();
                            return;
                        }
                    }
                } else {
                    line.text
                };
                self.last_search = Some(LastSearch { pattern, backward });
                self.motion(Motion::SearchNext { reverse: false }, state, window, cx);
            }
            CommandKind::Ex => {
                self.reset_pending();
                self.last_command = Some(line.text.clone());
                self.run_ex(&line.text, state, window, cx);
            }
        }
    }

    /// Run an Ex command line.
    pub(super) fn run_ex(
        &mut self,
        line: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let line = line.trim_start_matches([':', ' ']);
        let Some((rows, rest)) = self.parse_range(line, state) else {
            return;
        };
        let rest = rest.trim_start();
        let name_len = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let name = &rest[..name_len];
        let args = rest[name_len..].trim_start_matches('!');
        let current = text::row(&state.text, state.cursor());
        let rows_or_current = rows.clone().unwrap_or(current..current + 1);
        match name {
            "" if args.trim().is_empty() => {
                if let Some(rows) = rows {
                    let row = (rows.end - 1).min(text::last_row(&state.text));
                    let caret = text::first_non_blank(&state.text, row);
                    self.enter_normal_at(caret, state, cx);
                }
            }
            "w" | "write" | "up" | "update" => window.dispatch_action(Box::new(VimWrite), cx),
            "q" | "quit" | "qa" | "qall" | "quita" | "quitall" | "clo" | "close" => {
                window.dispatch_action(Box::new(VimQuit), cx)
            }
            "wq" | "wqa" | "wqall" | "x" | "xi" | "xit" | "xa" | "xall" | "exi" | "exit" => {
                window.dispatch_action(Box::new(VimWrite), cx);
                window.dispatch_action(Box::new(VimQuit), cx);
            }
            "s" | "su" | "substitute" => self.substitute(rows_or_current, args, state, window, cx),
            "noh" | "nohl" | "nohlsearch" => state.close_search(cx),
            "u" | "un" | "undo" => self.undo_redo(true, state, window, cx),
            "red" | "redo" => self.undo_redo(false, state, window, cx),
            "d" | "de" | "del" | "delete" | "y" | "ya" | "yank" => {
                let operator = if name.starts_with('d') {
                    Operator::Delete
                } else {
                    Operator::Yank
                };
                let register = args.trim().chars().next();
                self.register = register.filter(|c| super::register::is_valid(*c));
                // `:y` leaves the caret where it is.
                let caret = state.cursor();
                self.apply_operator(
                    operator,
                    Region::Lines(rows_or_current),
                    caret,
                    true,
                    1,
                    state,
                    window,
                    cx,
                );
            }
            _ => window.dispatch_action(
                Box::new(VimCommand {
                    command: SharedString::new(line),
                }),
                cx,
            ),
        }
    }

    /// The rows an Ex range names, and the rest of the line. `None` rows
    /// when there is no range.
    fn parse_range<'a>(
        &self,
        line: &'a str,
        state: &TextareaState,
    ) -> Option<(Option<Range<usize>>, &'a str)> {
        let last_row = text::last_row(&state.text);
        if let Some(rest) = line.strip_prefix('%') {
            return Some((Some(0..last_row + 1), rest));
        }
        let Some((first, rest)) = self.parse_address(line, state) else {
            return Some((None, line));
        };
        let (second, rest) = match rest.strip_prefix([',', ';']) {
            Some(rest) => self.parse_address(rest, state)?,
            None => (first, rest),
        };
        let (start, end) = (first.min(second), first.max(second));
        Some((Some(start.min(last_row)..end.min(last_row) + 1), rest))
    }

    /// One Ex address: `.`, `$`, a line number, `'<` or `'>`, with `+n` and
    /// `-n` after it.
    fn parse_address<'a>(&self, line: &'a str, state: &TextareaState) -> Option<(usize, &'a str)> {
        let text = &state.text;
        let current = text::row(text, state.cursor());
        let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let (mut row, mut rest) = if let Some(rest) = line.strip_prefix('.') {
            (current as isize, rest)
        } else if let Some(rest) = line.strip_prefix('$') {
            (text::last_row(text) as isize, rest)
        } else if let Some(rest) = line.strip_prefix("'<") {
            let visual = self.last_visual?;
            (
                text::row(text, visual.anchor.min(visual.head)) as isize,
                rest,
            )
        } else if let Some(rest) = line.strip_prefix("'>") {
            let visual = self.last_visual?;
            (
                text::row(text, visual.anchor.max(visual.head)) as isize,
                rest,
            )
        } else if line.starts_with(|c: char| c.is_ascii_digit()) {
            let len = digits(line);
            (line[..len].parse::<isize>().ok()? - 1, &line[len..])
        } else if line.starts_with(['+', '-']) {
            (current as isize, line)
        } else {
            return None;
        };
        while let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
            let after = &rest[1..];
            let len = digits(after);
            let amount = if len == 0 {
                1
            } else {
                after[..len].parse::<isize>().ok()?
            };
            row += if sign == '+' { amount } else { -amount };
            rest = &after[len..];
        }
        Some((row.max(0) as usize, rest))
    }

    /// `:s/pattern/replacement/flags` on `rows`. The flags `g` (every match
    /// in a line), `i` and `I` (ignore case or not) apply.
    fn substitute(
        &mut self,
        rows: Range<usize>,
        args: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        let mut chars = args.chars();
        let Some(delimiter) = chars.next() else {
            return;
        };
        if delimiter.is_alphanumeric() || matches!(delimiter, '\\' | '"' | '|' | ' ') {
            return;
        }
        let rest: String = chars.collect();
        let mut parts = split_unescaped(&rest, delimiter).into_iter();
        let pattern = parts.next().unwrap_or_default();
        let replacement = parts.next().unwrap_or_default();
        let flags = parts.next().unwrap_or_default();
        let pattern = if pattern.is_empty() {
            match &self.last_search {
                Some(search) => search.pattern.clone(),
                None => return,
            }
        } else {
            pattern
        };
        let global = flags.contains('g');
        let pattern_with_case = if flags.contains('i') {
            format!("\\c{pattern}")
        } else if flags.contains('I') {
            format!("\\C{pattern}")
        } else {
            pattern.clone()
        };
        let Some(regex) = search::regex(&pattern_with_case) else {
            return;
        };
        let replacement = search::replacement(&replacement);
        let text = state.text.clone();
        let mut edits = Vec::new();
        for row in rows {
            let line = text::line_text(&text, row);
            let replaced = if global {
                regex.replace_all(&line, replacement.as_str())
            } else {
                regex.replacen(&line, 1, replacement.as_str())
            };
            if replaced != line {
                let range = text::line_start(&text, row)..text::line_end(&text, row);
                edits.push((range, replaced.into_owned()));
            }
        }
        self.last_search = Some(LastSearch {
            pattern,
            backward: false,
        });
        let Some((last_range, last_text)) = edits.last().cloned() else {
            return;
        };
        // The last line changed, where the edits before it moved it.
        let shift: isize = edits[..edits.len() - 1]
            .iter()
            .map(|(range, new)| new.len() as isize - range.len() as isize)
            .sum();
        edit(state, edits, window, cx);
        // On the last line of what replaced it, which a line break in the
        // replacement may have split.
        let end = (last_range.start as isize + shift).max(0) as usize + last_text.len();
        let row = text::row(&state.text, end);
        let caret = text::first_non_blank(&state.text, row);
        self.enter_normal_at(caret, state, cx);
    }
}

/// Split `s` at each `delimiter` not escaped with a backslash. An escaped
/// delimiter becomes the plain character.
fn split_unescaped(s: &str, delimiter: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next) if next == delimiter => parts.last_mut().unwrap().push(next),
                Some(next) => {
                    let part = parts.last_mut().unwrap();
                    part.push('\\');
                    part.push(next);
                }
                None => parts.last_mut().unwrap().push('\\'),
            }
        } else if c == delimiter {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}
