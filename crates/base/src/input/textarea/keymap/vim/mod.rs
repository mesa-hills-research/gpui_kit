//! Vim's modes and keys.
//!
//! The scheme keeps a [`VimState`] per textarea. Its mode goes into the key
//! context as `vim_mode`, so each mode has bindings of its own, and decides
//! the caret's shape, the mode label and whether typed text is inserted.
//!
//! This holds the mode switch and a few motions to start from. Operators,
//! counts, registers, visual mode and the rest of Vim's keys go in this
//! folder, one module per area.

use gpui::{
    Context, Div, Entity, InteractiveElement as _, KeyBinding, KeyContext, SharedString, Stateful,
    Window, actions,
};

use super::{CursorShape, Keymap, KeymapPlatform, KeymapState, bind, common};
use crate::input::{MoveDown, MoveLeft, MoveRight, MoveUp, TextareaState};

actions!(
    vim,
    [
        /// Leave insert mode for normal mode.
        SwitchToNormalMode,
        /// Leave normal mode to insert text before the caret.
        SwitchToInsertMode,
    ]
);

/// Vim's modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VimMode {
    /// Keys are commands. Typed text is not inserted.
    #[default]
    Normal,
    /// Keys insert text.
    Insert,
}

impl VimMode {
    /// The value of `vim_mode` in the key context.
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Insert => "insert",
        }
    }
}

/// What Vim keeps for one textarea.
#[derive(Debug, Default)]
pub struct VimState {
    mode: VimMode,
}

impl VimState {
    pub fn mode(&self) -> VimMode {
        self.mode
    }
}

impl KeymapState for VimState {
    fn key_context(&self, context: &mut KeyContext) {
        context.set("vim_mode", self.mode.name());
    }

    fn cursor_shape(&self) -> CursorShape {
        match self.mode {
            VimMode::Normal => CursorShape::Block,
            VimMode::Insert => CursorShape::Bar,
        }
    }

    fn mode_label(&self) -> Option<SharedString> {
        Some(match self.mode {
            VimMode::Normal => "NORMAL".into(),
            VimMode::Insert => "INSERT".into(),
        })
    }

    fn accepts_text_input(&self) -> bool {
        self.mode == VimMode::Insert
    }
}

/// The key context of the bindings for `mode`.
fn mode_context(mode: VimMode) -> String {
    format!("{} && vim_mode == {}", Keymap::Vim.context(), mode.name())
}

/// Vim's bindings on `platform`.
pub(super) fn bindings(_platform: KeymapPlatform) -> Vec<KeyBinding> {
    let normal = &mode_context(VimMode::Normal);
    let insert = &mode_context(VimMode::Insert);
    let mut bindings = common::bindings(insert);
    bindings.extend([
        bind("escape", SwitchToNormalMode, insert),
        bind("i", SwitchToInsertMode, normal),
        bind("h", MoveLeft, normal),
        bind("j", MoveDown, normal),
        bind("k", MoveUp, normal),
        bind("l", MoveRight, normal),
        bind("left", MoveLeft, normal),
        bind("down", MoveDown, normal),
        bind("up", MoveUp, normal),
        bind("right", MoveRight, normal),
    ]);
    bindings
}

pub(super) fn new_state() -> Option<Box<dyn KeymapState>> {
    Some(Box::<VimState>::default())
}

fn set_mode(state: &mut TextareaState, mode: VimMode, cx: &mut Context<TextareaState>) {
    let Some(vim) = state.keymap_state_mut::<VimState>() else {
        cx.propagate();
        return;
    };
    vim.mode = mode;
    cx.notify();
}

/// Registers Vim's own actions.
pub(super) fn register_actions(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
) -> Stateful<Div> {
    element
        .on_action(
            window.listener_for(entity, |state, _: &SwitchToNormalMode, _, cx| {
                set_mode(state, VimMode::Normal, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &SwitchToInsertMode, _, cx| {
                set_mode(state, VimMode::Insert, cx)
            }),
        )
}

/// Text typed while Vim is active, before it is inserted. Outside insert
/// mode Vim takes it, so a key without a binding inserts nothing.
pub(super) fn typed_text(
    state: &mut TextareaState,
    _text: &str,
    _window: &mut Window,
    _cx: &mut Context<TextareaState>,
) -> bool {
    state
        .keymap_state::<VimState>()
        .is_some_and(|vim| vim.mode != VimMode::Insert)
}
