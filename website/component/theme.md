---
title: Theme
description: Customize colors, typography, radii, and light or dark appearance with the GPUI Component theme system.
example: false
---

# Theme

All components support theming through the built-in Theme system, the [ActiveTheme] trait provides access to the current theme colors:

```rs
use gpui_kit::component::{ActiveTheme as _};

// Access theme colors in your components
cx.theme().primary
cx.theme().background
cx.theme().foreground
```

So if you want use the colors from the current theme, you should keep your component or view have [App] context.

## Gradient Backgrounds

Theme color values remain backward compatible with the existing string format:

```json
{
  "colors": {
    "button.primary.background": "#4F46E5"
  }
}
```

Background tokens that opt in to gradient rendering can also use CSS-style two-stop linear gradients:

```json
{
  "colors": {
    "button.primary.background": "linear-gradient(135deg, #4F46E5, #06B6D4)",
    "button.primary.hover.background": "linear-gradient(to right, red-500 25%, blue-600 75%)"
  }
}
```

Top-level theme fields, such as `cx.theme().button_primary`, remain solid `Hsla` values for compatibility. Code that needs the full resolved token can use `cx.theme().tokens.button_primary`; its `.color` field is the solid representative color, and its `.background` field contains the configured `Background`, including gradients.

## Key names

A few keys also go by the names Zed themes use, such as `link.foreground` for `link` and `comment.doc` for `comment_doc`. [`.theme-schema.json`](https://github.com/longbridge/gpui-kit/blob/main/.theme-schema.json) lists them all. When a theme sets both names, the kit's own name wins.

## Editor colors

In a theme's `highlight` section, `editor.background` is the background of a code editor and a text editor, the input background by default. `editor.foreground` colors a code editor's text, and `editor.line_number` and `editor.active_line_number` color the line numbers and the caret line's number. They default to `foreground`, `muted.foreground` and the editor's text color.

`editor.gutter.border` colors the line between an editor's line numbers and its text. It defaults to `border`.

## Theme Registry

There have more than 20 built-in themes available in [themes](https://github.com/longbridge/gpui-kit/tree/main/themes) folder.

https://github.com/longbridge/gpui-kit/tree/main/themes

And we have a [ThemeRegistry] to help us to load themes.

Use the `name` of an entry in the `themes` array, such as `Ayu Light`, when looking up a theme from the registry.

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

`apply_config` replaces the whole theme. A setting the file leaves out, such as `radius`, `shadow`, a font or the `highlight` section, gets its default for the file's mode, so nothing of the theme before carries over. An application that sets its own radius on the theme applies it again after switching themes; for fonts, use [font overrides](#font-overrides).

### Bundled themes and a user folder

An application can embed its themes in the binary and still let users add their own. Add the embedded files with `add_bundled_themes_from_str` (or `add_bundled_themes` with themes you parsed yourself, for example on a background thread), and watch the user's folder with `watch_dir`:

```rs
const THEMES: &[&str] = &[
    include_str!("../themes/macos-classic.json"),
    include_str!("../themes/ayu.json"),
];

let registry = ThemeRegistry::global_mut(cx);
for theme in THEMES {
    registry.add_bundled_themes_from_str(theme)?;
}
ThemeRegistry::watch_dir(user_themes_dir, cx, |cx| { /* apply the saved theme */ })?;
```

Bundled themes stay registered whenever the folder reloads. A theme in the folder whose name matches a bundled one, ignoring case, replaces it, so a user can adjust a bundled theme by copying its file into the folder and editing it. The kit's two defaults are never replaced. Look a theme up with `ThemeRegistry::theme(name)`, which matches the exact name first and then ignores case; the folder is read and parsed on a background thread, and a file that fails to parse is skipped and logged. Themes added with `load_themes_from_str` last only until the folder next loads.

## Font overrides

A theme file may name its fonts, and one that names none resets them to the system's. So a font the user picked in the application's settings has to survive every theme switch. Set it as a `FontOverrides` once; `apply_config` puts it back over each theme it applies, in the same update:

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

Each field left unset follows the theme. `font_size` is also the application's zoom, since `Root` sets the window's rem size from it. A theme switch, a light or dark change and a reload of the themes folder all keep the overrides, and observers of the theme hear about each switch once. Re-applying fonts from an observer of `Theme` would update the theme a second time on every switch.

[ActiveTheme]: https://docs.rs/gpui-component/latest/gpui_component/theme/trait.ActiveTheme.html
[ThemeRegistry]: https://docs.rs/gpui-component/latest/gpui_component/theme/struct.ThemeRegistry.html
[App]: https://docs.rs/gpui/latest/gpui/struct.App.html
