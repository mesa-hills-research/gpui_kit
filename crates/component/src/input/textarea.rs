use std::rc::Rc;

use gpui::{
    App, DefiniteLength, Entity, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, prelude::FluentBuilder as _,
};

use super::{ContextMenuStyle, ContextMenuTarget, Input, TextareaState};
use crate::native_menu::NativeMenu;
use crate::{RoleOverride, Sizable, Size, StyledExt as _};

/// A styled ordinary multi-line text field.
#[derive(IntoElement)]
pub struct Textarea {
    token_renderer: Option<gpui_base::input::InlineTokenRenderer>,
    token_click_listener: Option<gpui_base::input::InlineTokenClickListener>,
    token_hover_listener: Option<gpui_base::input::InlineTokenHoverListener>,
    state: Entity<TextareaState>,
    style: StyleRefinement,
    size: Size,
    height: Option<DefiniteLength>,
    appearance: bool,
    bordered: bool,
    disabled: bool,
    readonly: bool,
    tab_index: isize,
    role: RoleOverride,
    accessibility_id: Option<SharedString>,
    aria_label: Option<SharedString>,

    /// An optional context menu builder to allow a custom context menu.
    ///
    /// If set, this overrides the built-in context menu.
    context_menu_builder: Option<Rc<dyn Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu>>,
    context_menu_target_builder: Option<super::input::ContextMenuTargetBuilder>,

    paste_handler: Option<Rc<dyn Fn(&gpui::ClipboardItem, &mut Window, &mut App) -> bool>>,

    suggestion_item: Option<gpui_base::input::SuggestionItemRenderer>,

    /// Paint the theme's editor background. See [`Input::editor_surface`].
    editor_surface: bool,

    context_menu_style: ContextMenuStyle,
}

