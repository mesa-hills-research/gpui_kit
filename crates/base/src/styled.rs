use gpui::{
    App, BoxShadow, Corners, DefiniteLength, Div, Edges, FocusHandle, Hsla, Pixels,
    Refineable as _, Role, StyleRefinement, Styled, Window, div, hsla, point, px,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RoleOverride {
    #[default]
    Implicit,
    Presentational,
    Role(Role),
}

impl RoleOverride {
    pub fn resolve(self, default: impl FnOnce() -> Role) -> Option<Role> {
        match self {
            Self::Implicit => Some(default()),
            Self::Presentational => None,
            Self::Role(role) => Some(role),
        }
    }
}
impl From<Role> for RoleOverride {
    fn from(role: Role) -> Self {
        Self::Role(role)
    }
}
impl From<Option<Role>> for RoleOverride {
    fn from(role: Option<Role>) -> Self {
        role.map_or(Self::Presentational, Self::Role)
    }
}

/// A row that centers its children on the cross axis.
///
/// See [`StyledExt::h_flex`] for the cross-axis rule, which is not symmetric
/// with [`v_flex`].
pub fn h_flex() -> Div {
    div().h_flex()
}

/// A column whose children stretch across the cross axis.
///
/// See [`StyledExt::v_flex`] for the cross-axis rule, which is not symmetric
/// with [`h_flex`].
pub fn v_flex() -> Div {
    div().v_flex()
}

pub fn box_shadow(
    x: impl Into<Pixels>,
    y: impl Into<Pixels>,
    blur: impl Into<Pixels>,
    spread: impl Into<Pixels>,
    color: Hsla,
) -> BoxShadow {
    BoxShadow {
        offset: point(x.into(), y.into()),
        blur_radius: blur.into(),
        spread_radius: spread.into(),
        inset: false,
        color,
    }
}

/// The corner radius that keeps a shape concentric inside one of radius
/// `outer`, when its edge is `inset` from the outer shape's edge.
///
/// `inset` is the padding plus any border drawn inside the outer box. A ring
/// drawn outside the box, such as a popover's edge, does not count. A shape
/// deeper than the corner arc reads fine with any radius, and the result is
/// zero once `inset` reaches `outer`.
pub fn concentric_radius(outer: Pixels, inset: Pixels) -> Pixels {
    (outer - inset).max(Pixels::ZERO)
}

/// The smallest corner radius a nested row or tile gets, capped at half its
/// frame's radius. See [`nested_radius`].
const NESTED_RADIUS_FLOOR: Pixels = px(3.);

/// The corner radius for a row or tile nested `inset` inside a frame of radius
/// `outer`, which never reads as square.
///
/// This is [`concentric_radius`] with a minimum. Where the inset is large next
/// to the frame's radius, the exact rule leaves a row with a 1 or 2 px corner
/// that looks sharp, so the result is at least 3 px, capped at half of `outer`:
///
/// ```text
/// max(outer - inset, min(3px, outer / 2))
/// ```
///
/// A 6 or 8 px frame gives its rows at least 3 px, a 4 px frame gives them
/// 2 px, and a frame with no radius stays square. Use it for rows, tiles and
/// buttons inside a frame. Use [`concentric_radius`] where the shape must meet
/// its frame exactly, such as a table header fill, a focus line, a segmented
/// tab or a switch thumb.
pub fn nested_radius(outer: Pixels, inset: Pixels) -> Pixels {
    concentric_radius(outer, inset).max(NESTED_RADIUS_FLOOR.min(outer / 2.))
}

/// The radius of a ring or frame drawn `inset` outside a shape of radius
/// `inner`. The inverse of [`concentric_radius`].
pub fn enclosing_radius(inner: Pixels, inset: Pixels) -> Pixels {
    inner + inset
}

macro_rules! font_weight {
    ($method:ident, $weight:ident) => {
        fn $method(self) -> Self {
            self.font_weight(gpui::FontWeight::$weight)
        }
    };
}

#[cfg_attr(
    any(feature = "inspector", debug_assertions),
    gpui_macros::derive_inspector_reflection
)]
pub trait StyledExt: Styled + Sized {
    fn refine_style(mut self, style: &StyleRefinement) -> Self {
        self.style().refine(style);
        self
    }

