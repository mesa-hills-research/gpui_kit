//! A window for comparing ways a tooltip's text can change when the pointer moves from a button to
//! its neighbour. Each row draws the switch one way. Rest the pointer on a button until its tooltip
//! shows, then move along the row.
//!
//! ```sh
//! cargo run --release -p tooltip_lab
//! ```
//!
//! The rows share one tooltip overlay, whose renderer draws the box and the texts the way the row's
//! variant says. Row A draws what the kit's own tooltips draw.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui_kit::{
    AbsoluteLength, AnyElement, AnyView, App, AppContext as _, AvailableSpace, Bounds, ContentMask,
    Context, Element, ElementId, Entity, GlobalElementId, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, MouseButton, ParentElement as _, Pixels,
    Refineable as _, Render, SharedString, StatefulInteractiveElement as _, Style, Styled,
    Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions,
    assets::{AllAssets, IconName},
    base::{
        ElementExt as _, Interpolate as _, Sequence, Tooltip as TooltipPopup, TooltipOverlay,
        TooltipRequest, TooltipTransition, TooltipTrigger, Transition,
        animation::{EffectTransition, ease_in_out_cubic, ease_out_cubic},
    },
    component::{
        ActiveTheme as _, Sizable as _, StyledExt as _, Theme, ThemeMode,
        button::{Button, ButtonVariants as _},
        h_flex,
        slider::{Slider, SliderEvent, SliderState},
        switch::Switch,
        v_flex,
    },
    div, point, px, size, transparent_black,
};

/// How long a tooltip takes to appear, as in the kit.
const ENTER_DURATION: Duration = Duration::from_millis(150);
/// The part of today's switch in which the old text fades out, as in the kit.
const SWITCH_FADE_OUT: f32 = 0.3;
/// The part of the cross-fade in which the old text fades out, fast at first. The new text starts
/// fading in halfway through it, when the old one is down to an eighth.
const CROSS_FADE_OUT: f32 = 0.3;
/// How much slow motion stretches a switch.
const SLOW_MOTION: u32 = 5;

/// The buttons of each row, like the actions under a message.
const ACTIONS: [(IconName, &str); 4] = [
    (IconName::SquarePen, "Edit"),
    (IconName::GitBranch, "Branch"),
    (IconName::Copy, "Copy"),
    (
        IconName::GitFork,
        "Branch into a new session with the conversation before this message",
    ),
];

/// A way to draw a switch between neighbouring tooltips.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Variant {
    Today,
    Swap,
    CrossFade,
    Slide,
    Wipe,
    Instant,
}

impl Variant {
    const ALL: [Self; 6] = [
        Self::Today,
        Self::Swap,
        Self::CrossFade,
        Self::Slide,
        Self::Wipe,
        Self::Instant,
    ];

    fn letter(self) -> &'static str {
        match self {
            Self::Today => "A",
            Self::Swap => "B",
            Self::CrossFade => "C",
            Self::Slide => "D",
            Self::Wipe => "E",
            Self::Instant => "F",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Swap => "Swap",
            Self::CrossFade => "Cross-fade",
            Self::Slide => "Slide",
            Self::Wipe => "Wipe",
            Self::Instant => "Instant",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Today => {
                "The old text fades out, then the new one fades in once the box holds it."
            }
            Self::Swap => "The text changes at once when the box holds the new one.",
            Self::CrossFade => "The old text fades out fast while the new one fades in.",
            Self::Slide => "The new tooltip slides into place on whole pixels.",
            Self::Wipe => "The moving box uncovers the new text.",
            Self::Instant => "The new tooltip replaces the old one at once.",
        }
    }
}

/// The opacities of the old text and the new one, `progress` into a switch (0 to 1, linear in
/// time). `old_end` is the progress at which the box stops holding the old text whole, and
/// `new_start` the one at which it starts holding the new text whole. Both are unknown on the
/// switch's first frame, before the new text is laid out.
fn texts(
    variant: Variant,
    progress: f32,
    old_end: Option<f32>,
    new_start: Option<f32>,
) -> (f32, f32) {
    if progress >= 1.0 {
        return (0.0, 1.0);
    }
    match variant {
        Variant::Today => {
            let end = old_end.map_or(SWITCH_FADE_OUT, |end| SWITCH_FADE_OUT.min(end));
            let old = if progress < end {
                1.0 - progress / end
            } else {
                0.0
            };
            let new = new_start.map_or(0.0, |start| {
                let start = SWITCH_FADE_OUT.max(start);
                ease_out_cubic((progress - start) / (1.0 - start))
            });
            (old, new)
        }
        Variant::Swap => match new_start {
            Some(start) if progress >= start => (0.0, 1.0),
            _ => (1.0, 0.0),
        },
        Variant::CrossFade => {
            let fade_in_start = CROSS_FADE_OUT / 2.0;
            (
                1.0 - ease_out_cubic(progress / CROSS_FADE_OUT),
                ease_out_cubic((progress - fade_in_start) / (1.0 - fade_in_start)),
            )
        }
        Variant::Wipe => match new_start {
            Some(_) => (0.0, 1.0),
            None => (1.0, 0.0),
        },
        Variant::Slide | Variant::Instant => (0.0, 1.0),
    }
}

