//! A diff of one or more files: line numbers, added and removed lines,
//! hunk headers, syntax colors and unchanged lines that unfold.

mod diff;

pub use diff::{Diff, DiffFile, DiffLine, LineChange, LineEnding};

use std::{collections::HashSet, ops::Range, sync::Arc};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, HighlightStyle, InteractiveElement as _,
    IntoElement, KeyBinding, ListHorizontalSizingBehavior, ParentElement as _, Pixels, Render,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled,
    StyledText, Task, UniformListScrollHandle, Window, div, point, prelude::FluentBuilder as _, px,
    uniform_list,
};
use gpui_base::{TestSupportExt as _, input::HighlightStyleResolver as _};
use rust_i18n::t;

use crate::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, ThemeStyled as _,
    actions::{
        SelectDown, SelectFirst, SelectLast, SelectLeft, SelectPageDown, SelectPageUp, SelectRight,
        SelectUp,
    },
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    highlighter::{HighlightTheme, SyntaxHighlighter},
    v_flex,
};

const CONTEXT: &str = "DiffView";

/// The characters of a line a view shows. A longer line, such as a
/// minified bundle's, shows its start and how much more it holds.
const MAX_LINE_CHARS: usize = 2000;

/// How far Left and Right scroll a diff sideways.
const SIDE_STEP: Pixels = px(48.);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("pageup", SelectPageUp, Some(CONTEXT)),
        KeyBinding::new("pagedown", SelectPageDown, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
    ]);
}

/// A row of the flattened view.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    /// A file's path, counts and Copy button.
    File(usize),
    /// A hunk's header, with the unchanged lines before it that can unfold.
    Hunk {
        file: usize,
        hunk: usize,
        hidden: usize,
    },
    /// Unchanged lines after the last hunk that can unfold.
    Tail { file: usize, hidden: usize },
    /// A line, by index into its file's lines.
    Line { file: usize, line: usize },
}

/// The syntax highlight names of one file's lines, by line index, such as
/// `string` or `comment`. They are found once, off the UI thread, and
/// colored from the theme as each row paints, so a theme switch is a
/// repaint.
#[derive(Default)]
struct FileHighlights {
    lines: Vec<Vec<(Range<usize>, SharedString)>>,
}

/// A diff shown by a [`DiffView`]: the diff, which unchanged runs the user
/// unfolded, the syntax highlights and the scroll position.
pub struct DiffViewState {
    diff: Arc<Diff>,
    /// Unfolded gaps, as `(file, gap)`. Gap `n` is the run before hunk `n`.
    unfolded: HashSet<(usize, usize)>,
    rows: Vec<Row>,
    /// The row with the longest text, measured for the horizontal scroll.
    widest_row: usize,
    /// The largest line number, for the width of the number columns.
    max_line: u32,
    /// Per file, whether its changed lines end in more than one way, so
    /// each shows how it ends.
    mixed_endings: Vec<bool>,
    scroll_handle: UniformListScrollHandle,
    focus_handle: FocusHandle,
    highlights: Vec<FileHighlights>,
    _highlight_task: Option<Task<()>>,
}

impl Focusable for DiffViewState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DiffViewState {
    /// A view of `diff`, its hunks shown and the unchanged lines between
    /// them folded.
    pub fn new(diff: Diff, cx: &mut Context<Self>) -> Self {
        let mut state = Self {
            diff: Arc::new(diff),
            unfolded: HashSet::new(),
            rows: Vec::new(),
            widest_row: 0,
            max_line: 1,
            mixed_endings: Vec::new(),
            scroll_handle: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle().tab_stop(true),
            highlights: Vec::new(),
            _highlight_task: None,
        };
        state.measure_diff();
        state.rebuild_rows();
        state.highlight(cx);
        state
    }

    /// The diff shown.
    pub fn diff(&self) -> &Diff {
        &self.diff
    }

