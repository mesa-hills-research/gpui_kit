use crate::{ActiveTheme, Sizable, Size, StyledExt};
use gpui::Bounds;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AbsoluteLength, Animation, AnimationExt as _, AnyElement, App, ElementId, Hsla, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled, Window, canvas, ease_in_out, px, relative,
};
use gpui_base::{Progress as BaseProgress, Transition, transition};
use instant::Duration;
use std::f32::consts::TAU;

use super::segment::{ProgressSegment, clamp_percent, describe_segments, segment_ends};
use crate::plot::shape::{Arc, ArcData};

/// A circular progress indicator element.
///
/// Draws one arc over a track, or several [`ProgressSegment`]s stacked along
/// it, and optionally a tick mark at a fixed value, such as a limit the value
/// is heading for.
#[derive(IntoElement)]
pub struct ProgressCircle {
    id: ElementId,
    style: StyleRefinement,
    color: Option<Hsla>,
    track_color: Option<Hsla>,
    stroke_width: Option<AbsoluteLength>,
    value: f32,
    segments: Vec<ProgressSegment>,
    tick: Option<f32>,
    tick_color: Option<Hsla>,
    accessibility_label: Option<SharedString>,
    accessibility_value: Option<SharedString>,
    size: Size,
    children: Vec<AnyElement>,
    loading: bool,
}

/// One arc to paint, from `start` to `end` in percent of the ring.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RingArc {
    start: f32,
    end: f32,
    color: Hsla,
}

/// Everything the ring's canvas paints.
#[derive(Clone, Debug)]
struct RingPaint {
    arcs: Vec<RingArc>,
    track: Hsla,
    stroke_width: Option<AbsoluteLength>,
    tick: Option<(f32, Hsla)>,
    /// Leave a hairline between neighbouring arcs, so segments of similar
    /// colors stay apart.
    separate: bool,
}

