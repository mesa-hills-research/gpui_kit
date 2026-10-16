---
title: Image Cache
description: 将解码后的图片控制在内存预算之内，并在后台线程解码和缩放内存中的图片。
order: 9
---

# Image Cache

长时间运行、显示大量图片的会话（例如聊天记录中的截图）不应在应用的整个生命周期内保留所有解码后的图片。`LruImageCache` 会保留解码后的图片，直到它们的总大小超过预算，然后卸载最久没有绘制的图片。

```rust
use gpui_kit::base::image_cache::LruImageCache;

let cache = LruImageCache::new(256 * 1024 * 1024, cx); // 256 MiB 的解码像素
```

## 两类图片

**文件、URL 和嵌入的资源**通过 GPUI 的 `ImageCache` trait 加载。把图片放在 `image_cache` 元素之下，或者把缓存传给单个 `img`：

```rust
image_cache(cache.clone()).child(img(path))
img(path).image_cache(&cache)
```

**内存中的图片**，即 `Arc<Image>`（例如工具返回的图片），通过 `LruImageCache::source` 加载。GPUI 会通过资源缓存绘制普通的 `ImageSource::Image`，而资源缓存会在应用的整个生命周期内保留每一张图片；这个 source 改为通过本缓存在后台线程解码：

```rust
// 完整图片，用于查看器。
img(LruImageCache::source(&cache, screenshot.clone(), None))

// 缩略图：在后台线程按窗口的缩放比例缩小到 12 rem 以内，
// 因此聊天记录中不会保留完整的像素。
let fit = size(rems(12.).to_pixels(window.rem_size()), rems(12.).to_pixels(window.rem_size()));
img(LruImageCache::source(&cache, screenshot.clone(), Some(fit)))
```

缩略图保持原有的宽高比，并且不会被放大。同一张图片的每种尺寸会分别缓存，因此缩略图和完整图片可以同时保留，预算会同时计算两者。

## 何时卸载图片

图片显示在屏幕上时永远不会被卸载，即使它位于回放缓存帧的视图中。当新图片使缓存超出预算时，卸载分两帧完成：

1. 标记自上次清理以来没有再绘制、且最久没有绘制的图片，并请绘制过它们的视图重新绘制；
2. 在那一帧之后，被标记但又重新绘制的图片会保留，其余的会被卸载，并释放它们的纹理。

预算是一个目标：如果屏幕上的图片加起来超过预算，它们都会保留，缓存会在下一张图片到来时卸载能卸载的部分。`used_bytes()` 返回当前保留的解码大小（每帧每像素 4 字节），`set_max_bytes` 修改预算，`clear` 立即卸载全部图片。
