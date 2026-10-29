use std::{rc::Rc, time::Duration};

use crate::animation::{Lerp, ease_in_out_cubic};
use crate::theme::contrast_ratio;
use crate::{
    ActiveTheme, Icon, IconName, Selectable, Sizable, Size, StyledExt, Theme, concentric_radius,
    h_flex, v_flex,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Background, ClickEvent, Div, Edges, ElementId,
    FocusHandle, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};

/// Tab variants.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Hash)]
pub enum TabVariant {
    #[default]
    Tab,
    Outline,
    Pill,
    Segmented,
    Underline,
}

/// The least contrast of the chosen segment's fill with the track.
const SEGMENT_FILL_MIN_CONTRAST: f32 = 1.15;
/// The least contrast of the chosen segment's outline with its fill and with
/// the track.
const SEGMENT_OUTLINE_MIN_CONTRAST: f32 = 1.4;
/// The least contrast of the track's outline with the window.
const TRACK_OUTLINE_MIN_CONTRAST: f32 = 1.2;
/// The most contrast of the track's outline with the window.
const TRACK_OUTLINE_MAX_CONTRAST: f32 = 2.;

impl TabVariant {
    /// The track behind segmented tabs, as an opaque color. It's the window
    /// background unless the theme sets `tab_bar.segmented.background`.
    fn segmented_track(theme: &Theme) -> Hsla {
        theme.background.blend(theme.tab_bar_segmented)
    }

    /// The fill and the outline of the chosen segment.
    ///
    /// The fill is `secondary`. Where that sits too close to the track, it's
    /// the foreground at 8% opacity on a light theme and 10% on a dark one,
    /// stronger where needed to stand out from the track. The outline is the
    /// foreground at 30% opacity on a light theme and 25% on a dark one,
    /// stronger where needed to stand out from both the fill and the track.
    fn segmented_selection(theme: &Theme) -> (Hsla, Hsla) {
        let track = Self::segmented_track(theme);
        let dark = theme.is_dark();
        let mut fill = track.blend(theme.secondary);
        if contrast_ratio(fill, track) < SEGMENT_FILL_MIN_CONTRAST {
            let mut alpha: f32 = if dark { 0.1 } else { 0.08 };
            let tint = |alpha: f32| track.blend(theme.foreground.opacity(alpha));
            while alpha < 1. && contrast_ratio(tint(alpha), track) < SEGMENT_FILL_MIN_CONTRAST {
                alpha = (alpha + 0.01).min(1.);
            }
            fill = tint(alpha);
        }
        let mut alpha: f32 = if dark { 0.25 } else { 0.3 };
        let faint = |alpha: f32| {
            let line = fill.blend(theme.foreground.opacity(alpha));
            contrast_ratio(line, fill).min(contrast_ratio(line, track))
                < SEGMENT_OUTLINE_MIN_CONTRAST
        };
        while alpha < 1. && faint(alpha) {
            alpha = (alpha + 0.05).min(1.);
        }
        (fill, theme.foreground.opacity(alpha))
    }

    /// The track's 1 px outline, in the theme's border color. A strong border
    /// is faded toward the window, so no theme frames the control heavily. A
    /// faint one gives way to the foreground at the least opacity that shows.
    pub(super) fn segmented_track_outline(theme: &Theme) -> Hsla {
        let track = Self::segmented_track(theme);
        let edge = |color: Hsla| contrast_ratio(track.blend(color), theme.background);
        if edge(theme.border) < TRACK_OUTLINE_MIN_CONTRAST {
            let mut alpha: f32 = 0.15;
            while alpha < 1. && edge(theme.foreground.opacity(alpha)) < TRACK_OUTLINE_MIN_CONTRAST {
                alpha = (alpha + 0.01).min(1.);
            }
            return theme.foreground.opacity(alpha);
        }
        let mut alpha: f32 = 1.;
        while alpha > 0. && edge(theme.border.opacity(alpha)) > TRACK_OUTLINE_MAX_CONTRAST {
            alpha = (alpha - 0.05).max(0.);
        }
        theme.border.opacity(alpha)
    }

    /// The chosen segment, filled and outlined. A disabled selection keeps
    /// the fill with a fainter outline.
    pub(super) fn segmented_pill<E: Styled>(pill: E, disabled: bool, cx: &App) -> E {
        let (fill, outline) = Self::segmented_selection(cx.theme());
        pill.bg(fill).border_1().border_color(if disabled {
            outline.opacity(0.5)
        } else {
            outline
        })
    }

