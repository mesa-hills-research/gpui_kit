//! Shapes nested in a rounded one follow its corners.
use super::*;
use gpui_kit::{
    Axis,
    component::{
        attachment::{
            Attachment, AttachmentContent, AttachmentDescription, AttachmentMedia, AttachmentTitle,
        },
        select::{SearchableVec, Select, SelectState},
    },
};

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

fn chip(size: gpui_kit::component::Size, axis: Axis) -> Attachment {
    Attachment::new()
        .with_size(size)
        .axis(axis)
        .media(AttachmentMedia::new().child(gpui_kit::component::Icon::new(IconName::File)))
        .content(
            AttachmentContent::new()
                .title(AttachmentTitle::new("report.pdf"))
                .description(AttachmentDescription::new("PDF")),
        )
}

struct AttachmentTiles;

impl Render for AttachmentTiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::Size;
        page(cx)
            .child(chip(Size::XSmall, Axis::Horizontal))
            .child(chip(Size::Small, Axis::Horizontal))
            .child(chip(Size::Medium, Axis::Horizontal))
            .child(chip(Size::Large, Axis::Horizontal))
            .child(chip(Size::Small, Axis::Vertical))
    }
}

/// The media tile of a chip takes the radius that keeps it concentric with the card, where the
/// card's padding leaves it inside the card's corner. Roomier sizes keep their own radius.
#[test]
fn attachment_media_follows_the_card_corner() {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (320., 460.), SCALE, |_, _| AttachmentTiles);
    goldens().assert("attachment-media", &app.capture(window).unwrap());
}
