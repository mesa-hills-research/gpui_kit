//! An image cache with a memory budget.
//!
//! [`LruImageCache`] keeps decoded images until their total size passes a
//! budget, then unloads the ones drawn longest ago. It serves the two kinds
//! of image an application shows:
//!
//! - files, URLs and embedded assets (`img(path)` inside
//!   `image_cache(cache)`), through GPUI's [`ImageCache`] trait;
//! - images held in memory, such as screenshots a tool returned
//!   ([`Arc<Image>`]), through [`LruImageCache::source`]. These are decoded
//!   on a background thread and can be scaled down to fit a thumbnail there,
//!   so a list of thumbnails never keeps the full-resolution pixels. GPUI
//!   draws an [`ImageSource::Image`] through its asset cache, which keeps
//!   every image for the life of the app.
//!
//! An image is never unloaded while it is on screen. When newly loaded
//! images take the cache past its budget, a trim begins. Each step of it
//! waits a frame: the cache notifies every view that drew an image it means
//! to unload, and unloads the image only if their next frame doesn't draw it
//! again, so a view that replays a cached frame is redrawn before its images
//! go. An image drawn again is kept for the rest of the trim, and the trim
//! ends at the first step with nothing left to mark. The budget is a target:
//! images on screen together may pass it, and stay until the next image
//! loads.
//!
//! In-memory images are checked before they are decoded, from their headers:
//! one whose decoded pixels would pass the whole budget is refused, and its
//! `img` shows its fallback. An animated GIF whose frames together pass it
//! keeps only its first frame, and a thumbnail decodes only the first frame
//! of any animation.

use std::{
    collections::HashMap,
    io::{BufRead, Cursor, Seek},
    sync::Arc,
};

use gpui::{
    AnyWindowHandle, App, AppContext as _, DevicePixels, Entity, EntityId, Image, ImageCache,
    ImageCacheError, ImageCacheItem, ImageFormat, ImageSource, Pixels, RenderImage, Resource, Size,
    SvgRenderer, Task, WeakEntity, Window, hash, size,
};
use smallvec::SmallVec;

/// An image cache that unloads the least recently drawn images once their
/// decoded size passes a budget. See the [module docs](self).
///
/// ```ignore
/// let cache = LruImageCache::new(256 * 1024 * 1024, cx);
///
/// // A screenshot in memory, decoded and scaled to fit 12 rem on a
/// // background thread:
/// img(LruImageCache::source(&cache, screenshot.clone(), Some(fit)))
///
/// // Files and URLs under an element:
/// image_cache(cache.clone()).child(img(path))
/// ```
pub struct LruImageCache {
    this: WeakEntity<Self>,
    ledger: Ledger,
    images: HashMap<u64, Load>,
    /// The trim step waiting for a frame, if any.
    pending: Option<PendingStep>,
    next_step: u64,
}

/// A trim step waiting for the next frame of any of `windows`. Every window
/// that draws an image from the cache meanwhile joins, and the first of them
/// to draw runs it, so a window that closes or stops drawing doesn't hold it
/// up.
struct PendingStep {
    id: u64,
    windows: SmallVec<[AnyWindowHandle; 2]>,
}

/// How a cached image is being loaded.
enum Load {
    /// A file, URL or embedded asset, loaded by GPUI's image loader.
    Resource(ImageCacheItem),
    /// An in-memory image being decoded, and maybe scaled, off the UI thread.
    /// The views waiting for it are notified when it is ready.
    Decoding {
        waiting: SmallVec<[EntityId; 2]>,
        _task: Task<()>,
    },
    /// An in-memory image, decoded.
    Decoded(Result<Arc<RenderImage>, ImageCacheError>),
}

impl Load {
    fn image(&self) -> Option<Arc<RenderImage>> {
        match self {
            Self::Resource(item) => item.get().and_then(Result::ok),
            Self::Decoded(result) => result.as_ref().ok().cloned(),
            Self::Decoding { .. } => None,
        }
    }
}

