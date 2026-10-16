use crate::root::WindowState;
use std::{rc::Rc, time::Duration};

use gpui::{
    Action, AnyElement, AnyView, App, AppContext, Context, ElementId, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_base::{
    Tooltip as BaseTooltip, TooltipOverlay as BaseTooltipOverlay,
    TooltipRequest as BaseTooltipRequest, TooltipTransition as BaseTooltipTransition,
    TooltipTrigger,
};

use crate::{
    ActiveTheme, Placement, StyledExt,
    animation::{EffectTransition, ease_in_out_cubic, ease_out_cubic},
    kbd::Kbd,
    text::Text,
};

pub(crate) fn init(_cx: &mut App) {
    // No app-level init needed — TooltipOverlay is per-window via Root.
}

// ── Tooltip view (unchanged API) ────────────────────────────────────────────

enum TooltipContext {
    Text(Text),
    Element(Box<dyn Fn(&mut Window, &mut App) -> AnyElement>),
}

/// A Tooltip element that can display text or custom content,
/// with optional key binding information.
pub struct Tooltip {
    style: StyleRefinement,
    content: TooltipContext,
    key_binding: Option<Kbd>,
    action: Option<(Box<dyn Action>, Option<SharedString>)>,
}

impl Tooltip {
    /// Create a Tooltip with a text content.
    pub fn new(text: impl Into<Text>) -> Self {
        Self {
            style: StyleRefinement::default(),
            content: TooltipContext::Text(text.into()),
            key_binding: None,
            action: None,
        }
    }

    /// Create a Tooltip with a custom element.
    pub fn element<E, F>(builder: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut Window, &mut App) -> E + 'static,
    {
        Self {
            style: StyleRefinement::default(),
            key_binding: None,
            action: None,
            content: TooltipContext::Element(Box::new(move |window, cx| {
                builder(window, cx).into_any_element()
            })),
        }
    }

    /// Set Action to display key binding information for the tooltip if it exists.
    pub fn action(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.action = Some((action.boxed_clone(), context.map(SharedString::new)));
        self
    }

    /// Set KeyBinding information for the tooltip.
    pub fn key_binding(mut self, key_binding: Option<Kbd>) -> Self {
        self.key_binding = key_binding;
        self
    }

    /// Build the tooltip and return it as an `AnyView`.
    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

impl FluentBuilder for Tooltip {}
impl Styled for Tooltip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key_binding = if let Some(key_binding) = &self.key_binding {
            Some(key_binding.clone())
        } else {
            if let Some((action, context)) = &self.action {
                Kbd::binding_for_action(
                    action.as_ref(),
                    context.as_ref().map(|s| s.as_ref()),
                    window,
                )
            } else {
                None
            }
        };

        div().child(
            // Wrap in a child, to ensure the left margin is applied to the tooltip
            BaseTooltip::new("tooltip-popup")
                .h_flex()
                .font_family(cx.theme().font_family.clone())
                .m_3()
                .bg(cx.theme().tokens.popover)
                .text_color(cx.theme().popover_foreground)
                .bg(cx.theme().tokens.popover)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_md()
                .rounded(cx.theme().radius)
                .justify_between()
                .py_0p5()
                .px_2()
                .text_sm()
                .gap_3()
                .refine_style(&self.style)
                .map(|this| {
                    this.child(div().map(|this| match self.content {
                        TooltipContext::Text(ref text) => this.child(text.clone()),
                        TooltipContext::Element(ref builder) => this.child(builder(window, cx)),
                    }))
                })
                .when_some(key_binding, |this, kbd| {
                    this.child(
                        div()
                            .text_xs()
                            .flex_shrink_0()
                            .text_color(cx.theme().muted_foreground)
                            .child(kbd),
                    )
                }),
        )
    }
}

// ── Managed tooltip system ──────────────────────────────────────────────────

