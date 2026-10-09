---
title: FontPicker
description: 从已安装的字体或字体文件中选择字体族，并设置字重、样式、字号、行高和 OpenType 特性。
---

# FontPicker

用于设置页面的字体选择器：可搜索的字体族列表，所选字体族的字重、斜体、字号和行高，字体提供的可选 OpenType 特性，以及一行用所选字体显示的预览。

## 导入

```rust
use gpui_kit::component::font_picker::{
    FontCatalog, FontPicker, FontPickerEvent, FontPickerState, FontSettings,
};
```

## 用法

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

状态创建后会在后台读取已安装的字体，读完后列出。用户的每次修改都会发出带有新设置的 `FontPickerEvent::Change`。`set_settings` 在代码中选择字体，不发出事件。

## 字体族

`FontCatalog` 列出文本系统可以绘制的字体族。各平台上的名称都来自文本系统，字重、样式和特性则读自字体文件本身：

- **字重**：每个字形的字重。可变字体还会列出其 `wght` 轴范围内的标准字重（100、200 … 900）。
- **斜体**：字体族是否有斜体或倾斜字形。
- **等宽**：每个字形都是等宽的。勾选 **仅显示等宽字体** 只列出这些字体族。
- **特性**：字体列出的可选 OpenType 特性，例如连字（`liga`、`calt`）、带斜线的零（`zero`）、数字样式和风格集（`ss01` … `ss20`）。排版时自动应用的特性不会列出。

换用其他字体族时，字号和行高保持不变，字重改为该字体族最接近的字重，斜体和特性只在该字体族提供时保留。

`catalog` 可以用自己的字体目录代替已安装的字体，例如应用自带的字体：

```rust
let catalog = FontCatalog::from_fonts(&[
    include_bytes!("../fonts/Inter-Regular.ttf").as_slice(),
    include_bytes!("../fonts/JetBrainsMono-Regular.ttf").as_slice(),
]);
FontPickerState::new(window, cx).catalog(catalog)
```

## 添加字体文件

`add_fonts` 把字体数据注册到文本系统，并把其中的字体族加入选择器，在列表中标为 **已添加**。它返回字体族名称，应用可以据此选择其中一个：

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

这样添加的字体在应用退出前一直可用。要长期保留，请把文件复制到应用自己的目录，在启动时用 `cx.text_system().add_fonts` 重新注册，并把同样的数据通过 `added_fonts` 交给选择器，这样它会把这些字体标为已添加，并列出字重和特性。应用自带的字体以同样的方式交给 `app_fonts`。

```rust
FontPickerState::new(window, cx)
    .app_fonts(bundled_fonts())
    .added_fonts(fonts_the_user_added())
```

## 使用设置

`FontSettings` 序列化为普通的值，可以直接写入应用的设置文件，缺少的字段取默认值：

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

`font()` 以 GPUI `Font` 的形式给出字体族、字重、样式和特性。字号和行高设置在元素上：

```rust
TextEditor::new(&document)
    .font(settings.font())
    .text_size(settings.size())
    .line_height(relative(settings.line_height()))
```

## 预览

预览行显示字母、容易混淆的数字，以及会被连字连接的字符组合。用 `preview_text` 设置自己的文字：

```rust
FontPicker::new(&picker).preview_text("fn main() { println!(\"0O 1lI\") }")
```

## 平台

字体族名称来自平台的文本系统：macOS 上是 Core Text，Windows 上是 DirectWrite，Linux 上是 fontconfig 的字体目录。字形读自各平台使用的字体目录。文本系统列出、但文件不在这些目录中的字体族（例如应用用自己的数据注册的字体），除非把其数据交给 `app_fonts` 或 `added_fonts`，否则只提供常规和粗体。
