//! Work Sans and JetBrains Mono, embedded as variable fonts.
//!
//! Each family is one variable font for upright text and one for italics,
//! covering every weight from 100 to 900 (JetBrains Mono: 100 to 800), so
//! medium, semibold and bold text use the real weights instead of a
//! synthesized or substituted face. Register them before the first window
//! opens, then name the families in the theme:
//!
//! ```rust,no_run
//! use gpui_kit_assets::fonts;
//!
//! gpui_platform::application().run(|cx| {
//!     cx.text_system()
//!         .add_fonts(fonts::all())
//!         .expect("the bundled fonts are valid");
//!     // Theme::update(cx, |theme| {
//!     //     theme.font_family = fonts::WORK_SANS_FAMILY.into();
//!     //     theme.mono_font_family = fonts::JETBRAINS_MONO_FAMILY.into();
//!     // });
//! });
//! ```
//!
//! Both fonts are licensed under the SIL Open Font License 1.1; the license
//! texts ship beside them in the crate's `fonts` folder. Keep them with an
//! application that redistributes the fonts. The four files add about
//! 1.3 MiB to a binary.

use std::borrow::Cow;

/// The family name inside [`WORK_SANS`] and [`WORK_SANS_ITALIC`].
pub const WORK_SANS_FAMILY: &str = "Work Sans";

/// The family name inside [`JETBRAINS_MONO`] and [`JETBRAINS_MONO_ITALIC`].
pub const JETBRAINS_MONO_FAMILY: &str = "JetBrains Mono";

/// Work Sans, upright, weights 100–900 (`WorkSans[wght].ttf` from Google
/// Fonts, Work Sans repository commit b35c810).
pub static WORK_SANS: &[u8] = include_bytes!("../fonts/work-sans/WorkSans-Variable.ttf");

/// Work Sans, italic, weights 100–900.
pub static WORK_SANS_ITALIC: &[u8] =
    include_bytes!("../fonts/work-sans/WorkSans-Italic-Variable.ttf");

/// JetBrains Mono 2.304, upright, weights 100–800.
pub static JETBRAINS_MONO: &[u8] =
    include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-Variable.ttf");

/// JetBrains Mono 2.304, italic, weights 100–800.
pub static JETBRAINS_MONO_ITALIC: &[u8] =
    include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-Italic-Variable.ttf");

/// All four fonts, borrowed, ready for `cx.text_system().add_fonts(..)`.
pub fn all() -> Vec<Cow<'static, [u8]>> {
    [
        WORK_SANS,
        WORK_SANS_ITALIC,
        JETBRAINS_MONO,
        JETBRAINS_MONO_ITALIC,
    ]
    .into_iter()
    .map(Cow::Borrowed)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `font` is a TrueType font with a variation (`fvar`) table.
    fn is_variable_truetype(font: &[u8]) -> bool {
        let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
        font.starts_with(&[0, 1, 0, 0])
            && (0..tables).any(|ix| &font[12 + 16 * ix..16 + 16 * ix] == b"fvar")
    }

    #[test]
    fn all_four_fonts_are_variable() {
        let fonts = all();
        assert_eq!(fonts.len(), 4);
        assert!(fonts.iter().all(|font| is_variable_truetype(font)));
    }
}