/// Duration of the slide-down enter animation.
const ENTER_DURATION: Duration = Duration::from_millis(150);
/// Duration of the position-slide animation when switching tooltips.
const SLIDE_DURATION: Duration = Duration::from_millis(200);
pub(crate) fn render_tooltip(
    content_view: AnyView,
    transition: BaseTooltipTransition,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    // The selector lets tests see whether a tooltip was painted.
    div()
        .debug_selector(|| "tooltip".into())
        .child(content_view)
        .map(|element| match transition {
            BaseTooltipTransition::Switch {
                epoch,
                previous,
                current,
            } => {
                let same_row = (current.origin.y - previous.origin.y).abs() < px(10.);
                if !same_row {
                    return element.into_any_element();
                }
                let dx = current.center().x - previous.center().x;
                EffectTransition::new(SLIDE_DURATION)
                    .ease(ease_in_out_cubic)
                    .slide_x(-dx, px(0.))
                    .apply(
                        element,
                        ElementId::NamedInteger("tooltip-slide".into(), epoch as u64),
                    )
                    .into_any_element()
            }
            BaseTooltipTransition::Enter { epoch } => EffectTransition::new(ENTER_DURATION)
                .ease(ease_out_cubic)
                .slide_y(px(4.), px(0.))
                .fade(0.0, 1.0)
                .apply(
                    element,
                    ElementId::NamedInteger("tooltip-enter".into(), epoch as u64),
                )
                .into_any_element(),
        })
}

// ── Extension trait for managed tooltips ─────────────────────────────────────

// ── Shared tooltip state for components ─────────────────────────────────────

/// Shared tooltip state that components (Button, Switch, Checkbox, Radio, etc.)
/// can embed to get `.tooltip()` support with minimal boilerplate.
#[derive(Default)]
pub(crate) struct ComponentTooltip {
    pub text: Option<(
        SharedString,
        Option<(Rc<Box<dyn Action>>, Option<SharedString>)>,
    )>,
    pub builder: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyView>>,
}

impl ComponentTooltip {
    /// Apply this tooltip to a `Stateful<Div>` (or any `ManagedTooltipExt` element).
    pub fn apply<E: ManagedTooltipExt>(self, el: E) -> E {
        if let Some(builder) = self.builder {
            el.managed_tooltip(move |window, cx| builder(window, cx))
        } else if let Some((text, action)) = self.text {
            el.managed_tooltip(move |window, cx| {
                Tooltip::new(text.clone())
                    .when_some(action.clone(), |this, (action, context)| {
                        this.action(
                            action.boxed_clone().as_ref(),
                            context.as_ref().map(|c| c.as_ref()),
                        )
                    })
                    .build(window, cx)
            })
        } else {
            el
        }
    }
}

// ── Internal managed tooltip trait ──────────────────────────────────────────

