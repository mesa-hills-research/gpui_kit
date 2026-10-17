//! The diff model: files, hunks and lines, parsed from unified diff text or
//! computed between two texts. Plain data, so a large diff can be built off
//! the UI thread.

use std::{collections::HashMap, ops::Range};

use gpui::SharedString;

/// How many unchanged lines a computed diff keeps around each change.
pub(super) const CONTEXT_LINES: usize = 3;

/// The edit steps the shortest-diff search takes before it gives up on a
/// stretch of lines: the search costs time and memory with the square of
/// it. Past it, the lines that occur once on each side anchor the diff, and
/// the stretches between them are diffed on their own.
const MAX_EDIT_DISTANCE: usize = 1500;

/// How many times a stretch is split at unique lines before the rest of it
/// shows as removed, then added.
const MAX_ANCHOR_DEPTH: usize = 8;

/// The marker git writes after a line that has no line break.
const NO_NEWLINE: &str = "\\ No newline at end of file";

/// Whether a line is kept, added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineChange {
    /// The line is in both versions.
    Unchanged,
    /// The line is only in the new version.
    Added,
    /// The line is only in the old version.
    Removed,
}

/// How a line ends. Two lines that differ only in how they end are a
/// change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineEnding {
    /// A line feed, `\n`.
    #[default]
    Lf,
    /// A carriage return and a line feed, `\r\n`.
    CrLf,
    /// No line break: the last line of a file that has no final newline.
    Missing,
}

/// One line of a diff, with its numbers in the old and new versions.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    change: LineChange,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: SharedString,
    ending: LineEnding,
}

impl DiffLine {
    /// Whether the line is kept, added or removed.
    pub fn change(&self) -> LineChange {
        self.change
    }

    /// Its 1-based number in the old version, unless it was added.
    pub fn old_line(&self) -> Option<u32> {
        self.old_line
    }

    /// Its 1-based number in the new version, unless it was removed.
    pub fn new_line(&self) -> Option<u32> {
        self.new_line
    }

    /// Its text, without the line break.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// How it ends.
    pub fn ending(&self) -> LineEnding {
        self.ending
    }
}

/// A run of lines shown together: changes and the unchanged lines around
/// them, after a `@@ -a,b +c,d @@` header.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DiffHunk {
    pub(super) header: SharedString,
    /// The hunk's lines, in [`DiffFile::lines`].
    pub(super) lines: Range<usize>,
}

/// The changes to one file.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffFile {
    old_path: Option<SharedString>,
    new_path: Option<SharedString>,
    /// Every line known. A diff computed between two texts knows the whole
    /// file, so the unchanged lines between hunks can be shown on request.
    /// A parsed diff knows only its hunks' lines.
    lines: Vec<DiffLine>,
    pub(super) hunks: Vec<DiffHunk>,
    added: usize,
    removed: usize,
    binary: bool,
    old_mode: Option<SharedString>,
    new_mode: Option<SharedString>,
}

impl DiffFile {
    fn new() -> Self {
        Self {
            old_path: None,
            new_path: None,
            lines: Vec::new(),
            hunks: Vec::new(),
            added: 0,
            removed: 0,
            binary: false,
            old_mode: None,
            new_mode: None,
        }
    }

    /// The file's path before the change, if it existed.
    pub fn old_path(&self) -> Option<&SharedString> {
        self.old_path.as_ref()
    }

    /// The file's path after the change, unless it was deleted.
    pub fn new_path(&self) -> Option<&SharedString> {
        self.new_path.as_ref()
    }

    /// The path to show: the new one, else the old one.
    pub fn path(&self) -> Option<&SharedString> {
        self.new_path.as_ref().or(self.old_path.as_ref())
    }

    /// Whether the file moved: it has an old and a new path, and they
    /// differ.
    pub fn is_rename(&self) -> bool {
        matches!((&self.old_path, &self.new_path), (Some(old), Some(new)) if old != new)
    }

    /// Whether git found the file binary, so the diff has no lines for it.
    pub fn is_binary(&self) -> bool {
        self.binary
    }

    /// The file's old and new mode, such as `100644` and `100755`, when the
    /// change set its executable bit or made it a link.
    pub fn mode_change(&self) -> Option<(&SharedString, &SharedString)> {
        self.old_mode.as_ref().zip(self.new_mode.as_ref())
    }

    /// Every line known, in order.
    pub fn lines(&self) -> &[DiffLine] {
        &self.lines
    }

    /// How many lines were added.
    pub fn added_lines(&self) -> usize {
        self.added
    }

    /// How many lines were removed.
    pub fn removed_lines(&self) -> usize {
        self.removed
    }

    /// The lines known but not in any hunk before hunk `ix` (after the
    /// previous one), which a viewer can reveal. `ix == hunks.len()` gives
    /// those after the last hunk.
    pub(super) fn gap_before(&self, ix: usize) -> Range<usize> {
        let start = ix
            .checked_sub(1)
            .and_then(|prev| self.hunks.get(prev))
            .map_or(0, |hunk| hunk.lines.end);
        let end = self
            .hunks
            .get(ix)
            .map_or(self.lines.len(), |hunk| hunk.lines.start);
        start..end.max(start)
    }

    fn count(&mut self) {
        self.added = self
            .lines
            .iter()
            .filter(|line| line.change == LineChange::Added)
            .count();
        self.removed = self
            .lines
            .iter()
            .filter(|line| line.change == LineChange::Removed)
            .count();
    }

