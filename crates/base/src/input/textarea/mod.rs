use gpui::{App, Div, Entity, IntoElement, RenderOnce, Stateful, Window};

use super::{InputBaseState, InputModeKind, TextareaExtras, TextareaMode};

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`]. What a textarea
/// offers instead are suggestions from an application's provider: see
/// [`super::SuggestionProvider`].
pub type TextareaState = InputBaseState<TextareaMode>;

impl InputModeKind for TextareaMode {
    const MULTI_LINE: bool = true;

    type Extras = TextareaExtras;

    fn did_edit(
        state: &mut InputBaseState<Self>,
        range: &std::ops::Range<usize>,
        new_len: usize,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.record_suggestion_edit(range, new_len, cx);
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
        TextareaState::register_suggestion_actions(element, entity, window)
    }
}

/// What a textarea exposes to the renderer. See [`crate::input::InputExtras`].
impl crate::input::InputExtras for TextareaExtras {
    fn ghost_text(&self) -> Option<&str> {
        self.suggestions.ghost_text()
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
