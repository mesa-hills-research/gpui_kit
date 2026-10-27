---
title: Tooltip
description: 在悬停或聚焦时显示提示信息，支持快捷键和自定义内容。
---

# Tooltip

Tooltip 用于在鼠标悬停或元素获得焦点时显示补充信息。它支持纯文本、自定义内容、快捷键信息以及多种触发方式，适合做解释说明、状态提示和操作说明。

## 移动端行为

iOS 和 Android 上会禁用由 Kit 管理的 Tooltip。移动端操作仍应提供可见或可访问的标签。GPUI 自带的 `.tooltip()` 不受该策略限制。自定义元素使用 `.managed_tooltip()` 即可遵循同样的策略。参阅[移动端](../docs/mobile.md)。

## 导入

```rust
use gpui_kit::component::tooltip::{ManagedTooltipExt as _, Tooltip};
```

## 用法

### 纯文本 Tooltip

```rust
div()
    .child("Hover me")
    .id("basic-tooltip")
    .managed_tooltip(|window, cx| {
        Tooltip::new("This is a helpful tooltip").build(window, cx)
    })
```

### 按钮 Tooltip

```rust
Button::new("save-btn")
    .label("Save")
    .tooltip("Save the current document")
```

### 携带快捷键信息

```rust
actions!(my_actions, [SaveDocument]);

Button::new("save-btn")
    .label("Save")
    .tooltip_with_action(
        "Save the current document",
        &SaveDocument,
        Some("MyContext")
    )
```

### 自定义内容 Tooltip

```rust
div()
    .child("Hover for rich content")
    .id("rich-tooltip")
    .managed_tooltip(|window, cx| {
        Tooltip::element(|_, cx| {
            h_flex()
                .gap_x_1()
                .child(IconName::Info)
                .child(
                    div()
                        .child("Muted Text")
                        .text_color(cx.theme().muted_foreground)
                )
                .child(
                    div()
                        .child("Danger Text")
                        .text_color(cx.theme().danger)
                )
                .child(IconName::ArrowUp)
        })
        .build(window, cx)
    })
```

### 手动指定快捷键

```rust
div()
    .child("Custom keybinding")
    .id("custom-kb")
    .managed_tooltip(|window, cx| {
        Tooltip::new("Delete item")
            .key_binding(Some(Kbd::new("Delete")))
            .build(window, cx)
    })
```

### 在 Tooltip 之间移动

指针在组件上停留半秒后，显示它的 Tooltip。当有 Tooltip 显示时，以及指针离开所有带 Tooltip 的元素后的 0.4 秒内，指针在另一个元素上停留十分之一秒，它的 Tooltip 就会取而代之。指针快速扫过一排按钮时，已显示的 Tooltip 保持原样，不会逐个闪现每个按钮的 Tooltip。

在同一行上两个用 `Tooltip` 构建的 Tooltip 之间切换时，提示框会从一个移动并缩放到另一个，新文字停在自己的位置，随着提示框移动逐渐显露出来。开启减少动态效果时，提示框不会移动，新的 Tooltip 立即取代旧的。

### 触发元素消失时

组件的 `tooltip` 或 `tooltip_with_action` 显示的 Tooltip 属于它所说明的元素。该元素不再绘制时，Tooltip 在同一帧消失，仍在等待显示延迟的 Tooltip 则不会出现。例如：

- 点击“关闭”按钮后，按钮所在的页面随之关闭。
- 指针停在某一行的操作按钮上时，快捷键删除了这一行。
- 列表重新渲染，指针下的那一行不再存在。

GPUI 沿用上一帧的视图中的触发元素，在指针停留其上时算作已绘制。指针离开后，无论窗口中其他部分如何重绘，Tooltip 都会保留到宽限期结束，或者被下一个按钮的 Tooltip 取代。这一行为无需任何设置。直接在 `div` 上调用 GPUI 自带的 `.tooltip()` 不经过这个覆盖层，保持 GPUI 原有的行为。

### 菜单打开时

`dropdown_menu`、`Popover` 或 `context_menu` 打开期间，它的触发元素以及触发元素内的所有元素都不显示 Tooltip，因此不会有 Tooltip 遮住菜单。菜单打开时，已经显示的 Tooltip 随之隐藏。指针离开触发元素后再移回来，在菜单关闭之前也不会显示 Tooltip。菜单关闭后，悬停时照常显示。菜单或 Popover 内部元素的 Tooltip 仍然正常显示。

## API 参考

### ManagedTooltipExt

为树标题等自定义元素导入 `ManagedTooltipExt`。先为 `div` 设置 id，再添加托管 Tooltip，
即可遵循与 Button Tooltip 相同的生命周期和菜单打开时的隐藏规则。

| 方法 | 说明 |
| --- | --- |
| `managed_tooltip(builder)` | 自动选择位置 |
| `managed_tooltip_at(placement, builder)` | 优先使用窗口内可容纳的指定位置 |
| `managed_tooltip_with_placement(placement, builder)` | 接受可选的首选位置 |

构建器返回 `AnyView`，通常由 `Tooltip::build` 创建。

### Tooltip

| 方法 | 说明 |
| --- | --- |
| `new(text)` | 创建文本型 Tooltip |
| `element(builder)` | 创建自定义内容 Tooltip |
| `action(action, context)` | 关联 action，显示对应快捷键信息 |
| `key_binding(kbd)` | 手动设置快捷键展示 |
| `build(window, cx)` | 构建并返回 Tooltip 视图 |

### 内置 Tooltip 方法

很多组件内置了 Tooltip 支持，常见形式包括：

| 方法 | 说明 |
| --- | --- |
| `tooltip(text)` | 添加简单文本提示 |
| `tooltip_with_action(text, action, context)` | 添加带快捷键的提示 |
| `tooltip(closure)` | 使用构建器生成自定义提示 |

## 样式

Tooltip 默认会自动应用与主题匹配的样式：

- 背景：`theme.popover`
- 文字：`theme.popover_foreground`
- 边框：`theme.border`
- 阴影：中等投影
- 圆角：约 `6px`

也可以继续通过 `Styled` trait 自定义：

```rust
Tooltip::new("Custom styled tooltip")
    .bg(cx.theme().accent)
    .text_color(cx.theme().accent_foreground)
    .build(window, cx)
```

## 示例

### 工具栏提示

```rust
h_flex()
    .gap_1()
    .child(
        Button::new("new")
            .icon(IconName::Plus)
            .tooltip_with_action("Create new file", &NewFile, Some("Editor"))
    )
    .child(
        Button::new("save")
            .icon(IconName::Save)
            .tooltip_with_action("Save file", &SaveFile, Some("Editor"))
    )
```

### 表单说明

```rust
v_flex()
    .gap_4()
    .child(
        Input::new("email")
            .placeholder("Enter your email")
            .tooltip("We'll never share your email address")
    )
    .child(
        Input::new("password")
            .input_type(InputType::Password)
            .placeholder("Password")
            .tooltip("Must be at least 8 characters with special characters")
    )
```

## 最佳实践

- 提示文案应简短直接，不重复界面上已经明显存在的信息。
- Tooltip 适合做补充说明，不应用来承载关键流程信息。
- 图标按钮、缩写和危险操作尤其适合配套 Tooltip。
- 需要高频触发的 Tooltip 应尽量避免复杂内容，以减少渲染开销。
