//! Keybinding schemes for a textarea: CUA (the default), Emacs and Vim.
//!
//! A textarea follows one [`Keymap`] at a time, set with
//! [`TextareaState::keymap`] or [`TextareaState::set_keymap`]. Each scheme is
//! a module of its own, which owns its binding tables, its per-textarea state
//! and any actions only it needs:
//!
//! | Module | Scheme |
//! |---|---|
//! | `cua` | [`Keymap::Cua`]: the platform's usual shortcuts, the kit's default |
//! | `emacs` | [`Keymap::Emacs`] |
//! | `vim` | [`Keymap::Vim`] |
//! | `common` | keys every scheme can share: arrows, Backspace, Enter, Tab… |
//! | `commands` | editing commands any scheme can bind (see below) |
//!
//! # Key contexts
//!
//! Every input's element has the key context `Input` with a `keymap` entry,
//! `cua` for single-line inputs and editors, and the textarea's scheme for a
//! textarea. A scheme's bindings use [`Keymap::context`], such as
//! `Input && keymap == emacs`, so only the active scheme's bindings apply. A
//! scheme's state adds entries of its own through
//! [`KeymapState::key_context`], such as Vim's `vim_mode == normal`, and
//! bindings that depend on them extend the scheme's context. Bindings an
//! application adds in the plain `Input` context apply in every scheme.
//!
//! # Binding tables
//!
//! Each scheme builds its table with a function that takes the platform as a
//! value, [`KeymapPlatform`], instead of `#[cfg]` attributes, so a test on one
//! platform can check every platform's table. [`init`] binds the tables of the
//! platform the app runs on, CUA's first, then Emacs's and Vim's.
//!
//! # Editing commands
//!
//! The actions schemes bind to. Most are the engine's own:
//!
//! - Moving: [`MoveLeft`](super::MoveLeft), `MoveRight`, `MoveUp`, `MoveDown`,
//!   `MoveHome` and `MoveEnd` (the visual row), `MoveToStartOfLine` and
//!   `MoveToEndOfLine`, `MoveToPreviousWord` (start of the word),
//!   `MoveToNextWord` (end of the word), [`MoveToNextWordStart`],
//!   [`MoveToParagraphStart`], [`MoveToParagraphEnd`], `MoveToStart`,
//!   `MoveToEnd`, `MovePageUp`, `MovePageDown`.
//! - Selecting: `SelectLeft`, `SelectRight`, `SelectUp`, `SelectDown`
//!   (in `crate::actions`), `SelectToStartOfLine`, `SelectToEndOfLine`,
//!   `SelectToPreviousWordStart`, `SelectToNextWordEnd`,
//!   [`SelectToNextWordStart`], [`SelectToParagraphStart`],
//!   [`SelectToParagraphEnd`], `SelectToStart`, `SelectToEnd`, `SelectAll`.
//! - Deleting: `Backspace`, `Delete`, `DeleteToBeginningOfLine`,
//!   `DeleteToEndOfLine`, `DeleteToPreviousWordStart`, `DeleteToNextWordEnd`,
//!   [`DeleteLine`].
//! - Lines: `Enter`, [`NewlineAbove`], [`NewlineBelow`], [`JoinLines`],
//!   `Indent`, `Outdent`, `IndentInline`, `OutdentInline`.
//! - Changing text: [`TransposeCharacters`], [`ConvertToUpperCase`],
//!   [`ConvertToLowerCase`], [`ConvertToTitleCase`].
//! - Clipboard and history: `Cut`, `Copy`, `Paste`, `Undo`, `Redo`.
//! - Everything else: `Escape`, `Search`, `Replace`, `ShowContextMenu`,
//!   `ShowCharacterPalette`, `AddCursorAbove`, `AddCursorBelow`, and the
//!   suggestion actions (`ShowSuggestions`, `AcceptSuggestion`, …).
//!
//! A command only one scheme needs, such as Emacs's kill ring or Vim's
//! operators, lives in that scheme's module with the state it keeps.

mod commands;
mod common;
pub mod cua;
mod emacs;
#[cfg(test)]
pub(crate) mod test;
mod vim;

use std::{any::Any, ops::Range};

use gpui::{
    Action, App, Context, Div, Entity, KeyBinding, KeyContext, SharedString, Stateful, Window,
};
use serde::{Deserialize, Serialize};

use super::TextareaState;

