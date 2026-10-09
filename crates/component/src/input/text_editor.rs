use gpui::{
    App, Entity, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Window, relative,
};

use super::{ContextMenuTarget, SuggestionItemContext, Textarea, TextareaState};
use crate::native_menu::NativeMenu;
use crate::{Sizable, Size};

/// A textarea laid out as a text editor: it fills its parent's height, with
/// square corners and no border, on the theme's `editor.background` like a
/// code editor.
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
                .editor_surface(true),
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
