---
title: Tabs
description: 带受控选择的标签列表和可访问标签控件。
order: 30
---

# Tabs

带受控选择的标签列表和可访问标签控件。

和所有 GPUI Base 原语一样，Tabs 只提供行为和语义结构，不规定产品视觉语言。请使用 GPUI 样式并组合导出的部件，使其符合你的设计系统。

## 示例

原生示例和页面上方的 WASM 预览共用同一份实现：

```bash
cargo run -p gpui-base-examples -- tabs
```

## 导入

```rust
use gpui_kit::base::{Tab, Tabs};
```

## 结构与 API

示例组合上述公开类型。GPUI 的标准样式和事件 trait 负责表现，Base 类型负责交互结构。权威实现位于 [`components/tabs.rs`](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/components/tabs.rs)，原生与浏览器预览编译的是同一文件。

## 状态与事件

父级保存活动标签，激活标签后更新对应面板。

受控状态应保存在父渲染类型或 GPUI entity 中；在回调中更新并调用 `cx.notify()`，不要在每次渲染时重建持久 entity。

## 完整 Rust 示例

<<< ../../../../crates/base/examples/showcase/components/tabs.rs{rust}

## 键盘

`Tabs::roving_focus` 让标签列表具备桌面标签控件的键盘行为。`RovingFocus` 为每个标签保存一个 focus handle，整个列表只占一个 Tab 停靠点，落在当前标签上。Left 和 Right 将焦点移到上一个和下一个可用标签，到两端时环绕，Home 和 End 移到第一个和最后一个。在获得焦点的标签上按 Enter 或 Space 会执行它的 `on_click`。

```rust
use gpui_kit::base::{RovingFocus, Tab, Tabs};

impl Render for Preferences {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = ["General", "Appearance", "Advanced"];
        let focus = RovingFocus::keyed("preferences-tabs-focus", window, cx);
        focus.set_items(labels.iter().map(|_| true), Some(self.selected), cx);

        Tabs::new("preferences-tabs")
            .roving_focus(&focus)
            .flex()
            .children(labels.into_iter().enumerate().map(|(ix, label)| {
                Tab::new(ix)
                    .selected(ix == self.selected)
                    .track_focus(&focus.handle(ix).unwrap())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = ix;
                        cx.notify();
                    }))
                    .child(label)
            }))
    }
}
```

其他各项共用一个 Tab 停靠点的复合控件（例如纵向列表）也可以直接使用 `RovingFocus`，在按键处理中调用 `move_focus`。

位于可滚动容器中的控件可以把获得焦点的项滚动到视野内。`track_scroll` 接收每项对应一个子元素的容器的 `ScrollHandle`。`track_list_scroll` 接收 GPUI `list` 的 `ListState`，这种列表的每一项可能包含控件的许多项（例如 Sidebar），各项通过 `item_prepainted` 报告自己绘制的位置。通过键盘获得焦点的项会被滚动到刚好进入视野，已在视野内时不会滚动。

## 可访问性

保留标签列表、标签与面板关系，以及方向键和焦点行为。

## 注意事项

在支持的位置使用稳定元素 ID，并在消费端设计系统中验证焦点、悬停、按下、选中、禁用、减少动态效果和高对比度状态。
