//! Drive a textarea with keystrokes in a test and check its text, selection
//! and caret.
//!
//! Text is written with markers: `ˇ` is the caret, and `«` `»` enclose a
//! selection. The caret of a selection is at `»`, or at `«` when written
//! `«ˇ`.
//!
//! ```ignore
//! let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::MacOS, "heˇllo");
//! test.keys("ctrl-e");
//! test.assert("helloˇ");
//! test.keys("shift-left shift-left");
//! test.assert("hel«ˇlo»");
//! ```

use std::ops::Range;

use gpui::{
    Action, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};

use super::{CursorShape, Keymap, KeymapPlatform};
use crate::input::TextareaState;

/// Text with its markers taken out, and the selection they marked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Marked {
    pub(crate) text: String,
    /// `None` when the text has no markers.
    pub(crate) selection: Option<Range<usize>>,
    pub(crate) reversed: bool,
}

/// Parse marked text. See the module docs.
pub(crate) fn parse(marked: &str) -> Marked {
    let mut text = String::with_capacity(marked.len());
    let (mut caret, mut open, mut close) = (None, None, None);
    for c in marked.chars() {
        match c {
            'ˇ' => caret = Some(text.len()),
            '«' => open = Some(text.len()),
            '»' => close = Some(text.len()),
            c => text.push(c),
        }
    }
    let (selection, reversed) = match (open, close, caret) {
        (Some(start), Some(end), caret) => (Some(start..end), caret == Some(start) && start != end),
        (None, None, Some(caret)) => (Some(caret..caret), false),
        (None, None, None) => (None, false),
        _ => panic!("unbalanced selection markers in {marked:?}"),
    };
    Marked {
        text,
        selection,
        reversed,
    }
}

/// The text with markers for `selection`.
fn mark(text: &str, selection: &Range<usize>, reversed: bool) -> String {
    if selection.is_empty() {
        return format!("{}ˇ{}", &text[..selection.start], &text[selection.start..]);
    }
    let (open, close) = if reversed {
        ("«ˇ", "»")
    } else {
        ("«", "ˇ»")
    };
    format!(
        "{}{open}{}{close}{}",
        &text[..selection.start],
        &text[selection.clone()],
        &text[selection.end..]
    )
}

struct Harness {
    textarea: Entity<TextareaState>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().h(px(300.)).child(self.textarea.clone()))
    }
}

/// A focused textarea following a keybinding scheme, with one platform's
/// binding tables bound.
pub(crate) struct KeymapTest {
    pub(crate) cx: VisualTestContext,
    pub(crate) textarea: Entity<TextareaState>,
}

impl KeymapTest {
    /// A textarea following `keymap`, holding `marked` text, with the binding
    /// tables of `platform`: a test on any platform can check every
    /// platform's keys.
    pub(crate) fn new(
        cx: &mut TestAppContext,
        keymap: Keymap,
        platform: KeymapPlatform,
        marked: &str,
    ) -> Self {
        cx.update(|cx| {
            crate::init(cx);
            cx.clear_key_bindings();
            for keymap in Keymap::ALL {
                cx.bind_keys(keymap.bindings(platform));
            }
        });
        let marked = parse(marked);
        let window = cx.open_window(size(px(600.), px(400.)), move |window, cx| Harness {
            textarea: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .keymap(keymap)
                    .default_value(marked.text)
            }),
        });
        let cx = VisualTestContext::from_window(window.into(), cx);
        let textarea = window
            .read_with(&cx, |harness, _| harness.textarea.clone())
            .unwrap();
        let mut test = Self { cx, textarea };
        test.update(|state, window, cx| {
            state.focus(window, cx);
            if let Some(selection) = marked.selection {
                state.set_selected_range(selection, cx);
            }
            if marked.reversed {
                state.active_selection_mut().reversed = true;
            }
        });
        test.draw();
        test
    }

    pub(crate) fn draw(&mut self) {
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        self.cx.run_until_parked();
    }

    /// Press space-separated keystrokes, such as `"ctrl-x ctrl-s"` or
    /// `"d w"`.
    pub(crate) fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.draw();
    }

    /// Type text, as an input method would.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.draw();
    }

    /// Dispatch an action to the focused textarea.
    pub(crate) fn dispatch(&mut self, action: impl Action) {
        self.cx.dispatch_action(action);
        self.draw();
    }

    pub(crate) fn update<R>(
        &mut self,
        f: impl FnOnce(&mut TextareaState, &mut Window, &mut Context<TextareaState>) -> R,
    ) -> R {
        let textarea = self.textarea.clone();
        let result = self
            .cx
            .update(|window, cx| textarea.update(cx, |state, cx| f(state, window, cx)));
        self.draw();
        result
    }

    /// The text with markers for the selection.
    pub(crate) fn marked(&self) -> String {
        self.textarea.read_with(&self.cx, |state, _| {
            let selection = state.selected_range();
            mark(
                &state.value(),
                &selection,
                state.active_selection().reversed,
            )
        })
    }

    /// Check the text and, when `marked` has markers, the selection and
    /// caret.
    #[track_caller]
    pub(crate) fn assert(&self, marked: &str) {
        let expected = parse(marked);
        let actual = self.marked();
        if expected.selection.is_some() {
            assert!(
                parse(&actual) == expected,
                "text and selection\n  actual: {actual:?}\nexpected: {marked:?}"
            );
        } else {
            assert_eq!(parse(&actual).text, expected.text, "text");
        }
    }

    pub(crate) fn mode_label(&self) -> Option<SharedString> {
        self.textarea
            .read_with(&self.cx, |state, _| state.keymap_mode_label())
    }

    pub(crate) fn cursor_shape(&self) -> CursorShape {
        self.textarea
            .read_with(&self.cx, |state, _| state.cursor_shape())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_round_trip() {
        for marked in ["ˇ", "abˇc", "a«bc»d", "a«ˇbc»d"] {
            let parsed = parse(marked);
            let selection = parsed.selection.clone().unwrap();
            let again = mark(&parsed.text, &selection, parsed.reversed);
            assert_eq!(parse(&again), parsed, "{marked}");
        }
        assert_eq!(parse("a«bcˇ»d"), parse("a«bc»d"));
        assert_eq!(parse("plain").selection, None);
    }
}
