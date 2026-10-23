use crate::root::WindowState;
use std::{rc::Rc, time::Duration};

use gpui::{
    Action, AnyElement, AnyView, App, AppContext, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, Entity, GlobalElementId, InspectorElementId, InteractiveElement,
    IntoElement, LayoutId, MouseButton, ParentElement, Pixels, Refineable as _, Render,
    SharedString, StatefulInteractiveElement, Style, StyleRefinement, Styled, Window, div, point,
    prelude::FluentBuilder, px, transparent_black,
};
use gpui_base::{
    Interpolate as _, Sequence, Tooltip as BaseTooltip, TooltipOverlay as BaseTooltipOverlay,
    TooltipRequest as BaseTooltipRequest, TooltipTransition as BaseTooltipTransition,
    TooltipTrigger, Transition,
};

use crate::{
    ActiveTheme, Placement, StyledExt,
    animation::{EffectTransition, ease_out_cubic},
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
    /// Shown by the tooltip overlay, which draws the box itself.
    in_overlay: bool,
}

impl Tooltip {
    /// Create a Tooltip with a text content.
    pub fn new(text: impl Into<Text>) -> Self {
        Self {
            style: StyleRefinement::default(),
            content: TooltipContext::Text(text.into()),
            key_binding: None,
            action: None,
            in_overlay: false,
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
            in_overlay: false,
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

    /// The style of the tooltip's box, for the overlay to draw it.
    fn box_style(&self, cx: &App) -> Style {
        let mut element = tooltip_box(div(), cx).refine_style(&self.style);
        let mut style = Style::default();
        style.refine(element.style());
        style
    }
}

/// The tooltip's box, with its padding, background, border, shadow and corners.
fn tooltip_box<E: Styled>(element: E, cx: &App) -> E {
    element
        .py_0p5()
        .px_2()
        .bg(cx.theme().tokens.popover)
        .border_1()
        .border_color(cx.theme().border)
        .shadow_md()
        .rounded(cx.theme().radius)
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

        let popup = tooltip_box(BaseTooltip::new("tooltip-popup"), cx)
            .h_flex()
            .font_family(cx.theme().font_family.clone())
            .when(!self.in_overlay, |this| this.m_3())
            .text_color(cx.theme().popover_foreground)
            .justify_between()
            .text_sm()
            .gap_3()
            .refine_style(&self.style)
            .when(self.in_overlay, |this| {
                this.bg(transparent_black())
                    .border_color(transparent_black())
                    .shadow(Vec::new())
            })
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
            });
        if self.in_overlay {
            popup.into_any_element()
        } else {
            // Wrap in a child, to ensure the left margin is applied to the tooltip
            div().child(popup).into_any_element()
        }
    }
}

// ── Managed tooltip system ──────────────────────────────────────────────────

/// Duration of the slide-down enter animation.
const ENTER_DURATION: Duration = Duration::from_millis(150);
/// How long a switch along a row moves the box. Shorter than the theme's fast
/// duration, because the new text can be read only once the box has
/// uncovered it.
const SWITCH_DURATION: Duration = Duration::from_millis(80);