    fn height(&self, size: Size) -> Pixels {
        match size {
            Size::XSmall => match self {
                TabVariant::Underline => px(26.),
                _ => px(20.),
            },
            Size::Small => match self {
                TabVariant::Underline => px(30.),
                _ => px(24.),
            },
            Size::Large => match self {
                TabVariant::Underline => px(44.),
                _ => px(36.),
            },
            _ => match self {
                TabVariant::Underline => px(36.),
                _ => px(32.),
            },
        }
    }

    pub(super) fn inner_height(&self, size: Size) -> Pixels {
        match size {
            Size::XSmall => match self {
                TabVariant::Tab | TabVariant::Outline | TabVariant::Pill => px(18.),
                TabVariant::Segmented => px(16.),
                TabVariant::Underline => px(20.),
            },
            Size::Small => match self {
                TabVariant::Tab | TabVariant::Outline | TabVariant::Pill => px(22.),
                TabVariant::Segmented => px(18.),
                TabVariant::Underline => px(22.),
            },
            Size::Large => match self {
                TabVariant::Tab | TabVariant::Outline | TabVariant::Pill => px(36.),
                TabVariant::Segmented => px(28.),
                TabVariant::Underline => px(32.),
            },
            _ => match self {
                TabVariant::Tab => px(30.),
                TabVariant::Outline | TabVariant::Pill => px(26.),
                TabVariant::Segmented => px(24.),
                TabVariant::Underline => px(26.),
            },
        }
    }

    /// Default px(12) to match a dock tab bar's px_3
    fn inner_paddings(&self, size: Size) -> Edges<Pixels> {
        let mut padding_x = match size {
            Size::XSmall => px(8.),
            Size::Small => px(10.),
            Size::Large => px(16.),
            _ => px(12.),
        };

        if matches!(self, TabVariant::Underline) {
            padding_x = px(0.);
        }

        Edges {
            left: padding_x,
            right: padding_x,
            ..Default::default()
        }
    }

    fn inner_margins(&self, size: Size) -> Edges<Pixels> {
        match size {
            Size::XSmall => match self {
                TabVariant::Underline => Edges {
                    top: px(1.),
                    bottom: px(2.),
                    ..Default::default()
                },
                _ => Edges::all(px(0.)),
            },
            Size::Small => match self {
                TabVariant::Underline => Edges {
                    top: px(2.),
                    bottom: px(3.),
                    ..Default::default()
                },
                _ => Edges::all(px(0.)),
            },
            Size::Large => match self {
                TabVariant::Underline => Edges {
                    top: px(5.),
                    bottom: px(6.),
                    ..Default::default()
                },
                _ => Edges::all(px(0.)),
            },
            _ => match self {
                TabVariant::Underline => Edges {
                    top: px(3.),
                    bottom: px(4.),
                    ..Default::default()
                },
                _ => Edges::all(px(0.)),
            },
        }
    }

    fn normal(&self, cx: &App) -> TabStyle {
        match self {
            TabVariant::Tab => TabStyle {
                fg: cx.theme().tab_foreground,
                bg: cx.theme().transparent.into(),
                borders: Edges {
                    left: px(1.),
                    right: px(1.),
                    ..Default::default()
                },
                border_color: cx.theme().transparent,
                ..Default::default()
            },
            TabVariant::Outline => TabStyle {
                fg: cx.theme().tab_foreground,
                bg: cx.theme().transparent.into(),
                borders: Edges::all(px(1.)),
                border_color: cx.theme().border,
                ..Default::default()
            },
            TabVariant::Pill => TabStyle {
                fg: cx.theme().foreground,
                bg: cx.theme().transparent.into(),
                ..Default::default()
            },
            TabVariant::Segmented => TabStyle {
                fg: cx.theme().muted_foreground,
                ..Default::default()
            },
            TabVariant::Underline => TabStyle {
                fg: cx.theme().tab_foreground,
                bg: cx.theme().transparent.into(),
                inner_bg: cx.theme().transparent.into(),
                borders: Edges {
                    bottom: px(2.),
                    ..Default::default()
                },
                border_color: cx.theme().transparent,
                ..Default::default()
            },
        }
    }

