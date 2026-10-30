use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AnyElement, Bounds, Context, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, Window, div, point, px, size,
};
use gpui_base::ElementExt as _;

struct PrepaintHarness {
    captured: Rc<RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

impl Render for PrepaintHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let captured = self.captured.clone();
        div()
            .size(px(80.))
            .on_prepaint(move |bounds, _, _| *captured.borrow_mut() = Some(bounds))
    }
}

#[gpui::test]
fn prepaint_callback_observes_the_parent_bounds(cx: &mut TestAppContext) {
    let captured = Rc::new(RefCell::new(None));
    let result = captured.clone();
    let (_, cx) = cx.add_window_view(move |_, _| PrepaintHarness { captured });

    cx.update(|window, cx| window.draw(cx).clear(cx));

    let bounds = result.borrow().expect("prepaint callback should run");
    assert_eq!(bounds.size.width, px(80.));
    assert_eq!(bounds.size.height, px(80.));
}

/// The bounds `on_prepaint` reports for `element`, drawn at the window's top
/// left corner.
fn reported_bounds(
    cx: &mut TestAppContext,
    element: impl FnOnce(Rc<Cell<Bounds<Pixels>>>) -> AnyElement,
) -> Bounds<Pixels> {
    let reported = Rc::new(Cell::new(Bounds::default()));
    let cx = cx.add_empty_window();
    cx.draw(point(px(0.), px(0.)), size(px(200.), px(100.)), |_, _| {
        element(reported.clone())
    });
    reported.get()
}

/// A tooltip trigger added after an icon, as `div().child(icon).managed_tooltip(..)`
/// adds it, reports the element and not the line below it.
#[gpui::test]
fn prepaint_callback_after_a_child_of_a_block_element_observes_the_element(
    cx: &mut TestAppContext,
) {
    let bounds = reported_bounds(cx, |reported| {
        div()
            .w(px(80.))
            .child(div().h(px(12.)))
            .on_prepaint(move |bounds, _, _| reported.set(bounds))
            .into_any_element()
    });
    assert_eq!(
        bounds,
        Bounds::new(point(px(0.), px(0.)), size(px(80.), px(12.)))
    );
}

#[gpui::test]
fn prepaint_callback_of_a_padded_flex_element_observes_it_inside_its_border(
    cx: &mut TestAppContext,
) {
    let bounds = reported_bounds(cx, |reported| {
        div()
            .flex()
            .w(px(80.))
            .h(px(20.))
            .px(px(8.))
            .border_1()
            .child(div().size(px(10.)))
            .on_prepaint(move |bounds, _, _| reported.set(bounds))
            .into_any_element()
    });
    // The padding doesn't shift it.
    assert_eq!(
        bounds,
        Bounds::new(point(px(1.), px(1.)), size(px(78.), px(18.)))
    );
}
