use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, Div, Element, ElementId, GlobalElementId,
    HitboxBehavior, HitboxId, InspectorElementId, InteractiveElement, IntoElement, LayoutId,
    ParentElement, Pixels, Render, RenderOnce, Role, Stateful, StatefulInteractiveElement, Styled,
    Task, WeakEntity, Window, deferred, div, prelude::FluentBuilder as _, px,
};

use crate::{Placement, Positioner};

const TOOLTIP_PRIORITY: usize = 200;
const WINDOW_MARGIN: Pixels = px(4.);
const GRACE_PERIOD: Duration = Duration::from_millis(300);
const SHOW_DELAY: Duration = Duration::from_millis(500);

type TooltipBuilder = Rc<dyn Fn(&mut Window, &mut App) -> AnyView>;
type TooltipRenderer = Rc<dyn Fn(AnyView, TooltipTransition, &mut Window, &mut App) -> AnyElement>;

/// An unstyled tooltip popup.
///
/// This corresponds to Base UI's `Tooltip.Popup`: it owns the accessible
/// tooltip role and accepts application-owned content and presentation.
#[derive(IntoElement)]
pub struct Tooltip {
    base: Stateful<Div>,
}

impl Tooltip {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id).role(Role::Tooltip),
        }
    }
}

impl Styled for Tooltip {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Tooltip {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(children);
    }
}

impl RenderOnce for Tooltip {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}

/// The element a tooltip belongs to.
///
/// Create one when building the trigger element and call [`Self::prepaint`]
/// from the element's prepaint, every frame. A request made with
/// [`TooltipRequest::for_trigger`] is then drawn only on frames that draw its
/// trigger: the tooltip disappears on the first frame without the trigger, and
/// one still waiting for its show delay never appears.
#[derive(Clone, Default)]
pub struct TooltipTrigger(Rc<RefCell<Option<Rc<TriggerState>>>>);

/// What the overlay knows of a trigger, kept across frames for as long as the
/// trigger element is drawn.
#[derive(Default)]
struct TriggerState {
    bounds: Cell<Bounds<Pixels>>,
    hitbox: Cell<Option<HitboxId>>,
    prepainted: Cell<bool>,
}

impl TooltipTrigger {
    /// A trigger not drawn yet. Keep it with the element it describes, and
    /// hand the same one to [`TooltipRequest::for_trigger`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the trigger's bounds for this frame. Call it from the trigger
    /// element's prepaint, such as through [`crate::ElementExt::on_prepaint`],
    /// inside the trigger element's own element ID: the state that lasts
    /// between frames is kept under that ID, so two triggers prepainted under
    /// one ID would share it.
    pub fn prepaint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        // The trigger is rebuilt on every render, so what lasts between frames
        // lives in element state, under the trigger's own element ID.
        let state = window.with_global_id("tooltip-trigger".into(), |id, window| {
            window.with_element_state::<Rc<TriggerState>, _>(id, |state, _| {
                let state = state.unwrap_or_default();
                (state.clone(), state)
            })
        });
        // A view cached by GPUI replays this hitbox without prepainting the
        // trigger again, which is how a reused trigger is still found.
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        state.bounds.set(bounds);
        state.hitbox.set(Some(hitbox.id));
        state.prepainted.set(true);
        *self.0.borrow_mut() = Some(state);
    }
}

impl TriggerState {
    /// Whether the frame being painted has the trigger in it. Call it once per
    /// frame, during paint: it consumes the mark the trigger's prepaint left.
    fn in_frame(&self, window: &Window) -> bool {
        // A trigger in a reused view was not prepainted, but the pointer
        // still rests on its replayed hitbox.
        self.prepainted.replace(false)
            || self
                .hitbox
                .get()
                .is_some_and(|hitbox| hitbox.should_handle_scroll(window))
    }
}

/// Content requested by a tooltip trigger.
#[derive(Clone)]
pub struct TooltipRequest {
    build: TooltipBuilder,
    trigger_bounds: Bounds<Pixels>,
    preferred_placement: Option<Placement>,
    trigger: Option<Rc<TriggerState>>,
}

impl TooltipRequest {
    /// A request anchored to `trigger_bounds`, shown until it is hidden. Prefer
    /// [`Self::for_trigger`], which also hides the tooltip with its trigger.
    pub fn new(
        trigger_bounds: Bounds<Pixels>,
        build: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        Self {
            build: Rc::new(build),
            trigger_bounds,
            preferred_placement: None,
            trigger: None,
        }
    }

    /// A request anchored to `trigger` and shown only while it is drawn.
    pub fn for_trigger(
        trigger: &TooltipTrigger,
        build: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        // Unset only before the trigger's first prepaint, when nothing can
        // have hovered it yet.
        let state = trigger.0.borrow().clone().unwrap_or_default();
        Self {
            build: Rc::new(build),
            trigger_bounds: state.bounds.get(),
            preferred_placement: None,
            trigger: Some(state),
        }
    }

