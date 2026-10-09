use gpui::{App, Div, Entity, IntoElement, RenderOnce, Stateful, Window};

use super::{InputBaseState, InputModeKind, TextareaExtras, TextareaMode};

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`]. What a textarea
/// offers instead are suggestions from an application's provider (see
/// [`super::SuggestionProvider`]), marks (see [`super::Mark`]), spell checking
/// (see [`super::SpellChecker`]) and a line-number gutter.
pub type TextareaState = InputBaseState<TextareaMode>;

impl InputModeKind for TextareaMode {
    const MULTI_LINE: bool = true;

    type Extras = TextareaExtras;

    fn adjust_annotations(
        state: &mut InputBaseState<Self>,
        range: &std::ops::Range<usize>,
        new_len: usize,
    ) {
        state.extras.marks.adjust_for_edit(range, new_len);
    }

    fn did_edit(
        state: &mut InputBaseState<Self>,
        range: &std::ops::Range<usize>,
        new_len: usize,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.record_suggestion_edit(range, new_len, cx);
        state.record_spelling_edit(range, new_len, cx);
    }

    fn on_render(
        state: &mut InputBaseState<Self>,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.spelling_on_render(window, cx);
    }

    fn on_text_typed(
        state: &mut InputBaseState<Self>,
        _range: &std::ops::Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.suggest_after_typing(text, window, cx);
    }

    fn accept_inline_completion(
        state: &mut InputBaseState<Self>,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) -> bool {
        state.accept_suggestion_on_tab(window, cx)
    }

    fn has_inline_completion(state: &InputBaseState<Self>) -> bool {
        state.has_suggestion_ghost()
    }

    fn clear_inline_completion(
        state: &mut InputBaseState<Self>,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.close_suggestions(cx);
    }

    fn hide_context_menu(
        state: &mut InputBaseState<Self>,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.close_suggestions(cx);
    }

    // `is_context_menu_open` keeps its default, `false`, while the suggestion
    // menu is open. The engine reads it to keep a menu open when the input
    // loses focus to it, but the suggestion menu never takes focus: losing
    // focus means the user went elsewhere, and the suggestions close.

    fn handle_context_menu_action(
        state: &mut InputBaseState<Self>,
        action: Box<dyn gpui::Action>,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) -> bool {
        state.handle_suggestion_action(&*action, window, cx)
    }

    fn register_actions(
        element: Stateful<Div>,
        entity: &Entity<InputBaseState<Self>>,
        window: &mut Window,
    ) -> Stateful<Div> {
        let element = TextareaState::register_suggestion_actions(element, entity, window);
        TextareaState::register_spelling_actions(element, entity, window)
    }
}

/// What a textarea exposes to the renderer. See [`crate::input::InputExtras`].
impl crate::input::InputExtras for TextareaExtras {
    fn ghost_text(&self) -> Option<&str> {
        self.suggestions.ghost_text()
    }

    fn line_number(&self) -> bool {
        self.line_number
    }

    fn underlines(
        &self,
        range: &std::ops::Range<usize>,
        style: &crate::input::InputEditorStyle,
    ) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
        self.marks.underlines(range, style)
    }
}

/// An unstyled ordinary multi-line text input.
#[derive(IntoElement)]
pub struct Textarea {
    presentation: super::InlineTokenPresentation,
    state: Entity<TextareaState>,
}

impl Textarea {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            presentation: Default::default(),
        }
    }
    /// The element each atomic token renders as; the input keeps editing and history.
    pub fn token<R: IntoElement>(
        mut self,
        render: impl Fn(&super::InlineTokenContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.presentation = self.presentation.token(render);
        self
    }
    /// Open a reference after a completed, unconsumed token click.
    pub fn on_token_click(
        mut self,
        listener: impl Fn(&super::InlineTokenClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.presentation = self.presentation.on_token_click(listener);
        self
    }
    /// Report pointer presence over a token; hover never selects or edits.
    pub fn on_token_hover(
        mut self,
        listener: impl Fn(&super::InlineTokenHoverEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.presentation = self.presentation.on_token_hover(listener);
        self
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.set_token_presentation(self.presentation)
        });
        self.state
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{
        AppContext as _, Context, IntoElement, ParentElement as _, Pixels, Point, Render,
        Styled as _, TestAppContext, VisualTestContext, div, px, size,
    };

    use super::*;
    use crate::input::InputContextMenuCapabilities;

    struct Harness {
        textareas: Vec<Entity<TextareaState>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(
                self.textareas
                    .iter()
                    .map(|textarea| div().h(px(80.)).child(textarea.clone())),
            )
        }
    }

    fn open(
        cx: &mut TestAppContext,
        states: Vec<fn(TextareaState) -> TextareaState>,
    ) -> (VisualTestContext, Vec<Entity<TextareaState>>) {
        cx.update(crate::init);
        let window = cx.open_window(size(px(400.), px(400.)), move |window, cx| Harness {
            textareas: states
                .into_iter()
                .map(|configure| {
                    cx.new(|cx| configure(TextareaState::new(window, cx).default_value("Hello")))
                })
                .collect(),
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let textareas = window
            .read_with(&cx, |harness, _| harness.textareas.clone())
            .unwrap();
        cx.update(|window, cx| {
            textareas[0].update(cx, |state, cx| state.focus(window, cx));
            window.draw(cx).clear(cx);
        });
        (cx, textareas)
    }

    /// Line numbers take a gutter, which moves the text right.
    #[gpui::test]
    fn line_numbers_take_a_gutter(cx: &mut TestAppContext) {
        let (cx, textareas) = open(cx, vec![|state| state, |state| state.line_number(true)]);
        let left = |textarea: &Entity<TextareaState>| -> Pixels {
            textarea.read_with(&cx, |state, _| {
                state.range_to_bounds(&(0..0)).unwrap().left() - state.input_bounds().left()
            })
        };
        let plain = left(&textareas[0]);
        let numbered = left(&textareas[1]);
        assert!(
            numbered > plain + px(10.),
            "{numbered:?} leaves room for the line numbers, {plain:?} does not"
        );
        assert!(textareas[1].read_with(&cx, |state, _| state.presentation().has_line_numbers()));
        assert!(!textareas[0].read_with(&cx, |state, _| state.presentation().has_line_numbers()));
    }

    /// Shift-F10 and the Menu key open the context menu under the caret, as
    /// a right-click on the caret's character would.
    #[gpui::test]
    fn the_keyboard_opens_the_context_menu_at_the_caret(cx: &mut TestAppContext) {
        let (mut cx, textareas) = open(cx, vec![|state| state]);
        let textarea = textareas[0].clone();
        let opened: Rc<RefCell<Vec<(InputContextMenuCapabilities, Point<Pixels>)>>> =
            Default::default();
        cx.update(|window, cx| {
            textarea.update(cx, |state, cx| {
                let opened = opened.clone();
                state.on_context_menu(Rc::new(move |_, capabilities, position, _, _| {
                    opened.borrow_mut().push((capabilities, position));
                }));
                state.set_selected_range(3..3, cx);
            });
            window.draw(cx).clear(cx);
        });

        for key in ["shift-f10", "menu"] {
            cx.simulate_keystrokes(key);
            cx.run_until_parked();
            let (capabilities, position) = opened.borrow_mut().pop().expect(key);
            assert_eq!(capabilities.opened_at(), Some(3));
            let caret = textarea.read_with(&cx, |state, _| state.range_to_bounds(&(3..3)).unwrap());
            assert_eq!(position, caret.bottom_left());
        }

        textarea.update(&mut cx, |state, _| state.set_context_menu_enabled(false));
        cx.simulate_keystrokes("shift-f10");
        cx.run_until_parked();
        assert!(opened.borrow().is_empty());
    }
}
