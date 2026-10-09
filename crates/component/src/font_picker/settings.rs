//! A font choice that applications save with their settings.

use std::{collections::BTreeMap, sync::Arc};

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, px};
use serde::{Deserialize, Serialize};

use super::catalog::FontFamily;

/// A font as a [`FontPicker`](super::FontPicker) chooses it: the family, its
/// weight and style, the size, the line height and the OpenType features.
///
/// It serializes to plain values, so an application can keep it in its
/// settings file. A missing field takes its default, so files written by an
/// older version still load:
///
/// ```json
/// { "family": "JetBrains Mono", "weight": 500, "size": 14.0,
///   "line_height": 1.5, "features": { "calt": false } }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSettings {
    family: SharedString,
    weight: u16,
    italic: bool,
    size: f32,
    line_height: f32,
    /// The features the user turned on or off. The others keep the font's
    /// own behaviour.
    features: BTreeMap<SharedString, bool>,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            family: ".SystemUIFont".into(),
            weight: 400,
            italic: false,
            size: 16.,
            line_height: 1.5,
            features: BTreeMap::new(),
        }
    }
}

/// The smallest and largest sizes, in pixels, a picker offers.
pub const FONT_SIZE_RANGE: (f32, f32) = (6., 96.);

/// The smallest and largest line heights, as multiples of the size.
pub const LINE_HEIGHT_RANGE: (f32, f32) = (1., 3.);

impl FontSettings {
    /// Settings for `family` at the regular weight, upright, with the
    /// default size and line height.
    pub fn new(family: impl Into<SharedString>) -> Self {
        Self {
            family: family.into(),
            ..Self::default()
        }
    }

    pub fn with_family(mut self, family: impl Into<SharedString>) -> Self {
        self.family = family.into();
        self
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight.0.round().clamp(1., 1000.) as u16;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// The size, kept within [`FONT_SIZE_RANGE`].
    pub fn with_size(mut self, size: Pixels) -> Self {
        self.size = f32::from(size).clamp(FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1);
        self
    }

    /// The line height as a multiple of the size, kept within
    /// [`LINE_HEIGHT_RANGE`].
    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height.clamp(LINE_HEIGHT_RANGE.0, LINE_HEIGHT_RANGE.1);
        self
    }

    /// Turn the OpenType feature `tag` on or off, such as `calt` or `ss01`.
    pub fn with_feature(mut self, tag: impl Into<SharedString>, on: bool) -> Self {
        self.features.insert(tag.into(), on);
        self
    }

    /// Leave the feature `tag` to the font's own behaviour.
    pub fn without_feature(mut self, tag: &str) -> Self {
        self.features.remove(tag);
        self
    }

    pub fn family(&self) -> &SharedString {
        &self.family
    }

    pub fn weight(&self) -> FontWeight {
        FontWeight(self.weight as f32)
    }

    pub fn is_italic(&self) -> bool {
        self.italic
    }

    pub fn size(&self) -> Pixels {
        px(self.size)
    }

    /// The line height as a multiple of the size.
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// Whether the user turned the feature `tag` on or off. `None` leaves it
    /// to the font.
    pub fn feature(&self, tag: &str) -> Option<bool> {
        self.features.get(tag).copied()
    }

    /// The features the user turned on or off, by tag.
    pub fn features(&self) -> impl Iterator<Item = (&SharedString, bool)> {
        self.features.iter().map(|(tag, on)| (tag, *on))
    }

    /// The font to draw with: family, weight, style and features. Set the
    /// size and line height on the element alongside it.
    pub fn font(&self) -> Font {
        Font {
            family: self.family.clone(),
            features: FontFeatures(Arc::new(
                self.features
                    .iter()
                    .map(|(tag, on)| (tag.to_string(), u32::from(*on)))
                    .collect(),
            )),
            fallbacks: None,
            weight: self.weight(),
            style: if self.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        }
    }

    /// These settings moved to `family`: the weight becomes the family's
    /// nearest, italic stays only if the family has it, and features the
    /// family lacks are dropped.
    pub fn for_family(&self, family: &FontFamily) -> Self {
        let offered = family.features();
        Self {
            family: family.name().clone(),
            weight: family.nearest_weight(self.weight()).0 as u16,
            italic: self.italic && family.has_italic(),
            size: self.size,
            line_height: self.line_height,
            features: self
                .features
                .iter()
                .filter(|(tag, _)| offered.iter().any(|feature| feature.tag() == *tag))
                .map(|(tag, on)| (tag.clone(), *on))
                .collect(),
        }
    }
}