    /// Clears the trigger's mark from frames before this request was shown.
    fn begin_showing(&self) {
        if let Some(trigger) = &self.trigger {
            trigger.prepainted.set(false);
        }
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.preferred_placement = Some(placement);
        self
    }
}

/// Presentation transition requested by the Base tooltip lifecycle.
#[derive(Clone, Copy, Debug)]
pub enum TooltipTransition {
    Enter {
        epoch: usize,
    },
    Switch {
        epoch: usize,
        previous: Bounds<Pixels>,
        current: Bounds<Pixels>,
    },
}

/// Per-window tooltip provider and overlay.
///
/// Show requests are ignored on iOS and Android, where touch input must not
/// open hover tooltips. This does not control GPUI's native `.tooltip()` API.
pub struct TooltipOverlay {
    enabled: bool,
    content: Option<TooltipRequest>,
    previous_bounds: Option<Bounds<Pixels>>,
    /// The trigger of the latest show request, until the pointer leaves it.
    hovered: Option<Rc<TriggerState>>,
    epoch: usize,
    had_recent_tooltip: bool,
    animation_epoch: usize,
    is_switching: bool,
    show_task: Option<Task<()>>,
    hide_task: Option<Task<()>>,
    renderer: TooltipRenderer,
}

impl TooltipOverlay {
    pub fn new() -> Self {
        Self {
            enabled: !crate::is_mobile(),
            content: None,
            previous_bounds: None,
            hovered: None,
            epoch: 0,
            had_recent_tooltip: false,
            animation_epoch: 0,
            is_switching: false,
            show_task: None,
            hide_task: None,
            renderer: Rc::new(|view, _, _, _| div().child(view).into_any_element()),
        }
    }

    pub fn render_with(
        mut self,
        renderer: impl Fn(AnyView, TooltipTransition, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.renderer = Rc::new(renderer);
        self
    }

    fn next_epoch(&mut self) -> usize {
        self.epoch += 1;
        self.epoch
    }

    pub fn request_show(
        &mut self,
        content: TooltipRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Gate both delayed display and the immediate grace-period switch.
        // Keep this in Base so every managed component shares the policy.
        if !self.enabled {
            return;
        }
        self.hide_task = None;
        self.hovered = content.trigger.clone();
        let was_visible = self.content.is_some();
        if was_visible || self.had_recent_tooltip {
            self.previous_bounds = self.content.as_ref().map(|content| content.trigger_bounds);
            content.begin_showing();
            self.content = Some(content);
            self.show_task = None;
            self.is_switching = was_visible;
            self.animation_epoch += 1;
            cx.notify();
            return;
        }

        let epoch = self.next_epoch();
        self.show_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SHOW_DELAY).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.epoch == epoch {
                    content.begin_showing();
                    this.content = Some(content);
                    this.previous_bounds = None;
                    this.is_switching = false;
                    this.animation_epoch += 1;
                    cx.notify();
                }
            });
        }));
    }

    /// Hides the tooltip after the grace period, as [`Self::request_hide`]
    /// does, when the pointer leaves `trigger`. GPUI may report the pointer
    /// entering the next trigger before leaving this one, and then the hide is
    /// left to the next trigger.
    pub fn request_hide_for(
        &mut self,
        trigger: &TooltipTrigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved_on = self.hovered.as_ref().is_some_and(|hovered| {
            !trigger
                .0
                .borrow()
                .as_ref()
                .is_some_and(|left| Rc::ptr_eq(left, hovered))
        });
        if !moved_on {
            self.request_hide(window, cx);
        }
    }

    pub fn request_hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_task = None;
        self.hovered = None;
        if self.content.is_none() {
            return;
        }
        let epoch = self.next_epoch();
        self.had_recent_tooltip = true;
        self.hide_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(GRACE_PERIOD).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.epoch == epoch {
                    this.content = None;
                    this.previous_bounds = None;
                    this.had_recent_tooltip = false;
                    cx.notify();
                }
            });
        }));
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        let changed = self.content.is_some()
            || self.previous_bounds.is_some()
            || self.had_recent_tooltip
            || self.show_task.is_some()
            || self.hide_task.is_some();
        self.content = None;
        self.previous_bounds = None;
        self.hovered = None;
        self.had_recent_tooltip = false;
        self.is_switching = false;
        self.show_task = None;
        self.hide_task = None;
        if changed {
            cx.notify();
        }
    }

    /// Hides the tooltip if it still belongs to `trigger`, which is gone.
    fn trigger_left(&mut self, trigger: &Rc<TriggerState>, cx: &mut Context<Self>) {
        let shown_for_trigger = self
            .content
            .as_ref()
            .and_then(|content| content.trigger.as_ref())
            .is_some_and(|shown| Rc::ptr_eq(shown, trigger));
        if shown_for_trigger {
            self.hide(cx);
        }
    }
}