impl Textarea {
    /// The element each atomic inline token renders as, in place of the default
    /// [`InputToken`](super::InputToken); editing and history stay
    /// with the input.
    pub fn token<R: IntoElement>(
        mut self,
        render: impl Fn(&super::InlineTokenContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.token_renderer = Some(Rc::new(move |token, window, cx| {
            render(token, window, cx).into_any_element()
        }));
        self
    }
    /// Open a reference after a completed, unconsumed token click.
    pub fn on_token_click(
        mut self,
        listener: impl Fn(&super::InlineTokenClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.token_click_listener = Some(Rc::new(listener));
        self
    }
    /// Report pointer presence over a token so the application can show a
    /// tooltip or run custom logic. Hover never selects or edits.
    pub fn on_token_hover(
        mut self,
        listener: impl Fn(&super::InlineTokenHoverEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.token_hover_listener = Some(Rc::new(listener));
        self
    }

    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            size: Size::default(),
            height: None,
            appearance: true,
            bordered: true,
            disabled: false,
            readonly: false,
            tab_index: 0,
            role: RoleOverride::default(),
            accessibility_id: None,
            aria_label: None,
            context_menu_builder: None,
            context_menu_target_builder: None,
            paste_handler: None,
            token_renderer: None,
            token_click_listener: None,
            token_hover_listener: None,
            suggestion_item: None,
            editor_surface: false,
            context_menu_style: ContextMenuStyle::default(),
        }
    }

    /// Paint the theme's `editor.background`, as [`super::TextEditor`] does.
    pub(crate) fn editor_surface(mut self, editor_surface: bool) -> Self {
        self.editor_surface = editor_surface;
        self
    }

    /// The element each suggestion renders as in the suggestion menu, in place
    /// of the default row: the label, with what was typed highlighted, and the
    /// detail.
    ///
    /// The menu shows the suggestions of the state's
    /// [`SuggestionProvider`](super::SuggestionProvider); the renderer only
    /// draws them. Selection, scrolling and accepting stay with the menu.
    pub fn suggestion_item<R: IntoElement>(
        mut self,
        render: impl Fn(&super::SuggestionItemContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.suggestion_item = Some(Rc::new(move |item, window, cx| {
            render(item, window, cx).into_any_element()
        }));
        self
    }

    pub fn h(mut self, height: impl Into<DefiniteLength>) -> Self {
        self.height = Some(height.into());
        self
    }

    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the textarea to read-only, default is `false`.
    ///
    /// Unlike [`Self::disabled`], a read-only textarea keeps the normal appearance
    /// and still can be focused, selected and copied, it only rejects the changes
    /// made by the user.
    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        self
    }

    pub fn role(mut self, role: impl Into<RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    /// Set the developer-assigned accessibility identifier.
    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.accessibility_id = Some(id.into());
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// Replace the built-in context menu shown on right-click.
    ///
    /// The closure receives an empty menu and returns the one to show, so it
    /// decides entirely what appears — the default items are not added. It
    /// shows only while the state's context menu is enabled, which is the
    /// default. [`Self::context_menu_at`] replaces it.
    pub fn context_menu(
        mut self,
        f: impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static,
    ) -> Self {
        self.context_menu_builder = Some(Rc::new(f));
        self.context_menu_target_builder = None;
        self
    }

    /// Build the context menu knowing where it opened: the offset, the word
    /// there, a misspelling and its suggestions, and the marks.
    ///
    /// The closure receives an empty menu and returns the one to show.
    /// [`ContextMenuTarget::spelling_items`] and
    /// [`ContextMenuTarget::standard_items`] add what the default menu shows,
    /// so an application can put its own items before, between or after them:
    ///
    /// ```ignore
    /// Textarea::new(&state).context_menu_at(|menu, target, _, _| {
    ///     let menu = target.spelling_items(menu);
    ///     let menu = match target.word() {
    ///         Some(word) => menu
    ///             .menu(format!("Look Up “{word}”"), Box::new(LookUp(word.clone())))
    ///             .separator(),
    ///         None => menu,
    ///     };
    ///     target.standard_items(menu)
    /// })
    /// ```
    ///
    /// It replaces [`Self::context_menu`].
    pub fn context_menu_at(
        mut self,
        f: impl Fn(NativeMenu, &ContextMenuTarget, &mut Window, &mut App) -> NativeMenu + 'static,
    ) -> Self {
        self.context_menu_target_builder = Some(Rc::new(f));
        self.context_menu_builder = None;
        self
    }

    /// What draws the right-click menu: the operating system's menu on macOS
    /// and Windows, the default, or GPUI's menu in the theme's colors with
    /// [`ContextMenuStyle::Drawn`].
    pub fn context_menu_style(mut self, style: ContextMenuStyle) -> Self {
        self.context_menu_style = style;
        self
    }

    /// The style [`Self::context_menu_style`] set, for tests.
    #[cfg(test)]
    pub(crate) fn current_context_menu_style(&self) -> ContextMenuStyle {
        self.context_menu_style
    }

    /// Intercept paste payloads (images, files) before the default text insertion.
    ///
    /// `true` consumes the paste so nothing is inserted, `false` falls through
    /// to `clipboard.text()`. Copied files arrive as `ExternalPaths` through
    /// the same hook. On web the clipboard reads `None`; image paste needs
    /// async clipboard access and is out of scope.
    pub fn on_paste(
        mut self,
        handler: impl Fn(&gpui::ClipboardItem, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.paste_handler = Some(Rc::new(handler));
        self
    }
}

impl Sizable for Textarea {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Styled for Textarea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Textarea {
    /// The [`Input`] this textarea renders, for a compound control that frames
    /// it.
    pub(crate) fn into_input(self) -> Input {
        Input::from_state(self.state.clone())
            .when_some(self.token_renderer, |this, render| {
                this.token(move |token, window, cx| render(token, window, cx))
            })
            .when_some(self.token_click_listener, |this, listener| {
                this.on_token_click(move |event, window, cx| listener(event, window, cx))
            })
            .when_some(self.token_hover_listener, |this, listener| {
                this.on_token_hover(move |event, window, cx| listener(event, window, cx))
            })
            .appearance(self.appearance)
            .bordered(self.bordered)
            .disabled(self.disabled)
            .readonly(self.readonly)
            .tab_index(self.tab_index)
            .role(self.role)
            .with_size(self.size)
            .when_some(self.height, |this, height| this.h(height))
            .when_some(self.accessibility_id, |this, id| this.accessibility_id(id))
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .when_some(self.context_menu_builder, |this, build| {
                this.context_menu(move |menu, window, cx| build(menu, window, cx))
            })
            .when_some(self.paste_handler, |this, handler| {
                this.on_paste(move |item, window, cx| handler(item, window, cx))
            })
            .suggestion_item_renderer(self.suggestion_item)
            .context_menu_target_builder(self.context_menu_target_builder)
            .editor_surface(self.editor_surface)
            .context_menu_style(self.context_menu_style)
            .refine_style(&self.style)
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.into_input()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_on_paste_builder(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Render};

        struct Probe;
        impl Render for Probe {
            fn render(
                &mut self,
                _: &mut Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::div()
            }
        }

        cx.update(crate::init);
        let _ = cx.add_window_view(|window, cx| {
            let state = cx.new(|cx| TextareaState::new(window, cx));
            assert!(Textarea::new(&state).paste_handler.is_none());
            let textarea = Textarea::new(&state).on_paste(|_, _, _| true);
            assert!(textarea.paste_handler.is_some());
            Probe
        });
    }

    #[gpui::test]
    fn test_on_token_hover_builder(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Render};

        struct Probe;
        impl Render for Probe {
            fn render(
                &mut self,
                _: &mut Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::div()
            }
        }

        cx.update(crate::init);
        let _ = cx.add_window_view(|window, cx| {
            let state = cx.new(|cx| TextareaState::new(window, cx));
            assert!(Textarea::new(&state).token_hover_listener.is_none());
            let textarea = Textarea::new(&state).on_token_hover(|_, _, _| {});
            assert!(textarea.token_hover_listener.is_some());
            Probe
        });
    }

    /// The styled textarea shows its suggestion menu by itself, with rows from
    /// the application's renderer when it gives one, and the keyboard reaches
    /// the menu through it.
    #[gpui::test]
    fn textarea_shows_the_suggestion_menu(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, InteractiveElement as _, ParentElement as _, Render, div};
        use std::cell::Cell;

        struct Probe {
            state: Entity<TextareaState>,
            rendered: Rc<Cell<usize>>,
            custom: bool,
        }

        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                let rendered = self.rendered.clone();
                div()
                    .p_4()
                    .size_full()
                    .child(Textarea::new(&self.state).when(self.custom, |this| {
                        this.suggestion_item(move |item, _, _| {
                            rendered.set(rendered.get() + 1);
                            let ix = item.ix();
                            div()
                                .debug_selector(move || format!("custom-{ix}"))
                                .child(item.suggestion().label().clone())
                        })
                    }))
            }
        }

        cx.update(crate::init);
        for custom in [false, true] {
            let rendered = Rc::new(Cell::new(0));
            let (probe, cx) = cx.add_window_view(|window, cx| Probe {
                state: cx.new(|cx| TextareaState::new(window, cx)),
                rendered: rendered.clone(),
                custom,
            });
            let state = probe.read_with(cx, |probe, _| probe.state.clone());
            cx.update(|window, cx| {
                state.update(cx, |state, cx| state.focus(window, cx));
                window.draw(cx).clear(cx);
                state.update(cx, |state, cx| {
                    state.present_suggestions(
                        vec![
                            super::super::Suggestion::new("hello"),
                            super::super::Suggestion::new("help").with_detail("word"),
                            super::super::Suggestion::new("helium"),
                        ],
                        cx,
                    )
                });
                window.draw(cx).clear(cx);
            });

            assert!(cx.debug_bounds("suggestion-menu").is_some());
            assert!(cx.debug_bounds("suggestion-2").is_some());
            assert_eq!(cx.debug_bounds("custom-0").is_some(), custom);
            assert_eq!(rendered.get() >= 3, custom);

            cx.simulate_keystrokes("down enter");
            assert_eq!(state.read_with(cx, |state, _| state.value()), "help");
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert!(cx.debug_bounds("suggestion-menu").is_none());
        }
    }
}