    fn hovered(&self, cx: &App) -> TabStyle {
        match self {
            TabVariant::Tab => TabStyle {
                fg: cx.theme().tab_active_foreground,
                bg: cx.theme().transparent.into(),
                borders: Edges {
                    left: px(1.),
                    right: px(1.),
                    ..Default::default()
                },
                border_color: cx.theme().transparent,
                ..Default::default()
            },
            TabVariant::Outline => TabStyle {
                fg: cx.theme().secondary_foreground,
                bg: cx.theme().tokens.secondary_hover.into(),
                borders: Edges::all(px(1.)),
                border_color: cx.theme().border,
                ..Default::default()
            },
            TabVariant::Pill => TabStyle {
                fg: cx.theme().secondary_foreground,
                bg: cx.theme().tokens.secondary.into(),
                ..Default::default()
            },
            TabVariant::Segmented => TabStyle {
                fg: cx.theme().foreground,
                ..Default::default()
            },
            TabVariant::Underline => TabStyle {
                fg: cx.theme().tab_active_foreground,
                bg: cx.theme().transparent.into(),
                inner_bg: cx.theme().transparent.into(),
                borders: Edges {
                    bottom: px(2.),
                    ..Default::default()
                },
                border_color: cx.theme().transparent,
                ..Default::default()
            },
        }
    }

    fn selected(&self, cx: &App) -> TabStyle {
        match self {
            TabVariant::Tab => TabStyle {
                fg: cx.theme().tab_active_foreground,
                bg: cx.theme().tokens.tab_active.into(),
                borders: Edges {
                    left: px(1.),
                    right: px(1.),
                    ..Default::default()
                },
                border_color: cx.theme().border,
                ..Default::default()
            },
            TabVariant::Outline => TabStyle {
                fg: cx.theme().primary,
                bg: cx.theme().transparent.into(),
                borders: Edges::all(px(1.)),
                border_color: cx.theme().primary,
                ..Default::default()
            },
            TabVariant::Pill => TabStyle {
                fg: cx.theme().primary_foreground,
                bg: cx.theme().tokens.primary.into(),
                ..Default::default()
            },
            TabVariant::Segmented => TabStyle {
                fg: cx.theme().foreground,
                ..Default::default()
            },
            TabVariant::Underline => TabStyle {
                fg: cx.theme().tab_active_foreground,
                bg: cx.theme().transparent.into(),
                borders: Edges {
                    bottom: px(2.),
                    ..Default::default()
                },
                border_color: cx.theme().primary,
                ..Default::default()
            },
        }
    }

