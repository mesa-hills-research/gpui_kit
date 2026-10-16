use std::rc::Rc;

use crate::{
    ActiveTheme, Icon, IconName, InteractiveElementExt as _, Sizable as _, StyledExt, h_flex,
};
use gpui::{
    AnyElement, App, ClickEvent, Context, Decorations, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Render, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, TitlebarOptions, Window,
    WindowButton, WindowButtonLayout, WindowControlArea, WindowControls as PlatformWindowControls,
    WindowOptions, div, prelude::FluentBuilder as _, px, rgb,
};
use rust_i18n::t;
use smallvec::SmallVec;

pub const TITLE_BAR_HEIGHT: Pixels = px(34.);
/// Room for macOS's traffic lights at the start of the bar.
const TRAFFIC_LIGHTS_INSET: Pixels = px(80.);
/// The bar's leading inset everywhere else.
const TITLE_BAR_INSET: Pixels = px(12.);

/// The width of a Windows caption button: Windows 11 draws its own 46
/// pixels wide, the full height of the bar.
const CAPTION_BUTTON_WIDTH: Pixels = px(46.);
/// The size of the caption glyphs, as Windows draws them.
const CAPTION_GLYPH_SIZE: Pixels = px(10.);
/// The fonts holding Windows' caption glyphs, newest first: Segoe Fluent
/// Icons on Windows 11, Segoe MDL2 Assets on Windows 10.
const CAPTION_GLYPH_FONTS: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];

/// The bar's leading inset: room for the traffic lights on macOS, except in
/// full screen, where AppKit hides them.
fn leading_inset(is_macos: bool, is_fullscreen: bool) -> Pixels {
    if is_macos && !is_fullscreen {
        TRAFFIC_LIGHTS_INSET
    } else {
        TITLE_BAR_INSET
    }
}

/// TitleBar used to customize the appearance of the title bar.
///
/// We can put some elements inside the title bar. It paints the theme's `title_bar` token, one
/// solid color unless the theme gives it a gradient.
///
/// The window buttons follow the platform: macOS keeps its traffic lights;
/// Windows gets Windows 11 caption buttons; Linux, under client-side
/// decorations, gets the buttons the compositor supports, on the sides and
/// in the order the desktop's button layout gives.
#[derive(IntoElement)]
pub struct TitleBar {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 1]>,
    on_close_window: Option<Rc<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>>,
    button_layout: Option<WindowButtonLayout>,
}

impl TitleBar {
    /// Create a new TitleBar.
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: SmallVec::new(),
            on_close_window: None,
            button_layout: None,
        }
    }

    /// Returns the default title bar options for compatible with the [`crate::TitleBar`].
    pub fn title_bar_options() -> TitlebarOptions {
        TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
        }
    }

    /// Returns the default window options for compatible with the [`crate::TitleBar`].
    ///
    /// Use this as the base of the [`WindowOptions`] of any window that renders a
    /// [`crate::TitleBar`], so the title bar owns dragging and double clicking itself:
    ///
    /// ```no_run
    /// # mod gpui_kit { pub use gpui::*; pub extern crate gpui_component as component; }
    /// # use gpui_kit::WindowOptions;
    /// # use gpui_kit::component::TitleBar;
    /// let options = WindowOptions {
    ///     window_min_size: None,
    ///     ..TitleBar::window_options()
    /// };
    /// ```
    ///
    /// On Linux, also set `app_id` to the name of the application's
    /// `.desktop` file (without `.desktop`), so the compositor groups its
    /// windows and shows its icon, and ask for client-side decorations with
    /// `window_decorations: Some(WindowDecorations::Client)` so compositors
    /// that would draw their own frame (KDE, Sway, X11) leave it to the
    /// title bar.
    pub fn window_options() -> WindowOptions {
        WindowOptions {
            titlebar: Some(Self::title_bar_options()),
            // The title bar draws itself and moves the window via `start_window_move`,
            // so AppKit must not treat it as a system window-move region. Otherwise macOS
            // handles title bar double clicks on its own (in addition to `on_double_click`
            // below) and delays title bar clicks while disambiguating double clicks.
            app_owns_titlebar_drag: true,
            ..Default::default()
        }
    }

    /// Add custom for close window event, default is None, then click X button will call `window.remove_window()`.
    /// Linux only, this will do nothing on other platforms.
    pub fn on_close_window(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        if cfg!(target_os = "linux") {
            self.on_close_window = Some(Rc::new(Box::new(f)));
        }
        self
    }

    /// Place the Linux window buttons as `layout` says, instead of the
    /// desktop's button layout (GNOME's `button-layout`), for an application
    /// that lets the user choose. Other platforms ignore it.
    pub fn button_layout(mut self, layout: WindowButtonLayout) -> Self {
        self.button_layout = Some(layout);
        self
    }
}

