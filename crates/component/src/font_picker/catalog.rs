//! The fonts a [`FontPicker`](super::FontPicker) offers: the families the
//! text system can draw, with the weights, styles and OpenType features their
//! files provide.

use std::{collections::BTreeMap, ops::RangeInclusive, sync::Arc};

use gpui::{App, FontWeight, SharedString, Task};

use super::features::FontFeature;

/// One face of a font family: a weight in a style.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    weight: FontWeight,
    italic: bool,
    monospace: bool,
    /// The weights a variable face spans along its `wght` axis.
    weight_range: Option<(f32, f32)>,
    /// The optional OpenType features the face's `GSUB` and `GPOS` tables
    /// list.
    features: Vec<FontFeature>,
}

impl FontFace {
    /// The face's weight. For a variable face, its default weight.
    pub fn weight(&self) -> FontWeight {
        self.weight
    }

    /// Whether the face is italic or oblique.
    pub fn is_italic(&self) -> bool {
        self.italic
    }

    /// Whether every character in the face is as wide as the others.
    pub fn is_monospace(&self) -> bool {
        self.monospace
    }

    /// The weights a variable face can be drawn at, along its `wght` axis.
    pub fn weight_range(&self) -> Option<RangeInclusive<FontWeight>> {
        self.weight_range
            .map(|(min, max)| FontWeight(min)..=FontWeight(max))
    }

    /// The optional OpenType features the face offers, such as ligatures
    /// and stylistic sets, with the names the font gives them.
    pub fn features(&self) -> &[FontFeature] {
        &self.features
    }

    /// Reads a face from a font file's data. `None` when the data is not a
    /// font this parser reads.
    fn read(data: &[u8], index: u32) -> Option<Self> {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let weight_range = face
            .variation_axes()
            .into_iter()
            .find(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wght"))
            .map(|axis| (axis.min_value, axis.max_value));
        let names = feature_names(&face);
        let mut features: Vec<FontFeature> = Vec::new();
        for table in [face.tables().gsub, face.tables().gpos]
            .into_iter()
            .flatten()
        {
            for feature in table.features {
                let tag = feature.tag.to_string();
                let Some(feature) = FontFeature::optional(&tag) else {
                    continue;
                };
                if !features.iter().any(|known| known.tag() == feature.tag()) {
                    let name = names.iter().find(|(named, _)| *named == tag);
                    features.push(feature.with_name(name.map(|(_, name)| name.clone())));
                }
            }
        }
        features.sort_by_key(FontFeature::order);
        Some(Self {
            weight: FontWeight(face.weight().to_number() as f32),
            italic: face.is_italic() || face.is_oblique(),
            monospace: face.is_monospaced() || has_even_advances(&face),
            weight_range,
            features,
        })
    }
}

/// The names a font gives its stylistic sets and character variants, by
/// feature tag, from the feature parameters in its `GSUB` table.
fn feature_names(face: &ttf_parser::Face) -> Vec<(String, SharedString)> {
    let Some(gsub) = face.raw_face().table(ttf_parser::Tag::from_bytes(b"GSUB")) else {
        return Vec::new();
    };
    let u16_at = |offset: usize| {
        gsub.get(offset..offset + 2)
            .map(|bytes| usize::from(u16::from_be_bytes([bytes[0], bytes[1]])))
    };
    // The header's FeatureList offset, then its feature records: a tag and
    // the offset of the feature table, whose first field is the offset of
    // its parameters. Both kinds of parameters keep a name ID at byte 2.
    let Some(list) = u16_at(6).filter(|offset| *offset != 0) else {
        return Vec::new();
    };
    let count = u16_at(list).unwrap_or(0);
    let mut names = Vec::new();
    for ix in 0..count {
        let record = list + 2 + ix * 6;
        let Some(tag) = gsub.get(record..record + 4) else {
            break;
        };
        if !(tag.starts_with(b"ss") || tag.starts_with(b"cv")) {
            continue;
        }
        let tag = String::from_utf8_lossy(tag).into_owned();
        let name = u16_at(record + 4)
            .map(|offset| list + offset)
            .and_then(|feature| {
                let params = u16_at(feature).filter(|offset| *offset != 0)?;
                u16_at(feature + params + 2)
            })
            .and_then(|id| font_name(face, id as u16));
        if let Some(name) = name {
            names.push((tag, name));
        }
    }
    names
}

/// The font's name with `id` in its `name` table, in English when it has
/// several.
fn font_name(face: &ttf_parser::Face, id: u16) -> Option<SharedString> {
    // IDs below 256 are the font's own names, never a feature's.
    if id < 256 {
        return None;
    }
    let mut found = None;
    for name in face.names() {
        if name.name_id != id {
            continue;
        }
        let Some(text) = name.to_string() else {
            continue;
        };
        if name.language_id == 0x0409 {
            return Some(text.into());
        }
        found.get_or_insert(text);
    }
    found.map(SharedString::from)
}

/// Whether narrow and wide letters advance the same distance, for monospace
/// fonts that leave their `post` table's fixed-pitch flag unset.
fn has_even_advances(face: &ttf_parser::Face) -> bool {
    let advance = |c| {
        face.glyph_index(c)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
    };
    match (advance('i'), advance('M'), advance('W'), advance('.')) {
        (Some(i), Some(m), Some(w), Some(dot)) => i == m && m == w && w == dot,
        _ => false,
    }
}

/// A font family: its name and the faces it has.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFamily {
    name: SharedString,
    faces: Vec<FontFace>,
    added: bool,
}

