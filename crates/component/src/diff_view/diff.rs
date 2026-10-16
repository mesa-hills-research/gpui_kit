//! The diff model: files, hunks and lines, parsed from unified diff text or
//! computed between two texts. Plain data, so a large diff can be built off
//! the UI thread.

use std::ops::Range;

use gpui::SharedString;

/// How many unchanged lines a computed diff keeps around each change.
pub(super) const CONTEXT_LINES: usize = 3;

/// A diff that grows past this many edit steps between two texts gives up
/// on finding the shortest one and shows the differing middle as removed,
/// then added: the search costs time and memory with the square of it.
const MAX_EDIT_DISTANCE: usize = 1500;

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

/// One line of a diff, with its numbers in the old and new versions.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    change: LineChange,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: SharedString,
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
    /// file, so the unchanged lines between hunks can be shown on request;
    /// a parsed diff knows only its hunks' lines.
    lines: Vec<DiffLine>,
    pub(super) hunks: Vec<DiffHunk>,
    added: usize,
    removed: usize,
}

impl DiffFile {
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
    /// previous one), which a viewer can reveal; `ix == hunks.len()` gives
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

    /// The file in unified diff format.
    fn write_unified(&self, out: &mut String) {
        out.push_str(&format!(
            "--- {}\n+++ {}\n",
            self.old_path
                .as_ref()
                .map_or("/dev/null".to_string(), |path| format!("a/{path}")),
            self.new_path
                .as_ref()
                .map_or("/dev/null".to_string(), |path| format!("b/{path}")),
        ));
        for hunk in &self.hunks {
            out.push_str(&hunk.header);
            out.push('\n');
            for line in &self.lines[hunk.lines.clone()] {
                out.push(match line.change {
                    LineChange::Unchanged => ' ',
                    LineChange::Added => '+',
                    LineChange::Removed => '-',
                });
                out.push_str(&line.text);
                out.push('\n');
            }
        }
    }
}

/// A diff of one or more files, for a [`DiffView`](super::DiffView).
///
/// Build it from unified diff text with [`Diff::parse`], or from the old and
/// new text of a file with [`Diff::between`]. Both are plain computations;
/// for a large input, build the diff on a background thread and hand it to
/// the view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    files: Vec<DiffFile>,
}

