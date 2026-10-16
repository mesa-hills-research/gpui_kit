//! A full-window viewer for browsing a set of images.
//!
//! Open one with [`WindowExt::open_image_viewer`](crate::WindowExt::open_image_viewer):
//! it covers the window below the title bar, shows one image fitted to the
//! window, and browses the others with the arrow keys. Escape closes it and
//! gives focus back to whatever had it before, usually the thumbnail that
//! opened it.

use std::rc::Rc;

use gpui::{
    AnimationExt as _, App, Context, Entity, EventEmitter, FocusHandle, Focusable, ImageSource,
    InteractiveElement as _, IntoElement, KeyBinding, ObjectFit, ParentElement as _, Render,
    RenderOnce, Role, SharedString, Size, StatefulInteractiveElement as _, Styled,
    StyledImage as _, Window, actions, div, img, prelude::FluentBuilder as _, px, size,
};
use gpui_base::{FocusTrapElement as _, InteractiveElementExt as _, TestSupportExt as _};
use rust_i18n::t;

use crate::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    actions::{Cancel, SelectFirst, SelectLast, SelectLeft, SelectRight},
    button::{Button, ButtonVariants as _},
    h_flex,
    time::time_field::tabular_figures,
    v_flex,
};

pub(crate) const CONTEXT: &str = "ImageViewer";

actions!(
    image_viewer,
    [
        /// Show the image at its actual size, or fit it to the window again.
        ToggleActualSize
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
        KeyBinding::new("z", ToggleActualSize, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
    ]);
}

/// One image an [`ImageViewer`] shows.
#[derive(Clone)]
pub struct ImageViewerItem {
    source: ImageSource,
    caption: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    dimensions: Option<Size<u32>>,
}

impl ImageViewerItem {
    /// An image from any source: a path, a URL, an `Arc<Image>`, or a source
    /// from an image cache such as `LruImageCache::source`.
    pub fn new(source: impl Into<ImageSource>) -> Self {
        Self {
            source: source.into(),
            caption: None,
            accessibility_label: None,
            dimensions: None,
        }
    }

    /// A line shown with the image, such as its path and pixel size.
    pub fn with_caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// What the image shows, read by assistive technology. Defaults to the
    /// caption, else "Image 2 of 5".
    pub fn with_accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// The image's size in pixels. With it, actual size shows one image
    /// pixel per screen pixel; without it, one per logical pixel.
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.dimensions = Some(size(width, height));
        self
    }

    /// The image's source.
    pub fn source(&self) -> &ImageSource {
        &self.source
    }

    /// The line shown with the image.
    pub fn caption(&self) -> Option<&SharedString> {
        self.caption.as_ref()
    }

    /// What the image shows, for assistive technology.
    pub fn accessibility_label(&self) -> Option<&SharedString> {
        self.accessibility_label.as_ref()
    }

    /// The image's size in pixels, when known.
    pub fn dimensions(&self) -> Option<Size<u32>> {
        self.dimensions
    }
}

/// A change in an [`ImageViewerState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageViewerEvent {
    /// Another image was selected; holds its index.
    Select(usize),
}

/// The images an [`ImageViewer`] browses, which one it shows, and at what
/// scale.
pub struct ImageViewerState {
    items: Vec<ImageViewerItem>,
    selected_index: usize,
    actual_size: bool,
    focus_handle: FocusHandle,
}

impl EventEmitter<ImageViewerEvent> for ImageViewerState {}

impl Focusable for ImageViewerState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ImageViewerState {
    /// A viewer of `items`, showing the one at `selected_index` (clamped),
    /// fitted to the window.
    pub fn new(
        items: impl IntoIterator<Item = ImageViewerItem>,
        selected_index: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let items = items.into_iter().collect::<Vec<_>>();
        Self {
            selected_index: selected_index.min(items.len().saturating_sub(1)),
            items,
            actual_size: false,
            focus_handle: cx.focus_handle().tab_stop(true),
        }
    }

    /// The images, in browsing order.
    pub fn items(&self) -> &[ImageViewerItem] {
        &self.items
    }

