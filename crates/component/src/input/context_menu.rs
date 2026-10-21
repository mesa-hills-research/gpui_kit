//! A textarea's right-click menu, with what is under the position it opened
//! at.

use std::ops::Range;

use gpui::{App, Entity, Global, SharedString};
use gpui_base::input::{InputContextMenuCapabilities, Mark, Misspelling, RopeExt as _};
use rust_i18n::t;

use super::TextareaState;
use crate::native_menu::NativeMenu;

/// How many of the spell checker's suggestions the menu lists.
const MAX_SPELLING_SUGGESTIONS: usize = 5;

/// What draws a text field's right-click menu.
///
/// [`super::TextEditor`] uses [`Self::Drawn`], and inputs and textareas
/// [`Self::Native`]. [`Self::set_app_default`] chooses one style for every
/// text field in the application, the ones inside other components too.
/// `context_menu_style` on [`super::Input`], [`super::Textarea`] or
/// [`super::TextEditor`] sets the style of one field, which wins over the
/// application's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContextMenuStyle {
    /// A [`PopupMenu`](crate::menu::PopupMenu) drawn by GPUI in the theme's
    /// colors, on every platform. It stays inside the window.
    Drawn,
    /// The operating system's menu on macOS and Windows, which can reach past
    /// the window's edge. Other platforms have none, and show the drawn menu.
    #[default]
    Native,
}

/// The style [`ContextMenuStyle::set_app_default`] chose.
struct AppContextMenuStyle(ContextMenuStyle);

impl Global for AppContextMenuStyle {}

impl ContextMenuStyle {
    /// Use `style` for the right-click menu of every text field that sets no
    /// style of its own with `context_menu_style`, text editors included.
    ///
    /// This reaches the fields that other components build for themselves,
    /// such as the search field of [`crate::setting::Settings`] or of a
    /// searchable [`crate::select::Select`], which an application can't style
    /// one by one. A menu reads the style as it opens, so calling this again
    /// changes the next menu of every field.
    ///
    /// ```ignore
    /// gpui_kit::init(cx);
    /// ContextMenuStyle::set_app_default(ContextMenuStyle::Drawn, cx);
    /// ```
    pub fn set_app_default(style: Self, cx: &mut App) {
        cx.set_global(AppContextMenuStyle(style));
    }

    /// The style [`Self::set_app_default`] chose, or `None` when the
    /// application chose none.
    pub fn app_default(cx: &App) -> Option<Self> {
        cx.try_global::<AppContextMenuStyle>().map(|style| style.0)
    }

    /// The style a text field's menu opens in: the field's `own` when it set
    /// one, else the application's `app` default, else the component's
    /// `fallback`.
    pub(crate) fn resolve(own: Option<Self>, app: Option<Self>, fallback: Self) -> Self {
        own.or(app).unwrap_or(fallback)
    }

    /// Whether the operating system draws a menu of this style on `os`, a
    /// [`std::env::consts::OS`] value.
    pub(crate) fn uses_os_menu(self, os: &str) -> bool {
        self == Self::Native && matches!(os, "macos" | "windows")
    }

    /// Show `menu` at `position` in this style.
    pub(crate) fn show(
        self,
        menu: NativeMenu,
        position: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut App,
    ) {
        #[cfg(test)]
        cx.default_global::<ShownMenuStyles>().0.push(self);
        if self.uses_os_menu(std::env::consts::OS) {
            menu.show(position, window, cx);
        } else {
            menu.show_drawn(position, window, cx);
        }
    }

    /// The styles text field menus opened in so far, oldest first, for tests.
    #[cfg(test)]
    pub(crate) fn shown(cx: &App) -> Vec<Self> {
        cx.try_global::<ShownMenuStyles>()
            .map(|shown| shown.0.clone())
            .unwrap_or_default()
    }
}

/// The styles text field menus opened in, for tests.
#[cfg(test)]
#[derive(Default)]
struct ShownMenuStyles(Vec<ContextMenuStyle>);

#[cfg(test)]
impl Global for ShownMenuStyles {}

/// Where a textarea's context menu opened and what is there, for a menu
/// builder given to [`super::Textarea::context_menu_at`].
///
/// A right-click opens the menu at the clicked character, and Shift-F10 or
/// the Menu key at the caret. [`Self::spelling_items`] and
/// [`Self::standard_items`] add the items the default menu shows, so a
/// builder can put its own around them.
pub struct ContextMenuTarget {
    offset: usize,
    capabilities: InputContextMenuCapabilities,
    word: Option<(Range<usize>, SharedString)>,
    misspelling: Option<Misspelling>,
    suggestions: Vec<SharedString>,
    marks: Vec<Mark>,
}