    fn disabled(&self, selected: bool, cx: &App) -> TabStyle {
        match self {
            TabVariant::Tab => TabStyle {
                fg: cx.theme().muted_foreground,
                bg: cx.theme().transparent.into(),
                border_color: if selected {
                    cx.theme().border
                } else {
                    cx.theme().transparent
                },
                borders: Edges {
                    left: px(1.),
                    right: px(1.),
                    ..Default::default()
                },
                ..Default::default()
            },
            TabVariant::Outline => TabStyle {
                fg: cx.theme().muted_foreground,
                bg: cx.theme().transparent.into(),
                borders: Edges::all(px(1.)),
                border_color: if selected {
                    cx.theme().primary
                } else {
                    cx.theme().border
                },
                ..Default::default()
            },
            TabVariant::Pill => TabStyle {
                fg: if selected {
                    cx.theme().primary_foreground.opacity(0.5)
                } else {
                    cx.theme().muted_foreground
                },
                bg: if selected {
                    cx.theme().primary.opacity(0.5).into()
                } else {
                    cx.theme().transparent.into()
                },
                ..Default::default()
            },
            TabVariant::Segmented => TabStyle {
                fg: cx.theme().muted_foreground.opacity(0.65),
                ..Default::default()
            },
            TabVariant::Underline => TabStyle {
                fg: cx.theme().muted_foreground,
                bg: cx.theme().transparent.into(),
                border_color: if selected {
                    cx.theme().border
                } else {
                    cx.theme().transparent
                },
                borders: Edges {
                    bottom: px(2.),
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }

    pub(super) fn tab_bar_radius(&self, size: Size, cx: &App) -> Pixels {
        if *self != TabVariant::Segmented {
            return px(0.);
        }

        match size {
            Size::XSmall | Size::Small => cx.theme().radius,
            Size::Large => cx.theme().radius_lg,
            _ => cx.theme().radius_lg,
        }
    }

    fn radius(&self, size: Size, cx: &App) -> Pixels {
        match self {
            TabVariant::Outline | TabVariant::Pill => cx.theme().radius_full(),
            TabVariant::Segmented => match size {
                Size::XSmall | Size::Small => cx.theme().radius,
                Size::Large => cx.theme().radius_lg,
                _ => cx.theme().radius_lg,
            },
            _ => px(0.),
        }
    }

    /// The gap between a segmented bar's outer edge, where its outline is
    /// drawn, and the edge of each segment, the same on every side.
    pub(super) fn segment_inset(size: Size) -> Pixels {
        match size {
            Size::XSmall => px(2.),
            Size::Small => px(3.),
            _ => px(4.),
        }
    }

    pub(super) fn inner_radius(&self, size: Size, cx: &App) -> Pixels {
        match self {
            // The bar's radius less the inset, so the two curves stay
            // concentric.
            TabVariant::Segmented => {
                concentric_radius(self.tab_bar_radius(size, cx), Self::segment_inset(size))
            }
            _ => px(0.),
        }
    }
}

/// A segment's label, as wide as its medium weight. The selected segment's
/// label is medium weight, and a hidden medium copy holds every segment at
/// that width, so selection never moves the other segments.
fn segment_label(label: SharedString) -> Div {
    v_flex()
        .items_center()
        .child(label.clone())
        .child(div().h_0().overflow_hidden().font_medium().child(label))
}

#[allow(dead_code)]
struct TabStyle {
    borders: Edges<Pixels>,
    border_color: Hsla,
    bg: Background,
    fg: Hsla,
    shadow: bool,
    inner_bg: Background,
}

impl Default for TabStyle {
    fn default() -> Self {
        TabStyle {
            borders: Edges::all(px(0.)),
            border_color: gpui::transparent_white(),
            bg: gpui::transparent_white().into(),
            fg: gpui::transparent_white(),
            shadow: false,
            inner_bg: gpui::transparent_white().into(),
        }
    }
}

/// A Tab element for the [`super::TabBar`].
#[derive(IntoElement)]
pub struct Tab {
    ix: usize,
    base: gpui_base::Tab,
    pub(super) label: Option<SharedString>,
    aria_label: Option<SharedString>,
    pub(super) icon: Option<Icon>,
    prefix: Option<AnyElement>,
    pub(super) tab_bar_prefix: Option<bool>,
    suffix: Option<AnyElement>,
    children: Vec<AnyElement>,
    variant: TabVariant,
    size: Size,
    pub(super) disabled: bool,
    pub(super) selected: bool,
    pub(super) indicator_active: bool,
    pub(super) indicator_ready: bool,
    /// Animation epoch of the [`super::TabBar`] indicator; increments on every
    /// tab switch. Used to key the selected tab's text color fade so it
    /// restarts in sync with the indicator slide.
    pub(super) indicator_epoch: u64,
    pub(super) max_width: Option<Pixels>,
    /// The handle the [`super::TabBar`]'s roving focus gives this tab.
    pub(super) focus_handle: Option<FocusHandle>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl From<&'static str> for Tab {
    fn from(label: &'static str) -> Self {
        Self::new().label(label)
    }
}

impl From<String> for Tab {
    fn from(label: String) -> Self {
        Self::new().label(label)
    }
}

impl From<SharedString> for Tab {
    fn from(label: SharedString) -> Self {
        Self::new().label(label)
    }
}

impl From<Icon> for Tab {
    fn from(icon: Icon) -> Self {
        Self::default().icon(icon)
    }
}

impl From<IconName> for Tab {
    fn from(icon_name: IconName) -> Self {
        Self::default().icon(Icon::new(icon_name))
    }
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            ix: 0,
            base: gpui_base::Tab::new(0usize),
            label: None,
            aria_label: None,
            icon: None,
            tab_bar_prefix: None,
            children: Vec::new(),
            disabled: false,
            selected: false,
            indicator_active: false,
            indicator_ready: true,
            indicator_epoch: 0,
            prefix: None,
            suffix: None,
            variant: TabVariant::default(),
            size: Size::default(),
            max_width: None,
            focus_handle: None,
            on_click: None,
        }
    }
}

impl Tab {
    /// Create a new tab with a label.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set label for the tab.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the accessible label for the tab.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    fn a11y_label(&self) -> Option<SharedString> {
        self.aria_label.clone().or_else(|| self.label.clone())
    }

    /// Set icon for the tab.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set Tab Variant.
    pub fn with_variant(mut self, variant: TabVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Use Pill variant.
    pub fn pill(mut self) -> Self {
        self.variant = TabVariant::Pill;
        self
    }

    /// Use outline variant.
    pub fn outline(mut self) -> Self {
        self.variant = TabVariant::Outline;
        self
    }

    /// Use Segmented variant.
    pub fn segmented(mut self) -> Self {
        self.variant = TabVariant::Segmented;
        self
    }

    /// Use Underline variant.
    pub fn underline(mut self) -> Self {
        self.variant = TabVariant::Underline;
        self
    }

    /// Set the left side of the tab
    pub fn prefix(mut self, prefix: impl IntoElement) -> Self {
        self.prefix = Some(prefix.into_any_element());
        self
    }

    /// Set the right side of the tab
    pub fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }

