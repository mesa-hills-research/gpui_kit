mod common;
use gpui_kit::component::{button::Button, menu::DropdownMenu};
use gpui_kit::test::{TestAppContextExt, TestSupportExt, TestWindowExt};
use gpui_kit::{AppContext, Context, TestAppContext, Window, actions, div, prelude::*, px, size};
use std::time::Duration;

actions!(menu_test, [Save, Unavailable]);
struct Commands {
    saved: bool,
    focus: gpui_kit::FocusHandle,
}
impl Render for Commands {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .p_4()
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                this.saved = true;
                cx.notify();
            }))
            .on_action(|_: &Unavailable, _, _| panic!("disabled command dispatched"))
            .child(
                Button::new("commands")
                    .label("Commands")
                    .dropdown_menu(|menu, window, cx| {
                        menu.menu_with_disabled("Unavailable", Box::new(Unavailable), true)
                            .menu("Save", Box::new(Save))
                            .submenu("More", window, cx, |menu, _, _| {
                                menu.menu("Save copy", Box::new(Save))
                            })
                    }),
            )
            .child(div().id("result").test_support().child(if self.saved {
                div().id("saved").test_support().child("Saved")
            } else {
                div().id("unsaved").test_support().child("Unsaved")
            }))
    }
}
#[gpui_kit::test]
async fn menu_skips_disabled_commands_confirms_and_restores_focus(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(640.), px(480.))), |window, cx| {
        let view = cx.new(|cx| Commands {
            saved: false,
            focus: cx.focus_handle(),
        });
        view.read(cx).focus.clone().focus(window, cx);
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("commands", cx);
        assert_eq!(window.find("popup-menu").focused(), Some(true));
        window.within("popup-menu").click(0usize, cx);
        assert!(window.find("unsaved").visible());
        window.within("popup-menu").press("down", cx);
        assert_eq!(
            window.within("popup-menu").find(1usize).selected(),
            Some(true)
        );
        window.within("popup-menu").press("enter", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none() && window.try_find("saved").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(window.find("workspace").focused(), Some(true));
        window.click("commands", cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn hovering_submenu_opens_and_clicking_item_dismisses_the_chain(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(640.), px(480.))), |window, cx| {
        let view = cx.new(|cx| Commands {
            saved: false,
            focus: cx.focus_handle(),
        });
        view.read(cx).focus.clone().focus(window, cx);
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("commands", cx);
        let mut menu = window.within("popup-menu");
        menu.hover(2usize, cx);
        assert_eq!(menu.find(2usize).selected(), Some(true));
    })
    .unwrap();
    // Submenus already own a native "submenu" identity scope.
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("submenu").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(
            window.within("submenu").find(0usize).label(),
            Some("Save copy")
        );
        window.within("submenu").click(0usize, cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("saved").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, _| {
        assert!(window.try_find("submenu").is_none());
        assert!(window.try_find("popup-menu").is_none());
    })
    .unwrap();
}

/// A scrollable menu whose only submenu item is the last of 31 rows, so it
/// starts scrolled out of the 160px viewport.
struct ScrollableCommands {
    focus: gpui_kit::FocusHandle,
}
impl Render for ScrollableCommands {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .p_4()
            .child(
                Button::new("commands")
                    .label("Commands")
                    .dropdown_menu(|menu, window, cx| {
                        (0..30)
                            .fold(menu.scrollable(true).max_h(px(160.)), |menu, ix| {
                                menu.menu(format!("Item {ix}"), Box::new(Save))
                            })
                            .submenu("More", window, cx, |menu, _, _| {
                                menu.menu("Save copy", Box::new(Save))
                            })
                    }),
            )
    }
}

