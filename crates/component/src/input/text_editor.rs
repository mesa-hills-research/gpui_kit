use gpui::{
    App, Entity, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Window, relative,
};

use super::{ContextMenuStyle, ContextMenuTarget, SuggestionItemContext, Textarea, TextareaState};
use crate::native_menu::NativeMenu;
use crate::{Sizable, Size};

/// A textarea laid out as a text editor: it fills its parent's height, with
/// square corners and no border, on the theme's `editor.background` like a
/// code editor. Its right-click menu is drawn by GPUI in the theme's colors on
/// every platform, see [`Self::context_menu_style`].
///
/// Give it a state set up with [`TextareaState::text_editor`], which turns on
/// line numbers, the search panel and soft wrap:
///
/// ```ignore
/// let document = cx.new(|cx| {
///     TextareaState::new(window, cx)
///         .text_editor()
///         .spell_checker(Rc::new(MyDictionary::load()))
///         .default_value(text)
/// });
///
/// TextEditor::new(&document)
/// ```
///
/// The text uses the inherited font. Set another with `font_family` and
/// `text_size`.
#[derive(IntoElement)]
pub struct TextEditor {
    textarea: Textarea,
}

impl TextEditor {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            textarea: Textarea::new(state)
                .h(relative(1.))
                .bordered(false)
                .rounded_none()
                .editor_surface(true)
                .context_menu_style(ContextMenuStyle::Drawn),
        }
    }

    /// Draw the textarea's border, default is `false`.
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.textarea = self.textarea.bordered(bordered);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.textarea = self.textarea.disabled(disabled);
        self
    }

    /// See [`Textarea::readonly`].
    pub fn readonly(mut self, readonly: bool) -> Self {
        self.textarea = self.textarea.readonly(readonly);
        self
    }

    pub fn tab_index(mut self, index: isize) -> Self {
        self.textarea = self.textarea.tab_index(index);
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.textarea = self.textarea.aria_label(label);
        self
    }

    /// See [`Textarea::context_menu`].
    pub fn context_menu(
        mut self,
        f: impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static,
    ) -> Self {
        self.textarea = self.textarea.context_menu(f);
        self
    }

    /// See [`Textarea::context_menu_at`].
    pub fn context_menu_at(
        mut self,
        f: impl Fn(NativeMenu, &ContextMenuTarget, &mut Window, &mut App) -> NativeMenu + 'static,
    ) -> Self {
        self.textarea = self.textarea.context_menu_at(f);
        self
    }

    /// What draws the right-click menu: GPUI's menu in the theme's colors,
    /// the default, or the operating system's menu on macOS and Windows with
    /// [`ContextMenuStyle::Native`].
    pub fn context_menu_style(mut self, style: ContextMenuStyle) -> Self {
        self.textarea = self.textarea.context_menu_style(style);
        self
    }

    /// See [`Textarea::suggestion_item`].
    pub fn suggestion_item<R: IntoElement>(
        mut self,
        render: impl Fn(&SuggestionItemContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.textarea = self.textarea.suggestion_item(render);
        self
    }

    /// See [`Textarea::on_paste`].
    pub fn on_paste(
        mut self,
        handler: impl Fn(&gpui::ClipboardItem, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.textarea = self.textarea.on_paste(handler);
        self
    }
}

impl Sizable for TextEditor {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.textarea = self.textarea.with_size(size);
        self
    }
}

impl Styled for TextEditor {
    fn style(&mut self) -> &mut StyleRefinement {
        self.textarea.style()
    }
}

impl RenderOnce for TextEditor {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.textarea
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext as _, Modifiers, MouseButton, ParentElement as _, Render, TestAppContext,
    };

    use super::*;

    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            gpui::div()
        }
    }

    /// A text editor's menu is drawn by GPUI whatever the platform, unless
    /// the application asks for the native one. A textarea keeps the native
    /// menu where there is one.
    #[gpui::test]
    fn the_text_editor_menu_is_drawn_on_every_platform(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let _ = cx.add_window_view(|window, cx| {
            let state = cx.new(|cx| TextareaState::new(window, cx).text_editor());
            let platforms = ["windows", "macos", "linux", "freebsd", "android", "ios"];

            let style = TextEditor::new(&state)
                .textarea
                .current_context_menu_style();
            assert_eq!(style, ContextMenuStyle::Drawn);
            assert!(platforms.iter().all(|os| !style.uses_os_menu(os)));

            let native = TextEditor::new(&state)
                .context_menu_style(ContextMenuStyle::Native)
                .textarea
                .current_context_menu_style();
            assert!(native.uses_os_menu("windows") && native.uses_os_menu("macos"));

            let textarea = Textarea::new(&state).current_context_menu_style();
            assert_eq!(textarea, ContextMenuStyle::Native);
            Probe
        });
    }

    struct Document {
        state: Entity<TextareaState>,
    }

    impl Render for Document {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            gpui::div().size_full().child(
                gpui::div()
                    .h(gpui::px(120.))
                    .child(TextEditor::new(&self.state)),
            )
        }
    }

    /// The menu GPUI draws opens complete: its first frame lays out the
    /// shortcuts beside the items, at the width it keeps.
    #[gpui::test]
    fn the_drawn_menu_opens_with_its_shortcuts(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut state = None;
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                let document = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .text_editor()
                        .default_value("A short note")
                });
                state = Some(document.clone());
                let view = cx.new(|_| Document { state: document });
                cx.new(|cx| crate::Root::new(view, window, cx))
            })
            .unwrap()
        });
        let state = state.unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let position = state.read_with(cx, |state, _| {
            state.range_to_bounds(&(2..3)).unwrap().center()
        });
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let select_all = if cfg!(target_os = "macos") {
            "kbd:cmd-a"
        } else {
            "kbd:ctrl-a"
        };
        assert!(cx.debug_bounds(select_all).is_some());
        let (first, settled) = cx.update(|window, cx| {
            let overlay = crate::root::WindowState::native_menu_overlay(window, cx).unwrap();
            let menu = overlay.read(cx).active_menu().expect("the menu is open");
            menu.read(cx).first_and_last_bounds()
        });
        assert_eq!(first.size, settled.size, "the menu must open at its width");
    }
}
