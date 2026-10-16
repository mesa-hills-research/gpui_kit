mod common;
use gpui_kit::component::{
    Root, WindowExt,
    button::Button,
    dialog::{DialogAction, DialogClose, DialogFooter},
    image_viewer::{ImageViewerItem, ImageViewerState},
    input::{Input, InputState},
    notification::Notification,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AppContext, Context, Entity, Focusable, TestAppContext, Window, div, prelude::*, px, size,
};
use std::time::Duration;

struct Workspace {
    saved: Entity<InputState>,
    draft: Entity<InputState>,
}
impl Render for Workspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let draft = self.draft.clone();
        let saved = self.saved.clone();
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(Input::new(&self.saved).id("name"))
            .child(
                Button::new("edit")
                    .label("Edit…")
                    .on_click(move |_, window, cx| {
                        let draft = draft.clone();
                        let saved = saved.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let draft = draft.clone();
                            let saved = saved.clone();
                            dialog
                                .title("Edit profile")
                                .child(Input::new(&draft).id("name"))
                                .footer(DialogFooter::new().child(
                                    DialogAction::new().child(Button::new("ok").label("Save")),
                                ))
                                .on_ok(move |_, window, cx| {
                                    if draft.read(cx).value().is_empty() {
                                        return false;
                                    }
                                    let value = draft.read(cx).value();
                                    saved
                                        .update(cx, |input, cx| input.set_value(value, window, cx));
                                    window.push_notification(
                                        Notification::new()
                                            .message("Profile saved")
                                            .autohide(false),
                                        cx,
                                    );
                                    true
                                })
                        });
                    }),
            )
            .child(
                Button::new("inspect")
                    .label("Inspect…")
                    .on_click(|_, window, cx| {
                        window.open_sheet(cx, |sheet, _, _| {
                            sheet.title("Inspector").child("Details")
                        });
                    }),
            )
            .child(
                Button::new("notify")
                    .label("Notify")
                    .on_click(|_, window, cx| {
                        window.push_notification(
                            Notification::new().message("Updated").autohide(true),
                            cx,
                        );
                    }),
            )
    }
}

#[gpui_kit::test]
async fn dialog_validates_scoped_input_saves_and_dismisses_notification(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(800.), px(700.))), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("name", cx);
        window.click("edit", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let dialog = window.within("dialog").find(0usize).bounds();
        assert!(dialog.left() >= px(0.) && dialog.right() <= window.viewport_size().width);
        assert!(dialog.top() >= px(0.) && dialog.bottom() <= window.viewport_size().height);
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("notification").is_none());
        assert!(
            window.find("dialog").visible(),
            "empty input must prevent confirmation"
        );
        window.within("dialog").click("name", cx);
        window.within("dialog").input("Ada 中文", cx);
        assert_eq!(
            window.within("dialog").find("name").value(),
            Some("Ada 中文")
        );
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none() && window.try_find("notification").is_some()
    })
    .await;
    // Toast entrance also uses GPUI's wall-clock Animation (400 ms).
    std::thread::sleep(Duration::from_millis(410));
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(window.find("name").value(), Some("Ada 中文"));
        assert!(window.find("notification").visible());
        window.hover("notification", cx);
        window.within("notification").click("close", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("notification").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn escape_dismisses_dialog_and_sheet_and_restores_focus(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(800.), px(700.))), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("name", cx);
        window.click("edit", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none() && window.find("name").focused() == Some(true)
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(window.find("name").focused(), Some(true));
        window.click("inspect", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("sheet-content").is_some()
    })
    .await;
    // GPUI's non-synced Animation uses std::time::Instant, not the test clock.
    // Finish the actual entrance (the theme's normal duration, 180 ms) before
    // asserting its final geometry.
    std::thread::sleep(Duration::from_millis(200));
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let sheet = window.find("sheet-content").bounds();
        assert!(
            sheet.size.width > px(0.) && sheet.right() <= window.viewport_size().width,
            "{sheet:?}"
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("sheet").is_none() && window.try_find("sheet-content").is_none()
    })
    .await;
    cx.update_window(handle.into(), |_, window, _| {
        assert_eq!(window.find("name").focused(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn notification_auto_dismisses_after_its_timer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(800.), px(700.))), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("notify", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("notification").is_some()
    })
    .await;
    // The default timeout is five seconds; the notification must remain
    // mounted before that deadline, not merely disappear eventually.
    for _ in 0..40 {
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("notification").is_some());
        })
        .unwrap();
    }
    cx.wait_for(handle.into(), Duration::from_secs(10), |window, _| {
        window.try_find("notification").is_none()
    })
    .await;
}