/// A Windows caption button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Caption {
    fn id(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Restore => "restore",
            Self::Close => "close",
        }
    }

    /// The Segoe Fluent Icons / Segoe MDL2 Assets glyph.
    fn glyph(self) -> &'static str {
        match self {
            Self::Minimize => "\u{E921}",
            Self::Maximize => "\u{E922}",
            Self::Restore => "\u{E923}",
            Self::Close => "\u{E8BB}",
        }
    }

    /// The icon drawn where neither caption font is installed.
    fn icon(self) -> IconName {
        match self {
            Self::Minimize => IconName::WindowMinimize,
            Self::Restore => IconName::WindowRestore,
            Self::Maximize => IconName::WindowMaximize,
            Self::Close => IconName::WindowClose,
        }
    }

    /// The button's accessible name.
    fn label(self) -> SharedString {
        match self {
            Self::Minimize => t!("TitleBar.minimize"),
            Self::Maximize => t!("TitleBar.maximize"),
            Self::Restore => t!("TitleBar.restore"),
            Self::Close => t!("TitleBar.close"),
        }
        .into()
    }

    fn area(self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Restore | Self::Maximize => WindowControlArea::Max,
            Self::Close => WindowControlArea::Close,
        }
    }
}

/// The colors of a caption button in each state, after Windows 11's caption
/// buttons: a subtle fill of the foreground on hover and press, the system's
/// close red behind white on close, dimmed glyphs while the window is
/// inactive or the button disabled.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CaptionColors {
    glyph: Hsla,
    hover_background: Hsla,
    hover_glyph: Hsla,
    pressed_background: Hsla,
    pressed_glyph: Hsla,
}

impl CaptionColors {
    fn new(caption: Caption, foreground: Hsla, active: bool, disabled: bool) -> Self {
        let glyph = if disabled {
            foreground.opacity(0.3)
        } else if active {
            foreground
        } else {
            foreground.opacity(0.45)
        };
        if caption == Caption::Close {
            // Windows draws the close button's hover in this red whatever the
            // theme: a platform convention, not the theme's danger color.
            let red: Hsla = rgb(0xC42B1C).into();
            let white: Hsla = rgb(0xFFFFFF).into();
            Self {
                glyph,
                hover_background: red,
                hover_glyph: white,
                pressed_background: red.opacity(0.9),
                pressed_glyph: white.opacity(0.7),
            }
        } else {
            Self {
                glyph,
                hover_background: foreground.opacity(0.06),
                hover_glyph: foreground,
                pressed_background: foreground.opacity(0.04),
                pressed_glyph: foreground.opacity(0.6),
            }
        }
    }
}

/// The font that draws Windows' caption glyphs on this machine, if any;
/// looked up once, by resolving just these families rather than listing
/// every installed font.
fn caption_glyph_font(cx: &App) -> Option<&'static str> {
    static FONT: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *FONT.get_or_init(|| {
        let text_system = cx.text_system();
        CAPTION_GLYPH_FONTS.into_iter().find(|family| {
            // A family that isn't installed resolves to a fallback font.
            let id = text_system.resolve_font(&gpui::font(*family));
            text_system
                .get_font_for_id(id)
                .is_some_and(|font| font.family.as_ref() == *family)
        })
    })
}

/// A Windows 11 caption button. Windows runs it through the hit-test code
/// its area answers, so it needs no click handler; a disabled one answers
/// none and occludes the drag area under it.
#[derive(IntoElement)]
struct CaptionButton {
    caption: Caption,
    disabled: bool,
}

