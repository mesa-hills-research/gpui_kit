//! Text editor spelling workflows: a right-click or Shift-F10 on a misspelled
//! word opens the context menu GPUI draws on every platform, and its items fix
//! the word.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    App, AppContext, Context, Entity, InputEvent as _, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, Result, SharedString, Task, TestAppContext, Window, WindowHandle,
    component::input::{SpellCheck, SpellCheckRequest, SpellChecker, TextEditor, TextareaState},
    div,
    prelude::*,
    px, size,
    test::{TestAppContextExt, TestWindowExt},
};

use crate::common;

#[cfg(target_os = "macos")]
const UNDO: &str = "cmd-z";
#[cfg(not(target_os = "macos"))]
const UNDO: &str = "ctrl-z";

/// Marks "teh", until it is added to the dictionary, and suggests three
/// replacements for it.
#[derive(Default)]
struct Teh {
    learned: Cell<bool>,
}

impl SpellChecker for Teh {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<Result<SpellCheck>> {
        let mut misspelled = Vec::new();
        for range in request.ranges().iter().filter(|_| !self.learned.get()) {
            let text = request.text().slice(range.clone()).to_string();
            misspelled.extend(
                text.match_indices("teh")
                    .map(|(ix, word)| range.start + ix..range.start + ix + word.len()),
            );
        }
        Task::ready(Ok(SpellCheck {
            misspelled,
            ..Default::default()
        }))
    }

    fn suggestions(&self, _: &str, _: &mut App) -> Vec<SharedString> {
        ["the", "tea", "ten"].map(SharedString::from).to_vec()
    }

    fn add_to_dictionary(&self, word: &str, _: &mut App) {
        self.learned.set(word == "teh");
    }
}

struct Writer {
    document: Entity<TextareaState>,
}

impl Render for Writer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(div().h(px(200.)).child(TextEditor::new(&self.document)))
    }
}

fn writer(
    cx: &mut TestAppContext,
    text: &'static str,
) -> (WindowHandle<gpui_kit::base::Root>, Entity<TextareaState>) {
    cx.update(gpui_kit::init);
    let (window, view) = common::open_window(cx, Some(size(px(480.), px(480.))), |window, cx| {
        cx.new(|cx| {
            let document = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .spell_checker(Rc::new(Teh::default()))
                    .default_value(text)
            });
            document.update(cx, |document, cx| document.focus(window, cx));
            Writer { document }
        })
    });
    let document = cx.update(|cx| view.read(cx).document.clone());
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    (window, document)
}

fn right_click(window: &mut Window, document: &Entity<TextareaState>, offset: usize, cx: &mut App) {
    window.render_frame(cx);
    let position = document
        .read(cx)
        .range_to_bounds(&(offset..offset + 1))
        .expect("laid-out character")
        .center();
    let modifiers = Modifiers::default();
    window.dispatch_event(
        MouseDownEvent {
            position,
            button: MouseButton::Right,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            position,
            button: MouseButton::Right,
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
}

fn value(document: &Entity<TextareaState>, cx: &mut TestAppContext) -> String {
    cx.update(|cx| document.read(cx).value().to_string())
}

#[gpui_kit::test]
async fn right_click_a_misspelling_and_choose_a_suggestion(cx: &mut TestAppContext) {
    let (window, document) = writer(cx, "I saw teh cat");
    cx.update_window(window.into(), |_, window, cx| {
        right_click(window, &document, 7, cx)
    })
    .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_some()
    })
    .await;
    cx.update_window(window.into(), |_, window, cx| {
        window.within("popup-menu").click(1usize, cx);
    })
    .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
    assert_eq!(value(&document, cx), "I saw tea cat");

    cx.update_window(window.into(), |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    assert_eq!(value(&document, cx), "I saw teh cat");
}

#[gpui_kit::test]
async fn shift_f10_opens_the_fixes_and_add_to_dictionary_unmarks_the_word(cx: &mut TestAppContext) {
    let (window, document) = writer(cx, "teh end, teh start");
    let marked = |cx: &mut TestAppContext| cx.update(|cx| document.read(cx).misspellings());
    assert_eq!(marked(cx), [0..3, 9..12]);

    cx.update(|cx| document.update(cx, |document, cx| document.set_selected_range(1..1, cx)));
    cx.update_window(window.into(), |_, window, cx| window.press("shift-f10", cx))
        .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_some()
    })
    .await;
    // The three suggestions and a separator come first.
    cx.update_window(window.into(), |_, window, cx| {
        window.within("popup-menu").click(4usize, cx);
    })
    .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
    // The dictionary changed, so the text is checked again, and the checker
    // knows the word now.
    cx.run_until_parked();
    assert!(marked(cx).is_empty());
    assert_eq!(value(&document, cx), "teh end, teh start");
}

/// The menu is driven from the keyboard: Shift-F10 opens it at the caret, the
/// arrow keys move through the items and Enter chooses one.
#[gpui_kit::test]
async fn the_keyboard_opens_the_menu_and_chooses_a_fix(cx: &mut TestAppContext) {
    let (window, document) = writer(cx, "I saw teh cat");
    cx.update(|cx| document.update(cx, |document, cx| document.set_selected_range(7..7, cx)));
    cx.update_window(window.into(), |_, window, cx| window.press("shift-f10", cx))
        .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_some()
    })
    .await;
    cx.update_window(window.into(), |_, window, cx| {
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.wait_for(window.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
    assert_eq!(value(&document, cx), "I saw tea cat");
}
