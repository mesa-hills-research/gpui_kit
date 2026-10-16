---
title: Image Cache
description: Keep decoded images within a memory budget, and decode and scale in-memory images off the UI thread.
order: 9
---

# Image Cache

A long session that shows many images, such as screenshots in a chat transcript, should not keep every decoded image for the life of the app. `LruImageCache` keeps decoded images until their total size passes a budget, then unloads the ones drawn longest ago.

```rust
use gpui_kit::base::image_cache::LruImageCache;

let cache = LruImageCache::new(256 * 1024 * 1024, cx); // 256 MiB of decoded pixels
```

## Two kinds of image

**Files, URLs and embedded assets** go through GPUI's `ImageCache` trait. Put the images under an `image_cache` element, or pass the cache to one `img`:

```rust
image_cache(cache.clone()).child(img(path))
img(path).image_cache(&cache)
```

**Images held in memory**, an `Arc<Image>` such as one a tool returned, go through `LruImageCache::source`. GPUI draws a plain `ImageSource::Image` through its asset cache, which keeps every image for the life of the app. This source decodes through the cache instead, on a background thread:

```rust
// The full image, for a viewer.
img(LruImageCache::source(&cache, screenshot.clone(), None))

// A thumbnail: scaled down on the background thread to fit 12 rem, at the
// window's scale factor, so the transcript never holds the full pixels.
let fit = size(rems(12.).to_pixels(window.rem_size()), rems(12.).to_pixels(window.rem_size()));
img(LruImageCache::source(&cache, screenshot.clone(), Some(fit)))
```

A thumbnail keeps its aspect ratio and is never scaled up. Each size of an image is cached on its own, so a thumbnail and the full image can be held together, and the budget counts both.

## When images are unloaded

An image is never unloaded while it is on screen, even inside a view that replays a cached frame. When newly loaded images take the cache past its budget, a trim begins. Each of its steps waits for the next frame:

1. The least recently drawn images that weren't drawn since the last step are marked, and every view that drew them is asked to draw again.
2. At the next step, a marked image that was drawn again stays for the rest of the trim. The others are unloaded and their textures freed.

The trim ends at the first step with nothing left to mark. The budget is a target: if the images on screen together pass it, they all stay until the next image loads and starts another trim. A step runs on the next frame of any window that draws an image from the cache, so a window that closes or stops drawing doesn't hold it up.

`used_bytes()` reports the decoded size held (4 bytes per pixel per frame), `set_max_bytes` changes the budget, and `clear` unloads everything at once. An image that failed to load is loaded again once a trim finds it off screen, or after `clear`.

## Images larger than the budget

An in-memory image is checked from its header before it's decoded. If its decoded pixels would pass the whole budget, it's refused, and its `img` shows its fallback with the reason in the error. An animated GIF whose frames together pass the budget keeps only its first frame, and a thumbnail decodes only the first frame of an animation. So pick a budget at least as large as the largest image shown at full size: a 4K screenshot takes 33 MB decoded.

## Debug builds

Thumbnails are scaled with the `image` crate, which is slow unoptimized: half a second for a screenshot. Optimize it in debug builds too:

```toml
[profile.dev.package]
image = { opt-level = 3 }
```
