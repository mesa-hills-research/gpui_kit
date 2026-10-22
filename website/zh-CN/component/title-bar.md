---
title: TitleBar
description: 支持窗口控制和自定义内容的自定义标题栏组件。
maturity: [platform-dependent]
---

# TitleBar

TitleBar 用于替换系统默认标题栏，提供可定制的窗口标题区域。它内置平台相关的窗口控制按钮，并支持插入菜单栏、状态信息和自定义操作区。组件会根据 macOS、Windows 和 Linux 自动调整行为和视觉样式。

## 导入

```rust
use gpui_kit::component::TitleBar;
```

## 用法

### 基础标题栏

```rust
TitleBar::new()
    .child(div().child("My Application"))
```

### 带自定义内容的标题栏

```rust
TitleBar::new()
    .child(
        div()
            .flex()
            .items_center()
            .gap_3()
            .child("App Name")
            .child(Badge::new().count(5))
    )
    .child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(Button::new("settings").icon(IconName::Settings))
            .child(Button::new("profile").icon(IconName::User))
    )
```

### 带菜单栏

```rust
TitleBar::new()
    .child(
        div()
            .flex()
            .items_center()
            .child(AppMenuBar::new(window, cx))
    )
    .child(
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .child(Button::new("github").icon(IconName::GitHub))
            .child(Button::new("notifications").icon(IconName::Bell))
    )
```

### 居中标题

`center(element)` 把内容放在标题栏中间，像窗口标题那样相对整个窗口居中，不受红绿灯按钮、窗口按钮和两侧内容宽度的影响。子元素放在它之前，`suffix(element)` 把内容放在它之后、紧挨窗口按钮：

```rust
TitleBar::new()
    .child(AppMenuBar::new(cx))
    .center(
        div()
            .min_w_0()
            .truncate()
            .text_sm()
            .child("Quarterly report.md"),
    )
    .suffix(Button::new("sidebar").icon(IconName::PanelLeft).ghost())
```

一侧内容太宽、标题无法保持居中时，标题会移向另一侧。标题栏空间不够时，标题在两侧内容之间收窄并被裁剪，因此要给它加上 `min_w_0()`，并给其中的文字加上 `truncate()`，让文字以省略号结尾。标题周围的空白仍可拖动窗口，双击的效果与标题栏其他位置相同。

### Linux 自定义关闭行为

```rust
TitleBar::new()
    .on_close_window(|_, window, cx| {
        window.push_notification("Saving before close...", cx);
        window.remove_window();
    })
    .child(div().child("Custom Close Behavior"))
```

### 自定义样式

```rust
TitleBar::new()
    .bg(cx.theme().primary)
    .border_color(cx.theme().primary_border)
    .child(
        div()
            .text_color(cx.theme().primary_foreground)
            .child("Styled Title Bar")
    )
```

### 窗口配置

推荐以 `TitleBar::window_options()` 作为窗口配置的基础，它会配置好标题栏所需的
全部选项，其中包括让标题栏（而不是系统）来处理拖动和双击。

```rust
use gpui_kit::WindowOptions;

WindowOptions {
    window_bounds: Some(window_bounds),
    ..TitleBar::window_options()
}
```

如果自行构造 [`WindowOptions`]，需要同时设置这两个字段：

```rust
use gpui_kit::WindowOptions;

WindowOptions {
    titlebar: Some(TitleBar::title_bar_options()),
    // macOS 上必须设置，否则系统也会处理标题栏双击，
    // 并且会为了判定双击而延迟投递标题栏点击。
    app_owns_titlebar_drag: true,
    ..Default::default()
}
```

## 平台差异

### macOS

- 使用原生红黄绿窗口按钮
- traffic light 默认位置为 `(9px, 9px)`
- 双击标题栏会调用 `window.titlebar_double_click()`
- 左侧默认预留 `80px`，全屏时红黄绿按钮隐藏，留白缩为 `12px`
- 默认表现为透明标题栏

### Windows

- Windows 11 标题按钮：宽 `46px`，与标题栏同高，使用 Segoe Fluent Icons 字形（Windows 10 上为 Segoe MDL2 Assets，两种字体都没有安装时使用套件的图标）
- 每个按钮通过 `WindowControlArea` 响应对应的系统命中测试代码，因此最小化、最大化和关闭由 Windows 执行，最大化按钮上会出现贴靠布局浮层
- 悬停和按下时以低不透明度的前景色填充按钮，关闭按钮变为 Windows 的关闭红色，字形为白色
- 不能最小化的窗口禁用最小化按钮，不能调整大小的窗口禁用最大化按钮
- 窗口不活动时字形变暗
- 左侧默认内边距为 `12px`

### Linux

- 仅在客户端装饰下绘制窗口按钮，并且只绘制合成器支持的按钮
- 按钮按照桌面的按钮布局（GNOME 的 `button-layout`，可以把关闭按钮放在左侧）放在两侧并排序，布局改变时会跟随。`button_layout(...)` 可以覆盖它
- 支持通过 `on_close_window()` 覆盖关闭逻辑
- 支持双击最大化 / 还原
- 支持右键弹出窗口菜单
- 支持在标题栏区域拖动窗口

### Linux：app id 与装饰

