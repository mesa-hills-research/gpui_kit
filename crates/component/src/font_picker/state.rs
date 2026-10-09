//! The state of a font picker: the catalog, the chosen font and the controls
//! that change it.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement,
    ParentElement as _, SharedString, Styled as _, Subscription, Task, Window,
    prelude::FluentBuilder as _, px,
};
use rust_i18n::t;

use super::{FontCatalog, FontFamily, FontSettings, settings};
use crate::{
    ActiveTheme as _, IndexPath, h_flex,
    input::{InputEvent, InputState},
    list::{ListDelegate, ListEvent, ListItem, ListState},
    select::{SelectEvent, SelectItem, SelectState},
};

/// What a [`FontPickerState`] reports.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FontPickerEvent {
    /// The user changed the font. Carries the new settings.
    Change(FontSettings),
}

/// A font picker's state: the families on offer, the chosen font and the
/// controls that change it. Render it with [`FontPicker`](super::FontPicker).
///
/// It lists the system's fonts, loaded in the background when the state is
/// created, unless [`Self::catalog`] gives it a catalog to show instead. Each
/// change the user makes emits [`FontPickerEvent::Change`].
///
/// ```ignore
/// let picker = cx.new(|cx| {
///     FontPickerState::new(window, cx)
///         .default_settings(FontSettings::new("JetBrains Mono").with_size(px(14.)))
///         .monospace_only(true)
/// });
/// cx.subscribe(&picker, |this, _, FontPickerEvent::Change(font), cx| {
///     this.editor_font = font.clone();
///     cx.notify();
/// });
/// ```
pub struct FontPickerState {
    catalog: Option<FontCatalog>,
    /// Fonts to read into the catalog once it is loaded, and whether the
    /// user added them.
    pending: Vec<(Cow<'static, [u8]>, bool)>,
    /// A family chosen while the catalog was still loading.
    pending_family: Option<SharedString>,
    settings: FontSettings,
    monospace_only: bool,
    pub(super) families: Entity<ListState<FamilyList>>,
    pub(super) weights: Entity<SelectState<Vec<WeightItem>>>,
    pub(super) size: Entity<InputState>,
    pub(super) line_height: Entity<InputState>,
    _load: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FontPickerEvent> for FontPickerState {}

impl FontPickerState {
    /// A picker with the default [`FontSettings`] that lists the system's
    /// fonts once they are read.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = FontSettings::default();
        let families =
            cx.new(|cx| ListState::new(FamilyList::default(), window, cx).searchable(true));
        let weights = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let size = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format_number(f32::from(settings.size())))
                .step(1.)
                .min(settings::FONT_SIZE_RANGE.0 as f64)
                .max(settings::FONT_SIZE_RANGE.1 as f64)
        });
        let line_height = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format_number(settings.line_height()))
                .step(0.1)
                .min(settings::LINE_HEIGHT_RANGE.0 as f64)
                .max(settings::LINE_HEIGHT_RANGE.1 as f64)
        });

        let subscriptions = vec![
            cx.subscribe_in(&families, window, |this, families, event, window, cx| {
                let (ListEvent::Select(ix) | ListEvent::Confirm(ix)) = event else {
                    return;
                };
                let family = families.read(cx).delegate().family(ix.row).cloned();
                if let Some(family) = family {
                    this.choose_family(&family, window, cx);
                }
            }),
            cx.subscribe_in(&weights, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(weight)) = event else {
                    return;
                };
                let settings = this
                    .settings
                    .clone()
                    .with_weight(FontWeight(*weight as f32));
                this.change(settings, window, cx);
            }),
            cx.subscribe_in(&size, window, |this, size, event, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // A size being typed, such as the 1 of 14, waits until it is
                // in range: the input clamps it when it loses focus.
                let (min, max) = settings::FONT_SIZE_RANGE;
                if let Ok(value) = size.read(cx).value().trim().parse::<f32>()
                    && (min..=max).contains(&value)
                {
                    let settings = this.settings.clone().with_size(px(value));
                    this.change(settings, window, cx);
                }
            }),
            cx.subscribe_in(
                &line_height,
                window,
                |this, line_height, event, window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let (min, max) = settings::LINE_HEIGHT_RANGE;
                    if let Ok(value) = line_height.read(cx).value().trim().parse::<f32>()
                        && (min..=max).contains(&value)
                    {
                        let settings = this.settings.clone().with_line_height(value);
                        this.change(settings, window, cx);
                    }
                },
            ),
        ];

        // Builders run after `new`, so the controls take their values, and
        // the system's fonts start loading, once they are done.
        cx.defer_in(window, |this, window, cx| this.start(window, cx));

        Self {
            catalog: None,
            pending: Vec::new(),
            pending_family: None,
            settings,
            monospace_only: false,
            families,
            weights,
            size,
            line_height,
            _load: Task::ready(()),
            _subscriptions: subscriptions,
        }
    }

    /// Show `catalog` instead of the system's fonts.
    pub fn catalog(mut self, catalog: FontCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Start from `settings`.
    pub fn default_settings(mut self, settings: FontSettings) -> Self {
        self.settings = settings;
        self
    }

    /// List only monospaced families. Off by default.
    pub fn monospace_only(mut self, monospace_only: bool) -> Self {
        self.monospace_only = monospace_only;
        self
    }

    /// Fonts the application registered with the text system itself, such
    /// as ones it bundles. The picker reads their weights and features from
    /// this data, since the installed font files don't have them.
    pub fn app_fonts(mut self, fonts: Vec<Cow<'static, [u8]>>) -> Self {
        self.pending
            .extend(fonts.into_iter().map(|font| (font, false)));
        self
    }

    /// Fonts the user added earlier, which the application registered with
    /// the text system again at start-up. They are listed as added.
    pub fn added_fonts(mut self, fonts: Vec<Cow<'static, [u8]>>) -> Self {
        self.pending
            .extend(fonts.into_iter().map(|font| (font, true)));
        self
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let monospace_only = self.monospace_only;
        self.families.update(cx, |families, _| {
            families.delegate_mut().monospace_only = monospace_only;
        });
        match self.catalog.clone() {
            Some(catalog) => self.set_catalog(catalog, window, cx),
            None => {
                self._load = cx.spawn_in(window, async move |this, cx| {
                    let Ok(load) = cx.update(|_, cx| FontCatalog::load(cx)) else {
                        return;
                    };
                    let catalog = load.await;
                    this.update_in(cx, |this, window, cx| this.set_catalog(catalog, window, cx))
                        .ok();
                });
                self.sync_controls(window, cx);
            }
        }
    }

    /// Show only monospaced families, or every family.
    pub fn set_monospace_only(&mut self, monospace_only: bool, cx: &mut Context<Self>) {
        self.monospace_only = monospace_only;
        self.families.update(cx, |families, cx| {
            families.delegate_mut().monospace_only = monospace_only;
            families.delegate_mut().refilter();
            cx.notify();
        });
        cx.notify();
    }

    /// Whether only monospaced families are listed.
    pub fn is_monospace_only(&self) -> bool {
        self.monospace_only
    }

    /// The chosen font.
    pub fn settings(&self) -> &FontSettings {
        &self.settings
    }

    /// The catalog on offer, once it is read.
    pub fn font_catalog(&self) -> Option<&FontCatalog> {
        self.catalog.as_ref()
    }

    /// Whether the system's fonts are still being read.
    pub fn is_loading(&self) -> bool {
        self.catalog.is_none()
    }

    /// The family of the chosen font, when the catalog has it.
    pub fn family(&self) -> Option<&FontFamily> {
        self.catalog.as_ref()?.family(self.settings.family())
    }

    /// Choose a font from code. Emits no event.
    pub fn set_settings(
        &mut self,
        settings: FontSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings;
        self.sync_controls(window, cx);
        cx.notify();
    }

    /// Register fonts with the text system and add them to the catalog, for
    /// font files the user chose. Each item is a font file's data.
    ///
    /// Returns the names of the families the data holds. Choosing one is up
    /// to the caller, with [`Self::choose_family_named`].
    pub fn add_fonts(
        &mut self,
        fonts: Vec<Cow<'static, [u8]>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Vec<SharedString>> {
        cx.text_system().add_fonts(fonts.clone())?;
        let names = match &mut self.catalog {
            Some(catalog) => catalog.add_fonts(&fonts),
            None => {
                let names = FontCatalog::from_fonts(&fonts)
                    .families()
                    .iter()
                    .map(|family| family.name().clone())
                    .collect();
                self.pending
                    .extend(fonts.into_iter().map(|font| (font, true)));
                names
            }
        };
        if let Some(catalog) = self.catalog.clone() {
            self.set_catalog(catalog, window, cx);
        }
        Ok(names)
    }

    /// Choose the family named `name`, as clicking it in the list does, and
    /// emit [`FontPickerEvent::Change`]. Does nothing when the catalog lacks
    /// it. While the catalog is loading, the choice waits for it.
    pub fn choose_family_named(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = &self.catalog else {
            self.pending_family = Some(SharedString::from(name.to_string()));
            return;
        };
        if let Some(family) = catalog.family(name).cloned() {
            // Search for it, so the list shows it whatever was searched.
            self.families.update(cx, |families, cx| {
                families.set_query(family.name(), window, cx)
            });
            self.choose_family(&family, window, cx);
            self.sync_family_row(window, cx);
        }
    }

    /// Turn italic on or off, when the family has an italic face.
    pub(super) fn set_italic(&mut self, italic: bool, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone().with_italic(italic);
        self.change(settings, window, cx);
    }

    /// Turn the OpenType feature `tag` on or off.
    pub(super) fn set_feature(
        &mut self,
        tag: SharedString,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone().with_feature(tag, on);
        self.change(settings, window, cx);
    }

    fn choose_family(&mut self, family: &FontFamily, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.for_family(family);
        self.change(settings, window, cx);
    }

    /// Apply a change the user made, and report it.
    fn change(&mut self, settings: FontSettings, window: &mut Window, cx: &mut Context<Self>) {
        if settings == self.settings {
            return;
        }
        let family_changed = settings.family() != self.settings.family();
        self.settings = settings;
        if family_changed {
            self.sync_weights(window, cx);
        }
        cx.emit(FontPickerEvent::Change(self.settings.clone()));
        cx.notify();
    }

    fn set_catalog(
        &mut self,
        mut catalog: FontCatalog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (font, added) in std::mem::take(&mut self.pending) {
            if added {
                catalog.add_fonts(&[font]);
            } else {
                catalog.include_fonts(&[font]);
            }
        }
        self.families.update(cx, |families, cx| {
            families.delegate_mut().catalog = Some(catalog.clone());
            families.delegate_mut().refilter();
            cx.notify();
        });
        self.catalog = Some(catalog);
        self.sync_controls(window, cx);
        if let Some(family) = self.pending_family.take() {
            self.choose_family_named(&family, window, cx);
        }
        cx.notify();
    }

    /// Show the settings in the controls.
    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = format_number(f32::from(self.settings.size()));
        let line_height = format_number(self.settings.line_height());
        self.size.update(cx, |input, cx| {
            if input.value().trim().parse::<f32>().ok() != size.parse().ok() {
                input.set_value(size, window, cx);
            }
        });
        self.line_height.update(cx, |input, cx| {
            if input.value().trim().parse::<f32>().ok() != line_height.parse().ok() {
                input.set_value(line_height, window, cx);
            }
        });
        self.sync_weights(window, cx);
        self.sync_family_row(window, cx);
    }

    /// Select the chosen family's row in the list and scroll to it.
    fn sync_family_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let family = self.settings.family().clone();
        self.families.update(cx, |families, cx| {
            let row = families.delegate().row_of(&family);
            families.delegate_mut().selected = row.map(|row| IndexPath::default().row(row));
            if let Some(row) = row {
                families.set_selected_index(Some(IndexPath::default().row(row)), window, cx);
                families.scroll_to_selected_item(window, cx);
            }
            cx.notify();
        });
    }

    /// Offer the chosen family's weights.
    fn sync_weights(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weights: Vec<WeightItem> = self
            .family()
            .map(FontFamily::weights)
            .unwrap_or_else(|| vec![self.settings.weight()])
            .into_iter()
            .map(WeightItem::new)
            .collect();
        let selected = self.settings.weight().0 as u16;
        self.weights.update(cx, |select, cx| {
            select.set_items(weights, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
    }
}

/// `14` for whole numbers, `1.5` otherwise.
fn format_number(value: f32) -> String {
    let rounded = (value * 100.).round() / 100.;
    if rounded.fract() == 0. {
        format!("{rounded:.0}")
    } else {
        format!("{rounded}")
    }
}

/// A weight in the weight menu: "Regular", "Bold", or the number for a
/// weight between the named ones.
#[derive(Clone, Debug)]
pub(super) struct WeightItem {
    weight: u16,
    title: SharedString,
}

impl WeightItem {
    fn new(weight: FontWeight) -> Self {
        let weight = weight.0.round() as u16;
        Self {
            weight,
            title: weight_name(weight),
        }
    }
}

impl SelectItem for WeightItem {
    type Value = u16;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.weight
    }
}

/// The common name of a weight, such as "Semibold" for 600.
pub(super) fn weight_name(weight: u16) -> SharedString {
    match weight {
        100 => t!("FontPicker.weight.thin").into(),
        200 => t!("FontPicker.weight.extra_light").into(),
        300 => t!("FontPicker.weight.light").into(),
        400 => t!("FontPicker.weight.regular").into(),
        500 => t!("FontPicker.weight.medium").into(),
        600 => t!("FontPicker.weight.semibold").into(),
        700 => t!("FontPicker.weight.bold").into(),
        800 => t!("FontPicker.weight.extra_bold").into(),
        900 => t!("FontPicker.weight.black").into(),
        other => other.to_string().into(),
    }
}

/// The family list: the catalog's families that match the search and the
/// monospace filter.
#[derive(Default)]
pub(super) struct FamilyList {
    catalog: Option<FontCatalog>,
    query: String,
    monospace_only: bool,
    /// Indices into the catalog's families.
    matches: Vec<usize>,
    selected: Option<IndexPath>,
}

impl FamilyList {
    fn refilter(&mut self) {
        let Some(catalog) = &self.catalog else {
            self.matches.clear();
            return;
        };
        let query = self.query.trim().to_lowercase();
        self.matches = catalog
            .families()
            .iter()
            .enumerate()
            .filter(|(_, family)| {
                (!self.monospace_only || family.is_monospace())
                    && (query.is_empty() || family.name().to_lowercase().contains(&query))
            })
            .map(|(ix, _)| ix)
            .collect();
    }

    fn family(&self, row: usize) -> Option<&FontFamily> {
        let ix = *self.matches.get(row)?;
        self.catalog.as_ref()?.families().get(ix)
    }

    fn row_of(&self, name: &str) -> Option<usize> {
        let catalog = self.catalog.as_ref()?;
        self.matches.iter().position(|ix| {
            catalog
                .families()
                .get(*ix)
                .is_some_and(|family| family.name().eq_ignore_ascii_case(name))
        })
    }
}

impl ListDelegate for FamilyList {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.to_string();
        self.refilter();
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.matches.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let family = self.family(ix.row)?;
        let added = family.is_added();
        let muted = cx.theme().muted_foreground;
        Some(
            ListItem::new(ix).selected(self.selected == Some(ix)).child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .child(family.name().clone())
                    .when(added, |this| {
                        this.child(
                            gpui::div()
                                .text_xs()
                                .text_color(muted)
                                .child(t!("FontPicker.added").to_string()),
                        )
                    }),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(t!("FontPicker.no_match").to_string())
    }

    fn loading(&self, _: &App) -> bool {
        self.catalog.is_none()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }
}
