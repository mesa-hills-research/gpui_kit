//! Searching: `/`, `?`, `n`, `N`, `*` and `#`, and Vim's patterns, which
//! `:s` uses too.
//!
//! Patterns follow Vim's default "magic" syntax: `.`, `*`, `[]`, `^` and
//! `$` are special as they are, `\(`, `\)`, `\|`, `\+`, `\?`, `\=` and
//! `\{n,m}` with a backslash, `\<` and `\>` match word edges, and `\c` or
//! `\C` anywhere makes the search ignore case or not. `\v` switches to "very
//! magic", where every one of them is special without the backslash.
//! Matches of a plain pattern are highlighted with the textarea's search.

use std::ops::Range;

use gpui::Context;
use regex::{Regex, RegexBuilder};
use ropey::Rope;

use super::text;
use crate::input::TextareaState;

/// The last search, which `n` and `N` repeat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LastSearch {
    pub(super) pattern: String,
    pub(super) backward: bool,
}

/// A Vim pattern as a regex, or as plain text when it is not a valid one.
pub(super) fn regex(pattern: &str) -> Option<Regex> {
    let (source, case_insensitive) = translate(pattern);
    RegexBuilder::new(&source)
        .case_insensitive(case_insensitive)
        .multi_line(true)
        .build()
        .or_else(|_| {
            RegexBuilder::new(&regex::escape(pattern))
                .case_insensitive(case_insensitive)
                .build()
        })
        .ok()
}

/// Translate a Vim pattern to the regex crate's syntax. Returns it and
/// whether `\c` asked to ignore case.
fn translate(pattern: &str) -> (String, bool) {
    let mut out = String::new();
    let mut ignore_case = false;
    let mut very_magic = false;
    let mut chars = pattern.chars().peekable();
    // Copy a `[...]` class as it is.
    let class = |chars: &mut std::iter::Peekable<std::str::Chars>, out: &mut String| {
        let mut class = String::from("[");
        let mut first = true;
        while let Some(c) = chars.next() {
            if c == ']' && !first && !class.ends_with("[^") {
                class.push(']');
                out.push_str(&class);
                return;
            }
            if c == '[' && chars.peek() == Some(&':') {
                // A named class such as `[:alpha:]`, as it is.
                class.push(c);
                for c in chars.by_ref() {
                    class.push(c);
                    if c == ']' {
                        break;
                    }
                }
            } else if c == '[' {
                class.push_str("\\[");
            } else {
                class.push(c);
            }
            first = false;
        }
        // No closing bracket: the `[` was literal.
        out.push_str(&regex::escape(&class));
    };
    // `{n,m}` and Vim's lazy `{-n,m}`, after `\{` or `{`.
    let braces = |chars: &mut std::iter::Peekable<std::str::Chars>, out: &mut String| {
        let mut inner = String::new();
        let mut closed = false;
        while let Some(c) = chars.next() {
            if c == '}' {
                closed = true;
                break;
            }
            if c == '\\' && chars.peek() == Some(&'}') {
                chars.next();
                closed = true;
                break;
            }
            inner.push(c);
        }
        if !closed {
            out.push_str(&regex::escape(&format!("{{{inner}")));
            return;
        }
        let lazy = inner.starts_with('-');
        let inner = inner.trim_start_matches('-');
        let bounds = if inner.is_empty() {
            "*".to_string()
        } else if inner.contains(',') {
            let (min, max) = inner.split_once(',').unwrap_or_default();
            format!("{{{},{max}}}", if min.is_empty() { "0" } else { min })
        } else {
            format!("{{{inner}}}")
        };
        out.push_str(&bounds);
        if lazy {
            out.push('?');
        }
    };
    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(next) = chars.next() else {
                out.push_str("\\\\");
                break;
            };
            match next {
                'c' => ignore_case = true,
                'C' => ignore_case = false,
                'v' => very_magic = true,
                'm' | 'M' | 'V' => very_magic = false,
                '<' | '>' => out.push_str("\\b"),
                '(' | ')' | '|' | '+' | '?' if !very_magic => out.push(next),
                '=' if !very_magic => out.push('?'),
                '{' if !very_magic => braces(&mut chars, &mut out),
                's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'n' | 't' | 'r' => {
                    out.push('\\');
                    out.push(next);
                }
                'a' => out.push_str("[A-Za-z]"),
                'A' => out.push_str("[^A-Za-z]"),
                'l' => out.push_str("[a-z]"),
                'L' => out.push_str("[^a-z]"),
                'u' => out.push_str("[A-Z]"),
                'U' => out.push_str("[^A-Z]"),
                'x' => out.push_str("[0-9A-Fa-f]"),
                'h' => out.push_str("[A-Za-z_]"),
                other => out.push_str(&regex::escape(&other.to_string())),
            }
        } else if very_magic {
            match c {
                '<' | '>' => out.push_str("\\b"),
                '=' => out.push('?'),
                '{' => braces(&mut chars, &mut out),
                '[' => class(&mut chars, &mut out),
                '(' | ')' | '|' | '+' | '?' | '.' | '*' | '^' | '$' => out.push(c),
                other => out.push_str(&regex::escape(&other.to_string())),
            }
        } else {
            match c {
                '.' | '*' | '^' | '$' => out.push(c),
                '[' => class(&mut chars, &mut out),
                other => out.push_str(&regex::escape(&other.to_string())),
            }
        }
    }
    (out, ignore_case)
}