impl RenderOnce for CaptionButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let caption = self.caption;
        let colors = CaptionColors::new(
            caption,
            cx.theme().foreground,
            window.is_window_active(),
            self.disabled,
        );
        let glyph_font = caption_glyph_font(cx);

        div()
            .id(caption.id())
            .role(Role::Button)
            .aria_label(caption.label())
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(CAPTION_BUTTON_WIDTH)
            .h_full()
            .text_color(colors.glyph)
            .map(|this| {
                if self.disabled {
                    this.occlude()
                } else {
                    this.window_control_area(caption.area())
                        .hover(|style| {
                            style
                                .bg(colors.hover_background)
                                .text_color(colors.hover_glyph)
                        })
                        .active(|style| {
                            style
                                .bg(colors.pressed_background)
                                .text_color(colors.pressed_glyph)
                        })
                }
            })
            .map(|this| match glyph_font {
                Some(font) => this.child(
                    div()
                        .font_family(font)
                        .text_size(CAPTION_GLYPH_SIZE)
                        .line_height(CAPTION_GLYPH_SIZE)
                        .child(caption.glyph()),
                ),
                None => this.child(Icon::new(caption.icon()).small()),
            })
    }
}

/// A Linux window button, drawn by the title bar under client-side
/// decorations.
#[derive(IntoElement)]
struct LinuxButton {
    button: WindowButton,
    on_close_window: Option<Rc<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>>,
}

impl RenderOnce for LinuxButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_close = self.button == WindowButton::Close;
        let (hover_fg, hover_bg, active_bg) = if is_close {
            (
                cx.theme().danger_foreground,
                cx.theme().danger,
                cx.theme().danger_active,
            )
        } else {
            (
                cx.theme().secondary_foreground,
                cx.theme().secondary_hover,
                cx.theme().secondary_active,
            )
        };
        let caption = match self.button {
            WindowButton::Minimize => Caption::Minimize,
            WindowButton::Maximize if window.is_maximized() => Caption::Restore,
            WindowButton::Maximize => Caption::Maximize,
            WindowButton::Close => Caption::Close,
        };
        let button = self.button;
        let on_close_window = self.on_close_window;

        div()
            .id(caption.id())
            .role(Role::Button)
            .aria_label(caption.label())
            .flex()
            .w(TITLE_BAR_HEIGHT)
            .h_full()
            .flex_shrink_0()
            .justify_center()
            .content_center()
            .items_center()
            .text_color(cx.theme().foreground)
            .hover(|style| style.bg(hover_bg).text_color(hover_fg))
            .active(|style| style.bg(active_bg).text_color(hover_fg))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                match button {
                    WindowButton::Minimize => window.minimize_window(),
                    WindowButton::Maximize => window.zoom_window(),
                    WindowButton::Close => {
                        if let Some(f) = on_close_window.clone() {
                            f(&ClickEvent::default(), window, cx);
                        } else {
                            window.remove_window();
                        }
                    }
                }
            })
            .child(Icon::new(caption.icon()).small())
    }
}

/// The Linux buttons for each side of the bar: the layout's buttons, in its
/// order, that the compositor supports. Close is always offered.
fn linux_buttons(
    layout: &WindowButtonLayout,
    supported: PlatformWindowControls,
) -> [Vec<WindowButton>; 2] {
    let offered = |button: &WindowButton| match button {
        WindowButton::Minimize => supported.minimize,
        WindowButton::Maximize => supported.maximize,
        WindowButton::Close => true,
    };
    [&layout.left, &layout.right].map(|side| {
        side.iter()
            .flatten()
            .filter(|button| offered(button))
            .copied()
            .collect()
    })
}

/// Which side of the bar a group of window buttons sits on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlsSide {
    Start,
    End,
}

#[derive(IntoElement)]
struct WindowControls {
    side: ControlsSide,
    on_close_window: Option<Rc<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>>,
    button_layout: Option<WindowButtonLayout>,
}

impl RenderOnce for WindowControls {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = match self.side {
            ControlsSide::Start => "window-controls-start",
            ControlsSide::End => "window-controls",
        };
        let controls = h_flex().id(id).items_center().flex_shrink_0().h_full();