    /// Show another diff, folded, from the top.
    pub fn set_diff(&mut self, diff: Diff, cx: &mut Context<Self>) {
        self.diff = Arc::new(diff);
        self.unfolded.clear();
        self.measure_diff();
        self.rebuild_rows();
        self.highlight(cx);
        self.scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// Unfold every run of unchanged lines the diff knows.
    pub fn unfold_all(&mut self, cx: &mut Context<Self>) {
        for (file_ix, file) in self.diff.files().iter().enumerate() {
            for gap in 0..=file.hunks.len() {
                self.unfolded.insert((file_ix, gap));
            }
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// Unfold the unchanged lines before hunk `gap` of `file`, or after the
    /// last hunk when `gap` is the number of hunks.
    fn unfold(&mut self, file: usize, gap: usize, cx: &mut Context<Self>) {
        if self.unfolded.insert((file, gap)) {
            self.rebuild_rows();
            cx.notify();
        }
    }

    /// The number of rows the view lays out, unfolded lines included.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// What the diff needs measured once: its largest line number, and
    /// which files mix line endings among their changes.
    fn measure_diff(&mut self) {
        self.max_line = self
            .diff
            .files()
            .iter()
            .flat_map(|file| file.lines().iter())
            .flat_map(|line| [line.old_line(), line.new_line()])
            .flatten()
            .max()
            .unwrap_or(1);
        self.mixed_endings = self
            .diff
            .files()
            .iter()
            .map(|file| {
                let mut endings = file
                    .lines()
                    .iter()
                    .filter(|line| line.change() != LineChange::Unchanged)
                    .map(|line| line.ending())
                    .filter(|ending| *ending != LineEnding::Missing);
                endings
                    .next()
                    .is_some_and(|first| endings.any(|ending| ending != first))
            })
            .collect();
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (file_ix, file) in self.diff.files().iter().enumerate() {
            rows.push(Row::File(file_ix));
            let lines = |range: Range<usize>| {
                range.map(move |line| Row::Line {
                    file: file_ix,
                    line,
                })
            };
            for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
                let gap = file.gap_before(hunk_ix);
                // A one-line gap shows its line: a button would take its row.
                let unfolded = gap.len() == 1 || self.unfolded.contains(&(file_ix, hunk_ix));
                if unfolded {
                    rows.extend(lines(gap.clone()));
                }
                rows.push(Row::Hunk {
                    file: file_ix,
                    hunk: hunk_ix,
                    hidden: if unfolded { 0 } else { gap.len() },
                });
                rows.extend(lines(hunk.lines.clone()));
            }
            let tail = file.gap_before(file.hunks.len());
            if !tail.is_empty() {
                if tail.len() == 1 || self.unfolded.contains(&(file_ix, file.hunks.len())) {
                    rows.extend(lines(tail));
                } else {
                    rows.push(Row::Tail {
                        file: file_ix,
                        hidden: tail.len(),
                    });
                }
            }
        }
        // Measured in characters, as wide characters and ASCII take one
        // monospace cell or so each, and as a long line shows its start.
        self.widest_row = rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| match row {
                Row::Line { file, line } => self.diff.files()[*file].lines()[*line]
                    .text()
                    .chars()
                    .take(MAX_LINE_CHARS + 1)
                    .count(),
                Row::Hunk { file, hunk, .. } => {
                    self.diff.files()[*file].hunks[*hunk].header.chars().count()
                }
                _ => 0,
            })
            .map_or(0, |(ix, _)| ix);
        self.rows = rows;
    }

    /// Find the syntax highlight names of the diff on a background thread.
    fn highlight(&mut self, cx: &mut Context<Self>) {
        self.highlights.clear();
        let diff = self.diff.clone();
        let task = cx
            .background_executor()
            .spawn(async move { diff.files().iter().map(highlight_file).collect::<Vec<_>>() });
        self._highlight_task = Some(cx.spawn(async move |this, cx| {
            let highlights = task.await;
            _ = this.update(cx, |this, cx| {
                this.highlights = highlights;
                cx.notify();
            });
        }));
    }

    /// Scroll by `delta`, up and left positive, within the diff.
    fn scroll_by(&mut self, delta: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let handle = self.scroll_handle.0.borrow().base_handle.clone();
        let (offset, max) = (handle.offset(), handle.max_offset());
        handle.set_offset(point(
            (offset.x + delta.x).clamp(-max.x, px(0.)),
            (offset.y + delta.y).clamp(-max.y, px(0.)),
        ));
        cx.notify();
    }

    /// The height of the visible rows, less one row, as Page Up and Page
    /// Down scroll.
    fn page(&self, cx: &App) -> Pixels {
        let row = DiffView::row_height(cx);
        let viewport = self
            .scroll_handle
            .0
            .borrow()
            .base_handle
            .bounds()
            .size
            .height;
        (viewport - row).max(row)
    }
}

impl Render for DiffViewState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The language of a path, by its extension or file name.
fn language_of(path: &str) -> SharedString {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, extension)) => crate::highlighter::language_name(extension),
        None => crate::highlighter::language_name(name),
    }
}

