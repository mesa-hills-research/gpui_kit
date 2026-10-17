mod common;
use gpui::{AppContext, Context, Entity, TestAppContext, Window, div, prelude::*, px, size};
use gpui_component::{
    Disableable,
    button::Button,
    clipboard::Clipboard,
    input::{Input, InputState},
    popover::Popover,
};
use gpui_kit::test::{TestSupportExt, TestWindowExt};

struct Controls {
    input: Entity<InputState>,
    clicks: usize,
}
impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                Button::new("disabled")
                    .label("Disabled")
                    .disabled(true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
            )
            .child(Input::new(&self.input).id("search").w(px(240.)))
            .child(
                Popover::new("popover-host")
                    .trigger(Button::new("open").label("Open"))
                    .content(|_, _, _| {
                        div()
                            .id("popover-content")
                            .test_support()
                            .w(px(120.))
                            .h(px(50.))
                            .child("Details")
                    }),
            )
    }
}

#[gpui::test]
fn kit_controls_use_native_events_and_report_state(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (handle, handle_content) =
        common::open_window(cx, Some(size(px(600.), px(500.))), |window, cx| {
            cx.new(|cx| Controls {
                input: cx.new(|cx| InputState::new(window, cx)),
                clicks: 0,
            })
        });
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(window.find("disabled").disabled(), None);
        window.click("disabled", cx);
        window.click("search", cx);
        assert_eq!(window.find("search").focused(), Some(true));
        window.input("GPUI 中文 🦀", cx);
        assert_eq!(window.find("search").value(), Some("GPUI 中文 🦀"));
        assert!(window.try_find("popover-content").is_none());
        window.click("open", cx);
        let content = window.find("popover-content");
        assert!(content.visible());
        assert!(content.bounds().top() >= window.find("open").bounds().bottom());
    })
    .unwrap();
    common::update_content(handle, &handle_content, cx, |view, _, _| {
        assert_eq!(view.clicks, 0)
    })
    .unwrap();
}

struct NamedButton;
impl Render for NamedButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Button::new("save")
            .label("Save")
            .accessibility_label("Save this document")
    }
}

#[gpui::test]
fn button_reports_accessibility_name_without_claiming_visible_text(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (handle, _) = common::open_window(cx, None, |_, cx| cx.new(|_| NamedButton));
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(window.find("save").label(), Some("Save this document"));
    })
    .unwrap();
}

struct NamedClipboard;
impl Render for NamedClipboard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(
                Clipboard::new("copy-key")
                    .value("sk-1234")
                    .tooltip("Copy")
                    .accessibility_label("Copy API key"),
            )
            .child(
                Clipboard::new("copy-plain")
                    .value("sk-1234")
                    .tooltip("Copy"),
            )
    }
}

#[gpui::test]
fn clipboard_reports_default_and_explicit_accessibility_names(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (handle, _) = common::open_window(cx, None, |_, cx| cx.new(|_| NamedClipboard));
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(window.find("copy-key").label(), Some("Copy API key"));
        assert_eq!(
            window.find("copy-plain").label(),
            Some("Copy"),
            "an icon-only Clipboard needs a name without a caller-provided label"
        );
    })
    .unwrap();
}

struct ScrollFocus {
    focus: gpui::FocusHandle,
}
impl Render for ScrollFocus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::scroll::ScrollableElement as _;
        div()
            .id("original")
            .test_support()
            .size(px(100.))
            .overflow_y_scrollbar()
            .id("scroll")
            .track_focus(&self.focus)
    }
}
#[gpui_kit::test]
fn scrollable_elements_forward_observed_focus_binding(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (handle, handle_content) = common::open_window(cx, None, |_, cx| {
        cx.new(|cx| ScrollFocus {
            focus: cx.focus_handle(),
        })
    });
    let focus =
        common::update_content(handle, &handle_content, cx, |view, _, _| view.focus.clone())
            .unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        let content = (gpui::ElementId::from("scroll"), "content");
        window.render_frame(cx);
        assert_eq!(
            window.within("scroll").find(content.clone()).focused(),
            Some(false)
        );
        window.focus(&focus, cx);
        window.render_frame(cx);
        assert_eq!(
            window.within("scroll").find(content.clone()).focused(),
            Some(true)
        );
        window.blur(cx);
        window.render_frame(cx);
        assert_eq!(window.within("scroll").find(content).focused(), Some(false));
    })
    .unwrap();
}

struct Changes {
    diff: Entity<gpui_component::diff_view::DiffViewState>,
}
impl Render for Changes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(gpui_component::diff_view::DiffView::new(&self.diff).h(px(400.)))
    }
}