impl Default for TooltipOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for TooltipOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(content) = self.content.as_ref() else {
            return div().into_any_element();
        };
        let view = (content.build)(window, cx);
        let transition = match (self.is_switching, self.previous_bounds) {
            (true, Some(previous)) => TooltipTransition::Switch {
                epoch: self.animation_epoch,
                previous,
                current: content.trigger_bounds,
            },
            _ => TooltipTransition::Enter {
                epoch: self.animation_epoch,
            },
        };
        let rendered = (self.renderer)(view, transition, window, cx);
        let positioned = TooltipPositioner::new(content.trigger_bounds)
            .when_some(content.preferred_placement, |this, placement| {
                this.placement(placement)
            })
            .child(rendered)
            .into_any_element();
        deferred(TriggerGate {
            trigger: content.trigger.clone(),
            leaving: self.hide_task.is_some(),
            overlay: cx.entity().downgrade(),
            child: positioned,
        })
        .with_priority(TOOLTIP_PRIORITY)
        .into_any_element()
    }
}

/// Paints the tooltip only on frames that have its trigger.
///
/// Deferred draws are painted after every element has been prepainted, and
/// after GPUI has hit-tested the new frame, so by then the trigger has either
/// marked itself or is missing from the frame.
///
/// Once the pointer has left the trigger, the tooltip is in its grace period
/// and the grace timer ends it. A trigger in a view GPUI reuses from the last
/// frame is then found by nothing (it isn't prepainted, and the pointer is no
/// longer on its hitbox), so a frame drawn for something else mustn't take it
/// for gone: that would hide the tooltip at once and cost the next trigger
/// its immediate switch.
struct TriggerGate {
    trigger: Option<Rc<TriggerState>>,
    /// The overlay is in its grace period: the pointer left the trigger.
    leaving: bool,
    overlay: WeakEntity<TooltipOverlay>,
    child: AnyElement,
}

impl IntoElement for TriggerGate {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TriggerGate {
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
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
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
        let Some(trigger) = self.trigger.clone() else {
            self.child.paint(window, cx);
            return;
        };
        if trigger.in_frame(window) || self.leaving {
            self.child.paint(window, cx);
            return;
        }
        let overlay = self.overlay.clone();
        window.defer(cx, move |_, cx| {
            overlay
                .update(cx, |overlay, cx| overlay.trigger_left(&trigger, cx))
                .ok();
        });
    }
}

/// An unstyled tooltip positioner with viewport-aware flipping and clamping.
///
/// This is a tooltip-named view of [`crate::Positioner`]'s side placement. It
/// adds no element of its own; the shared positioner is what gets rendered.
pub struct TooltipPositioner(Positioner);

impl TooltipPositioner {
    pub fn new(trigger_bounds: Bounds<Pixels>) -> Self {
        Self(Positioner::side(trigger_bounds).margin(WINDOW_MARGIN))
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.0 = self.0.placement(placement);
        self
    }
}

impl ParentElement for TooltipPositioner {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.0.extend(elements);
    }
}

impl IntoElement for TooltipPositioner {
    type Element = Positioner;

    fn into_element(self) -> Self::Element {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, point, size};

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[gpui::test]
    fn provider_owns_grace_switch_and_dismiss(cx: &mut gpui::TestAppContext) {
        let state = cx.update(|cx| cx.new(|_| TooltipOverlay::new()));
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            state.update(cx, |tooltip, cx| {
                tooltip.had_recent_tooltip = true;
                tooltip.request_show(
                    TooltipRequest::new(bounds(0., 0., 20., 20.), |_, _| {
                        panic!("content is not rendered by this lifecycle test")
                    }),
                    window,
                    cx,
                );
            });
        });
        cx.update(|_, cx| assert!(state.read(cx).content.is_some()));

        cx.update(|_, cx| {
            state.update(cx, |tooltip, cx| tooltip.hide(cx));
        });
        cx.update(|_, cx| assert!(state.read(cx).content.is_none()));
    }

    #[test]
    fn tooltip_priority_exceeds_popup_layer() {
        assert!(TOOLTIP_PRIORITY > crate::POPUP_PRIORITY);
    }

    #[gpui::test]
    fn disabled_provider_ignores_delayed_and_immediate_requests(cx: &mut gpui::TestAppContext) {
        let state = cx.update(|cx| {
            cx.new(|_| TooltipOverlay {
                enabled: false,
                ..TooltipOverlay::new()
            })
        });
        let cx = cx.add_empty_window();
        for had_recent_tooltip in [false, true] {
            cx.update(|window, cx| {
                state.update(cx, |tooltip, cx| {
                    tooltip.had_recent_tooltip = had_recent_tooltip;
                    tooltip.request_show(
                        TooltipRequest::new(bounds(0., 0., 20., 20.), |_, _| {
                            panic!("disabled tooltips must not build content")
                        }),
                        window,
                        cx,
                    );
                    assert!(tooltip.content.is_none());
                    assert!(tooltip.show_task.is_none());
                    assert!(tooltip.hide_task.is_none());
                    assert_eq!(tooltip.animation_epoch, 0);
                });
            });
        }
    }
}
