use gpui::{
    App, ElementId, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, div, prelude::FluentBuilder as _,
};

use super::{Progress, ProgressSegment, segment::drawn_values};
use crate::{ActiveTheme as _, Sizable, Size, StyledExt as _, h_flex, v_flex};

/// A labelled usage meter: what is measured and how much, a bar, and for a
/// segmented bar a legend that names each segment in text.
///
/// ```text
/// Context                     142k / 1M
/// ██████▌▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░
/// ■ System and tools          about 18k
/// ■ Conversation             about 124k
/// Hands over at 950k
/// ```
///
/// The bar is a [`Progress`] named after the label, whose accessible value
/// is the value text, so the amount never rests on the bar's length or the
/// legend's colors alone.
#[derive(IntoElement)]
pub struct UsageBar {
    id: ElementId,
    style: StyleRefinement,
    label: SharedString,
    value_text: Option<SharedString>,
    description: Option<SharedString>,
    value: f32,
    color: Option<Hsla>,
    segments: Vec<ProgressSegment>,
    tick: Option<f32>,
    size: Size,
}

impl UsageBar {
    /// A meter for `label`, e.g. "Context" or "5-hour limit".
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            label: label.into(),
            value_text: None,
            description: None,
            value: 0.,
            color: None,
            segments: Vec::new(),
            tick: None,
            size: Size::default(),
        }
    }

    /// The amount as text, shown opposite the label, e.g. "142k / 1M".
    pub fn value_text(mut self, text: impl Into<SharedString>) -> Self {
        self.value_text = Some(text.into());
        self
    }

    /// A muted line under the bar, e.g. "Resets at 14:00".
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The filled percentage of the bar, 0–100.
    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// The bar's color. Defaults to the theme's progress bar color; use
    /// `warning` or `danger` only when the amount means that.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Fill the bar with segments and list them in a legend under it, each
    /// with its color, label and value text.
    pub fn segments(mut self, segments: impl IntoIterator<Item = ProgressSegment>) -> Self {
        self.segments = segments.into_iter().collect();
        self
    }

    /// Mark `value` percent of the bar with a tick, e.g. a limit.
    pub fn tick(mut self, value: f32) -> Self {
        self.tick = Some(value);
        self
    }

    fn render_legend(segments: &[ProgressSegment], cx: &App) -> gpui::Div {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_1()
            .text_xs()
            .children(
                segments
                    .iter()
                    .zip(drawn_values(segments))
                    .map(|(segment, drawn)| {
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size_2()
                                    .rounded_xs()
                                    .bg(segment.color()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when_some(segment.label().cloned(), |this, label| {
                                        this.child(label)
                                    }),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(muted)
                                    .font_features(crate::time::time_field::tabular_figures())
                                    .child(segment.spoken_value(drawn)),
                            )
                    }),
            )
    }
}

impl Styled for UsageBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for UsageBar {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for UsageBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let has_segments = !self.segments.is_empty();
        let legend = has_segments.then(|| Self::render_legend(&self.segments, cx));

        v_flex()
            .w_full()
            .gap_1p5()
            .refine_style(&self.style)
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .text_sm()
                    .child(div().min_w_0().truncate().child(self.label.clone()))
                    .when_some(self.value_text.clone(), |this, text| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .text_color(muted)
                                .font_features(crate::time::time_field::tabular_figures())
                                .child(text),
                        )
                    }),
            )
            .child(
                Progress::new(self.id)
                    .with_size(self.size)
                    .accessibility_label(self.label)
                    .when_some(self.value_text, |this, text| this.accessibility_value(text))
                    .when_some(self.color, |this, color| this.color(color))
                    .when_some(self.tick, |this, tick| this.tick(tick))
                    .map(|this| {
                        if has_segments {
                            this.segments(self.segments)
                        } else {
                            this.value(self.value)
                        }
                    }),
            )
            .children(legend)
            .when_some(self.description, |this, description| {
                this.child(div().text_xs().text_color(muted).child(description))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn test_usage_bar_builder() {
        let color = hsla(0.1, 0.8, 0.5, 1.);
        let bar = UsageBar::new("context", "Context")
            .value_text("142k / 1M")
            .description("Hands over at 950k")
            .value(14.2)
            .color(color)
            .tick(95.)
            .segments([
                ProgressSegment::new(2., color).with_label("System and tools"),
                ProgressSegment::new(12.2, color).with_label("Conversation"),
            ])
            .small();
        assert_eq!(bar.label.as_ref(), "Context");
        assert_eq!(bar.value_text.as_deref(), Some("142k / 1M"));
        assert_eq!(bar.description.as_deref(), Some("Hands over at 950k"));
        assert_eq!(bar.value, 14.2);
        assert_eq!(bar.color, Some(color));
        assert_eq!(bar.tick, Some(95.));
        assert_eq!(bar.segments.len(), 2);
        assert_eq!(bar.size, Size::Small);
    }
}
