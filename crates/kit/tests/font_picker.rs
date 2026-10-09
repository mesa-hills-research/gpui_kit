//! The font picker's model: the families read from font files, the monospace filter, the weights
//! and features a family offers, the settings an application saves, and the state's events.
//!
//! The fonts are the ones the web gallery bundles, under `crates/story-web/fonts`.

mod common;

use std::{borrow::Cow, cell::RefCell, rc::Rc};

use gpui::{
    AnyWindowHandle, AppContext as _, Context, Entity, FontStyle, FontWeight, IntoElement,
    ParentElement as _, Pixels, Render, ScrollDelta, Styled as _, TestAppContext, Window, div,
    point, px, size,
};
use gpui_component::{
    IndexPath,
    checkbox::Checkbox,
    font_picker::{
        FontCatalog, FontFamily, FontPicker, FontPickerEvent, FontPickerState, FontSettings,
    },
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use std::time::Duration;

const IBM_PLEX_SANS: &[u8] = include_bytes!("../../story-web/fonts/IBMPlexSans-Regular.ttf");
/// Inter's variable font, "Inter Variable", with a `wght` axis from 100 to 900.
const INTER: &[u8] = include_bytes!("../../story-web/fonts/Inter-Regular.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../../story-web/fonts/JetBrainsMono-Regular.ttf");
/// The same face before subsetting, which lists one more character variant.
const JETBRAINS_MONO_SOURCE: &[u8] =
    include_bytes!("../../story-web/fonts/JetBrainsMono-Regular.source.ttf");

fn catalog() -> FontCatalog {
    FontCatalog::from_fonts(&[JETBRAINS_MONO, IBM_PLEX_SANS, INTER])
}

fn names<'a>(families: impl IntoIterator<Item = &'a FontFamily>) -> Vec<String> {
    families
        .into_iter()
        .map(|family| family.name().to_string())
        .collect()
}

#[test]
fn families_are_read_from_font_data_and_sorted_by_name() {
    let catalog = catalog();
    assert_eq!(
        names(catalog.families()),
        ["IBM Plex Sans", "Inter Variable", "JetBrains Mono"]
    );
    assert!(!catalog.families().iter().any(|family| family.is_added()));
    assert!(catalog.family("jetbrains mono").is_some());
    assert!(catalog.family("Comic Sans").is_none());
}

#[test]
fn data_that_is_not_a_font_is_skipped() {
    let catalog = FontCatalog::from_fonts(&[b"not a font".as_slice(), JETBRAINS_MONO]);
    assert_eq!(names(catalog.families()), ["JetBrains Mono"]);
}

#[test]
fn search_filters_by_name_and_monospace() {
    let catalog = catalog();
    assert_eq!(names(catalog.search("", true)), ["JetBrains Mono"]);
    assert_eq!(names(catalog.search("PLEX", false)), ["IBM Plex Sans"]);
    assert_eq!(
        names(catalog.search("in", false)),
        ["Inter Variable", "JetBrains Mono"]
    );
    assert!(catalog.search("plex", true).next().is_none());
}

#[test]
fn weights_and_styles_come_from_the_faces() {
    let catalog = catalog();
    let mono = catalog.family("JetBrains Mono").unwrap();
    assert!(mono.is_monospace());
    assert!(!mono.has_italic());
    assert_eq!(mono.weights(), [FontWeight::NORMAL]);
    assert_eq!(mono.nearest_weight(FontWeight::BOLD), FontWeight::NORMAL);

    let plex = catalog.family("IBM Plex Sans").unwrap();
    assert!(!plex.is_monospace());
    assert_eq!(plex.faces().len(), 1);
    assert_eq!(plex.faces()[0].weight(), FontWeight::NORMAL);
}