/// The pattern as plain text, when it matches only that text: what the
/// textarea's search can highlight. Word edges are dropped. Returns the text
/// and whether case is ignored.
fn plain(pattern: &str) -> Option<(String, bool)> {
    let mut text = String::new();
    let mut ignore_case = false;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '<' | '>' => {}
                'c' => ignore_case = true,
                'C' => ignore_case = false,
                c @ ('\\' | '/' | '.' | '*' | '[' | ']' | '~' | '^' | '$') => text.push(c),
                _ => return None,
            },
            '.' | '*' | '[' | '^' | '$' => return None,
            c => text.push(c),
        }
    }
    (!text.is_empty()).then_some((text, ignore_case))
}

/// Highlight the matches of `pattern` with the textarea's search, when it
/// can show them.
pub(super) fn highlight(pattern: &str, state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    match plain(pattern) {
        Some((text, ignore_case)) => state.set_search_query(text, ignore_case, cx),
        None => state.close_search(cx),
    }
}

/// Where the matches of `pattern` start.
fn match_starts(text: &Rope, pattern: &str) -> Vec<usize> {
    let Some(regex) = regex(pattern) else {
        return Vec::new();
    };
    let source = text.to_string();
    regex
        .find_iter(&source)
        .map(|found| found.start())
        .collect()
}

/// The start of the `count`-th match after `from`, or before it, wrapping
/// around the ends of the text.
pub(super) fn find(
    text: &Rope,
    pattern: &str,
    from: usize,
    backward: bool,
    count: usize,
) -> Option<usize> {
    let starts = match_starts(text, pattern);
    if starts.is_empty() {
        return None;
    }
    let mut position = from;
    for _ in 0..count.max(1) {
        position = if backward {
            starts
                .iter()
                .rev()
                .find(|start| **start < position)
                .or(starts.last())
                .copied()?
        } else {
            starts
                .iter()
                .find(|start| **start > position)
                .or(starts.first())
                .copied()?
        };
    }
    Some(position)
}

/// The keyword under the caret, or the first after it on the line, for `*`
/// and `#`: its range and the pattern that finds it as a whole word.
pub(super) fn word_pattern(text: &Rope, offset: usize) -> Option<(Range<usize>, String)> {
    let row = text::row(text, offset);
    let end = text::line_end(text, row);
    let start_of_line = text::line_start(text, row);
    let mut position = offset;
    while position < end && !text::char_at(text, position).is_some_and(text::is_word_char) {
        position = text::next_offset(text, position);
    }
    let keyword = position < end;
    let is_part = |c: char| {
        if keyword {
            text::is_word_char(c)
        } else {
            !c.is_whitespace()
        }
    };
    if !keyword {
        // No keyword: the non-blank text under the caret.
        position = offset;
        while position < end && text::char_at(text, position).is_some_and(char::is_whitespace) {
            position = text::next_offset(text, position);
        }
        if position >= end {
            return None;
        }
    }
    let mut start = position;
    while start > start_of_line && text::char_before(text, start).is_some_and(is_part) {
        start = text::prev_offset(text, start);
    }
    let mut stop = position;
    while stop < end && text::char_at(text, stop).is_some_and(is_part) {
        stop = text::next_offset(text, stop);
    }
    let word = text.slice(start..stop).to_string();
    let escaped: String = word
        .chars()
        .flat_map(|c| {
            let special = matches!(c, '\\' | '/' | '.' | '*' | '[' | ']' | '~' | '^' | '$');
            special
                .then_some('\\')
                .into_iter()
                .chain(std::iter::once(c))
        })
        .collect();
    let pattern = if keyword {
        format!("\\<{escaped}\\>")
    } else {
        escaped
    };
    Some((start..stop, pattern))
}

/// The replacement of `:s`, in the regex crate's syntax: `&` and `\0` the
/// whole match, `\1`–`\9` groups, `\r` and `\n` a line break, `\&` a plain
/// `&`.
pub(super) fn replacement(replacement: &str) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some(digit @ '0'..='9') => {
                    out.push_str("${");
                    out.push(digit);
                    out.push('}');
                }
                Some('r' | 'n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}