/// The tooltip's box, styled as the kit's.
fn tip_box<E: Styled>(element: E, cx: &App) -> E {
    element
        .py_0p5()
        .px_2()
        .bg(cx.theme().tokens.popover)
        .border_1()
        .border_color(cx.theme().border)
        .shadow_md()
        .rounded(cx.theme().radius)
}

fn box_style(cx: &App) -> Style {
    let mut element = tip_box(div(), cx);
    let mut style = Style::default();
    style.refine(element.style());
    style
}

/// A tooltip's text, and how its row switches. The overlay draws its box.
struct Tip {
    variant: Variant,
    label: SharedString,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tip_box(TooltipPopup::new("tooltip-popup"), cx)
            .h_flex()
            .font_family(cx.theme().font_family.clone())
            .text_color(cx.theme().popover_foreground)
            .text_sm()
            .bg(transparent_black())
            .border_color(transparent_black())
            .shadow(Vec::new())
            .child(div().child(self.label.clone()))
    }
}

/// The overlay's renderer, the kit's own with the switch drawn as the tooltip's row says.
fn render_tooltip(
    view: AnyView,
    transition: TooltipTransition,
    duration: Duration,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let element = match view.downcast::<Tip>() {
        Ok(tip) => div()
            .p_3()
            .child(TipBox::new(tip, transition, duration, window, cx)),
        Err(view) => div().child(view),
    };
    match transition {
        TooltipTransition::Switch { .. } => element.into_any_element(),
        TooltipTransition::Enter { epoch } => EffectTransition::new(ENTER_DURATION)
            .ease(ease_out_cubic)
            .slide_y(px(4.), px(0.))
            .fade(0.0, 1.0)
            .apply(
                element,
                ElementId::NamedInteger("tooltip-enter".into(), epoch as u64),
            )
            .into_any_element(),
    }
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

/// A tooltip whose box the overlay draws. On a switch along a row, the box moves and resizes from
/// the last tooltip's to this one's on whole device pixels, and the texts change as the row's
/// variant says, clipped by the box.
struct TipBox {
    tip: Entity<Tip>,
    content: AnyElement,
    outgoing: Option<(AnyElement, Bounds<Pixels>)>,
    /// The box the switch moves from, while it moves.
    from: Option<Bounds<Pixels>>,
    progress: f32,
    /// How far left of its place the sliding tooltip is drawn this frame.
    slide: Pixels,
    handover: Entity<Handover>,
    painted: Bounds<Pixels>,
}

impl TipBox {
    fn new(
        tip: Entity<Tip>,
        transition: TooltipTransition,
        duration: Duration,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let variant = tip.read(cx).variant;
        let (epoch, same_row, shift) = match transition {
            TooltipTransition::Enter { epoch } => (epoch, false, px(0.)),
            TooltipTransition::Switch {
                epoch,
                previous,
                current,
            } => (
                epoch,
                (current.origin.y - previous.origin.y).abs() < px(10.),
                previous.center().x - current.center().x,
            ),
        };
        let animated = same_row && variant != Variant::Instant;
        let handover = window.use_keyed_state("tooltip-handover", cx, |_, _| Handover::default());
        handover.update(cx, |handover, _| {
            if handover.epoch != epoch {
                handover.epoch = epoch;
                handover.outgoing = handover.drawn.take().filter(|_| animated);
                handover.from = handover.painted.filter(|_| animated);
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
            // The box eases with `ease_out_cubic`, which `progress_at` inverts.
            .with_step(1.0, Transition::new(duration).ease(|t| t))
            .sample(window, cx)
            .into_value(),
            None => 1.0,
        };
        // A text counts as held while the box holds it with half its padding around, so no
        // frame cuts a glyph at the box's edge.
        let padding = box_style(cx).padding;
        let rem_size = window.rem_size();
        let text = |bounds: Bounds<Pixels>| {
            let padding = padding.to_pixels(bounds.size.map(AbsoluteLength::Pixels), rem_size);
            Bounds::from_corners(
                point(
                    bounds.left() + padding.left / 2.,
                    bounds.top() + padding.top / 2.,
                ),
                point(
                    bounds.right() - padding.right / 2.,
                    bounds.bottom() - padding.bottom / 2.,
                ),
            )
        };
        let holds = |region: Bounds<Pixels>| {
            from.zip(to)
                .map(|(from, to)| box_holds(from, to, text(region)))
        };
        let new_start = to.and_then(holds).map(|(start, _)| progress_at(start));
        let old_end = outgoing
            .as_ref()
            .and_then(|(_, bounds)| holds(*bounds))
            .map(|(start, end)| if start > 0.0 { 0.0 } else { progress_at(end) });
        let (old_opacity, new_opacity) = match from {
            Some(_) => texts(variant, progress, old_end, new_start),
            None => (0.0, 1.0),
        };
        let sliding = variant == Variant::Slide && progress < 1.0;
        Self {
            content: div()
                .opacity(new_opacity)
                .child(tip.clone())
                .into_any_element(),
            outgoing: outgoing
                .filter(|_| old_opacity > 0.0)
                .map(|(view, bounds)| {
                    (
                        div().opacity(old_opacity).child(view).into_any_element(),
                        bounds,
                    )
                }),
            from: from.filter(|_| progress < 1.0 && variant != Variant::Slide),
            progress,
            slide: match from {
                Some(_) if sliding => shift * (1.0 - ease_in_out_cubic(progress)),
                _ => px(0.),
            },
            tip,
            handover,
            painted: Bounds::default(),
        }
    }
}

/// The span of the box's eased progress, within 0 to 1, in which the box moving from `from` to
/// `to` holds `region`. Each edge moves linearly with the eased progress, so each holds the region
/// on one side of a threshold.
fn box_holds(from: Bounds<Pixels>, to: Bounds<Pixels>, region: Bounds<Pixels>) -> (f32, f32) {
    let edges = [
        (from.left(), to.left(), region.left(), -1.0),
        (from.top(), to.top(), region.top(), -1.0),
        (from.right(), to.right(), region.right(), 1.0),
        (from.bottom(), to.bottom(), region.bottom(), 1.0),
    ];
    edges.into_iter().fold(
        (0.0_f32, 1.0_f32),
        |(start, end), (from, to, limit, outward)| {
            // The edge holds the region where `held + gain * eased >= 0`.
            let held = outward * f32::from(from - limit);
            let gain = outward * f32::from(to - from);
            if gain > 0. {
                (start.max(-held / gain), end)
            } else if gain < 0. {
                (start, end.min(-held / gain))
            } else if held < 0. {
                (1.0, 0.0)
            } else {
                (start, end)
            }
        },
    )
}

/// The switch's progress at which its `ease_out_cubic` reaches `eased`.
fn progress_at(eased: f32) -> f32 {
    1.0 - (1.0 - eased.clamp(0.0, 1.0)).cbrt()
}

impl IntoElement for TipBox {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TipBox {
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
        let scale = window.scale_factor();
        let snap = |value: Pixels| px((f32::from(value) * scale).round() / scale);
        // A slide moves by whole device pixels, so its text is drawn as it is at rest.
        let offset = point(snap(self.slide), px(0.));
        window.with_element_offset(offset, |window| self.content.prepaint(window, cx));
        let shown = self.from.map_or(bounds + offset, |from| {
            from.interpolate(&bounds, ease_out_cubic(self.progress))
        });
        // Whole device pixels keep the box's edges crisp while it moves.
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
        let (view, painted) = (AnyView::from(self.tip.clone()), self.painted);
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
        box_style(cx).paint(self.painted, window, cx, |_, _| {});
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

struct TooltipLab {
    overlay: Entity<TooltipOverlay>,
    duration: Entity<SliderState>,
    /// The slider's switch duration, in milliseconds.
    duration_ms: f32,
    slow_motion: bool,
    /// The switch duration in effect, which the overlay's renderer reads.
    switch_duration: Rc<Cell<Duration>>,
    _duration_changes: Subscription,
}

impl TooltipLab {
    fn new(cx: &mut Context<Self>) -> Self {
        let fast = cx.theme().motion_tokens().duration_fast;
        let switch_duration = Rc::new(Cell::new(fast));
        let overlay = cx.new(|_| {
            let switch_duration = switch_duration.clone();
            TooltipOverlay::new().render_with(move |view, transition, window, cx| {
                render_tooltip(view, transition, switch_duration.get(), window, cx)
            })
        });
        let duration_ms = fast.as_millis() as f32;
        let duration = cx.new(|_| {
            SliderState::new()
                .min(40.)
                .max(600.)
                .step(10.)
                .default_value(duration_ms)
        });
        let _duration_changes = cx.subscribe(&duration, |this, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(value) = event {
                this.duration_ms = value.start();
                this.apply(cx);
            }
        });
        Self {
            overlay,
            duration,
            duration_ms,
            slow_motion: false,
            switch_duration,
            _duration_changes,
        }
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let factor = if self.slow_motion { SLOW_MOTION } else { 1 };
        self.switch_duration
            .set(Duration::from_secs_f32(self.duration_ms / 1000.) * factor);
        cx.notify();
    }

    fn row(&self, row: usize, variant: Variant, cx: &App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .h(px(56.))
            .gap_4()
            .child(
                h_flex()
                    .w(px(140.))
                    .flex_none()
                    .gap_2()
                    .child(div().font_semibold().child(variant.letter()))
                    .child(div().text_sm().child(variant.name())),
            )
            .child(
                h_flex()
                    .flex_none()
                    .children(ACTIONS.iter().enumerate().map(|(ix, (icon, label))| {
                        self.action(row * ACTIONS.len() + ix, *icon, label, variant, muted)
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(variant.description()),
            )
    }

    /// A button whose tooltip the lab's overlay shows, as a kit button's tooltip shows in the
    /// kit's overlay.
    fn action(
        &self,
        id: usize,
        icon: IconName,
        label: &'static str,
        variant: Variant,
        color: Hsla,
    ) -> impl IntoElement {
        let trigger = TooltipTrigger::new();
        let overlay = self.overlay.clone();
        div()
            .id(("action", id))
            .on_prepaint({
                let trigger = trigger.clone();
                move |bounds, window, _| trigger.prepaint(bounds, window)
            })
            .on_hover({
                let overlay = overlay.clone();
                move |hovered, window, cx| {
                    overlay.update(cx, |overlay, cx| {
                        if *hovered {
                            let request = TooltipRequest::for_trigger(&trigger, move |_, cx| {
                                AnyView::from(cx.new(|_| Tip {
                                    variant,
                                    label: label.into(),
                                }))
                            });
                            overlay.request_show(request, window, cx);
                        } else {
                            overlay.request_hide_for(&trigger, window, cx);
                        }
                    })
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                overlay.update(cx, |overlay, cx| overlay.hide(cx))
            })
            .child(
                Button::new(("button", id))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .text_color(color),
            )
    }
}

impl Render for TooltipLab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let dark = theme.mode.is_dark();
        v_flex()
            .size_full()
            .p(px(20.))
            .gap_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_lg().font_semibold().child("Tooltip lab"))
            .child(
                h_flex()
                    .gap_6()
                    .flex_wrap()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().text_sm().child("Duration"))
                            .child(div().w(px(180.)).child(Slider::new(&self.duration)))
                            .child(
                                div()
                                    .w(px(56.))
                                    .text_sm()
                                    .child(format!("{} ms", self.duration_ms)),
                            ),
                    )
                    .child(
                        Switch::new("slow-motion")
                            .label(format!("Slow motion ×{SLOW_MOTION}"))
                            .checked(self.slow_motion)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.slow_motion = *checked;
                                this.apply(cx);
                            })),
                    )
                    .child(
                        Switch::new("reduce-motion")
                            .label("Reduce motion")
                            .checked(cx.reduce_motion())
                            .on_click(|checked, _, cx| cx.set_reduce_motion(*checked)),
                    )
                    .child(Switch::new("dark").label("Dark").checked(dark).on_click(
                        |checked, window, cx| {
                            let mode = if *checked {
                                ThemeMode::Dark
                            } else {
                                ThemeMode::Light
                            };
                            Theme::change(mode, Some(window), cx);
                        },
                    )),
            )
            .child(
                v_flex().children(
                    Variant::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(row, variant)| self.row(row, variant, cx)),
                ),
            )
            .child(self.overlay.clone())
    }
}

