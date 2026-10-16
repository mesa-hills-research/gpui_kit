---
title: Image Viewer
description: Browse a set of images over the window, fitted to it or at actual size.
---

# Image Viewer

Image Viewer shows one image of a set over the window, below its title bar, with a "2 / 5" counter and the image's caption. It opens from a thumbnail, browses with the keyboard, and closes back to that thumbnail.

## Import

```rust
use gpui_kit::component::{
    WindowExt as _,
    image_viewer::{ImageViewerItem, ImageViewerState},
};
```

## Usage

Open it from the window with the images and the one to show first:

```rust
let items = screenshots.iter().map(|shot| {
    ImageViewerItem::new(shot.image.clone())
        .with_caption(format!("{} · {} × {}", shot.path, shot.width, shot.height))
        .with_dimensions(shot.width, shot.height)
});
let viewer: Entity<ImageViewerState> = window.open_image_viewer(items, ix, cx);
```

An item takes any `ImageSource`: a path, a URL, an `Arc<Image>`, or a source from an image cache. For images held in memory, prefer `LruImageCache::source` (see [Image Cache](../base/image-cache.md)): it decodes off the UI thread and lets a long session unload the images it no longer shows.

The viewer takes focus when it opens. Escape or the close button closes it, and focus goes back to what had it, which is the thumbnail when the user opened it from the keyboard. Opening another viewer replaces the first. `window.has_active_image_viewer(cx)` and `window.close_image_viewer(cx)` query and close it.

## Keyboard and pointer

| Key or gesture | Action |
| --- | --- |
| `←` / `→` | Previous or next image, stopping at the first and last. At actual size, pan an image wider than the window |
| `↑` / `↓` | At actual size, pan an image taller than the window |
| `Page Up` / `Page Down` | Previous or next image, at any size |
| `Home` / `End` | First or last image |
| `Z`, or a double-click on the image | Actual size, or fitted to the window again |
| `Escape` | Close |
| `Tab` | Moves between the viewer's buttons, inside the viewer |

Fitted is the default and each image starts fitted. Fitting shrinks an image to the window and never enlarges it. At actual size an image larger than the window scrolls with the wheel, and the arrow keys pan it 48 px a press while Page Up and Page Down go on browsing. Give the item its pixel size with `with_dimensions` for actual size to mean one image pixel per screen pixel. Without it, one image pixel takes one logical pixel.

## Loading, errors and dialogs

While an image decodes, the viewer shows a spinner. An image that fails to load shows "Couldn’t load this image.", with the reason when its source gives one, such as an `LruImageCache` refusing an image larger than its budget. A viewer with no images says "No images".

A dialog opened while the viewer is up, such as a question the app asks, shows over the viewer and takes focus. Closing it gives focus back to the viewer. A viewer opened from a dialog covers that dialog, and Escape closes the viewer first.

## Thumbnails

The viewer shows full images; the thumbnails that open it should not hold full images too. Scale them on a background thread with an `LruImageCache` source that fits the thumbnail's box:

```rust
let fit = size(rems(12.).to_pixels(window.rem_size()), rems(12.).to_pixels(window.rem_size()));
img(LruImageCache::source(&cache, shot.image.clone(), Some(fit)))
```

## Motion

The viewer fades in over the theme's fast duration, and appears at once under reduced motion. Browsing swaps images in place, with no slide.

## Accessibility

The viewer is a dialog named "Image viewer". The image is named by `with_accessibility_label`, else its caption, else "Image 2 of 5". The icon buttons have names, and their tooltips show their keys.

## Following the viewer

`ImageViewerState` emits `ImageViewerEvent::Select(ix)` when another image is shown, and `selected_index`, `select_next`, `set_selected_index`, `is_actual_size` and `set_actual_size` read and change it.

## API Reference

| Item | Description |
| --- | --- |
| `WindowExt::open_image_viewer(items, ix, cx)` | Open a viewer over the window; returns its `Entity<ImageViewerState>` |
| `WindowExt::close_image_viewer(cx)` | Close it, giving focus back |
| `WindowExt::has_active_image_viewer(cx)` | Whether one is open |
| `ImageViewerItem::new(source)` | An image from any `ImageSource` |
| `.with_caption(s)` | The line shown with it |
| `.with_accessibility_label(s)` | What it shows, for assistive technology |
| `.with_dimensions(w, h)` | Its size in pixels, for actual size |
| `ImageViewerState::selected_index()` / `set_selected_index(ix, cx)` | The image shown |
| `select_previous` / `select_next` / `select_first` / `select_last` | Browse |
| `is_actual_size()` / `set_actual_size(b, cx)` / `toggle_actual_size(cx)` | The scale |
| `set_items(items, cx)` | Replace the images |
| `ImageViewer::new(&state).on_close(f)` | The surface, to mount in an overlay of your own |