impl ContextMenuTarget {
    pub(crate) fn new(
        state: &Entity<TextareaState>,
        capabilities: InputContextMenuCapabilities,
        cx: &mut App,
    ) -> Self {
        let textarea = state.read(cx);
        let offset = capabilities
            .opened_at()
            .unwrap_or_else(|| textarea.cursor());
        let misspelling = textarea.misspelling_at(offset);
        let marks = textarea.marks_at(offset);
        let text = textarea.text();
        let word = match &misspelling {
            Some(misspelling) => Some((misspelling.range(), misspelling.word().clone())),
            None => text
                .word_range(offset)
                .or_else(|| text.word_range(offset.checked_sub(1)?))
                .map(|range| {
                    let word = SharedString::new(text.slice(range.clone()).to_string());
                    (range, word)
                }),
        };
        let mut suggestions = misspelling
            .as_ref()
            .map(|misspelling| misspelling.suggestions(cx))
            .unwrap_or_default();
        suggestions.truncate(MAX_SPELLING_SUGGESTIONS);
        Self {
            offset,
            capabilities,
            word,
            misspelling,
            suggestions,
            marks,
        }
    }

    /// The byte offset the menu opened at.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// What the textarea allows: editing, copying, whether text is selected.
    pub fn capabilities(&self) -> InputContextMenuCapabilities {
        self.capabilities
    }

    /// The word at [`Self::offset`]: the misspelled word when there is one,
    /// otherwise the run of letters, digits and underscores there.
    pub fn word(&self) -> Option<&SharedString> {
        self.word.as_ref().map(|(_, word)| word)
    }

    /// The byte range of [`Self::word`].
    pub fn word_range(&self) -> Option<Range<usize>> {
        self.word.as_ref().map(|(range, _)| range.clone())
    }

    /// The misspelled word at [`Self::offset`], when the spell checker marked
    /// one there.
    pub fn misspelling(&self) -> Option<&Misspelling> {
        self.misspelling.as_ref()
    }

    /// The spell checker's replacements for [`Self::misspelling`], best
    /// first: at most five.
    pub fn spelling_suggestions(&self) -> &[SharedString] {
        &self.suggestions
    }

    /// The marks at [`Self::offset`], the spelling underline among them.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// Add the fixes for the misspelled word: its replacements (or a
    /// disabled **No Suggestions**), then **Add to Dictionary** and **Ignore**,
    /// each group followed by a separator. Adds nothing when no word there is
    /// misspelled.
    pub fn spelling_items(&self, mut menu: NativeMenu) -> NativeMenu {
        let Some(misspelling) = &self.misspelling else {
            return menu;
        };
        let editable = self.capabilities.is_editable();
        if self.suggestions.is_empty() {
            menu =
                menu.menu_with_disabled(t!("Input.No Suggestions"), true, Box::new(gpui::NoAction));
        }
        for suggestion in &self.suggestions {
            menu = menu.menu_with_disabled(
                suggestion.clone(),
                !editable,
                Box::new(misspelling.replace_with(suggestion.clone())),
            );
        }
        menu.separator()
            .menu(
                t!("Input.Add to Dictionary"),
                Box::new(misspelling.add_to_dictionary()),
            )
            .menu(t!("Input.Ignore"), Box::new(misspelling.ignore()))
            .separator()
    }

    /// Add Cut, Copy, Paste and Select All, each disabled when it cannot
    /// apply.
    pub fn standard_items(&self, menu: NativeMenu) -> NativeMenu {
        standard_items(menu, self.capabilities)
    }

    /// The menu a textarea shows without a builder of its own: the spelling
    /// fixes, then the standard items.
    pub(crate) fn default_menu(&self) -> NativeMenu {
        self.standard_items(self.spelling_items(NativeMenu::new()))
    }
}

