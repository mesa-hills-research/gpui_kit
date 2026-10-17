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

**内存中的图片**，即 `Arc<Image>`（例如工具返回的图片），通过 `LruImageCache::source` 加载。GPUI 会通过资源缓存绘制普通的 `ImageSource::Image`，而资源缓存会在应用的整个生命周期内保留每一张图片。这个 source 改为通过本缓存在后台线程解码：

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

图片显示在屏幕上时永远不会被卸载，即使它位于回放缓存帧的视图中。当新加载的图片使缓存超出预算时，会开始一次清理。清理的每一步都会等待下一帧：

1. 标记自上一步以来没有再绘制、且最久没有绘制的图片，并请绘制过它们的每个视图重新绘制。
2. 到下一步时，被标记但又重新绘制的图片在本次清理中保留，其余的会被卸载，并释放它们的纹理。

清理在第一个没有可标记图片的步骤结束。预算是一个目标：如果屏幕上的图片加起来超过预算，它们都会保留，直到下一张图片加载并开始新的清理。清理步骤在任一从缓存绘制图片的窗口的下一帧运行，因此关闭或停止绘制的窗口不会拖住它。

`used_bytes()` 返回当前保留的解码大小（每帧每像素 4 字节），`set_max_bytes` 修改预算，`clear` 立即卸载全部图片。加载失败的图片会在清理发现它不在屏幕上之后，或在 `clear` 之后重新加载。

## 超出预算的图片

内存中的图片在解码前会先根据文件头检查。如果解码后的像素会超过整个预算，它会被拒绝，`img` 显示其后备内容，错误中写明原因。各帧合计超出预算的 GIF 动画只保留第一帧，缩略图也只解码动画的第一帧。因此预算应至少能容纳以完整尺寸显示的最大图片：一张 4K 截图解码后占 33 MB。

## 调试构建

图片的解码和缩略图的缩放都由 `image` crate 完成，未优化时很慢。在调试构建中也优化它，一张 2560×1600 截图的缩略图只需约 0.13 秒，而不是半秒：

```toml
[profile.dev.package]
image = { opt-level = 3 }
```
