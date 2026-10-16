---
title: Diff View
description: Show the changes to files with line numbers, added and removed lines, hunk headers and syntax colors.
---

# Diff View

Diff View shows the changes to one or more files: each file's path with its added and removed counts and a Copy button, hunk headers, and the lines with their old and new numbers. Added lines sit on the success color and removed lines on the danger color, at low opacity, and each is also marked `+` or `-`, so the change never rests on color alone. Lines use the theme's code font and syntax colors chosen by the file's extension. Long diffs scroll smoothly: only the rows on screen are laid out.

## Import

```rust
use gpui_kit::component::diff_view::{Diff, DiffView, DiffViewState};
```

## Usage

Build a `Diff`, keep it in a `DiffViewState`, and render a `DiffView`:

```rust
// Between two versions of a file:
let diff = Diff::between("src/net.rs", &old_text, &new_text);
// Or from unified diff text (`git diff`, `diff -u`, or bare `@@` hunks):
let diff = Diff::parse(&unified_text);

let state = cx.new(|cx| DiffViewState::new(diff, cx));

DiffView::new(&state).h(px(360.))
```

`Diff::between` computes a line diff (Myers' shortest edit script) and groups the changes into hunks with three unchanged lines around each. It knows the whole file, so the unchanged lines between hunks fold into a row that unfolds them in place, and `unfold_all` unfolds every one. A run of a single line shows that line instead of a row to unfold it. A parsed diff knows only its hunks' lines, so between its hunks there is only the header.

Both are plain computations on data you own. For a large input, build the `Diff` on a background thread and hand it to `DiffViewState::new` or `set_diff`. Measured in a debug build: a 10,000-line file with 100 scattered edits diffs in about 4 ms, and two unrelated 5,000-line files in about 28 ms. Past 1,500 edits the search stops, the lines that occur once in each version anchor the diff, and the stretches between them are diffed on their own. A change to every tenth line of a long file still shows as those lines.

## Sizing

By default the view fills its parent, or takes the size you give it, and scrolls inside. Inside a list whose items vary in height, such as a tool call's edit in a transcript, let it take the height of its rows with `max_rows`:

```rust
DiffView::new(&state).max_rows(24)
```

A five-line edit then takes five rows (plus its file row and hunk header), and a long one takes 24 and scrolls. `DiffView::row_height(cx)` gives the height of a row for sizing it yourself.

## Keyboard

The view is a Tab stop, and a click inside it focuses it too. Focused, `↑` and `↓` scroll a row, `←` and `→` scroll sideways, `Page Up` and `Page Down` scroll a page, and `Home` and `End` go to the top and the bottom.

## What it shows

- **Line breaks.** A line whose break changed between `\n` and `\r\n` is a change. When the changed lines of a file end in both ways, each shows `CRLF` or `LF` after its text.
- **The final newline.** A last line without a line break shows "No newline at end of file" where it changed, and the copied diff keeps git's `\ No newline at end of file` marker.
- **Renames, modes and binary files.** From `git diff` headers, a renamed file shows `old → new`, a mode change shows `mode 100644 → 100755`, and a binary file says so, in place of line counts it doesn't have. Quoted paths, as git writes names with unusual characters, are unquoted.
- **Long lines.** A line longer than 2,000 characters, such as a minified bundle's, shows its first 2,000 and how many more it holds.

A parsed hunk ends where its header's counts are spent, though it takes in further added and removed lines, as hand-written diffs often miscount. So the output of `git log -p` or `git format-patch` parses without commit messages or signatures in its hunks.

## Syntax colors

The highlight names (`string`, `comment` and so on) are found once, on a background thread, for each side of the diff: the unchanged and removed lines as the old version, the unchanged and added lines as the new one, so a string or comment spanning lines colors correctly within a hunk. Each row looks up their colors in the active highlight theme as it paints, so a theme switch is a repaint. Without the `tree-sitter` feature, or for an extension with no grammar, the lines are plain.

## Copying

Each file's Copy button copies that file's changes as unified diff text, built only when clicked. `Diff::to_unified` gives the whole diff.

## API Reference

| Item | Description |
| --- | --- |
| `Diff::between(path, old, new)` | The line diff between two versions of a file |
| `Diff::parse(text)` | Parse unified diff text, one file or many |
| `Diff::files()` | The files: each has `path()`, `old_path()`, `new_path()`, `lines()`, `added_lines()`, `removed_lines()` |
| `DiffFile::is_rename()` / `is_binary()` / `mode_change()` | What git's headers say about a file |
| `Diff::added_lines()` / `removed_lines()` / `is_empty()` | Totals |
| `Diff::to_unified()` / `file_unified(ix)` | Unified diff text |
| `DiffLine::change()` / `old_line()` / `new_line()` / `text()` | One line: `LineChange::{Unchanged, Added, Removed}` and its numbers |
| `DiffLine::ending()` | How it ends: `LineEnding::{Lf, CrLf, Missing}` |
| `DiffViewState::new(diff, cx)` / `set_diff(diff, cx)` | Show a diff, folded, from the top |
| `DiffViewState::unfold_all(cx)` | Unfold every run of unchanged lines |
| `DiffView::new(&state)` | The view, `Styled` for its size |
| `DiffView::max_rows(n)` | Take the height of the rows, up to `n` |
| `DiffView::row_height(cx)` | The height of one row |