    /// The file in unified diff format, with git's headers for a rename, a
    /// mode change or a binary file.
    fn write_unified(&self, out: &mut String) {
        let old = self
            .old_path
            .as_ref()
            .map_or("/dev/null".to_string(), |path| quoted_path("a/", path));
        let new = self
            .new_path
            .as_ref()
            .map_or("/dev/null".to_string(), |path| quoted_path("b/", path));
        if self.is_rename() || self.binary || self.mode_change().is_some() {
            let (old_path, new_path) = (self.path_or_other(true), self.path_or_other(false));
            out.push_str(&format!(
                "diff --git {} {}\n",
                quoted_path("a/", old_path),
                quoted_path("b/", new_path)
            ));
            if let Some((old_mode, new_mode)) = self.mode_change() {
                out.push_str(&format!("old mode {old_mode}\nnew mode {new_mode}\n"));
            }
            if self.is_rename() {
                out.push_str(&format!(
                    "rename from {}\nrename to {}\n",
                    quoted_path("", old_path),
                    quoted_path("", new_path)
                ));
            }
            if self.binary {
                out.push_str(&format!("Binary files {old} and {new} differ\n"));
                return;
            }
            if self.hunks.is_empty() {
                return;
            }
        }
        out.push_str(&format!("--- {old}\n+++ {new}\n"));
        for hunk in &self.hunks {
            out.push_str(&self.hunk_header(hunk));
            out.push('\n');
            for line in &self.lines[hunk.lines.clone()] {
                out.push(match line.change {
                    LineChange::Unchanged => ' ',
                    LineChange::Added => '+',
                    LineChange::Removed => '-',
                });
                out.push_str(&line.text);
                match line.ending {
                    LineEnding::Lf => out.push('\n'),
                    LineEnding::CrLf => out.push_str("\r\n"),
                    LineEnding::Missing => {
                        out.push('\n');
                        out.push_str(NO_NEWLINE);
                        out.push('\n');
                    }
                }
            }
        }
    }

    /// A hunk's header, its counts made to match its lines: a hand-written
    /// hunk may hold more or fewer lines than its header said, and a patch
    /// tool rejects a header that doesn't match.
    fn hunk_header(&self, hunk: &DiffHunk) -> SharedString {
        let lines = &self.lines[hunk.lines.clone()];
        let old_count = lines.iter().filter(|l| l.old_line.is_some()).count() as u32;
        let new_count = lines.iter().filter(|l| l.new_line.is_some()).count() as u32;
        match hunk_header(&hunk.header) {
            Some((_, old, _, new)) if (old, new) == (old_count, new_count) => hunk.header.clone(),
            Some((old_start, _, new_start, _)) => {
                // Whatever follows the closing `@@`, such as a function name.
                let section = hunk
                    .header
                    .trim_start_matches('@')
                    .split_once("@@")
                    .map_or("", |(_, section)| section);
                format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@{section}").into()
            }
            None => hunk.header.clone(),
        }
    }

    /// The old (or new) path, else the other one, for git's header.
    fn path_or_other(&self, old: bool) -> &str {
        let (first, second) = if old {
            (&self.old_path, &self.new_path)
        } else {
            (&self.new_path, &self.old_path)
        };
        first.as_ref().or(second.as_ref()).map_or("", |path| path)
    }
}

/// A diff of one or more files, for a [`DiffView`](super::DiffView).
///
/// Build it from unified diff text with [`Diff::parse`], or from the old and
/// new text of a file with [`Diff::between`]. Both are plain computations,
/// so for a large input, build the diff on a background thread and hand it
/// to the view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    files: Vec<DiffFile>,
}