    /// Set disabled state to the tab, default false.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the click handler for the tab.
    pub fn on_click(
        mut self,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(on_click));
        self
    }

    /// Set index to the tab.
    pub(crate) fn ix(mut self, ix: usize) -> Self {
        self.ix = ix;
        self.base = self.base.id(ix);
        self
    }

    /// Set if the tab bar has a prefix.
    pub(crate) fn tab_bar_prefix(mut self, tab_bar_prefix: bool) -> Self {
        self.tab_bar_prefix = Some(tab_bar_prefix);
        self
    }

    /// Set the maximum width of the tab, see [`super::TabBar::max_width`].
    pub(super) fn max_width(mut self, max_width: Option<Pixels>) -> Self {
        self.max_width = max_width;
        self
    }
}

impl ParentElement for Tab {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Selectable for Tab {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl InteractiveElement for Tab {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Tab {}

impl Styled for Tab {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl Sizable for Tab {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut normal_style = self.variant.normal(cx);
        let mut selected_style = self.variant.selected(cx);
        let mut disabled_style = self.variant.disabled(self.selected, cx);
        let mut hover_style = self.variant.hovered(cx);
        if self.disabled {
            hover_style = self.variant.disabled(self.selected, cx);
        }
        let tab_bar_prefix = self.tab_bar_prefix.unwrap_or_default();
        if !tab_bar_prefix {
            if self.ix == 0 && self.variant == TabVariant::Tab {
                normal_style.borders.left = px(0.);
                selected_style.borders.left = px(0.);
                disabled_style.borders.left = px(0.);
                hover_style.borders.left = px(0.);
            }
        }
        let tab_style = if self.disabled {
            &disabled_style
        } else if self.selected {
            &selected_style
        } else {
            &normal_style
        };
        let radius = self.variant.radius(self.size, cx);
        let inner_radius = self.variant.inner_radius(self.size, cx);
        let inner_paddings = self.variant.inner_paddings(self.size);
        let inner_margins = self.variant.inner_margins(self.size);
        let inner_height = self.variant.inner_height(self.size);
        let height = self.variant.height(self.size);
        let aria_label = self.a11y_label();
        // A press leaves focus where it is, so a ring only ever marks a tab
        // the keyboard moved to.
        let focus_visible = !self.disabled
            && window.last_input_was_keyboard()
            && self
                .focus_handle
                .as_ref()
                .is_some_and(|focus_handle| focus_handle.is_focused(window));

        // A selected segment draws its own pill until the bar's sliding
        // indicator is ready, and always outside a bar.
        let segmented_pill = self.variant == TabVariant::Segmented
            && self.selected
            && !(self.indicator_active && self.indicator_ready);

        // The sliding indicator alone represents selection once it is ready.
        // Disabled tabs keep their dimmed styling.
        let suppress_active_visual =
            self.selected && !self.disabled && self.indicator_active && self.indicator_ready;
        // Pill paints its active state via the outer `bg`.
        let selected_outer_bg = if suppress_active_visual && self.variant == TabVariant::Pill {
            cx.theme().transparent.into()
        } else {
            selected_style.bg
        };
        // Underline paints its active state via the bottom `border_color`.
        let selected_outer_border_color =
            if suppress_active_visual && self.variant == TabVariant::Underline {
                cx.theme().transparent
            } else {
                selected_style.border_color
            };

        // For Pill, the newly selected tab's text color (`primary_foreground`)
        // would otherwise snap to white instantly while the indicator is still
        // sliding into place. Fade it from the normal color in sync with the
        // indicator slide (keyed on the indicator epoch so it restarts on each
        // switch). `epoch == 0` is the initial layout (no slide), so we skip it.
        let animate_fg = self.selected
            && !self.disabled
            && self.variant == TabVariant::Pill
            && self.indicator_active
            && self.indicator_ready
            && self.indicator_epoch > 0;
        let fg_from = self.variant.normal(cx).fg;
        let fg_to = tab_style.fg;
        // Icon-only tabs are fixed-size and exempt from `max_width`.
        let max_width = self.max_width.filter(|_| self.icon.is_none());