pub use commands::{
    ConvertToLowerCase, ConvertToTitleCase, ConvertToUpperCase, DeleteLine, JoinLines,
    MoveToNextWordStart, MoveToParagraphEnd, MoveToParagraphStart, NewlineAbove, NewlineBelow,
    SelectToNextWordStart, SelectToParagraphEnd, SelectToParagraphStart, TransposeCharacters,
};
pub use emacs::{EmacsState, SaveBuffer, WriteFile};
pub use vim::{VimMode, VimState};

/// Which keybinding scheme a textarea follows.
///
/// ```
/// use gpui_base::input::Keymap;
///
/// assert_eq!(Keymap::default(), Keymap::Cua);
/// assert_eq!(Keymap::Emacs.context(), "Input && keymap == emacs");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keymap {
    /// The platform's usual shortcuts: Ctrl on Windows and Linux, Cmd on
    /// macOS.
    #[default]
    Cua,
    /// Emacs keys. On macOS, Option is Meta.
    Emacs,
    /// Vim's modes and keys.
    Vim,
}

impl Keymap {
    /// Every scheme, in the order their bindings are added.
    pub const ALL: [Keymap; 3] = [Keymap::Cua, Keymap::Emacs, Keymap::Vim];

    /// The value of the `keymap` entry in a textarea's key context: `cua`,
    /// `emacs` or `vim`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cua => "cua",
            Self::Emacs => "emacs",
            Self::Vim => "vim",
        }
    }

    /// The key-context predicate of the scheme's bindings, such as
    /// `Input && keymap == emacs`.
    pub fn context(self) -> &'static str {
        match self {
            Self::Cua => "Input && keymap == cua",
            Self::Emacs => "Input && keymap == emacs",
            Self::Vim => "Input && keymap == vim",
        }
    }

    /// The scheme's bindings on `platform`.
    pub fn bindings(self, platform: KeymapPlatform) -> Vec<KeyBinding> {
        match self {
            Self::Cua => cua::bindings(platform),
            Self::Emacs => emacs::bindings(platform),
            Self::Vim => vim::bindings(platform),
        }
    }

    /// A fresh state for one textarea, for a scheme that keeps one.
    fn new_state(self) -> Option<Box<dyn KeymapState>> {
        match self {
            Self::Cua => None,
            Self::Emacs => emacs::new_state(),
            Self::Vim => vim::new_state(),
        }
    }
}

/// The platform a binding table is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeymapPlatform {
    MacOS,
    Windows,
    /// Linux and the other platforms: the web and the BSDs take the same
    /// table.
    Linux,
}

impl KeymapPlatform {
    pub const ALL: [KeymapPlatform; 3] = [Self::MacOS, Self::Windows, Self::Linux];

    /// The platform this build runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub fn is_macos(self) -> bool {
        self == Self::MacOS
    }

    /// The modifier of the platform's own shortcuts, such as Copy: `cmd` on
    /// macOS, `ctrl` elsewhere.
    pub fn primary(self) -> &'static str {
        if self.is_macos() { "cmd" } else { "ctrl" }
    }
}

/// How the caret is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// A thin bar between characters.
    #[default]
    Bar,
    /// A block over the character after the caret.
    Block,
    /// A line under the character after the caret.
    Underline,
}

/// What a keybinding scheme keeps for one textarea, such as Vim's mode,
/// pending operator, count and registers.
///
/// The scheme creates it when the textarea switches to it, and its actions
/// reach it with [`TextareaState::keymap_state`] and
/// [`TextareaState::keymap_state_mut`]. A scheme action that changes it
/// calls `cx.notify()`, so the key context, caret and mode label follow.
pub trait KeymapState: Any {
    /// Add entries to the textarea's key context, for bindings that depend on
    /// this state, such as `vim_mode = normal`.
    fn key_context(&self, context: &mut KeyContext) {
        _ = context;
    }

    /// How the caret is drawn. Defaults to a bar.
    fn cursor_shape(&self) -> CursorShape {
        CursorShape::Bar
    }

    /// A short name for the current mode, for an application to show, such as
    /// `NORMAL`. Defaults to none.
    fn mode_label(&self) -> Option<SharedString> {
        None
    }

    /// Whether typed text goes into the text. A mode whose letters are
    /// commands answers false. Defaults to true.
    fn accepts_text_input(&self) -> bool {
        true
    }

    /// Follow an edit that replaced `range` with `new_len` bytes, to keep
    /// offsets the state holds, such as Emacs's mark, on the same text.
    fn adjust_for_edit(&mut self, range: &Range<usize>, new_len: usize) {
        _ = (range, new_len);
    }
}

