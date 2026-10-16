use gpui::{Hsla, SharedString};

/// One part of a segmented [`Progress`](super::Progress) bar or
/// [`ProgressCircle`](super::ProgressCircle) ring.
///
/// Segments are stacked from the start of the track in the order they are
/// given, each taking `value` percent of the whole. Give every segment a
/// label: the color tells segments apart at a glance, the label says what
/// each one is, so the meaning never rests on color alone.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressSegment {
    value: f32,
    color: Hsla,
    label: Option<SharedString>,
    value_text: Option<SharedString>,
}

impl ProgressSegment {
    /// A segment taking `value` percent (0–100) of the track, painted in
    /// `color`.
    pub fn new(value: f32, color: impl Into<Hsla>) -> Self {
        Self {
            value: clamp_percent(value),
            color: color.into(),
            label: None,
            value_text: None,
        }
    }

    /// Names what the segment measures, e.g. "Tool results".
    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The segment's amount as text, e.g. "about 42k", shown in a
    /// [`UsageBar`](super::UsageBar) legend and read by assistive technology.
    /// Without it the percentage is used.
    pub fn with_value_text(mut self, value_text: impl Into<SharedString>) -> Self {
        self.value_text = Some(value_text.into());
        self
    }

    /// The percentage of the track the segment takes, 0–100.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// The segment's color.
    pub fn color(&self) -> Hsla {
        self.color
    }

    /// What the segment measures.
    pub fn label(&self) -> Option<&SharedString> {
        self.label.as_ref()
    }

    /// The segment's amount as text.
    pub fn value_text(&self) -> Option<&SharedString> {
        self.value_text.as_ref()
    }

    /// The amount as shown and read out: the value text, else the rounded
    /// percentage `drawn`, what the track has room to draw of it.
    pub(super) fn spoken_value(&self, drawn: f32) -> SharedString {
        self.value_text
            .clone()
            .unwrap_or_else(|| format!("{}%", drawn.round()).into())
    }
}

/// Clamps a percentage to `0..=100`, reading NaN as 0.
pub(super) fn clamp_percent(value: f32) -> f32 {
    if value.is_nan() {
        0.
    } else {
        value.clamp(0., 100.)
    }
}

/// Where each segment ends along the track, in percent, stacked in order and
/// cut off at 100.
pub(super) fn segment_ends(segments: &[ProgressSegment]) -> Vec<f32> {
    let mut end = 0.;
    segments
        .iter()
        .map(|segment| {
            end = (end + segment.value).min(100.);
            end
        })
        .collect()
}

/// How much of each segment the track draws, in percent: segments stacked
/// past 100 are cut off there.
pub(super) fn drawn_values(segments: &[ProgressSegment]) -> Vec<f32> {
    let mut start = 0.;
    segment_ends(segments)
        .into_iter()
        .map(|end| {
            let drawn = end - start;
            start = end;
            drawn
        })
        .collect()
}

/// The accessible value of a segmented indicator: each labelled segment and
/// its amount, in order, e.g. "System 8%, Conversation about 42k".
pub(super) fn describe_segments(segments: &[ProgressSegment]) -> SharedString {
    segments
        .iter()
        .zip(drawn_values(segments))
        .map(|(segment, drawn)| match segment.label() {
            Some(label) => format!("{label} {}", segment.spoken_value(drawn)),
            None => segment.spoken_value(drawn).to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn segments_stack_in_order_and_stop_at_the_end_of_the_track() {
        let color = hsla(0., 0., 0., 1.);
        let segments = [
            ProgressSegment::new(30., color),
            ProgressSegment::new(f32::NAN, color),
            ProgressSegment::new(50., color),
            ProgressSegment::new(40., color),
            ProgressSegment::new(-5., color),
        ];
        assert_eq!(segment_ends(&segments), vec![30., 30., 80., 100., 100.]);
        assert!(segment_ends(&[]).is_empty());
    }

    #[test]
    fn segments_are_described_by_label_and_amount() {
        let color = hsla(0., 0., 0., 1.);
        let segments = [
            ProgressSegment::new(8.2, color).with_label("System"),
            ProgressSegment::new(42., color)
                .with_label("Conversation")
                .with_value_text("about 42k"),
            ProgressSegment::new(10., color),
        ];
        assert_eq!(
            describe_segments(&segments).as_ref(),
            "System 8%, Conversation about 42k, 10%"
        );
        assert_eq!(
            segments[1].label().map(|l| l.as_ref()),
            Some("Conversation")
        );
        assert_eq!(
            segments[1].value_text().map(|t| t.as_ref()),
            Some("about 42k")
        );
        assert_eq!(segments[2].value(), 10.);
        assert_eq!(ProgressSegment::new(120., color).value(), 100.);
    }

    /// A segment cut off at the end of the track reads the part drawn.
    #[test]
    fn segments_past_the_track_read_what_is_drawn() {
        let color = hsla(0., 0., 0., 1.);
        let segments = [
            ProgressSegment::new(80., color).with_label("Conversation"),
            ProgressSegment::new(40., color).with_label("Tool results"),
            ProgressSegment::new(10., color).with_label("Free"),
        ];
        assert_eq!(drawn_values(&segments), vec![80., 20., 0.]);
        assert_eq!(
            describe_segments(&segments).as_ref(),
            "Conversation 80%, Tool results 20%, Free 0%"
        );
    }
}