impl ProgressCircle {
    /// Create a new circular progress indicator.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: Default::default(),
            color: None,
            track_color: None,
            stroke_width: None,
            segments: Vec::new(),
            tick: None,
            tick_color: None,
            accessibility_label: None,
            accessibility_value: None,
            style: StyleRefinement::default(),
            size: Size::default(),
            children: Vec::new(),
            loading: false,
        }
    }

    /// Enable indeterminate loading animation.
    ///
    /// When `loading` is `true`, the `value` is ignored and an infinite
    /// rotating arc animation is shown instead.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Set the color of the progress circle.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the color of the track the arc runs over.
    ///
    /// Defaults to the arc's color at 20 % opacity.
    pub fn track_color(mut self, color: impl Into<Hsla>) -> Self {
        self.track_color = Some(color.into());
        self
    }

    /// Set the width of the ring's stroke, e.g. `rems(0.125)`.
    ///
    /// Defaults to 15 % of the circle's width, at most 5 px.
    pub fn stroke_width(mut self, width: impl Into<AbsoluteLength>) -> Self {
        self.stroke_width = Some(width.into());
        self
    }

    /// Set the percentage value of the progress circle.
    ///
    /// The value should be between 0.0 and 100.0.
    pub fn value(mut self, value: f32) -> Self {
        self.value = clamp_percent(value);
        self
    }

    /// Stack these segments along the ring instead of drawing one arc.
    ///
    /// Each segment takes its value in percent of the ring, in order, from
    /// the top. Together they replace [`ProgressCircle::value`] and
    /// [`ProgressCircle::color`]. Label each segment and show the labels near
    /// the ring (a tooltip or popover will do), since a reader can't tell
    /// what a color means.
    pub fn segments(mut self, segments: impl IntoIterator<Item = ProgressSegment>) -> Self {
        self.segments = segments.into_iter().collect();
        self
    }

    /// Draw a tick mark across the ring at `value` percent, e.g. the point
    /// where the measured quantity runs into a limit.
    pub fn tick(mut self, value: f32) -> Self {
        self.tick = Some(clamp_percent(value));
        self
    }

    /// Set the color of the tick mark. Defaults to the theme's foreground.
    pub fn tick_color(mut self, color: impl Into<Hsla>) -> Self {
        self.tick_color = Some(color.into());
        self
    }

    /// Set the accessible name exposed by the progress indicator.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// Set the value assistive technology reads out, e.g. "142k of 1M
    /// tokens", in place of a bare percentage.
    ///
    /// With segments and no value text, each labelled segment and its amount
    /// is read out.
    pub fn accessibility_value(mut self, value: impl Into<SharedString>) -> Self {
        self.accessibility_value = Some(value.into());
        self
    }

    /// The value the indicator reports: the sum of its segments when it has
    /// any.
    fn total_value(&self) -> f32 {
        if self.segments.is_empty() {
            self.value
        } else {
            segment_ends(&self.segments).last().copied().unwrap_or(0.)
        }
    }

    /// The accessible value: the explicit text, else a description of the
    /// segments, else none (the numeric percentage is read).
    fn spoken_value(&self) -> Option<SharedString> {
        self.accessibility_value
            .clone()
            .or_else(|| (!self.segments.is_empty()).then(|| describe_segments(&self.segments)))
    }

    /// Render the ring's canvas.
    fn render_ring(paint: RingPaint) -> impl IntoElement {
        struct PrepaintState {
            paint: RingPaint,
            inner_radius: f32,
            outer_radius: f32,
            stroke_width: f32,
            bounds: Bounds<Pixels>,
        }

        canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut App| {
                let stroke_width = paint
                    .stroke_width
                    .map(|width| width.to_pixels(window.rem_size()))
                    .unwrap_or_else(|| (bounds.size.width * 0.15).min(px(5.)));
                let actual_size = bounds.size.width.min(bounds.size.height);
                let radius = (actual_size.as_f32() - stroke_width.as_f32()) / 2.;
                PrepaintState {
                    inner_radius: radius - stroke_width.as_f32() / 2.,
                    outer_radius: radius + stroke_width.as_f32() / 2.,
                    stroke_width: stroke_width.as_f32(),
                    paint,
                    bounds,
                }
            },
            move |_bounds, prepaint, window: &mut Window, _cx: &mut App| {
                let PrepaintState {
                    paint,
                    inner_radius,
                    outer_radius,
                    stroke_width,
                    bounds,
                } = prepaint;
                let arc = Arc::new()
                    .inner_radius(inner_radius)
                    .outer_radius(outer_radius);

                arc.paint(
                    &ArcData::new(&(), 0, 100., 0., TAU),
                    paint.track,
                    &bounds,
                    window,
                );

                // A hairline's worth of angle at the outer edge.
                let gap = if paint.separate && outer_radius > 0. {
                    1. / outer_radius
                } else {
                    0.
                };
                for (ix, ring_arc) in paint.arcs.iter().enumerate() {
                    let Some((start, end)) = segment_angles(ring_arc.start, ring_arc.end, gap)
                    else {
                        continue;
                    };
                    let data = ArcData::new(&(), ix + 1, ring_arc.end - ring_arc.start, start, end);
                    arc.paint(&data, ring_arc.color, &bounds, window);
                }

                if let Some((value, color)) = paint.tick {
                    // A short radial bar across the stroke, reaching half a
                    // stroke into the ring so it reads over arc and track.
                    let width = (stroke_width * 0.5).max(1.);
                    let tick_outer = outer_radius;
                    let tick_inner = (inner_radius - stroke_width * 0.5).max(0.);
                    let half = (width / 2.) / tick_outer.max(1.);
                    let angle = (value / 100.) * TAU;
                    Arc::new()
                        .inner_radius(tick_inner)
                        .outer_radius(tick_outer)
                        .paint(
                            &ArcData::new(&(), 0, 0., angle - half, angle + half),
                            color,
                            &bounds,
                            window,
                        );
                }
            },
        )
        .absolute()
        .size_full()
    }
}

impl Styled for ProgressCircle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for ProgressCircle {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl ParentElement for ProgressCircle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ProgressCircle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let value = self.total_value();
        let spoken_value = self.spoken_value();
        let loading = self.loading;
        let accessibility_label = self.accessibility_label.clone();
        let motion = cx.theme().motion_tokens();
        let policy = Transition::new(motion.duration_normal).easing(motion.easing_move.clone());

        let color = self.color.unwrap_or(cx.theme().progress_bar);
        let track = self.track_color.unwrap_or(color.opacity(0.2));
        let tick = self
            .tick
            .map(|tick| (tick, self.tick_color.unwrap_or(cx.theme().foreground)));

        // Each arc's end moves on its own transition, so a segment that
        // grows slides its successors along instead of jumping.
        let arcs = if self.segments.is_empty() {
            let end = transition((self.id.clone(), "value"), value, policy, window, cx);
            vec![RingArc {
                start: 0.,
                end,
                color,
            }]
        } else {
            let mut start = 0.;
            segment_ends(&self.segments)
                .into_iter()
                .zip(&self.segments)
                .enumerate()
                .map(|(ix, (end, segment))| {
                    let channel = SharedString::from(format!("segment-{ix}"));
                    let end =
                        transition((self.id.clone(), channel), end, policy.clone(), window, cx);
                    let arc = RingArc {
                        start,
                        end,
                        color: segment.color(),
                    };
                    start = end;
                    arc
                })
                .collect()
        };
        let paint = RingPaint {
            separate: arcs.len() > 1,
            arcs,
            track,
            stroke_width: self.stroke_width,
            tick,
        };