#[cfg(test)]
mod spelling_tests {
    use std::cell::RefCell;

    use gpui::{
        AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, MouseDownEvent,
        MouseUpEvent, ParentElement as _, Render, SharedString, Task, TestAppContext,
        VisualTestContext, Window, div, px,
    };

    use super::*;
    use crate::input::{
        ContextMenuTarget, ReplaceMisspelling, SpellCheck, SpellCheckRequest, SpellChecker,
    };

    /// Marks every word not in its list, and suggests a fixed list for "teh".
    struct Words(&'static [&'static str]);

    impl SpellChecker for Words {
        fn check(
            &self,
            request: &SpellCheckRequest,
            _: &mut App,
        ) -> Task<anyhow::Result<SpellCheck>> {
            let mut misspelled = Vec::new();
            for range in request.ranges() {
                let text = request.text().slice(range.clone()).to_string();
                let mut offset = range.start;
                for word in text.split(|c: char| !c.is_alphabetic()) {
                    if !word.is_empty() && !self.0.contains(&word) {
                        misspelled.push(offset..offset + word.len());
                    }
                    offset += word.len() + 1;
                }
            }
            Task::ready(Ok(SpellCheck {
                misspelled,
                ..Default::default()
            }))
        }

        fn suggestions(&self, word: &str, _: &mut App) -> Vec<SharedString> {
            match word {
                "teh" => ["the", "ten", "tea", "tech", "then", "they"]
                    .map(SharedString::from)
                    .to_vec(),
                _ => Vec::new(),
            }
        }

        fn add_to_dictionary(&self, _: &str, _: &mut App) {}
    }

    struct Probe {
        state: Entity<TextareaState>,
        targets: Rc<RefCell<Vec<(usize, Option<SharedString>, Vec<String>)>>>,
        custom: bool,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let targets = self.targets.clone();
            div()
                .p_4()
                .size_full()
                .child(
                    Textarea::new(&self.state)
                        .h(px(120.))
                        .when(self.custom, |this| {
                            this.context_menu_at(move |menu, target, _, _| {
                                let menu = target.spelling_items(menu);
                                let menu = target.standard_items(menu);
                                targets.borrow_mut().push((
                                    target.offset(),
                                    target.word().cloned(),
                                    menu.item_labels(),
                                ));
                                menu
                            })
                        }),
                )
        }
    }

    const WORDS: &[&str] = &["I", "saw", "the", "cat", "a", "dog"];

    fn open<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
        custom: bool,
    ) -> (
        Entity<TextareaState>,
        Rc<RefCell<Vec<(usize, Option<SharedString>, Vec<String>)>>>,
        &'a mut VisualTestContext,
    ) {
        cx.update(crate::init);
        let targets: Rc<RefCell<Vec<_>>> = Default::default();
        let text = text.to_string();
        let (probe, cx) = cx.add_window_view({
            let targets = targets.clone();
            move |window, cx| Probe {
                state: cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .spell_checker(Rc::new(Words(WORDS)))
                        .default_value(text)
                }),
                targets,
                custom,
            }
        });
        let state = probe.read_with(cx, |probe, _| probe.state.clone());
        cx.update(|window, cx| {
            state.update(cx, |state, cx| state.focus(window, cx));
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        (state, targets, cx)
    }

    fn target_at(
        state: &Entity<TextareaState>,
        offset: usize,
        cx: &mut VisualTestContext,
    ) -> ContextMenuTarget {
        cx.update(|_, cx| {
            let capabilities = state.read(cx).context_menu_capabilities().offset(offset);
            ContextMenuTarget::new(state, capabilities, cx)
        })
    }

    /// On a misspelled word the menu lists five replacements, Add to
    /// Dictionary and Ignore, then the standard items. On a word spelled
    /// right it shows the standard items alone.
    #[gpui::test]
    fn the_menu_of_a_misspelled_word(cx: &mut TestAppContext) {
        let (state, _, cx) = open(cx, "I saw teh cat and a dgo", false);

        let target = target_at(&state, 7, cx);
        assert_eq!(target.word().map(|word| word.as_ref()), Some("teh"));
        assert_eq!(target.word_range(), Some(6..9));
        assert_eq!(target.misspelling().unwrap().range(), 6..9);
        assert_eq!(target.marks().len(), 1);
        assert_eq!(
            target.default_menu().item_labels(),
            [
                "the",
                "ten",
                "tea",
                "tech",
                "then",
                "-",
                "Add to Dictionary",
                "Ignore",
                "-",
                "Cut (disabled)",
                "Copy (disabled)",
                "Paste",
                "-",
                "Select All",
            ]
        );
        let revision = state.read_with(cx, |state, _| state.revision());
        assert!(
            target.default_menu().item_actions()[0].partial_eq(&ReplaceMisspelling {
                range: 6..9,
                text: "the".into(),
                revision,
            })
        );

        // A misspelling the checker has nothing for.
        let target = target_at(&state, 21, cx);
        assert_eq!(target.word().map(|word| word.as_ref()), Some("dgo"));
        assert_eq!(
            target.default_menu().item_labels()[..4],
            [
                "No Suggestions (disabled)",
                "-",
                "Add to Dictionary",
                "Ignore"
            ]
        );

        // A word spelled right.
        let target = target_at(&state, 3, cx);
        assert_eq!(target.word().map(|word| word.as_ref()), Some("saw"));
        assert!(target.misspelling().is_none());
        assert_eq!(
            target.default_menu().item_labels(),
            [
                "Cut (disabled)",
                "Copy (disabled)",
                "Paste",
                "-",
                "Select All"
            ]
        );
    }

    /// In a read-only textarea the replacements are disabled, and adding
    /// and ignoring, which leave the text alone, stay available.
    #[gpui::test]
    fn a_read_only_menu_keeps_the_dictionary_items(cx: &mut TestAppContext) {
        let (state, _, cx) = open(cx, "I saw teh cat", false);
        let labels = cx.update(|_, cx| {
            let capabilities = state
                .read(cx)
                .context_menu_capabilities()
                .readonly(true)
                .offset(7);
            ContextMenuTarget::new(&state, capabilities, cx)
                .default_menu()
                .item_labels()
        });
        assert_eq!(labels[0], "the (disabled)");
        assert_eq!(labels[6..8], ["Add to Dictionary", "Ignore"]);
    }

    /// A right-click gives `context_menu_at` the clicked word, and so does
    /// Shift-F10 at the caret.
    #[gpui::test]
    fn context_menu_at_hears_where_the_menu_opened(cx: &mut TestAppContext) {
        let (state, targets, cx) = open(cx, "I saw teh cat", true);
        let word = state.read_with(cx, |state, _| state.range_to_bounds(&(6..9)).unwrap());
        let position = word.center();
        for event in [true, false] {
            if event {
                cx.simulate_event(MouseDownEvent {
                    position,
                    button: MouseButton::Right,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                });
            } else {
                cx.simulate_event(MouseUpEvent {
                    position,
                    button: MouseButton::Right,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                });
            }
        }
        cx.run_until_parked();
        let (offset, word, labels) = targets.borrow_mut().pop().expect("the menu opened");
        assert!((6..=9).contains(&offset));
        assert_eq!(word.as_deref(), Some("teh"));
        assert_eq!(labels[0], "the");

        cx.update(|_, cx| state.update(cx, |state, cx| state.set_selected_range(2..2, cx)));
        cx.simulate_keystrokes("shift-f10");
        cx.run_until_parked();
        let (offset, word, labels) = targets.borrow_mut().pop().expect("the menu opened");
        assert_eq!(offset, 2);
        assert_eq!(word.as_deref(), Some("saw"));
        assert_eq!(labels[0], "Cut (disabled)");
    }
}