impl Diff {
    /// Parse unified diff text: `git diff`, `git log -p` or
    /// `git format-patch` output, or `diff -u`, one file or many, or bare
    /// `@@` hunks. A hunk ends where its header's counts are spent, though
    /// it takes in further added and removed lines, as hand-written diffs
    /// often miscount. Lines outside hunks other than file headers are
    /// skipped.
    pub fn parse(text: &str) -> Self {
        let mut files = Vec::new();
        let mut file: Option<DiffFile> = None;
        // Whether the current file has had its `---` header.
        let mut has_header = false;
        let (mut old_line, mut new_line) = (0u32, 0u32);
        // The lines the current hunk's header says are left on each side.
        let (mut old_left, mut new_left) = (0u32, 0u32);
        let mut in_hunk = false;

        for raw in text.split_inclusive('\n') {
            let raw = raw.strip_suffix('\n').unwrap_or(raw);
            let (line, crlf) = match raw.strip_suffix('\r') {
                Some(line) => (line, true),
                None => (raw, false),
            };
            let counts_left = old_left > 0 || new_left > 0;
            let hunk_open = in_hunk && counts_left;

            if line.starts_with("diff --git ") {
                finish_file(file.take(), &mut files);
                let mut started = DiffFile::new();
                if let Some((old, new)) = git_paths(line) {
                    started.old_path = Some(old);
                    started.new_path = Some(new);
                }
                file = Some(started);
                has_header = false;
                in_hunk = false;
                continue;
            }
            if !hunk_open && let Some(path) = line.strip_prefix("--- ") {
                // A second header, or one after hunks, starts the next file
                // (unless a `diff --git` line already did).
                if file
                    .as_ref()
                    .is_some_and(|file| has_header || !file.hunks.is_empty())
                {
                    finish_file(file.take(), &mut files);
                }
                file.get_or_insert_with(DiffFile::new).old_path = header_path(path, "a/");
                has_header = true;
                in_hunk = false;
                continue;
            }
            if !hunk_open && let Some(path) = line.strip_prefix("+++ ") {
                file.get_or_insert_with(DiffFile::new).new_path = header_path(path, "b/");
                in_hunk = false;
                continue;
            }
            if line.starts_with("@@") {
                // A combined diff's `@@@` or a broken header ends the hunk
                // before it, and its lines are skipped.
                in_hunk = false;
                let Some(header) = hunk_header(line) else {
                    continue;
                };
                let file = file.get_or_insert_with(DiffFile::new);
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.lines.end = file.lines.len();
                }
                let start = file.lines.len();
                file.hunks.push(DiffHunk {
                    header: line.to_string().into(),
                    lines: start..start,
                });
                (old_line, old_left, new_line, new_left) = header;
                in_hunk = true;
                continue;
            }
            if !in_hunk {
                if let Some(file) = file.as_mut().filter(|file| file.hunks.is_empty()) {
                    extended_header(file, line);
                }
                continue;
            }
            let Some(file) = file.as_mut() else {
                continue;
            };
            let first = line.as_bytes().first().copied();
            if first == Some(b'\\') {
                // "\ No newline at end of file", after the line it is about.
                let hunk_start = file.hunks.last().map_or(0, |hunk| hunk.lines.start);
                if file.lines.len() > hunk_start
                    && let Some(last) = file.lines.last_mut()
                {
                    last.ending = LineEnding::Missing;
                }
                continue;
            }
            // Past its counts, a hunk takes only more added or removed
            // lines. Anything else ends it: context, a blank line, a commit
            // message or a `-- ` signature.
            if !counts_left && (!matches!(first, Some(b'+' | b'-')) || line == "-- ") {
                in_hunk = false;
                continue;
            }
            let (change, text) = match first {
                Some(b'+') => (LineChange::Added, &line[1..]),
                Some(b'-') => (LineChange::Removed, &line[1..]),
                Some(b' ') => (LineChange::Unchanged, &line[1..]),
                // An empty line inside a hunk is an unchanged empty line
                // whose leading space an editor trimmed.
                None => (LineChange::Unchanged, ""),
                _ => {
                    in_hunk = false;
                    continue;
                }
            };
            let (old, new) = match change {
                LineChange::Unchanged => (Some(old_line), Some(new_line)),
                LineChange::Added => (None, Some(new_line)),
                LineChange::Removed => (Some(old_line), None),
            };
            if old.is_some() {
                old_line = old_line.saturating_add(1);
                old_left = old_left.saturating_sub(1);
            }
            if new.is_some() {
                new_line = new_line.saturating_add(1);
                new_left = new_left.saturating_sub(1);
            }
            file.lines.push(DiffLine {
                change,
                old_line: old,
                new_line: new,
                text: text.to_string().into(),
                ending: if crlf {
                    LineEnding::CrLf
                } else {
                    LineEnding::Lf
                },
            });
        }
        finish_file(file, &mut files);
        files.retain(|file| {
            !file.hunks.is_empty()
                || file.path().is_some()
                || file.binary
                || file.mode_change().is_some()
        });
        Self { files }
    }

    /// The line diff between two versions of the file at `path`.
    ///
    /// The diff knows the whole file: changes are grouped into hunks with
    /// three unchanged lines around each, and a [`DiffView`](super::DiffView)
    /// can reveal the unchanged lines between them. Lines that differ only
    /// in their line break, `\n` or `\r\n`, or a final newline added or
    /// taken away, are changes.
    pub fn between(path: impl Into<SharedString>, old: &str, new: &str) -> Self {
        let path = path.into();
        // Each line with its line break, so a changed break is a change.
        let old_lines = old.split_inclusive('\n').collect::<Vec<_>>();
        let new_lines = new.split_inclusive('\n').collect::<Vec<_>>();
        let mut lines = Vec::with_capacity(old_lines.len().max(new_lines.len()));
        let (mut old_line, mut new_line) = (1u32, 1u32);
        for op in line_diff(&old_lines, &new_lines) {
            let (change, raw) = match op {
                Op::Equal(ix, _) => (LineChange::Unchanged, old_lines[ix]),
                Op::Delete(ix) => (LineChange::Removed, old_lines[ix]),
                Op::Insert(ix) => (LineChange::Added, new_lines[ix]),
            };
            let (text, ending) = split_ending(raw);
            let (old_no, new_no) = match change {
                LineChange::Unchanged => (Some(old_line), Some(new_line)),
                LineChange::Added => (None, Some(new_line)),
                LineChange::Removed => (Some(old_line), None),
            };
            old_line = old_line.saturating_add(old_no.is_some() as u32);
            new_line = new_line.saturating_add(new_no.is_some() as u32);
            lines.push(DiffLine {
                change,
                old_line: old_no,
                new_line: new_no,
                text: text.to_string().into(),
                ending,
            });
        }
        let hunks = hunks_with_context(&lines, CONTEXT_LINES);
        let mut file = DiffFile {
            old_path: Some(path.clone()),
            new_path: Some(path),
            lines,
            hunks,
            ..DiffFile::new()
        };
        file.count();
        Self { files: vec![file] }
    }

    /// The files, in order.
    pub fn files(&self) -> &[DiffFile] {
        &self.files
    }

    /// Whether nothing changed: no line, and no file renamed, made binary or
    /// given another mode.
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|file| {
            file.added == 0
                && file.removed == 0
                && !file.is_rename()
                && !file.binary
                && file.mode_change().is_none()
        })
    }

    /// How many lines were added, in all files.
    pub fn added_lines(&self) -> usize {
        self.files.iter().map(|file| file.added).sum()
    }

    /// How many lines were removed, in all files.
    pub fn removed_lines(&self) -> usize {
        self.files.iter().map(|file| file.removed).sum()
    }

    /// One file of the diff as unified diff text, what its Copy button
    /// copies. Empty for an index out of range.
    pub fn file_unified(&self, ix: usize) -> String {
        let mut out = String::new();
        if let Some(file) = self.files.get(ix) {
            file.write_unified(&mut out);
        }
        out
    }

    /// The diff as unified diff text.
    pub fn to_unified(&self) -> String {
        let mut out = String::new();
        for file in &self.files {
            file.write_unified(&mut out);
        }
        out
    }
}

