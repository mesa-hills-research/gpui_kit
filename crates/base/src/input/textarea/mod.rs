use gpui::{App, Div, Entity, IntoElement, RenderOnce, Stateful, Window};

use super::{InputBaseState, InputModeKind, TextareaExtras, TextareaMode};

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`]. What a textarea
/// offers instead are suggestions from an application's provider (see
/// [`super::SuggestionProvider`]), marks (see [`super::Mark`]), spell checking
/// (see [`super::SpellChecker`]), keybinding schemes (see [`super::Keymap`])
/// and a line-number gutter.
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
        state.extras.keymap.adjust_for_edit(range, new_len);
        state.tentative.adjust_for_edit(range, new_len);
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
        state.keymap_on_render(cx);
        state.spelling_on_render(window, cx);
    }

    fn on_line_break(
        state: &mut InputBaseState<Self>,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.spelling_after_typing("\n", cx);
    }

    fn on_text_typed(
        state: &mut InputBaseState<Self>,
        _range: &std::ops::Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
        state.spelling_after_typing(text, cx);
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
        let element = TextareaState::register_spelling_actions(element, entity, window);
        TextareaState::register_keymap_actions(element, entity, window)
    }

    fn takes_typed_text(
        state: &mut InputBaseState<Self>,
        text: &str,
        window: &mut Window,
        cx: &mut gpui::Context<InputBaseState<Self>>,
    ) -> bool {
        state.keymap_takes_typed_text(text, window, cx)
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

    fn key_context(&self, context: &mut gpui::KeyContext) {
        self.keymap.key_context(context);
    }

    fn cursor_shape(&self) -> crate::input::CursorShape {
        self.keymap.cursor_shape()
    }

    fn accepts_text_input(&self) -> bool {
        self.keymap.accepts_text_input()
    }

    fn underlines(
        &self,
        range: &std::ops::Range<usize>,
        style: &crate::input::InputEditorStyle,
    ) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
        self.marks.underlines(range, style)
    }

    fn caret_offset(&self) -> Option<usize> {
        self.keymap.caret_offset()
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
        AppContext as _, Context, Focusable as _, InteractiveElement as _, IntoElement,
        ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, VisualTestContext,
        div, px, size,
    };

    use super::*;
    use crate::input::{InputContextMenuCapabilities, InputEvent};

    struct Harness {
        textareas: Vec<Entity<TextareaState>>,
        /// For a menu drawn in the window to focus.
        menu: gpui::FocusHandle,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(
                    self.textareas
                        .iter()
                        .map(|textarea| div().h(px(80.)).child(textarea.clone())),
                )
                .child(div().track_focus(&self.menu).h(px(20.)))
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
            menu: cx.focus_handle(),
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

    /// Line numbers take a gutter, which moves the text right. The text
    /// editor preset turns them on with search.
    #[gpui::test]
    fn line_numbers_take_a_gutter(cx: &mut TestAppContext) {
        let (cx, textareas) = open(
            cx,
            vec![|state| state, |state| state.line_number(true), |state| {
                state.text_editor()
            }],
        );
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
        assert_eq!(left(&textareas[2]), numbered);

        textareas[2].read_with(&cx, |state, _| {
            assert!(state.presentation().has_line_numbers());
            assert!(state.searchable);
            assert!(state.soft_wrap);
        });
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

    /// A right-click on a selection keeps it for the menu: on it, just past
    /// its end, and after Select All past the end of the last line and in the
    /// empty space below the text. So does the menu key, at the caret on the
    /// selection's end. A right-click away from the selection moves the caret
    /// there, and the menu's Paste replaces what stays selected.
    #[gpui::test]
    fn a_right_click_on_the_selection_keeps_it(cx: &mut TestAppContext) {
        use gpui::{ClipboardItem, Modifiers, MouseButton, point};

        const TEXT: &str = "Hello world\nSecond line";
        let (mut cx, textareas) = open(cx, vec![|state| state.default_value(TEXT)]);
        let textarea = textareas[0].clone();
        let opened = Rc::new(RefCell::new(0));
        cx.update(|window, cx| {
            textarea.update(cx, |state, cx| {
                let opened = opened.clone();
                state.on_context_menu(Rc::new(move |_, _, _, _, _| *opened.borrow_mut() += 1));
                state.select_all(window, cx);
            });
            window.draw(cx).clear(cx);
        });
        let right_click = |position: Point<Pixels>, cx: &mut VisualTestContext| {
            cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
            cx.run_until_parked();
        };
        let bounds = |range: std::ops::Range<usize>, cx: &VisualTestContext| {
            textarea.read_with(cx, |state, _| state.range_to_bounds(&range).unwrap())
        };
        let selected =
            |cx: &VisualTestContext| textarea.read_with(cx, |state, _| state.selected_range());

        let end = bounds(TEXT.len()..TEXT.len(), &cx);
        let below = point(
            end.left() + px(4.),
            textarea.read_with(&cx, |state, _| state.input_bounds().bottom()) - px(4.),
        );
        assert!(
            below.y > end.bottom() + px(4.),
            "the click is below the text"
        );
        for position in [end.center(), end.center() + point(px(60.), px(0.)), below] {
            right_click(position, &mut cx);
            assert_eq!(selected(&cx), 0..TEXT.len(), "at {position:?}");
        }
        cx.simulate_keystrokes("shift-f10");
        cx.run_until_parked();
        assert_eq!(selected(&cx), 0..TEXT.len());
        assert_eq!(*opened.borrow(), 4);

        // "world", which ends at the end of its line.
        textarea.update(&mut cx, |state, cx| state.set_selected_range(6..11, cx));
        let world_end = bounds(11..11, &cx);
        right_click(world_end.center() + point(px(40.), px(0.)), &mut cx);
        assert_eq!(selected(&cx), 6..11);
        right_click(bounds(0..1, &cx).left_center(), &mut cx);
        assert_eq!(selected(&cx), 0..0);

        cx.update(|window, cx| textarea.update(cx, |state, cx| state.select_all(window, cx)));
        cx.write_to_clipboard(ClipboardItem::new_string("Pasted".into()));
        right_click(below, &mut cx);
        cx.dispatch_action(crate::input::Paste);
        assert_eq!(textarea.read_with(&cx, |state, _| state.value()), "Pasted");
    }

    /// The focus and blur events a textarea emits from now on, as "focus" and
    /// "blur", in an active window.
    fn record_focus_events(
        textarea: &Entity<TextareaState>,
        cx: &mut VisualTestContext,
    ) -> Rc<RefCell<Vec<&'static str>>> {
        let events: Rc<RefCell<Vec<&'static str>>> = Default::default();
        // An inactive window sends no focus events.
        cx.update(|window, _| window.activate_window());
        settle(cx);
        cx.update(|_, cx| {
            let events = events.clone();
            cx.subscribe(textarea, move |_, event: &InputEvent, _| match event {
                InputEvent::Focus => events.borrow_mut().push("focus"),
                InputEvent::Blur => events.borrow_mut().push("blur"),
                _ => {}
            })
            .detach();
        });
        events
    }

    /// Run what the last input started, and draw, which sends the focus
    /// events.
    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// A context menu handler may move the focus somewhere else and show no
    /// menu. The field it opened on loses the focus, as for any other move.
    #[gpui::test]
    fn a_context_menu_handler_can_move_the_focus_away(cx: &mut TestAppContext) {
        let (mut cx, textareas) = open(cx, vec![|state| state, |state| state]);
        let other = textareas[1].clone();
        cx.update(|_, cx| {
            textareas[0].update(cx, |state, _| {
                state.on_context_menu(Rc::new(move |_, _, _, window, cx| {
                    other.update(cx, |other, cx| other.focus(window, cx));
                }));
            });
        });
        let events = record_focus_events(&textareas[0], &mut cx);

        cx.simulate_keystrokes("shift-f10");
        settle(&mut cx);

        assert!(cx.update(|window, cx| textareas[1].focus_handle(cx).is_focused(window)));
        assert!(!cx.update(|window, cx| textareas[0].read(cx).is_focused(window, cx)));
        assert_eq!(*events.borrow(), ["blur"]);
    }

    /// A drawn menu that takes the focus from its field holds it for the
    /// field. The field goes on drawing as focused and sends no blur, and when
    /// the focus comes back no focus. Focus that moves on from the menu to
    /// another field blurs it then.
    #[gpui::test]
    fn a_drawn_context_menu_holds_the_focus_for_its_field(cx: &mut TestAppContext) {
        let (mut cx, textareas) = open(cx, vec![|state| state, |state| state]);
        let field = textareas[0].clone();
        let menu = cx.update(|window, cx| {
            let harness = window.root::<Harness>().flatten().unwrap();
            harness.read(cx).menu.clone()
        });
        cx.update(|_, cx| {
            let menu = menu.clone();
            field.update(cx, |state, _| {
                state.on_context_menu(Rc::new(move |_, _, _, window, cx| {
                    let from = window.focused(cx).expect("the field has the focus");
                    menu.focus(window, cx);
                    crate::input::drawn_menu_took_focus(&from, &menu, cx);
                }));
            });
        });
        let events = record_focus_events(&field, &mut cx);
        let field_focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| field.read(cx).is_focused(window, cx))
        };

        cx.simulate_keystrokes("shift-f10");
        settle(&mut cx);
        assert!(cx.update(|window, _| menu.is_focused(window)));
        assert!(field_focused(&mut cx));

        cx.update(|window, cx| field.update(cx, |state, cx| state.focus(window, cx)));
        settle(&mut cx);
        assert!(field_focused(&mut cx));
        assert_eq!(*events.borrow(), [] as [&str; 0]);

        cx.simulate_keystrokes("shift-f10");
        settle(&mut cx);
        assert!(field_focused(&mut cx));
        cx.update(|window, cx| textareas[1].update(cx, |state, cx| state.focus(window, cx)));
        settle(&mut cx);
        assert!(!field_focused(&mut cx));
        assert_eq!(*events.borrow(), ["blur"]);
    }
}