/// A binding in `context`, for building a scheme's table.
pub(crate) fn bind<A: Action>(keystrokes: &str, action: A, context: &str) -> KeyBinding {
    KeyBinding::new(keystrokes, action, Some(context))
}

/// Bind every scheme's table for the platform the app runs on.
pub(crate) fn init(cx: &mut App) {
    let platform = KeymapPlatform::current();
    for keymap in Keymap::ALL {
        cx.bind_keys(keymap.bindings(platform));
    }
}

/// A textarea's scheme: which one, and the state it keeps.
#[derive(Default)]
pub(crate) struct KeymapSettings {
    keymap: Keymap,
    state: Option<Box<dyn KeymapState>>,
}

impl KeymapSettings {
    fn set(&mut self, keymap: Keymap) {
        self.keymap = keymap;
        self.state = keymap.new_state();
    }

    pub(crate) fn key_context(&self, context: &mut KeyContext) {
        context.set("keymap", self.keymap.name());
        if let Some(state) = &self.state {
            state.key_context(context);
        }
    }

    pub(crate) fn cursor_shape(&self) -> CursorShape {
        self.state
            .as_ref()
            .map_or(CursorShape::Bar, |state| state.cursor_shape())
    }

    pub(crate) fn accepts_text_input(&self) -> bool {
        self.state
            .as_ref()
            .is_none_or(|state| state.accepts_text_input())
    }

    pub(crate) fn adjust_for_edit(&mut self, range: &Range<usize>, new_len: usize) {
        if let Some(state) = &mut self.state {
            state.adjust_for_edit(range, new_len);
        }
    }
}

/// Methods for a textarea's keybinding scheme. See [`Keymap`].
impl TextareaState {
    /// Follow the keybinding scheme `keymap`. CUA by default.
    pub fn keymap(mut self, keymap: Keymap) -> Self {
        self.extras.keymap.set(keymap);
        self
    }

    /// Switch to the keybinding scheme `keymap`. The scheme starts afresh,
    /// for Vim in normal mode.
    pub fn set_keymap(&mut self, keymap: Keymap, cx: &mut Context<Self>) {
        self.extras.keymap.set(keymap);
        cx.notify();
    }

    /// The keybinding scheme the textarea follows.
    pub fn current_keymap(&self) -> Keymap {
        self.extras.keymap.keymap
    }

    /// The scheme's state, when it is a `T`.
    pub fn keymap_state<T: KeymapState>(&self) -> Option<&T> {
        let state: &dyn Any = self.extras.keymap.state.as_deref()?;
        state.downcast_ref()
    }

    /// The scheme's state, when it is a `T`, to change it.
    pub fn keymap_state_mut<T: KeymapState>(&mut self) -> Option<&mut T> {
        let state: &mut dyn Any = self.extras.keymap.state.as_deref_mut()?;
        state.downcast_mut()
    }

    /// How the caret is drawn, as the scheme's state asks.
    pub fn cursor_shape(&self) -> CursorShape {
        self.extras.keymap.cursor_shape()
    }

    /// The scheme's current mode, for an application to show, such as
    /// `NORMAL` or `INSERT` in Vim. Observe the state to follow it.
    pub fn keymap_mode_label(&self) -> Option<SharedString> {
        self.extras.keymap.state.as_ref()?.mode_label()
    }

    /// Offer typed text to the scheme first. Returns whether the scheme took
    /// it, in which case it is not inserted.
    pub(crate) fn keymap_takes_typed_text(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.current_keymap() {
            Keymap::Cua => false,
            Keymap::Emacs => emacs::typed_text(self, text, window, cx),
            Keymap::Vim => vim::typed_text(self, text, window, cx),
        }
    }

