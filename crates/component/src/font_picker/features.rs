//! The OpenType features a font picker lets the user switch on and off.

use gpui::SharedString;
use rust_i18n::t;

/// An optional OpenType feature a font offers, such as ligatures, a slashed
/// zero or a stylistic set.
///
/// Fonts list many features that shaping applies by itself, such as `ccmp`
/// or `mark`. A picker offers only the ones a reader chooses between, which
/// [`FontFeature::optional`] recognizes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFeature {
    tag: SharedString,
    /// The name the font gives a stylistic set or character variant.
    name: Option<SharedString>,
}

/// Features that are on unless turned off.
const ON_BY_DEFAULT: [&str; 3] = ["calt", "kern", "liga"];

/// The optional features with a fixed place in the list, in that order.
/// Stylistic sets and character variants follow them.
const NAMED: [&str; 16] = [
    "liga", "calt", "dlig", "kern", "zero", "tnum", "pnum", "onum", "lnum", "case", "frac", "smcp",
    "c2sc", "salt", "swsh", "ordn",
];

impl FontFeature {
    /// The feature with `tag`, when it is one a reader chooses: one of the
    /// common optional features, a stylistic set (`ss01` … `ss20`) or a
    /// character variant (`cv01` … `cv99`).
    pub fn optional(tag: &str) -> Option<Self> {
        let known = NAMED.contains(&tag)
            || numbered(tag, "ss").is_some_and(|n| (1..=20).contains(&n))
            || numbered(tag, "cv").is_some_and(|n| (1..=99).contains(&n));
        known.then(|| Self {
            tag: SharedString::from(tag.to_string()),
            name: None,
        })
    }

    pub(super) fn with_name(mut self, name: Option<SharedString>) -> Self {
        self.name = name;
        self
    }

    /// The four-letter OpenType tag, such as `liga`.
    pub fn tag(&self) -> &SharedString {
        &self.tag
    }

    /// Whether text uses the feature when the user has not chosen: true for
    /// ligatures, contextual alternates and kerning.
    pub fn is_on_by_default(&self) -> bool {
        ON_BY_DEFAULT.contains(&self.tag.as_ref())
    }

    /// The name the font gives the feature, for stylistic sets and
    /// character variants that have one, such as "Slashed zero".
    pub fn name(&self) -> Option<&SharedString> {
        self.name.as_ref()
    }

    /// What a picker calls the feature: the font's name for it when it has
    /// one, otherwise a name in the interface language, such as "Ligatures"
    /// or "Stylistic set 1".
    pub fn label(&self) -> SharedString {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let tag = self.tag.as_ref();
        if let Some(n) = numbered(tag, "ss") {
            return t!("FontPicker.feature.ss", n = n).into();
        }
        if let Some(n) = numbered(tag, "cv") {
            return t!("FontPicker.feature.cv", n = n).into();
        }
        match tag {
            "liga" => t!("FontPicker.feature.liga"),
            "calt" => t!("FontPicker.feature.calt"),
            "dlig" => t!("FontPicker.feature.dlig"),
            "kern" => t!("FontPicker.feature.kern"),
            "zero" => t!("FontPicker.feature.zero"),
            "tnum" => t!("FontPicker.feature.tnum"),
            "pnum" => t!("FontPicker.feature.pnum"),
            "onum" => t!("FontPicker.feature.onum"),
            "lnum" => t!("FontPicker.feature.lnum"),
            "case" => t!("FontPicker.feature.case"),
            "frac" => t!("FontPicker.feature.frac"),
            "smcp" => t!("FontPicker.feature.smcp"),
            "c2sc" => t!("FontPicker.feature.c2sc"),
            "salt" => t!("FontPicker.feature.salt"),
            "swsh" => t!("FontPicker.feature.swsh"),
            "ordn" => t!("FontPicker.feature.ordn"),
            _ => return self.tag.clone(),
        }
        .into()
    }

    /// Where the feature goes in a list: the named features first, then the
    /// stylistic sets and the character variants.
    pub(super) fn order(&self) -> (u8, u8) {
        let tag = self.tag.as_ref();
        if let Some(ix) = NAMED.iter().position(|named| *named == tag) {
            (0, ix as u8)
        } else if let Some(n) = numbered(tag, "ss") {
            (1, n)
        } else {
            (2, numbered(tag, "cv").unwrap_or(u8::MAX))
        }
    }
}

/// The number in a tag such as `ss01`, when it starts with `prefix`.
fn numbered(tag: &str, prefix: &str) -> Option<u8> {
    let digits = tag.strip_prefix(prefix)?;
    (digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_features_a_reader_chooses_are_optional() {
        for tag in ["liga", "calt", "zero", "ss01", "ss20", "cv11"] {
            assert!(FontFeature::optional(tag).is_some(), "{tag}");
        }
        for tag in [
            "ccmp", "mark", "mkmk", "locl", "ss21", "ss1", "cv00", "rlig",
        ] {
            assert!(FontFeature::optional(tag).is_none(), "{tag}");
        }
    }

    #[test]
    fn ligatures_are_on_by_default() {
        let feature = |tag| FontFeature::optional(tag).unwrap();
        assert!(feature("liga").is_on_by_default());
        assert!(feature("calt").is_on_by_default());
        assert!(!feature("zero").is_on_by_default());
        assert!(!feature("ss01").is_on_by_default());
    }

    #[test]
    fn named_features_come_before_sets_and_variants() {
        let mut features: Vec<FontFeature> = ["cv02", "ss02", "zero", "ss01", "liga"]
            .into_iter()
            .filter_map(FontFeature::optional)
            .collect();
        features.sort_by_key(FontFeature::order);
        let tags: Vec<&str> = features.iter().map(|f| f.tag().as_ref()).collect();
        assert_eq!(tags, ["liga", "zero", "ss01", "ss02", "cv02"]);
    }
}