#[test]
fn a_variable_font_offers_the_weights_along_its_axis() {
    let catalog = FontCatalog::from_fonts(&[INTER]);
    let family = catalog.family("Inter Variable").unwrap();
    let face = &family.faces()[0];
    assert_eq!(face.weight(), FontWeight::NORMAL);
    assert_eq!(
        face.weight_range(),
        Some(FontWeight::THIN..=FontWeight::BLACK)
    );
    let weights: Vec<f32> = family.weights().iter().map(|weight| weight.0).collect();
    assert_eq!(
        weights,
        [100., 200., 300., 400., 500., 600., 700., 800., 900.]
    );
    assert_eq!(family.nearest_weight(FontWeight(540.)), FontWeight::MEDIUM);
}

#[test]
fn features_are_the_optional_ones_the_font_lists() {
    let catalog = FontCatalog::from_fonts(&[JETBRAINS_MONO_SOURCE]);
    let family = catalog.family("JetBrains Mono").unwrap();
    let tags: Vec<String> = family
        .features()
        .iter()
        .map(|feature| feature.tag().to_string())
        .collect();
    assert!(tags.contains(&"calt".to_string()), "{tags:?}");
    assert!(tags.contains(&"zero".to_string()), "{tags:?}");
    assert!(tags.iter().any(|tag| tag.starts_with("ss")), "{tags:?}");
    // Features shaping applies by itself are not offered.
    assert!(
        !tags.iter().any(|tag| tag == "ccmp" || tag == "mark"),
        "{tags:?}"
    );

    let calt = family
        .features()
        .into_iter()
        .find(|f| f.tag() == "calt")
        .unwrap();
    assert!(calt.is_on_by_default());
    assert_eq!(calt.label(), "Contextual alternates");

    // The font names its stylistic sets, and not its character variants.
    let feature = |tag: &str| {
        family
            .features()
            .into_iter()
            .find(|feature| feature.tag() == tag)
            .unwrap()
    };
    assert_eq!(feature("ss01").label(), "Classic construction");
    assert!(!feature("ss01").is_on_by_default());
    assert_eq!(feature("cv01").name(), None);
    assert_eq!(feature("cv01").label(), "Character variant 1");
}

#[test]
fn adding_fonts_extends_a_family_and_reports_its_name() {
    let mut catalog = FontCatalog::from_fonts(&[IBM_PLEX_SANS]);
    assert_eq!(
        catalog.add_fonts(&[JETBRAINS_MONO_SOURCE]),
        ["JetBrains Mono"]
    );
    assert_eq!(catalog.add_fonts(&[JETBRAINS_MONO]), ["JetBrains Mono"]);
    assert_eq!(
        names(catalog.families()),
        ["IBM Plex Sans", "JetBrains Mono"]
    );
    assert_eq!(catalog.family("JetBrains Mono").unwrap().faces().len(), 2);
    assert!(catalog.family("JetBrains Mono").unwrap().is_added());
    assert!(!catalog.family("IBM Plex Sans").unwrap().is_added());
}

#[test]
fn settings_round_trip_through_json() {
    let settings = FontSettings::new("JetBrains Mono")
        .with_weight(FontWeight::MEDIUM)
        .with_italic(true)
        .with_size(px(15.))
        .with_line_height(1.6)
        .with_feature("calt", false)
        .with_feature("ss02", true);
    let json = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        json,
        r#"{"family":"JetBrains Mono","weight":500,"italic":true,"size":15.0,"line_height":1.6,"features":{"calt":false,"ss02":true}}"#
    );
    let loaded: FontSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, settings);

    let font = loaded.font();
    assert_eq!(font.family, "JetBrains Mono");
    assert_eq!(font.weight, FontWeight::MEDIUM);
    assert_eq!(font.style, FontStyle::Italic);
    assert_eq!(
        font.features.tag_value_list(),
        [("calt".to_string(), 0), ("ss02".to_string(), 1)]
    );
}

