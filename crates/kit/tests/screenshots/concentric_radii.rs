//! Shapes nested in a rounded one follow its corners.
use super::*;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};

struct SelectPopup {
    language: Entity<SelectState<SearchableVec<&'static str>>>,
}

impl Render for SelectPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(Select::new(&self.language).id("language").w(px(220.)))
    }
}

/// The rows of an open Select sit inside its list padding, so the highlight follows the popup's
/// corners.
#[test]
fn select_rows_follow_the_popup_corner() {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (260., 190.), SCALE, |window, cx| SelectPopup {
        language: cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec!["Rust", "Go", "Zig"]),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        }),
    });
    act(&mut app, window, |window, cx| {
        window.within("language").click("input", cx);
    });
    app.run_until_parked();
    app.capture(window).unwrap();
    goldens().assert("select-popup", &app.capture(window).unwrap());
}
