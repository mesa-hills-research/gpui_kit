//! A textarea's right-click menu, with what is under the position it opened
//! at.

use std::ops::Range;

use gpui::{App, Entity, SharedString};
use gpui_base::input::{InputContextMenuCapabilities, Mark, Misspelling, RopeExt as _};
use rust_i18n::t;

use super::TextareaState;
use crate::native_menu::NativeMenu;

/// How many of the spell checker's suggestions the menu lists.
const MAX_SPELLING_SUGGESTIONS: usize = 5;

/// What draws a text field's right-click menu.
///
/// [`super::TextEditor`] uses [`Self::Drawn`], and inputs and textareas
/// [`Self::Native`]. Change it with `context_menu_style` on [`super::Input`],
/// [`super::Textarea`] or [`super::TextEditor`].
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

impl ContextMenuStyle {
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
        if self.uses_os_menu(std::env::consts::OS) {
            menu.show(position, window, cx);
        } else {
            menu.show_drawn(position, window, cx);
        }
    }
}

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
    use super::ContextMenuStyle;

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