    /// Lays children out in a row, centered on the cross axis.
    ///
    /// The centering is the desktop default for a row of controls — an icon
    /// beside its label lines up without either side asking for it — but it is
    /// **not** the mirror image of [`Self::v_flex`], which leaves the cross axis
    /// stretching. A column placed in a row therefore does not take the row's
    /// height: it takes its content's height and is centered inside the row.
    /// When its content is taller than the row, it overflows equally above and
    /// below, so the column's header is pushed off the top edge and clipped.
    ///
    /// Give a full-height column `h_full()` (or the row `items_start()` /
    /// `items_stretch()`) whenever the child owns a header, a footer, or a
    /// scroll region that has to resolve against the row's height.
    ///
    /// ```
    /// use gpui_base::StyledExt as _;
    /// use gpui::{ParentElement as _, Styled as _, div};
    ///
    /// // A sidebar beside a detail pane, both spanning the full height.
    /// div().h_flex().size_full().child(div().w_64().h_full());
    /// ```
    fn h_flex(self) -> Self {
        self.flex().flex_row().items_center()
    }

    /// Lays children out in a column, stretching them across the cross axis.
    ///
    /// Unlike [`Self::h_flex`] this installs no cross-axis alignment, so a child
    /// without a width fills the column. See `h_flex` for the asymmetry.
    fn v_flex(self) -> Self {
        self.flex().flex_col()
    }

    fn paddings<L>(self, paddings: impl Into<Edges<L>>) -> Self
    where
        L: Into<DefiniteLength> + Clone + Default + std::fmt::Debug + PartialEq,
    {
        let paddings = paddings.into();
        self.pt(paddings.top.into())
            .pb(paddings.bottom.into())
            .pl(paddings.left.into())
            .pr(paddings.right.into())
    }

    fn margins<L>(self, margins: impl Into<Edges<L>>) -> Self
    where
        L: Into<DefiniteLength> + Clone + Default + std::fmt::Debug + PartialEq,
    {
        let margins = margins.into();
        self.mt(margins.top.into())
            .mb(margins.bottom.into())
            .ml(margins.left.into())
            .mr(margins.right.into())
    }

    fn debug_red(self) -> Self {
        if cfg!(debug_assertions) {
            self.border_1().border_color(hsl(0., 72.2, 50.6))
        } else {
            self
        }
    }

    fn debug_blue(self) -> Self {
        if cfg!(debug_assertions) {
            self.border_1().border_color(hsl(217.2, 91.2, 59.8))
        } else {
            self
        }
    }

    fn debug_yellow(self) -> Self {
        if cfg!(debug_assertions) {
            self.border_1().border_color(hsl(47.9, 95.8, 53.1))
        } else {
            self
        }
    }

    fn debug_green(self) -> Self {
        if cfg!(debug_assertions) {
            self.border_1().border_color(hsl(142.1, 70.6, 45.3))
        } else {
            self
        }
    }

    fn debug_pink(self) -> Self {
        if cfg!(debug_assertions) {
            self.border_1().border_color(hsl(330.4, 81.2, 60.4))
        } else {
            self
        }
    }

    fn debug_focused(self, focus_handle: &FocusHandle, window: &Window, cx: &App) -> Self {
        if cfg!(debug_assertions) && focus_handle.contains_focused(window, cx) {
            self.debug_blue()
        } else {
            self
        }
    }

    font_weight!(font_thin, THIN);
    font_weight!(font_extralight, EXTRA_LIGHT);
    font_weight!(font_light, LIGHT);
    font_weight!(font_normal, NORMAL);
    font_weight!(font_medium, MEDIUM);
    font_weight!(font_semibold, SEMIBOLD);
    font_weight!(font_bold, BOLD);
    font_weight!(font_extrabold, EXTRA_BOLD);
    font_weight!(font_black, BLACK);

    fn corner_radii(self, radius: Corners<Pixels>) -> Self {
        self.rounded_tl(radius.top_left)
            .rounded_tr(radius.top_right)
            .rounded_bl(radius.bottom_left)
            .rounded_br(radius.bottom_right)
    }
}

impl<E: Styled> StyledExt for E {}