        let inner_content = h_flex()
            .flex_1()
            .h(inner_height)
            .line_height(relative(1.25))
            .whitespace_nowrap()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .margins(inner_margins)
            // Normally the label decides the tab width, so it never shrinks. With
            // `max_width` it is the one part that gives way.
            .map(|this| match max_width {
                Some(_) => this.flex_auto(),
                None => this.flex_shrink_0(),
            })
            .map(|this| match self.icon {
                Some(icon) => this
                    .w(inner_height * 1.25)
                    .child(icon.map(|this| match self.size {
                        Size::XSmall => this.size_2p5(),
                        Size::Small => this.size_3p5(),
                        Size::Large => this.size_4(),
                        _ => this.size_4(),
                    })),
                None => this
                    .paddings(inner_paddings)
                    .map(|this| match (self.label, max_width) {
                        // Text always takes its natural width, so it needs a box
                        // that is allowed to shrink to ellipsize inside of.
                        (Some(label), Some(_)) => this.child(
                            div()
                                .min_w_0()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(label),
                        ),
                        (Some(label), None) if self.variant == TabVariant::Segmented => {
                            this.child(segment_label(label))
                        }
                        (Some(label), None) => this.child(label),
                        (None, _) => this,
                    })
                    .children(self.children),
            })
            .bg(tab_style.inner_bg)
            .rounded(inner_radius)
            // Every segment has room for the pill's outline, so all keep one size.
            .when(self.variant == TabVariant::Segmented, |this| {
                this.border_1().border_color(cx.theme().transparent)
            })
            .when(segmented_pill, |this| {
                TabVariant::segmented_pill(this, self.disabled, cx)
            })
            .when(tab_style.shadow, |this| this.shadow_xs())
            .hover(|this| {
                let this = this.rounded(inner_radius);
                if segmented_pill {
                    this
                } else {
                    this.bg(hover_style.inner_bg)
                }
            });

        let inner_element = if animate_fg {
            inner_content
                .with_animation(
                    ElementId::NamedInteger("tab-fg".into(), self.indicator_epoch),
                    Animation::new(Duration::from_millis(200)).with_easing(ease_in_out_cubic),
                    move |this, delta| this.text_color(Lerp::lerp(&fg_from, &fg_to, delta)),
                )
                .into_any_element()
        } else {
            inner_content.into_any_element()
        };