/// The submenu is painted as a deferred draw outside the items container, so
/// the container's `overflow_y_scroll` clip must not hide it and its items
/// must still be hit-testable.
#[gpui_kit::test]
async fn submenu_opens_unclipped_from_a_scrollable_menu(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(640.), px(480.))), |window, cx| {
        let view = cx.new(|cx| ScrollableCommands {
            focus: cx.focus_handle(),
        });
        view.read(cx).focus.clone().focus(window, cx);
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("commands", cx);
        let mut menu = window.within("popup-menu");
        // `up` wraps to the last item and scrolls it into view, where the
        // pointer can reach it.
        menu.press("up", cx);
        assert!(menu.find(30usize).visible(), "{:?}", menu.find(30usize));
        menu.hover(30usize, cx);
        assert_eq!(menu.find(30usize).selected(), Some(true));
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("submenu").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let submenu = window.find("submenu");
        let viewport = gpui_kit::Bounds::new(Default::default(), window.viewport_size());
        assert!(submenu.visible(), "submenu is clipped: {submenu:?}");
        assert!(
            viewport.contains(&submenu.bounds().origin)
                && viewport.contains(&submenu.bounds().bottom_right()),
            "submenu {:?} is outside the window {viewport:?}",
            submenu.bounds()
        );
        let mut submenu = window.within("submenu");
        assert_eq!(submenu.find(0usize).label(), Some("Save copy"));
        submenu.click(0usize, cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
}

#[cfg(not(target_os = "macos"))]
mod menu_bar_keys {
    use super::*;
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gpui_kit::{
        AnyWindowHandle, Capslock, Entity, Modifiers, ModifiersChangedEvent, MouseButton,
        MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, VisualTestContext,
        component::{GlobalState, WindowExt as _, menu::AppMenuBar},
    };

    actions!(menu_bar_test, [NewFile, Undo]);

    /// A window with a menu bar in its title bar, a focusable document and a
    /// text field.
    struct MenuBarWindow {
        menu_bar: gpui_kit::Entity<gpui_kit::component::menu::AppMenuBar>,
        doc: gpui_kit::FocusHandle,
        notes: gpui_kit::Entity<gpui_kit::component::input::InputState>,
        /// Whether Alt was down, at each modifier change the content saw.
        alt_seen: std::rc::Rc<std::cell::RefCell<Vec<bool>>>,
    }

    impl Render for MenuBarWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let alt_seen = self.alt_seen.clone();
            div()
                .size_full()
                .on_modifiers_changed(move |event, _, _| {
                    alt_seen.borrow_mut().push(event.modifiers.alt)
                })
                .child(div().h(px(32.)).child(self.menu_bar.clone()))
                .child(gpui_kit::component::input::Input::new(&self.notes).id("notes"))
                .child(
                    div()
                        .id("doc")
                        .test_support()
                        .track_focus(&self.doc)
                        .size_full(),
                )
        }
    }

    /// A window with File and Edit menus, the document focused.
    fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<AppMenuBar>) {
        let (handle, bar, _) = open_watching_alt(cx);
        (handle, bar)
    }

    /// The same, and whether Alt was down at each modifier change the
    /// content saw.
    fn open_watching_alt(
        cx: &mut TestAppContext,
    ) -> (AnyWindowHandle, Entity<AppMenuBar>, Rc<RefCell<Vec<bool>>>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            GlobalState::global_mut(cx).set_app_menus(vec![
                gpui_kit::Menu::new("File")
                    .items([gpui_kit::MenuItem::action("New", NewFile)])
                    .owned(),
                gpui_kit::Menu::new("Edit")
                    .items([gpui_kit::MenuItem::action("Undo", Undo)])
                    .owned(),
            ]);
        });
        let (handle, view) =
            common::open_window(cx, Some(size(px(600.), px(400.))), |window, cx| {
                cx.new(|cx| {
                    let doc = cx.focus_handle();
                    doc.focus(window, cx);
                    MenuBarWindow {
                        menu_bar: AppMenuBar::new(cx),
                        doc,
                        notes: cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx)),
                        alt_seen: Rc::default(),
                    }
                })
            });
        let (menu_bar, alt_seen) =
            view.read_with(cx, |view, _| (view.menu_bar.clone(), view.alt_seen.clone()));
        let handle: AnyWindowHandle = handle.into();
        // Active, as an app's window is: GPUI reports focus changes and
        // activation only for an active window.
        cx.update_window(handle, |_, window, cx| {
            window.activate_window();
            window.render_frame(cx);
        })
        .unwrap();
        settle_after_focus(cx);
        (handle, menu_bar, alt_seen)
    }

    /// Let the moment after the window gained focus pass, in which an Alt
    /// reported down counts as held from before.
    fn settle_after_focus(cx: &mut TestAppContext) {
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }

    fn state(bar: &Entity<AppMenuBar>, cx: &mut TestAppContext) -> (Option<usize>, Option<usize>) {
        bar.read_with(cx, |bar, _| (bar.highlighted_index(), bar.open_index()))
    }

    fn act(
        cx: &mut TestAppContext,
        handle: AnyWindowHandle,
        f: impl FnOnce(&mut Window, &mut gpui_kit::App),
    ) {
        cx.update_window(handle, |_, window, cx| {
            f(window, cx);
            window.render_frame(cx);
        })
        .unwrap()
    }

    fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
        act(cx, handle, |window, cx| window.press(key, cx));
    }

    fn modifiers(window: &mut Window, cx: &mut gpui_kit::App, modifiers: Modifiers) {
        window.dispatch_event(
            PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                modifiers,
                capslock: Capslock { on: false },
            }),
            cx,
        );
    }

    /// Alt down and up with nothing between: GPUI turns the release into the
    /// lone `alt` keystroke.
    fn tap_alt(cx: &mut TestAppContext, handle: AnyWindowHandle) {
        act(cx, handle, |window, cx| {
            modifiers(window, cx, Modifiers::alt());
            modifiers(window, cx, Modifiers::default());
        });
    }

    fn focused(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<bool> {
        cx.update_window(handle, |_, window, _| window.find(id).focused())
            .unwrap()
    }

    /// F10 or a tap of Alt focuses the first menu's title, Left and Right
    /// move between titles, Down opens one, Escape goes back to the document,
    /// and Alt with a menu's letter opens that menu.
    #[gpui_kit::test]
    fn the_menu_bar_is_reachable_from_the_keyboard(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);

        press(cx, handle, "f10");
        assert_eq!(focused(cx, handle, "app-menu-bar"), Some(true));
        assert_eq!(state(&bar, cx), (Some(0), None));
        press(cx, handle, "right");
        assert_eq!(state(&bar, cx), (Some(1), None));
        press(cx, handle, "right");
        assert_eq!(state(&bar, cx), (Some(0), None), "Right wraps around");
        press(cx, handle, "down");
        assert_eq!(
            state(&bar, cx),
            (None, Some(0)),
            "Down opens the focused menu"
        );
        press(cx, handle, "escape");
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "doc"), Some(true), "focus goes back");

        // A tap of Alt toggles the bar, as F10 does.
        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (Some(0), None));
        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "doc"), Some(true));

        // Alt with the Edit menu's letter opens it straight away.
        press(cx, handle, "alt-e");
        assert_eq!(state(&bar, cx), (None, Some(1)));
        press(cx, handle, "escape");
        assert_eq!(focused(cx, handle, "doc"), Some(true));
    }

    /// Alt+click (a caret added in a text area, a column selection) and
    /// Alt+wheel are no taps: letting Alt go leaves focus where it was.
    #[gpui_kit::test]
    fn alt_with_the_pointer_is_no_tap(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        let position = cx
            .update_window(handle, |_, window, cx| {
                window.click("notes", cx);
                window.find("notes").bounds().center()
            })
            .unwrap();
        assert_eq!(focused(cx, handle, "notes"), Some(true));

        act(cx, handle, |window, cx| {
            modifiers(window, cx, Modifiers::alt());
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::alt(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::alt(),
                    click_count: 1,
                }),
                cx,
            );
            modifiers(window, cx, Modifiers::default());
        });
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "notes"), Some(true));

        act(cx, handle, |window, cx| {
            modifiers(window, cx, Modifiers::alt());
            window.dispatch_event(
                PlatformInput::ScrollWheel(gpui_kit::ScrollWheelEvent {
                    position,
                    modifiers: Modifiers::alt(),
                    ..Default::default()
                }),
                cx,
            );
            modifiers(window, cx, Modifiers::default());
        });
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "notes"), Some(true));
    }

    /// Alt pressed, the window loses focus (Alt+Tab away), and Alt is let go
    /// once it is back: no tap.
    #[gpui_kit::test]
    fn alt_across_a_focus_change_is_no_tap(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        act(cx, handle, |window, cx| {
            modifiers(window, cx, Modifiers::alt())
        });
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.deactivate_window();
        act(cx, handle, |window, cx| {
            window.activate_window();
            modifiers(window, cx, Modifiers::default());
        });
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "doc"), Some(true));
    }

    /// While a dialog is open, F10, Alt and Alt with a letter don't reach the
    /// menu bar behind it.
    #[gpui_kit::test]
    fn the_menu_bar_keys_stop_at_a_dialog(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        act(cx, handle, |window, cx| {
            window.open_dialog(cx, |dialog, _, _| dialog.title("Delete?"));
        });
        press(cx, handle, "f10");
        tap_alt(cx, handle);
        press(cx, handle, "alt-f");
        assert_eq!(state(&bar, cx), (None, None));
        let dialog_open = cx
            .update_window(handle, |_, window, cx| window.has_active_dialog(cx))
            .unwrap();
        assert!(dialog_open);
        assert_ne!(focused(cx, handle, "app-menu-bar"), Some(true));
    }

    /// Clicking another control ends keyboard mode there: the next Alt tap
    /// enters it afresh, and the one after gives focus back to that control.
    #[gpui_kit::test]
    fn a_click_away_ends_keyboard_mode(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        press(cx, handle, "f10");
        assert_eq!(state(&bar, cx), (Some(0), None));
        act(cx, handle, |window, cx| window.click("notes", cx));
        assert_eq!(focused(cx, handle, "notes"), Some(true));
        assert_eq!(state(&bar, cx), (None, None));

        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (Some(0), None));
        tap_alt(cx, handle);
        assert_eq!(focused(cx, handle, "notes"), Some(true));
    }

    /// A tap of Alt, and Alt let go after Alt+click, both reach the window's
    /// modifier listeners as Alt let go, so access keys stop being
    /// underlined and nothing is left thinking Alt is down.
    #[gpui_kit::test]
    fn alt_let_go_reaches_the_window_after_a_tap_or_not(cx: &mut TestAppContext) {
        let (handle, bar, alt_seen) = open_watching_alt(cx);
        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (Some(0), None), "a tap");
        assert_eq!(alt_seen.borrow().last(), Some(&false), "and Alt let go");
        press(cx, handle, "escape");

        let position = cx
            .update_window(handle, |_, window, cx| {
                window.click("notes", cx);
                window.find("notes").bounds().center()
            })
            .unwrap();
        alt_seen.borrow_mut().clear();
        act(cx, handle, |window, cx| {
            modifiers(window, cx, Modifiers::alt());
            for down in [true, false] {
                let (button, modifiers) = (MouseButton::Left, Modifiers::alt());
                window.dispatch_event(
                    if down {
                        PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        })
                    } else {
                        PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position,
                            modifiers,
                            click_count: 1,
                        })
                    },
                    cx,
                );
            }
            modifiers(window, cx, Modifiers::default());
        });
        assert_eq!(state(&bar, cx), (None, None), "no tap");
        assert_eq!(*alt_seen.borrow(), vec![true, false], "Alt let go");
    }

    /// The window gains focus with Alt already held, as after Alt+Tab or a
    /// tiling compositor's Alt+key, and Alt is let go: no tap. A tap a moment
    /// later still is one.
    #[gpui_kit::test]
    fn alt_held_when_the_window_gains_focus_is_no_tap(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.deactivate_window();
        act(cx, handle, |window, _| window.activate_window());
        cx.run_until_parked();
        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "doc"), Some(true));

        settle_after_focus(cx);
        tap_alt(cx, handle);
        assert_eq!(state(&bar, cx), (Some(0), None));
    }

    /// A drag begun before Alt went down and let go with Alt held (Alt pressed
    /// mid-drag for a column selection) makes Alt's release no tap.
    #[gpui_kit::test]
    fn a_drag_finished_with_alt_is_no_tap(cx: &mut TestAppContext) {
        let (handle, bar) = open(cx);
        let bounds = cx
            .update_window(handle, |_, window, cx| {
                window.click("notes", cx);
                window.find("notes").bounds()
            })
            .unwrap();
        let (start, end) = (
            bounds.origin + gpui_kit::point(px(4.), px(4.)),
            bounds.center(),
        );
        act(cx, handle, |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: start,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            modifiers(window, cx, Modifiers::alt());
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: end,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::alt(),
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: end,
                    modifiers: Modifiers::alt(),
                    click_count: 1,
                }),
                cx,
            );
            modifiers(window, cx, Modifiers::default());
        });
        assert_eq!(state(&bar, cx), (None, None));
        assert_eq!(focused(cx, handle, "notes"), Some(true));
    }
}
