---
title: Kbd
description: 以符合平台习惯的方式显示键盘快捷键。
---

# Kbd

Kbd 用于展示键盘快捷键和组合键，并会自动根据平台采用合适的显示格式。macOS 会使用符号，Windows 和 Linux 则使用文本标签，便于文档、菜单和帮助面板保持一致的快捷键表达。

## 导入

```rust
use gpui_kit::component::kbd::Kbd;
use gpui_kit::Keystroke;
```

## 用法

### 基础快捷键

```rust
let kbd = Kbd::new(Keystroke::parse("cmd-shift-p").unwrap());
let kbd: Kbd = Keystroke::parse("escape").unwrap().into();
```

### 常见快捷键

```rust
Kbd::new(Keystroke::parse("cmd-shift-p").unwrap())
Kbd::new(Keystroke::parse("cmd-t").unwrap())
Kbd::new(Keystroke::parse("cmd--").unwrap())
Kbd::new(Keystroke::parse("cmd-+").unwrap())
Kbd::new(Keystroke::parse("escape").unwrap())
Kbd::new(Keystroke::parse("enter").unwrap())
Kbd::new(Keystroke::parse("backspace").unwrap())
```

### 多修饰键

```rust
Kbd::new(Keystroke::parse("cmd-ctrl-shift-a").unwrap())
Kbd::new(Keystroke::parse("cmd-alt-backspace").unwrap())
Kbd::new(Keystroke::parse("ctrl-alt-shift-a").unwrap())
```

### 方向键与功能键

```rust
Kbd::new(Keystroke::parse("left").unwrap())
Kbd::new(Keystroke::parse("right").unwrap())
Kbd::new(Keystroke::parse("up").unwrap())
Kbd::new(Keystroke::parse("down").unwrap())
Kbd::new(Keystroke::parse("f12").unwrap())
Kbd::new(Keystroke::parse("secondary-f12").unwrap())
Kbd::new(Keystroke::parse("pageup").unwrap())
Kbd::new(Keystroke::parse("pagedown").unwrap())
```

### 关闭默认外观

```rust
Kbd::new(Keystroke::parse("cmd-s").unwrap())
    .appearance(false)
```

### 从 Action 绑定读取

```rust
use gpui_kit::{Action, Window, FocusHandle};

if let Some(kbd) = Kbd::binding_for_action(&MyAction {}, None, window) {
    // 显示该 action 绑定的快捷键
}

if let Some(kbd) = Kbd::binding_for_action(&MyAction {}, Some("Editor"), window) {
    // 显示特定上下文中的快捷键
}

if let Some(kbd) = Kbd::binding_for_action_in(&MyAction {}, &focus_handle, window) {
    // 显示焦点元素上的快捷键
}
```

## 平台差异

### macOS

- 使用符号：⌃ ⌥ ⇧ ⌘
- 修饰键之间不加分隔符
- 顺序为 Control、Option、Shift、Command
- 特殊键使用 ⌫、⎋、⏎、← → ↑ ↓ 等符号

### Windows / Linux

- 使用文本标签：Ctrl、Alt、Shift、Win
- 修饰键之间使用 `+`
- 顺序为 Ctrl、Alt、Shift、Win
- 特殊键显示为 Backspace、Esc、Enter、Left、Right、Up、Down

### 平台示例

| 输入 | macOS | Windows / Linux |
| --- | --- | --- |
| `cmd-a` | ⌘A | Win+A |
| `ctrl-shift-a` | ⌃⇧A | Ctrl+Shift+A |
| `cmd-alt-backspace` | ⌥⌘⌫ | Win+Alt+Backspace |
| `escape` | ⎋ | Esc |
| `enter` | ⏎ | Enter |
| `left` | ← | Left |

## 示例

### 快捷键帮助面板

```rust
use gpui_kit::{div, h_flex, v_flex};

v_flex()
    .gap_2()
    .child(
        h_flex()
            .gap_2()
            .items_center()
            .child("Open command palette:")
            .child(Kbd::new(Keystroke::parse("cmd-shift-p").unwrap()))
    )
    .child(
        h_flex()
            .gap_2()
            .items_center()
            .child("Save file:")
            .child(Kbd::new(Keystroke::parse("cmd-s").unwrap()))
    )
    .child(
        h_flex()
            .gap_2()
            .items_center()
            .child("Find in files:")
            .child(Kbd::new(Keystroke::parse("cmd-shift-f").unwrap()))
    )
```

### 带快捷键的菜单项

```rust
h_flex()
    .justify_between()
    .items_center()
    .child("New File")
    .child(Kbd::new(Keystroke::parse("cmd-n").unwrap()))
```

### 行内说明

```rust
div()
    .child("Press ")
    .child(Kbd::new(Keystroke::parse("escape").unwrap()))
    .child(" to cancel or ")
    .child(Kbd::new(Keystroke::parse("enter").unwrap()))
    .child(" to confirm.")
```

### 自定义样式

```rust
Kbd::new(Keystroke::parse("cmd-k").unwrap())
    .text_color(cx.theme().accent)
    .border_color(cx.theme().accent)
    .bg(cx.theme().accent.opacity(0.1))
```

### 仅获取文本格式

```rust
let shortcut_text = Kbd::format(&Keystroke::parse("cmd-shift-p").unwrap());
div().child(format!("Shortcut: {}", shortcut_text))
```

## 样式

快捷键绘制成一个键帽：一个带圆角的小按键，整个快捷键写在同一个键帽上，例如 `Ctrl+Shift+Z` 或 `⇧⌘Z`。由多个按键依次组成的快捷键（例如 Emacs 的 `ctrl-x ctrl-s`）每个按键各占一个键帽。菜单、菜单栏、文本框的右键菜单、工具提示和命令面板都以这种方式显示快捷键。

键帽在主题的 `muted.background` 上以 `muted.foreground` 显示文字，边框由 `muted.foreground` 得出，底边略粗。有了这道边框，键帽在菜单中高亮的行上也清晰可见。`outline()` 改用主题背景色填充键帽。文字为超小字号，圆角跟随主题的圆角设置。

所有样式都可以通过 `Styled` trait 的方法覆盖。
