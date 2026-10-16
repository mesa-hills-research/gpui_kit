use crate::{ActiveTheme, Sizable, Size, StyledExt, h_flex};
use gpui::{
    Animation, AnimationExt as _, App, Background, ElementId, Hsla, IntoElement, IsZero as _,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled, Window, div, ease_in_out, prelude::FluentBuilder, px, relative,
};
use gpui_base::{
    Progress as BaseProgress, ProgressIndicator, ProgressTrack, Transition, transition,
};
use instant::Duration;

use super::segment::{ProgressSegment, clamp_percent, describe_segments, segment_ends};

/// A linear horizontal progress bar element.
///
/// Fills with one value, or with several [`ProgressSegment`]s side by side,
/// and can mark a fixed value with a tick.
#[derive(IntoElement)]
pub struct Progress {
    id: ElementId,
    style: StyleRefinement,
    color: Option<Hsla>,
    track_color: Option<Hsla>,
    value: f32,
    segments: Vec<ProgressSegment>,
    tick: Option<f32>,
    tick_color: Option<Hsla>,
    accessibility_label: Option<SharedString>,
    accessibility_value: Option<SharedString>,
    size: Size,
    loading: bool,
}

impl Progress {
    /// Create a new Progress bar.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: Default::default(),
            color: None,
            track_color: None,
            segments: Vec::new(),
            tick: None,
            tick_color: None,
            accessibility_label: None,
            accessibility_value: None,
            style: StyleRefinement::default(),
            size: Size::default(),
            loading: false,
        }
    }

    /// Enable indeterminate loading animation.
    ///
    /// When `loading` is `true`, the `value` is ignored and an infinite
    /// sliding animation is shown instead.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Set the color of the progress bar.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the color of the track behind the bar.
    ///
    /// Defaults to the bar's color at 20 % opacity.
    pub fn track_color(mut self, color: impl Into<Hsla>) -> Self {
        self.track_color = Some(color.into());
        self
    }

    /// Set the percentage value of the progress bar.
    ///
    /// The value should be between 0.0 and 100.0.
    pub fn value(mut self, value: f32) -> Self {
        self.value = clamp_percent(value);
        self
    }

    /// Fill the bar with these segments, side by side, instead of one value.
    ///
    /// Each segment takes its value in percent of the bar, in order, from the
    /// left; together they replace [`Progress::value`] and
    /// [`Progress::color`], and a hairline keeps neighbours apart. Show each
    /// segment's label as text next to the bar — a
    /// [`UsageBar`](super::UsageBar) does — since a reader can't tell what a
    /// color means.
    pub fn segments(mut self, segments: impl IntoIterator<Item = ProgressSegment>) -> Self {
        self.segments = segments.into_iter().collect();
        self
    }

    /// Mark `value` percent of the bar with a tick, e.g. a limit the value is
    /// heading for.
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

    /// Set the value assistive technology reads out, e.g. "142k of 950k
    /// tokens", in place of a bare percentage.
    ///
    /// With segments and no value text, each labelled segment and its amount
    /// is read out.
    pub fn accessibility_value(mut self, value: impl Into<SharedString>) -> Self {
        self.accessibility_value = Some(value.into());
        self
    }

    /// The value the bar reports: the sum of its segments when it has any.
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
}

impl Styled for Progress {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for Progress {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Progress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bg = self
            .color
            .map(Background::from)
            .unwrap_or(cx.theme().tokens.progress_bar.into());
        let track = self
            .track_color
            .map(Background::from)
            .unwrap_or(bg.opacity(0.2));
        let value = self.total_value();
        let spoken_value = self.spoken_value();
        let loading = self.loading;
        let accessibility_label = self.accessibility_label.clone();
        let reduce_motion = cx.reduce_motion();
        let tick = self
            .tick
            .map(|tick| (tick, self.tick_color.unwrap_or(cx.theme().foreground)));
        let tick_edge = cx.theme().background;

        let radius = self.style.corner_radii.clone();
        let mut inner_style = StyleRefinement::default();
        inner_style.corner_radii = radius;

        let (height, pill_radius) = match self.size {
            Size::XSmall => (px(4.), px(2.)),
            Size::Small => (px(6.), px(3.)),
            Size::Medium => (px(8.), px(4.)),
            Size::Large => (px(10.), px(5.)),
            Size::Size(s) => (s, s / 2.),
        };
        // The bar reads as a pill of half its own height, and squares off with
        // the rest of the UI when the theme has no radius.
        let radius = if cx.theme().radius.is_zero() {
            px(0.)
        } else {
            pill_radius
        };

        let motion = cx.theme().motion_tokens();
        let policy = Transition::new(motion.duration_normal).easing(motion.easing_move.clone());
        let animated_value = transition(
            (self.id.clone(), "indicator"),
            value,
            policy.clone(),
            window,
            cx,
        );
        // Each segment's end moves on its own transition, so a segment that
        // grows pushes its successors along instead of jumping.
        let segment_widths = {
            let mut start = 0.;
            segment_ends(&self.segments)
                .into_iter()
                .zip(&self.segments)
                .enumerate()
                .map(|(ix, (end, segment))| {
                    let channel = SharedString::from(format!("segment-{ix}"));
                    let end =
                        transition((self.id.clone(), channel), end, policy.clone(), window, cx);
                    let width = (end - start).max(0.);
                    start = end;
                    (width, segment.color())
                })
                .collect::<Vec<_>>()
        };