#[test]
fn settings_missing_fields_take_their_defaults() {
    let loaded: FontSettings = serde_json::from_str(r#"{"family":"Inter","size":13}"#).unwrap();
    assert_eq!(loaded, FontSettings::new("Inter").with_size(px(13.)));
    assert_eq!(loaded.weight(), FontWeight::NORMAL);
    assert_eq!(loaded.line_height(), 1.5);
}

#[test]
fn settings_stay_within_the_offered_ranges() {
    let settings = FontSettings::default()
        .with_size(px(500.))
        .with_line_height(0.2);
    assert_eq!(settings.size(), px(96.));
    assert_eq!(settings.line_height(), 1.);
}

#[test]
fn moving_to_another_family_keeps_what_it_offers() {
    let catalog = FontCatalog::from_fonts(&[JETBRAINS_MONO, IBM_PLEX_SANS]);
    let settings = FontSettings::new("JetBrains Mono")
        .with_weight(FontWeight::LIGHT)
        .with_italic(true)
        .with_feature("zero", true)
        .with_feature("calt", false);
    let plex = settings.for_family(catalog.family("IBM Plex Sans").unwrap());
    assert_eq!(plex.family(), "IBM Plex Sans");
    assert_eq!(plex.weight(), FontWeight::NORMAL);
    assert!(!plex.is_italic());
    // IBM Plex Sans has a slashed zero, and no contextual alternates.
    assert_eq!(plex.feature("zero"), Some(true));
    assert_eq!(plex.feature("calt"), None);
    assert_eq!(plex.size(), settings.size());
}

struct PickerView {
    picker: Entity<FontPickerState>,
}

impl Render for PickerView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        FontPicker::new(&self.picker)
    }
}

#[gpui::test]
fn choosing_a_family_reports_the_new_settings(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let events: Rc<RefCell<Vec<FontSettings>>> = Rc::default();
    let (window, view) = common::open_window(cx, Some(size(px(800.), px(600.))), {
        let events = events.clone();
        move |window, cx| {
            let picker = cx.new(|cx| {
                FontPickerState::new(window, cx)
                    .catalog(FontCatalog::from_fonts(&[JETBRAINS_MONO, INTER]))
                    .default_settings(FontSettings::new("Inter Variable").with_size(px(14.)))
            });
            cx.subscribe(&picker, move |_, event: &FontPickerEvent, _| {
                let FontPickerEvent::Change(settings) = event else {
                    return;
                };
                events.borrow_mut().push(settings.clone());
            })
            .detach();
            cx.new(|_| PickerView { picker })
        }
    });
    cx.run_until_parked();
    let picker = cx.update(|cx| view.read(cx).picker.clone());
    cx.update(|cx| {
        let picker = picker.read(cx);
        assert!(!picker.is_loading());
        assert_eq!(picker.family().unwrap().name(), "Inter Variable");
    });

    common::update_content(window, &view, cx, |view, window, cx| {
        view.picker.update(cx, |picker, cx| {
            picker.choose_family_named("JetBrains Mono", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].family(), "JetBrains Mono");
        assert_eq!(events[0].size(), px(14.));
    }

    common::update_content(window, &view, cx, |view, window, cx| {
        view.picker.update(cx, |picker, cx| {
            let added = picker
                .add_fonts(vec![Cow::Borrowed(IBM_PLEX_SANS)], window, cx)
                .unwrap();
            assert_eq!(added, ["IBM Plex Sans"]);
            let catalog = picker.font_catalog().unwrap();
            assert!(catalog.family("IBM Plex Sans").unwrap().is_added());
        });
    })
    .unwrap();
    // Adding fonts reports nothing: choosing one is up to the application.
    assert_eq!(events.borrow().len(), 1);
}

/// A page that scrolls, with the picker between a checkbox at the top and
/// filler that makes the page taller than the window.
struct ScrollingPage {
    picker: Entity<FontPickerState>,
    compact: bool,
}

impl Render for ScrollingPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let picker = FontPicker::new(&self.picker);
        div().size_full().overflow_y_scrollbar().child(
            v_flex()
                .p_4()
                .gap_4()
                .child(Checkbox::new("page-top").label("Top"))
                .child(if self.compact {
                    picker.compact()
                } else {
                    picker
                })
                .child(div().h(px(1200.)).flex_shrink_0()),
        )
    }
}

