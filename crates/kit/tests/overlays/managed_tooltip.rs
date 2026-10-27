use super::common;
use gpui_kit::component::{
    menu::{ContextMenuExt as _, PopupMenuItem},
    tooltip::{ManagedTooltipExt as _, Tooltip},
};
use gpui_kit::test::{TestSupportExt as _, TestWindowExt as _};
use gpui_kit::{
    AppContext as _, Context, Modifiers, Render, TestAppContext, VisualTestContext, Window, div,
    point, prelude::*, px, size,
};
use std::time::Duration;

struct Heading;

impl Render for Heading {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(
            div()
                .id("heading")
                .test_support()
                .w_40()
                .h_10()
                .child("Project")
                .managed_tooltip(|window, cx| Tooltip::new("Project folder").build(window, cx))
                .context_menu(|menu, _, _| menu.item(PopupMenuItem::new("Open folder"))),
        )
    }
}

#[gpui_kit::test]
fn a_custom_heading_uses_managed_tooltips_and_open_menu_suppression(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let (window, _) = common::open_window(cx, Some(size(px(400.), px(300.))), |_, cx| {
        cx.new(|_| Heading)
    });
    let rest = |cx: &mut TestAppContext| {
        for _ in 0..10 {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
                .unwrap();
        }
    };
    let move_to = |x, y, cx: &mut TestAppContext| {
        VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
            point(px(x), px(y)),
            None,
            Modifiers::default(),
        );
        rest(cx);
    };
    let tooltip = |cx: &mut TestAppContext| {
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("tooltip")
            .is_some()
    };
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    move_to(20., 20., cx);
    assert!(tooltip(cx));
    cx.update_window(window.into(), |_, window, cx| {
        window.right_click("heading", cx)
    })
    .unwrap();
    move_to(390., 290., cx);
    move_to(20., 20., cx);
    cx.update_window(window.into(), |_, window, _| {
        assert!(window.try_find("popup-menu").is_some());
    })
    .unwrap();
    assert!(!tooltip(cx));
    cx.update_window(window.into(), |_, window, cx| window.press("escape", cx))
        .unwrap();
    move_to(390., 290., cx);
    move_to(20., 20., cx);
    assert!(tooltip(cx));
}