/// A view with one focusable control behind a dialog whose footer carries
/// both the confirm action and a `DialogClose` trigger.
struct Stealer {
    outside: Entity<InputState>,
}
impl Render for Stealer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(Input::new(&self.outside).id("outside"))
            .child(
                Button::new("open")
                    .label("Open…")
                    .on_click(|_, window, cx| {
                        window.open_dialog(cx, |dialog, _, _| {
                            dialog.title("Stolen focus").footer(
                                DialogFooter::new()
                                    .child(div().id("footer-close").child(
                                        DialogClose::new().trigger(|button| button.label("Cancel")),
                                    ))
                                    .child(
                                        DialogAction::new().child(Button::new("ok").label("OK")),
                                    ),
                            )
                        });
                    }),
            )
            .child(
                Button::new("open-alert")
                    .label("Alert…")
                    .on_click(|_, window, cx| {
                        window.open_alert_dialog(cx, |dialog, _, _| {
                            dialog.title("Stolen focus").confirm()
                        });
                    }),
            )
    }
}

/// Opens the dialog, then moves focus back to the control behind it — what a
/// native web view or an always-on-top window does on its own.
async fn open_dialog_and_steal_focus(
    cx: &mut TestAppContext,
    open: &'static str,
) -> gpui_kit::AnyWindowHandle {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(800.), px(700.))), |window, cx| {
        let view = cx.new(|cx| Stealer {
            outside: cx.new(|cx| InputState::new(window, cx)),
        });
        view
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(open, cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |root, window, cx| {
        let root = root.downcast::<Root>().unwrap().read(cx).view().clone();
        let outside = root.downcast::<Stealer>().unwrap().read(cx).outside.clone();
        let outside = outside.read(cx).focus_handle(cx);
        outside.focus(window, cx);
        window.render_frame(cx);
        assert!(
            outside.is_focused(window),
            "focus must sit outside the dialog"
        );
    })
    .unwrap();
    handle.into()
}

#[gpui_kit::test]
async fn dialog_action_confirms_its_dialog_when_focus_was_stolen(cx: &mut TestAppContext) {
    let handle = open_dialog_and_steal_focus(cx, "open").await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle, Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn dialog_close_trigger_cancels_its_dialog_when_focus_was_stolen(cx: &mut TestAppContext) {
    let handle = open_dialog_and_steal_focus(cx, "open").await;
    cx.update_window(handle, |_, window, cx| {
        window
            .within("dialog")
            .within("footer-close")
            .click("close", cx);
    })
    .unwrap();
    cx.wait_for(handle, Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn alert_dialog_default_buttons_reach_their_dialog_when_focus_was_stolen(
    cx: &mut TestAppContext,
) {
    let handle = open_dialog_and_steal_focus(cx, "open-alert").await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle, Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
}

struct SnappingSheet;
impl Render for SnappingSheet {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                Button::new("inspect")
                    .label("Inspect…")
                    .on_click(|_, window, cx| {
                        window.open_sheet(cx, |sheet, _, _| {
                            sheet
                                .title("Inspector")
                                .transition_duration(Duration::ZERO)
                                .child("Details")
                        });
                    }),
            )
    }
}

/// A sheet with a zero transition is in place on the frame it opens, where
/// the slide would start it off the window's edge.
#[gpui_kit::test]
async fn a_sheet_without_a_transition_opens_in_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _) = common::open_window(cx, Some(size(px(800.), px(600.))), |_, cx| {
        cx.new(|_| SnappingSheet)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("inspect", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("sheet-content").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, _| {
        let sheet = window.find("sheet-content").bounds();
        assert_eq!(sheet.right(), window.viewport_size().width, "{sheet:?}");
    })
    .unwrap();
}

struct Gallery {
    viewer: Option<Entity<ImageViewerState>>,
}
impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(Button::new("thumbnail").label("Open").on_click(cx.listener(
                |this, _, window, cx| {
                    let items = (1..=3).map(|n| {
                        ImageViewerItem::new(format!("missing-{n}.png"))
                            .with_caption(format!("shot-{n}.png"))
                    });
                    this.viewer = Some(window.open_image_viewer(items, 0, cx));
                },
            )))
    }
}

