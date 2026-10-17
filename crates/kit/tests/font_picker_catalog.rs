mod common;

use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window,
    component::font_picker::{FontCatalog, FontPicker, FontPickerState, FontSettings},
    px, size,
    test::TestWindowExt as _,
};

const MONO: &[u8] = include_bytes!("../../story-web/fonts/JetBrainsMono-Regular.ttf");
const SANS: &[u8] = include_bytes!("../../story-web/fonts/IBMPlexSans-Regular.ttf");
const INTER: &[u8] = include_bytes!("../../story-web/fonts/Inter-Regular.ttf");

struct PickerView {
    picker: Entity<FontPickerState>,
}

impl Render for PickerView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        FontPicker::new(&self.picker).compact()
    }
}

#[gpui_kit::test]
async fn replacing_the_catalog_keeps_an_open_search_usable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, view) = common::open_window(cx, Some(size(px(720.), px(500.))), |window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .catalog(FontCatalog::from_fonts(&[MONO, SANS]))
                .default_settings(FontSettings::new("JetBrains Mono"))
        });
        cx.new(|_| PickerView { picker })
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.within("font-picker-family").click("input", cx);
        window.input("plex", cx);
    })
    .unwrap();
    cx.run_until_parked();
    common::update_content(window, &view, cx, |view, window, cx| {
        view.picker.update(cx, |picker, cx| {
            picker.set_catalog(FontCatalog::from_fonts(&[MONO, SANS, INTER]), window, cx);
        });
    })
    .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("font-picker-family").expanded(), Some(true));
        assert_eq!(
            window.find("font-picker-family").value(),
            Some("JetBrains Mono")
        );
        // Typing again checks that the same search field still owns focus.
        window.input(" sans", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).picker.read(cx).settings().family(),
            "IBM Plex Sans"
        );
    });
}
