//! A diff of one or more files: line numbers, added and removed lines,
//! hunk headers, syntax colors and unchanged lines that unfold.

mod diff;

pub use diff::{Diff, DiffFile, DiffLine, LineChange};

use std::{collections::HashSet, ops::Range, sync::Arc};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, HighlightStyle, InteractiveElement as _,
    IntoElement, ListHorizontalSizingBehavior, ParentElement as _, Pixels, Render, RenderOnce,
    Role, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, StyledText, Task,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use rust_i18n::t;

use crate::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    highlighter::{HighlightTheme, SyntaxHighlighter},
    v_flex,
};

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

/// The syntax colors of one file's lines, by line index; computed off the
/// UI thread for one highlight theme.
#[derive(Default)]
struct FileHighlights {
    lines: Vec<Vec<(Range<usize>, HighlightStyle)>>,
}

/// A diff shown by a [`DiffView`]: the diff, which unchanged runs the user
/// unfolded, the syntax colors and the scroll position.
pub struct DiffViewState {
    diff: Arc<Diff>,
    /// Unfolded gaps, as `(file, gap)`; gap `n` is the run before hunk `n`.
    unfolded: HashSet<(usize, usize)>,
    rows: Vec<Row>,
    /// The row with the longest text, measured for the horizontal scroll.
    widest_row: usize,
    scroll_handle: UniformListScrollHandle,
    focus_handle: FocusHandle,
    highlights: Vec<FileHighlights>,
    /// The highlight theme the colors were computed for.
    highlight_theme: Option<Arc<HighlightTheme>>,
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
            scroll_handle: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            highlights: Vec::new(),
            highlight_theme: None,
            _highlight_task: None,
        };
        state.rebuild_rows();
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
        self.highlights.clear();
        self.highlight_theme = None;
        self._highlight_task = None;
        self.rebuild_rows();
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

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (file_ix, file) in self.diff.files().iter().enumerate() {
            rows.push(Row::File(file_ix));
            for (hunk_ix, hunk) in file.hunks.iter().enumerate() {
                let gap = file.gap_before(hunk_ix);
                let unfolded = self.unfolded.contains(&(file_ix, hunk_ix));
                if unfolded {
                    rows.extend(gap.clone().map(|line| Row::Line {
                        file: file_ix,
                        line,
                    }));
                }
                rows.push(Row::Hunk {
                    file: file_ix,
                    hunk: hunk_ix,
                    hidden: if unfolded { 0 } else { gap.len() },
                });
                rows.extend(hunk.lines.clone().map(|line| Row::Line {
                    file: file_ix,
                    line,
                }));
            }
            let tail = file.gap_before(file.hunks.len());
            if !tail.is_empty() {
                if self.unfolded.contains(&(file_ix, file.hunks.len())) {
                    rows.extend(tail.map(|line| Row::Line {
                        file: file_ix,
                        line,
                    }));
                } else {
                    rows.push(Row::Tail {
                        file: file_ix,
                        hidden: tail.len(),
                    });
                }
            }
        }
        self.widest_row = rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| match row {
                Row::Line { file, line } => self.diff.files()[*file].lines()[*line].text().len(),
                Row::Hunk { file, hunk, .. } => self.diff.files()[*file].hunks[*hunk].header.len(),
                _ => 0,
            })
            .map_or(0, |(ix, _)| ix);
        self.rows = rows;
    }

    /// Start computing syntax colors for `theme` on a background thread,
    /// unless they are already computed or on their way.
    fn highlight(&mut self, theme: &Arc<HighlightTheme>, cx: &mut Context<Self>) {
        if self
            .highlight_theme
            .as_ref()
            .is_some_and(|used| Arc::ptr_eq(used, theme))
        {
            return;
        }
        self.highlight_theme = Some(theme.clone());
        let diff = self.diff.clone();
        let theme = theme.clone();
        let task = cx.background_executor().spawn(async move {
            diff.files()
                .iter()
                .map(|file| highlight_file(file, &theme))
                .collect::<Vec<_>>()
        });
        self._highlight_task = Some(cx.spawn(async move |this, cx| {
            let highlights = task.await;
            _ = this.update(cx, |this, cx| {
                this.highlights = highlights;
                cx.notify();
            });
        }));
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

/// Syntax colors for every line of `file`. Each side of the diff is
/// highlighted as one text, the unchanged and removed lines as the old
/// version and the unchanged and added lines as the new, so a string or
/// comment spanning lines colors right within a hunk.
fn highlight_file(file: &DiffFile, theme: &HighlightTheme) -> FileHighlights {
    let language = file
        .path()
        .map(|path| language_of(path))
        .unwrap_or_else(|| "text".into());
    let mut lines = vec![Vec::new(); file.lines().len()];
    for side in [LineChange::Removed, LineChange::Added] {
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
        let styles = highlighter.styles(&(0..text.len()), theme);

        let mut style_ix = 0;
        for (position, &ix) in indices.iter().enumerate() {
            // Unchanged lines take the new side's colors.
            if side == LineChange::Removed && file.lines()[ix].change() == LineChange::Unchanged {
                continue;
            }
            let start = starts[position];
            let end = start + file.lines()[ix].text().len();
            while style_ix < styles.len() && styles[style_ix].0.end <= start {
                style_ix += 1;
            }
            let mut line_styles = Vec::new();
            let mut cursor = style_ix;
            while cursor < styles.len() && styles[cursor].0.start < end {
                let (range, style) = &styles[cursor];
                let range = range.start.max(start) - start..range.end.min(end) - start;
                if !range.is_empty() && *style != HighlightStyle::default() {
                    line_styles.push((range, *style));
                }
                cursor += 1;
            }
            lines[ix] = line_styles;
        }
    }
    FileHighlights { lines }
}

/// A virtualized view of a [`Diff`]: each file's path with its counts and a
/// Copy button, hunk headers, and lines with their old and new numbers,
/// added lines on the success color and removed lines on the danger color
/// (each also marked `+` or `-`, so the change never rests on color alone),
/// in the code font with syntax colors by file extension. Unchanged lines
/// between hunks fold into a row that unfolds them.
///
/// Give it a height (it scrolls inside) or let it fill its parent.
#[derive(IntoElement)]
pub struct DiffView {
    state: Entity<DiffViewState>,
    style: StyleRefinement,
}

impl DiffView {
    /// The view of `state`.
    pub fn new(state: &Entity<DiffViewState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
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
        let theme = cx.theme();
        let highlight_theme = theme.highlight_theme.clone();
        self.state
            .update(cx, |state, cx| state.highlight(&highlight_theme, cx));

        let state = self.state.read(cx);
        let theme = cx.theme();
        let font_size = theme.mono_font_size;
        let max_line = state
            .diff
            .files()
            .iter()
            .flat_map(|file| file.lines().iter())
            .flat_map(|line| [line.old_line(), line.new_line()])
            .flatten()
            .max()
            .unwrap_or(1);
        let digits = (max_line.max(1).ilog10() + 1).max(2) as f32;
        let font_id = window
            .text_system()
            .resolve_font(&gpui::font(theme.mono_font_family.clone()));
        let char_width = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6);
        let paint = Paint {
            font_family: theme.mono_font_family.clone(),
            font_size,
            row_height: (font_size * 1.6).round(),
            // Digits plus `px_2` either side.
            gutter_width: char_width * digits + px(16.),
        };
        let row_count = state.rows.len();
        let widest_row = state.widest_row;
        let scroll_handle = state.scroll_handle.clone();
        let entity = self.state.clone();
        let label: SharedString = match state.diff.files() {
            [file] => file
                .path()
                .map(|path| t!("DiffView.label_of", path = path).into())
                .unwrap_or_else(|| t!("DiffView.label").into()),
            _ => t!("DiffView.label").into(),
        };
        let empty = state.diff.files().is_empty();

        v_flex()
            .id(("diff-view", self.state.entity_id()))
            .role(Role::Group)
            .aria_label(label)
            .size_full()
            .overflow_hidden()
            .bg(theme.tokens.background)
            .text_color(theme.foreground)
            .font_family(paint.font_family.clone())
            .text_size(paint.font_size)
            .refine_style(&self.style)
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
        .h(paint.row_height)
        .w_full()
        .flex()
        .items_center()
        .whitespace_nowrap();
    match &state.rows[ix] {
        Row::File(file_ix) => {
            let file = &state.diff.files()[*file_ix];
            let path: SharedString = file
                .path()
                .cloned()
                .unwrap_or_else(|| t!("DiffView.untitled").into());
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
                .child(
                    div()
                        .text_color(theme.success)
                        .child(format!("+{}", file.added_lines())),
                )
                .child(
                    div()
                        .text_color(theme.danger)
                        .child(format!("−{}", file.removed_lines())),
                )
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
            let styles = state
                .highlights
                .get(*file)
                .and_then(|highlights| highlights.lines.get(*line))
                .cloned()
                .unwrap_or_default();
            let (sign, sign_color, background) = match diff_line.change() {
                LineChange::Unchanged => (" ", theme.muted_foreground, None),
                LineChange::Added => ("+", theme.success, Some(theme.success.opacity(0.14))),
                LineChange::Removed => ("-", theme.danger, Some(theme.danger.opacity(0.14))),
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
                        .child(StyledText::new(diff_line.text().clone()).with_highlights(styles)),
                )
                .into_any_element()
        }
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
