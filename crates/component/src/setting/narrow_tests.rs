use super::*;
use gpui::Focusable as _;

#[gpui::test]
fn narrow_navigation_precedes_the_page_and_preserves_selection_on_resize(cx: &mut TestAppContext) {
    let (host, cx) = setup(cx);
    cx.simulate_resize(size(px(479.), px(700.)));
    draw(cx);
    let state = cx.update(|_, cx| host.read(cx).state.clone().unwrap());
    let search_input = cx.update(|_, cx| state.read(cx).search_input.clone());
    let page_select = cx.update(|_, cx| state.read(cx).page_select.clone());
    let narrow = cx.debug_bounds("setting-1-0-0").unwrap();
    assert_eq!(narrow.left(), px(16.));
    assert!(narrow.size.width > px(440.));
    let search_bounds = cx.update(|_, cx| search_input.read(cx).text_bounds().unwrap());
    assert!(search_bounds.bottom() < narrow.top());
    assert_eq!(selection(&host, cx), (1, None));
    cx.update(|window, cx| search_input.focus_handle(cx).focus(window, cx));
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(cx.update(|window, cx| page_select.focus_handle(cx).is_focused(window)));

    // The breakpoint depends on the whole Settings surface, not the page column.
    cx.simulate_resize(size(px(480.), px(700.)));
    draw(cx);
    assert!(cx.debug_bounds("setting-1-0-0").unwrap().left() > px(250.));
    assert_eq!(selection(&host, cx), (1, None));
    cx.simulate_resize(size(px(460.), px(700.)));
    draw(cx);
    assert_eq!(selection(&host, cx), (1, None));
    assert!(cx.update(|window, cx| page_select.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn narrow_page_picker_supports_keyboard_search_and_focus_order(cx: &mut TestAppContext) {
    let (host, cx) = setup(cx);
    let input = cx.update(|window, cx| cx.new(|cx| InputState::new(window, cx)));
    host.update(cx, |host, cx| {
        let field = input.clone();
        host.pages[2] =
            SettingPage::new("Editor")
                .resettable(false)
                .group(
                    SettingGroup::new().item(
                        SettingItem::render(move |_, _, _| Input::new(&field))
                            .keywords(["theme editor"]),
                    ),
                );
        cx.notify();
    });
    cx.simulate_resize(size(px(460.), px(700.)));
    draw(cx);
    let state = cx.update(|_, cx| host.read(cx).state.clone().unwrap());
    let search_input = cx.update(|_, cx| state.read(cx).search_input.clone());
    let page_select = cx.update(|_, cx| state.read(cx).page_select.clone());
    cx.update(|window, cx| search_input.focus_handle(cx).focus(window, cx));
    cx.simulate_keystrokes("tab enter down enter");
    draw(cx);
    assert_eq!(selection(&host, cx), (2, None));
    assert!(cx.update(|window, cx| page_select.focus_handle(cx).is_focused(window)));
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(cx.update(|window, cx| input.focus_handle(cx).is_focused(window)));
    cx.simulate_input("edited");
    assert_eq!(cx.update(|_, cx| input.read(cx).value()), "edited");
    cx.simulate_keystrokes("shift-tab shift-tab");
    draw(cx);
    assert!(cx.update(|window, cx| search_input.focus_handle(cx).is_focused(window)));

    // Removing the leading page must not renumber the picker's values.
    search(&host, "theme", cx);
    cx.simulate_keystrokes("tab enter up enter");
    draw(cx);
    assert_eq!(selection(&host, cx), (1, None));
    cx.simulate_keystrokes("enter down escape");
    draw(cx);
    assert_eq!(selection(&host, cx), (1, None));
    assert!(cx.update(|window, cx| page_select.focus_handle(cx).is_focused(window)));
    search(&host, "nothing matches", cx);
    assert!(cx.update(|_, cx| state.read(cx).choices.is_empty()));
    search(&host, "", cx);
    assert_eq!(selection(&host, cx), (1, None));
}
