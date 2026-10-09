//! Text objects: `iw`, `a(`, `it` and the rest, for operators and visual
//! mode.

use std::ops::Range;

use ropey::Rope;

use super::text;

/// A kind of text object. `around` (`a`) or inner (`i`) is chosen with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Object {
    /// `w`, `W`
    Word { big: bool },
    /// `"`, `'` and `` ` ``
    Quote(char),
    /// `(` `)` `b`, `[` `]`, `{` `}` `B`, `<` `>`
    Bracket { open: char, close: char },
    /// `t`: an XML or HTML element.
    Tag,
    /// `p`
    Paragraph,
}

/// What an object covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ObjectRange {
    pub(super) range: Range<usize>,
    /// Whole lines: `ip`, `ap`, and `i{` when the braces are on lines of
    /// their own.
    pub(super) linewise: bool,
}

impl Object {
    /// The text the object covers around `offset`, `count` levels or words
    /// out. In visual mode, `selection` is what is selected now, which a
    /// bracket object grows beyond.
    pub(super) fn range(
        self,
        text: &Rope,
        offset: usize,
        around: bool,
        count: usize,
        selection: Option<Range<usize>>,
    ) -> Option<ObjectRange> {
        let count = count.max(1);
        let chars = |range: Range<usize>| ObjectRange {
            range,
            linewise: false,
        };
        match self {
            Self::Word { big } => word(text, offset, around, count, big).map(chars),
            Self::Quote(quote) => quoted(text, offset, quote, around).map(chars),
            Self::Bracket { open, close } => {
                bracket(text, offset, open, close, around, count, selection)
            }
            Self::Tag => tag(text, offset, around, count, selection).map(chars),
            Self::Paragraph => paragraph(text, offset, around, count),
        }
    }
}

/// `iw`, `aw`, `iW`, `aW`. Within a line: a run of word characters, of
/// other characters, or of blanks.
fn word(text: &Rope, offset: usize, around: bool, count: usize, big: bool) -> Option<Range<usize>> {
    let row = text::row(text, offset);
    let line_start = text::line_start(text, row);
    let line_end = text::line_end(text, row);
    if line_start == line_end {
        return None;
    }
    let class_at = |offset: usize| text::char_at(text, offset).map_or(0, |c| text::class(c, big));
    // The run of one class around `offset`.
    let run = |offset: usize| -> Range<usize> {
        let class = class_at(offset);
        let mut start = offset;
        while start > line_start && class_at(text::prev_offset(text, start)) == class {
            start = text::prev_offset(text, start);
        }
        let mut end = offset;
        while end < line_end && class_at(end) == class {
            end = text::next_offset(text, end);
        }
        start..end
    };

    let offset = offset.min(text::prev_offset(text, line_end));
    let first = run(offset);
    let mut range = first.clone();
    let on_blank = class_at(offset) == 0;
    if !around {
        // Each count adds the next run: word, blanks, word…
        for _ in 1..count {
            if range.end >= line_end {
                break;
            }
            range.end = run(range.end).end;
        }
        return Some(range);
    }

    if on_blank {
        // Blanks and the word after them.
        for _ in 0..count {
            if range.end >= line_end {
                break;
            }
            range.end = run(range.end).end;
            if range.end < line_end && class_at(range.end) == 0 {
                range.end = run(range.end).end;
            }
        }
        return Some(range);
    }

    // The word with the blanks after it, or before it when none follow.
    for ix in 0..count {
        if ix > 0 {
            if range.end >= line_end {
                break;
            }
            range.end = run(range.end).end;
        }
        if range.end < line_end && class_at(range.end) == 0 {
            range.end = run(range.end).end;
        } else if ix == 0 && range.start > line_start {
            let before = text::prev_offset(text, range.start);
            if class_at(before) == 0 {
                let blanks = run(before);
                // Keep the indentation when the word starts the line's text.
                if blanks.start != line_start {
                    range.start = blanks.start;
                }
            }
        }
    }
    Some(range)
}

/// `i"`, `a"` and the other quotes, within one line. The pair around the
/// caret, or the first pair after it.
fn quoted(text: &Rope, offset: usize, quote: char, around: bool) -> Option<Range<usize>> {
    let row = text::row(text, offset);
    let line_start = text::line_start(text, row);
    let line_end = text::line_end(text, row);
    let mut quotes = Vec::new();
    let mut position = line_start;
    let mut escaped = false;
    for c in text.chars_at(line_start) {
        if position >= line_end {
            break;
        }
        if c == quote && !escaped {
            quotes.push(position);
        }
        escaped = c == '\\' && !escaped;
        position += c.len_utf8();
    }
    let pairs: Vec<(usize, usize)> = quotes
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    let (open, close) = pairs
        .iter()
        .copied()
        .find(|(open, close)| *open <= offset && offset <= *close)
        .or_else(|| pairs.iter().copied().find(|(open, _)| *open > offset))?;
    let close_end = close + quote.len_utf8();
    if !around {
        return Some(open + quote.len_utf8()..close);
    }
    // The quotes and the blanks after them, or before them when none follow.
    let mut end = close_end;
    while end < line_end && matches!(text::char_at(text, end), Some(' ' | '\t')) {
        end = text::next_offset(text, end);
    }
    let mut start = open;
    if end == close_end {
        while start > line_start && matches!(text::char_before(text, start), Some(' ' | '\t')) {
            start = text::prev_offset(text, start);
        }
    }
    Some(start..end)
}