/// A line's text and how it ends.
fn split_ending(raw: &str) -> (&str, LineEnding) {
    if let Some(text) = raw.strip_suffix("\r\n") {
        (text, LineEnding::CrLf)
    } else if let Some(text) = raw.strip_suffix('\n') {
        (text, LineEnding::Lf)
    } else {
        (raw, LineEnding::Missing)
    }
}

/// Close the last hunk of `file`, count its changes and add it to `files`.
fn finish_file(file: Option<DiffFile>, files: &mut Vec<DiffFile>) {
    if let Some(mut file) = file {
        if let Some(hunk) = file.hunks.last_mut() {
            hunk.lines.end = file.lines.len();
        }
        file.count();
        files.push(file);
    }
}

/// Take in one of git's extended header lines, between `diff --git` and the
/// first hunk: a rename, a mode change, a new or deleted file, a binary
/// file.
fn extended_header(file: &mut DiffFile, line: &str) {
    if let Some(path) = line
        .strip_prefix("rename from ")
        .or_else(|| line.strip_prefix("copy from "))
    {
        file.old_path = Some(unquoted_path(path).into());
    } else if let Some(path) = line
        .strip_prefix("rename to ")
        .or_else(|| line.strip_prefix("copy to "))
    {
        file.new_path = Some(unquoted_path(path).into());
    } else if let Some(mode) = line.strip_prefix("old mode ") {
        file.old_mode = Some(mode.trim().to_string().into());
    } else if let Some(mode) = line.strip_prefix("new mode ") {
        file.new_mode = Some(mode.trim().to_string().into());
    } else if line.starts_with("new file mode ") {
        file.old_path = None;
    } else if line.starts_with("deleted file mode ") {
        file.new_path = None;
    } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
        file.binary = true;
    }
}

/// The paths of a `diff --git a/old b/new` line, either of them quoted.
fn git_paths(line: &str) -> Option<(SharedString, SharedString)> {
    let rest = line.strip_prefix("diff --git ")?;
    let (old, rest) = if rest.starts_with('"') {
        let (old, rest) = unquote(rest)?;
        (old, rest.strip_prefix(' ')?)
    } else {
        // An unquoted path may hold spaces, so split before the new one.
        let ix = rest.find(" b/").or_else(|| rest.find(" \"b/"))?;
        (rest[..ix].to_string(), &rest[ix + 1..])
    };
    let new = if rest.starts_with('"') {
        unquote(rest)?.0
    } else {
        rest.to_string()
    };
    Some((
        old.strip_prefix("a/")?.to_string().into(),
        new.strip_prefix("b/")?.to_string().into(),
    ))
}

/// The path of a `---` or `+++` header, unquoted and without its `a/` or
/// `b/` prefix and any tab-separated timestamp. `None` for `/dev/null`.
fn header_path(path: &str, prefix: &str) -> Option<SharedString> {
    let path = if path.starts_with('"') {
        unquote(path)?.0
    } else {
        path.split('\t')
            .next()
            .unwrap_or(path)
            .trim_end()
            .to_string()
    };
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .unwrap_or(&path)
            .to_string()
            .into(),
    )
}

/// A path git may have quoted, as in `rename from "sp ace.txt"`.
fn unquoted_path(path: &str) -> String {
    let path = path.trim_end();
    match unquote(path) {
        Some((path, _)) => path,
        None => path.to_string(),
    }
}

/// The text of a C-style quoted string at the start of `s`, as git quotes
/// paths with unusual characters (octal escapes for bytes past ASCII), and
/// what follows it.
fn unquote(s: &str) -> Option<(String, &str)> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut out = Vec::new();
    let mut ix = 1;
    while let Some(&byte) = bytes.get(ix) {
        match byte {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &s[ix + 1..])),
            b'\\' => {
                let escaped = *bytes.get(ix + 1)?;
                ix += 2;
                match escaped {
                    // Up to three octal digits: a byte.
                    b'0'..=b'7' => {
                        let mut value = (escaped - b'0') as u32;
                        for _ in 0..2 {
                            let Some(digit @ b'0'..=b'7') = bytes.get(ix).copied() else {
                                break;
                            };
                            value = value * 8 + (digit - b'0') as u32;
                            ix += 1;
                        }
                        out.push(value as u8);
                    }
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    other => out.push(other),
                }
            }
            byte => {
                out.push(byte);
                ix += 1;
            }
        }
    }
    None
}

/// `prefix` and `path`, quoted as git quotes a path holding a quote, a
/// backslash or a control character.
fn quoted_path(prefix: &str, path: &str) -> String {
    if !path
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        return format!("{prefix}{path}");
    }
    let mut out = format!("\"{prefix}");
    for c in path.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let mut buf = [0; 4];
                for byte in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("\\{byte:03o}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The old start and count and the new start and count of a
/// `@@ -a,b +c,d @@` header. A count left out is 1.
fn hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = line.trim_start_matches('@').split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |range: &str| -> Option<(u32, u32)> {
        let mut parts = range.split(',');
        let start = parts.next()?.parse().ok()?;
        let count = parts.next().map_or(Some(1), |count| count.parse().ok())?;
        Some((start, count))
    };
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

/// Group the changed lines into hunks with `context` unchanged lines around
/// each, merging hunks whose context would touch.
fn hunks_with_context(lines: &[DiffLine], context: usize) -> Vec<DiffHunk> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (ix, line) in lines.iter().enumerate() {
        if line.change == LineChange::Unchanged {
            continue;
        }
        let start = ix.saturating_sub(context);
        let end = (ix + 1 + context).min(lines.len());
        match ranges.last_mut() {
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => ranges.push(start..end),
        }
    }
    ranges
        .into_iter()
        .map(|range| {
            let slice = &lines[range.clone()];
            let old_count = slice.iter().filter(|l| l.old_line.is_some()).count();
            let new_count = slice.iter().filter(|l| l.new_line.is_some()).count();
            let old_start = slice.iter().find_map(|l| l.old_line).unwrap_or_else(|| {
                lines[..range.start]
                    .iter()
                    .rev()
                    .find_map(|l| l.old_line)
                    .unwrap_or(0)
            });
            let new_start = slice.iter().find_map(|l| l.new_line).unwrap_or_else(|| {
                lines[..range.start]
                    .iter()
                    .rev()
                    .find_map(|l| l.new_line)
                    .unwrap_or(0)
            });
            DiffHunk {
                header: format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@").into(),
                lines: range,
            }
        })
        .collect()
}