    /// Registers the editing commands and every scheme's own actions on the
    /// textarea's root element. A scheme's actions are bound only in its own
    /// context, so the others never reach them.
    pub(crate) fn register_keymap_actions(
        element: Stateful<Div>,
        entity: &Entity<Self>,
        window: &mut Window,
    ) -> Stateful<Div> {
        let element = commands::register_actions(element, entity, window);
        let element = emacs::register_actions(element, entity, window);
        vim::register_actions(element, entity, window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `bindings` binds the keystroke `keys`.
    fn binds(bindings: &[KeyBinding], keys: &str) -> bool {
        let keys = gpui::Keystroke::parse(keys).unwrap();
        bindings
            .iter()
            .any(|binding| binding.keystrokes()[0].inner() == &keys)
    }

    /// Each scheme's bindings are in its own context, on every platform.
    #[test]
    fn every_binding_is_in_its_schemes_context() {
        for keymap in Keymap::ALL {
            for platform in KeymapPlatform::ALL {
                for binding in keymap.bindings(platform) {
                    let context = binding.predicate().map(|predicate| predicate.to_string());
                    assert!(
                        context.as_deref().is_some_and(
                            |context| context.contains(&format!("keymap == {}", keymap.name()))
                        ),
                        "{keymap:?} on {platform:?}: {:?} is bound in {context:?}",
                        binding.keystrokes()
                    );
                }
            }
        }
    }

    use super::test::KeymapTest;
    use gpui::TestAppContext;

    /// Only the active scheme's keys apply: Ctrl-A selects all in CUA and
    /// moves to the line's start in Emacs. Switching takes effect at once.
    #[gpui::test]
    fn only_the_active_schemes_bindings_apply(cx: &mut TestAppContext) {
        let mut test = KeymapTest::new(cx, Keymap::Cua, KeymapPlatform::Linux, "one\ntwˇo");
        test.keys("ctrl-a");
        test.assert("«one\ntwo»");

        test.update(|state, _, cx| {
            state.set_keymap(Keymap::Emacs, cx);
            state.set_selected_range(6..6, cx);
        });
        test.keys("ctrl-a");
        test.assert("one\nˇtwo");
        test.keys("ctrl-e alt-b");
        test.assert("one\nˇtwo");
        test.keys("ctrl-p ctrl-f");
        test.assert("oˇne\ntwo");

        // Actions reach the textarea whatever the scheme.
        test.dispatch(crate::input::SelectAll);
        test.assert("«one\ntwo»");
    }

    /// On macOS, Emacs's Meta is Option and Cmd keeps the system shortcuts.
    #[gpui::test]
    fn emacs_on_macos_uses_option_as_meta(cx: &mut TestAppContext) {
        let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::MacOS, "ˇhello world");
        test.keys("alt-f");
        test.assert("helloˇ world");
        test.keys("cmd-a");
        test.assert("«hello world»");
    }

    /// Vim's state decides the key context, the caret, the mode label and
    /// whether typed text is inserted.
    #[gpui::test]
    fn a_modal_scheme_owns_its_state(cx: &mut TestAppContext) {
        let mut test = KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::Linux, "aˇbc");
        assert_eq!(test.mode_label().as_deref(), Some("NORMAL"));
        assert_eq!(test.cursor_shape(), CursorShape::Block);
        test.keys("l");
        test.assert("abˇc");
        test.type_text("x");
        test.assert("abˇc");

        test.keys("i");
        assert_eq!(test.mode_label().as_deref(), Some("INSERT"));
        assert_eq!(test.cursor_shape(), CursorShape::Bar);
        test.type_text("xy");
        test.assert("abxyˇc");
        test.keys("escape h");
        assert_eq!(test.mode_label().as_deref(), Some("NORMAL"));
        test.assert("abxˇyc");
        assert!(
            test.textarea.read_with(&test.cx, |state, _| state
                .keymap_state::<VimState>()
                .map(VimState::mode))
                == Some(VimMode::Normal)
        );

        test.update(|state, _, cx| state.set_keymap(Keymap::Cua, cx));
        assert_eq!(test.mode_label(), None);
        assert_eq!(test.cursor_shape(), CursorShape::Bar);
        test.type_text("z");
        test.assert("abxzˇyc");
    }

    /// The schemes reuse one platform's keys where they share them, and
    /// differ where the platforms do.
    #[test]
    fn tables_follow_the_platform() {
        let mac = Keymap::Cua.bindings(KeymapPlatform::MacOS);
        let linux = Keymap::Cua.bindings(KeymapPlatform::Linux);
        assert!(binds(&mac, "cmd-c"));
        assert!(!binds(&mac, "ctrl-c"));
        assert!(binds(&linux, "ctrl-c"));
        assert!(!binds(&linux, "cmd-c"));

        let emacs = Keymap::Emacs.bindings(KeymapPlatform::MacOS);
        assert!(binds(&emacs, "alt-f"), "Option is Meta");
        assert!(binds(&emacs, "cmd-c"), "Cmd keeps Copy");
        assert!(!binds(
            &Keymap::Emacs.bindings(KeymapPlatform::Linux),
            "cmd-c"
        ));
    }
}