impl LruImageCache {
    /// A cache that unloads images once their decoded size passes
    /// `max_bytes` (4 bytes per pixel, per frame). An in-memory image larger
    /// than `max_bytes` on its own is refused before it is decoded.
    pub fn new(max_bytes: usize, cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|cx| Self {
            this: cx.weak_entity(),
            ledger: Ledger::new(max_bytes),
            images: HashMap::new(),
            pending: None,
            next_step: 0,
        });
        cx.observe_release(&cache, |cache, cx| {
            for (_, load) in cache.images.drain() {
                if let Some(image) = load.image() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        cache
    }

    /// An image source for an image held in memory, decoded through this
    /// cache on a background thread.
    ///
    /// With `fit`, the decoded image is scaled down on that thread to fit
    /// within `fit` (logical pixels, at the window's scale factor), keeping
    /// its aspect ratio. It is never scaled up, and an animation keeps only
    /// its first frame. Use it for thumbnails, and no `fit` for the full
    /// image. Each size is cached on its own.
    pub fn source(
        cache: &Entity<Self>,
        image: Arc<Image>,
        fit: Option<Size<Pixels>>,
    ) -> ImageSource {
        let cache = cache.clone();
        ImageSource::Custom(Arc::new(move |window, cx| {
            cache.update(cx, |cache, cx| cache.load_image(&image, fit, window, cx))
        }))
    }

    /// The budget, in bytes of decoded pixels.
    pub fn max_bytes(&self) -> usize {
        self.ledger.max_bytes
    }

    /// Change the budget. A lower budget takes effect as the next image is
    /// drawn, and a higher one lets larger in-memory images load from then
    /// on.
    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.ledger.max_bytes = max_bytes;
        self.ledger.grew = true;
    }

    /// The decoded size of the images held, in bytes.
    pub fn used_bytes(&self) -> usize {
        self.ledger.used_bytes
    }

    /// The number of images held, loaded, loading or failed.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether the cache holds no images.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Unload every image now, including any on screen, and forget failed
    /// loads. They load again when drawn.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        for (_, load) in self.images.drain() {
            if let Some(image) = load.image() {
                cx.drop_image(image, Some(window));
            }
        }
        self.ledger = Ledger::new(self.ledger.max_bytes);
        self.pending = None;
    }

    /// Decode `image` (scaled to fit `fit`, if given) or return it from the
    /// cache. Returns `None` while it is decoding; the current view is
    /// notified when it is ready. Only for [`Self::source`]: it must run
    /// while an element is drawn.
    fn load_image(
        &mut self,
        image: &Arc<Image>,
        fit: Option<Size<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let fit = fit.map(|fit| device_size(fit, window.scale_factor()));
        let key = hash(&(image.id(), fit));
        let view = window.current_view();
        self.ledger.touch(key, view);

        match self.images.get_mut(&key) {
            Some(Load::Decoded(result)) => {
                let result = result.clone();
                self.trim_if_grown(window);
                return Some(result);
            }
            Some(Load::Decoding { waiting, .. }) => {
                if !waiting.contains(&view) {
                    waiting.push(view);
                }
                return None;
            }
            Some(Load::Resource(_)) | None => {}
        }

        let image = image.clone();
        let svg_renderer = cx.svg_renderer();
        let limit = self.ledger.max_bytes;
        let this = self.this.clone();
        let task = cx.spawn(async move |cx| {
            let result = cx
                .background_executor()
                .spawn(async move { decode(&image, svg_renderer, fit, limit) })
                .await;
            _ = cx.update(|cx| this.update(cx, |cache, cx| cache.finish_decoding(key, result, cx)));
        });
        self.images.insert(
            key,
            Load::Decoding {
                waiting: smallvec::smallvec![view],
                _task: task,
            },
        );
        None
    }

    fn finish_decoding(
        &mut self,
        key: u64,
        result: Result<Arc<RenderImage>, ImageCacheError>,
        cx: &mut App,
    ) {
        let Some(Load::Decoding { waiting, .. }) = self.images.remove(&key) else {
            return;
        };
        match &result {
            Ok(image) => self.ledger.account(key, image_bytes(image)),
            Err(_) => self.ledger.fail(key),
        }
        self.images.insert(key, Load::Decoded(result));
        // Their next draw starts the trim the new bytes may call for. If
        // they're gone, the next image drawn from this cache starts it.
        for view in waiting {
            cx.notify(view);
        }
    }

    /// Run a trim step on this window's next frame if a step is waiting for
    /// another window, or begin a trim if images loaded since the last one
    /// took the cache past its budget.
    fn trim_if_grown(&mut self, window: &mut Window) {
        let handle = window.window_handle();
        match &mut self.pending {
            Some(pending) => {
                if !pending.windows.contains(&handle) {
                    pending.windows.push(handle);
                    let id = pending.id;
                    self.run_step_on_next_frame(id, window);
                }
            }
            None => {
                if self.ledger.start_trim() {
                    self.schedule_step(window);
                }
            }
        }
    }

    fn schedule_step(&mut self, window: &mut Window) {
        self.next_step += 1;
        self.pending = Some(PendingStep {
            id: self.next_step,
            windows: smallvec::smallvec![window.window_handle()],
        });
        self.run_step_on_next_frame(self.next_step, window);
    }

    fn run_step_on_next_frame(&self, id: u64, window: &mut Window) {
        let this = self.this.clone();
        window.on_next_frame(move |window, cx| {
            _ = this.update(cx, |cache, cx| cache.trim_step(id, window, cx));
        });
    }

    /// Unload the images marked by the last step that the frame since didn't
    /// draw, then mark the next ones and ask their views to draw again.
    fn trim_step(&mut self, id: u64, window: &mut Window, cx: &mut App) {
        if self.pending.as_ref().is_none_or(|pending| pending.id != id) {
            // Another window's frame ran it first.
            return;
        }
        self.pending = None;
        let step = self.ledger.step();
        for key in step.unloaded {
            if let Some(image) = self.images.remove(&key).and_then(|load| load.image()) {
                cx.drop_image(image, Some(window));
            }
        }
        for view in step.notify {
            cx.notify(view);
        }
        if step.more {
            self.schedule_step(window);
        }
    }
}

