---
title: Progress
description: Displays an indicator showing the completion progress of a task, typically displayed as a progress bar or circular indicator.
---

# Progress

Progress components visually represent the completion percentage of a task. The library provides three variants:

- **[Progress](#progress)** - A linear horizontal progress bar
- **[ProgressCircle](#progresscircle)** - A circular progress indicator
- **[UsageBar](#usagebar)** - A labelled meter: a bar with its amount in text and a legend for its segments

The bar and the ring feature smooth transition animations when the value changes (one frame under reduced motion), a loading (indeterminate) animation mode, segments, a tick mark at a fixed value, customizable colors, and automatic styling that adapts to the current theme.

## Progress

```rust
use gpui_kit::component::progress::Progress;
```

### Usage

```rust
Progress::new("my-progress")
    .value(75.0) // 75% complete
```

### Different Progress Values

```rust
Progress::new("progress-0").value(0.0)
Progress::new("progress-25").value(25.0)
Progress::new("progress-75").value(75.0)
Progress::new("progress-100").value(100.0)
```

### Loading State

Use `.loading(true)` to show an indeterminate animation when the actual progress is unknown. The `value` is ignored while loading is active.

```rust
// Indeterminate loading animation
Progress::new("loading").loading(true)

// Toggle between loading and determinate
Progress::new("my-progress")
    .loading(self.is_loading)
    .value(self.progress)
```

### Sizes

`Progress` implements the `Sizable` trait:

```rust
Progress::new("xs").value(50.0).xsmall()  // 4px height
Progress::new("sm").value(50.0).small()   // 6px height
Progress::new("md").value(50.0)           // 8px height (default)
Progress::new("lg").value(50.0).large()   // 10px height
```

### Custom Style

The component implements the `Styled` trait, allowing custom height, border radius, color, and border:

```rust
Progress::new("custom")
    .value(32.0)
    .h(px(16.))
    .rounded(px(2.))
    .color(cx.theme().green_light)
    .border_2()
    .border_color(cx.theme().green)
```

### Dynamic Progress Updates

```rust
struct MyView {
    value: f32,
    is_loading: bool,
}

impl Render for MyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("toggle-loading")
                            .label("Loading")
                            .selected(self.is_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.is_loading = !this.is_loading;
                                cx.notify();
                            })),
                    )
                    .child(Button::new("inc").icon(IconName::Plus).on_click(
                        cx.listener(|this, _, _, _| {
                            this.value = (this.value + 10.).min(100.);
                        }),
                    )),
            )
            .child(
                Progress::new("progress")
                    .value(self.value)
                    .loading(self.is_loading),
            )
    }
}
```

### Segments

`segments` fills the bar with several parts side by side instead of one value, each a [`ProgressSegment`](#progresssegment) with its own value (in percent of the bar), color and label. A hairline keeps neighbours apart, and each segment's end moves on its own transition. Segments that add up past 100 are cut off at the end, and a segment without value text shows and reads the part drawn.

```rust
use gpui_kit::component::progress::{Progress, ProgressSegment};

Progress::new("context-split").segments([
    ProgressSegment::new(2.0, cx.theme().chart_1).with_label("System and tools"),
    ProgressSegment::new(12.2, cx.theme().chart_2)
        .with_label("Conversation")
        .with_value_text("about 122k"),
])
```

Color alone can't tell a reader what a segment is. Show each label as text next to the bar; [`UsageBar`](#usagebar) does that with a legend. Without an explicit accessible value, the bar reads out each labelled segment and its amount ("System and tools 2%, Conversation about 122k").

### Tick

`tick` marks a fixed value on the bar, such as a limit the value is heading for. The mark uses the theme's `foreground`; `tick_color` overrides it.

```rust
Progress::new("context").value(62.0).tick(95.0)
```

The mark is a 2 px core edged with a pixel of the theme's background on each side, so the core or its edges stand out on both the fill and the track in every bundled theme, before and after the value passes it. Assistive technology doesn't see the tick: say the limit in the accessible value, e.g. "142k of 1M tokens, hands over at 950k".

### Accessible Value

Progress reports its percentage to assistive technology. When the amount means more as text, give that text with `accessibility_value`:

```rust
Progress::new("context")
    .value(15.0)
    .accessibility_label("Context")
    .accessibility_value("142k of 950k tokens")
```

### API Reference

| Method | Type | Description |
|---|---|---|
| `new(id)` | `ElementId` | Create a new progress bar |
| `value(v)` | `f32` | Set progress value (0–100), clamped automatically |
| `loading(v)` | `bool` | Enable indeterminate loading animation; ignores `value` when `true` |
| `color(c)` | `impl Into<Hsla>` | Override the fill color (defaults to `theme.progress_bar`) |
| `track_color(c)` | `impl Into<Hsla>` | Override the track color (defaults to the fill color at 20 % opacity) |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | Fill with segments side by side; replaces `value` and `color` |
| `tick(v)` | `f32` | Mark a value (0–100) with a tick |
| `tick_color(c)` | `impl Into<Hsla>` | Override the tick color (defaults to `theme.foreground`) |
| `accessibility_label(s)` | `impl Into<SharedString>` | Set the accessible name |
| `accessibility_value(s)` | `impl Into<SharedString>` | Set the value read out, e.g. "142k of 950k tokens" |
| `xsmall()` / `small()` / `large()` | — | Set predefined height via `Sizable` |
| `Styled` trait methods | — | Custom height, border radius, border, etc. |

## ProgressCircle

A circular progress indicator that displays progress as an arc. Ideal for compact spaces, inline labels, or as a download/upload indicator.

```rust
use gpui_kit::component::progress::ProgressCircle;
```

### Usage

```rust
ProgressCircle::new("circle").value(50.0)
```

### Loading State

```rust
// Indeterminate rotating arc animation
ProgressCircle::new("loading").loading(true)

// Toggle between loading and determinate
ProgressCircle::new("circle")
    .loading(self.is_loading)
    .value(self.progress)
```

### Sizes

`ProgressCircle` implements the `Sizable` trait. Named sizes map to fixed pixel dimensions; use `.size(px(n))` for custom sizes:

```rust
ProgressCircle::new("xs").value(50.0).xsmall()    // size_2
ProgressCircle::new("sm").value(50.0).small()     // size_3
ProgressCircle::new("md").value(50.0)             // size_4 (default)
ProgressCircle::new("lg").value(50.0).large()     // size_5
ProgressCircle::new("xl").value(50.0).size_20()   // 80px
```

### Custom Color

```rust
ProgressCircle::new("green").value(75.0).color(cx.theme().green)
ProgressCircle::new("yellow").value(40.0).color(cx.theme().yellow)
ProgressCircle::new("primary").value(60.0).color(cx.theme().primary)
```

### With Inner Content

`ProgressCircle` implements `ParentElement`, so you can place content inside the circle:

```rust
ProgressCircle::new("circle-with-label")
    .value(self.value)
    .size_20()
    .child(
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                div()
                    .child(format!("{}%", self.value as i32))
                    .text_color(cx.theme().progress_bar),
            )
            .child(div().child("Loading").text_xs()),
    )
```

### Inline with Label

```rust
h_flex()
    .gap_2()
    .items_center()
    .child(
        ProgressCircle::new("download")
            .color(cx.theme().primary)
            .value(self.progress)
            .size_4(),
    )
    .child("Downloading...")
```

### Tick, Stroke and Track

A context meter needs to show where a limit sits. `tick` draws a short mark across the ring at a value; `stroke_width` and `track_color` set the ring's thickness and the color it runs over.

```rust
ProgressCircle::new("context")
    .value(used_percent)
    .tick(95.0) // where the session hands over
    .stroke_width(rems(0.125))
    .track_color(cx.theme().muted)
```

The stroke defaults to 15 % of the circle's width, at most 5 px; the track to the arc's color at 20 % opacity; the tick to `theme.foreground`.

### Segments

`segments` stacks [`ProgressSegment`](#progresssegment)s along the ring from the top, in order, with a hairline between them. Name the segments in text near the ring, for example in its tooltip or popover.

```rust
ProgressCircle::new("split").size_10().segments([
    ProgressSegment::new(10.0, cx.theme().chart_1).with_label("System"),
    ProgressSegment::new(35.0, cx.theme().chart_2).with_label("Conversation"),
])
```

### Accessible Value

Give the ratio as text with `accessibility_value`, so a screen reader says "142k of 1M tokens" rather than "14 percent":

```rust
ProgressCircle::new("context")
    .value(14.2)
    .tick(95.0)
    .accessibility_label("Context")
    .accessibility_value("142k of 1M tokens")
```

### API Reference

| Method | Type | Description |
|---|---|---|
| `new(id)` | `ElementId` | Create a new circular progress indicator |
| `value(v)` | `f32` | Set progress value (0–100), clamped automatically |
| `loading(v)` | `bool` | Enable indeterminate loading animation; ignores `value` when `true` |
| `color(c)` | `impl Into<Hsla>` | Override the arc color (defaults to `theme.progress_bar`) |
| `track_color(c)` | `impl Into<Hsla>` | Override the track color (defaults to the arc color at 20 % opacity) |
| `stroke_width(w)` | `impl Into<AbsoluteLength>` | Set the ring's thickness (defaults to 15 % of the width, at most 5 px) |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | Stack segments along the ring; replaces `value` and `color` |
| `tick(v)` | `f32` | Mark a value (0–100) with a tick across the ring |
| `tick_color(c)` | `impl Into<Hsla>` | Override the tick color (defaults to `theme.foreground`) |
| `accessibility_label(s)` | `impl Into<SharedString>` | Set the accessible name |
| `accessibility_value(s)` | `impl Into<SharedString>` | Set the value read out, e.g. "142k of 1M tokens" |
| `xsmall()` / `small()` / `large()` | — | Set predefined size via `Sizable` |
| `size(px(n))` | `Pixels` | Set custom size |
| `ParentElement` | — | Place content inside the circle |

## UsageBar

A labelled meter for a quantity against a whole: the label and the amount as text on one line, the bar, an optional legend naming each segment with its color and amount, and an optional muted description. Use it where the numbers matter, such as a usage breakdown in a popover.

```rust
use gpui_kit::component::progress::{ProgressSegment, UsageBar};

UsageBar::new("context-split", "Context")
    .value_text("142k / 1M")
    .tick(95.0)
    .segments([
        ProgressSegment::new(2.0, cx.theme().chart_1)
            .with_label("System and tools")
            .with_value_text("about 18k"),
        ProgressSegment::new(12.2, cx.theme().chart_2)
            .with_label("Conversation")
            .with_value_text("about 124k"),
    ])
    .description("Hands over at 950k")

UsageBar::new("plan", "5-hour limit")
    .small()
    .value(82.0)
    .color(cx.theme().warning)
    .value_text("82%")
    .description("Resets at 14:00")
```

The bar is a `Progress` named after the label, whose accessible value is the value text. Numbers use tabular figures, so they don't jitter as they change. Use `warning` or `danger` for the color only when the amount means that.

| Method | Type | Description |
|---|---|---|
| `new(id, label)` | `ElementId`, `impl Into<SharedString>` | Create a meter for `label` |
| `value_text(s)` | `impl Into<SharedString>` | The amount as text, opposite the label |
| `value(v)` | `f32` | The filled percentage (0–100) |
| `color(c)` | `impl Into<Hsla>` | Override the bar color |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | Fill with segments and list them in a legend |
| `tick(v)` | `f32` | Mark a value (0–100) with a tick |
| `description(s)` | `impl Into<SharedString>` | A muted line under the bar |
| `xsmall()` / `small()` / `large()` | — | Set the bar's height via `Sizable` |

## ProgressSegment

One part of a segmented bar or ring.

| Method | Type | Description |
|---|---|---|
| `new(value, color)` | `f32`, `impl Into<Hsla>` | A segment of `value` percent in `color` |
| `with_label(s)` | `impl Into<SharedString>` | What the segment measures |
| `with_value_text(s)` | `impl Into<SharedString>` | Its amount as text (defaults to the percentage) |
| `value()` / `color()` / `label()` / `value_text()` | — | Read the segment back |

## Examples

### File Upload

```rust
struct FileUpload {
    uploaded: u64,
    total: u64,
}

impl FileUpload {
    fn progress(&self) -> f32 {
        if self.total == 0 { return 0.0; }
        (self.uploaded as f32 / self.total as f32) * 100.0
    }

    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child("Uploading...")
                    .child(format!("{:.0}%", self.progress())),
            )
            .child(Progress::new("upload").value(self.progress()))
    }
}
```

### Initialization with Loading State

```rust
struct AppInit {
    loading: bool,
    progress: f32,
}

impl Render for AppInit {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        ProgressCircle::new("init-circle")
                            .loading(self.loading)
                            .value(self.progress)
                            .size_4(),
                    )
                    .child(if self.loading { "Initializing..." } else { "Ready" }),
            )
            .child(
                Progress::new("init-bar")
                    .loading(self.loading)
                    .value(self.progress),
            )
    }
}
```

### Multi-Step Process

```rust
struct Install {
    step: usize,       // current package index
    total: usize,      // total packages
    step_progress: f32,
}

impl Install {
    fn overall(&self) -> f32 {
        if self.total == 0 { return 0.0; }
        (self.step as f32 + self.step_progress / 100.0) / self.total as f32 * 100.0
    }

    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(format!("Package {}/{}", self.step + 1, self.total))
                    .child(format!("{:.0}%", self.overall())),
            )
            .child(Progress::new("overall").value(self.overall()))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Progress::new("package").value(self.step_progress).small())
                    .child("Current package"),
            )
    }
}
```