/// One step of a line diff, by index into the old or new lines.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Equal(usize, usize),
    Delete(usize),
    Insert(usize),
}

impl Op {
    fn offset(self, old: usize, new: usize) -> Self {
        match self {
            Op::Equal(a, b) => Op::Equal(a + old, b + new),
            Op::Delete(a) => Op::Delete(a + old),
            Op::Insert(b) => Op::Insert(b + new),
        }
    }
}

/// An edit script from `old` to `new`: the shortest (Myers) while it takes
/// at most [`MAX_EDIT_DISTANCE`] steps, else anchored on unique lines.
fn line_diff(old: &[&str], new: &[&str]) -> Vec<Op> {
    line_diff_within(old, new, MAX_EDIT_DISTANCE)
}

fn line_diff_within(old: &[&str], new: &[&str], max_distance: usize) -> Vec<Op> {
    let mut ops = Vec::with_capacity(old.len().max(new.len()));
    diff_into(old, new, (0, 0), max_distance, MAX_ANCHOR_DEPTH, &mut ops);
    ops
}

/// Append the edit script from `old` to `new`, whose first lines are at
/// `at` in the whole texts. Lines shared at the start and end are trimmed
/// first. If the shortest script takes more than `max_distance` steps, the
/// lines that occur once on each side, in an order both keep (patience
/// diff), anchor the script, and each stretch between them is diffed the
/// same way, `depth` times at most. A stretch past that, or with no such
/// lines, shows as removed, then added.
fn diff_into(
    old: &[&str],
    new: &[&str],
    at: (usize, usize),
    max_distance: usize,
    depth: usize,
    ops: &mut Vec<Op>,
) {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    ops.extend((0..prefix).map(|ix| Op::Equal(at.0 + ix, at.1 + ix)));
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    let mid_at = (at.0 + prefix, at.1 + prefix);

    if let Some(middle) = myers(old_mid, new_mid, max_distance) {
        ops.extend(middle.into_iter().map(|op| op.offset(mid_at.0, mid_at.1)));
    } else {
        let anchors = if depth > 0 {
            unique_anchors(old_mid, new_mid)
        } else {
            Vec::new()
        };
        if anchors.is_empty() {
            ops.extend((0..old_mid.len()).map(|ix| Op::Delete(mid_at.0 + ix)));
            ops.extend((0..new_mid.len()).map(|ix| Op::Insert(mid_at.1 + ix)));
        } else {
            let (mut old_ix, mut new_ix) = (0, 0);
            for (old_anchor, new_anchor) in anchors {
                diff_into(
                    &old_mid[old_ix..old_anchor],
                    &new_mid[new_ix..new_anchor],
                    (mid_at.0 + old_ix, mid_at.1 + new_ix),
                    max_distance,
                    depth - 1,
                    ops,
                );
                ops.push(Op::Equal(mid_at.0 + old_anchor, mid_at.1 + new_anchor));
                (old_ix, new_ix) = (old_anchor + 1, new_anchor + 1);
            }
            diff_into(
                &old_mid[old_ix..],
                &new_mid[new_ix..],
                (mid_at.0 + old_ix, mid_at.1 + new_ix),
                max_distance,
                depth - 1,
                ops,
            );
        }
    }

    let end = (at.0 + old.len() - suffix, at.1 + new.len() - suffix);
    ops.extend((0..suffix).map(|ix| Op::Equal(end.0 + ix, end.1 + ix)));
}

/// The lines that occur exactly once in `old` and once in `new`, as
/// `(old, new)` index pairs, keeping the longest run whose order both sides
/// agree on.
fn unique_anchors(old: &[&str], new: &[&str]) -> Vec<(usize, usize)> {
    // Per line: its count and last index on each side.
    let mut seen: HashMap<&str, (u32, u32, usize, usize)> = HashMap::new();
    for (ix, line) in old.iter().enumerate() {
        let entry = seen.entry(line).or_default();
        entry.0 += 1;
        entry.2 = ix;
    }
    for (ix, line) in new.iter().enumerate() {
        let entry = seen.entry(line).or_default();
        entry.1 += 1;
        entry.3 = ix;
    }
    let mut pairs = seen
        .into_values()
        .filter(|&(old_count, new_count, ..)| old_count == 1 && new_count == 1)
        .map(|(_, _, old_ix, new_ix)| (old_ix, new_ix))
        .collect::<Vec<_>>();
    pairs.sort_unstable();
    longest_increasing(&pairs)
}

