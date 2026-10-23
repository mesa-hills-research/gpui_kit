use std::{rc::Rc, time::Duration};

use gpui::{
    App, ClipboardItem, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_base::{Transition, transition};
use rust_i18n::t;

use crate::{
    ActiveTheme as _, Icon, IconName, Sizable, Size, StyledExt as _,
    button::{Button, ButtonVariants as _},
};

/// How long the check mark stays once it has faded in, before it fades back
/// to the copy icon.
const COPIED_HOLD: Duration = Duration::from_secs(1);

/// An element that provides clipboard copy functionality.
///
/// A click copies the value, and the copy icon cross-fades to a check mark in
/// the theme's success color, holds for a second, then fades back.
#[derive(IntoElement)]
pub struct Clipboard {
    id: ElementId,
    style: StyleRefinement,
    value: SharedString,
    value_fn: Option<Rc<dyn Fn(&mut Window, &mut App) -> SharedString>>,
    on_copied: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    tooltip_text: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    tab_stop: bool,
    size: Size,
}

impl Sizable for Clipboard {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

/// Styles the button, such as its icon's color with `text_color`.
impl Styled for Clipboard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Clipboard {
    /// Create a new Clipboard element with the given ID.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            value: SharedString::default(),
            value_fn: None,
            on_copied: None,
            tooltip_text: None,
            accessibility_label: None,
            tab_stop: true,
            size: Size::XSmall,
        }
    }

    /// Set tooltip text for the clipboard button.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip_text = Some(tooltip.into());
        self
    }

    /// Set the name a screen reader announces. Defaults to the localized "Copy".
    ///
    /// The clipboard button shows only an icon, so it has no visible label to
    /// read, and its tooltip is a hint rather than a name.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// Set whether Tab stops at the button, as [`Button::tab_stop`]. Default is true.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the value for copying to the clipboard. Default is an empty string.
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    /// Set the value of the clipboard to the result of the given function. Default is None.
    ///
    /// When used this, the copy value will use the result of the function.
    pub fn value_fn(
        mut self,
        value: impl Fn(&mut Window, &mut App) -> SharedString + 'static,
    ) -> Self {
        self.value_fn = Some(Rc::new(value));
        self
    }

    /// Set a callback to be invoked when the content is copied to the clipboard.
    pub fn on_copied<F>(mut self, handler: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_copied = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Clipboard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| ClipboardState::default());

        let value = self.value.clone();
        let clipboard_id = self.id.clone();
        let copied = state.read(cx).copied;
        let value_fn = self.value_fn.clone();
        let motion = cx.theme().motion_tokens();
        let fade = motion.duration_fast;
        // How far the check mark has faded in over the copy icon.
        let check = transition(
            (self.id.clone(), "copied"),
            if copied { 1. } else { 0. },
            Transition::new(fade).easing(motion.easing_move.clone()),
            window,
            cx,
        );
        // The size the button gives its icon.
        let icon_size = match self.size {
            Size::Size(size) => Size::Size(size * 0.75),
            size => size,
        };

        div()
            .relative()
            .flex_none()
            .child(
                Button::new(clipboard_id)
                    .icon(Icon::new(IconName::Copy).opacity(1. - check))
                    .ghost()
                    .with_size(self.size)
                    .tab_stop(self.tab_stop)
                    .refine_style(&self.style)
                    .when_some(self.tooltip_text, |this, text| this.tooltip(text))
                    .accessibility_label(
                        self.accessibility_label
                            .unwrap_or_else(|| t!("Copy").into()),
                    )
                    .when(!copied, |this| {
                        this.on_click({
                            let state = state.clone();
                            let on_copied = self.on_copied.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                let value = value_fn
                                    .as_ref()
                                    .map(|f| f(window, cx))
                                    .unwrap_or_else(|| value.clone());
                                cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
                                state.update(cx, |state, cx| {
                                    state.copied = true;
                                    cx.notify();
                                });

                                let state = state.clone();
                                cx.spawn(async move |cx| {
                                    cx.background_executor().timer(fade + COPIED_HOLD).await;
                                    _ = state.update(cx, |state, cx| {
                                        state.copied = false;
                                        cx.notify();
                                    });
                                })
                                .detach();

                                if let Some(on_copied) = &on_copied {
                                    on_copied(value.clone(), window, cx);
                                }
                            }
                        })
                    }),
            )
            // Over the button, as large as it, and the pointer still reaches
            // the button.
            .when(check > 0., |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .map(|this| match self.size {
                            Size::Size(size) => this.size(size),
                            Size::XSmall => this.size_5(),
                            Size::Small => this.size_6(),
                            Size::Large | Size::Medium => this.size_8(),
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .debug_selector(|| "clipboard-check".into())
                        .child(
                            Icon::new(IconName::Check)
                                .with_size(icon_size)
                                .text_color(cx.theme().success)
                                .opacity(check),
                        ),
                )
            })
    }
}

#[doc(hidden)]
#[derive(Default)]
struct ClipboardState {
    copied: bool,
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Modifiers, Render, TestAppContext, point, px};

    use super::*;

    struct Copy;

    impl Render for Copy {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .p(px(10.))
                .child(Clipboard::new("copy").value("sk-1234"))
        }
    }

    /// A click copies the value and fades the check mark in, which holds for
    /// a second and then fades back to the copy icon.
    #[gpui::test]
    fn a_copy_shows_the_check_mark_for_a_second(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| Copy);
        let check = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.debug_bounds("clipboard-check").is_some()
        };
        let fade = cx.update(|_, cx| cx.theme().motion_tokens().duration_fast);
        assert!(!check(cx));
        cx.simulate_click(point(px(15.), px(15.)), Modifiers::default());
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("sk-1234".into())
        );
        check(cx);
        cx.executor().advance_clock(fade);
        assert!(check(cx), "no check mark after the click");
        cx.executor().advance_clock(COPIED_HOLD / 2);
        assert!(check(cx), "the check mark left during its hold");
        // The hold ends, and the check mark fades out.
        cx.executor().advance_clock(COPIED_HOLD / 2);
        cx.run_until_parked();
        check(cx);
        cx.executor().advance_clock(fade);
        assert!(!check(cx), "the copy icon didn't come back");
    }
}