        if cfg!(target_os = "windows") {
            if self.side == ControlsSide::Start {
                return controls;
            }
            return controls
                .child(CaptionButton {
                    caption: Caption::Minimize,
                    disabled: !window.is_minimizable(),
                })
                .child(CaptionButton {
                    caption: if window.is_maximized() {
                        Caption::Restore
                    } else {
                        Caption::Maximize
                    },
                    disabled: !window.is_resizable(),
                })
                .child(CaptionButton {
                    caption: Caption::Close,
                    disabled: false,
                });
        }

        if !cfg!(target_os = "linux") {
            return controls;
        }

        // Under server-side decorations the window manager already renders
        // its own title bar, complete with min/max/close; drawing ours as
        // well stacks a duplicate set of controls on top of it (most
        // visibly two close buttons). gpui falls back to server-side
        // decorations on X11 sessions without a compositor, and a Wayland
        // compositor may grant server mode even when client mode was
        // requested. Skip ours unless this window is actually client-side
        // decorated, mirroring the `is_client_decorated` gating of the
        // title bar's window-menu overlay.
        if !matches!(window.window_decorations(), Decorations::Client { .. }) {
            return controls;
        }

        let Some(layout) = self.button_layout.or_else(|| cx.button_layout()) else {
            return controls;
        };
        // The window manager declares which controls it can honor; a tiling
        // compositor may support neither minimize nor maximize. Close is
        // always ours to offer.
        let [start, end] = linux_buttons(&layout, window.window_controls());
        let buttons = match self.side {
            ControlsSide::Start => start,
            ControlsSide::End => end,
        };
        let on_close_window = self.on_close_window;
        controls.children(buttons.into_iter().map(|button| LinuxButton {
            button,
            on_close_window: on_close_window.clone(),
        }))
    }
}

impl Styled for TitleBar {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for TitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

struct TitleBarState {
    should_move: bool,
}

// TODO: Remove this when GPUI has released v0.2.3
impl Render for TitleBarState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let is_web = cfg!(target_family = "wasm");
        let is_linux = cfg!(target_os = "linux");
        let is_macos = cfg!(target_os = "macos");

        let state = window.use_state(cx, |_, _| TitleBarState { should_move: false });
        // Redraw when the desktop's button layout changes.
        #[cfg(target_os = "linux")]
        window.use_keyed_state("title-bar-button-layout", cx, |window, _| {
            window.observe_button_layout_changed(|window, _| window.refresh())
        });
        // Buttons at the start sit at the window's edge, as the desktop puts
        // them.
        let has_start_buttons = is_linux
            && is_client_decorated
            && self
                .button_layout
                .or_else(|| cx.button_layout())
                .is_some_and(|layout| {
                    let [start, _] = linux_buttons(&layout, window.window_controls());
                    !start.is_empty()
                });

