---
title: FontPicker
description: Choose a font family from the installed fonts or font files, with its weight, style, size, line height and OpenType features.
---

# FontPicker

A font picker for settings pages: a searchable list of font families, the
chosen family's weights, italic, size and line height, the optional OpenType
features the font offers, and a preview line in the chosen font. A compact
layout puts the families in a dropdown and the features behind a disclosure.

## Import

```rust
use gpui_kit::component::font_picker::{
    FontCatalog, FontPicker, FontPickerEvent, FontPickerState, FontSettings,
};
```

## Usage

```rust
let picker = cx.new(|cx| {
    FontPickerState::new(window, cx)
        .default_settings(FontSettings::new("JetBrains Mono").with_size(px(14.)))
        .monospace_only(true)
});

cx.subscribe(&picker, |this, _, event: &FontPickerEvent, cx| {
    let FontPickerEvent::Change(font) = event else {
        return;
    };
    this.editor_font = font.clone();
    cx.notify();
})
.detach();

FontPicker::new(&picker)
```

The state reads the installed fonts in the background when it is created and
lists them once they are read. Each change the user makes emits
`FontPickerEvent::Change` with the new settings. `set_settings` chooses a font
from code without an event.

## Compact layout

`compact` lays the picker out as a column of rows, each with its label at the
start and its control at the end, the way a settings page lines up its
options:

```rust
FontPicker::new(&picker).compact()
```

- **Font**: a dropdown of the families with a search field, and the
  **Monospace only** checkbox.
- **Weight**: the family's weights, and **Italic**.
- **Size** and **Line height**.
- **Font features**: a button that shows and hides the features, grouped into
  the font's named features, its stylistic sets and its character variants.
  It says how many the user changed. The features start hidden, and
  `set_features_open` shows them from code.
- The preview line.

The full layout can keep its features behind the same button with
`collapsible_features(true)`.

## Scrolling

The family list and the features scroll with the wheel or trackpad while the
pointer is over them, and the page around the picker stays put. At the end of
a list, the page scrolls with the next gesture. The arrow keys move through the
family list once it has focus, choosing each family in turn.

## The families

`FontCatalog` lists the families the text system can draw. The names come from
the text system on every platform, and the weights, styles and features from
the font files themselves:

- **Weights**: each face's weight, and for a variable font the standard
  weights (100, 200 … 900) along its `wght` axis.
- **Italic**: whether the family has an italic or oblique face.
- **Monospace**: every face is fixed-pitch. The **Monospace only** checkbox
  lists just these.
- **Features**: the optional OpenType features the font lists, such as
  ligatures (`liga`, `calt`), a slashed zero (`zero`), figure styles and
  stylistic sets (`ss01` … `ss20`). Features shaping applies by itself are
  left out.

Choosing another family keeps the size and line height, moves the weight to
the nearest one the family has, and keeps italic and features only when the
family offers them.

`catalog` shows a catalog of your own instead of the installed fonts, for
example the fonts an application bundles:

```rust
let catalog = FontCatalog::from_fonts(&[
    include_bytes!("../fonts/Inter-Regular.ttf").as_slice(),
    include_bytes!("../fonts/JetBrainsMono-Regular.ttf").as_slice(),
]);
FontPickerState::new(window, cx).catalog(catalog)
```

## Adding a font file

`add_fonts` registers font data with the text system and adds its families to
the picker, marked **Added** in the list. It returns the family names, so the
application can choose one:

```rust
let data = std::fs::read(&path)?;
picker.update(cx, |picker, cx| {
    let families = picker.add_fonts(vec![data.into()], window, cx)?;
    if let Some(family) = families.first() {
        picker.choose_family_named(family, window, cx);
    }
    anyhow::Ok(())
})?;
```

Fonts added this way last until the application quits. To keep them, copy the
file somewhere the application owns, register it again at start-up with
`cx.text_system().add_fonts`, and hand the same data to the picker with
`added_fonts`, so it lists them as added with their weights and features.
Fonts the application bundles go to `app_fonts` the same way.

```rust
FontPickerState::new(window, cx)
    .app_fonts(bundled_fonts())
    .added_fonts(fonts_the_user_added())
```

## Using the settings

`FontSettings` serializes to plain values, so it can go straight into an
application's settings file, and a missing field takes its default:

```json
{
  "family": "JetBrains Mono",
  "weight": 500,
  "italic": false,
  "size": 14.0,
  "line_height": 1.5,
  "features": { "calt": false, "zero": true }
}
```

`font()` gives the family, weight, style and features as a GPUI `Font`. The
size and line height go on the element:

```rust
TextEditor::new(&document)
    .font(settings.font())
    .text_size(settings.size())
    .line_height(relative(settings.line_height()))
```

## Preview

The preview line shows letters, figures that are easy to confuse and pairs
that ligatures join. Set your own text with `preview_text`:

```rust
FontPicker::new(&picker).preview_text("fn main() { println!(\"0O 1lI\") }")
```

## Platforms

The family names come from the platform's text system: Core Text on macOS,
DirectWrite on Windows and fontconfig's font folders on Linux. The faces are
read from the font folders each platform uses. A family the text system lists
whose files are not in those folders, such as one an application registered
from its own data, shows Regular and Bold unless its data is given to
`app_fonts` or `added_fonts`.
