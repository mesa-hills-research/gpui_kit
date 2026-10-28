use super::*;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{
    Selectable as _,
    tab::{Tab, TabBar},
};

struct SegmentedStates;

impl Render for SegmentedStates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx)
            .child(
                TabBar::new("session")
                    .segmented()
                    .selected_index(0)
                    .child(Tab::new().label("Session"))
                    .child(Tab::new().label("All"))
                    .on_click(|index, _, _| assert_ne!(*index, 2))
                    .child(
                        Tab::new()
                            .disabled(true)
                            .child(div().id("unavailable").test_support().child("Unavailable")),
                    ),
            )
            .child(
                TabBar::new("all")
                    .segmented()
                    .xsmall()
                    .selected_index(1)
                    .child(Tab::new().label("Session").disabled(true))
                    .child(Tab::new().label("All")),
            )
            .child(
                TabBar::new("disabled-selected")
                    .segmented()
                    .selected_index(0)
                    .child(Tab::new().label("Disabled selection").disabled(true))
                    .child(Tab::new().label("Enabled")),
            )
            .child(Tab::new().segmented().selected(true).label("Standalone"))
    }
}

/// A segmented bar sized by its labels, with a marker after it.
struct SegmentedWidth(usize);

impl Render for SegmentedWidth {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(
            div()
                .flex()
                .items_center()
                .child(
                    TabBar::new("width")
                        .segmented()
                        .selected_index(self.0)
                        .child(Tab::new().label("Session"))
                        .child(Tab::new().label("All")),
                )
                .child(div().id("after").test_support().size_2()),
        )
    }
}

/// The selected label is medium weight, and every segment keeps that width,
/// so changing the selection never resizes the bar.
#[test]
fn segmented_selection_keeps_the_bar_width() {
    let mut app = app(ThemeMode::Light);
    let mut positions = Vec::new();
    for selected in [0, 1] {
        let window = open(&mut app, (320., 80.), SCALE, move |_, _| {
            SegmentedWidth(selected)
        });
        app.capture(window).unwrap();
        act(&mut app, window, |window, _| {
            positions.push(window.find("after").bounds().origin);
        });
    }
    assert_eq!(positions[0], positions[1]);
}

#[test]
fn segmented_selection_and_disabled_options() {
    for (mode, suffix) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
        let mut app = app(mode);
        app.update(|cx| cx.set_reduce_motion(true));
        let window = open(&mut app, (440., 240.), SCALE, |_, _| SegmentedStates);
        let screenshot = app.capture(window).unwrap();
        goldens().assert(&format!("segmented-tabs-{suffix}"), &screenshot);
        act(&mut app, window, |window, cx| {
            window.hover("unavailable", cx);
            window.click("unavailable", cx);
        });
        assert_eq!(screenshot.image, app.capture(window).unwrap().image);
    }
}
