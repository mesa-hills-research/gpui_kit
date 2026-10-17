use super::*;
use crate::{
    Root,
    setting::{SettingField, SettingGroup, SettingItem},
};
use gpui::{Bounds, Context, Render, TestAppContext, VisualTestContext, size};

/// Fits a wide row beside its title, but not a row near the stacking threshold.
const LONG_CHOICE: &str = "Enter sends, Shift-Enter starts a new line";
/// Wider than every row the tests lay out.
const ENDLESS_CHOICE: &str = "Use the model chosen for the main agent, or the provider's default \
    model when the main agent has none, and otherwise the first model in the list";
const ITEMS: usize = 4;

struct RowsHost;

impl Render for RowsHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let choice = |label: &'static str| {
            SettingField::dropdown(
                vec![
                    (label.into(), label.into()),
                    ("short".into(), "Short".into()),
                ],
                move |_| label.into(),
                |_, _| {},
            )
        };
        Settings::new("row-layout").page(
            SettingPage::new("General").resettable(false).group(
                SettingGroup::new()
                    .item(
                        SettingItem::new("Send key", choice(LONG_CHOICE))
                            .description("Choose the key that sends a message from the composer."),
                    )
                    .item(
                        SettingItem::new("Model", choice(ENDLESS_CHOICE))
                            .description("Choose the model for new conversations."),
                    )
                    .item(
                        SettingItem::new("Name", SettingField::input(|_| "Ada".into(), |_, _| {}))
                            .description("Shown to the people you share conversations with."),
                    )
                    .item(SettingItem::new(
                        "Spell check",
                        SettingField::switch(|_| true, |_, _| {}),
                    )),
            ),
        )
    }
}

fn setup(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| {
        crate::init(cx);
        crate::Theme::global_mut(cx).font_size = px(16.);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| RowsHost);
        Root::new(view, window, cx)
    });
    cx
}

fn layout(width: f32, cx: &mut VisualTestContext) {
    cx.simulate_resize(size(px(width), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn bounds(part: &str, item_ix: usize, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    let selector = format!("setting-{part}-0-0-{item_ix}");
    cx.debug_bounds(selector.leak())
}

fn row_parts(item_ix: usize, cx: &mut VisualTestContext) -> [Bounds<Pixels>; 3] {
    ["row", "title", "field"].map(|part| bounds(part, item_ix, cx).unwrap())
}

/// Whether the field shares a line with its title.
fn is_beside(title: Bounds<Pixels>, field: Bounds<Pixels>) -> bool {
    field.top() < title.bottom()
}

#[gpui::test]
fn fields_stay_within_their_rows_at_every_width(cx: &mut TestAppContext) {
    let cx = setup(cx);
    let tolerance = px(0.5);
    for width in (480..=1300).step_by(10) {
        layout(width as f32, cx);
        for item_ix in 0..ITEMS {
            let [row, title, field] = row_parts(item_ix, cx);
            let context = format!("{width} px, item {item_ix}: row {row:?}, field {field:?}");
            assert!(field.left() >= row.left() - tolerance, "{context}");
            assert!(field.right() <= row.right() + tolerance, "{context}");
            assert!(title.right() <= row.right() + tolerance, "{context}");
            if is_beside(title, field) {
                assert!(title.right() <= field.left() + tolerance, "{context}");
                // Two fifths of the row stay with the title.
                assert!(
                    title.size.width >= row.size.width * 0.4 - tolerance,
                    "{context}, title {title:?}"
                );
            }
            if let Some(button) = bounds("dropdown", item_ix, cx) {
                assert!(button.right() <= row.right() + tolerance, "{context}");
                assert!(button.right() <= field.right() + tolerance, "{context}");
            }
        }
    }
}

#[gpui::test]
fn a_field_that_does_not_fit_beside_its_title_moves_below_it(cx: &mut TestAppContext) {
    let cx = setup(cx);
    // The narrowest width whose rows are horizontal, just over the stacking threshold.
    let width = (500..=1000)
        .step_by(2)
        .find(|&width| {
            layout(width as f32, cx);
            let [_, title, field] = row_parts(ITEMS - 1, cx);
            is_beside(title, field)
        })
        .unwrap();
    let [row, title, field] = row_parts(0, cx);
    assert!(
        row.size.width < STACKED_LAYOUT_MAX_WIDTH,
        "{width} px: {row:?}"
    );
    assert!(field.top() >= title.bottom(), "{title:?} {field:?}");
    assert_eq!(field.left(), row.left());
    // The switch still sits at the end of its row.
    let [row, _, field] = row_parts(ITEMS - 1, cx);
    assert!((field.right() - row.right()).abs() <= px(0.5), "{field:?}");
    // A label wider than the row truncates inside it.
    let [row, _, _] = row_parts(1, cx);
    let button = bounds("dropdown", 1, cx).unwrap();
    assert!(
        (button.size.width - row.size.width).abs() <= px(0.5),
        "{button:?}"
    );

    layout(1300., cx);
    let [row, title, field] = row_parts(0, cx);
    assert!(is_beside(title, field), "{title:?} {field:?}");
    assert!((field.right() - row.right()).abs() <= px(0.5));
    let [_, title, field] = row_parts(1, cx);
    assert!(!is_beside(title, field), "{title:?} {field:?}");
}
