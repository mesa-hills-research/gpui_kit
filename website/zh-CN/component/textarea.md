---
title: Textarea
description: 支持固定行数、软换行和自动增高的多行文本输入组件。
---

# Textarea

`Textarea` 用于普通多行文本。单行输入请使用 [Input](./input.md)，源代码编辑请使用 [Editor](./editor.md)。

## 导入

```rust
use gpui_kit::component::input::{Textarea, TextareaState};
```

## 基础用法

```rust
let notes = cx.new(|cx| {
    TextareaState::new(window, cx)
        .rows(5)
        .placeholder("备注")
});

Textarea::new(&notes)
```

## 自动增高

```rust
let message = cx.new(|cx| {
    TextareaState::new(window, cx)
        .auto_grow(2, 8)
        .placeholder("输入消息")
});

Textarea::new(&message)
```

组件最多增长到 `max_rows`，之后内容在内部滚动。

## 值与事件

```rust
let value = notes.read(cx).value();

notes.update(cx, |state, cx| {
    state.set_value("更新后的备注", window, cx);
});

cx.subscribe(&notes, |this, state, event: &InputEvent, cx| {
    if matches!(event, InputEvent::Change) {
        this.notes = state.read(cx).value();
        cx.notify();
    }
});
```

`TextareaState` 还提供 `insert`、`replace`、`cursor_position`、
`soft_wrap`、`searchable` 和 `submit_keys`。

### 发送键

聊天输入框用部分 Enter 组合键发送，用其他组合键换行。`submit_keys` 指定发送用的组合键，可从 `SubmitKey::Enter`、`SubmitKey::ShiftEnter` 和 `SubmitKey::SecondaryEnter`（macOS 上为 Cmd+Enter，其他平台为 Ctrl+Enter）中选择。这些组合键不插入任何内容，并发出 `InputEvent::PressEnter`，可用 `is_submit_chord` 识别。其他组合键插入换行。未设置发送键时，所有 Enter 组合键都换行。输入法正在组字时，Enter 只会确认组字内容。

如果由用户选择发送键，并且无论选择什么，Cmd+Enter 或 Ctrl+Enter 都要发送，就同时设置所选的键和 secondary Enter：

```rust
let composer = cx.new(|cx| {
    TextareaState::new(window, cx)
        .auto_grow(1, 6)
        .submit_keys([SubmitKey::Enter, SubmitKey::SecondaryEnter])
});

cx.subscribe_in(&composer, window, |this, composer, event, window, cx| {
    if let InputEvent::PressEnter { secondary, shift } = event
        && composer.read(cx).is_submit_chord(*secondary, *shift)
    {
        this.send(composer, window, cx);
    }
})
.detach();

// 根据设置在运行时切换：Shift+Enter 发送，Enter 换行。
composer.update(cx, |state, cx| {
    state.set_submit_keys([SubmitKey::ShiftEnter, SubmitKey::SecondaryEnter], cx)
});
```

`SubmitKey` 在设置文件中序列化为 `"enter"`、`"shift_enter"` 和 `"secondary_enter"`。旧的 `submit_on_enter(true)` 用 Enter 以及 Cmd+Enter 或 Ctrl+Enter 发送，等同于 `submit_keys([SubmitKey::Enter, SubmitKey::SecondaryEnter])`。

## 尺寸

```rust
Textarea::new(&notes).large()
Textarea::new(&notes) // medium（默认）
Textarea::new(&notes).small()
```

尺寸会同时改变文字大小和文本四周的内边距，但不决定高度：固定高度用 `h` 设置，随内容增高的 Textarea 由 `rows` 或 `auto_grow` 决定高度。

## 外观

```rust
Textarea::new(&notes)
    .h(px(160.))
    .bordered(true)
    .disabled(false)
    .readonly(false)
    .aria_label("备注")
```

与 `disabled` 不同，只读 Textarea 保持正常外观，仍然可以聚焦、选中和复制，只是拒绝用户对内容的修改。

`Textarea` 不提供只适用于单行 Input 的前后缀、密码显示切换和清除按钮；相关操作应组合在 Textarea 外部。

## 原子行内 token

在多行消息中加入人员提及、文件引用或命令时，可以使用 token。通过 `TextareaState::replace_with_token` 插入，或用 `set_value` 恢复已有草稿。默认标签可以直接使用，需要图标等自定义内容时再设置 renderer：

```rust
Textarea::new(&state)
    .token(|token, _, _| InputToken::new(token).icon(IconName::File))
```

从 `gpui_kit::component::input` 导入 `InputToken`，从 `gpui_kit::component` 导入 `IconName`。token 会整块换到下一行，自动增高会相应调整输入框高度。换行符应放在 token 之间的普通文本中。使用 `on_token_hover` 可在不选中、不编辑的情况下显示提示或预览；禁用 token 不会上报悬停进入事件；禁用已悬停的 token 时会发送离开事件。编辑、激活、草稿保存、模式限制和 JavaScript API 见 [Input：原子行内 token](./input.md#原子行内-token)。

## 拼写检查与单词建议

Textarea 可以使用应用提供的 `SpellChecker` 标记拼写错误，并使用 `SuggestionProvider` 在光标后显示补全。正在输入的单词不会被标记：光标处的拼写错误会暂缓标记，在已标记的单词中输入会先去掉它的下划线。单词一结束（在其后输入空格、标点或换行，或光标移开）就会立即检查，下划线随即出现或消失，无需等到停止输入。Provider 的 `did_change` 收到的每个编辑都带有 `origin`，说明它的来源：`Typed`（输入）、`Suggestion`（按 Tab 接受的建议）、`Correction`（从拼写错误菜单中选择的更正）、`Pasted`（粘贴）、`History`（撤销与重做）或 `Program`（程序），因此从文本中学习的 Provider 可以只学习用户亲手写下的内容。`gpui-kit-writing` 提供了带英语词典的拼写检查器，以及从用户输入中学习的单词建议，见 [拼写检查与单词建议](../docs/writing)。