impl ImageCache for LruImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let key = hash(resource);
        self.ledger.touch(key, window.current_view());
        let result = match self.images.get(&key) {
            Some(Load::Resource(item)) => item.use_image(window),
            _ => {
                let item = ImageCacheItem::new(resource, cx);
                let result = item.use_image(window);
                self.images.insert(key, Load::Resource(item));
                result
            }
        };
        match &result {
            Some(Ok(image)) if !self.ledger.is_accounted(key) => {
                self.ledger.account(key, image_bytes(image));
            }
            Some(Err(_)) => self.ledger.fail(key),
            _ => {}
        }
        self.trim_if_grown(window);
        result
    }
}

/// The decoded size of an image: 4 bytes per pixel, per frame.
fn image_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

fn device_size(fit: Size<Pixels>, scale_factor: f32) -> Size<DevicePixels> {
    size(
        DevicePixels((fit.width.as_f32() * scale_factor).round().max(1.) as i32),
        DevicePixels((fit.height.as_f32() * scale_factor).round().max(1.) as i32),
    )
}

/// What an image file's header says, read without decoding it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ImageHeader {
    width: u32,
    height: u32,
    frames: usize,
}

impl ImageHeader {
    /// The decoded size of `frames` of its frames.
    fn decoded_bytes(&self, frames: usize) -> u64 {
        self.width as u64 * self.height as u64 * 4 * frames as u64
    }
}

/// The dimensions and frame count of `image`, from its header; `None` for
/// an SVG, whose size shows only once it is parsed.
fn probe(image: &Image) -> Result<Option<ImageHeader>, ImageCacheError> {
    let format = match image.format() {
        ImageFormat::Gif => return gif_header(image.bytes()).map(Some),
        ImageFormat::Svg => return Ok(None),
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Ico => image::ImageFormat::Ico,
        ImageFormat::Pnm => image::ImageFormat::Pnm,
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(image.bytes()), format)
        .into_dimensions()
        .map_err(|err| ImageCacheError::Image(Arc::new(err)))?;
    Ok(Some(ImageHeader {
        width,
        height,
        frames: 1,
    }))
}

/// A GIF's logical screen size and its number of frames, walking its blocks
/// without decoding them. A file cut short counts the frames it begins.
fn gif_header(bytes: &[u8]) -> Result<ImageHeader, ImageCacheError> {
    let malformed = || ImageCacheError::Asset("not a GIF file".into());
    if bytes.len() < 13 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Err(malformed());
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    // A color table's size, from the flags byte that announces it.
    let color_table = |flags: u8| {
        if flags & 0x80 != 0 {
            3 * (2usize << (flags & 7))
        } else {
            0
        }
    };
    // Skips a run of data sub-blocks; `None` once the bytes run out.
    let skip_sub_blocks = |mut pos: usize| -> Option<usize> {
        loop {
            let len = *bytes.get(pos)? as usize;
            pos += 1 + len;
            if len == 0 {
                return Some(pos);
            }
        }
    };
    let mut pos = 13 + color_table(bytes[10]);
    let mut frames = 0;
    loop {
        let next = match bytes.get(pos) {
            // An extension: its label, then data sub-blocks.
            Some(0x21) => skip_sub_blocks(pos + 2),
            // An image: its descriptor, a local color table, the LZW code
            // size, then data sub-blocks.
            Some(0x2C) => {
                frames += 1;
                bytes
                    .get(pos + 9)
                    .and_then(|&flags| skip_sub_blocks(pos + 11 + color_table(flags)))
            }
            // The trailer, or the end of a truncated file.
            Some(0x3B) | None => break,
            Some(_) => return Err(malformed()),
        };
        let Some(next) = next else { break };
        pos = next;
    }
    Ok(ImageHeader {
        width,
        height,
        frames: frames.max(1),
    })
}

/// The error for an image refused before decoding.
fn too_large(width: u32, height: u32, bytes: u64, limit: usize) -> ImageCacheError {
    const MB: f64 = 1024. * 1024.;
    ImageCacheError::Asset(
        format!(
            "A {width}×{height} image takes {:.1} MB decoded, more than the {:.1} MB image budget.",
            bytes as f64 / MB,
            limit as f64 / MB,
        )
        .into(),
    )
}