impl Diff {
    /// Parse unified diff text: `git diff` output or `diff -u`, one file or
    /// many, or bare `@@` hunks. Lines that are neither file headers, hunk
    /// headers nor hunk lines are skipped.
    pub fn parse(text: &str) -> Self {
        let mut files = Vec::new();
        let mut file: Option<DiffFile> = None;
        let (mut old_line, mut new_line) = (0u32, 0u32);
        // The lines the current hunk's header says are left on each side.
        let (mut old_left, mut new_left) = (0u32, 0u32);
        let mut in_hunk = false;

        let new_file = || DiffFile {
            old_path: None,
            new_path: None,
            lines: Vec::new(),
            hunks: Vec::new(),
            added: 0,
            removed: 0,
        };

        for raw in text.lines() {
            let hunk_open = in_hunk && (old_left > 0 || new_left > 0);
            if raw.starts_with("diff --git ") {
                finish_file(file.take(), &mut files);
                let mut started = new_file();
                if let Some((old, new)) = git_paths(raw) {
                    started.old_path = Some(old);
                    started.new_path = Some(new);
                }
                file = Some(started);
                in_hunk = false;
                continue;
            }
            if !hunk_open && let Some(path) = raw.strip_prefix("--- ") {
                // A header after hunks starts the next file, unless a
                // `diff --git` line already did.
                if file.as_ref().is_some_and(|file| !file.hunks.is_empty()) {
                    finish_file(file.take(), &mut files);
                }
                file.get_or_insert_with(new_file).old_path = header_path(path, "a/");
                in_hunk = false;
                continue;
            }
            if !hunk_open && let Some(path) = raw.strip_prefix("+++ ") {
                file.get_or_insert_with(new_file).new_path = header_path(path, "b/");
                in_hunk = false;
                continue;
            }
            if raw.starts_with("@@") {
                let Some(header) = hunk_header(raw) else {
                    continue;
                };
                let file = file.get_or_insert_with(new_file);
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.lines.end = file.lines.len();
                }
                let start = file.lines.len();
                file.hunks.push(DiffHunk {
                    header: raw.to_string().into(),
                    lines: start..start,
                });
                (old_line, old_left, new_line, new_left) = header;
                in_hunk = true;
                continue;
            }
            // Past the counts its header gave, a hunk still takes lines that
            // look like its own: hand-written diffs often miscount.
            if !in_hunk {
                continue;
            }
            let Some(file) = file.as_mut() else {
                continue;
            };
            let (change, text) = match raw.as_bytes().first() {
                Some(b'+') => (LineChange::Added, &raw[1..]),
                Some(b'-') => (LineChange::Removed, &raw[1..]),
                Some(b' ') => (LineChange::Unchanged, &raw[1..]),
                // An empty line inside a hunk is an unchanged empty line
                // whose leading space an editor trimmed.
                None => (LineChange::Unchanged, ""),
                // "\ No newline at end of file" and anything else.
                _ => continue,
            };
            let (old, new) = match change {
                LineChange::Unchanged => (Some(old_line), Some(new_line)),
                LineChange::Added => (None, Some(new_line)),
                LineChange::Removed => (Some(old_line), None),
            };
            if old.is_some() {
                old_line += 1;
                old_left = old_left.saturating_sub(1);
            }
            if new.is_some() {
                new_line += 1;
                new_left = new_left.saturating_sub(1);
            }
            file.lines.push(DiffLine {
                change,
                old_line: old,
                new_line: new,
                text: text.to_string().into(),
            });
        }
        finish_file(file, &mut files);
        files.retain(|file| !file.hunks.is_empty() || file.path().is_some());
        Self { files }
    }

    /// The line diff between two versions of the file at `path`.
    ///
    /// The diff knows the whole file: changes are grouped into hunks with
    /// three unchanged lines around each, and a [`DiffView`](super::DiffView)
    /// can reveal the unchanged lines between them.
    pub fn between(path: impl Into<SharedString>, old: &str, new: &str) -> Self {
        let path = path.into();
        let old_lines = old.lines().collect::<Vec<_>>();
        let new_lines = new.lines().collect::<Vec<_>>();
        let mut lines = Vec::with_capacity(old_lines.len().max(new_lines.len()));
        let (mut old_line, mut new_line) = (1u32, 1u32);
        for op in line_diff(&old_lines, &new_lines) {
            let (change, text) = match op {
                Op::Equal(ix, _) => (LineChange::Unchanged, old_lines[ix]),
                Op::Delete(ix) => (LineChange::Removed, old_lines[ix]),
                Op::Insert(ix) => (LineChange::Added, new_lines[ix]),
            };
            let (old_no, new_no) = match change {
                LineChange::Unchanged => (Some(old_line), Some(new_line)),
                LineChange::Added => (None, Some(new_line)),
                LineChange::Removed => (Some(old_line), None),
            };
            old_line += old_no.is_some() as u32;
            new_line += new_no.is_some() as u32;
            lines.push(DiffLine {
                change,
                old_line: old_no,
                new_line: new_no,
                text: text.to_string().into(),
            });
        }
        let hunks = hunks_with_context(&lines, CONTEXT_LINES);
        let mut file = DiffFile {
            old_path: Some(path.clone()),
            new_path: Some(path),
            lines,
            hunks,
            added: 0,
            removed: 0,
        };
        file.count();
        Self { files: vec![file] }
    }

    /// The files, in order.
    pub fn files(&self) -> &[DiffFile] {
        &self.files
    }

    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.files
            .iter()
            .all(|file| file.added == 0 && file.removed == 0)
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
    /// copies; empty for an index out of range.
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

/// The paths of a `diff --git a/old b/new` line.
fn git_paths(line: &str) -> Option<(SharedString, SharedString)> {
    let rest = line.strip_prefix("diff --git a/")?;
    let (old, new) = rest.split_once(" b/")?;
    Some((old.to_string().into(), new.to_string().into()))
}

/// The path of a `---` or `+++` header, without its `a/` or `b/` prefix and
/// any tab-separated timestamp; `None` for `/dev/null`.
fn header_path(path: &str, prefix: &str) -> Option<SharedString> {
    let path = path.split('\t').next().unwrap_or(path).trim_end();
    if path == "/dev/null" {
        return None;
    }
    Some(path.strip_prefix(prefix).unwrap_or(path).to_string().into())
}

/// The old start and count and the new start and count of a
/// `@@ -a,b +c,d @@` header; a count left out is 1.
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

/// The shortest edit script from `old` to `new` (Myers), after trimming the
/// lines they share at the start and end. Past [`MAX_EDIT_DISTANCE`], the
/// differing middle is removed and then added as a whole.
fn line_diff(old: &[&str], new: &[&str]) -> Vec<Op> {
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
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];

    let mut ops = (0..prefix).map(|ix| Op::Equal(ix, ix)).collect::<Vec<_>>();
    match myers(old_mid, new_mid, MAX_EDIT_DISTANCE) {
        Some(middle) => ops.extend(middle.into_iter().map(|op| match op {
            Op::Equal(a, b) => Op::Equal(a + prefix, b + prefix),
            Op::Delete(a) => Op::Delete(a + prefix),
            Op::Insert(b) => Op::Insert(b + prefix),
        })),
        None => {
            ops.extend((0..old_mid.len()).map(|ix| Op::Delete(ix + prefix)));
            ops.extend((0..new_mid.len()).map(|ix| Op::Insert(ix + prefix)));
        }
    }
    ops.extend((0..suffix).map(|ix| Op::Equal(old.len() - suffix + ix, new.len() - suffix + ix)));
    ops
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
}