    /// Replace the images, keeping the selected index where it still fits.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = ImageViewerItem>,
        cx: &mut Context<Self>,
    ) {
        self.items = items.into_iter().collect();
        self.selected_index = self.selected_index.min(self.items.len().saturating_sub(1));
        cx.notify();
    }

    /// The index of the image shown.
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// The image shown.
    pub fn selected_item(&self) -> Option<&ImageViewerItem> {
        self.items.get(self.selected_index)
    }

    /// Show the image at `ix`, fitted to the window. Out-of-range indices
    /// are ignored.
    pub fn set_selected_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.items.len() || ix == self.selected_index {
            return;
        }
        self.selected_index = ix;
        self.actual_size = false;
        cx.emit(ImageViewerEvent::Select(ix));
        cx.notify();
    }

    /// Show the previous image; the first stays.
    pub fn select_previous(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected_index.checked_sub(1) {
            self.set_selected_index(ix, cx);
        }
    }

    /// Show the next image; the last stays.
    pub fn select_next(&mut self, cx: &mut Context<Self>) {
        self.set_selected_index(self.selected_index + 1, cx);
    }

    /// Show the first image.
    pub fn select_first(&mut self, cx: &mut Context<Self>) {
        self.set_selected_index(0, cx);
    }

    /// Show the last image.
    pub fn select_last(&mut self, cx: &mut Context<Self>) {
        self.set_selected_index(self.items.len().saturating_sub(1), cx);
    }

    /// Whether the image is shown at its actual size rather than fitted to
    /// the window.
    pub fn is_actual_size(&self) -> bool {
        self.actual_size
    }

    /// Show the image at its actual size, or fitted to the window.
    pub fn set_actual_size(&mut self, actual_size: bool, cx: &mut Context<Self>) {
        if self.actual_size != actual_size {
            self.actual_size = actual_size;
            cx.notify();
        }
    }

    /// Switch between actual size and fitted to the window.
    pub fn toggle_actual_size(&mut self, cx: &mut Context<Self>) {
        self.set_actual_size(!self.actual_size, cx);
    }

    /// The counter text, "2 / 5".
    fn counter(&self) -> SharedString {
        format!("{} / {}", self.selected_index + 1, self.items.len()).into()
    }

    /// The accessible name of the image shown.
    fn image_label(&self) -> SharedString {
        let item = self.selected_item();
        item.and_then(|item| item.accessibility_label.clone())
            .or_else(|| item.and_then(|item| item.caption.clone()))
            .unwrap_or_else(|| {
                t!(
                    "ImageViewer.image",
                    current = self.selected_index + 1,
                    total = self.items.len()
                )
                .into()
            })
    }
}

impl Render for ImageViewerState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The size an image takes at actual size: one image pixel per screen pixel.
fn actual_size(dimensions: Size<u32>, scale_factor: f32) -> Size<gpui::Pixels> {
    let scale_factor = scale_factor.max(f32::EPSILON);
    size(
        px(dimensions.width as f32 / scale_factor),
        px(dimensions.height as f32 / scale_factor),
    )
}

/// The viewer's surface: the image, its counter and caption, and the
/// controls. [`WindowExt::open_image_viewer`](crate::WindowExt::open_image_viewer)
/// mounts one over the window; mount it yourself only in an overlay of your
/// own, and close that overlay in [`ImageViewer::on_close`].
#[derive(IntoElement)]
pub struct ImageViewer {
    state: Entity<ImageViewerState>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ImageViewer {
    /// The surface of `state`.
    pub fn new(state: &Entity<ImageViewerState>) -> Self {
        Self {
            state: state.clone(),
            on_close: None,
        }
    }

    /// Called when the viewer asks to close: Escape or its close button.
    pub fn on_close(mut self, on_close: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(on_close));
        self
    }
}