/// Cut, Copy, Paste and Select All, as every input's menu shows them.
pub(crate) fn standard_items(
    menu: NativeMenu,
    capabilities: InputContextMenuCapabilities,
) -> NativeMenu {
    let enabled = !capabilities.is_disabled();
    let editable = enabled && !capabilities.is_readonly();
    menu.menu_with_disabled(
        t!("Input.Cut"),
        !(editable && capabilities.is_copyable()),
        Box::new(gpui_base::input::Cut),
    )
    .menu_with_disabled(
        t!("Input.Copy"),
        !capabilities.is_copyable(),
        Box::new(gpui_base::input::Copy),
    )
    // Offered whenever the text can change, without peeking at the clipboard:
    // the synchronous read is always empty on the web, and an empty clipboard
    // pastes nothing.
    .menu_with_disabled(
        t!("Input.Paste"),
        !editable,
        Box::new(gpui_base::input::Paste),
    )
    .separator()
    .menu(
        t!("Input.Select All"),
        Box::new(gpui_base::input::SelectAll),
    )
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
        MouseButton, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
        Window, div, px,
    };

    use super::ContextMenuStyle;
    use crate::input::{Editor, EditorState, Input, InputState, TextEditor, Textarea};

    /// A field's own style wins, then the application's, then the
    /// component's.
    #[test]
    fn a_field_style_wins_over_the_app_default() {
        use ContextMenuStyle::{Drawn, Native};
        for fallback in [Drawn, Native] {
            assert_eq!(ContextMenuStyle::resolve(None, None, fallback), fallback);
            for app in [Drawn, Native] {
                assert_eq!(ContextMenuStyle::resolve(None, Some(app), fallback), app);
                for own in [Drawn, Native] {
                    let style = ContextMenuStyle::resolve(Some(own), Some(app), fallback);
                    assert_eq!(style, own);
                }
            }
        }
    }

    /// One field of each kind, and an input that sets its own style.
    struct Fields {
        input: Entity<InputState>,
        native_input: Entity<InputState>,
        textarea: Entity<crate::input::TextareaState>,
        text_editor: Entity<crate::input::TextareaState>,
        editor: Entity<EditorState>,
    }

    impl Render for Fields {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let field =
                |selector: &'static str| div().w_full().debug_selector(move || selector.into());
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(field("input").child(Input::new(&self.input)))
                .child(field("native-input").child(
                    Input::new(&self.native_input).context_menu_style(ContextMenuStyle::Native),
                ))
                .child(field("textarea").child(Textarea::new(&self.textarea).h(px(60.))))
                .child(
                    field("text-editor")
                        .h(px(60.))
                        .child(TextEditor::new(&self.text_editor)),
                )
                .child(field("editor").child(Editor::new(&self.editor).h(px(60.))))
        }
    }

    fn open(cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|window, cx| Fields {
            input: cx.new(|cx| InputState::new(window, cx).default_value("one")),
            native_input: cx.new(|cx| InputState::new(window, cx).default_value("two")),
            textarea: cx
                .new(|cx| crate::input::TextareaState::new(window, cx).default_value("three")),
            text_editor: cx.new(|cx| {
                crate::input::TextareaState::new(window, cx)
                    .text_editor()
                    .default_value("four")
            }),
            editor: cx.new(|cx| EditorState::new(window, cx).default_value("five")),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx
    }

    /// Right-click the field `selector` names, and return the style its menu
    /// opened in.
    fn menu_style(selector: &'static str, cx: &mut VisualTestContext) -> ContextMenuStyle {
        let position = cx.debug_bounds(selector).expect(selector).center();
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let shown = cx.update(|_, cx| ContextMenuStyle::shown(cx));
        *shown.last().expect("the menu opened")
    }

    /// Without an application default, inputs, textareas and code editors
    /// keep the native menu and text editors draw theirs. An application
    /// default reaches every field without a style of its own, as its next
    /// menu opens, and a field's own style still wins.
    #[gpui::test]
    fn fields_follow_the_app_default(cx: &mut TestAppContext) {
        use ContextMenuStyle::{Drawn, Native};
        let cx = open(cx);
        let fields = ["input", "native-input", "textarea", "text-editor", "editor"];
        let styles = |cx: &mut VisualTestContext| fields.map(|field| menu_style(field, cx));

        assert_eq!(cx.update(|_, cx| ContextMenuStyle::app_default(cx)), None);
        assert_eq!(styles(cx), [Native, Native, Native, Drawn, Native]);

        cx.update(|_, cx| ContextMenuStyle::set_app_default(Drawn, cx));
        assert_eq!(
            cx.update(|_, cx| ContextMenuStyle::app_default(cx)),
            Some(Drawn)
        );
        assert_eq!(styles(cx), [Drawn, Native, Drawn, Drawn, Drawn]);

        cx.update(|_, cx| ContextMenuStyle::set_app_default(Native, cx));
        assert_eq!(styles(cx), [Native, Native, Native, Native, Native]);
    }

    /// The drawn style is drawn by GPUI everywhere. The native one is the
    /// operating system's where there is one.
    #[test]
    fn which_menu_each_style_shows_on_each_platform() {
        for os in ["windows", "macos", "linux", "freebsd", "android", "ios"] {
            assert!(!ContextMenuStyle::Drawn.uses_os_menu(os), "{os}");
            assert_eq!(
                ContextMenuStyle::Native.uses_os_menu(os),
                matches!(os, "windows" | "macos"),
                "{os}"
            );
        }
    }
}
