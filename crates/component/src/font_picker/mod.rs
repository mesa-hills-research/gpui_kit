//! A font picker: choose a font family from the installed fonts or font
//! files the user adds, then its weight, style, size, line height and
//! OpenType features.
//!
//! - [`FontCatalog`] lists the families, read from the font files.
//! - [`FontSettings`] is the choice, ready to save and to draw with.
//! - [`FontPickerState`] and [`FontPicker`] are the control.

mod catalog;
mod features;
mod picker;
mod settings;
mod state;

pub use catalog::{FontCatalog, FontFace, FontFamily};
pub use features::FontFeature;
pub use picker::FontPicker;
pub use settings::{FONT_SIZE_RANGE, FontSettings, LINE_HEIGHT_RANGE};
pub use state::{FontPickerEvent, FontPickerState};