pub(crate) fn render_tooltip(
    content_view: AnyView,
    transition: BaseTooltipTransition,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    // The selector lets tests see whether a tooltip was painted.
    div()
        .debug_selector(|| "tooltip".into())
        .map(|this| match content_view.downcast::<Tooltip>() {
            Ok(tooltip) => this
                .p_3()
                .child(TooltipBox::new(tooltip, transition, window, cx)),
            Err(view) => this.child(view),
        })
        .map(|element| match transition {
            BaseTooltipTransition::Switch { .. } => element.into_any_element(),
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

/// What a switch hands over from the tooltip drawn before it.
#[derive(Default)]
struct Handover {
    epoch: usize,
    /// The view drawn last frame and its box as laid out.
    drawn: Option<(AnyView, Bounds<Pixels>)>,
    /// The box painted last frame, which a switch moves on from.
    painted: Option<Bounds<Pixels>>,
    /// The tooltip being replaced and the box it was laid out in.
    outgoing: Option<(AnyView, Bounds<Pixels>)>,
    /// The box the switch moves from.
    from: Option<Bounds<Pixels>>,
}

/// A tooltip of the kit's own, whose box the overlay draws.
///
/// On a switch along a row, the box moves and resizes from the last tooltip's
/// to this one's, on whole device pixels. The texts never move. The old one
/// stays for the switch's first frame, until the new one is laid out, and then
/// the new one shows whole at its place, clipped by the box, which uncovers it
/// as it moves.
struct TooltipBox {
    tooltip: Entity<Tooltip>,
    content: AnyElement,
    outgoing: Option<(AnyElement, Bounds<Pixels>)>,
    from: Option<Bounds<Pixels>>,
    progress: f32,
    handover: Entity<Handover>,
    painted: Bounds<Pixels>,
}

impl TooltipBox {
    fn new(
        tooltip: Entity<Tooltip>,
        transition: BaseTooltipTransition,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        tooltip.update(cx, |tooltip, _| tooltip.in_overlay = true);
        let (epoch, same_row) = match transition {
            BaseTooltipTransition::Enter { epoch } => (epoch, false),
            BaseTooltipTransition::Switch {
                epoch,
                previous,
                current,
            } => (
                epoch,
                (current.origin.y - previous.origin.y).abs() < px(10.),
            ),
        };
        let handover = window.use_keyed_state("tooltip-handover", cx, |_, _| Handover::default());
        handover.update(cx, |handover, _| {
            if handover.epoch != epoch {
                handover.epoch = epoch;
                handover.outgoing = handover.drawn.take().filter(|_| same_row);
                handover.from = handover.painted.filter(|_| same_row);
            }
        });
        let (outgoing, from, to) = {
            let handover = handover.read(cx);
            let to = handover.drawn.as_ref().map(|(_, bounds)| *bounds);
            (handover.outgoing.clone(), handover.from, to)
        };
        let progress = match from {
            Some(_) => Sequence::new(
                ElementId::NamedInteger("tooltip-switch".into(), epoch as u64),
                0.0,
            )
            // The box eases with `ease_out_cubic` as it is painted.
            .with_step(1.0, Transition::new(SWITCH_DURATION).ease(|t| t))
            .sample(window, cx)
            .into_value(),
            None => 1.0,
        };
        // The old text stays for the switch's first frame, and the new one
        // shows from the second on, once it has been laid out.
        let laid_out = progress >= 1.0 || to.is_some();
        Self {
            content: div()
                .opacity(if laid_out { 1.0 } else { 0.0 })
                .child(tooltip.clone())
                .into_any_element(),
            outgoing: outgoing
                .filter(|_| !laid_out)
                .map(|(view, bounds)| (div().child(view).into_any_element(), bounds)),
            from: from.filter(|_| progress < 1.0),
            tooltip,
            progress,
            handover,
            painted: Bounds::default(),
        }
    }
}

impl IntoElement for TooltipBox {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TooltipBox {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.content.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.content.prepaint(window, cx);
        let shown = self.from.map_or(bounds, |from| {
            from.interpolate(&bounds, ease_out_cubic(self.progress))
        });
        // Whole device pixels keep the box's edges crisp while it moves.
        let scale = window.scale_factor();
        let snap = |value: Pixels| px((f32::from(value) * scale).round() / scale);
        self.painted = Bounds::from_corners(
            point(snap(shown.left()), snap(shown.top())),
            point(snap(shown.right()), snap(shown.bottom())),
        );
        if let Some((outgoing, laid_out)) = &mut self.outgoing {
            window.with_element_namespace("outgoing", |window| {
                outgoing.prepaint_as_root(
                    laid_out.origin,
                    laid_out.size.map(AvailableSpace::Definite),
                    window,
                    cx,
                );
            });
        }
        let (view, painted) = (AnyView::from(self.tooltip.clone()), self.painted);
        self.handover.update(cx, |handover, _| {
            handover.drawn = Some((view, bounds));
            handover.painted = Some(painted);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self.tooltip.read(cx).box_style(cx);
        style.paint(self.painted, window, cx, |_, _| {});
        let mask = ContentMask {
            bounds: self.painted,
        };
        window.with_content_mask(Some(mask), |window| {
            if let Some((outgoing, _)) = &mut self.outgoing {
                window.with_element_namespace("outgoing", |window| outgoing.paint(window, cx));
            }
            self.content.paint(window, cx);
        });
    }
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
                            o.request_hide_for(&trigger, window, cx);
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

    use crate::{
        IconName, Sizable as _,
        button::{Button, ButtonVariants as _},
    };

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
    /// else redraws the window, such as streaming text: the first
    /// tooltip stays through its grace period and until the second replaces
    /// it.
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
        cx.update(|_, cx| window_view.update(cx, |_, cx| cx.notify()));
        draw(cx);
        assert!(painted(cx), "A's tooltip stays until B's replaces it");
        wait(cx, LONG);
        assert!(
            shows_tooltip_of("b", cx),
            "B's shows and stays while the pointer rests on B"
        );
    }

    /// Icon buttons side by side, as under a message.
    struct Actions;

    impl Render for Actions {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().p(px(20.)).pl(px(200.)).flex().children(
                ["Edit", "Branch", "Copy"].map(|name| {
                    Button::new(name)
                        .debug_selector(move || name.into())
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .tooltip(name)
                }),
            )
        }
    }

    /// Whether the tooltip painted is the one of the button named `name`.
    fn shows_tooltip_of(name: &'static str, cx: &mut VisualTestContext) -> bool {
        let button = cx.debug_bounds(name).expect("the button is painted");
        cx.debug_bounds("tooltip")
            .is_some_and(|tooltip| (tooltip.center().x - button.center().x).abs() < px(1.))
    }

    /// A window with the icon buttons, and motion as `reduce_motion` says.
    fn actions(cx: &mut TestAppContext, reduce_motion: bool) -> &mut VisualTestContext {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(reduce_motion);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Actions);
            crate::Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(500.), px(200.)));
        draw(cx);
        cx
    }

    /// Moves the pointer onto the middle of the button named `name`.
    fn point_at(name: &'static str, cx: &mut VisualTestContext) -> Point<Pixels> {
        let center = cx
            .debug_bounds(name)
            .expect("the button is painted")
            .center();
        cx.simulate_mouse_move(center, None, Modifiers::default());
        draw(cx);
        center
    }

    /// GPUI reports the pointer entering the next button before it reports the
    /// pointer leaving the last one when the pointer moves right.
    #[gpui::test]
    fn moving_along_adjacent_buttons_shows_each_ones_tooltip(cx: &mut TestAppContext) {
        let cx = actions(cx, true);

        for name in ["Edit", "Branch", "Copy", "Branch", "Edit"] {
            point_at(name, cx);
            wait(cx, LONG);
            assert!(
                shows_tooltip_of(name, cx),
                "{name}'s tooltip shows while the pointer rests on it"
            );
        }
    }

    /// Hiding never waits for the switch's animation.
    #[gpui::test]
    fn a_click_during_a_switch_hides_the_tooltip_at_once(cx: &mut TestAppContext) {
        let cx = actions(cx, false);
        point_at("Edit", cx);
        wait(cx, LONG);
        let branch = point_at("Branch", cx);
        // The switch delay, then partway through the switch.
        wait(cx, Duration::from_millis(100));
        wait(cx, Duration::from_millis(30));
        assert!(painted(cx), "the tooltip is switching");

        cx.simulate_mouse_down(branch, MouseButton::Left, Modifiers::default());
        draw(cx);
        assert!(!painted(cx), "the next frame has no tooltip");
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
