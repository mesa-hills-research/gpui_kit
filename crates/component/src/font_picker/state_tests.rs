use super::*;
use gpui::{TestAppContext, point};
use std::{cell::Cell, rc::Rc};

const MONO: &[u8] = include_bytes!("../../../story-web/fonts/JetBrainsMono-Regular.ttf");
const SANS: &[u8] = include_bytes!("../../../story-web/fonts/IBMPlexSans-Regular.ttf");
const INTER: &[u8] = include_bytes!("../../../story-web/fonts/Inter-Regular.ttf");

#[gpui::test]
fn replacing_a_catalog_keeps_search_selection_and_open_controls(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let cx = cx.add_empty_window();
    let events = Rc::new(Cell::new(0));
    cx.update(|window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .catalog(FontCatalog::from_fonts(&[MONO, SANS]))
                .default_settings(FontSettings::new("JetBrains Mono").with_size(px(17.)))
        });
        cx.subscribe(&picker, {
            let events = events.clone();
            move |_, _, _| events.set(events.get() + 1)
        })
        .detach();
        picker.update(cx, |picker, cx| {
            picker.start(window, cx);
            picker.set_features_open(true, cx);
            picker.families.update(cx, |list, cx| {
                list.set_query("mono", window, cx);
            });
            picker.family_select.update(cx, |select, cx| {
                select.state.open = true;
                select.state.list.update(cx, |list, cx| {
                    list.set_query("plex", window, cx);
                    list.scroll_handle()
                        .base_handle()
                        .set_offset(point(px(0.), px(-12.)));
                });
            });
            let settings = picker.settings().clone();
            picker.set_catalog(FontCatalog::from_fonts(&[INTER, MONO, SANS]), window, cx);
            assert_eq!(picker.settings(), &settings);
            assert!(picker.is_features_open());
            assert!(!picker.is_loading());
            let families = picker.families.read(cx);
            assert_eq!(families.delegate().query, "mono");
            assert_eq!(families.delegate().matches.len(), 1);
            assert_eq!(
                families.delegate().family(0).unwrap().name(),
                "JetBrains Mono"
            );
            assert_eq!(families.selected_index(), Some(IndexPath::new(0)));
            let select = picker.family_select.read(cx);
            assert!(select.state.is_open());
            assert_eq!(select.selected_value(), Some(&"JetBrains Mono".into()));
            let list = select.state.list.read(cx);
            assert_eq!(list.query_input.read(cx).value(), "plex");
            assert_eq!(list.delegate().delegate.items_count(0), 1);
            assert_eq!(
                list.delegate()
                    .delegate
                    .item(IndexPath::new(0))
                    .unwrap()
                    .name,
                "IBM Plex Sans"
            );
            assert_eq!(
                list.scroll_handle().base_handle().offset(),
                point(px(0.), px(-12.))
            );
            assert_eq!(events.get(), 0);
        });
    });
}

#[gpui::test]
fn known_fonts_can_be_searched_and_chosen_while_loading(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .app_fonts(vec![Cow::Borrowed(MONO), Cow::Borrowed(SANS)])
                .default_settings(FontSettings::new("IBM Plex Sans"))
        });
        picker.update(cx, |picker, cx| {
            // Supply the result explicitly so the test controls when loading ends.
            picker.apply_catalog(FontCatalog::default(), window, cx);
            assert!(picker.is_loading());
            picker.families.update(cx, |list, cx| {
                list.set_query("mono", window, cx);
            });
            assert_eq!(picker.families.read(cx).delegate().matches.len(), 1);
            picker.choose_family_named("JetBrains Mono", window, cx);
            assert_eq!(picker.settings().family(), "JetBrains Mono");
            picker.set_catalog(FontCatalog::from_fonts(&[INTER]), window, cx);
            assert!(!picker.is_loading());
            assert_eq!(picker.settings().family(), "JetBrains Mono");
            assert!(
                picker
                    .font_catalog()
                    .unwrap()
                    .family("IBM Plex Sans")
                    .is_some()
            );
            assert!(
                picker
                    .font_catalog()
                    .unwrap()
                    .family("Inter Variable")
                    .is_some()
            );
            assert_eq!(picker.families.read(cx).delegate().query, "JetBrains Mono");
        });
    });
}

#[gpui::test]
fn an_explicit_catalog_wins_over_the_initial_load(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let cx = cx.add_empty_window();
    let picker = cx.update(|window, cx| {
        let picker = cx.new(|cx| FontPickerState::new(window, cx));
        picker.update(cx, |picker, cx| {
            picker.start(window, cx);
            picker.set_catalog(FontCatalog::from_fonts(&[MONO]), window, cx);
        });
        picker
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let picker = picker.read(cx);
        assert!(!picker.is_loading());
        assert_eq!(picker.font_catalog().unwrap().families().len(), 1);
        assert!(
            picker
                .font_catalog()
                .unwrap()
                .family("JetBrains Mono")
                .is_some()
        );
    });
}

#[gpui::test]
fn replacing_a_catalog_keeps_an_unavailable_current_font(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .catalog(FontCatalog::from_fonts(&[MONO]))
                .default_settings(FontSettings::new("JetBrains Mono"))
        });
        picker.update(cx, |picker, cx| {
            picker.start(window, cx);
            picker.set_catalog(FontCatalog::from_fonts(&[SANS]), window, cx);
            assert_eq!(picker.settings().family(), "JetBrains Mono");
            assert_eq!(picker.families.read(cx).selected_index(), None);
            assert_eq!(picker.family_select.read(cx).selected_value(), None);
        });
    });
}
