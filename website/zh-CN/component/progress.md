---
title: Progress
description: 用于显示任务完成进度的线性或环形指示器。
---

# Progress

Progress 组件用于直观展示任务完成百分比。库中提供三种形式：

- **[Progress](#progress)**：线性水平进度条
- **[ProgressCircle](#progresscircle)**：环形进度指示器
- **[UsageBar](#usagebar)**：带标签的用量条，用文字给出数值，并用图例说明各个分段

进度条和环形进度都支持数值变化动画（开启“减少动态效果”时只显示最终状态）、加载中（不确定进度）状态、分段、在固定数值处显示刻度、自定义颜色以及自动适配当前主题。

## Progress

```rust
use gpui_kit::component::progress::Progress;
```

### 用法

```rust
Progress::new("my-progress")
    .value(75.0)
```

### 不同进度值

```rust
Progress::new("progress-0").value(0.0)
Progress::new("progress-25").value(25.0)
Progress::new("progress-75").value(75.0)
Progress::new("progress-100").value(100.0)
```

### 加载状态

当实际进度未知时，可通过 `.loading(true)` 显示不确定进度动画。启用后会忽略 `value`。

```rust
Progress::new("loading").loading(true)

Progress::new("my-progress")
    .loading(self.is_loading)
    .value(self.progress)
```

### 尺寸

`Progress` 实现了 `Sizable` trait：

```rust
Progress::new("xs").value(50.0).xsmall()
Progress::new("sm").value(50.0).small()
Progress::new("md").value(50.0)
Progress::new("lg").value(50.0).large()
```

### 自定义样式

组件实现了 `Styled` trait，可自定义高度、圆角、颜色和边框：

```rust
Progress::new("custom")
    .value(32.0)
    .h(px(16.))
    .rounded(px(2.))
    .color(cx.theme().green_light)
    .border_2()
    .border_color(cx.theme().green)
```

### 动态更新进度

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

### 分段

`segments` 用多个分段并排填充进度条，取代单一数值。每个分段是一个 [`ProgressSegment`](#progresssegment)，有自己的数值（占整条的百分比）、颜色和标签。相邻分段之间留有一条细缝，每个分段的终点各自过渡。

```rust
use gpui_kit::component::progress::{Progress, ProgressSegment};

Progress::new("context-split").segments([
    ProgressSegment::new(2.0, cx.theme().chart_1).with_label("System and tools"),
    ProgressSegment::new(12.2, cx.theme().chart_2)
        .with_label("Conversation")
        .with_value_text("about 122k"),
])
```

仅凭颜色，读者无法知道一个分段代表什么。请在进度条旁用文字显示每个标签；[`UsageBar`](#usagebar) 会用图例完成这件事。未设置无障碍数值时，进度条会依次读出每个带标签的分段及其数量（“System and tools 2%, Conversation about 122k”）。

### 刻度

`tick` 在进度条上标出一个固定数值，例如数值正在接近的上限。刻度使用主题的 `foreground` 颜色，可以用 `tick_color` 覆盖。

```rust
Progress::new("context").value(62.0).tick(95.0)
```

### 无障碍数值

Progress 默认向辅助技术报告百分比。当数量用文字表达更有意义时，用 `accessibility_value` 提供这段文字：

```rust
Progress::new("context")
    .value(15.0)
    .accessibility_label("Context")
    .accessibility_value("142k of 950k tokens")
```

### API 参考

| 方法 | 类型 | 说明 |
|---|---|---|
| `new(id)` | `ElementId` | 创建新的进度条 |
| `value(v)` | `f32` | 设置进度值，范围 0 到 100，超出时会自动裁剪 |
| `loading(v)` | `bool` | 开启不确定进度动画；为 `true` 时忽略 `value` |
| `color(c)` | `impl Into<Hsla>` | 覆盖填充颜色，默认使用 `theme.progress_bar` |
| `track_color(c)` | `impl Into<Hsla>` | 覆盖轨道颜色，默认是填充颜色的 20 % 不透明度 |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | 用并排的分段填充，取代 `value` 和 `color` |
| `tick(v)` | `f32` | 在某个数值（0 到 100）处显示刻度 |
| `tick_color(c)` | `impl Into<Hsla>` | 覆盖刻度颜色，默认使用 `theme.foreground` |
| `accessibility_label(s)` | `impl Into<SharedString>` | 设置无障碍名称 |
| `accessibility_value(s)` | `impl Into<SharedString>` | 设置读出的数值，例如 “142k of 950k tokens” |
| `xsmall()` / `small()` / `large()` | — | 通过 `Sizable` 设置预定义高度 |
| `Styled` trait methods | — | 自定义高度、圆角、边框等 |

## ProgressCircle

环形进度指示器适合用于紧凑场景、内联状态或下载上传进度。

```rust
use gpui_kit::component::progress::ProgressCircle;
```

### 用法

```rust
ProgressCircle::new("circle").value(50.0)
```

### 加载状态

```rust
ProgressCircle::new("loading").loading(true)

ProgressCircle::new("circle")
    .loading(self.is_loading)
    .value(self.progress)
```

### 尺寸

`ProgressCircle` 也实现了 `Sizable` trait。命名尺寸映射到固定像素值，也可以通过 `.size(px(n))` 设置自定义尺寸：

```rust
ProgressCircle::new("xs").value(50.0).xsmall()
ProgressCircle::new("sm").value(50.0).small()
ProgressCircle::new("md").value(50.0)
ProgressCircle::new("lg").value(50.0).large()
ProgressCircle::new("xl").value(50.0).size_20()
```

### 自定义颜色

```rust
ProgressCircle::new("green").value(75.0).color(cx.theme().green)
ProgressCircle::new("yellow").value(40.0).color(cx.theme().yellow)
ProgressCircle::new("primary").value(60.0).color(cx.theme().primary)
```

### 内部内容

`ProgressCircle` 实现了 `ParentElement`，所以你可以在环形内部放置内容：

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

### 和文本一起内联显示

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

### 刻度、描边与轨道

上下文用量指示器需要显示上限所在的位置。`tick` 在某个数值处画一道横跨圆环的短刻度；`stroke_width` 和 `track_color` 设置圆环的粗细和底部轨道的颜色。

```rust
ProgressCircle::new("context")
    .value(used_percent)
    .tick(95.0) // 会话交接的位置
    .stroke_width(rems(0.125))
    .track_color(cx.theme().muted)
```

描边默认是圆形宽度的 15 %，最大 5 px；轨道默认是弧线颜色的 20 % 不透明度；刻度默认使用 `theme.foreground`。

### 分段

`segments` 将多个 [`ProgressSegment`](#progresssegment) 从顶部开始依次排列在圆环上，分段之间留有细缝。请在圆环附近用文字说明各个分段，例如放在它的提示或弹出层中。

```rust
ProgressCircle::new("split").size_10().segments([
    ProgressSegment::new(10.0, cx.theme().chart_1).with_label("System"),
    ProgressSegment::new(35.0, cx.theme().chart_2).with_label("Conversation"),
])
```

### 无障碍数值

用 `accessibility_value` 以文字给出比例，屏幕阅读器就会读出 “142k of 1M tokens”，而不是 “14 percent”：

```rust
ProgressCircle::new("context")
    .value(14.2)
    .tick(95.0)
    .accessibility_label("Context")
    .accessibility_value("142k of 1M tokens")
```

### API 参考

| 方法 | 类型 | 说明 |
|---|---|---|
| `new(id)` | `ElementId` | 创建新的环形进度组件 |
| `value(v)` | `f32` | 设置进度值，范围 0 到 100，超出时会自动裁剪 |
| `loading(v)` | `bool` | 开启不确定进度动画；为 `true` 时忽略 `value` |
| `color(c)` | `impl Into<Hsla>` | 覆盖弧线颜色，默认使用 `theme.progress_bar` |
| `track_color(c)` | `impl Into<Hsla>` | 覆盖轨道颜色，默认是弧线颜色的 20 % 不透明度 |
| `stroke_width(w)` | `impl Into<AbsoluteLength>` | 设置圆环粗细，默认是宽度的 15 %，最大 5 px |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | 沿圆环排列分段，取代 `value` 和 `color` |
| `tick(v)` | `f32` | 在某个数值（0 到 100）处画一道横跨圆环的刻度 |
| `tick_color(c)` | `impl Into<Hsla>` | 覆盖刻度颜色，默认使用 `theme.foreground` |
| `accessibility_label(s)` | `impl Into<SharedString>` | 设置无障碍名称 |
| `accessibility_value(s)` | `impl Into<SharedString>` | 设置读出的数值，例如 “142k of 1M tokens” |
| `xsmall()` / `small()` / `large()` | — | 通过 `Sizable` 设置预定义尺寸 |
| `size(px(n))` | `Pixels` | 设置自定义尺寸 |
| `ParentElement` | — | 允许在环形内部放置内容 |

## UsageBar

带标签的用量条，表示某个数量占整体的多少：一行显示标签和文字形式的数量，下面是进度条，可选的图例用颜色和数量说明每个分段，还可以加一行次要说明。适合数字很重要的场合，例如弹出层中的用量明细。

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

其中的进度条是一个以标签命名的 `Progress`，无障碍数值就是数量文字。数字使用等宽数字，数值变化时不会左右跳动。只有当数量确实表示警告或危险时，才把颜色设为 `warning` 或 `danger`。

| 方法 | 类型 | 说明 |
|---|---|---|
| `new(id, label)` | `ElementId`, `impl Into<SharedString>` | 为 `label` 创建用量条 |
| `value_text(s)` | `impl Into<SharedString>` | 文字形式的数量，显示在标签对侧 |
| `value(v)` | `f32` | 填充百分比（0 到 100） |
| `color(c)` | `impl Into<Hsla>` | 覆盖进度条颜色 |
| `segments(s)` | `impl IntoIterator<Item = ProgressSegment>` | 用分段填充，并在图例中列出 |
| `tick(v)` | `f32` | 在某个数值（0 到 100）处显示刻度 |
| `description(s)` | `impl Into<SharedString>` | 进度条下方的一行次要说明 |
| `xsmall()` / `small()` / `large()` | — | 通过 `Sizable` 设置进度条高度 |

## ProgressSegment

分段进度条或圆环中的一个分段。

| 方法 | 类型 | 说明 |
|---|---|---|
| `new(value, color)` | `f32`, `impl Into<Hsla>` | 一个占 `value` 百分比、颜色为 `color` 的分段 |
| `with_label(s)` | `impl Into<SharedString>` | 分段代表的内容 |
| `with_value_text(s)` | `impl Into<SharedString>` | 文字形式的数量，默认是百分比 |
| `value()` / `color()` / `label()` / `value_text()` | — | 读取分段的设置 |

## 示例

### 文件上传

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

### 初始化加载状态

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

### 多步骤流程

```rust
struct Install {
    step: usize,
    total: usize,
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