/// The syntax highlight names of every line of `file`. Each side of the
/// diff is highlighted as one text, the unchanged and removed lines as the
/// old version and the unchanged and added lines as the new, so a string or
/// comment spanning lines highlights right within a hunk.
fn highlight_file(file: &DiffFile) -> FileHighlights {
    let language = file
        .path()
        .map(|path| language_of(path))
        .unwrap_or_else(|| "text".into());
    let mut lines = vec![Vec::new(); file.lines().len()];
    if file.lines().is_empty() {
        return FileHighlights { lines };
    }
    for side in [LineChange::Removed, LineChange::Added] {
        // The lines of this side that take its colors: unchanged lines take
        // the new side's.
        let indices = file
            .lines()
            .iter()
            .enumerate()
            .filter(|(_, line)| line.change() == LineChange::Unchanged || line.change() == side)
            .map(|(ix, _)| ix)
            .collect::<Vec<_>>();
        let mut text = String::new();
        let mut starts = Vec::with_capacity(indices.len());
        for &ix in &indices {
            starts.push(text.len());
            text.push_str(file.lines()[ix].text());
            text.push('\n');
        }
        let rope = ropey::Rope::from_str(&text);
        let mut highlighter = SyntaxHighlighter::new(&language);
        highlighter.update(None, &rope, None);

        for (range, name) in highlighter.highlight_names(&(0..text.len())) {
            // The lines it covers, as a block comment may cover several.
            let first = starts.partition_point(|&start| start <= range.start) - 1;
            for (position, &ix) in indices.iter().enumerate().skip(first) {
                let start = starts[position];
                if start >= range.end {
                    break;
                }
                let takes_colors =
                    side == LineChange::Added || file.lines()[ix].change() != LineChange::Unchanged;
                let end = start + file.lines()[ix].text().len();
                let clipped = range.start.max(start) - start..range.end.min(end).max(start) - start;
                if takes_colors && !clipped.is_empty() {
                    lines[ix].push((clipped, name.clone()));
                }
            }
        }
    }
    FileHighlights { lines }
}

/// The styles of a line's highlight names in `theme`, where a later name
/// covers an earlier one it overlaps, cut to the line's first `len` bytes.
fn resolve_highlights(
    names: &[(Range<usize>, SharedString)],
    len: usize,
    theme: &HighlightTheme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut bounds = names
        .iter()
        .flat_map(|(range, _)| [range.start.min(len), range.end.min(len)])
        .collect::<Vec<_>>();
    bounds.sort_unstable();
    bounds.dedup();
    let mut styles: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for pair in bounds.windows(2) {
        let segment = pair[0]..pair[1];
        let mut style = HighlightStyle::default();
        for (range, name) in names {
            if range.start <= segment.start
                && segment.end <= range.end
                && let Some(named) = theme.style(name)
            {
                merge_style(&mut style, &named);
            }
        }
        if style == HighlightStyle::default() {
            continue;
        }
        match styles.last_mut() {
            Some((last, last_style)) if last.end == segment.start && *last_style == style => {
                last.end = segment.end;
            }
            _ => styles.push((segment, style)),
        }
    }
    styles
}

/// Set on `style` every field `other` sets.
fn merge_style(style: &mut HighlightStyle, other: &HighlightStyle) {
    style.color = other.color.or(style.color);
    style.font_weight = other.font_weight.or(style.font_weight);
    style.font_style = other.font_style.or(style.font_style);
    style.background_color = other.background_color.or(style.background_color);
    style.underline = other.underline.or(style.underline);
    style.strikethrough = other.strikethrough.or(style.strikethrough);
    style.fade_out = other.fade_out.or(style.fade_out);
}

