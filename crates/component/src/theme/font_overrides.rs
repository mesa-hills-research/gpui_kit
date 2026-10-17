use gpui::{FontFeatures, Pixels, SharedString};

/// The application's own font choices, applied over every theme.
///
/// A theme file may name its fonts, and a theme that names none resets them
/// to the system's, so a font picked in an application's settings would be
/// lost at the next theme switch. Set the choices once with
/// [`Theme::set_font_overrides`](super::Theme::set_font_overrides) instead:
/// [`Theme::apply_config`](super::Theme::apply_config) puts them back over
/// each theme it applies, in the same update, so a theme switch, a light or
/// dark change and a reload of the themes folder all keep them, without a
/// second update from an observer.
///
/// Each field left unset follows the theme, and unset monospace font
/// features, which themes leave to the application, keep the font's own.
/// `font_size` is also the application's zoom: [`Root`](crate::Root) sets
/// the window's rem size from it.
///
/// ```ignore
/// Theme::update(cx, |theme| {
///     theme.set_font_overrides(
///         FontOverrides::new()
///             .with_font_family("Work Sans")
///             .with_font_size(px(16.) * zoom)
///             .with_mono_font_family("JetBrains Mono")
///             .with_mono_font_features(FontFeatures(Arc::new(vec![
///                 ("calt".into(), 0),
///                 ("liga".into(), 0),
///             ]))),
///     )
/// });
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontOverrides {
    font_family: Option<SharedString>,
    font_size: Option<Pixels>,
    mono_font_family: Option<SharedString>,
    mono_font_size: Option<Pixels>,
    mono_font_features: Option<FontFeatures>,
}

impl FontOverrides {
    /// No overrides: every font follows the theme.
    pub fn new() -> Self {
        Self::default()
    }

    /// The UI font family, e.g. "Work Sans". Name a family that is installed
    /// or registered with `add_fonts`.
    pub fn with_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// The base font size, which is also the application's zoom (the rem
    /// size).
    pub fn with_font_size(mut self, size: Pixels) -> Self {
        self.font_size = Some(size);
        self
    }

    /// The monospace font family, used for code and key caps.
    pub fn with_mono_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.mono_font_family = Some(family.into());
        self
    }

    /// The monospace font size.
    pub fn with_mono_font_size(mut self, size: Pixels) -> Self {
        self.mono_font_size = Some(size);
        self
    }

    /// The OpenType features of the monospace font, e.g. `calt` and `liga`
    /// set to 0 to show code without ligatures. They apply wherever the
    /// components set text in the monospace font: code blocks and inline code
    /// in text views, diffs, code editors and key caps.
    pub fn with_mono_font_features(mut self, features: FontFeatures) -> Self {
        self.mono_font_features = Some(features);
        self
    }

    /// The UI font family, when overridden.
    pub fn font_family(&self) -> Option<&SharedString> {
        self.font_family.as_ref()
    }

    /// The base font size, when overridden.
    pub fn font_size(&self) -> Option<Pixels> {
        self.font_size
    }

    /// The monospace font family, when overridden.
    pub fn mono_font_family(&self) -> Option<&SharedString> {
        self.mono_font_family.as_ref()
    }

    /// The monospace font size, when overridden.
    pub fn mono_font_size(&self) -> Option<Pixels> {
        self.mono_font_size
    }

    /// The monospace font's OpenType features, when overridden.
    pub fn mono_font_features(&self) -> Option<&FontFeatures> {
        self.mono_font_features.as_ref()
    }

    /// Whether every font follows the theme.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
