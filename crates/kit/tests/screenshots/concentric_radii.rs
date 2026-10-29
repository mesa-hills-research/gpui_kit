//! Shapes nested in a rounded one follow its corners.
use super::*;
use gpui_kit::{
    Axis,
    component::{
        WindowExt as _,
        accordion::Accordion,
        attachment::{
            Attachment, AttachmentContent, AttachmentDescription, AttachmentMedia, AttachmentTitle,
        },
        description_list::{DescriptionItem, DescriptionList},
        notification::Notification,
        select::{SearchableVec, Select, SelectState},
        table::{Column, DataTable, TableDelegate, TableState},
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

struct NotificationHost;

impl Render for NotificationHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx)
    }
}

/// The close button's hover fill follows the card's corner.
#[test]
fn notification_close_button_follows_the_card_corner() {
    let mut app = app(ThemeMode::Light);
    app.update(|cx| cx.set_reduce_motion(true));
    let window = open(&mut app, (480., 130.), SCALE, |_, _| NotificationHost);
    act(&mut app, window, |window, cx| {
        window.push_notification(
            Notification::new()
                .title("Saved")
                .message("The file was written.")
                .autohide(false),
            cx,
        );
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(1));
    app.capture(window).unwrap();
    act(&mut app, window, |window, cx| {
        let close = window.try_find("close").expect("the close button");
        let position = close.bounds().center();
        window.dispatch_event(
            gpui_kit::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
    });
    app.run_until_parked();
    goldens().assert("notification-close", &app.capture(window).unwrap());
}

struct Rows;

impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        2
    }
    fn rows_count(&self, _: &App) -> usize {
        2
    }
    fn column(&self, ix: usize, _: &App) -> Column {
        Column::new(
            format!("column-{ix}"),
            if ix == 0 { "Name" } else { "Status" },
        )
        .width(px(140.))
    }
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().child(format!("{row}:{col}"))
    }
}

struct Fills {
    table: Entity<TableState<Rows>>,
}

impl Render for Fills {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = |layout: Axis| {
            DescriptionList::new()
                .bordered(true)
                .layout(layout)
                .columns(1)
                .children([
                    DescriptionItem::new("Name").value("Notes"),
                    DescriptionItem::new("Status").value("Saved"),
                ])
        };
        // The page color would hide a fill that pokes out, so the items take another.
        let muted = cx.theme().muted;
        page(cx)
            .child(
                div()
                    .h(px(76.))
                    .flex_none()
                    .child(DataTable::new(&self.table)),
            )
            .child(div().flex_none().child(list(Axis::Horizontal)))
            .child(div().flex_none().child(list(Axis::Vertical)))
            .child(
                div().h(px(84.)).flex_none().child(
                    Accordion::new("fills")
                        .bordered(true)
                        .item(|item| item.title("First").bg(muted))
                        .item(|item| item.title("Second").bg(muted)),
                ),
            )
    }
}

/// A fill at the corner of a bordered frame follows the frame's arc, with no square corner past
/// it.
#[test]
fn fills_at_a_frames_corners_follow_its_arc() {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (320., 420.), SCALE, |window, cx| Fills {
        table: cx.new(|cx| TableState::new(Rows, window, cx)),
    });
    goldens().assert("rounded-fills", &app.capture(window).unwrap());
}
