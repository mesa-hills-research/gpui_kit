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
//! An image is never unloaded while it is on screen. Unloading takes two
//! frames: the cache first notifies the views that last drew an image it
//! means to unload, and unloads it only if their next frame doesn't draw it
//! again. So a view that replays a cached frame is redrawn before its
//! images go. The budget is a target: images on screen together may pass
//! it, and the cache unloads what it can when the next image arrives.

use std::{collections::HashMap, sync::Arc};

use gpui::{
    App, AppContext as _, DevicePixels, Entity, EntityId, Image, ImageCache, ImageCacheError,
    ImageCacheItem, ImageSource, Pixels, RenderImage, Resource, Size, SvgRenderer, Task,
    WeakEntity, Window, hash, size,
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
    /// Whether a trim step is scheduled for a coming frame.
    trimming: bool,
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
    /// `max_bytes` (4 bytes per pixel, per frame).
    pub fn new(max_bytes: usize, cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|cx| Self {
            this: cx.weak_entity(),
            ledger: Ledger::new(max_bytes),
            images: HashMap::new(),
            trimming: false,
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
    /// its aspect ratio; it is never scaled up. Use it for thumbnails, and
    /// no `fit` for the full image. Each size is cached on its own.
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

    /// Change the budget. A lower budget takes effect as the next image
    /// arrives.
    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.ledger.max_bytes = max_bytes;
    }

    /// The decoded size of the images held, in bytes.
    pub fn used_bytes(&self) -> usize {
        self.ledger.used_bytes
    }

    /// The number of images held, loaded or loading.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether the cache holds no images.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Unload every image now, including any on screen; they load again
    /// when drawn.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        for (_, load) in self.images.drain() {
            if let Some(image) = load.image() {
                cx.drop_image(image, Some(window));
            }
        }
        self.ledger = Ledger::new(self.ledger.max_bytes);
    }

    /// Decode `image` (scaled to fit `fit`, if given) or return it from the
    /// cache. Returns `None` while it is decoding; the current view is
    /// notified when it is ready.
    pub fn load_image(
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
                self.trim_if_over_budget(window);
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
        let this = self.this.clone();
        let task = cx.spawn(async move |cx| {
            let result = cx
                .background_executor()
                .spawn(async move { decode(&image, svg_renderer, fit) })
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
        if let Ok(image) = &result {
            self.ledger.account(key, image_bytes(image));
        }
        self.images.insert(key, Load::Decoded(result));
        for view in waiting {
            cx.notify(view);
        }
    }

    /// Start trimming on the next frame if the images pass the budget.
    fn trim_if_over_budget(&mut self, window: &mut Window) {
        if self.trimming || !self.ledger.is_over_budget() {
            return;
        }
        self.trimming = true;
        self.schedule_trim_step(window);
    }

    fn schedule_trim_step(&self, window: &mut Window) {
        let this = self.this.clone();
        window.on_next_frame(move |window, cx| {
            _ = this.update(cx, |cache, cx| cache.trim_step(window, cx));
        });
    }

    /// Unload the images marked by the last step that the frame since didn't
    /// draw, then mark the next ones and ask their views to draw again.
    fn trim_step(&mut self, window: &mut Window, cx: &mut App) {
        let step = self.ledger.step();
        for key in step.unloaded {
            if let Some(image) = self.images.remove(&key).and_then(|load| load.image()) {
                cx.drop_image(image, Some(window));
            }
        }
        for view in step.notify {
            cx.notify(view);
        }
        if step.marked {
            self.schedule_trim_step(window);
        } else {
            self.trimming = false;
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
        if let Some(Ok(image)) = &result
            && !self.ledger.is_accounted(key)
        {
            self.ledger.account(key, image_bytes(image));
        }
        self.trim_if_over_budget(window);
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

/// Decode an in-memory image, scaled down to fit `fit`. Runs off the UI
/// thread.
fn decode(
    image: &Image,
    svg_renderer: SvgRenderer,
    fit: Option<Size<DevicePixels>>,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let decoded = image.to_image_data(svg_renderer)?;
    Ok(match fit {
        Some(fit) => scale_to_fit(&decoded, fit).unwrap_or(decoded),
        None => decoded,
    })
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

/// Every frame of `image` scaled down to fit `fit`, or `None` when it
/// already fits.
fn scale_to_fit(image: &RenderImage, fit: Size<DevicePixels>) -> Option<Arc<RenderImage>> {
    let (width, height) = fitted_size(image.size(0), fit)?;
    let frames = (0..image.frame_count())
        .map(|ix| {
            let size = image.size(ix);
            let buffer = image::RgbaImage::from_raw(
                size.width.0 as u32,
                size.height.0 as u32,
                image.as_bytes(ix)?.to_vec(),
            )?;
            // The bytes are BGRA; scaling treats every channel alike.
            let scaled = image::imageops::resize(
                &buffer,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            Some(image::Frame::from_parts(scaled, 0, 0, image.delay(ix)))
        })
        .collect::<Option<SmallVec<[image::Frame; 1]>>>()?;
    Some(Arc::new(RenderImage::new(frames)))
}

/// The cache's bookkeeping: which images were drawn when, their sizes, and
/// which are marked to unload. Kept apart from the images so the policy can
/// be tested on its own.
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
}

#[derive(Default)]
struct LedgerEntry {
    /// Decoded bytes, once loaded.
    bytes: Option<usize>,
    generation: u64,
    last_drawn: u64,
    /// The views that drew it, notified before it is unloaded.
    views: SmallVec<[EntityId; 2]>,
    marked: bool,
}

/// What a trim step decided.
#[derive(Debug, Default, PartialEq)]
struct TrimStep {
    /// Entries to unload now: marked by the last step, not drawn since.
    unloaded: Vec<u64>,
    /// Views to redraw, so the entries just marked get a chance to be drawn.
    notify: Vec<EntityId>,
    /// Whether entries were marked, so another step must follow a frame.
    marked: bool,
}

impl Ledger {
    fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            used_bytes: 0,
            generation: 0,
            clock: 0,
            entries: HashMap::new(),
        }
    }

    /// Record that `view` drew the entry `key` in the current frame.
    fn touch(&mut self, key: u64, view: EntityId) {
        self.clock += 1;
        let entry = self.entries.entry(key).or_default();
        entry.generation = self.generation;
        entry.last_drawn = self.clock;
        if !entry.views.contains(&view) {
            if entry.views.len() == 4 {
                entry.views.remove(0);
            }
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
    }

    fn is_over_budget(&self) -> bool {
        self.used_bytes > self.max_bytes
    }

    /// Unload the entries marked last step that weren't drawn since, then,
    /// while over budget, mark the least recently drawn of those not drawn
    /// since the last step, and advance the generation.
    fn step(&mut self) -> TrimStep {
        let mut step = TrimStep::default();
        let since = self.generation;

        let mut unloaded = Vec::new();
        for (key, entry) in &mut self.entries {
            if entry.marked {
                if entry.generation >= since {
                    entry.marked = false;
                } else {
                    unloaded.push(*key);
                }
            }
        }
        for key in unloaded {
            if let Some(entry) = self.entries.remove(&key) {
                self.used_bytes -= entry.bytes.unwrap_or(0);
            }
            step.unloaded.push(key);
        }

        if self.is_over_budget() {
            let mut candidates = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.bytes.is_some() && entry.generation < since)
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
                step.marked = true;
            }
        }

        self.generation += 1;
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: u64) -> EntityId {
        EntityId::from(id)
    }

    #[test]
    fn unloads_only_what_the_frame_after_marking_did_not_draw() {
        let mut ledger = Ledger::new(1000);
        // Frame 1 draws A and B.
        for key in [1, 2] {
            ledger.touch(key, view(10));
            ledger.account(key, 400);
        }
        assert!(!ledger.is_over_budget());

        // Frame 2 draws C alone; the budget is passed. A step marks nothing
        // yet: A and B were drawn since the last step, and may still be on
        // screen in a frame replayed from a cache.
        ledger.touch(3, view(10));
        ledger.account(3, 400);
        assert!(ledger.is_over_budget());
        assert_eq!(ledger.step(), TrimStep::default());

        // Frame 3 draws D alone. Now A and B were not drawn since the last
        // step: the least recently drawn is marked and its view notified.
        ledger.touch(4, view(11));
        ledger.account(4, 400);
        let step = ledger.step();
        assert!(step.marked);
        assert!(step.unloaded.is_empty());
        assert_eq!(step.notify, vec![view(10)]);

        // The notified view redraws B (still on screen) but not A: A goes,
        // B stays, and as C is now the stalest, it is marked.
        ledger.touch(2, view(10));
        ledger.touch(4, view(11));
        let step = ledger.step();
        assert_eq!(step.unloaded, vec![1]);
        assert_eq!(ledger.used_bytes, 1200);

        // Nothing more to mark once everything left was drawn since.
        ledger.touch(2, view(10));
        ledger.touch(4, view(11));
        let step = ledger.step();
        assert_eq!(step.unloaded, vec![3]);
        assert_eq!(ledger.used_bytes, 800);
        assert!(!ledger.is_over_budget());
        assert!(!ledger.step().marked);
    }

    #[test]
    fn a_visible_set_larger_than_the_budget_is_kept() {
        let mut ledger = Ledger::new(500);
        for key in [1, 2, 3] {
            ledger.touch(key, view(1));
            ledger.account(key, 400);
        }
        for _ in 0..3 {
            let step = ledger.step();
            for key in [1, 2, 3] {
                ledger.touch(key, view(1));
            }
            assert!(step.unloaded.is_empty());
        }
        assert_eq!(ledger.entries.len(), 3);
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

    struct Thumbnails {
        cache: Entity<LruImageCache>,
        images: Vec<Arc<Image>>,
        fit: Option<Size<Pixels>>,
    }

    impl gpui::Render for Thumbnails {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{ParentElement as _, Styled as _};
            gpui::div()
                .size_full()
                .children(self.images.iter().map(|image| {
                    gpui::img(LruImageCache::source(&self.cache, image.clone(), self.fit)).size_8()
                }))
        }
    }

    fn svg(width: u32, height: u32, color: &str) -> Arc<Image> {
        Arc::new(Image::from_bytes(
            gpui::ImageFormat::Svg,
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="{color}"/></svg>"#
            )
            .into_bytes(),
        ))
    }

    /// In-memory images decode off the UI thread, scaled to fit, and the
    /// cache unloads the one no longer drawn once it passes its budget.
    #[gpui::test]
    fn decodes_scales_and_unloads_in_memory_images(cx: &mut gpui::TestAppContext) {
        let (red, green, blue) = (
            svg(80, 40, "red"),
            svg(80, 40, "green"),
            svg(80, 40, "blue"),
        );
        let cache = cx.update(|cx| LruImageCache::new(1, cx));
        let (view, cx) = cx.add_window_view(|_, _| Thumbnails {
            cache: cache.clone(),
            images: vec![red.clone()],
            fit: Some(size(gpui::px(20.), gpui::px(20.))),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let scale = cx.update(|window, _| window.scale_factor());
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

        // Two more images, drawn one after another, push the cache past its
        // one-byte budget; the earlier ones go once the frames after the
        // trim steps no longer draw them.
        for image in [green.clone(), blue.clone()] {
            view.update(cx, |view, cx| {
                view.images = vec![image];
                cx.notify();
            });
            for _ in 0..4 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
                cx.update(|window, cx| window.simulate_next_frame(cx));
            }
        }
        cache.read_with(cx, |cache, _| {
            assert_eq!(cache.len(), 1, "only the image on screen stays");
            assert!(cache.used_bytes() > 0);
        });
    }
}