/// Opens a scrolling page with the picker on JetBrains Mono, whose source
/// face offers more features than fit in the full layout's features list.
/// Records the settings each change reports.
fn open_page(
    cx: &mut TestAppContext,
    compact: bool,
) -> (AnyWindowHandle, Rc<RefCell<Vec<FontSettings>>>) {
    cx.update(gpui_component::init);
    let events: Rc<RefCell<Vec<FontSettings>>> = Rc::default();
    let (window, _) = common::open_window(cx, Some(size(px(800.), px(600.))), {
        let events = events.clone();
        move |window, cx| {
            let picker = cx.new(|cx| {
                FontPickerState::new(window, cx)
                    .catalog(FontCatalog::from_fonts(&[
                        JETBRAINS_MONO_SOURCE,
                        IBM_PLEX_SANS,
                        INTER,
                    ]))
                    .default_settings(FontSettings::new("JetBrains Mono").with_size(px(14.)))
            });
            cx.subscribe(&picker, move |_, event: &FontPickerEvent, _| {
                let FontPickerEvent::Change(settings) = event else {
                    return;
                };
                events.borrow_mut().push(settings.clone());
            })
            .detach();
            cx.new(|_| ScrollingPage { picker, compact })
        }
    });
    cx.run_until_parked();
    (window.into(), events)
}

fn top(window: &Window, id: &'static str) -> Pixels {
    window.find(id).bounds().top()
}

fn wheel(dy: f32) -> ScrollDelta {
    ScrollDelta::Pixels(point(px(0.), px(dy)))
}

#[gpui_kit::test]
async fn the_wheel_over_the_features_scrolls_them_and_not_the_page(cx: &mut TestAppContext) {
    let (window, _) = open_page(cx, false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let page = top(window, "page-top");
        let feature = top(window, "font-picker-feature-calt");

        window.scroll("font-picker-feature-calt", wheel(-40.), cx);
        assert_eq!(top(window, "font-picker-feature-calt"), feature - px(40.));
        assert_eq!(top(window, "page-top"), page);

        // Over the rest of the page, the page scrolls.
        window.scroll("page-top", wheel(-40.), cx);
        assert_eq!(top(window, "page-top"), page - px(40.));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn arrow_keys_move_through_the_family_list(cx: &mut TestAppContext) {
    let (window, events) = open_page(cx, false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The families are sorted: IBM Plex Sans, Inter Variable, JetBrains Mono.
        window.click(IndexPath::default().row(0), cx);
        window.press("down", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let families: Vec<String> = events
        .borrow()
        .iter()
        .map(|settings| settings.family().to_string())
        .collect();
    assert_eq!(families, ["IBM Plex Sans", "Inter Variable"]);
}

#[gpui_kit::test]
async fn the_compact_picker_chooses_a_family_from_a_dropdown(cx: &mut TestAppContext) {
    let (window, events) = open_page(cx, true);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("font-picker-family").value(),
            Some("JetBrains Mono")
        );
        assert_eq!(window.find("font-picker-family").expanded(), Some(false));
        window.within("font-picker-family").click("input", cx);
        assert_eq!(window.find("font-picker-family").expanded(), Some(true));
        window.input("plex", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.wait_for(window, Duration::from_millis(500), |window, _| {
        window.find("font-picker-family").expanded() == Some(false)
            && window.find("font-picker-family").value() == Some("IBM Plex Sans")
    })
    .await;
    assert_eq!(events.borrow().last().unwrap().family(), "IBM Plex Sans");
}

#[gpui_kit::test]
async fn the_compact_picker_keeps_the_features_behind_a_disclosure(cx: &mut TestAppContext) {
    let (window, events) = open_page(cx, true);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("font-picker-feature-calt").is_none());

        window.click("font-picker-features-toggle", cx);
        assert_eq!(
            window.find("font-picker-feature-calt").checked(),
            Some(true)
        );
        window.click("font-picker-feature-calt", cx);
        assert_eq!(
            window.find("font-picker-feature-calt").checked(),
            Some(false)
        );

        window.click("font-picker-features-toggle", cx);
        assert!(window.try_find("font-picker-feature-calt").is_none());
    })
    .unwrap();
    assert_eq!(events.borrow().last().unwrap().feature("calt"), Some(false));
}
