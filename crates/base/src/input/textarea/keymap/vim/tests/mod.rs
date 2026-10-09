//! Vim's tests, by area. Text is marked as [`KeymapTest`] reads it: in
//! normal mode `ˇ` stands before the character under the block caret, and
//! a visual selection is written `«…»`, which holds the character under
//! the caret.

mod editing;
mod ex;
mod insert;
mod motions;
mod objects;
mod operators;
mod registers;
mod repeat;
mod schemes;
mod visual;

use gpui::TestAppContext;

use super::super::{Keymap, KeymapPlatform, test::KeymapTest};
use super::VimMode;
use crate::input::{CursorShape, VimState};

/// A textarea in Vim's normal mode with Linux's tables.
fn vim(cx: &mut TestAppContext, marked: &str) -> KeymapTest {
    KeymapTest::new(cx, Keymap::Vim, KeymapPlatform::Linux, marked)
}

/// A textarea in Vim's normal mode with `platform`'s tables.
fn vim_on(cx: &mut TestAppContext, platform: KeymapPlatform, marked: &str) -> KeymapTest {
    KeymapTest::new(cx, Keymap::Vim, platform, marked)
}

trait VimTest {
    fn mode(&self) -> VimMode;
    #[track_caller]
    fn assert_mode(&self, mode: VimMode);
    fn shape(&self) -> CursorShape;
    fn label(&self) -> String;
}

impl VimTest for KeymapTest {
    fn mode(&self) -> VimMode {
        self.textarea.read_with(&self.cx, |state, _| {
            state
                .keymap_state::<VimState>()
                .map(VimState::mode)
                .expect("Vim is the textarea's scheme")
        })
    }

    #[track_caller]
    fn assert_mode(&self, mode: VimMode) {
        assert_eq!(self.mode(), mode);
    }

    fn shape(&self) -> CursorShape {
        self.cursor_shape()
    }

    fn label(&self) -> String {
        self.mode_label()
            .map(|label| label.to_string())
            .unwrap_or_default()
    }
}
