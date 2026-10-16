use gpui_kit::{AssetSource, IntoElement, ParentElement, assets::IconName};

gpui_kit::assets::icon_assets!(AppAssets, [Search, Check]);

#[test]
fn assets_are_usable_through_kit_without_component() {
    let _ = gpui_kit::div().child(IconName::Search).into_any_element();
    assert!(AppAssets.load(&IconName::Search.path()).unwrap().is_some());
    assert!(
        AppAssets
            .load(&IconName::Accessibility.path())
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "component")]
#[test]
fn component_accepts_shared_names_and_preserves_legacy_views() {
    use gpui_kit::component::{Icon, IconNameExt};
    use gpui_kit::component::{IconName as LegacyIconName, IconNamed};
    let _: fn(LegacyIconName, &mut gpui_kit::App) -> gpui_kit::Entity<Icon> = LegacyIconName::view;
    assert_eq!(LegacyIconName::Search.path(), "icons/search.svg");
    let _: gpui_kit::AnyElement = LegacyIconName::Search.into();
    let shared: IconName = LegacyIconName::Search.into();
    assert_eq!(shared, IconName::Search);
    let _ = Icon::new(shared);
    let _ = Icon::new(IconName::Accessibility);
    let _ = Icon::new(LegacyIconName::Search);
    let _: fn(IconName, &mut gpui_kit::App) -> gpui_kit::Entity<Icon> = IconNameExt::view;
}

/// The bundled-fonts snippet of the docs, as written: the fonts it sets
/// survive switching to a theme that names none, which every bundled theme
/// is.
#[cfg(feature = "component")]
#[gpui::test]
fn the_documented_bundled_fonts_snippet_keeps_its_fonts(cx: &mut gpui::TestAppContext) {
    use gpui_kit::assets::fonts;
    use gpui_kit::component::{FontOverrides, Theme, ThemeConfig};
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.text_system()
            .add_fonts(fonts::all())
            .expect("the bundled fonts are valid");
        // As font overrides, which every later theme switch keeps.
        Theme::update(cx, |theme| {
            theme.set_font_overrides(
                FontOverrides::new()
                    .with_font_family(fonts::WORK_SANS_FAMILY)
                    .with_mono_font_family(fonts::JETBRAINS_MONO_FAMILY),
            )
        });

        let dark = std::rc::Rc::new(
            serde_json::from_value::<ThemeConfig>(serde_json::json!({
                "name": "Plain Dark",
                "mode": "dark"
            }))
            .unwrap(),
        );
        Theme::update(cx, |theme| theme.apply_config(&dark));
        let theme = Theme::global(cx);
        assert_eq!(theme.font_family.as_ref(), fonts::WORK_SANS_FAMILY);
        assert_eq!(
            theme.mono_font_family.as_ref(),
            fonts::JETBRAINS_MONO_FAMILY
        );
    });
}