/// A virtualized view of a [`Diff`]: each file's path with its counts and a
/// Copy button, hunk headers, and lines with their old and new numbers,
/// added lines on the success color and removed lines on the danger color
/// (each also marked `+` or `-`, so the change never rests on color alone),
/// in the code font with syntax colors by file extension. Unchanged lines
/// between hunks fold into a row that unfolds them.
///
/// Give it a height (it scrolls inside), let it fill its parent, or let it
/// take the height of its rows with [`DiffView::max_rows`]. Focused, the
/// arrow keys, Page Up, Page Down, Home and End scroll it.
#[derive(IntoElement)]
pub struct DiffView {
    state: Entity<DiffViewState>,
    max_rows: Option<usize>,
    style: StyleRefinement,
}

impl DiffView {
    /// The view of `state`.
    pub fn new(state: &Entity<DiffViewState>) -> Self {
        Self {
            state: state.clone(),
            max_rows: None,
            style: StyleRefinement::default(),
        }
    }

    /// Take the height of the rows, up to `max_rows` of them, and scroll
    /// past that: for a diff inside a list of variable-height items, such as
    /// a tool call's edit. Without it, the view fills its parent.
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// The height of one row in the current theme, to size a view yourself.
    pub fn row_height(cx: &App) -> Pixels {
        (cx.theme().mono_font_size * 1.6).round()
    }
}

impl Styled for DiffView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// What every row needs to render, resolved once per frame.
#[derive(Clone)]
struct Paint {
    font_family: SharedString,
    font_size: Pixels,
    row_height: Pixels,
    gutter_width: Pixels,
}