/// The weights offered for a family whose files could not be read: nearly
/// every family has these two, and the text system picks the nearest face.
const FALLBACK_WEIGHTS: [FontWeight; 2] = [FontWeight::NORMAL, FontWeight::BOLD];

impl FontFamily {
    /// The family's name, as the text system knows it.
    pub fn name(&self) -> &SharedString {
        &self.name
    }

    /// The family's faces. Empty when the text system lists the family but
    /// its files could not be read.
    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    /// Whether the family's fonts were added at run time, with
    /// [`FontCatalog::add_fonts`], rather than installed.
    pub fn is_added(&self) -> bool {
        self.added
    }

    /// Whether every face of the family is monospaced.
    pub fn is_monospace(&self) -> bool {
        !self.faces.is_empty() && self.faces.iter().all(FontFace::is_monospace)
    }

    /// Whether the family has an italic or oblique face.
    pub fn has_italic(&self) -> bool {
        self.faces.iter().any(FontFace::is_italic)
    }

    /// The weights the family offers, lightest first: each face's weight, and
    /// for a variable face the standard weights (100, 200 … 900) its `wght`
    /// axis spans.
    pub fn weights(&self) -> Vec<FontWeight> {
        if self.faces.is_empty() {
            return FALLBACK_WEIGHTS.to_vec();
        }
        let mut weights: Vec<u16> = Vec::new();
        for face in &self.faces {
            weights.push(face.weight.0.round() as u16);
            if let Some((min, max)) = face.weight_range {
                weights.extend(
                    (1..=9)
                        .map(|step| step * 100)
                        .filter(|weight| (min..=max).contains(&(*weight as f32))),
                );
            }
        }
        weights.sort_unstable();
        weights.dedup();
        weights
            .into_iter()
            .map(|weight| FontWeight(weight as f32))
            .collect()
    }

    /// The weight among [`Self::weights`] nearest to `weight`.
    pub fn nearest_weight(&self, weight: FontWeight) -> FontWeight {
        self.weights()
            .into_iter()
            .min_by(|a, b| (a.0 - weight.0).abs().total_cmp(&(b.0 - weight.0).abs()))
            .unwrap_or(FontWeight::NORMAL)
    }

    /// The optional OpenType features the family's faces offer, such as
    /// ligatures and stylistic sets, in a stable order.
    pub fn features(&self) -> Vec<FontFeature> {
        let mut features: Vec<FontFeature> = self
            .faces
            .iter()
            .flat_map(|face| face.features.iter().cloned())
            .collect();
        // One of each, keeping a name a face gives it.
        features.sort_by_key(|feature| (feature.order(), feature.name().is_none()));
        features.dedup_by(|a, b| a.tag() == b.tag());
        features
    }
}

/// The font families a picker offers, sorted by name.
///
/// [`FontCatalog::load`] lists the families the platform's text system can
/// draw and reads their faces from the installed font files.
/// [`FontCatalog::from_fonts`] reads them from font data instead, for fonts
/// an application bundles, or a fixed set in tests.
///
/// A catalog is cheap to clone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontCatalog {
    families: Arc<Vec<FontFamily>>,
}

impl FontCatalog {
    /// Lists the families the text system can draw, in the background.
    ///
    /// The names come from the text system, so they include fonts added with
    /// `add_fonts` before the call. The weights, styles and features come
    /// from the installed font files. A family whose files are not found
    /// there, such as one an application registered from its own data, is
    /// listed with no faces until [`Self::add_fonts`] reads them.
    pub fn load(cx: &App) -> Task<Self> {
        let names = cx.text_system().all_font_names();
        cx.background_executor()
            .spawn(async move { Self::with_system_faces(names) })
    }

    #[cfg(not(target_family = "wasm"))]
    fn with_system_faces(names: Vec<String>) -> Self {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let faces = faces_by_family(&database);
        Self::from_names(names, faces)
    }

