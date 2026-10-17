---
title: Theme
description: 使用 GPUI Component 主题系统定制颜色、字体、圆角以及明暗外观。
example: false
---

# Theme

所有组件都支持内置主题系统。[ActiveTheme] trait 用于访问当前主题中的颜色值：

```rs
use gpui_kit::component::{ActiveTheme as _};

// Access theme colors in your components
cx.theme().primary
cx.theme().background
cx.theme().foreground
```

因此，如果你希望组件使用当前主题的颜色，组件或视图就需要运行在带有 [App] 上下文的环境中。

## 渐变背景

主题颜色值继续兼容既有的字符串格式：

```json
{
  "colors": {
    "button.primary.background": "#4F46E5"
  }
}
```

支持渐变渲染的背景 token 也可以使用 CSS 风格的两段线性渐变：

```json
{
  "colors": {
    "button.primary.background": "linear-gradient(135deg, #4F46E5, #06B6D4)",
    "button.primary.hover.background": "linear-gradient(to right, red-500 25%, blue-600 75%)"
  }
}
```

`cx.theme().button_primary` 等顶层字段仍然是纯色 `Hsla`，保持兼容。需要完整 resolved token 时使用 `cx.theme().tokens.button_primary`；其中 `.color` 是纯色代表色，`.background` 是实际配置的 `Background`，包含渐变。

## 键名