/// A computed diff folds the unchanged lines between its hunks behind a row
/// that unfolds them, and each file's Copy button copies its unified diff.
#[gpui::test]
fn diff_view_unfolds_unchanged_lines_and_copies_the_diff(cx: &mut TestAppContext) {
    use gpui_component::diff_view::{Diff, DiffViewState};
    cx.update(gpui_component::init);
    let old = (1..=30).map(|n| format!("line {n}\n")).collect::<String>();
    let new = old
        .replace("line 3\n", "line three\n")
        .replace("line 25\n", "line twenty-five\n");
    let (handle, view) = common::open_window(cx, Some(size(px(600.), px(500.))), |_, cx| {
        let diff = cx.new(|cx| DiffViewState::new(Diff::between("notes.txt", &old, &new), cx));
        cx.new(|_| Changes { diff })
    });
    let diff = view.read_with(cx, |view, _| view.diff.clone());
    let rows = |cx: &mut TestAppContext| diff.read_with(cx, |state, _| state.row_count());
    // The file row, the first hunk (7 lines, from line 1, so nothing folds
    // before it), the second hunk (8 lines) after its folded gap, and the
    // folded last two lines.
    assert_eq!(rows(cx), 1 + (1 + 7) + (1 + 8) + 1);

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        // The 15 unchanged lines between the hunks unfold in place.
        window.click(("diff-unfold", 1usize), cx);
    })
    .unwrap();
    assert_eq!(rows(cx), 1 + (1 + 7) + (1 + 8) + 1 + 15);

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("diff-copy", 0usize), cx);
    })
    .unwrap();
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert!(
        copied.as_deref().is_some_and(|text| text
            .starts_with("--- a/notes.txt\n+++ b/notes.txt\n@@ -1,6 +1,6 @@\n")
            && text.contains("-line 25\n+line twenty-five\n")),
        "{copied:?}"
    );
}

struct SizedChanges {
    diff: Entity<gpui_component::diff_view::DiffViewState>,
    max_rows: usize,
}
impl Render for SizedChanges {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(gpui_component::diff_view::DiffView::new(&self.diff).max_rows(self.max_rows))
    }
}

/// With `max_rows`, a diff view takes the height of its rows, up to that
/// many, as a diff inside a list item does. Focused, the arrow keys, Page
/// Up and Page Down, Home and End scroll it.
#[gpui::test]
fn a_diff_view_sizes_to_its_rows_and_scrolls_by_keyboard(cx: &mut TestAppContext) {
    use gpui_component::diff_view::{Diff, DiffView, DiffViewState};
    cx.update(gpui_component::init);
    let small = Diff::between("a.txt", "a\nb\nc\n", "a\nB\nc\n");
    let old = (1..=60).map(|n| format!("line {n}\n")).collect::<String>();
    let new = old.replace("line 1\n", "line one\n");
    let large = Diff::between("b.txt", &old, &new);
    let (handle, view) = common::open_window(cx, Some(size(px(600.), px(800.))), |_, cx| {
        let diff = cx.new(|cx| DiffViewState::new(small, cx));
        cx.new(|_| SizedChanges { diff, max_rows: 10 })
    });
    let diff = view.read_with(cx, |view, _| view.diff.clone());
    let id = ("diff-view", diff.entity_id());
    let row_height = cx.update(|cx| DiffView::row_height(cx));
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        // The file row, the hunk header, and its four lines.
        assert_eq!(window.find(id).bounds().size.height, row_height * 6.);
    })
    .unwrap();

    diff.update(cx, |diff, cx| {
        diff.set_diff(large, cx);
        diff.unfold_all(cx);
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(id).bounds().size.height, row_height * 10.);
        window.focus_next(cx);
        window.render_frame(cx);
        assert_eq!(window.find(id).focused(), Some(true));
        let top = window.find(("diff-row", 0usize)).bounds().origin.y;

        window.press("down", cx);
        window.press("down", cx);
        let scrolled = window.find(("diff-row", 2usize)).bounds().origin.y;
        assert_eq!(scrolled, top, "two rows down");
        window.press("up", cx);
        assert_eq!(window.find(("diff-row", 1usize)).bounds().origin.y, top);
        window.press("end", cx);
        assert!(
            window.try_find(("diff-row", 0usize)).is_none(),
            "scrolled to the end"
        );
        window.press("home", cx);
        assert_eq!(window.find(("diff-row", 0usize)).bounds().origin.y, top);
        window.press("pagedown", cx);
        assert!(
            window.try_find(("diff-row", 0usize)).is_none(),
            "a page down"
        );
    })
    .unwrap();
}

/// A run of one unchanged line between hunks shows that line, where a
/// button to show it would take the same row.
#[gpui::test]
fn a_one_line_gap_shows_its_line(cx: &mut TestAppContext) {
    use gpui_component::diff_view::{Diff, DiffViewState};
    cx.update(gpui_component::init);
    let old = (1..=9).map(|n| format!("line {n}\n")).collect::<String>();
    // Changes at lines 1 and 9: their context leaves line 5 between them.
    let new = old
        .replace("line 1\n", "line one\n")
        .replace("line 9\n", "line nine\n");
    let diff = cx.new(|cx| DiffViewState::new(Diff::between("f.txt", &old, &new), cx));
    let file = diff.read_with(cx, |diff, _| diff.diff().files()[0].clone());
    assert_eq!(file.lines().len(), 11);
    // The file row, the hunks' 1 + 5 and 1 + 5 rows, and line 5 between.
    assert_eq!(
        diff.read_with(cx, |diff, _| diff.row_count()),
        1 + 6 + 1 + 6
    );
}