fn main() {
    gpui_kit::application().with_assets(AllAssets).run(|cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Tooltip lab".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::centered(size(px(860.), px(520.)), cx)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(TooltipLab::new))
            .expect("couldn't open the window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// About what Edit to Branch gives, the box holding the old text until 16 % of the switch and
    /// the new one from 49 %.
    const EDIT_TO_BRANCH: (Option<f32>, Option<f32>) = (Some(0.16), Some(0.49));

    #[test]
    fn only_today_leaves_the_box_empty_and_only_the_cross_fade_draws_both_texts() {
        for variant in Variant::ALL {
            for (old_end, new_start) in [(None, None), EDIT_TO_BRANCH] {
                for step in 0..=100 {
                    let progress = step as f32 / 100.;
                    let (old, new) = texts(variant, progress, old_end, new_start);
                    let at = format!("{variant:?} at {progress}");
                    assert!(old > 0. || new > 0. || variant == Variant::Today, "{at}");
                    assert!(
                        old == 0. || new == 0. || variant == Variant::CrossFade,
                        "{at}"
                    );
                }
            }
        }
        let empty = (0..=100).any(|step| {
            let (old_end, new_start) = EDIT_TO_BRANCH;
            texts(Variant::Today, step as f32 / 100., old_end, new_start) == (0.0, 0.0)
        });
        assert!(empty, "today's switch has frames without a text");
    }
}