#[cfg(any(feature = "inspector", debug_assertions))]
pub fn styled_ext_reflection_methods<T: Styled + 'static>()
-> Vec<gpui::inspector_reflection::FunctionReflection<T>> {
    styled_ext_reflection::methods::<T>()
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Hsla {
    hsla(hue / 360., saturation / 100., lightness / 100., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, TestAppContext,
        px,
    };

    fn column(selector: &'static str, height: f32) -> Div {
        div()
            .w(px(20.))
            .h(px(height))
            .flex_shrink_0()
            .debug_selector(move || selector.to_string())
    }

    struct CrossAxisTest;

    impl Render for CrossAxisTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(200.))
                .h(px(100.))
                .child(
                    h_flex()
                        .size_full()
                        .child(column("row-child", 40.))
                        .child(column("row-overflowing-child", 140.))
                        .child(
                            div()
                                .w(px(20.))
                                .debug_selector(|| "row-stretch".to_string()),
                        ),
                )
                .child(
                    v_flex().size_full().child(
                        div()
                            .h(px(20.))
                            .debug_selector(|| "col-stretch".to_string()),
                    ),
                )
        }
    }

    /// `h_flex` centers on the cross axis while `v_flex` leaves the default
    /// stretch. The asymmetry is deliberate but easy to trip over, so lock it.
    #[gpui::test]
    fn h_flex_centers_and_v_flex_stretches_on_the_cross_axis(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| CrossAxisTest);
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });

        // A shorter child of a 100px row sits at (100 - 40) / 2, not at the top,
        // and a height-less child keeps its content height instead of filling.
        let child = cx.debug_bounds("row-child").unwrap();
        assert_eq!(child.top(), px(30.));
        assert_eq!(cx.debug_bounds("row-stretch").unwrap().size.height, px(0.));

        // And a child taller than the row is centered too, so its top — a
        // column's header, in a real layout — is pushed off the top edge.
        let overflowing = cx.debug_bounds("row-overflowing-child").unwrap();
        assert_eq!(overflowing.top(), px(-20.));

        // A width-less child of a column does fill the cross axis.
        assert_eq!(cx.debug_bounds("col-stretch").unwrap().size.width, px(200.));
    }

    #[test]
    fn concentric_radius_follows_the_inset_and_stops_at_zero() {
        assert_eq!(concentric_radius(px(6.), px(4.)), px(2.));
        assert_eq!(concentric_radius(px(8.), px(5.)), px(3.));
        assert_eq!(concentric_radius(px(6.), px(0.)), px(6.));
        assert_eq!(concentric_radius(px(6.), px(6.)), px(0.));
        assert_eq!(concentric_radius(px(6.), px(9.)), px(0.));
        // A frame drawn outside grows the radius instead.
        assert_eq!(concentric_radius(px(6.), px(-3.)), px(9.));
    }

    #[test]
    fn nested_radius_never_drops_below_the_minimum() {
        // The default popup frames. Rows 4 px in, tiles 7 px in and rows 5 px
        // in all get 3 px, where the exact rule gives 2, 1 and 3.
        assert_eq!(nested_radius(px(6.), px(4.)), px(3.));
        assert_eq!(nested_radius(px(8.), px(7.)), px(3.));
        assert_eq!(nested_radius(px(8.), px(5.)), px(3.));
        // An inset at or past the frame's radius still rounds.
        assert_eq!(nested_radius(px(8.), px(8.)), px(3.));
        assert_eq!(nested_radius(px(8.), px(12.)), px(3.));
    }

    #[test]
    fn nested_radius_keeps_the_exact_radius_above_the_minimum() {
        assert_eq!(nested_radius(px(8.), px(4.)), px(4.));
        assert_eq!(nested_radius(px(8.), px(0.)), px(8.));
        assert_eq!(nested_radius(px(12.), px(4.)), px(8.));
        // The cap never lowers a radius that is already above it.
        assert_eq!(nested_radius(px(4.), px(1.)), px(3.));
        // A frame drawn outside grows the radius, as it does for the exact rule.
        assert_eq!(nested_radius(px(6.), px(-3.)), px(9.));
    }

    #[test]
    fn nested_radius_caps_the_minimum_at_half_the_frame() {
        assert_eq!(nested_radius(px(4.), px(4.)), px(2.));
        assert_eq!(nested_radius(px(2.), px(2.)), px(1.));
        // A square theme stays square.
        assert_eq!(nested_radius(px(0.), px(0.)), px(0.));
        assert_eq!(nested_radius(px(0.), px(4.)), px(0.));
    }

    #[test]
    fn enclosing_radius_undoes_concentric_radius() {
        assert_eq!(enclosing_radius(px(6.), px(3.)), px(9.));
        assert_eq!(
            concentric_radius(enclosing_radius(px(6.), px(3.)), px(3.)),
            px(6.)
        );
    }
}