impl RenderOnce for ImageViewer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let focus_handle = state.focus_handle.clone();
        let count = state.items.len();
        let ix = state.selected_index;
        let is_actual_size = state.actual_size;
        let counter = state.counter();
        let image_label = state.image_label();
        let item = state.selected_item().cloned();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let error_color = theme.muted_foreground;
        let top = theme.sheet.margin_top;
        let insets = crate::window_border::window_content_insets(window);
        let scale_factor = window.scale_factor();
        let close = self.on_close.clone();
        let close_button = self.on_close.clone();
        let entity = self.state.clone();

        let image = item.map(|item| {
            let size = item
                .dimensions
                .filter(|_| is_actual_size)
                .map(|dimensions| actual_size(dimensions, scale_factor));
            img(item.source.clone())
                .id(("image-viewer-image", ix))
                .with_fallback(move || {
                    div()
                        .text_sm()
                        .text_color(error_color)
                        .child(SharedString::from(t!("ImageViewer.error")))
                        .into_any_element()
                })
                .map(|this| {
                    if is_actual_size {
                        this.flex_none()
                            .when_some(size, |this, size| this.w(size.width).h(size.height))
                    } else {
                        // Fitted: shrunk to the window, never enlarged.
                        this.size_full().object_fit(ObjectFit::ScaleDown)
                    }
                })
        });

        let surface = div()
            .id("image-viewer")
            .test_support()
            .key_context(CONTEXT)
            .track_focus(&focus_handle)
            .role(Role::Dialog)
            .aria_label(SharedString::from(t!("ImageViewer.label")))
            .size_full()
            .occlude()
            .bg(theme.tokens.background)
            .text_color(theme.foreground)
            .on_action(
                window.listener_for(&entity, |state, _: &SelectLeft, _, cx| {
                    state.select_previous(cx)
                }),
            )
            .on_action(
                window.listener_for(&entity, |state, _: &SelectRight, _, cx| {
                    state.select_next(cx)
                }),
            )
            .on_action(
                window.listener_for(&entity, |state, _: &SelectFirst, _, cx| {
                    state.select_first(cx)
                }),
            )
            .on_action(
                window.listener_for(&entity, |state, _: &SelectLast, _, cx| {
                    state.select_last(cx)
                }),
            )
            .on_action(
                window.listener_for(&entity, |state, _: &ToggleActualSize, _, cx| {
                    state.toggle_actual_size(cx)
                }),
            )
            .on_action(move |_: &Cancel, window, cx| {
                if let Some(close) = close.as_ref() {
                    close(window, cx);
                }
            })
            // The theme's overlay dims the background a step, so the viewer
            // reads as a layer above the window it covers.
            .child(div().absolute().inset_0().bg(theme.overlay))
            .child(
                v_flex()
                    .relative()
                    .size_full()
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .child(
                                div()
                                    .id("image-viewer-counter")
                                    .flex_none()
                                    .text_color(muted)
                                    .font_features(tabular_figures())
                                    .child(counter),
                            )
                            .child(
                                div()
                                    .id("image-viewer-caption")
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when_some(
                                        state.selected_item().and_then(|item| item.caption.clone()),
                                        |this, caption| this.child(caption),
                                    ),
                            )
                            .child(
                                Button::new("image-viewer-actual-size")
                                    .ghost()
                                    .small()
                                    .icon(if is_actual_size {
                                        IconName::Minimize
                                    } else {
                                        IconName::Maximize
                                    })
                                    .accessibility_label(SharedString::from(if is_actual_size {
                                        t!("ImageViewer.fit")
                                    } else {
                                        t!("ImageViewer.actual_size")
                                    }))
                                    .tooltip_with_action(
                                        SharedString::from(if is_actual_size {
                                            t!("ImageViewer.fit")
                                        } else {
                                            t!("ImageViewer.actual_size")
                                        }),
                                        &ToggleActualSize,
                                        Some(CONTEXT),
                                    )
                                    .on_click(window.listener_for(&entity, |state, _, _, cx| {
                                        state.toggle_actual_size(cx)
                                    })),
                            )
                            .child(
                                Button::new("image-viewer-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .accessibility_label(SharedString::from(t!(
                                        "ImageViewer.close"
                                    )))
                                    .tooltip_with_action(
                                        SharedString::from(t!("ImageViewer.close")),
                                        &Cancel,
                                        Some(CONTEXT),
                                    )
                                    .on_click(move |_, window, cx| {
                                        if let Some(close) = close_button.as_ref() {
                                            close(window, cx);
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("image-viewer-stage")
                            .on_double_click(window.listener_for(&entity, |state, _, _, cx| {
                                state.toggle_actual_size(cx)
                            }))
                            .test_support()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .role(Role::Image)
                            .aria_label(image_label)
                            .map(|this| {
                                if is_actual_size {
                                    // At actual size the image may be larger than
                                    // the window; it scrolls, centred while it fits.
                                    this.child(
                                        div()
                                            .id("image-viewer-scroll")
                                            .size_full()
                                            .overflow_scroll()
                                            .child(
                                                h_flex()
                                                    .min_w_full()
                                                    .min_h_full()
                                                    .justify_center()
                                                    .children(image),
                                            ),
                                    )
                                } else {
                                    this.child(
                                        h_flex()
                                            .size_full()
                                            .px_12()
                                            .pb_4()
                                            .justify_center()
                                            .children(image),
                                    )
                                }
                            })
                            .when(count > 1, |this| {
                                this.child(
                                    h_flex().absolute().left_2().top_0().bottom_0().child(
                                        Button::new("image-viewer-previous")
                                            .ghost()
                                            .icon(IconName::ChevronLeft)
                                            .disabled(ix == 0)
                                            .accessibility_label(SharedString::from(t!(
                                                "ImageViewer.previous"
                                            )))
                                            .tooltip_with_action(
                                                SharedString::from(t!("ImageViewer.previous")),
                                                &SelectLeft,
                                                Some(CONTEXT),
                                            )
                                            .on_click(
                                                window.listener_for(&entity, |state, _, _, cx| {
                                                    state.select_previous(cx)
                                                }),
                                            ),
                                    ),
                                )
                                .child(
                                    h_flex().absolute().right_2().top_0().bottom_0().child(
                                        Button::new("image-viewer-next")
                                            .ghost()
                                            .icon(IconName::ChevronRight)
                                            .disabled(ix + 1 >= count)
                                            .accessibility_label(SharedString::from(t!(
                                                "ImageViewer.next"
                                            )))
                                            .tooltip_with_action(
                                                SharedString::from(t!("ImageViewer.next")),
                                                &SelectRight,
                                                Some(CONTEXT),
                                            )
                                            .on_click(
                                                window.listener_for(&entity, |state, _, _, cx| {
                                                    state.select_next(cx)
                                                }),
                                            ),
                                    ),
                                )
                            }),
                    ),
            )
            .focus_trap("image-viewer", &focus_handle);

        // Only a fade: browsing swaps images in place, and reduced motion
        // shows the viewer at once.
        div()
            .absolute()
            .top(top)
            .left(insets.left)
            .right(insets.right)
            .bottom(insets.bottom)
            .child(surface)
            .with_animation(
                "image-viewer-fade",
                gpui::Animation::new(cx.theme().motion_tokens().duration_fast),
                |this, delta| this.opacity(delta),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};

    fn items(count: usize) -> Vec<ImageViewerItem> {
        (0..count)
            .map(|ix| {
                ImageViewerItem::new(format!("image-{ix}.png"))
                    .with_caption(format!("image-{ix}.png · 800 × 600"))
            })
            .collect()
    }

    #[test]
    fn test_image_viewer_item_builder() {
        let item = ImageViewerItem::new("shot.png")
            .with_caption("shot.png · 1920 × 1080")
            .with_accessibility_label("The login page")
            .with_dimensions(1920, 1080);
        assert_eq!(
            item.caption().map(|c| c.as_ref()),
            Some("shot.png · 1920 × 1080")
        );
        assert_eq!(
            item.accessibility_label().map(|l| l.as_ref()),
            Some("The login page")
        );
        assert_eq!(item.dimensions(), Some(size(1920, 1080)));
        assert_eq!(
            actual_size(size(1920, 1080), 2.),
            size(px(960.), px(540.)),
            "one image pixel per screen pixel"
        );
    }

    #[gpui::test]
    fn browsing_stops_at_the_ends_and_resets_the_scale(cx: &mut TestAppContext) {
        let state = cx.new(|cx| ImageViewerState::new(items(3), 9, cx));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        cx.update(|cx| {
            let events = events.clone();
            cx.subscribe(&state, move |_, event: &ImageViewerEvent, _| {
                events.borrow_mut().push(*event)
            })
            .detach();
        });

        state.update(cx, |state, cx| {
            assert_eq!(state.selected_index(), 2, "clamped to the last image");
            assert_eq!(state.counter().as_ref(), "3 / 3");
            state.select_next(cx);
            assert_eq!(state.selected_index(), 2, "the last image stays");

            state.toggle_actual_size(cx);
            assert!(state.is_actual_size());
            state.select_previous(cx);
            assert_eq!(state.selected_index(), 1);
            assert!(!state.is_actual_size(), "a new image starts fitted");

            state.select_first(cx);
            state.select_previous(cx);
            assert_eq!(state.selected_index(), 0, "the first image stays");
            state.select_last(cx);
            assert_eq!(state.counter().as_ref(), "3 / 3");

            assert_eq!(state.image_label().as_ref(), "image-2.png · 800 × 600");
            state.set_items(items(2), cx);
            assert_eq!(state.selected_index(), 1);
        });
        cx.run_until_parked();
        assert_eq!(
            *events.borrow(),
            vec![
                ImageViewerEvent::Select(1),
                ImageViewerEvent::Select(0),
                ImageViewerEvent::Select(2)
            ]
        );

        let unnamed = cx.new(|cx| {
            ImageViewerState::new(
                [ImageViewerItem::new("a.png"), ImageViewerItem::new("b.png")],
                1,
                cx,
            )
        });
        unnamed.read_with(cx, |state, _| {
            assert_eq!(state.image_label().as_ref(), "Image 2 of 2")
        });
    }
}