/// The viewer opens over the window with focus, browses with the keys a
/// reader expects, switches to actual size by double-click and Z, and Escape
/// closes it with focus back on the thumbnail that opened it.
#[gpui_kit::test]
fn image_viewer_browses_by_keyboard_and_returns_focus(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, gallery) = common::open_window(cx, Some(size(px(800.), px(600.))), |_, cx| {
        cx.new(|_| Gallery { viewer: None })
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        // The keyboard path: focus the thumbnail (Tab only reaches Root's
        // binding once something holds focus, so the first stop is what Tab
        // does), open it with Enter.
        window.focus_next(cx);
        window.render_frame(cx);
        assert_eq!(window.find("thumbnail").focused(), Some(true));
        window.press("enter", cx);
        window.render_frame(cx);
        let viewer = window.find("image-viewer");
        assert_eq!(viewer.role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(viewer.focused(), Some(true));
        assert_eq!(viewer.bounds().size.width, px(800.));
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-1.png")
        );

        window.press("right", cx);
        window.press("right", cx);
        window.press("right", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-3.png")
        );
        window.press("home", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-1.png")
        );
        window.press("end", cx);
        window.press("left", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-2.png")
        );
    })
    .unwrap();
    let viewer = common::update_content(handle, &gallery, cx, |gallery, _, _| {
        gallery.viewer.clone().expect("opened")
    })
    .unwrap();
    let actual_size = |cx: &mut TestAppContext| viewer.read_with(cx, |v, _| v.is_actual_size());

    cx.update_window(handle.into(), |_, window, cx| {
        window.double_click("image-viewer-stage", cx)
    })
    .unwrap();
    assert!(actual_size(cx));
    cx.update_window(handle.into(), |_, window, cx| window.press("z", cx))
        .unwrap();
    assert!(!actual_size(cx));

    cx.update_window(handle.into(), |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("image-viewer").is_none());
        assert!(!window.has_active_image_viewer(cx));
        assert_eq!(window.find("thumbnail").focused(), Some(true));
    })
    .unwrap();
}

fn open_gallery(cx: &mut TestAppContext) -> (gpui_kit::WindowHandle<Root>, Entity<Gallery>) {
    cx.update(gpui_kit::init);
    common::open_window(cx, Some(size(px(800.), px(600.))), |_, cx| {
        cx.new(|_| Gallery { viewer: None })
    })
}

/// Clicking Next twice quickly browses twice; the second click doesn't also
/// reach the stage as a double click that switches to actual size.
#[gpui_kit::test]
fn quick_clicks_on_next_only_browse(cx: &mut TestAppContext) {
    let (handle, _) = open_gallery(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let items = (0..5).map(|ix| ImageViewerItem::new(format!("missing-{ix}.png")));
        let viewer = window.open_image_viewer(items, 0, cx);
        window.click("image-viewer-next", cx);
        window.double_click("image-viewer-next", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).selected_index(), 3);
        assert!(!viewer.read(cx).is_actual_size());
    })
    .unwrap();
}