        BaseProgress::new(self.id)
            .value(value)
            .indeterminate(loading)
            .when_some(accessibility_label, |this, label| {
                this.accessibility_label(label)
            })
            .when_some(spoken_value.filter(|_| !loading), |this, value| {
                this.aria_value(value)
            })
            .w_full()
            .relative()
            .h(height)
            .rounded(radius)
            .refine_style(&self.style)
            .child(
                ProgressTrack::new()
                    .absolute()
                    .size_full()
                    .bg(track)
                    .rounded(radius)
                    .refine_style(&inner_style),
            )
            .map(|this| {
                if !segment_widths.is_empty() && !loading {
                    // Segments sit in one clipped row, so only the bar's own
                    // ends are rounded.
                    this.child(
                        h_flex()
                            .absolute()
                            .size_full()
                            .gap_px()
                            .overflow_hidden()
                            .rounded(radius)
                            .refine_style(&inner_style)
                            .children(segment_widths.into_iter().map(|(width, color)| {
                                ProgressIndicator::new()
                                    .h_full()
                                    .w(relative(width / 100.))
                                    .bg(color)
                                    .when(width <= 0., |this| this.hidden())
                                    .into_any_element()
                            })),
                    )
                } else {
                    this.child(
                        ProgressIndicator::new()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .bg(bg)
                            .rounded(radius)
                            .refine_style(&inner_style)
                            .map(|this| {
                                if loading && !reduce_motion {
                                    this.with_animation(
                                        "progress-loading",
                                        Animation::new(Duration::from_secs(1)).repeat(),
                                        move |this, delta| {
                                            let start = relative(ease_in_out(
                                                ((delta - 0.5) / 0.5).clamp(0., 1.),
                                            ));
                                            let end = relative(ease_in_out(1.0 - delta));
                                            this.when(delta > 0.5, |this| this.left(start))
                                                .right(end)
                                        },
                                    )
                                    .into_any_element()
                                } else if loading {
                                    this.left(relative(0.325))
                                        .right(relative(0.325))
                                        .into_any_element()
                                } else {
                                    this.w(relative((animated_value / 100.).clamp(0., 1.)))
                                        .into_any_element()
                                }
                            }),
                    )
                }
            })
            .when_some(tick.filter(|_| !loading), |this, (tick, color)| {
                // A zero-width anchor at the value centres the mark on it: a
                // core of the tick color edged with a pixel of the background
                // on each side, so either the core or its edges stand out on
                // the fill and on the track in every theme.
                this.child(
                    h_flex()
                        .absolute()
                        .top_0()
                        .h_full()
                        .w_0()
                        .left(relative(tick / 100.))
                        .justify_center()
                        .child(
                            div()
                                .flex_shrink_0()
                                .h_full()
                                .w_1()
                                .border_x_1()
                                .border_color(tick_edge)
                                .bg(color),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn test_progress_builder() {
        let red = hsla(0., 1., 0.5, 1.);
        let blue = hsla(0.6, 1., 0.5, 1.);
        let bar = Progress::new("context")
            .value(-20.)
            .color(red)
            .track_color(blue)
            .tick(95.)
            .tick_color(blue)
            .accessibility_label("Context")
            .accessibility_value("142k of 950k tokens")
            .loading(false)
            .large();
        assert_eq!(bar.value, 0.);
        assert_eq!(bar.color, Some(red));
        assert_eq!(bar.track_color, Some(blue));
        assert_eq!(bar.tick, Some(95.));
        assert_eq!(bar.tick_color, Some(blue));
        assert_eq!(bar.size, Size::Large);
        assert_eq!(bar.accessibility_label.as_deref(), Some("Context"));
        assert_eq!(bar.spoken_value().as_deref(), Some("142k of 950k tokens"));
        assert_eq!(Progress::new("plain").value(40.).spoken_value(), None);
    }

    /// The tick's core (the foreground) or its edges (the background) stand
    /// out at 3:1 on the bar's fill and on its track, in every variant of
    /// every bundled theme, so a limit stays visible once the value passes
    /// it.
    #[test]
    fn the_tick_stands_out_on_fill_and_track_in_every_bundled_theme() {
        use crate::{Theme, ThemeSet, theme::contrast_ratio};
        use std::rc::Rc;

        fn over(color: Hsla, alpha: f32, background: Hsla) -> Hsla {
            let (top, bottom) = (color.to_rgb(), background.to_rgb());
            gpui::Rgba {
                r: top.r * alpha + bottom.r * (1. - alpha),
                g: top.g * alpha + bottom.g * (1. - alpha),
                b: top.b * alpha + bottom.b * (1. - alpha),
                a: 1.,
            }
            .into()
        }

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
        let mut variants = 0;
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let set: ThemeSet =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for config in set.themes {
                let name = config.name.clone();
                let mut theme = Theme::default();
                theme.apply_config(&Rc::new(config));
                let (background, foreground) = (theme.background, theme.foreground);
                let fill = over(theme.progress_bar, theme.progress_bar.a, background);
                let track = over(fill, 0.2, background);
                for (region, color) in [("fill", fill), ("track", track)] {
                    let best =
                        contrast_ratio(foreground, color).max(contrast_ratio(background, color));
                    if best < 3. {
                        failures.push(format!("{name}: {region} {best:.2}"));
                    }
                }
                variants += 1;
            }
        }
        assert!(variants >= 71, "{variants} variants read");
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn segments_set_the_value_and_describe_themselves() {
        let color = hsla(0., 0., 0.5, 1.);
        let bar = Progress::new("split").value(90.).segments([
            ProgressSegment::new(10., color).with_label("System"),
            ProgressSegment::new(25., color)
                .with_label("Conversation")
                .with_value_text("about 25k"),
        ]);
        assert_eq!(bar.total_value(), 35.);
        assert_eq!(
            bar.spoken_value().as_deref(),
            Some("System 10%, Conversation about 25k")
        );
    }
}