/// Decode an in-memory image, scaled down to fit `fit`, refusing one whose
/// decoded pixels would pass `limit` bytes. Runs off the UI thread.
fn decode(
    image: &Image,
    svg_renderer: SvgRenderer,
    fit: Option<Size<DevicePixels>>,
    limit: usize,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let decoded = match probe(image)? {
        Some(header) => {
            let first = header.decoded_bytes(1);
            if first > limit as u64 {
                return Err(too_large(header.width, header.height, first, limit));
            }
            // A thumbnail, or an animation too large to hold whole, keeps
            // its first frame.
            let frames = if fit.is_some() || header.decoded_bytes(header.frames) > limit as u64 {
                1
            } else {
                header.frames
            };
            match image.format() {
                ImageFormat::Gif => decode_gif(Cursor::new(image.bytes()), frames, limit)?,
                _ => image.to_image_data(svg_renderer)?,
            }
        }
        // GPUI caps an SVG's rendering at 8192 pixels a side.
        None => {
            let decoded = image.to_image_data(svg_renderer)?;
            let bytes = image_bytes(&decoded) as u64;
            if bytes > limit as u64 {
                let size = decoded.size(0);
                return Err(too_large(
                    size.width.0 as u32,
                    size.height.0 as u32,
                    bytes,
                    limit,
                ));
            }
            decoded
        }
    };
    Ok(match fit {
        Some(fit) => scale_to_fit(&decoded, fit).unwrap_or(decoded),
        None => decoded,
    })
}

/// Up to `max_frames` frames of a GIF, in GPUI's BGRA, with no allocation
/// past `limit` bytes. Frames that fail to decode are skipped, as GPUI does.
fn decode_gif(
    bytes: impl BufRead + Seek,
    max_frames: usize,
    limit: usize,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    use image::{AnimationDecoder as _, ImageDecoder as _};
    let image_error = |err| ImageCacheError::Image(Arc::new(err));
    let mut decoder = image::codecs::gif::GifDecoder::new(bytes).map_err(image_error)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(limit as u64);
    decoder.set_limits(limits).map_err(image_error)?;
    let frames = decoder
        .into_frames()
        .take(max_frames)
        .filter_map(Result::ok)
        .map(|mut frame| {
            for pixel in frame.buffer_mut().chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            frame
        })
        .collect::<SmallVec<[image::Frame; 1]>>();
    if frames.is_empty() {
        return Err(ImageCacheError::Asset("no frame of the GIF decodes".into()));
    }
    Ok(Arc::new(RenderImage::new(frames)))
}

/// The size `image` takes when scaled down, keeping its aspect ratio, to fit
/// within `fit`; `None` when it already fits.
fn fitted_size(image: Size<DevicePixels>, fit: Size<DevicePixels>) -> Option<(u32, u32)> {
    let (width, height) = (image.width.0.max(1) as f64, image.height.0.max(1) as f64);
    let scale = (fit.width.0.max(1) as f64 / width).min(fit.height.0.max(1) as f64 / height);
    (scale < 1.).then(|| {
        (
            ((width * scale).round() as u32).max(1),
            ((height * scale).round() as u32).max(1),
        )
    })
}

/// The first frame of `image` scaled down to fit `fit`, or `None` when it
/// already fits. Scales straight from the decoded pixels, without a copy.
fn scale_to_fit(image: &RenderImage, fit: Size<DevicePixels>) -> Option<Arc<RenderImage>> {
    let (width, height) = fitted_size(image.size(0), fit)?;
    let size = image.size(0);
    let buffer = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
        size.width.0 as u32,
        size.height.0 as u32,
        image.as_bytes(0)?,
    )?;
    // The bytes are BGRA; scaling treats every channel alike.
    let scaled = image::imageops::resize(
        &buffer,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    let frame = image::Frame::from_parts(scaled, 0, 0, image.delay(0));
    Some(Arc::new(RenderImage::new(smallvec::smallvec![frame])))
}

/// The cache's bookkeeping: which images were drawn when and by which
/// views, their sizes, and which are marked to unload. Kept apart from the
/// images so the policy can be tested on its own.
struct Ledger {
    max_bytes: usize,
    used_bytes: usize,
    /// Advances at each trim step; an entry drawn since the last step has
    /// this generation.
    generation: u64,
    /// Advances at each draw, ordering entries from least to most recently
    /// drawn.
    clock: u64,
    entries: HashMap<u64, LedgerEntry>,
    /// Whether images loaded, or the budget changed, since the last trim
    /// began. Only that begins a trim.
    grew: bool,
    /// Whether the next step is the first of a trim, which marks nothing
    /// the frame before drew.
    first_step: bool,
}

#[derive(Default)]
struct LedgerEntry {
    /// Decoded bytes, once loaded.
    bytes: Option<usize>,
    failed: bool,
    generation: u64,
    last_drawn: u64,
    /// Every view that drew it: any of them may replay a cached frame that
    /// shows it, so all are notified before it is unloaded.
    views: SmallVec<[EntityId; 2]>,
    marked: bool,
    /// Marked in this trim and drawn again, so on screen: not marked again
    /// until the next trim.
    kept: bool,
}

/// What a trim step decided.
#[derive(Debug, Default, PartialEq)]
struct TrimStep {
    /// Entries to unload now: marked by the last step and not drawn since,
    /// and failed loads not drawn since.
    unloaded: Vec<u64>,
    /// Views to redraw, so the entries just marked get a chance to be drawn.
    notify: Vec<EntityId>,
    /// Whether another step must follow a frame.
    more: bool,
}