        div().flex_shrink_0().child(
            div()
                .id("title-bar")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(TITLE_BAR_HEIGHT)
                .when(!has_start_buttons, |this| {
                    this.pl(leading_inset(is_macos, window.is_fullscreen()))
                })
                .border_b_1()
                .border_color(cx.theme().title_bar_border)
                .bg(cx.theme().tokens.title_bar)
                .refine_style(&self.style)
                .when(is_linux, |this| {
                    this.on_double_click(|_, window, _| window.zoom_window())
                })
                .when(is_macos, |this| {
                    this.on_double_click(|_, window, _| window.titlebar_double_click())
                })
                .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
                    state.should_move = false;
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| {
                        state.should_move = true;
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| {
                        state.should_move = false;
                    }),
                )
                .on_mouse_move(window.listener_for(&state, |state, _, window, _| {
                    if state.should_move {
                        state.should_move = false;
                        window.start_window_move();
                    }
                }))
                .when(has_start_buttons, |this| {
                    this.child(WindowControls {
                        side: ControlsSide::Start,
                        on_close_window: self.on_close_window.clone(),
                        button_layout: self.button_layout,
                    })
                })
                .child(
                    h_flex()
                        .id("bar")
                        .h_full()
                        .justify_between()
                        .flex_shrink_0()
                        .flex_1()
                        .when(has_start_buttons, |this| this.pl_2())
                        .when(!is_web, |this| {
                            this.window_control_area(WindowControlArea::Drag).when(
                                is_linux && is_client_decorated,
                                |this| {
                                    this.child(
                                        div()
                                            .top_0()
                                            .left_0()
                                            .absolute()
                                            .size_full()
                                            .h_full()
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                move |ev, window, _| {
                                                    window.show_window_menu(ev.position)
                                                },
                                            ),
                                    )
                                },
                            )
                        })
                        .children(self.children),
                )
                .child(WindowControls {
                    side: ControlsSide::End,
                    on_close_window: self.on_close_window,
                    button_layout: self.button_layout,
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_traffic_lights_inset_drops_in_full_screen() {
        assert_eq!(leading_inset(true, false), TRAFFIC_LIGHTS_INSET);
        assert_eq!(leading_inset(true, true), TITLE_BAR_INSET);
        assert_eq!(leading_inset(false, false), TITLE_BAR_INSET);
        assert_eq!(leading_inset(false, true), TITLE_BAR_INSET);
    }

    #[test]
    fn linux_buttons_follow_the_layout_and_what_the_compositor_supports() {
        let layout = WindowButtonLayout {
            left: [
                Some(WindowButton::Close),
                Some(WindowButton::Minimize),
                None,
            ],
            right: [Some(WindowButton::Maximize), None, None],
        };
        let all = PlatformWindowControls::default();
        assert_eq!(
            linux_buttons(&layout, all),
            [
                vec![WindowButton::Close, WindowButton::Minimize],
                vec![WindowButton::Maximize]
            ]
        );
        // A tiling compositor that offers neither keeps only close.
        let tiling = PlatformWindowControls {
            minimize: false,
            maximize: false,
            ..all
        };
        assert_eq!(
            linux_buttons(&layout, tiling),
            [vec![WindowButton::Close], vec![]]
        );
    }

    /// Every window button is a button named for what it does, so a screen
    /// reader says "Minimize" rather than a private-use glyph.
    #[gpui::test]
    fn window_buttons_are_named_buttons(cx: &mut gpui::TestAppContext) {
        use gpui::{Element as _, accesskit};
        cx.update(crate::init);
        let window = cx.add_empty_window();
        window.update(|window, cx| {
            for (caption, name) in [
                (Caption::Minimize, "Minimize"),
                (Caption::Maximize, "Maximize"),
                (Caption::Restore, "Restore"),
                (Caption::Close, "Close"),
            ] {
                let element = CaptionButton {
                    caption,
                    disabled: false,
                }
                .render(window, cx)
                .into_element();
                assert_eq!(element.a11y_role(), Some(Role::Button));
                let mut node = accesskit::Node::new(Role::Button);
                element.write_a11y_info(&mut node);
                assert_eq!(node.label(), Some(name));
            }
            let element = LinuxButton {
                button: WindowButton::Close,
                on_close_window: None,
            }
            .render(window, cx)
            .into_element();
            let mut node = accesskit::Node::new(Role::Button);
            element.write_a11y_info(&mut node);
            assert_eq!(node.label(), Some("Close"));
        });
    }

    #[test]
    fn caption_buttons_dim_while_inactive_or_disabled_and_close_turns_red() {
        let foreground: Hsla = rgb(0x1a1a1a).into();
        let active = CaptionColors::new(Caption::Minimize, foreground, true, false);
        let inactive = CaptionColors::new(Caption::Minimize, foreground, false, false);
        let disabled = CaptionColors::new(Caption::Maximize, foreground, true, true);
        assert_eq!(active.glyph, foreground);
        assert!(inactive.glyph.a < active.glyph.a);
        assert!(disabled.glyph.a < inactive.glyph.a);
        assert!(active.hover_background.a < 0.1, "a subtle fill");

        let close = CaptionColors::new(Caption::Close, foreground, true, false);
        assert_eq!(close.hover_background, rgb(0xC42B1C).into());
        assert_eq!(close.hover_glyph, rgb(0xFFFFFF).into());
        assert!(close.pressed_glyph.a < 1.);

        assert_eq!(Caption::Close.glyph(), "\u{E8BB}");
        assert_eq!(Caption::Restore.area(), WindowControlArea::Max);
    }
}
