//! Run the ignored timing test with `cargo test -p gpui-component --lib --release
//! settings_typing_and_change_cost -- --ignored --nocapture`.
use super::*;
use crate::{
    Root,
    setting::{SettingField, SettingGroup, SettingItem},
};
use gpui::{Context, Focusable as _, Render, TestAppContext, VisualTestContext, size};
use std::time::Instant;

struct PerformanceHost {
    input: Entity<InputState>,
    revision: u64,
    cached: bool,
}

impl Render for PerformanceHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Settings::new("performance-settings").page(
            SettingPage::new("Preferences")
                .resettable(false)
                .groups((0..8).map(|group_ix| {
                    let input = self.input.clone();
                    let value = group_ix != 0 || self.revision.is_multiple_of(2);
                    let group = if self.cached {
                        SettingGroup::new().key(if group_ix == 0 { self.revision } else { 0 })
                    } else {
                        SettingGroup::new()
                    };
                    group
                        .title(format!("Group {}", group_ix + 1))
                        .items((0..5).map(move |item_ix| {
                            if group_ix == 0 && item_ix == 0 {
                                let input = input.clone();
                                SettingItem::new(
                                    "Instructions",
                                    SettingField::render(move |_, _, _| Input::new(&input).w_64()),
                                )
                            } else {
                                SettingItem::new(
                                    format!("Preference {}", group_ix * 5 + item_ix + 1),
                                    SettingField::switch(move |_| value, |_, _| {}),
                                )
                                .description(
                                    "Choose whether this preference applies to new documents.",
                                )
                            }
                        }))
                })),
        )
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
#[ignore]
fn settings_typing_and_change_cost_cached(cx: &mut TestAppContext) {
    measure(cx, true);
}

#[gpui::test]
#[ignore]
fn settings_typing_and_change_cost_uncached(cx: &mut TestAppContext) {
    measure(cx, false);
}

fn measure(cx: &mut TestAppContext, cached: bool) {
    cx.update(|cx| {
        crate::init(cx);
        crate::Theme::global_mut(cx).font_size = px(16.);
    });
    let mut host = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| PerformanceHost {
            input: cx.new(|cx| InputState::new(window, cx)),
            revision: 0,
            cached,
        });
        host = Some(view.clone());
        Root::new(view, window, cx)
    });
    let host = host.unwrap();
    cx.simulate_resize(size(px(1300.), px(2400.)));
    draw(cx);
    cx.update(|window, cx| host.read(cx).input.focus_handle(cx).focus(window, cx));
    draw(cx);
    // Time a field laid out at the width an application would give it.
    let field = cx.update(|_, cx| host.read(cx).input.read(cx).input_bounds());
    assert!(field.size.width >= px(200.), "{field:?}");
    for _ in 0..10 {
        cx.simulate_input("a");
        draw(cx);
    }
    for _ in 0..3 {
        let start = Instant::now();
        for _ in 0..60 {
            cx.simulate_input("a");
            draw(cx);
        }
        let typing = start.elapsed().as_secs_f64() * 1000. / 60.;
        let start = Instant::now();
        for _ in 0..60 {
            host.update(cx, |host, cx| {
                host.revision += 1;
                cx.notify();
            });
            draw(cx);
        }
        let change = start.elapsed().as_secs_f64() * 1000. / 60.;
        eprintln!(
            "Settings 40 items, 1300×2400, cached={cached}, typing {typing:.3} ms, change {change:.3} ms"
        );
    }
}
