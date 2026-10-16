---
title: Tooltip
description: 与触发元素关联、延迟显示且可定位的说明。
order: 34
---

# Tooltip

与触发元素关联、延迟显示且可定位的说明。

和所有 GPUI Base 原语一样，Tooltip 只提供行为和语义结构，不规定产品视觉语言。请使用 GPUI 样式并组合导出的部件，使其符合你的设计系统。

## 示例

原生示例和页面上方的 WASM 预览共用同一份实现：

```bash
cargo run -p gpui-base-examples -- tooltip
```

## 导入

```rust
use gpui_kit::base::{Tooltip};
```

## 结构与 API

示例组合上述公开类型。GPUI 的标准样式和事件 trait 负责表现，Base 类型负责交互结构。权威实现位于 [`components/tooltip.rs`](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/components/tooltip.rs)，原生与浏览器预览编译的是同一文件。

## 状态与事件

指针悬停或键盘聚焦后延迟显示，离开或失焦后关闭。

受控状态应保存在父渲染类型或 GPUI entity 中；在回调中更新并调用 `cx.notify()`，不要在每次渲染时重建持久 entity。

### 让 Tooltip 跟随触发元素

`TooltipOverlay` 显示一个 `TooltipRequest`。用 `TooltipRequest::for_trigger` 创建的请求只在同时绘制其触发元素的帧中绘制，因此 Tooltip 会随它所说明的元素一起消失（例如点击后关闭所在页面的按钮），仍在等待显示延迟的 Tooltip 也不会出现。构建触发元素时创建一个 `TooltipTrigger`，并在元素的 prepaint 中每帧记录它的边界：

```rust
use gpui_kit::base::{ElementExt as _, TooltipOverlay, TooltipRequest, TooltipTrigger};

let trigger = TooltipTrigger::new();
div()
    .id("save")
    .on_prepaint({
        let trigger = trigger.clone();
        move |bounds, window, _| trigger.prepaint(bounds, window)
    })
    .on_hover(move |hovered, window, cx| {
        if *hovered {
            overlay.update(cx, |overlay: &mut TooltipOverlay, cx| {
                let request = TooltipRequest::for_trigger(&trigger, |_, cx| {
                    cx.new(|_| SaveTooltip).into()
                });
                overlay.request_show(request, window, cx);
            });
        }
    })
```

GPUI 沿用上一帧的视图中的触发元素，只要指针停在其上，就算作已绘制；在 `request_hide` 之后的宽限期内，Tooltip 会一直绘制，直到宽限计时结束。请在触发元素自己的元素 ID 内调用 `prepaint`：触发元素的状态保存在这个 ID 之下。`TooltipRequest::new` 则把请求锚定在固定的边界上，Tooltip 会一直显示，直到被隐藏。

## 完整 Rust 示例

<<< ../../../../crates/base/examples/showcase/components/tooltip.rs{rust}

## 可访问性

Tooltip 只补充说明，不能承载完成任务所必需的信息；触发器必须可聚焦。

## 注意事项

在支持的位置使用稳定元素 ID，并在消费端设计系统中验证焦点、悬停、按下、选中、禁用、减少动态效果和高对比度状态。