在窗口配置中把 `app_id` 设为应用 `.desktop` 文件的名称（不含扩展名），这样合成器会把它的窗口归为一组并显示图标。并在所有环境中请求客户端装饰，这样原本会在标题栏上方绘制自己边框的合成器（KDE、Sway、X11）会交给标题栏处理：

```rust
WindowOptions {
    app_id: Some("com.example.App".into()),
    window_decorations: Some(WindowDecorations::Client),
    ..TitleBar::window_options()
}
```

随应用一起分发 `com.example.App.desktop`（在 X11 上加上 `StartupWMClass=com.example.App`）及其图标。

## 菜单栏与键盘

在 Windows 和 Linux 上，标题栏中的 [AppMenuBar](#appmenubar) 会按 Windows 用户习惯的方式响应键盘，适用于所有通过 `gpui_kit::open_window` 打开的窗口：

- **F10** 或单独按一下 **Alt** 会聚焦第一个菜单的标题，再按一次则归还焦点。
- **Left** 和 **Right** 在菜单标题之间移动，**Down**、**Enter** 或 **Space** 打开当前菜单，**Escape** 回到之前的焦点位置。
- **Alt** 加菜单名中带下划线的字母会直接打开该菜单。按住 Alt 或菜单栏获得焦点时会显示下划线。每个菜单取其名称中尚未被前面菜单占用的第一个字母。

窗口中没有任何元素获得焦点时（例如窗口还没被点击过），这些按键同样有效，离开菜单栏后也不会有元素获得焦点。此时按住 Alt 同样会显示字母下划线，只是在 Alt 按下之后稍有延迟，因为窗口在处于活动状态且没有元素获得焦点时，每秒会检查几次修饰键。有元素获得焦点时，下划线会随 Alt 立即出现。

单独按一下 Alt，是指按下并松开 Alt，中间没有其他操作。按住 Alt 时点击、拖动或滚动滚轮（在文本区域中 Alt 加点击会添加一个光标）都不算，在按下 Alt 之前开始的拖动也不算。按住 Alt 期间窗口失去焦点（例如 Alt+Tab 切走再切回）不算，窗口获得焦点时 Alt 已经按住（例如平铺式合成器用 Alt 加按键切换窗口）也不算。无论哪种情况，窗口的修饰键监听器都会收到 Alt 松开。点击其他控件会在那里结束键盘模式，下一次单独按 Alt 会重新进入。窗口上打开了对话框、Sheet 或图片查看器时，这些按键都不会传到菜单栏。

如果某个输入框绑定了 Alt 加字母，它会保留这个按键：只有当前焦点没有处理该按键时，菜单栏才会收到它。这些绑定都没有上下文：F10 是 `FocusAppMenuBar`，单独按 Alt 是 `AltTap`。在你的键位映射中把这些按键绑定到其他命令即可接管它们。

### AppMenuBar

| 方法 | 说明 |
| --- | --- |
| `AppMenuBar::new(cx)` | 由 `GlobalState::app_menus()` 中的菜单组成的菜单栏 |
| `reload(cx)` | 重新读取菜单 |
| `toggle_focus(window, cx)` | 从键盘聚焦第一个菜单标题，或归还焦点（F10） |
| `open_menu_with_key(key, window, cx)` | 打开该访问键对应的菜单，并返回是否打开 |
| `is_active()` | 菜单栏是否占有键盘：有标题获得焦点或有菜单打开 |
| `highlighted_index()` / `open_index()` | 获得焦点的标题、打开的菜单 |

## API 参考

### TitleBar

| 方法 | 说明 |
| --- | --- |
| `new()` | 创建标题栏 |
| `child(element)` | 向标题栏中添加子元素，有居中内容时放在它之前 |
| `center(element)` | 相对窗口居中的内容（如标题），会为两侧内容让出位置 |
| `suffix(element)` | 标题栏末端、窗口按钮之前的内容 |
| `on_close_window(fn)` | 自定义关闭行为，仅 Linux 有效 |
| `button_layout(layout)` | 按 `layout` 而不是桌面设置放置 Linux 窗口按钮，仅 Linux 有效 |
| `title_bar_options()` | 获取窗口可用的默认标题栏配置 |
| `window_options()` | 获取标题栏配套的默认窗口配置 |

### 窗口配置项

| 属性 | 说明 |
| --- | --- |
| `appears_transparent` | 标题栏透明（默认 true） |
| `traffic_light_position` | macOS 红黄绿按钮位置 |
| `title` | 窗口标题（使用自定义标题栏时可选） |
| `app_owns_titlebar_drag` | 由标题栏自行处理拖动与双击（仅 macOS） |

### 常量

| 常量 | 值 | 说明 |
| --- | --- | --- |
| `TITLE_BAR_HEIGHT` | `34px` | 标准标题栏高度 |
| 左侧留白 | `80px`（macOS，非全屏），`12px`（其他） | 内容区域左侧留白 |

## 说明

- 组件会自动处理平台相关的标题栏行为
- Windows 和 Linux 才会渲染自定义窗口控制按钮
- 拖拽窗口的逻辑已内置在合适区域中
- 自定义样式时应考虑各平台对标题栏的交互习惯
