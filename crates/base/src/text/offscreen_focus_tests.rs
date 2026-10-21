//! The keys of a text view that holds focus while a list no longer draws it.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ListAlignment, ListOffset, ListState, Modifiers, MouseButton, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div, list, point, px,
};

use super::{TextView, TextViewState};
use crate::{
    Root, TextSelection, TextSelectionMenuRequest,
    input::{Input, InputState},
};

#[cfg(target_os = "macos")]
const COPY: &str = "cmd-c";
#[cfg(not(target_os = "macos"))]
const COPY: &str = "ctrl-c";
#[cfg(target_os = "macos")]
const SELECT_ALL: &str = "cmd-a";
#[cfg(not(target_os = "macos"))]
const SELECT_ALL: &str = "ctrl-a";

/// The whole selection the drag in [`open`] makes, as a copy takes it.
const SELECTION: &str = "Alpha one.\n\nBravo two.\n\nCharlie three.";

/// A text field above a list of rows of one document, as an application
/// shows a long document, with a focus handle around the list.
struct Rows {
    list: ListState,
    views: Vec<Entity<TextViewState>>,
    input: Entity<InputState>,
    container: FocusHandle,
}

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let views = self.views.clone();
        div()
            .w(px(400.))
            .flex()
            .flex_col()
            .child(div().h(px(30.)).child(Input::new(&self.input)))
            .child(
                div().track_focus(&self.container).child(
                    list(self.list.clone(), move |ix, _, _| {
                        TextView::new(&views[ix]).into_any_element()
                    })
                    .h(px(90.)),
                ),
            )
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Eight rows of one document under the window's `Root`, a drag from the
/// start of the first row to the end of the third, and the list scrolled
/// until it no longer draws the first row, which holds focus.
fn open(cx: &mut TestAppContext) -> (Entity<Rows>, &mut VisualTestContext) {
    cx.update(crate::init);
    let words = [
        "Alpha one.",
        "Bravo two.",
        "Charlie three.",
        "Delta four.",
        "Echo five.",
        "Foxtrot six.",
        "Golf seven.",
        "Hotel eight.",
    ];
    let document = cx.new(|cx| TextViewState::markdown(&words.join("\n\n"), cx));
    cx.run_until_parked();
    let views = (0..words.len())
        .map(|ix| cx.new(|cx| TextViewState::block_range(&document, ix..=ix, cx)))
        .collect::<Vec<_>>();
    let mut rows = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let content = cx.new(|cx| Rows {
            list: ListState::new(views.len(), ListAlignment::Top, px(0.)),
            views: views.clone(),
            input: cx.new(|cx| InputState::new(window, cx).default_value("typed words")),
            container: cx.focus_handle(),
        });
        rows = Some(content.clone());
        Root::new(content, window, cx)
    });
    let rows = rows.unwrap();
    draw(cx);

    let first = views[0].read_with(cx, |view, _| view.bounds());
    let third = views[2].read_with(cx, |view, _| view.bounds());
    let start = point(first.left() + px(1.), first.top() + px(4.));
    let end = point(third.right() - px(1.), third.bottom() - px(4.));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert!(
        cx.update(|window, cx| views[0].read(cx).focus_handle().is_focused(window)),
        "the drag focuses the row it starts in"
    );
    assert!(
        cx.update(|window, cx| rows.read(cx).container.contains_focused(window, cx)),
        "the row drawn holds focus in the list"
    );

    scroll_to(&rows, 1, cx);
    assert_eq!(
        selected_text(cx),
        SELECTION,
        "the row scrolled away is kept"
    );
    (rows, cx)
}

fn scroll_to(rows: &Entity<Rows>, item_ix: usize, cx: &mut VisualTestContext) {
    rows.read_with(cx, |rows, _| {
        rows.list.scroll_to(ListOffset {
            item_ix,
            offset_in_item: px(0.),
        })
    });
    rows.update(cx, |_, cx| cx.notify());
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(
        rows.read_with(cx, |rows, _| rows.list.logical_scroll_top().item_ix),
        item_ix
    );
}

fn selected_text(cx: &mut VisualTestContext) -> String {
    cx.update(|window, cx| TextSelection::selected_text(window, cx))
        .trim()
        .to_string()
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// Cmd-C or Ctrl-C copies the whole selection, with the blank lines between
/// its paragraphs, while the row holding focus is not drawn. So does the
/// Copy of the selection's context menu.
#[gpui::test]
fn the_copy_keys_copy_the_selection_of_a_focused_row_not_drawn(cx: &mut TestAppContext) {
    let (rows, cx) = open(cx);
    assert!(
        !cx.update(|window, cx| rows.read(cx).container.contains_focused(window, cx)),
        "the list no longer draws the row holding focus"
    );

    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.simulate_keystrokes(COPY);
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(SELECTION));

    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.update(|window, cx| window.dispatch_action(Box::new(crate::CopyTextSelection), cx));
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(SELECTION), "the menu's Copy");
}

/// Shift-F10 and the Menu key open the selection's context menu, under the
/// end of the selection still drawn, and Cmd-A or Ctrl-A selects all of the
/// row, as they do while the row is drawn. Drawn again, the row takes its
/// keys back.
#[gpui::test]
fn the_other_keys_of_a_focused_row_not_drawn_still_work(cx: &mut TestAppContext) {
    let requests = Rc::new(RefCell::new(Vec::<TextSelectionMenuRequest>::new()));
    let (rows, cx) = open(cx);
    cx.update(|_, cx| {
        let requests = requests.clone();
        TextSelection::on_context_menu(
            move |request, _, _| requests.borrow_mut().push(request.clone()),
            cx,
        );
    });

    let third = rows.read_with(cx, |rows, cx| rows.views[2].read(cx).bounds());
    for key in ["shift-f10", "menu"] {
        cx.simulate_keystrokes(key);
        draw(cx);
        let request = requests.borrow_mut().pop().expect(key);
        assert_eq!(request.selected_text().as_ref(), SELECTION, "{key}");
        assert!(request.supports_select_all(), "{key}");
        let position = request.position();
        assert!(
            third.top() < position.y && position.y <= third.bottom(),
            "{key} opens the menu under the end of the selection drawn, {position:?} in {third:?}"
        );
    }

    let first = rows.read_with(cx, |rows, _| rows.views[0].clone());
    cx.simulate_keystrokes(SELECT_ALL);
    draw(cx);
    assert!(first.read_with(cx, |view, _| view.is_all_selected()));

    scroll_to(&rows, 0, cx);
    assert!(
        cx.update(|window, cx| rows.read(cx).container.contains_focused(window, cx)),
        "the row drawn again holds focus in the list"
    );
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.simulate_keystrokes(COPY);
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some(selected_text(cx).as_str()));
}

/// A text field with focus copies its own selection, not the window's text
/// selection.
#[gpui::test]
fn a_focused_text_field_copies_its_own_selection(cx: &mut TestAppContext) {
    let (rows, cx) = open(cx);
    let input = rows.read_with(cx, |rows, _| rows.input.clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        selected_text(cx),
        SELECTION,
        "the window keeps its selection"
    );

    cx.simulate_keystrokes(COPY);
    draw(cx);
    assert_eq!(clipboard(cx).as_deref(), Some("typed words"));
}