        BaseProgress::new(self.id.clone())
            .value(value)
            .indeterminate(loading)
            .when_some(accessibility_label, |this, label| {
                this.accessibility_label(label)
            })
            .when_some(spoken_value.filter(|_| !loading), |this, value| {
                this.aria_value(value)
            })
            .flex()
            .items_center()
            .justify_center()
            .line_height(relative(1.))
            .map(|this| match self.size {
                Size::XSmall => this.size_2(),
                Size::Small => this.size_3(),
                Size::Medium => this.size_4(),
                Size::Large => this.size_5(),
                Size::Size(s) => this.size(s * 0.75),
            })
            .refine_style(&self.style)
            .children(self.children)
            .map(|this| {
                if loading {
                    this.with_animation(
                        "progress-circle-loading",
                        Animation::new(Duration::from_secs(1)).repeat(),
                        move |this, delta| {
                            let end = ease_in_out(delta) * 100.;
                            let start = ease_in_out(((delta - 0.5) / 0.5).clamp(0., 1.)) * 100.;
                            this.child(Self::render_ring(RingPaint {
                                arcs: vec![RingArc { start, end, color }],
                                track,
                                stroke_width: paint.stroke_width,
                                tick: None,
                                separate: false,
                            }))
                        },
                    )
                    .into_any_element()
                } else {
                    this.child(Self::render_ring(paint)).into_any_element()
                }
            })
    }
}

/// The angles of a ring segment from `start` to `end` percent, inset by
/// half of `gap` (an angle) at each end so it stays apart from its
/// neighbours, whatever its span. `None` when nothing is left to draw.
fn segment_angles(start: f32, end: f32, gap: f32) -> Option<(f32, f32)> {
    let start_angle = (start / 100.) * TAU + gap / 2.;
    let end_angle = (end / 100.) * TAU - gap / 2.;
    (end_angle > start_angle).then_some((start_angle, end_angle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{hsla, rems};

    #[test]
    fn test_progress_circle_builder() {
        let red = hsla(0., 1., 0.5, 1.);
        let blue = hsla(0.6, 1., 0.5, 1.);
        let circle = ProgressCircle::new("context")
            .value(140.)
            .color(red)
            .track_color(blue)
            .stroke_width(rems(0.125))
            .tick(95.)
            .tick_color(blue)
            .accessibility_label("Context")
            .accessibility_value("142k of 1M tokens")
            .loading(false)
            .small();
        assert_eq!(circle.value, 100.);
        assert_eq!(circle.color, Some(red));
        assert_eq!(circle.track_color, Some(blue));
        assert_eq!(circle.stroke_width, Some(rems(0.125).into()));
        assert_eq!(circle.tick, Some(95.));
        assert_eq!(circle.tick_color, Some(blue));
        assert_eq!(circle.size, Size::Small);
        assert_eq!(circle.accessibility_label.as_deref(), Some("Context"));
        assert_eq!(circle.spoken_value().as_deref(), Some("142k of 1M tokens"));

        let plain = ProgressCircle::new("upload").value(f32::NAN).tick(-3.);
        assert_eq!(plain.value, 0.);
        assert_eq!(plain.tick, Some(0.));
        assert_eq!(plain.spoken_value(), None);
    }

    #[test]
    fn segments_set_the_value_and_describe_themselves() {
        let color = hsla(0., 0., 0.5, 1.);
        let circle = ProgressCircle::new("split").value(10.).segments([
            ProgressSegment::new(30., color).with_label("System"),
            ProgressSegment::new(50., color).with_label("Conversation"),
            ProgressSegment::new(40., color).with_label("Tool results"),
        ]);
        assert_eq!(circle.total_value(), 100.);
        // The last segment is cut off at the end of the ring: it reads the
        // part drawn.
        assert_eq!(
            circle.spoken_value().as_deref(),
            Some("System 30%, Conversation 50%, Tool results 20%")
        );

        // Explicit text wins over the generated description.
        let circle = circle.accessibility_value("All of it");
        assert_eq!(circle.spoken_value().as_deref(), Some("All of it"));
    }

    /// Every segment keeps its gap, one past half the ring included.
    #[test]
    fn ring_segments_keep_their_gaps_at_any_span() {
        let gap = 0.02;
        let (start, end) = segment_angles(0., 60., gap).unwrap();
        assert!((start - gap / 2.).abs() < 1e-6);
        assert!(((end - start) - (0.6 * TAU - gap)).abs() < 1e-5);
        let (next_start, _) = segment_angles(60., 90., gap).unwrap();
        assert!(
            ((next_start - end) - gap).abs() < 1e-5,
            "a gap between them"
        );
        assert_eq!(segment_angles(10., 10.1, gap), None, "too thin to draw");
        assert_eq!(
            segment_angles(0., 100., 0.),
            Some((0., TAU)),
            "a lone ring is whole"
        );
    }
}
