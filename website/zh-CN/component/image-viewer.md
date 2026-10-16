---
title: Image Viewer
description: 在窗口之上浏览一组图片，可以适合窗口，也可以按实际大小显示。
---

# Image Viewer

Image Viewer 在窗口之上（标题栏以下）显示一组图片中的一张，并显示 “2 / 5” 计数和图片的说明文字。它从缩略图打开，用键盘浏览，关闭后回到那张缩略图。

## 导入

```rust
use gpui_kit::component::{
    WindowExt as _,
    image_viewer::{ImageViewerItem, ImageViewerState},
};
```

## 用法

在窗口上打开它，传入图片和最先显示的那一张：

```rust
let items = screenshots.iter().map(|shot| {
    ImageViewerItem::new(shot.image.clone())
        .with_caption(format!("{} · {} × {}", shot.path, shot.width, shot.height))
        .with_dimensions(shot.width, shot.height)
});
let viewer: Entity<ImageViewerState> = window.open_image_viewer(items, ix, cx);
```

每一项都接受任意 `ImageSource`：路径、URL、`Arc<Image>`，或图片缓存提供的 source。对于内存中的图片，建议使用 `LruImageCache::source`（见 [Image Cache](../base/image-cache.md)）：它在 UI 线程之外解码，并让长时间运行的会话卸载不再显示的图片。

查看器打开时会获得焦点。按 Escape 或点击关闭按钮会关闭它，焦点回到之前拥有焦点的元素；用户用键盘打开时，就是那张缩略图。再打开一个查看器会替换前一个。`window.has_active_image_viewer(cx)` 和 `window.close_image_viewer(cx)` 用于查询和关闭它。

## 键盘与指针

| 按键或手势 | 操作 |
| --- | --- |
| `←` / `→` | 上一张或下一张；在第一张和最后一张时保持不动 |
| `Home` / `End` | 第一张或最后一张 |
| `Z`，或双击图片 | 切换实际大小与适合窗口 |
| `Escape` | 关闭 |
| `Tab` | 在查看器的按钮之间移动，不会离开查看器 |

默认适合窗口，每张图片都从适合窗口开始。适合窗口只会把图片缩小到窗口大小，不会放大。按实际大小显示时，比窗口大的图片可以滚动。用 `with_dimensions` 提供图片的像素尺寸，实际大小就表示一个图片像素对应一个屏幕像素；否则一个图片像素占一个逻辑像素。

## 缩略图

查看器显示完整图片；打开它的缩略图不应同时保留完整图片。用适合缩略图尺寸的 `LruImageCache` source 在后台线程缩放：

```rust
let fit = size(rems(12.).to_pixels(window.rem_size()), rems(12.).to_pixels(window.rem_size()));
img(LruImageCache::source(&cache, shot.image.clone(), Some(fit)))
```

## 动效

查看器按主题的 fast 时长淡入；开启“减少动态效果”时直接出现。浏览时在原位置切换图片，没有滑动。

## 无障碍

查看器是名为 “Image viewer” 的对话框。图片的名称来自 `with_accessibility_label`，其次是说明文字，再次是 “Image 2 of 5”。图标按钮都有名称，提示中会显示它们的按键。

## 跟随查看器

显示另一张图片时，`ImageViewerState` 会发出 `ImageViewerEvent::Select(ix)`；`selected_index`、`select_next`、`set_selected_index`、`is_actual_size` 和 `set_actual_size` 用于读取和修改状态。

## API 参考

| 项 | 说明 |
| --- | --- |
| `WindowExt::open_image_viewer(items, ix, cx)` | 在窗口之上打开查看器，返回它的 `Entity<ImageViewerState>` |
| `WindowExt::close_image_viewer(cx)` | 关闭查看器并归还焦点 |
| `WindowExt::has_active_image_viewer(cx)` | 是否有打开的查看器 |
| `ImageViewerItem::new(source)` | 来自任意 `ImageSource` 的图片 |
| `.with_caption(s)` | 与图片一起显示的说明文字 |
| `.with_accessibility_label(s)` | 图片内容，供辅助技术读取 |
| `.with_dimensions(w, h)` | 图片的像素尺寸，用于实际大小 |
| `ImageViewerState::selected_index()` / `set_selected_index(ix, cx)` | 当前显示的图片 |
| `select_previous` / `select_next` / `select_first` / `select_last` | 浏览 |
| `is_actual_size()` / `set_actual_size(b, cx)` / `toggle_actual_size(cx)` | 显示比例 |
| `set_items(items, cx)` | 替换图片 |
| `ImageViewer::new(&state).on_close(f)` | 查看器表面，可放进你自己的浮层 |