少数键也可以使用 Zed 主题的写法，例如用 `link.foreground` 表示 `link`、用 `comment.doc` 表示 `comment_doc`。完整列表见 [`.theme-schema.json`](https://github.com/longbridge/gpui-kit/blob/main/.theme-schema.json)。两种写法同时出现时，以 GPUI Kit 自己的键名为准。

## 编辑器颜色

主题 `highlight` 部分中的 `editor.background` 设置代码编辑器和文本编辑器的背景，默认使用输入框背景。`editor.foreground` 设置代码编辑器的文本颜色，`editor.line_number` 和 `editor.active_line_number` 分别设置行号和光标所在行行号的颜色，默认分别使用 `foreground`、`muted.foreground` 和编辑器的文本颜色。

`editor.gutter.border` 设置编辑器行号与文本之间分隔线的颜色，默认使用 `border`。

## Theme Registry

仓库在 [themes](https://github.com/longbridge/gpui-kit/tree/main/themes) 目录下内置了 20+ 主题。

你可以通过 [ThemeRegistry] 来加载和监听这些主题文件：

从 registry 查找主题时使用 `themes` 数组中条目的 `name`，例如 `Ayu Light`。

```rs
use std::path::PathBuf;
use gpui_kit::{App, SharedString};
use gpui_kit::component::{Theme, ThemeRegistry};

pub fn init(cx: &mut App) {
    let theme_name = SharedString::from("Ayu Light");
    // Load and watch themes from ./themes directory
    if let Err(err) = ThemeRegistry::watch_dir(PathBuf::from("./themes"), cx, move |cx| {
        if let Some(theme) = ThemeRegistry::global(cx)
            .themes()
            .get(&theme_name)
            .cloned()
        {
            Theme::update(cx, |current| current.apply_config(&theme));
        }
    }) {
        tracing::error!("Failed to watch themes directory: {}", err);
    }
}
```

`apply_config` 会替换整个主题。主题文件没有设置的项，例如 `radius`、`shadow`、字体或 `highlight` 部分，都会使用该文件所属模式的默认值，因此之前主题的设置不会保留下来。如果应用自己在主题上设置了圆角，切换主题后需要重新设置。字体请使用[字体覆盖](#字体覆盖)。

### 内置主题与用户文件夹

应用可以把主题嵌入二进制文件，同时允许用户添加自己的主题。用 `add_bundled_themes_from_str` 添加嵌入的文件（也可以自己解析主题，例如在后台线程中解析，再交给 `add_bundled_themes`），并用 `watch_dir` 监听用户的文件夹：

```rs
const THEMES: &[&str] = &[
    include_str!("../themes/macos-classic.json"),
    include_str!("../themes/ayu.json"),
];

let registry = ThemeRegistry::global_mut(cx);
for theme in THEMES {
    registry.add_bundled_themes_from_str(theme)?;
}
ThemeRegistry::watch_dir(user_themes_dir, cx, |cx| { /* 应用已保存的主题 */ })?;
```

文件夹每次重新加载时，内置主题都会保留。文件夹中的主题如果与某个内置主题同名（不区分大小写），就会替换它，因此用户可以把内置主题的文件复制到文件夹中再修改。任何主题都不会占用套件两个默认主题的名称。标记为 `is_default` 的文件夹主题会改为成为其模式的默认主题，直到它的文件被删除。如果副本的模式与它替换的主题不同，那么在该主题是当前浅色或深色主题时，副本不会接管它。用 `ThemeRegistry::theme(name)` 查找主题，它先精确匹配名称，再忽略大小写匹配。文件夹在后台线程中读取和解析，无法解析的文件会被跳过并记录日志。通过 `load_themes_from_str` 添加的主题会一直保留，占用其他主题没有使用的名称。

`ThemeRegistry::load_dir` 以同样的方式读取一次文件夹，但不监听它。适用于不需要热重载的场合，以及测试：`watch_dir` 的监听器运行在自己的线程上，`#[gpui::test]` 的确定性执行器不允许这样做。每次调用 `watch_dir` 或 `load_dir` 都会替换之前的文件夹：之前的监听会停止，之前文件夹的读取如果较晚结束会被丢弃，因此最后请求的文件夹就是注册的那个。

## 字体覆盖

主题文件可以指定字体，没有指定字体的主题会把字体重置为系统字体。因此，用户在应用设置中选择的字体需要在每次切换主题后保留下来。把它设置为一个 `FontOverrides` 即可，`apply_config` 每次应用主题时，都会在同一次更新中把它重新覆盖到主题之上：

```rs
use gpui_kit::component::{FontOverrides, Theme};

Theme::update(cx, |theme| {
    theme.set_font_overrides(
        FontOverrides::new()
            .with_font_family("Work Sans")
            .with_font_size(px(16.) * zoom)
            .with_mono_font_family("JetBrains Mono")
            .with_mono_font_size(px(13.)),
    )
});
```

未设置的字段跟随主题。`font_size` 同时也是应用的缩放比例，因为 `Root` 会用它设置窗口的 rem 大小。切换主题、切换浅色或深色模式、重新加载主题文件夹，都会保留这些覆盖设置，并且主题的观察者每次切换只会收到一次通知。如果在 `Theme` 的观察者中重新设置字体，每次切换都会再更新一次主题。

### 代码字体特性

`with_mono_font_features` 用来设置等宽字体的 OpenType 特性。如果应用提供了代码连字的设置，可以这样关闭连字：

```rs
use std::sync::Arc;
use gpui_kit::FontFeatures;

let no_ligatures = FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]));
Theme::update(cx, |theme| {
    theme.set_font_overrides(
        FontOverrides::new()
            .with_mono_font_family("JetBrains Mono")
            .with_mono_font_features(no_ligatures),
    )
});
```

JetBrains Mono 通过 `calt` 绘制连字，另一些字体则通过 `liga`，所以示例把两者都关闭了。组件中所有使用等宽字体的文本都会应用这些特性，包括 `TextView` 里的代码块和行内代码、`DiffView`、代码 `Editor` 以及按键提示（`Kbd`）。它们周围使用界面字体的文本保留自己的特性。字体特性由应用决定，主题不会设置，所以没有这项覆盖时，字体使用自身的默认特性。应用自己用等宽字体绘制的元素，可以在 `.font_family(cx.theme().mono_font_family.clone())` 旁边加上 `.font_features(cx.theme().mono_font_features.clone())`，以跟随这项设置。

[ActiveTheme]: https://docs.rs/gpui-component/latest/gpui_component/theme/trait.ActiveTheme.html
[ThemeRegistry]: https://docs.rs/gpui-component/latest/gpui_component/theme/struct.ThemeRegistry.html
[App]: https://docs.rs/gpui/latest/gpui/struct.App.html