impl RenderOnce for DiffView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let theme = cx.theme();
        let font_size = theme.mono_font_size;
        let digits = (state.max_line.max(1).ilog10() + 1).max(2) as f32;
        let font_id = window
            .text_system()
            .resolve_font(&gpui::font(theme.mono_font_family.clone()));
        let char_width = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6);
        let row_height = Self::row_height(cx);
        let paint = Paint {
            font_family: theme.mono_font_family.clone(),
            font_size,
            row_height,
            // Digits plus `px_2` either side.
            gutter_width: char_width * digits + px(16.),
        };
        let row_count = state.rows.len();
        let widest_row = state.widest_row;
        let scroll_handle = state.scroll_handle.clone();
        let focus_handle = state.focus_handle.clone();
        let entity = self.state.clone();
        let label: SharedString = match state.diff.files() {
            [file] => file
                .path()
                .map(|path| t!("DiffView.label_of", path = path).into())
                .unwrap_or_else(|| t!("DiffView.label").into()),
            _ => t!("DiffView.label").into(),
        };
        let empty = state.diff.files().is_empty();
        let focused = focus_handle.is_focused(window) && window.last_input_was_keyboard();

        v_flex()
            .id(("diff-view", self.state.entity_id()))
            .test_support()
            .key_context(CONTEXT)
            .track_focus(&focus_handle)
            .role(Role::Group)
            .aria_label(label)
            .map(|this| match self.max_rows {
                Some(max_rows) => this
                    .w_full()
                    .flex_none()
                    .h(row_height * row_count.clamp(1, max_rows.max(1)) as f32),
                None => this.size_full(),
            })
            .overflow_hidden()
            .bg(theme.tokens.background)
            .text_color(theme.foreground)
            .font_family(paint.font_family.clone())
            .text_size(paint.font_size)
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectUp, _, cx| {
                    state.scroll_by(point(px(0.), DiffView::row_height(cx)), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectDown, _, cx| {
                    state.scroll_by(point(px(0.), -DiffView::row_height(cx)), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectLeft, _, cx| {
                    state.scroll_by(point(SIDE_STEP, px(0.)), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectRight, _, cx| {
                    state.scroll_by(point(-SIDE_STEP, px(0.)), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectPageUp, _, cx| {
                    let page = state.page(cx);
                    state.scroll_by(point(px(0.), page), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectPageDown, _, cx| {
                    let page = state.page(cx);
                    state.scroll_by(point(px(0.), -page), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectFirst, _, cx| {
                    state.scroll_by(point(px(0.), Pixels::MAX), cx)
                }),
            )
            .on_action(
                window.listener_for(&self.state, |state, _: &SelectLast, _, cx| {
                    state.scroll_by(point(px(0.), -Pixels::MAX), cx)
                }),
            )
            .refine_style(&self.style)
            .when(focused, |this| this.focus_ring_style(window, cx))
            .when(empty, |this| {
                this.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(t!("DiffView.no_changes"))),
                )
            })
            .when(!empty, |this| {
                this.child(
                    uniform_list(
                        ("diff-view-rows", self.state.entity_id()),
                        row_count,
                        move |range, window, cx| {
                            let state = entity.read(cx);
                            range
                                .map(|ix| render_row(&entity, state, ix, &paint, window, cx))
                                .collect::<Vec<_>>()
                        },
                    )
                    .flex_1()
                    .with_width_from_item(Some(widest_row))
                    .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                    .track_scroll(&scroll_handle),
                )
            })
    }
}

/// A muted note after a line's text or a file's path.
fn note(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn render_row(
    entity: &Entity<DiffViewState>,
    state: &DiffViewState,
    ix: usize,
    paint: &Paint,
    _: &mut Window,
    cx: &App,
) -> gpui::AnyElement {
    let theme = cx.theme();
    let row = div()
        .id(("diff-row", ix))
        .test_support()
        .h(paint.row_height)
        .w_full()
        .flex()
        .items_center()
        .whitespace_nowrap();
    match &state.rows[ix] {
        Row::File(file_ix) => {
            let file = &state.diff.files()[*file_ix];
            let path: SharedString = match (file.old_path(), file.new_path()) {
                (Some(old), Some(new)) if file.is_rename() => format!("{old} → {new}").into(),
                _ => file
                    .path()
                    .cloned()
                    .unwrap_or_else(|| t!("DiffView.untitled").into()),
            };
            let counts = !file.is_binary()
                && (file.added_lines() + file.removed_lines() > 0
                    || !(file.is_rename() || file.mode_change().is_some()));
            let diff = state.diff.clone();
            let file_ix = *file_ix;
            row.gap_2()
                .px_3()
                .bg(theme.muted)
                .border_b_1()
                .border_color(theme.border)
                .font_family(theme.font_family.clone())
                .text_sm()
                .child(div().flex_1().min_w_0().truncate().child(path))
                .when_some(file.mode_change(), |this, (old, new)| {
                    this.child(note(
                        t!("DiffView.mode", old = old, new = new).to_string(),
                        cx,
                    ))
                })
                .when(file.is_binary(), |this| {
                    this.child(note(t!("DiffView.binary").to_string(), cx))
                })
                .when(counts, |this| {
                    this.child(
                        div()
                            .text_color(theme.success)
                            .child(format!("+{}", file.added_lines())),
                    )
                    .child(
                        div()
                            .text_color(theme.danger)
                            .child(format!("−{}", file.removed_lines())),
                    )
                })
                .child(
                    // The text is built only when copied.
                    Clipboard::new(("diff-copy", file_ix))
                        .xsmall()
                        .value_fn(move |_, _| diff.file_unified(file_ix).into())
                        .tooltip(SharedString::from(t!("DiffView.copy"))),
                )
                .into_any_element()
        }
        Row::Hunk { file, hunk, hidden } => {
            let header = state.diff.files()[*file].hunks[*hunk].header.clone();
            let (file, gap) = (*file, *hunk);
            row.gap_2()
                .px_3()
                .bg(theme.muted.opacity(0.5))
                .text_color(theme.muted_foreground)
                .when(*hidden > 0, |this| {
                    this.child(unfold_button(entity, file, gap, *hidden))
                })
                .child(header)
                .into_any_element()
        }
        Row::Tail { file, hidden } => {
            let gap = state.diff.files()[*file].hunks.len();
            row.px_3()
                .bg(theme.muted.opacity(0.5))
                .child(unfold_button(entity, *file, gap, *hidden))
                .into_any_element()
        }
        Row::Line { file, line } => {
            let diff_line = &state.diff.files()[*file].lines()[*line];
            let (text, more) = shown_text(diff_line.text());
            let styles = state
                .highlights
                .get(*file)
                .and_then(|highlights| highlights.lines.get(*line))
                .map(|names| resolve_highlights(names, text.len(), &theme.highlight_theme))
                .unwrap_or_default();
            let (sign, sign_color, background) = match diff_line.change() {
                LineChange::Unchanged => (" ", theme.muted_foreground, None),
                LineChange::Added => ("+", theme.success, Some(theme.success.opacity(0.14))),
                LineChange::Removed => ("-", theme.danger, Some(theme.danger.opacity(0.14))),
            };
            // How a changed line ends, when that is part of the change.
            let ending = match (diff_line.change(), diff_line.ending()) {
                (LineChange::Unchanged, _) => None,
                (_, LineEnding::Missing) => Some(t!("DiffView.no_newline").to_string()),
                (_, LineEnding::CrLf) if state.mixed_endings[*file] => Some("CRLF".to_string()),
                (_, LineEnding::Lf) if state.mixed_endings[*file] => Some("LF".to_string()),
                _ => None,
            };
            let number = |number: Option<u32>| {
                div()
                    .flex_none()
                    .w(paint.gutter_width)
                    .px_2()
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .when_some(number, |this, number| this.child(number.to_string()))
            };
            row.when_some(background, |this, background| this.bg(background))
                .child(number(diff_line.old_line()))
                .child(number(diff_line.new_line()))
                .child(
                    div()
                        .flex_none()
                        .w(paint.font_size)
                        .text_color(sign_color)
                        .child(sign),
                )
                .child(
                    div()
                        .pr_3()
                        .when(more.is_some() || ending.is_some(), |this| {
                            this.flex().gap(paint.font_size)
                        })
                        .child(StyledText::new(text).with_highlights(styles))
                        .when_some(more, |this, more| {
                            this.child(note(
                                t!("DiffView.more_characters", count = more).to_string(),
                                cx,
                            ))
                        })
                        .when_some(ending, |this, ending| this.child(note(ending, cx))),
                )
                .into_any_element()
        }
    }
}

/// The part of a line a row shows: up to [`MAX_LINE_CHARS`] characters, and
/// how many more it leaves out.
fn shown_text(text: &SharedString) -> (SharedString, Option<usize>) {
    match text.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => (
            text[..cut].to_string().into(),
            Some(text[cut..].chars().count()),
        ),
        None => (text.clone(), None),
    }
}

fn unfold_button(
    entity: &Entity<DiffViewState>,
    file: usize,
    gap: usize,
    hidden: usize,
) -> impl IntoElement {
    let entity = entity.clone();
    Button::new(("diff-unfold", file * 100_000 + gap))
        .ghost()
        .xsmall()
        .icon(IconName::ChevronsUpDown)
        .label(SharedString::from(t!("DiffView.unfold", count = hidden)))
        .on_click(move |_, _, cx| entity.update(cx, |state, cx| state.unfold(file, gap, cx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_shows_its_start() {
        let long = SharedString::from("é".repeat(MAX_LINE_CHARS + 5));
        let (shown, more) = shown_text(&long);
        assert_eq!(shown.chars().count(), MAX_LINE_CHARS);
        assert_eq!(more, Some(5));
        let short = SharedString::from("fn main() {}");
        assert_eq!(shown_text(&short), (short.clone(), None));
    }

    /// Highlight names resolve against the theme at paint: the same names
    /// give each theme's colors, and a later name covers an earlier one.
    #[test]
    fn highlight_names_take_the_theme_they_paint_in() {
        let names = vec![
            (0..10, SharedString::from("keyword")),
            (2..4, SharedString::from("string")),
        ];
        let mut resolved = Vec::new();
        for theme in [
            HighlightTheme::default_light(),
            HighlightTheme::default_dark(),
        ] {
            let styles = resolve_highlights(&names, 8, &theme);
            let style_at = |byte: usize| {
                styles
                    .iter()
                    .find(|(range, _)| range.contains(&byte))
                    .map_or(HighlightStyle::default(), |(_, style)| *style)
            };
            let keyword = theme.style("keyword").unwrap_or_default();
            let mut string = keyword;
            merge_style(&mut string, &theme.style("string").unwrap_or_default());
            for byte in 0..8 {
                let expected = if (2..4).contains(&byte) {
                    string
                } else {
                    keyword
                };
                assert_eq!(style_at(byte), expected, "byte {byte}");
            }
            assert!(
                styles.iter().all(|(range, _)| range.end <= 8),
                "cut to the line"
            );
            resolved.push(styles);
        }
        assert_ne!(resolved[0], resolved[1], "each theme its colors");
    }
}