    #[cfg(target_family = "wasm")]
    fn with_system_faces(names: Vec<String>) -> Self {
        Self::from_names(names, BTreeMap::new())
    }

    /// A catalog of the families in font files' data, such as the fonts an
    /// application bundles, or a fixed set for tests.
    ///
    /// Each item is the data of a font file: a TrueType or OpenType font, or
    /// a collection of them. Data that is not a font is skipped.
    pub fn from_fonts<D: AsRef<[u8]>>(fonts: &[D]) -> Self {
        let mut catalog = Self::default();
        catalog.include_fonts(fonts);
        catalog
    }

    fn from_names(names: Vec<String>, mut faces: BTreeMap<String, Vec<FontFace>>) -> Self {
        let mut families: Vec<FontFamily> = names
            .into_iter()
            .map(|name| FontFamily {
                faces: faces.remove(&name).unwrap_or_default(),
                name: name.into(),
                added: false,
            })
            .collect();
        sort_families(&mut families);
        families.dedup_by(|a, b| a.name == b.name);
        Self {
            families: Arc::new(families),
        }
    }

    /// Reads the families in font files' data the user added and adds
    /// them, marked as added. A family the catalog already has gains the new
    /// faces.
    ///
    /// Returns the names of the families the data holds. This only updates
    /// the catalog: register the same data with the text system, through
    /// `cx.text_system().add_fonts`, to draw with it, or call
    /// [`FontPickerState::add_fonts`](super::FontPickerState::add_fonts),
    /// which does both.
    pub fn add_fonts<D: AsRef<[u8]>>(&mut self, fonts: &[D]) -> Vec<SharedString> {
        self.insert_fonts(fonts, true)
    }

    /// Reads the families in font files' data an application registered
    /// itself, such as fonts it bundles, so their faces are known. They are
    /// not marked as added.
    pub fn include_fonts<D: AsRef<[u8]>>(&mut self, fonts: &[D]) -> Vec<SharedString> {
        self.insert_fonts(fonts, false)
    }

    fn insert_fonts<D: AsRef<[u8]>>(&mut self, fonts: &[D], added: bool) -> Vec<SharedString> {
        let mut database = fontdb::Database::new();
        for font in fonts {
            database.load_font_data(font.as_ref().to_vec());
        }
        let found = faces_by_family(&database);
        let names: Vec<SharedString> = found.keys().cloned().map(SharedString::from).collect();

        let families = Arc::make_mut(&mut self.families);
        for (name, faces) in found {
            match families
                .iter_mut()
                .find(|family| family.name.as_ref() == name)
            {
                Some(family) => {
                    for face in faces {
                        if !family.faces.contains(&face) {
                            family.faces.push(face);
                        }
                    }
                    family.added |= added;
                }
                None => families.push(FontFamily {
                    name: name.into(),
                    faces,
                    added,
                }),
            }
        }
        sort_families(families);
        names
    }

    /// Every family, sorted by name.
    pub fn families(&self) -> &[FontFamily] {
        &self.families
    }

    /// The family named `name`, ignoring case.
    pub fn family(&self, name: &str) -> Option<&FontFamily> {
        self.families
            .iter()
            .find(|family| family.name.eq_ignore_ascii_case(name))
    }

    /// The families whose names contain `query`, ignoring case, and with
    /// `monospace_only` only the monospaced ones.
    pub fn search<'a>(
        &'a self,
        query: &'a str,
        monospace_only: bool,
    ) -> impl Iterator<Item = &'a FontFamily> + 'a {
        let query = query.trim().to_lowercase();
        self.families.iter().filter(move |family| {
            (!monospace_only || family.is_monospace())
                && (query.is_empty() || family.name.to_lowercase().contains(&query))
        })
    }
}

fn sort_families(families: &mut [FontFamily]) {
    families.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// The faces in `database`, by the English name of their family, with each
/// family's faces lightest first and upright before italic.
fn faces_by_family(database: &fontdb::Database) -> BTreeMap<String, Vec<FontFace>> {
    let mut families: BTreeMap<String, Vec<FontFace>> = BTreeMap::new();
    for info in database.faces() {
        let Some((family, _)) = info.families.first() else {
            continue;
        };
        let Some(face) = database
            .with_face_data(info.id, |data, index| FontFace::read(data, index))
            .flatten()
        else {
            continue;
        };
        let faces = families.entry(family.clone()).or_default();
        if !faces.contains(&face) {
            faces.push(face);
        }
    }
    for faces in families.values_mut() {
        faces.sort_by(|a, b| {
            a.italic
                .cmp(&b.italic)
                .then_with(|| a.weight.0.total_cmp(&b.weight.0))
        });
    }
    families
}