impl Ledger {
    fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            used_bytes: 0,
            generation: 0,
            clock: 0,
            entries: HashMap::new(),
            grew: false,
            first_step: false,
        }
    }

    /// Record that `view` drew the entry `key` in the current frame.
    fn touch(&mut self, key: u64, view: EntityId) {
        self.clock += 1;
        let entry = self.entries.entry(key).or_default();
        entry.generation = self.generation;
        entry.last_drawn = self.clock;
        if !entry.views.contains(&view) {
            entry.views.push(view);
        }
    }

    fn is_accounted(&self, key: u64) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|entry| entry.bytes.is_some())
    }

    /// Record the decoded size of a loaded entry.
    fn account(&mut self, key: u64, bytes: usize) {
        let entry = self.entries.entry(key).or_default();
        if let Some(previous) = entry.bytes.replace(bytes) {
            self.used_bytes -= previous;
        }
        self.used_bytes += bytes;
        self.grew = true;
    }

    /// Record that an entry failed to load. It is forgotten, so loaded
    /// again when drawn, once a trim finds it off screen.
    fn fail(&mut self, key: u64) {
        self.entries.entry(key).or_default().failed = true;
    }

    fn is_over_budget(&self) -> bool {
        self.used_bytes > self.max_bytes
    }

    /// Begin a trim if images loaded since the last one began and the cache
    /// is over budget. Returns whether it began.
    fn start_trim(&mut self) -> bool {
        if !self.grew || !self.is_over_budget() {
            return false;
        }
        self.grew = false;
        self.first_step = true;
        for entry in self.entries.values_mut() {
            entry.kept = false;
        }
        true
    }

    /// Unload the entries marked last step that weren't drawn since, and
    /// the failed ones not drawn since. Then, while over budget, mark the
    /// least recently drawn of those not drawn since (and not kept in this
    /// trim), and advance the generation. The trim ends at a step that
    /// marks nothing, except its first: the frame before it may have drawn
    /// everything.
    fn step(&mut self) -> TrimStep {
        let mut step = TrimStep::default();
        let since = self.generation;

        for (key, entry) in &mut self.entries {
            let drawn_since = entry.generation >= since;
            if entry.marked && drawn_since {
                entry.marked = false;
                entry.kept = true;
            } else if (entry.marked || entry.failed) && !drawn_since {
                step.unloaded.push(*key);
            }
        }
        for key in &step.unloaded {
            if let Some(entry) = self.entries.remove(key) {
                self.used_bytes -= entry.bytes.unwrap_or(0);
            }
        }

        let mut marked = false;
        if self.is_over_budget() {
            let mut candidates = self
                .entries
                .iter()
                .filter(|(_, entry)| {
                    entry.bytes.is_some() && entry.generation < since && !entry.kept
                })
                .map(|(key, entry)| (entry.last_drawn, *key))
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            let mut excess = self.used_bytes - self.max_bytes;
            for (_, key) in candidates {
                if excess == 0 {
                    break;
                }
                let entry = self.entries.get_mut(&key).expect("a candidate");
                entry.marked = true;
                excess = excess.saturating_sub(entry.bytes.unwrap_or(0));
                for view in &entry.views {
                    if !step.notify.contains(view) {
                        step.notify.push(*view);
                    }
                }
                marked = true;
            }
        }

        step.more = marked || std::mem::take(&mut self.first_step);
        self.generation += 1;
        step
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        AnyView, Context, IntoElement, ParentElement as _, Render, StyleRefinement, Styled as _,
        TestAppContext, WindowHandle, div, img, px,
    };

    use super::*;

    fn view(id: u64) -> EntityId {
        EntityId::from(id)
    }

    #[test]
    fn unloads_only_what_the_frame_after_marking_did_not_draw() {
        let mut ledger = Ledger::new(1000);
        for key in [1, 2] {
            ledger.touch(key, view(10));
            ledger.account(key, 400);
        }
        assert!(!ledger.start_trim(), "within budget");

        // C takes the cache past its budget, and a trim begins. Its first
        // step marks nothing: A and B were drawn since the last step, and
        // may still be on screen in a frame replayed from a cache.
        ledger.touch(3, view(11));
        ledger.account(3, 400);
        assert!(ledger.start_trim());
        let step = ledger.step();
        assert_eq!(
            step,
            TrimStep {
                more: true,
                ..TrimStep::default()
            }
        );

        // The next frame draws C alone. A and B weren't drawn since: the
        // least recently drawn, A, is enough to mark, and its view is
        // notified.
        ledger.touch(3, view(11));
        let step = ledger.step();
        assert_eq!(
            step,
            TrimStep {
                unloaded: vec![],
                notify: vec![view(10)],
                more: true,
            }
        );

        // The notified view redraws B but not A: A goes, the cache is
        // within budget, and the trim ends.
        ledger.touch(2, view(10));
        let step = ledger.step();
        assert_eq!(
            step,
            TrimStep {
                unloaded: vec![1],
                notify: vec![],
                more: false,
            }
        );
        assert_eq!(ledger.used_bytes, 800);
    }

    /// Two views each replay a cached frame with an image, together over
    /// the budget: each is asked to redraw once, keeps its image, and the
    /// trim ends.
    #[test]
    fn a_trim_over_images_on_screen_ends() {
        let mut ledger = Ledger::new(500);
        for key in [1, 2] {
            ledger.touch(key, view(key));
            ledger.account(key, 400);
        }
        assert!(ledger.start_trim());
        let mut notified = Vec::new();
        for steps in 0.. {
            assert!(steps < 10, "the trim should end");
            let step = ledger.step();
            assert!(step.unloaded.is_empty(), "both are on screen");
            // The notified views redraw their image; the other replays.
            for (key, id) in [(1, view(1)), (2, view(2))] {
                if step.notify.contains(&id) {
                    ledger.touch(key, id);
                }
            }
            notified.extend(step.notify);
            if !step.more {
                break;
            }
        }
        assert_eq!(notified, vec![view(1), view(2)], "each redrawn once");

        // Drawing one again starts no trim; a new image does, in which the
        // ones kept may be marked again: here the other, drawn least
        // recently.
        ledger.touch(1, view(1));
        assert!(!ledger.start_trim());
        ledger.touch(3, view(3));
        ledger.account(3, 100);
        assert!(ledger.start_trim());
        assert_eq!(ledger.step().notify, vec![view(2)]);
    }

    #[test]
    fn every_view_that_drew_an_image_is_notified_before_it_goes() {
        let mut ledger = Ledger::new(100);
        for id in 1..=6 {
            ledger.touch(1, view(id));
        }
        ledger.account(1, 400);
        assert!(ledger.start_trim());
        ledger.step();
        let step = ledger.step();
        assert_eq!(step.notify, (1..=6).map(view).collect::<Vec<_>>());
        assert!(step.unloaded.is_empty());
    }

    #[test]
    fn failed_loads_are_forgotten_once_off_screen() {
        let mut ledger = Ledger::new(100);
        ledger.touch(1, view(1));
        ledger.fail(1);
        ledger.touch(2, view(1));
        ledger.account(2, 400);
        assert!(ledger.start_trim());
        assert!(ledger.step().unloaded.is_empty(), "drawn since");
        ledger.touch(2, view(1));
        let step = ledger.step();
        assert_eq!(step.unloaded, vec![1]);
        assert!(!step.more);
    }

    #[test]
    fn thumbnails_fit_without_growing() {
        let image = size(DevicePixels(2000), DevicePixels(1000));
        assert_eq!(
            fitted_size(image, size(DevicePixels(400), DevicePixels(400))),
            Some((400, 200))
        );
        assert_eq!(
            fitted_size(image, size(DevicePixels(4000), DevicePixels(100))),
            Some((200, 100))
        );
        assert_eq!(
            fitted_size(image, size(DevicePixels(4000), DevicePixels(4000))),
            None
        );
    }

    /// A GIF whose logical screen is `width`×`height`, with `frames` frames
    /// of one pixel each: a few bytes per frame that each decode to the
    /// whole screen.
    fn gif(width: u16, height: u16, frames: usize) -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        // A global color table of two colors.
        bytes.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        for _ in 0..frames {
            // A graphic control extension: a 10 cs delay.
            bytes.extend([0x21, 0xF9, 4, 0, 10, 0, 0, 0]);
            // A 1×1 image at the origin.
            bytes.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
            bytes.extend([2, 2, 0x44, 0x01, 0]);
        }
        bytes.push(0x3B);
        bytes
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A small PNG whose header claims `width`×`height`.
    fn png_claiming(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::new(2, 2)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        // The IHDR chunk follows the 8-byte signature and its 4-byte length.
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        bytes
    }

    #[test]
    fn a_gif_header_counts_its_frames_without_decoding() {
        let bomb = gif(2000, 2000, 25);
        assert_eq!(bomb.len(), 595);
        assert_eq!(
            gif_header(&bomb).unwrap(),
            ImageHeader {
                width: 2000,
                height: 2000,
                frames: 25,
            }
        );
        assert_eq!(gif_header(&bomb[..bomb.len() / 2]).unwrap().frames, 12);
        assert!(gif_header(b"\x89PNG\r\n\x1a\n").is_err());
    }

    /// A 595-byte GIF whose 25 frames decode to 400 MB, a GIF of one
    /// 256 MB frame, and a PNG claiming 20000×20000: each refused, or cut to
    /// a first frame, from its header.
    #[gpui::test]
    fn images_past_the_budget_are_refused_before_decoding(cx: &mut TestAppContext) {
        let svg = cx.update(|cx| cx.svg_renderer());
        let limit = 64 * 1024 * 1024;
        let decode_bytes = |format, bytes: Vec<u8>, fit: Option<Size<DevicePixels>>| {
            decode(&Image::from_bytes(format, bytes), svg.clone(), fit, limit)
        };

        let bomb = gif(2000, 2000, 25);
        let full = decode_bytes(ImageFormat::Gif, bomb.clone(), None).unwrap();
        assert_eq!(full.frame_count(), 1, "the animation passes the budget");
        assert_eq!(full.size(0), size(DevicePixels(2000), DevicePixels(2000)));
        let thumbnail = decode_bytes(
            ImageFormat::Gif,
            bomb,
            Some(size(DevicePixels(40), DevicePixels(40))),
        )
        .unwrap();
        assert_eq!(thumbnail.frame_count(), 1);
        assert_eq!(thumbnail.size(0), size(DevicePixels(40), DevicePixels(40)));

        let small = decode_bytes(ImageFormat::Gif, gif(20, 10, 3), None).unwrap();
        assert_eq!(small.frame_count(), 3, "an animation within it plays");

        for (format, bytes) in [
            (ImageFormat::Gif, gif(8000, 8000, 1)),
            (ImageFormat::Png, png_claiming(20000, 20000)),
        ] {
            let error = decode_bytes(format, bytes, None).unwrap_err();
            assert!(
                error.to_string().contains("image budget"),
                "refused for its size: {error}"
            );
        }
    }

    /// One in-memory image, drawn through the cache, counting renders.
    struct Thumb {
        cache: Entity<LruImageCache>,
        image: Option<Arc<Image>>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Thumb {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().children(
                self.image
                    .clone()
                    .map(|image| img(LruImageCache::source(&self.cache, image, None)).size_8()),
            )
        }
    }

    /// Cached child views side by side, like the rows of a transcript.
    struct Rows {
        rows: Vec<Entity<Thumb>>,
        visible: usize,
    }

    impl Render for Rows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().children(
                self.rows[..self.visible].iter().map(|row| {
                    AnyView::from(row.clone()).cached(StyleRefinement::default().size_8())
                }),
            )
        }
    }

    impl Rows {
        fn new(
            cache: &Entity<LruImageCache>,
            images: impl IntoIterator<Item = Option<Arc<Image>>>,
            cx: &mut Context<Self>,
        ) -> Self {
            let rows = images
                .into_iter()
                .map(|image| {
                    cx.new(|_| Thumb {
                        cache: cache.clone(),
                        image,
                        renders: Rc::default(),
                    })
                })
                .collect::<Vec<_>>();
            Self {
                visible: rows.len(),
                rows,
            }
        }
    }

    fn svg(width: u32, height: u32, color: &str) -> Arc<Image> {
        Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="{color}"/></svg>"#
            )
            .into_bytes(),
        ))
    }

    /// One frame as the platform runs it: the next-frame callbacks, a draw,
    /// then the executor (decodes, notifications).
    fn frame(window: impl Into<AnyWindowHandle>, cx: &mut TestAppContext) {
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn renders(window: WindowHandle<Rows>, cx: &mut TestAppContext) -> usize {
        window
            .update(cx, |rows, _, cx| {
                rows.rows.iter().map(|row| row.read(cx).renders.get()).sum()
            })
            .unwrap()
    }

    /// Two cached views each show an image (51,200 bytes, an 80×40 SVG
    /// rendered at twice its size), together over an 80,000-byte budget.
    /// Redrawing one of them doesn't set off trims that redraw them again
    /// and again.
    #[gpui::test]
    fn images_on_screen_over_budget_settle(cx: &mut TestAppContext) {
        let cache = cx.update(|cx| LruImageCache::new(80_000, cx));
        let window = cx.add_window(|_, cx| {
            Rows::new(
                &cache,
                [Some(svg(80, 40, "red")), Some(svg(80, 40, "green"))],
                cx,
            )
        });
        for _ in 0..8 {
            frame(window, cx);
        }
        let settled = renders(window, cx);
        for _ in 0..10 {
            frame(window, cx);
        }
        assert_eq!(renders(window, cx), settled, "idle");

        // Something redraws the first row once (a hover, a streamed line).
        let first = window
            .update(cx, |rows, _, _| rows.rows[0].clone())
            .unwrap();
        first.update(cx, |_, cx| cx.notify());
        for _ in 0..60 {
            frame(window, cx);
        }
        let redrawn = renders(window, cx) - settled;
        assert!(redrawn <= 4, "the rows redrew {redrawn} times in 60 frames");
        cache.read_with(cx, |cache, _| {
            assert_eq!(cache.len(), 2);
            assert_eq!(cache.used_bytes(), 2 * 51_200, "both shown");
        });
    }

    /// A window closes while a trim step waits for its next frame; the other
    /// windows go on trimming.
    #[gpui::test]
    fn a_window_closed_mid_trim_leaves_trimming_to_the_others(cx: &mut TestAppContext) {
        let cache = cx.update(|cx| LruImageCache::new(80_000, cx));
        let closing = cx.add_window(|_, cx| {
            Rows::new(
                &cache,
                [Some(svg(80, 40, "red")), Some(svg(80, 41, "red"))],
                cx,
            )
        });
        // Decode, then draw the decoded images, which pass the budget and
        // schedule a trim step on the window's next frame; close it first.
        for _ in 0..3 {
            cx.update_window(closing.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            cx.run_until_parked();
        }
        assert!(cache.read_with(cx, |cache, _| cache.pending.is_some()));
        cx.update_window(closing.into(), |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();

        // Another window shows twelve images in turn.
        let window = cx.add_window(|_, cx| Rows::new(&cache, [None], cx));
        let row = window
            .update(cx, |rows, _, _| rows.rows[0].clone())
            .unwrap();
        for ix in 0..12 {
            row.update(cx, |row, cx| {
                row.image = Some(svg(80, 40 + ix, "blue"));
                cx.notify();
            });
            for _ in 0..5 {
                frame(window, cx);
            }
        }
        let held = cache.read_with(cx, |cache, _| cache.len());
        assert!(held <= 3, "the cache holds {held} images");
    }

    /// One image drawn by five cached views, four of which leave the screen:
    /// when a new image starts a trim, the one left is notified too, redraws,
    /// and keeps its image.
    #[gpui::test]
    fn an_image_in_many_views_stays_under_each(cx: &mut TestAppContext) {
        let cache = cx.update(|cx| LruImageCache::new(100_000, cx));
        let shared = svg(80, 40, "red");
        let window = cx.add_window(|_, cx| {
            let mut images = vec![Some(shared.clone()); 5];
            images.push(None);
            let mut rows = Rows::new(&cache, images, cx);
            rows.visible = 5;
            rows
        });
        let rows = window.update(cx, |rows, _, _| rows.rows.clone()).unwrap();
        for _ in 0..4 {
            frame(window, cx);
        }
        let decoded = cache.read_with(cx, |cache, _| {
            cache
                .images
                .values()
                .find_map(Load::image)
                .expect("decoded")
        });
        // All five draw it in one frame.
        for row in &rows[..5] {
            row.update(cx, |_, cx| cx.notify());
        }
        frame(window, cx);

        // Rows 2-5 leave the screen; row 1 stays, replaying its frame.
        window
            .update(cx, |rows, _, cx| {
                rows.rows.swap(1, 5);
                rows.visible = 2;
                cx.notify();
            })
            .unwrap();
        frame(window, cx);
        let before = rows[0].read_with(cx, |row, _| row.renders.get());

        // A new image (76,800 bytes) takes the cache past its budget.
        rows[5].update(cx, |row, cx| {
            row.image = Some(svg(80, 60, "blue"));
            cx.notify();
        });
        for _ in 0..10 {
            frame(window, cx);
        }
        let redrawn = rows[0].read_with(cx, |row, _| row.renders.get()) - before;
        assert!(redrawn > 0, "the first row was asked to redraw");
        let in_atlas = cx
            .update_window(window.into(), |_, window, _| {
                window.has_image_atlas_entry(&decoded)
            })
            .unwrap();
        assert!(in_atlas, "the image under the first row stays");
    }

    /// In-memory images decode off the UI thread, scaled to fit, and the
    /// cache unloads the ones no longer drawn once it passes its budget.
    #[gpui::test]
    fn decodes_scales_and_unloads_in_memory_images(cx: &mut TestAppContext) {
        struct Shows(ImageSource);
        impl Render for Shows {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(img(self.0.clone()).size_8())
            }
        }
        // Room for one full image (51,200 bytes), not two.
        let cache = cx.update(|cx| LruImageCache::new(80_000, cx));
        let fit = Some(size(px(20.), px(20.)));
        let window =
            cx.add_window(|_, _| Shows(LruImageCache::source(&cache, svg(80, 40, "red"), fit)));
        for _ in 0..3 {
            frame(window, cx);
        }
        let scale = window
            .update(cx, |_, window, _| window.scale_factor())
            .unwrap();
        let thumbnail = cache.read_with(cx, |cache, _| {
            cache
                .images
                .values()
                .find_map(Load::image)
                .expect("decoded")
                .size(0)
        });
        assert_eq!(
            thumbnail,
            size(
                DevicePixels((20. * scale) as i32),
                DevicePixels((10. * scale) as i32)
            ),
            "scaled to fit, keeping the aspect ratio"
        );

        // Two full images drawn one after another: the earlier ones go.
        for color in ["green", "blue"] {
            window
                .update(cx, |view, _, cx| {
                    view.0 = LruImageCache::source(&cache, svg(80, 40, color), None);
                    cx.notify();
                })
                .unwrap();
            for _ in 0..5 {
                frame(window, cx);
            }
        }
        cache.read_with(cx, |cache, _| {
            assert_eq!(cache.len(), 1, "only the image on screen stays");
            assert_eq!(cache.used_bytes(), 51_200);
        });
    }
}