        self.base
            .id(self.ix)
            .selected(self.selected)
            .disabled(self.disabled)
            .when_some(self.focus_handle, |this, focus_handle| {
                this.track_focus(&focus_handle)
            })
            .when_some(aria_label, |this, label| this.accessibility_label(label))
            .styles(|styles| {
                styles
                    .selected(|style| {
                        style
                            .text_color(selected_style.fg)
                            .bg(selected_outer_bg)
                            .border_l(selected_style.borders.left)
                            .border_r(selected_style.borders.right)
                            .border_t(selected_style.borders.top)
                            .border_b(selected_style.borders.bottom)
                            .border_color(selected_outer_border_color)
                    })
                    .disabled(|style| {
                        style
                            .text_color(disabled_style.fg)
                            .bg(disabled_style.bg)
                            .border_l(disabled_style.borders.left)
                            .border_r(disabled_style.borders.right)
                            .border_t(disabled_style.borders.top)
                            .border_b(disabled_style.borders.bottom)
                            .border_color(disabled_style.border_color)
                    })
            })
            .relative()
            .flex()
            // Wrapping would move the overflow onto a clipped second line instead
            // of letting the label shrink, so a capped tab lays out on one line.
            .map(|this| match max_width {
                Some(max_width) => this.flex_nowrap().max_w(max_width),
                None => this.flex_wrap(),
            })
            .gap_1()
            .items_center()
            .flex_shrink_0()
            .h(height)
            .overflow_hidden()
            .map(|this| match self.size {
                Size::XSmall => this.text_xs(),
                Size::Large => this.text_base(),
                _ => this.text_sm(),
            })
            .when(
                self.variant == TabVariant::Segmented && self.selected,
                |this| this.font_medium(),
            )
            .rounded(radius)
            .when(!self.selected && !self.disabled, |this| {
                this.text_color(normal_style.fg)
                    .bg(normal_style.bg)
                    .border_l(normal_style.borders.left)
                    .border_r(normal_style.borders.right)
                    .border_t(normal_style.borders.top)
                    .border_b(normal_style.borders.bottom)
                    .border_color(normal_style.border_color)
            })
            .hover(|this| {
                // Always register the hover style: GPUI only refreshes the cached
                // hover state while one is present. If the selected tab skipped it,
                // the stale state would keep hover colors after unselecting.
                if self.selected || self.disabled {
                    return this;
                }
                this.text_color(hover_style.fg)
                    .bg(hover_style.bg)
                    .border_l(hover_style.borders.left)
                    .border_r(hover_style.borders.right)
                    .border_t(hover_style.borders.top)
                    .border_b(hover_style.borders.bottom)
                    .border_color(hover_style.border_color)
                    .rounded(radius)
            })
            // Under `max_width` the label is the only part that gives way, so
            // hold the prefix and suffix (e.g. a close button) at their full size.
            .when_some(self.prefix, |this, prefix| {
                this.child(
                    div()
                        .when_some(max_width, |this, _| this.flex_shrink_0())
                        .child(prefix),
                )
            })
            .child(inner_element)
            .when_some(self.suffix, |this, suffix| {
                this.child(
                    div()
                        .when_some(max_width, |this, _| this.flex_shrink_0())
                        .child(suffix),
                )
            })
            // Inside the tab, because the tab and its bar clip what's outside.
            .when(focus_visible, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(radius)
                        .border_2()
                        .border_color(cx.theme().ring),
                )
            })
            .when_some(self.on_click.clone(), |this, on_click| {
                this.on_click(move |event, window, cx| on_click(event, window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab::TabBar;
    use gpui::{Context, Render, TestAppContext, VisualTestContext};

    const VARIANTS: [TabVariant; 5] = [
        TabVariant::Tab,
        TabVariant::Outline,
        TabVariant::Pill,
        TabVariant::Segmented,
        TabVariant::Underline,
    ];

    const LONG_LABEL: &str = "Account Settings & Preferences";

    /// One [`TabBar`], optionally capped, holding the tab `build` returns.
    struct TabBarTest {
        variant: TabVariant,
        max_width: Option<Pixels>,
        build: fn(Tab) -> Tab,
    }

    impl Render for TabBarTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            TabBar::new("tabs")
                .with_variant(self.variant)
                .selected_index(0)
                .when_some(self.max_width, |this, width| this.max_width(width))
                .child((self.build)(
                    Tab::new().debug_selector(|| "tab".to_string()),
                ))
        }
    }

    fn show(
        cx: &mut TestAppContext,
        variant: TabVariant,
        max_width: Option<Pixels>,
        build: fn(Tab) -> Tab,
    ) -> &mut VisualTestContext {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| TabBarTest {
            variant,
            max_width,
            build,
        });
        cx.run_until_parked();
        cx
    }

    /// Outer width of a labelled tab in every variant, capped or not.
    fn variant_widths(
        cx: &mut TestAppContext,
        build: fn(Tab) -> Tab,
        max_width: Option<Pixels>,
    ) -> Vec<Pixels> {
        VARIANTS
            .into_iter()
            .map(|variant| {
                show(cx, variant, max_width, build)
                    .debug_bounds("tab")
                    .expect("tab not rendered")
                    .size
                    .width
            })
            .collect()
    }

    /// In every bundled theme and both defaults, the track's outline shows
    /// without framing the control heavily, the chosen segment's fill and
    /// outline stand out, and both labels are legible.
    #[test]
    fn segmented_tabs_keep_their_contrast_in_every_bundled_theme() {
        use crate::ThemeSet;

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        paths.push(concat!(env!("CARGO_MANIFEST_DIR"), "/src/theme/default-theme.json").into());
        let mut variants = 0;
        let mut failures = Vec::new();
        for path in paths {
            let set: ThemeSet =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for config in set.themes {
                let name = config.name.clone();
                let mut theme = Theme::default();
                theme.apply_config(&Rc::new(config));
                let track = TabVariant::segmented_track(&theme);
                let (fill, outline) = TabVariant::segmented_selection(&theme);
                let edge = track.blend(TabVariant::segmented_track_outline(&theme));
                let line = fill.blend(outline);
                let checks = [
                    (
                        "track outline",
                        contrast_ratio(edge, theme.background),
                        TRACK_OUTLINE_MIN_CONTRAST,
                        TRACK_OUTLINE_MAX_CONTRAST,
                    ),
                    (
                        "fill",
                        contrast_ratio(fill, track),
                        SEGMENT_FILL_MIN_CONTRAST,
                        21.,
                    ),
                    (
                        "outline on fill",
                        contrast_ratio(line, fill),
                        SEGMENT_OUTLINE_MIN_CONTRAST,
                        21.,
                    ),
                    (
                        "outline on track",
                        contrast_ratio(line, track),
                        SEGMENT_OUTLINE_MIN_CONTRAST,
                        21.,
                    ),
                    (
                        "chosen label",
                        contrast_ratio(fill.blend(theme.foreground), fill),
                        4.5,
                        21.,
                    ),
                    (
                        "other labels",
                        contrast_ratio(track.blend(theme.muted_foreground), track),
                        4.5,
                        21.,
                    ),
                ];
                for (what, ratio, least, most) in checks {
                    if !(least..=most).contains(&ratio) {
                        failures.push(format!("{name}: {what} {ratio:.2}"));
                    }
                }
                variants += 1;
            }
        }
        assert!(variants >= 73, "{variants} variants read");
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[gpui::test]
    fn a11y_label_defaults_to_visible_label(_cx: &mut TestAppContext) {
        let tab = Tab::new().label("Account");

        assert_eq!(tab.a11y_label(), Some("Account".into()));
    }

    #[gpui::test]
    fn explicit_a11y_label_overrides_visible_label(_cx: &mut TestAppContext) {
        let tab = Tab::new().label("Acct").aria_label("Account settings");

        assert_eq!(tab.a11y_label(), Some("Account settings".into()));
    }

    #[gpui::test]
    fn max_width_leaves_short_tabs_untouched(cx: &mut TestAppContext) {
        // The box the cap wraps the label in must not report a different
        // intrinsic width than the bare label it replaces.
        let build: fn(Tab) -> Tab = |tab| tab.label("Go");

        let uncapped = variant_widths(cx, build, None);
        let capped = variant_widths(cx, build, Some(px(200.)));

        assert_eq!(uncapped, capped, "a tab under the cap must not be resized");
    }

    #[gpui::test]
    fn max_width_caps_long_tabs(cx: &mut TestAppContext) {
        let build: fn(Tab) -> Tab = |tab| tab.label(LONG_LABEL);

        let uncapped = variant_widths(cx, build, None);
        let capped = variant_widths(cx, build, Some(px(120.)));

        for (variant, (uncapped, capped)) in
            VARIANTS.into_iter().zip(uncapped.into_iter().zip(capped))
        {
            assert!(
                uncapped > px(120.),
                "{variant:?} is not long enough to exercise the cap ({uncapped:?})"
            );
            assert!(
                capped <= px(120.),
                "{variant:?} width {capped:?} exceeds max_width"
            );
        }
    }

    #[gpui::test]
    fn max_width_keeps_prefix_and_suffix_intact(cx: &mut TestAppContext) {
        let cx = show(cx, TabVariant::Segmented, Some(px(140.)), |tab| {
            tab.prefix(Icon::new(IconName::BookOpen))
                .label(LONG_LABEL)
                .suffix(div().size(px(16.)).debug_selector(|| "suffix".to_string()))
        });

        let tab = cx.debug_bounds("tab").expect("tab not rendered");
        let suffix = cx.debug_bounds("suffix").expect("suffix not rendered");

        assert!(tab.size.width <= px(140.));
        assert_eq!(
            suffix.size.width,
            px(16.),
            "the label should absorb the truncation, not the suffix"
        );
        assert!(
            suffix.right() <= tab.right(),
            "suffix must stay within the tab"
        );
        // A wrapping tab pushes the suffix onto a second line, which the fixed
        // tab height then clips: it keeps its size and stays inside the tab,
        // but lands back at the left edge instead of after the label.
        assert!(
            suffix.left() > tab.center().x,
            "suffix must follow the label, not wrap below it"
        );
    }

    /// Icon-only tabs are sized to a square by construction, so the cap has to
    /// leave them alone however narrow it is.
    #[gpui::test]
    fn max_width_exempts_icon_only_tabs(cx: &mut TestAppContext) {
        let build: fn(Tab) -> Tab = |tab| tab.icon(Icon::new(IconName::BookOpen));
        let width = |cx: &mut TestAppContext, max_width| {
            show(cx, TabVariant::Tab, max_width, build)
                .debug_bounds("tab")
                .expect("tab not rendered")
                .size
                .width
        };

        let uncapped = width(cx, None);
        assert!(
            uncapped > px(16.),
            "the cap has to be narrower than the icon tab to be meaningful"
        );
        assert_eq!(
            width(cx, Some(px(16.))),
            uncapped,
            "an icon-only tab must ignore max_width"
        );
    }
}