/// The longest subsequence of `pairs` (sorted by their first index) whose
/// second indices increase too.
fn longest_increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // For each length, the pair ending the run of that length with the
    // smallest second index.
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; pairs.len()];
    for (ix, &(_, second)) in pairs.iter().enumerate() {
        let len = tails.partition_point(|&tail| pairs[tail].1 < second);
        if len > 0 {
            previous[ix] = Some(tails[len - 1]);
        }
        if len == tails.len() {
            tails.push(ix);
        } else {
            tails[len] = ix;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(ix) = at {
        run.push(pairs[ix]);
        at = previous[ix];
    }
    run.reverse();
    run
}

/// Myers' O((N+M)D) shortest edit script, or `None` past `max_distance`
/// steps. Keeps each step's furthest-reaching paths (O(D²) memory) to walk
/// the script back.
fn myers(old: &[&str], new: &[&str], max_distance: usize) -> Option<Vec<Op>> {
    let (n, m) = (old.len() as isize, new.len() as isize);
    if n == 0 {
        return Some((0..new.len()).map(Op::Insert).collect());
    }
    if m == 0 {
        return Some((0..old.len()).map(Op::Delete).collect());
    }
    let max = (n + m) as usize;
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    // trace[d] holds v[-d..=d] as it was before step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max.min(max_distance) as isize {
        trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let ix = (offset + k) as usize;
            let mut x = if k == -d || (k != d && v[ix - 1] < v[ix + 1]) {
                v[ix + 1]
            } else {
                v[ix - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }
            v[ix] = x;
            if x >= n && y >= m {
                return Some(backtrack(&trace, n, m));
            }
            k += 2;
        }
    }
    None
}