/// `i(`, `a(` and the other brackets: the `count`-th pair enclosing
/// `offset`, nesting counted, across lines.
fn bracket(
    text: &Rope,
    offset: usize,
    open: char,
    close: char,
    around: bool,
    count: usize,
    selection: Option<Range<usize>>,
) -> Option<ObjectRange> {
    let mut levels = count;
    let mut search_from = offset;
    loop {
        let (start, end) = enclosing_pair(text, search_from, open, close, levels)?;
        let result = bracket_range(text, start, end, open, close, around);
        // In visual mode, a selection that already covers this pair grows to
        // the next one out.
        if let Some(selection) = &selection
            && selection.len() > 1
            && result.range.start >= selection.start
            && result.range.end <= selection.end
        {
            levels = 1;
            if start == 0 {
                return None;
            }
            search_from = text::prev_offset(text, start);
            continue;
        }
        return Some(result);
    }
}

fn bracket_range(
    text: &Rope,
    start: usize,
    end: usize,
    open: char,
    close: char,
    around: bool,
) -> ObjectRange {
    if around {
        return ObjectRange {
            range: start..end + close.len_utf8(),
            linewise: false,
        };
    }
    let mut inner_start = start + open.len_utf8();
    let mut inner_end = end;
    let start_row = text::row(text, start);
    let end_row = text::row(text, end);
    // A bracket ending its line, and a closing bracket with only blanks
    // before it: the lines between them, whole.
    let opens_line = inner_start == text::line_end(text, start_row) && end_row > start_row;
    let closes_line = end_row > start_row && text::first_non_blank(text, end_row) == end;
    if opens_line {
        inner_start = text::line_start(text, start_row + 1);
    }
    if closes_line {
        inner_end = text::line_start(text, end_row);
    }
    let linewise = opens_line && closes_line && inner_start < inner_end;
    ObjectRange {
        range: inner_start..inner_end.max(inner_start),
        linewise,
    }
}

/// The `levels`-th pair of `open` and `close` around `offset`: its opening
/// and closing bracket. A caret on a bracket uses that bracket's pair.
fn enclosing_pair(
    text: &Rope,
    offset: usize,
    open: char,
    close: char,
    levels: usize,
) -> Option<(usize, usize)> {
    let at = text::char_at(text, offset);
    // The opening bracket: back from the caret, skipping nested pairs.
    let mut start = None;
    let mut depth = 0usize;
    let mut found = 0;
    let mut position = if at == Some(open) {
        offset + open.len_utf8()
    } else if at == Some(close) {
        offset
    } else {
        offset
    };
    for c in text.chars_at(position).reversed() {
        position -= c.len_utf8();
        if c == close {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                found += 1;
                if found == levels {
                    start = Some(position);
                    break;
                }
            } else {
                depth -= 1;
            }
        }
    }
    let start = start?;
    // Its closing bracket, forward.
    let mut depth = 0usize;
    let mut position = start;
    for c in text.chars_at(start) {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return (position >= offset || at == Some(close)).then_some((start, position));
            }
        }
        position += c.len_utf8();
    }
    None
}

/// `it`, `at`: the `count`-th element enclosing `offset`.
fn tag(
    text: &Rope,
    offset: usize,
    around: bool,
    count: usize,
    selection: Option<Range<usize>>,
) -> Option<Range<usize>> {
    let source = text.to_string();
    let pattern = regex::Regex::new(r"<(/?)([A-Za-z][A-Za-z0-9:._-]*)[^<>]*?(/?)>").ok()?;
    // Pair the tags: (open start, open end, close start, close end).
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut elements: Vec<(usize, usize, usize, usize)> = Vec::new();
    for captures in pattern.captures_iter(&source) {
        let whole = captures.get(0)?;
        let closing = !captures[1].is_empty();
        let self_closing = !captures[3].is_empty();
        let name = captures[2].to_ascii_lowercase();
        if self_closing {
            continue;
        }
        if !closing {
            stack.push((name, whole.start(), whole.end()));
        } else if let Some(ix) = stack.iter().rposition(|(open, _, _)| *open == name) {
            let (_, start, inner_start) = stack[ix].clone();
            stack.truncate(ix);
            elements.push((start, inner_start, whole.start(), whole.end()));
        }
    }
    let mut enclosing: Vec<(usize, usize, usize, usize)> = elements
        .into_iter()
        .filter(|(start, _, _, end)| *start <= offset && offset < *end)
        .collect();
    // Innermost first.
    enclosing.sort_by_key(|(start, _, _, end)| end - start);
    let mut skip = count - 1;
    for (start, inner_start, inner_end, end) in enclosing {
        let range = if around {
            start..end
        } else {
            inner_start..inner_end
        };
        if let Some(selection) = &selection
            && selection.len() > 1
            && range.start >= selection.start
            && range.end <= selection.end
        {
            continue;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        return Some(range);
    }
    None
}

/// `ip`, `ap`: runs of lines with text or of blank lines, whole.
fn paragraph(text: &Rope, offset: usize, around: bool, count: usize) -> Option<ObjectRange> {
    let last_row = text::last_row(text);
    let row = text::row(text, offset);
    let blank = |row: usize| text::is_blank_line(text, row);
    let run_end = |row: usize| {
        let kind = blank(row);
        let mut end = row;
        while end < last_row && blank(end + 1) == kind {
            end += 1;
        }
        end
    };
    let kind = blank(row);
    let mut start = row;
    while start > 0 && blank(start - 1) == kind {
        start -= 1;
    }
    let mut end = run_end(row);
    let mut runs = if around { count * 2 } else { count } - 1;
    while runs > 0 && end < last_row {
        end = run_end(end + 1);
        runs -= 1;
    }
    if around && !kind && !blank(end) {
        // No blank lines follow the paragraph: take the ones before it.
        let mut before = start;
        while before > 0 && blank(before - 1) {
            before -= 1;
        }
        start = before;
    }
    Some(ObjectRange {
        range: text::rows_range(text, &(start..end + 1)),
        linewise: true,
    })
}