/// A dialog that opens while the viewer is up shows over it and takes the
/// keys, and Escape closes it with focus back on the viewer. A viewer opened
/// from a dialog covers that dialog, and Escape closes the viewer first.
#[gpui_kit::test]
async fn the_viewer_and_dialogs_stack_in_the_order_they_open(cx: &mut TestAppContext) {
    let (handle, _) = open_gallery(cx);
    let allowed = std::rc::Rc::new(std::cell::Cell::new(0));
    let question = {
        let allowed = allowed.clone();
        move |dialog: gpui_kit::component::dialog::Dialog, _: &mut Window, _: &mut gpui_kit::App| {
            let allowed = allowed.clone();
            dialog.title("A question").child(
                Button::new("allow")
                    .label("Allow")
                    .on_click(move |_, _, _| allowed.set(allowed.get() + 1)),
            )
        }
    };
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("thumbnail", cx);
        window.render_frame(cx);
        window.open_dialog(cx, question.clone());
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("allow").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("image-viewer").focused(), Some(false));
        // The pointer reaches the dialog's button, not the viewer under it.
        window.click("allow", cx);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert!(window.has_active_image_viewer(cx));
    })
    .unwrap();
    assert_eq!(allowed.get(), 1, "the click landed on the dialog");
    cx.background_executor.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("image-viewer").focused(), Some(true));
        window.close_image_viewer(cx);
        window.open_dialog(cx, question);
    })
    .unwrap();

    // Now the other way around.
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("allow").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.open_image_viewer([ImageViewerItem::new("missing.png")], 0, cx);
        window.render_frame(cx);
        assert_eq!(window.find("image-viewer").focused(), Some(true));
        // The dialog's button lies under the viewer.
        window.click("allow", cx);
        window.press("escape", cx);
        assert!(!window.has_active_image_viewer(cx));
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    assert_eq!(allowed.get(), 1, "the viewer covered the dialog");
}

/// At actual size, the arrows pan an image larger than the window, and
/// Page Up and Page Down browse. Fitted, the arrows browse.
#[gpui_kit::test]
fn the_arrows_pan_a_large_image_at_actual_size(cx: &mut TestAppContext) {
    let (handle, _) = open_gallery(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let items =
            [(4000, 3000), (100, 100)]
                .into_iter()
                .enumerate()
                .map(|(ix, (width, height))| {
                    ImageViewerItem::new(format!("missing-{ix}.png"))
                        .with_caption(format!("shot-{ix}.png"))
                        .with_dimensions(width, height)
                });
        window.open_image_viewer(items, 0, cx);
        window.press("z", cx);
        let start = window.find("image-viewer-content").bounds().origin;

        window.press("right", cx);
        window.press("right", cx);
        window.press("down", cx);
        let panned = window.find("image-viewer-content").bounds().origin;
        assert_eq!(panned.x, start.x - px(96.));
        assert_eq!(panned.y, start.y - px(48.));
        window.press("left", cx);
        window.press("up", cx);
        window.press("up", cx);
        let back = window.find("image-viewer-content").bounds().origin;
        assert_eq!(
            (back.x, back.y),
            (start.x - px(48.), start.y),
            "stops at the top"
        );
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-0.png")
        );

        window.press("pagedown", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-1.png")
        );
        // A small image at actual size has nothing to pan: the arrows browse.
        window.press("z", cx);
        window.press("left", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-0.png")
        );
        window.press("pagedown", cx);
        window.press("pageup", cx);
        assert_eq!(
            window.find("image-viewer-stage").label(),
            Some("shot-0.png")
        );
    })
    .unwrap();
}

/// A viewer of no images says so, with no counter, and its keys do nothing.
#[gpui_kit::test]
fn an_empty_viewer_says_it_has_no_images(cx: &mut TestAppContext) {
    let (handle, _) = open_gallery(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let state = window.open_image_viewer(Vec::<ImageViewerItem>::new(), 0, cx);
        window.render_frame(cx);
        assert_eq!(window.find("image-viewer-stage").label(), Some("No images"));
        for key in ["right", "end", "pagedown", "z"] {
            window.press(key, cx);
        }
        assert!(!state.read(cx).is_actual_size());
        assert_eq!(state.read(cx).selected_index(), 0);
    })
    .unwrap();
}
