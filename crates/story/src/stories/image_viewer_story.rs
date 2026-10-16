use std::sync::Arc;

use gpui_kit::base::LruImageCache;
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _, h_flex,
    image_viewer::{ImageViewerItem, ImageViewerState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Image, ImageFormat,
    InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img, px, rems,
    size,
};

use crate::{Story, section};

/// Screenshots held in memory, the way a tool returns them.
struct Screenshot {
    name: SharedString,
    image: Arc<Image>,
    width: u32,
    height: u32,
}

fn screenshot(name: &str, width: u32, height: u32, hue: u32) -> Screenshot {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">
<rect width="100%" height="100%" fill="hsl({hue}, 45%, 62%)"/>
<rect x="5%" y="6%" width="90%" height="10%" rx="12" fill="hsl({hue}, 40%, 82%)"/>
<rect x="5%" y="22%" width="38%" height="68%" rx="12" fill="hsl({hue}, 40%, 75%)"/>
<rect x="48%" y="22%" width="47%" height="30%" rx="12" fill="hsl({hue}, 40%, 88%)"/>
<rect x="48%" y="58%" width="47%" height="32%" rx="12" fill="hsl({hue}, 40%, 70%)"/>
</svg>"#
    );
    Screenshot {
        name: name.to_string().into(),
        image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
        width,
        height,
    }
}

pub struct ImageViewerStory {
    focus_handle: FocusHandle,
    cache: Entity<LruImageCache>,
    screenshots: Vec<Screenshot>,
    thumbnail_focus: Vec<FocusHandle>,
    viewer: Option<Entity<ImageViewerState>>,
}

impl ImageViewerStory {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let screenshots = vec![
            screenshot("login.png", 1600, 1000, 210),
            screenshot("settings.png", 1280, 800, 150),
            screenshot("error-dialog.png", 900, 640, 10),
            screenshot("diff.png", 2400, 1400, 280),
        ];
        Self {
            focus_handle: cx.focus_handle(),
            cache: LruImageCache::new(64 * 1024 * 1024, cx),
            thumbnail_focus: screenshots
                .iter()
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
            screenshots,
            viewer: None,
        }
    }

    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.screenshots.iter().map(|shot| {
            ImageViewerItem::new(LruImageCache::source(&self.cache, shot.image.clone(), None))
                .with_caption(format!("{} · {} × {}", shot.name, shot.width, shot.height))
                .with_dimensions(shot.width, shot.height)
        });
        self.viewer = Some(window.open_image_viewer(items, ix, cx));
    }
}

impl Story for ImageViewerStory {
    fn title() -> &'static str {
        "Image Viewer"
    }

    fn description() -> &'static str {
        "Browse a set of images over the window, fitted or at actual size."
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        Self::view(window, cx)
    }
}

impl Focusable for ImageViewerStory {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ImageViewerStory {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Thumbnails are scaled on a background thread to the box they fill,
        // so the list holds thumbnail pixels, not the screenshots'.
        let fit = size(
            rems(10.).to_pixels(window.rem_size()),
            rems(6.).to_pixels(window.rem_size()),
        );
        let used = self.cache.read(cx).used_bytes();
        let viewing = if window.has_active_image_viewer(cx) {
            self.viewer
                .as_ref()
                .map(|viewer| viewer.read(cx).selected_index() + 1)
        } else {
            None
        };
        v_flex().size_full().gap_6().child(
            section("Screenshots")
                .description("Click a thumbnail, or focus it and press Enter. ←/→, Home/End browse; Z or a double-click toggles actual size; Escape closes.")
                .child(
                    v_flex()
                        .gap_3()
                        .child(h_flex().gap_3().flex_wrap().children(
                            self.screenshots.iter().enumerate().map(|(ix, shot)| {
                                div()
                                    .id(("thumbnail", ix))
                                    .track_focus(&self.thumbnail_focus[ix])
                                    .w(rems(10.))
                                    .h(rems(6.))
                                    .p_1()
                                    .rounded(cx.theme().radius)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .focus_visible(|this| this.border_color(cx.theme().ring))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open(ix, window, cx)
                                    }))
                                    .on_key_down(cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                                        if event.keystroke.key == "enter" {
                                            this.open(ix, window, cx);
                                        }
                                    }))
                                    .child(
                                        img(LruImageCache::source(
                                            &self.cache,
                                            shot.image.clone(),
                                            Some(fit),
                                        ))
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                    )
                            }),
                        ))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "Image cache: {:.1} MB decoded{}",
                                    used as f64 / (1024. * 1024.),
                                    viewing
                                        .map(|ix| format!(", viewing {ix}"))
                                        .unwrap_or_default()
                                )),
                        )
                        .w(px(560.)),
                ),
        )
    }
}