/// Walk the furthest-reaching paths back from the end to the start.
fn backtrack(trace: &[Vec<isize>], n: isize, m: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for (d, v) in trace.iter().enumerate().rev() {
        let d = d as isize;
        if d == 0 {
            while x > 0 && y > 0 {
                x -= 1;
                y -= 1;
                ops.push(Op::Equal(x as usize, y as usize));
            }
            break;
        }
        let at = |k: isize| v[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Equal(x as usize, y as usize));
        }
        if x == prev_x {
            y -= 1;
            ops.push(Op::Insert(y as usize));
        } else {
            x -= 1;
            ops.push(Op::Delete(x as usize));
        }
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(file: &DiffFile) -> String {
        file.lines()
            .iter()
            .map(|line| {
                format!(
                    "{}{}",
                    match line.change() {
                        LineChange::Unchanged => ' ',
                        LineChange::Added => '+',
                        LineChange::Removed => '-',
                    },
                    line.text()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Applying the script to the old lines gives the new lines.
    fn apply(old: &[&str], new: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for op in line_diff(old, new) {
            match op {
                Op::Equal(a, b) => {
                    assert_eq!(old[a], new[b]);
                    out.push(old[a].to_string());
                }
                Op::Insert(b) => out.push(new[b].to_string()),
                Op::Delete(_) => {}
            }
        }
        out
    }

    #[test]
    fn line_diffs_rebuild_the_new_text_with_the_fewest_edits() {
        let cases: &[(&[&str], &[&str])] = &[
            (&[], &[]),
            (&["a"], &[]),
            (&[], &["a"]),
            (&["a", "b", "c"], &["a", "b", "c"]),
            (
                &["a", "b", "c", "a", "b", "b", "a"],
                &["c", "b", "a", "b", "a", "c"],
            ),
            (&["x", "a", "y"], &["a"]),
            (&["one", "two", "three"], &["one", "2", "three", "four"]),
        ];
        for (old, new) in cases {
            assert_eq!(apply(old, new), new.to_vec(), "{old:?} -> {new:?}");
        }
        // The classic example takes 5 edits.
        let ops = line_diff(
            &["a", "b", "c", "a", "b", "b", "a"],
            &["c", "b", "a", "b", "a", "c"],
        );
        let edits = ops.iter().filter(|op| !matches!(op, Op::Equal(..))).count();
        assert_eq!(edits, 5);
    }

    #[test]
    fn a_diff_past_the_distance_cap_replaces_the_middle() {
        let old = (0..10).map(|ix| format!("old {ix}")).collect::<Vec<_>>();
        let new = (0..10).map(|ix| format!("new {ix}")).collect::<Vec<_>>();
        let old = old.iter().map(String::as_str).collect::<Vec<_>>();
        let new = new.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(myers(&old, &new, 3), None);
        assert_eq!(myers(&old, &new, 100).map(|ops| ops.len()), Some(20));
    }

    #[test]
    fn between_two_texts_groups_changes_into_hunks_with_context() {
        let old = (1..=20).map(|n| format!("line {n}\n")).collect::<String>();
        let new = old
            .replace("line 5\n", "line five\n")
            .replace("line 18\n", "");
        let diff = Diff::between("src/lib.rs", &old, &new);
        let file = &diff.files()[0];
        assert_eq!(file.path().map(|p| p.as_ref()), Some("src/lib.rs"));
        assert_eq!((file.added_lines(), file.removed_lines()), (1, 2));
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(file.hunks[0].header.as_ref(), "@@ -2,7 +2,7 @@");
        assert_eq!(file.hunks[1].header.as_ref(), "@@ -15,6 +15,5 @@");
        // The lines between the hunks and after them can be revealed.
        assert_eq!(file.gap_before(0), 0..1);
        assert_eq!(file.gap_before(1), 9..15);
        assert!(
            file.gap_before(2).is_empty(),
            "the last hunk reaches the end"
        );
        assert!(!diff.is_empty());
        assert!(Diff::between("same", "a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn parses_git_diffs_with_several_files_and_their_numbers() {
        let diff = Diff::parse(
            "diff --git a/src/a.rs b/src/a.rs\n\
             index 1..2 100644\n\
             --- a/src/a.rs\n\
             +++ b/src/a.rs\n\
             @@ -10,4 +10,4 @@ fn main() {\n \
              keep\n\
             --- old dashes\n\
             +new\n \
              keep\n\
             \\ No newline at end of file\n\
             diff --git a/new.txt b/new.txt\n\
             new file mode 100644\n\
             --- /dev/null\n\
             +++ b/new.txt\n\
             @@ -0,0 +1,2 @@\n\
             +hello\n\
             +\n",
        );
        assert_eq!(diff.files().len(), 2);
        let a = &diff.files()[0];
        assert_eq!(a.path().map(|p| p.as_ref()), Some("src/a.rs"));
        assert_eq!(changes(a), " keep\n--- old dashes\n+new\n keep");
        let numbers = a
            .lines()
            .iter()
            .map(|l| (l.old_line(), l.new_line()))
            .collect::<Vec<_>>();
        assert_eq!(
            numbers,
            vec![
                (Some(10), Some(10)),
                (Some(11), None),
                (None, Some(11)),
                (Some(12), Some(12)),
            ]
        );
        let new = &diff.files()[1];
        assert_eq!(new.old_path(), None);
        assert_eq!(new.new_path().map(|p| p.as_ref()), Some("new.txt"));
        assert_eq!((new.added_lines(), new.removed_lines()), (2, 0));
        assert_eq!((diff.added_lines(), diff.removed_lines()), (3, 1));
        // A parsed diff knows no lines outside its hunks.
        assert!(a.gap_before(0).is_empty() && a.gap_before(1).is_empty());
    }

    #[test]
    fn parses_bare_hunks_and_round_trips_to_unified_text() {
        let diff = Diff::parse("@@ -1,2 +1,2 @@\n-a\n+b\n c\n");
        assert_eq!(diff.files().len(), 1);
        assert_eq!(diff.files()[0].path(), None);
        let unified = Diff::between("f.txt", "a\nc\n", "b\nc\n").to_unified();
        assert_eq!(
            unified,
            "--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n"
        );
        assert_eq!(Diff::parse(&unified).to_unified(), unified);
    }

    fn lines_of(file: &DiffFile) -> Vec<(LineChange, &str, LineEnding)> {
        file.lines()
            .iter()
            .map(|line| (line.change(), line.text().as_ref(), line.ending()))
            .collect()
    }

    /// Past the edit cap, unique lines anchor the diff: a change on every
    /// 13th line of 10,000 shows as those lines, not the whole file.
    #[test]
    fn many_changes_in_a_long_file_stay_local() {
        for (every, changed) in [(13, 770), (10, 1000)] {
            let line = |ix: usize, renamed: bool| {
                let name = if renamed { "renamed" } else { "value" };
                format!("    let {name}_{ix} = compute({ix});\n")
            };
            let old = (0..10_000).map(|ix| line(ix, false)).collect::<String>();
            let new = (0..10_000)
                .map(|ix| line(ix, ix % every == 0))
                .collect::<String>();
            let diff = Diff::between("f.rs", &old, &new);
            assert_eq!(
                (diff.added_lines(), diff.removed_lines()),
                (changed, changed),
                "every {every}th line"
            );
        }
    }

    /// xorshift, for test cases without a crate.
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    /// With the search cut short, the anchored script still rebuilds the
    /// new text, whatever repeats.
    #[test]
    fn a_capped_search_still_rebuilds_the_new_text() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let alphabet = ["a", "b", "c", "", "}", "d", "e", "f"];
        for case in 0..2000 {
            let mut side = || {
                (0..rng.below(16))
                    .map(|_| alphabet[rng.below(8) as usize])
                    .collect::<Vec<_>>()
            };
            let (old, new) = (side(), side());
            for max_distance in [0, 1, 2] {
                let mut rebuilt = Vec::new();
                for op in line_diff_within(&old, &new, max_distance) {
                    match op {
                        Op::Equal(a, b) => {
                            assert_eq!(old[a], new[b], "case {case}");
                            rebuilt.push(new[b]);
                        }
                        Op::Insert(b) => rebuilt.push(new[b]),
                        Op::Delete(_) => {}
                    }
                }
                assert_eq!(rebuilt, new, "case {case}");
            }
        }
    }

    /// A line whose break changes, or a final newline added, is a change,
    /// and its copy keeps the break and git's marker.
    #[test]
    fn line_breaks_and_final_newlines_are_changes() {
        let crlf = Diff::between("f", "a\r\nb\r\n", "a\nb\n");
        assert!(!crlf.is_empty());
        assert_eq!(
            lines_of(&crlf.files()[0]),
            vec![
                (LineChange::Removed, "a", LineEnding::CrLf),
                (LineChange::Removed, "b", LineEnding::CrLf),
                (LineChange::Added, "a", LineEnding::Lf),
                (LineChange::Added, "b", LineEnding::Lf),
            ]
        );
        let unified = crlf.to_unified();
        assert!(unified.contains("-a\r\n-b\r\n+a\n+b\n"), "{unified:?}");
        assert_eq!(Diff::parse(&unified), crlf);

        let final_newline = Diff::between("f", "a\nb", "a\nb\n");
        assert_eq!(
            lines_of(&final_newline.files()[0]),
            vec![
                (LineChange::Unchanged, "a", LineEnding::Lf),
                (LineChange::Removed, "b", LineEnding::Missing),
                (LineChange::Added, "b", LineEnding::Lf),
            ]
        );
        assert_eq!(
            final_newline.to_unified(),
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+b\n"
        );
    }

    #[test]
    fn the_no_newline_marker_survives_a_round_trip() {
        let text = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+a\n";
        let diff = Diff::parse(text);
        assert_eq!((diff.added_lines(), diff.removed_lines()), (1, 1));
        assert_eq!(diff.files()[0].lines()[0].ending(), LineEnding::Missing);
        assert_eq!(diff.to_unified(), text);
    }

    /// A hunk ends where its counts do: a `format-patch` signature and the
    /// next commit of `git log -p` stay out of it.
    #[test]
    fn a_hunk_ends_where_its_counts_do() {
        let patch = "From 1234 Mon Sep 17 00:00:00 2001\nSubject: [PATCH] x\n\n---\n \
                     f.txt | 2 +-\n\ndiff --git a/f.txt b/f.txt\nindex 1..2 100644\n\
                     --- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n-- \n2.39.0\n\n";
        let diff = Diff::parse(patch);
        assert_eq!(diff.files().len(), 1);
        assert_eq!(
            lines_of(&diff.files()[0]),
            vec![
                (LineChange::Removed, "a", LineEnding::Lf),
                (LineChange::Added, "b", LineEnding::Lf),
                (LineChange::Unchanged, "c", LineEnding::Lf),
            ]
        );

        let log = "commit aaaa\nAuthor: A <a@b>\n\n    first\n\ndiff --git a/f.txt b/f.txt\n\
                   --- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n\ncommit bbbb\n\n    \
                   second commit message\n\ndiff --git a/g.txt b/g.txt\n--- a/g.txt\n\
                   +++ b/g.txt\n@@ -1 +1 @@\n-x\n+y\n";
        let diff = Diff::parse(log);
        let counts = diff
            .files()
            .iter()
            .map(|file| (file.path().unwrap().to_string(), file.lines().len()))
            .collect::<Vec<_>>();
        assert_eq!(counts, vec![("f.txt".into(), 3), ("g.txt".into(), 2)]);

        // Hand-written diffs miscount: extra changes still belong.
        let miscounted = Diff::parse("@@ -1,1 +1,1 @@\n-a\n+b\n+c\n");
        assert_eq!(miscounted.added_lines(), 2);
    }

    /// Untrusted numbers neither panic nor wrap.
    #[test]
    fn hunk_numbers_saturate() {
        let diff = Diff::parse("@@ -4294967295,2 +1,2 @@\n a\n b\n");
        let numbers = diff.files()[0]
            .lines()
            .iter()
            .map(|line| line.old_line())
            .collect::<Vec<_>>();
        assert_eq!(numbers, vec![Some(u32::MAX), Some(u32::MAX)]);
    }

    #[test]
    fn a_broken_hunk_header_and_a_header_without_hunks_stand_alone() {
        let diff =
            Diff::parse("--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+b\n@@ -x,y +z @@\n-c\n+d\n");
        assert_eq!(diff.files()[0].lines().len(), 2, "c and d have no hunk");

        let diff = Diff::parse("--- a/x\n+++ b/x\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-a\n+b\n");
        let paths = diff
            .files()
            .iter()
            .map(|file| file.path().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["x", "y"]);
    }

    /// git's extended headers: a rename, a mode change, a binary file and a
    /// quoted path.
    #[test]
    fn renames_modes_binary_files_and_quoted_paths() {
        let diff = Diff::parse(
            "diff --git a/old.rs b/new.rs\nsimilarity index 100%\nrename from old.rs\n\
             rename to new.rs\ndiff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n\
             diff --git a/img.png b/img.png\nindex 1..2 100644\n\
             Binary files a/img.png and b/img.png differ\n\
             diff --git \"a/sp ace\\303\\266.txt\" \"b/sp ace\\303\\266.txt\"\n\
             --- \"a/sp ace\\303\\266.txt\"\n+++ \"b/sp ace\\303\\266.txt\"\n@@ -1 +1 @@\n-a\n+b\n",
        );
        let [rename, mode, binary, quoted] = diff.files() else {
            panic!("four files: {:?}", diff.files());
        };
        assert!(rename.is_rename());
        assert_eq!(rename.old_path().unwrap().as_ref(), "old.rs");
        assert_eq!(rename.new_path().unwrap().as_ref(), "new.rs");
        assert_eq!(
            mode.mode_change()
                .map(|(old, new)| (old.as_ref(), new.as_ref())),
            Some(("100644", "100755"))
        );
        assert!(binary.is_binary());
        assert_eq!(quoted.path().unwrap().as_ref(), "sp aceö.txt");
        assert_eq!(quoted.added_lines(), 1);

        let copies = (0..3).map(|ix| diff.file_unified(ix)).collect::<Vec<_>>();
        assert_eq!(
            copies,
            vec![
                "diff --git a/old.rs b/new.rs\nrename from old.rs\nrename to new.rs\n",
                "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n",
                "diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n",
            ]
        );
        for (ix, copy) in copies.iter().enumerate() {
            assert_eq!(Diff::parse(copy).files()[0], diff.files()[ix]);
        }
        assert!(!Diff::parse(&copies[0]).is_empty(), "a rename is a change");
        assert_eq!(
            quoted_path("a/", "say \"hi\".txt"),
            "\"a/say \\\"hi\\\".txt\""
        );
        assert_eq!(
            unquote("\"a/say \\\"hi\\\".txt\"").map(|(path, _)| path),
            Some("a/say \"hi\".txt".to_string())
        );
    }

    /// A hunk that holds more lines than its header said (hand-written
    /// diffs often miscount) is copied with counts that match its lines,
    /// and with whatever followed its header.
    #[test]
    fn a_miscounted_hunk_is_copied_with_counts_that_match() {
        let diff = Diff::parse("--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@ fn main\n-a\n+b\n+c\n");
        assert_eq!(
            diff.to_unified(),
            "--- a/f\n+++ b/f\n@@ -1,1 +1,2 @@ fn main\n-a\n+b\n+c\n"
        );
        // A header that matches is copied as it was, count left out and all.
        let exact = "--- a/f\n+++ b/f\n@@ -3 +3 @@\n-a\n+b\n";
        assert_eq!(Diff::parse(exact).to_unified(), exact);
    }
}
