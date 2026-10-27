use super::*;
use gpui_kit::{
    Overflow, StyleRefinement,
    component::text::{TextView, TextViewStyle},
};

struct TableSpacing;

impl Render for TableSpacing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).children([false, true].map(|scroll| {
            let mut table = StyleRefinement::default();
            if scroll {
                table.overflow.x = Some(Overflow::Scroll);
            }
            let style = TextViewStyle::default().table(table);
            let frame = || div().w_full().bg(cx.theme().muted);
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(if scroll { "Scroll" } else { "Wrap" })
                .child(
                    frame().child(
                        TextView::markdown(
                            if scroll { "scroll-table" } else { "wrap-table" },
                            "| Name | Status |\n|:--|:--|\n| Notes | Saved |",
                        )
                        .style(style.clone()),
                    ),
                )
                .child(
                    frame().child(
                        TextView::markdown(
                            if scroll {
                                "scroll-paragraph"
                            } else {
                                "wrap-paragraph"
                            },
                            "A final paragraph.",
                        )
                        .style(style),
                    ),
                )
        }))
    }
}

#[test]
fn final_tables_and_paragraphs_end_without_an_outside_gap() {
    for (mode, name) in [
        (ThemeMode::Light, "table-spacing"),
        (ThemeMode::Dark, "table-spacing-dark"),
    ] {
        let mut app = app(mode);
        let window = open(&mut app, (360., 340.), SCALE, |_, _| TableSpacing);
        goldens().assert(name, &app.capture(window).unwrap());
    }
}