pub(crate) trait ManagedTooltipExt:
    StatefulInteractiveElement + crate::ElementExt + Sized
{
    fn managed_tooltip(
        self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.managed_tooltip_with_placement(None, build_tooltip)
    }

    fn managed_tooltip_at(
        self,
        placement: Placement,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.managed_tooltip_with_placement(Some(placement), build_tooltip)
    }

    fn managed_tooltip_with_placement(
        self,
        preferred_placement: Option<Placement>,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        let build_tooltip = Rc::new(build_tooltip);
        let trigger = TooltipTrigger::new();

        self.on_prepaint({
            let trigger = trigger.clone();
            move |bounds, window, _| trigger.prepaint(bounds, window)
        })
        .on_hover({
            let build_tooltip = build_tooltip.clone();
            move |hovered, window, cx| {
                if let Some(overlay) = WindowState::tooltip_overlay(window, cx) {
                    if *hovered {
                        overlay.update(cx, |o: &mut BaseTooltipOverlay, cx| {
                            let build = build_tooltip.clone();
                            let request =
                                BaseTooltipRequest::for_trigger(&trigger, move |window, cx| {
                                    build(window, cx)
                                });
                            let request = match preferred_placement {
                                Some(placement) => request.placement(placement),
                                None => request,
                            };
                            o.request_show(request, window, cx);
                        });
                    } else {
                        overlay.update(cx, |o: &mut BaseTooltipOverlay, cx| {
                            o.request_hide(window, cx);
                        });
                    }
                }
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            if let Some(overlay) = WindowState::tooltip_overlay(window, cx) {
                overlay.update(cx, |overlay, cx| {
                    overlay.hide(cx);
                });
            }
        })
    }
}

impl<E: StatefulInteractiveElement + crate::ElementExt> ManagedTooltipExt for E {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::{
        AppContext as _, Context, Entity, InputEvent as _, InteractiveElement as _, IntoElement,
        Modifiers, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
        Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point,
        prelude::FluentBuilder as _, px, size,
    };

    use crate::button::Button;

    /// Longer than any show delay or grace period.
    const LONG: Duration = Duration::from_secs(2);

    /// A page with a close button, like a settings page that closes itself.
    struct Page {
        open: bool,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let page = cx.entity();
            div().size_full().p(px(20.)).when(self.open, |this| {
                this.child(
                    Button::new("close")
                        .debug_selector(|| "close".into())
                        .label("Close")
                        .tooltip("Close Settings")
                        .on_click(move |_, _, cx| {
                            page.update(cx, |page, cx| {
                                page.open = false;
                                cx.notify();
                            })
                        }),
                )
            })
        }
    }

    fn window(cx: &mut TestAppContext) -> (Entity<Page>, &mut VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut page = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Page { open: true });
            page = Some(view.clone());
            crate::Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(400.), px(300.)));
        draw(cx);
        (page.unwrap(), cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn wait(cx: &mut VisualTestContext, duration: Duration) {
        cx.executor().advance_clock(duration);
        draw(cx);
    }

    fn painted(cx: &mut VisualTestContext) -> bool {
        cx.debug_bounds("tooltip").is_some()
    }

    fn button_center(cx: &mut VisualTestContext) -> Point<Pixels> {
        cx.debug_bounds("close")
            .expect("the close button is painted")
            .center()
    }

    /// Rests the pointer on the close button until its tooltip shows.
    fn hover_until_shown(cx: &mut VisualTestContext) -> Point<Pixels> {
        let center = button_center(cx);
        cx.simulate_mouse_move(center, None, Modifiers::default());
        draw(cx);
        assert!(!painted(cx), "a tooltip waits for the show delay");
        wait(cx, LONG);
        assert!(painted(cx), "a resting pointer shows the tooltip");
        center
    }

    fn close(page: &Entity<Page>, cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            page.update(cx, |page, cx| {
                page.open = false;
                cx.notify();
            })
        });
    }

    fn assert_gone_for_good(cx: &mut VisualTestContext) {
        assert!(
            cx.debug_bounds("close").is_none(),
            "the button left the window"
        );
        assert!(
            !painted(cx),
            "the tooltip disappears with the frame that drops its button"
        );
        wait(cx, LONG);
        assert!(!painted(cx), "and does not come back");
    }

    /// The click that removes the button, as Windows delivers it: capturing
    /// the mouse on press and releasing it on release each produce a mouse
    /// move at the same position, and the release's move arrives before the
    /// frame that drops the button.
    #[gpui::test]
    fn clicking_a_button_that_closes_its_page_takes_the_tooltip_with_it(cx: &mut TestAppContext) {
        let (_, cx) = window(cx);
        let position = hover_until_shown(cx);

        cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            window.dispatch_event(
                MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
        });
        draw(cx);

        assert_gone_for_good(cx);
    }

    /// A shortcut can close the page while the pointer rests on the button.
    #[gpui::test]
    fn a_shown_tooltip_disappears_when_its_button_is_removed(cx: &mut TestAppContext) {
        let (page, cx) = window(cx);
        hover_until_shown(cx);

        close(&page, cx);
        draw(cx);

        assert_gone_for_good(cx);
    }

    #[gpui::test]
    fn a_pending_tooltip_never_shows_once_its_button_is_removed(cx: &mut TestAppContext) {
        let (page, cx) = window(cx);
        let center = button_center(cx);
        cx.simulate_mouse_move(center, None, Modifiers::default());
        draw(cx);
        wait(cx, Duration::from_millis(100));
        assert!(!painted(cx));

        close(&page, cx);
        draw(cx);

        assert_gone_for_good(cx);
    }

    /// The show timer can fire after the update that removes the button and
    /// before the frame that drops it.
    #[gpui::test]
    fn a_tooltip_shown_after_its_button_was_removed_is_never_painted(cx: &mut TestAppContext) {
        let (page, cx) = window(cx);
        let center = button_center(cx);
        cx.simulate_mouse_move(center, None, Modifiers::default());
        draw(cx);

        cx.update(|_, cx| {
            page.update(cx, |page, _| page.open = false);
        });
        cx.executor().advance_clock(LONG);
        draw(cx);

        assert_gone_for_good(cx);
    }

    /// A window drawing its page as a cached view, which GPUI reuses from the
    /// last frame while only the window around it renders again.
    struct Shell {
        page: Entity<Page>,
    }

    impl Render for Shell {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.page
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
        }
    }

    #[gpui::test]
    fn a_tooltip_stays_while_its_button_is_reused_from_the_last_frame(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut shell = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let page = cx.new(|_| Page { open: true });
            let view = cx.new(|_| Shell { page });
            shell = Some(view.clone());
            crate::Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(400.), px(300.)));
        draw(cx);
        let shell = shell.unwrap();
        hover_until_shown(cx);

        for _ in 0..3 {
            cx.update(|_, cx| shell.update(cx, |_, cx| cx.notify()));
            draw(cx);
            assert!(painted(cx), "the reused button keeps its tooltip");
        }
        wait(cx, LONG);
        assert!(painted(cx), "for as long as the pointer rests on it");
    }

    /// Two buttons with tooltips, a pointer's skim apart.
    struct Pair;

    impl Render for Pair {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .p(px(20.))
                .flex()
                .gap(px(80.))
                .child(
                    Button::new("a")
                        .debug_selector(|| "a".into())
                        .label("Alpha")
                        .tooltip("Tip A"),
                )
                .child(
                    Button::new("b")
                        .debug_selector(|| "b".into())
                        .label("Beta")
                        .tooltip("Tip B"),
                )
        }
    }

    /// A window with the pair in a view GPUI reuses from the last frame.
    struct CachedPair {
        pair: Entity<Pair>,
    }

    impl Render for CachedPair {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.pair
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
        }
    }

    /// Moving from one button to the next in a reused view while something
    /// else redraws the window, such as a streaming transcript: the first
    /// tooltip stays through its grace period, and the second shows at once.
    #[gpui::test]
    fn a_redraw_elsewhere_keeps_a_reused_buttons_tooltip_through_its_grace(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut window_view = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let pair = cx.new(|_| Pair);
            let view = cx.new(|_| CachedPair { pair });
            window_view = Some(view.clone());
            crate::Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(500.), px(200.)));
        draw(cx);
        let window_view = window_view.unwrap();
        let a = cx.debug_bounds("a").expect("a").center();
        let b = cx.debug_bounds("b").expect("b").center();
        let gap = point((a.x + b.x) / 2., a.y + px(60.));

        cx.simulate_mouse_move(a, None, Modifiers::default());
        draw(cx);
        wait(cx, LONG);
        assert!(painted(cx), "A's tooltip shows");

        cx.simulate_mouse_move(gap, None, Modifiers::default());
        draw(cx);
        cx.update(|_, cx| window_view.update(cx, |_, cx| cx.notify()));
        draw(cx);
        assert!(painted(cx), "a redraw elsewhere keeps it through its grace");

        wait(cx, Duration::from_millis(50));
        cx.simulate_mouse_move(b, None, Modifiers::default());
        draw(cx);
        assert!(painted(cx), "B's tooltip shows at once");
        wait(cx, LONG);
        assert!(painted(cx), "and stays while the pointer rests on B");
    }

    #[gpui::test]
    fn a_tooltip_hides_after_the_pointer_leaves(cx: &mut TestAppContext) {
        let (_, cx) = window(cx);
        hover_until_shown(cx);

        cx.simulate_mouse_move(point(px(390.), px(290.)), None, Modifiers::default());
        draw(cx);
        assert!(painted(cx), "the tooltip outlasts the pointer briefly");
        wait(cx, LONG);
        assert!(!painted(cx), "then hides");
    }
}
