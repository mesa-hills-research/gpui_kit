use std::{cell::Cell, ops::Deref, panic::Location, rc::Rc};

use instant::Duration;
use web_time::Instant;

use crate::{
    AxisExt,
    animation::{ease_in_cubic, ease_out_cubic},
    theme::ActiveTheme as _,
};
use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, AvailableSpace, Axis, Background,
    BorderStyle, Bounds, ContentMask, CursorStyle, Edges, Element, ElementId, EntityId,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, IsZero,
    LayoutId, ListState, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintEffect, PaintQuad,
    Pixels, Point, Position, ScrollHandle, ScrollWheelEvent, Size, Style, TouchDragEvent,
    TouchPhase, UniformListScrollHandle, Window, fill, point, prelude::FluentBuilder, px, relative,
    size,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The width of the scrollbar (THUMB_ACTIVE_INSET * 2 + THUMB_ACTIVE_WIDTH)
const WIDTH: Pixels = px(4. * 2. + 8.);
const MIN_THUMB_SIZE: Pixels = px(48.);

const THUMB_WIDTH: Pixels = px(6.);
const THUMB_RADIUS: Pixels = Pixels::ZERO;
const THUMB_INSET: Pixels = px(4.);

const THUMB_ACTIVE_WIDTH: Pixels = px(8.);
const THUMB_ACTIVE_RADIUS: Pixels = Pixels::ZERO;
const THUMB_ACTIVE_INSET: Pixels = px(4.);

/// How long visibility is held after the last activity, when the styled layer
/// projects no [`ScrollbarMotion`] of its own.
///
/// This is a visibility hold rather than motion: without it [`ScrollbarMode::Scrolling`]
/// could never reveal the scrollbar.
const DEFAULT_IDLE: Duration = Duration::from_secs(2);

fn clamp_thumb_radius(radius: Pixels, bounds: Bounds<Pixels>) -> Pixels {
    radius
        .min(bounds.size.width / 2.)
        .min(bounds.size.height / 2.)
}

/// Scrollbar show mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Default, JsonSchema)]
pub enum ScrollbarMode {
    /// Show scrollbar when scrolling, will fade out after idle.
    #[default]
    Scrolling,
    /// Show scrollbar on hover.
    Hover,
    /// Always show scrollbar.
    Always,
}

impl ScrollbarMode {
    fn is_hover(&self) -> bool {
        matches!(self, Self::Hover)
    }

    fn is_always(&self) -> bool {
        matches!(self, Self::Always)
    }
}

/// A trait for scroll handles that can get and set offset.
pub trait ScrollbarHandle: 'static {
    /// Bounds of the viewport the scrollbar overlays.
    fn viewport_bounds(&self) -> Bounds<Pixels>;
    /// Get the current offset of the scroll handle.
    fn offset(&self) -> Point<Pixels>;
    /// Set the offset of the scroll handle.
    fn set_offset(&self, offset: Point<Pixels>);
    /// The full size of the content, including padding.
    fn content_size(&self) -> Size<Pixels>;
    /// Called when start dragging the scrollbar thumb.
    fn start_drag(&self) {}
    /// Called when end dragging the scrollbar thumb.
    fn end_drag(&self) {}
}

impl ScrollbarHandle for ScrollHandle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.bounds()
    }

    fn offset(&self) -> Point<Pixels> {
        self.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        (self.max_offset() + self.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for UniformListScrollHandle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().base_handle.bounds()
    }

    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().base_handle.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.0.borrow_mut().base_handle.set_offset(offset)
    }

    fn content_size(&self) -> Size<Pixels> {
        let base_handle = &self.0.borrow().base_handle;
        (base_handle.max_offset() + base_handle.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for ListState {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        ListState::viewport_bounds(self)
    }

    fn offset(&self) -> Point<Pixels> {
        self.scroll_px_offset_for_scrollbar()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset_from_scrollbar(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        self.viewport_bounds().size + self.max_offset_for_scrollbar().into()
    }

    fn start_drag(&self) {
        self.scrollbar_drag_started();
    }

    fn end_drag(&self) {
        self.scrollbar_drag_ended();
    }
}

#[doc(hidden)]
#[derive(Debug, Clone)]
struct ScrollbarState(Rc<Cell<ScrollbarStateInner>>);

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
struct ScrollbarStateInner {
    hovered_axis: Option<Axis>,
    hovered_on_thumb: Option<Axis>,
    dragged_axis: Option<Axis>,
    drag_pos: Point<Pixels>,
    last_scroll_offset: Point<Pixels>,
    last_scroll_time: Option<Instant>,
    // Last update offset
    last_update: Instant,
    drag_update_scheduled: bool,
    drag_update_generation: u64,
    idle_timer_scheduled: bool,
    visibility: VisibilityAnimation,
    vertical_width: WidthAnimation,
    horizontal_width: WidthAnimation,
    /// The effect the last frame painted the scrollbar with. The fade and the
    /// slide run in the paint phase, so events read the opacity from this rather
    /// than from the sample taken when the scrollbar was last laid out.
    painted: PaintEffect,
}

impl ScrollbarState {
    /// A hidden scrollbar's state, made at `now` by the window's clock.
    fn new(now: Instant) -> Self {
        Self(Rc::new(Cell::new(ScrollbarStateInner {
            hovered_axis: None,
            hovered_on_thumb: None,
            dragged_axis: None,
            drag_pos: point(px(0.), px(0.)),
            last_scroll_offset: point(px(0.), px(0.)),
            last_scroll_time: None,
            last_update: now,
            drag_update_scheduled: false,
            drag_update_generation: 0,
            idle_timer_scheduled: false,
            visibility: VisibilityAnimation::hidden(now),
            vertical_width: WidthAnimation::new(now),
            horizontal_width: WidthAnimation::new(now),
            painted: PaintEffect::opacity(0.0),
        })))
    }
}

#[derive(Debug, Clone, Copy)]
struct ScalarTransition<T> {
    from: T,
    target: T,
    started_at: Instant,
    duration: Duration,
}

impl<T: Copy + PartialEq> ScalarTransition<T> {
    fn settled(value: T, now: Instant) -> Self {
        Self {
            from: value,
            target: value,
            started_at: now,
            duration: Duration::ZERO,
        }
    }

    fn sample(&self, now: Instant, interpolate: impl FnOnce(T, T, f32) -> T) -> (T, bool) {
        if self.from == self.target || self.duration.is_zero() {
            return (self.target, false);
        }
        let linear = now.saturating_duration_since(self.started_at).as_secs_f32()
            / self.duration.as_secs_f32();
        if linear >= 1.0 {
            (self.target, false)
        } else {
            (
                interpolate(self.from, self.target, linear.clamp(0.0, 1.0)),
                true,
            )
        }
    }

    fn start(&mut self, from: T, target: T, duration: Duration, now: Instant) {
        self.from = from;
        self.target = target;
        self.started_at = now;
        self.duration = duration;
    }

    fn settle(&mut self, target: T, now: Instant) {
        self.start(target, target, Duration::ZERO, now);
    }
}

#[derive(Debug, Clone, Copy)]
struct WidthAnimation {
    transition: ScalarTransition<Pixels>,
    initialized: bool,
}

impl WidthAnimation {
    fn new(now: Instant) -> Self {
        Self {
            transition: ScalarTransition::settled(Pixels::ZERO, now),
            initialized: false,
        }
    }

    fn sample(&self, now: Instant) -> (Pixels, bool) {
        self.transition.sample(now, |from, target, linear| {
            from + (target - from) * ease_out_cubic(linear)
        })
    }

    /// Move toward `target` over `duration`. A zero duration adopts the target
    /// immediately, which is how reduced motion and a motionless theme arrive here.
    fn set_target(&mut self, target: Pixels, duration: Duration, now: Instant) -> (Pixels, bool) {
        if duration.is_zero() || !self.initialized {
            self.transition.settle(target, now);
            self.initialized = true;
        } else if self.transition.target != target {
            let from = self.sample(now).0;
            self.transition.start(from, target, duration, now);
        }
        self.sample(now)
    }
}

/// How a scrollbar becomes visible.
///
/// The styled layer chooses the choreography; Base only plays it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbarEntrance {
    /// Fade in without moving.
    #[default]
    Fade,
    /// Slide in from the nearest edge while fading.
    SlideAndFade,
}

/// Motion tokens used by [`Scrollbar`].
///
/// Base installs no motion of its own: every transition duration defaults to
/// zero, so visibility and thumb width snap. Product timing belongs to the
/// styled layer, which projects it through [`crate::ScrollbarTheme`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarMotion {
    idle: Duration,
    enter: Duration,
    exit: Duration,
    expand: Duration,
    entrance: ScrollbarEntrance,
    thumb_hover_entrance: ScrollbarEntrance,
}

impl Default for ScrollbarMotion {
    fn default() -> Self {
        Self {
            idle: DEFAULT_IDLE,
            enter: Duration::ZERO,
            exit: Duration::ZERO,
            expand: Duration::ZERO,
            entrance: ScrollbarEntrance::Fade,
            thumb_hover_entrance: ScrollbarEntrance::Fade,
        }
    }
}

impl ScrollbarMotion {
    /// How long visibility is held after the last scroll, drag, or hover.
    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// How long the scrollbar takes to become fully visible.
    pub fn with_enter(mut self, enter: Duration) -> Self {
        self.enter = enter;
        self
    }

    /// How long the scrollbar takes to fade away once the idle hold expires.
    pub fn with_exit(mut self, exit: Duration) -> Self {
        self.exit = exit;
        self
    }

    /// How long the thumb takes to reach a new width.
    pub fn with_expand(mut self, expand: Duration) -> Self {
        self.expand = expand;
        self
    }

    /// Which entrance choreography to play.
    pub fn with_entrance(mut self, entrance: ScrollbarEntrance) -> Self {
        self.entrance = entrance;
        self
    }

    /// Which entrance choreography to play when hover reveals the thumb.
    pub fn with_thumb_hover_entrance(mut self, entrance: ScrollbarEntrance) -> Self {
        self.thumb_hover_entrance = entrance;
        self
    }

    pub fn idle(&self) -> Duration {
        self.idle
    }

    pub fn enter(&self) -> Duration {
        self.enter
    }

    pub fn exit(&self) -> Duration {
        self.exit
    }

    pub fn expand(&self) -> Duration {
        self.expand
    }

    pub fn entrance(&self) -> ScrollbarEntrance {
        self.entrance
    }

    pub fn thumb_hover_entrance(&self) -> ScrollbarEntrance {
        self.thumb_hover_entrance
    }

    fn entrance_for(&self, mode: ScrollbarMode, thumb_hovered: bool) -> ScrollbarEntrance {
        if mode.is_hover() && thumb_hovered {
            self.thumb_hover_entrance
        } else {
            self.entrance
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct VisibilityAnimation {
    opacity: ScalarTransition<f32>,
    position: ScalarTransition<f32>,
    entrance: ScrollbarEntrance,
    /// Counts the transitions started. The paint-driven fade is keyed on it, so
    /// each new transition starts a new run of frames.
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct VisibilitySample {
    opacity: f32,
    position: f32,
    running: bool,
}

impl VisibilityAnimation {
    fn hidden(now: Instant) -> Self {
        Self {
            opacity: ScalarTransition::settled(0.0, now),
            position: ScalarTransition::settled(0.0, now),
            entrance: ScrollbarEntrance::Fade,
            generation: 0,
        }
    }

    fn sample(&self, now: Instant) -> VisibilitySample {
        let entering =
            self.opacity.target > self.opacity.from || self.position.target > self.position.from;
        let (opacity, opacity_running) = self.opacity.sample(now, |from, target, linear| {
            let factor = if entering {
                linear
            } else {
                ease_in_cubic(linear)
            };
            from + (target - from) * factor
        });
        let (position, position_running) = self.position.sample(now, |from, target, linear| {
            let factor = if entering {
                ease_out_cubic(linear)
            } else {
                ease_in_cubic(linear)
            };
            from + (target - from) * factor
        });

        VisibilitySample {
            opacity,
            position,
            running: opacity_running || position_running,
        }
    }

    /// The paint effect for the axis of a scrollbar `track_width` wide, at `now`, or at the end of
    /// the transition with None.
    ///
    /// The opacity is the sample of the transition. The offset is the distance the thumb has
    /// left to slide in from the edge, toward the edge it leaves by.
    fn effect(&self, axis: Axis, track_width: Pixels, now: Option<Instant>) -> PaintEffect {
        let (opacity, position) = match now {
            Some(now) => {
                let sample = self.sample(now);
                (sample.opacity, sample.position)
            }
            None => (self.opacity.target, self.position.target),
        };
        PaintEffect::opacity(opacity).with_translation(visibility_translation(
            axis,
            track_width,
            position,
        ))
    }

    /// Reverse or start a transition toward `visible`.
    ///
    /// The leg runs for `enter` or `exit` scaled by the distance still to cover,
    /// so an interrupted transition keeps its speed instead of restarting.
    fn set_visible(
        &mut self,
        visible: bool,
        entrance: ScrollbarEntrance,
        enter: Duration,
        exit: Duration,
        now: Instant,
    ) {
        let target = if visible { 1.0 } else { 0.0 };
        let full_duration = if visible { enter } else { exit };
        if full_duration.is_zero() {
            // A motionless policy — reduced motion, an always-visible scrollbar,
            // or a theme that projects no motion — adopts the target outright,
            // even if a transition was in flight when the policy changed.
            self.opacity.settle(target, now);
            self.position.settle(target, now);
            self.entrance = entrance;
            return;
        }
        if self.opacity.target == target
            && self.position.target == target
            && self.entrance == entrance
        {
            return;
        }

        let sample = self.sample(now);
        let from_position = if visible && entrance == ScrollbarEntrance::Fade {
            1.0
        } else {
            sample.position
        };
        let distance = (target - sample.opacity)
            .abs()
            .max((target - from_position).abs());
        let duration = full_duration.mul_f32(distance);
        self.opacity.start(sample.opacity, target, duration, now);
        self.position.start(from_position, target, duration, now);
        self.entrance = entrance;
        self.generation += 1;
    }
}

fn visibility_translation(axis: Axis, track_width: Pixels, progress: f32) -> Point<Pixels> {
    let offset = track_width * (1.0 - progress.clamp(0.0, 1.0));
    if axis.is_vertical() {
        point(offset, px(0.))
    } else {
        point(px(0.), offset)
    }
}

fn wants_visible(
    mode: ScrollbarMode,
    is_hovered: bool,
    is_dragging: bool,
    last_scroll_time: Option<Instant>,
    idle: Duration,
    now: Instant,
) -> bool {
    mode.is_always()
        || is_dragging
        || (mode.is_hover() && is_hovered)
        || last_scroll_time.is_some_and(|last| now.saturating_duration_since(last) < idle)
}

fn tracks_thumb_hover(mode: ScrollbarMode, is_visible: bool) -> bool {
    mode.is_hover() || is_visible
}

fn hover_keeps_visible(mode: ScrollbarMode, is_hovered: bool, is_currently_visible: bool) -> bool {
    is_hovered && (mode.is_hover() || (mode == ScrollbarMode::Scrolling && is_currently_visible))
}

impl Deref for ScrollbarState {
    type Target = Rc<Cell<ScrollbarStateInner>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ScrollbarState {
    fn notify_drag(
        &self,
        now: Instant,
        interval: Duration,
        view: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut inner = self.get();
        if inner.drag_update_scheduled {
            return;
        }
        let elapsed = now.saturating_duration_since(inner.last_update);
        if elapsed >= interval {
            self.set(inner.with_last_update(now));
            cx.notify(view);
            return;
        }
        inner.drag_update_scheduled = true;
        self.set(inner);
        let generation = inner.drag_update_generation;
        let delay = interval - elapsed;
        let state = self.clone();
        window
            .spawn(cx, async move |cx| {
                cx.background_executor().timer(delay).await;
                let mut inner = state.get();
                // A release or new drag supersedes the pending trailing update.
                if !inner.drag_update_scheduled || inner.drag_update_generation != generation {
                    return;
                }
                inner.drag_update_scheduled = false;
                inner.last_update = cx.background_executor().now();
                state.set(inner);
                cx.update(|_, cx| cx.notify(view)).ok();
            })
            .detach();
    }
}

impl ScrollbarStateInner {
    fn with_drag_pos(&self, axis: Axis, pos: Point<Pixels>, now: Instant) -> Self {
        let mut state = *self;
        if axis.is_vertical() {
            state.drag_pos.y = pos.y;
        } else {
            state.drag_pos.x = pos.x;
        }

        state.dragged_axis = Some(axis);
        state.last_update = now;
        state.drag_update_scheduled = false;
        state.drag_update_generation = state.drag_update_generation.wrapping_add(1);
        state
    }

    fn with_unset_drag_pos(&self, now: Instant) -> Self {
        let mut state = *self;
        state.dragged_axis = None;
        state.drag_update_scheduled = false;
        state.drag_update_generation = state.drag_update_generation.wrapping_add(1);
        state.last_scroll_time = Some(now);
        state
    }

    fn with_hovered(&self, axis: Option<Axis>, now: Instant) -> Self {
        let mut state = *self;
        state.hovered_axis = axis;
        state.last_scroll_time = Some(now);
        state
    }

    fn with_hovered_on_thumb(&self, axis: Option<Axis>, now: Instant) -> Self {
        let mut state = *self;
        state.hovered_on_thumb = axis;
        if self.is_scrollbar_visible(now) {
            if axis.is_some() {
                state.last_scroll_time = Some(now);
            }
        }
        state
    }

    fn with_last_scroll(
        &self,
        last_scroll_offset: Point<Pixels>,
        last_scroll_time: Option<Instant>,
    ) -> Self {
        let mut state = *self;
        state.last_scroll_offset = last_scroll_offset;
        state.last_scroll_time = last_scroll_time;
        state
    }

    fn with_last_update(&self, t: Instant) -> Self {
        let mut state = *self;
        state.last_update = t;
        state
    }

    fn with_idle_timer_scheduled(&self, scheduled: bool) -> Self {
        let mut state = *self;
        state.idle_timer_scheduled = scheduled;
        state
    }

    fn is_scrollbar_visible(&self, now: Instant) -> bool {
        self.dragged_axis.is_some() || self.visibility.sample(now).opacity > 0.0
    }
}

/// Scrollbar axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAxis {
    /// Vertical scrollbar.
    Vertical,
    /// Horizontal scrollbar.
    Horizontal,
    /// Show both vertical and horizontal scrollbars.
    Both,
}

/// Paint-only styles for a scrollbar track.
#[derive(Clone, Default)]
pub struct ScrollbarTrackStyle {
    background: Option<Hsla>,
    border: Option<Hsla>,
    width: Option<Pixels>,
}

impl ScrollbarTrackStyle {
    pub fn bg(mut self, background: Hsla) -> Self {
        self.background = Some(background);
        self
    }

    pub fn border_color(mut self, border: Hsla) -> Self {
        self.border = Some(border);
        self
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }
}

impl FluentBuilder for ScrollbarTrackStyle {}

/// Paint-only styles for a scrollbar thumb.
#[derive(Clone, Default)]
pub struct ScrollbarThumbStyle {
    background: Option<Background>,
    width: Option<Pixels>,
    inset: Option<Pixels>,
    radius: Option<Pixels>,
    min_length: Option<Pixels>,
}

impl ScrollbarThumbStyle {
    pub fn bg(mut self, background: impl Into<Background>) -> Self {
        self.background = Some(background.into());
        self
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn inset(mut self, inset: impl Into<Pixels>) -> Self {
        self.inset = Some(inset.into());
        self
    }

    pub fn radius(mut self, radius: impl Into<Pixels>) -> Self {
        self.radius = Some(radius.into());
        self
    }

    pub fn min_length(mut self, min_length: impl Into<Pixels>) -> Self {
        self.min_length = Some(min_length.into());
        self
    }
}

impl FluentBuilder for ScrollbarThumbStyle {}

/// Typed paint styles supported by [`Scrollbar`].
#[derive(Clone, Default)]
pub struct ScrollbarStyles {
    track: ScrollbarTrackStyle,
    track_hover: ScrollbarTrackStyle,
    track_active: ScrollbarTrackStyle,
    thumb: ScrollbarThumbStyle,
    thumb_hover: ScrollbarThumbStyle,
    thumb_active: ScrollbarThumbStyle,
}

impl ScrollbarStyles {
    pub fn track(mut self, build: impl FnOnce(ScrollbarTrackStyle) -> ScrollbarTrackStyle) -> Self {
        self.track = build(self.track);
        self
    }

    pub fn track_hover(
        mut self,
        build: impl FnOnce(ScrollbarTrackStyle) -> ScrollbarTrackStyle,
    ) -> Self {
        self.track_hover = build(self.track_hover);
        self
    }

    pub fn track_active(
        mut self,
        build: impl FnOnce(ScrollbarTrackStyle) -> ScrollbarTrackStyle,
    ) -> Self {
        self.track_active = build(self.track_active);
        self
    }

    pub fn thumb(mut self, build: impl FnOnce(ScrollbarThumbStyle) -> ScrollbarThumbStyle) -> Self {
        self.thumb = build(self.thumb);
        self
    }

    pub fn thumb_hover(
        mut self,
        build: impl FnOnce(ScrollbarThumbStyle) -> ScrollbarThumbStyle,
    ) -> Self {
        self.thumb_hover = build(self.thumb_hover);
        self
    }

    pub fn thumb_active(
        mut self,
        build: impl FnOnce(ScrollbarThumbStyle) -> ScrollbarThumbStyle,
    ) -> Self {
        self.thumb_active = build(self.thumb_active);
        self
    }
}

impl FluentBuilder for ScrollbarStyles {}

impl From<Axis> for ScrollbarAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Vertical => Self::Vertical,
            Axis::Horizontal => Self::Horizontal,
        }
    }
}

impl ScrollbarAxis {
    /// Return true if the scrollbar axis is vertical.
    #[inline]
    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Vertical)
    }

    /// Return true if the scrollbar axis is horizontal.
    #[inline]
    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal)
    }

    /// Return true if the scrollbar axis is both vertical and horizontal.
    #[inline]
    pub fn is_both(&self) -> bool {
        matches!(self, Self::Both)
    }

    /// Return true if the scrollbar has vertical axis.
    #[inline]
    pub fn has_vertical(&self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }

    /// Return true if the scrollbar has horizontal axis.
    #[inline]
    pub fn has_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }

    #[inline]
    fn all(&self) -> Vec<Axis> {
        match self {
            Self::Vertical => vec![Axis::Vertical],
            Self::Horizontal => vec![Axis::Horizontal],
            // This should keep Horizontal first, Vertical is the primary axis
            // if Vertical not need display, then Horizontal will not keep right margin.
            Self::Both => vec![Axis::Horizontal, Axis::Vertical],
        }
    }
}

/// Scrollbar control for scroll-area or a uniform-list.
pub struct Scrollbar {
    pub(crate) id: ElementId,
    axis: ScrollbarAxis,
    mode: Option<ScrollbarMode>,
    scroll_handle: Rc<dyn ScrollbarHandle>,
    scroll_size: Option<Size<Pixels>>,
    viewport_bounds: Option<Bounds<Pixels>>,
    use_layout_bounds: bool,
    /// Maximum frames per second for scrolling by drag. Default is 120 FPS.
    ///
    /// This is used to limit the update rate of the scrollbar when it is
    /// being dragged for some complex interactions for reducing CPU usage.
    max_fps: usize,
    styles: ScrollbarStyles,
}

impl Scrollbar {
    /// Create a new scrollbar.
    ///
    /// This will have both vertical and horizontal scrollbars.
    #[track_caller]
    pub fn new<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::CodeLocation(*caller),
            axis: ScrollbarAxis::Both,
            mode: None,
            scroll_handle: Rc::new(scroll_handle.clone()),
            max_fps: 120,
            scroll_size: None,
            viewport_bounds: None,
            use_layout_bounds: false,
            styles: ScrollbarStyles::default(),
        }
    }

    /// Create with horizontal scrollbar.
    #[track_caller]
    pub fn horizontal<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Horizontal)
    }

    /// Create with vertical scrollbar.
    #[track_caller]
    pub fn vertical<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Vertical)
    }

    /// Set a specific element id, default is the [`Location::caller`].
    ///
    /// NOTE: In most cases, you don't need to set a specific id for scrollbar.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the scrollbar show mode [`ScrollbarMode`].
    ///
    /// If unset, the current application theme projection is used.
    pub fn mode(mut self, mode: ScrollbarMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set a special scroll size of the content area, default is None.
    ///
    /// Default will sync the `content_size` from `scroll_handle`.
    pub fn scroll_size(mut self, scroll_size: Size<Pixels>) -> Self {
        self.scroll_size = Some(scroll_size);
        self
    }

    /// Override the viewport bounds that this scrollbar overlays.
    ///
    /// Most scroll containers should rely on the bounds reported by their
    /// scroll handle. Custom-painted viewports, such as the text editor, can
    /// use this when their visible bounds differ from the handle's layout
    /// bounds.
    pub fn viewport_bounds(mut self, bounds: Bounds<Pixels>) -> Self {
        self.viewport_bounds = Some(bounds);
        self
    }

    /// Use the scrollbar element's layout bounds as its viewport.
    ///
    /// This is useful for composite widgets whose scrollbar viewport excludes
    /// fixed headers or columns and is therefore defined by a positioned
    /// overlay container rather than by the underlying scroll handle.
    pub fn viewport_from_layout(mut self) -> Self {
        self.use_layout_bounds = true;
        self
    }

    fn resolved_viewport_bounds(&self, layout_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        self.viewport_bounds.unwrap_or_else(|| {
            if self.use_layout_bounds {
                layout_bounds
            } else {
                self.scroll_handle.viewport_bounds()
            }
        })
    }

    /// Set scrollbar axis.
    pub fn axis(mut self, axis: impl Into<ScrollbarAxis>) -> Self {
        self.axis = axis.into();
        self
    }

    pub fn styles(mut self, build: impl FnOnce(ScrollbarStyles) -> ScrollbarStyles) -> Self {
        self.styles = build(self.styles);
        self
    }

    /// Set maximum frames per second for scrolling by drag. Default is 120 FPS.
    ///
    /// If you have very high CPU usage, consider reducing this value to improve performance.
    ///
    /// Available values: 30..120
    #[doc(hidden)]
    pub fn max_fps(mut self, max_fps: usize) -> Self {
        self.max_fps = max_fps.clamp(30, 120);
        self
    }

    // Get the width of the scrollbar.
    #[doc(hidden)]
    pub const fn width() -> Pixels {
        WIDTH
    }

    fn resolve_track(
        &self,
        cx: &App,
        state: &ScrollbarTrackStyle,
        global_state: &ScrollbarTrackStyle,
        default_border: Hsla,
    ) -> (Hsla, Hsla) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();
        (
            state
                .background
                .or(self.styles.track.background)
                .or(global_state.background)
                .or(global.track.background)
                .unwrap_or_else(gpui::transparent_black),
            state
                .border
                .or(self.styles.track.border)
                .or(global_state.border)
                .or(global.track.border)
                .unwrap_or(default_border),
        )
    }

    fn resolve_thumb(
        &self,
        cx: &App,
        state: &ScrollbarThumbStyle,
        global_state: &ScrollbarThumbStyle,
        defaults: ScrollbarThumbStyle,
    ) -> (Background, Pixels, Pixels, Pixels, Pixels) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();
        (
            state
                .background
                .or(self.styles.thumb.background)
                .or(global_state.background)
                .or(global.thumb.background)
                .unwrap_or_else(|| defaults.background.unwrap()),
            state
                .width
                .or(self.styles.thumb.width)
                .or(global_state.width)
                .or(global.thumb.width)
                .unwrap_or_else(|| defaults.width.unwrap()),
            state
                .inset
                .or(self.styles.thumb.inset)
                .or(global_state.inset)
                .or(global.thumb.inset)
                .unwrap_or_else(|| defaults.inset.unwrap()),
            state
                .radius
                .or(self.styles.thumb.radius)
                .or(global_state.radius)
                .or(global.thumb.radius)
                .unwrap_or_else(|| defaults.radius.unwrap()),
            state
                .min_length
                .or(self.styles.thumb.min_length)
                .or(global_state.min_length)
                .or(global.thumb.min_length)
                .or(defaults.min_length)
                .unwrap_or(MIN_THUMB_SIZE),
        )
    }

    /// The thumb colour a scrollbar falls back to when nothing has overridden
    /// it, taken from the active theme rather than fixed.
    ///
    /// It used to be literal black at these alphas. That reads as an ordinary
    /// grey thumb on a light surface and as very nearly nothing at all on a
    /// dark one, and no palette an application installed could change it:
    /// `Theme` carries `scrollbar` beside `tokens` rather than derived from
    /// them, so a theme swap moved every token except the ones the scrollbar
    /// actually paints with.
    ///
    /// `foreground` is the token that already means "ink on this surface" and
    /// already flips with the appearance, so on a light theme this stays within
    /// a hair of the old constant and on a dark one it becomes visible. An
    /// explicit `ScrollbarStyles` still wins: this is the bottom of the
    /// cascade, not a new top of it.
    fn thumb_default_background(cx: &App, alpha: f32) -> Background {
        cx.theme().tokens.colors.foreground.alpha(alpha).into()
    }

    fn thumb_defaults(
        background: Background,
        width: Pixels,
        inset: Pixels,
        radius: Pixels,
    ) -> ScrollbarThumbStyle {
        ScrollbarThumbStyle {
            background: Some(background),
            width: Some(width),
            inset: Some(inset),
            radius: Some(radius),
            min_length: Some(MIN_THUMB_SIZE),
        }
    }

    fn style_for_active(
        &self,
        cx: &App,
    ) -> (Background, Hsla, Hsla, Pixels, Pixels, Pixels, Pixels) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();
        let (track, border) = self.resolve_track(
            cx,
            &self.styles.track_active,
            &global.track_active,
            gpui::transparent_black(),
        );
        let (thumb, width, inset, radius, min_length) = self.resolve_thumb(
            cx,
            &self.styles.thumb_active,
            &global.thumb_active,
            Self::thumb_defaults(
                Self::thumb_default_background(cx, 0.55),
                THUMB_ACTIVE_WIDTH,
                THUMB_ACTIVE_INSET,
                THUMB_ACTIVE_RADIUS,
            ),
        );
        (thumb, track, border, width, inset, radius, min_length)
    }

    fn style_for_hovered_thumb(
        &self,
        cx: &App,
    ) -> (Background, Hsla, Hsla, Pixels, Pixels, Pixels, Pixels) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();
        let (track, border) = self.resolve_track(
            cx,
            &self.styles.track_active,
            &global.track_active,
            gpui::transparent_black(),
        );
        let (thumb, width, inset, radius, min_length) = self.resolve_thumb(
            cx,
            &self.styles.thumb_hover,
            &global.thumb_hover,
            Self::thumb_defaults(
                Self::thumb_default_background(cx, 0.55),
                THUMB_ACTIVE_WIDTH,
                THUMB_ACTIVE_INSET,
                THUMB_ACTIVE_RADIUS,
            ),
        );
        (thumb, track, border, width, inset, radius, min_length)
    }

    fn style_for_hovered_bar(
        &self,
        cx: &App,
    ) -> (Background, Hsla, Hsla, Pixels, Pixels, Pixels, Pixels) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();
        let (track, border) = self.resolve_track(
            cx,
            &self.styles.track_hover,
            &global.track_hover,
            gpui::transparent_black(),
        );
        let (thumb, width, inset, radius, min_length) = self.resolve_thumb(
            cx,
            &self.styles.thumb,
            &global.thumb,
            Self::thumb_defaults(
                Self::thumb_default_background(cx, 0.35),
                THUMB_WIDTH,
                THUMB_INSET,
                THUMB_RADIUS,
            ),
        );
        (thumb, track, border, width, inset, radius, min_length)
    }

    fn style_for_normal(
        &self,
        cx: &App,
    ) -> (Background, Hsla, Hsla, Pixels, Pixels, Pixels, Pixels) {
        let theme = cx.theme();
        let global = theme.scrollbar.styles();

        let (track, border) = self.resolve_track(
            cx,
            &self.styles.track,
            &global.track,
            gpui::transparent_black(),
        );
        let (thumb, width, inset, radius, min_length) = self.resolve_thumb(
            cx,
            &self.styles.thumb,
            &global.thumb,
            Self::thumb_defaults(
                Self::thumb_default_background(cx, 0.35),
                THUMB_WIDTH,
                THUMB_INSET,
                THUMB_RADIUS,
            ),
        );
        (thumb, track, border, width, inset, radius, min_length)
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[doc(hidden)]
pub struct PrepaintState {
    hitbox: Hitbox,
    scrollbar_state: ScrollbarState,
    states: Vec<AxisPrepaintState>,
    /// The quads of each axis, painted with the fade and the slide as a paint
    /// effect. Empty while the scrollbar is hidden, or has no axis to show.
    quads: Vec<AnyElement>,
}

#[doc(hidden)]
pub struct AxisPrepaintState {
    axis: Axis,
    bar_hitbox: Hitbox,
    bounds: Bounds<Pixels>,
    radius: Pixels,
    bg: Hsla,
    border: Hsla,
    thumb_bounds: Bounds<Pixels>,
    // Bounds of thumb to be rendered.
    thumb_fill_bounds: Bounds<Pixels>,
    thumb_bg: Background,
    geometry: ThumbGeometry,
    active_geometry: ThumbGeometry,
    track_width: Pixels,
    visibility_requested: bool,
}

/// What one axis of a scrollbar paints.
struct AxisQuads {
    bounds: Bounds<Pixels>,
    track: Hsla,
    border: Hsla,
    thumb_bounds: Bounds<Pixels>,
    thumb: Background,
    radius: Pixels,
}

/// The track, border and thumb of one axis, without the listeners.
///
/// They are an element of their own so that the fade and the slide can be a paint
/// effect, which GPUI applies to what an element painted, in every frame, without
/// laying out the view around it again.
struct ScrollbarQuads {
    clip: Bounds<Pixels>,
    quads: AxisQuads,
}

impl IntoElement for ScrollbarQuads {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ScrollbarQuads {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let clip = self.clip;
        let axis = &self.quads;
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
            window.paint_layer(clip, |window| {
                window.paint_quad(fill(axis.bounds, axis.track));

                window.paint_quad(PaintQuad {
                    bounds: axis.bounds,
                    corner_radii: (0.).into(),
                    background: gpui::transparent_black().into(),
                    border_widths: Edges {
                        top: px(0.),
                        right: px(0.),
                        bottom: px(0.),
                        left: px(0.),
                    },
                    border_color: axis.border,
                    border_style: BorderStyle::default(),
                });

                window.paint_quad(fill(axis.thumb_bounds, axis.thumb).corner_radii(axis.radius));
            });
        });
    }
}

/// The same longitudinal geometry drives painting, track clicks and dragging.
/// `length` excludes the inset, while `travel` excludes the full logical thumb.
#[derive(Clone, Copy)]
struct ThumbGeometry {
    origin: Pixels,
    inset: Pixels,
    length: Pixels,
    travel: Pixels,
    extent: Pixels,
}

impl ThumbGeometry {
    fn new(
        origin: Pixels,
        container: Pixels,
        content: Pixels,
        margin_end: Pixels,
        inset: Pixels,
        min_length: Pixels,
    ) -> Self {
        let track = (container - margin_end).max(px(0.));
        let logical_length = (container / content * container).max(min_length).min(track);
        let inset = inset.clamp(px(0.), logical_length / 2.);
        Self {
            origin,
            inset,
            length: logical_length - inset * 2.,
            travel: track - logical_length,
            extent: content - container,
        }
    }

    fn start(self, offset: Pixels) -> Pixels {
        self.origin + self.inset + (-offset / self.extent).clamp(0., 1.) * self.travel
    }

    fn offset(self, position: Pixels, grab: Pixels) -> Pixels {
        if self.travel <= px(0.) {
            return px(0.);
        }
        -self.extent * ((position - self.origin - self.inset - grab) / self.travel).clamp(0., 1.)
    }

    fn drag_offset(
        self,
        axis: Axis,
        position: Point<Pixels>,
        grab: Point<Pixels>,
        mut offset: Point<Pixels>,
    ) -> Point<Pixels> {
        if self.travel <= px(0.) {
            return offset;
        }
        if axis.is_vertical() {
            offset.y = self.offset(position.y, grab.y);
        } else {
            offset.x = self.offset(position.x, grab.x);
        }
        offset
    }
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<gpui::ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let bounds = self.resolved_viewport_bounds(bounds);
        let hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        });

        // The clock the window's animations and timers run on, so that a test clock
        // moves the scrollbar as it moves everything else.
        let now = cx.background_executor().now();
        let state = window
            .use_state(cx, |_, _| ScrollbarState::new(now))
            .read(cx)
            .clone();
        let base_theme = cx.theme();
        let mode = self.mode.unwrap_or(base_theme.scrollbar.mode());
        let motion = base_theme.scrollbar.motion();
        // Always-visible scrollbars skip visibility motion but still animate
        // their activity width. Reduced motion snaps every channel.
        let reduce_motion = cx.reduce_motion();
        let (enter, exit) = if !mode.is_always() && !reduce_motion {
            (motion.enter(), motion.exit())
        } else {
            (Duration::ZERO, Duration::ZERO)
        };
        let expand = if reduce_motion {
            Duration::ZERO
        } else {
            motion.expand()
        };

        let mut inner = state.get();
        let current_offset = self.scroll_handle.offset();
        if current_offset != inner.last_scroll_offset {
            inner = inner.with_last_scroll(current_offset, Some(now));
        }

        let is_hovered = inner.hovered_axis.is_some() || inner.hovered_on_thumb.is_some();
        let is_dragging = inner.dragged_axis.is_some();
        let is_currently_visible = inner.visibility.sample(now).opacity > 0.0;
        let visible = hover_keeps_visible(mode, is_hovered, is_currently_visible)
            || wants_visible(
                mode,
                is_hovered,
                is_dragging,
                inner.last_scroll_time,
                motion.idle(),
                now,
            );
        inner.visibility.set_visible(
            visible,
            motion.entrance_for(mode, inner.hovered_on_thumb.is_some()),
            enter,
            exit,
            now,
        );
        let visibility = inner.visibility.sample(now);
        inner.painted = PaintEffect::opacity(visibility.opacity);

        if !is_hovered && !is_dragging {
            if let Some(last_time) = inner.last_scroll_time {
                let elapsed = now.saturating_duration_since(last_time);
                if elapsed < motion.idle() && !inner.idle_timer_scheduled {
                    inner.idle_timer_scheduled = true;
                    let state = state.clone();
                    let current_view = window.current_view();
                    let next_delay = motion.idle() - elapsed;
                    window
                        .spawn(cx, async move |cx| {
                            cx.background_executor().timer(next_delay).await;
                            state.set(state.get().with_idle_timer_scheduled(false));
                            cx.update(|_, cx| cx.notify(current_view)).ok();
                        })
                        .detach();
                }
            }
        }
        state.set(inner);

        let mut states = vec![];
        let mut has_both = self.axis.is_both();
        let scroll_size = self
            .scroll_size
            .unwrap_or(self.scroll_handle.content_size());

        for axis in self.axis.all().into_iter() {
            let is_vertical = axis.is_vertical();
            let track_width = self
                .styles
                .track
                .width
                .or(cx.theme().scrollbar.styles().track.width)
                .unwrap_or(WIDTH);
            let (scroll_area_size, container_size, scroll_position) = if is_vertical {
                (
                    scroll_size.height,
                    hitbox.size.height,
                    self.scroll_handle.offset().y,
                )
            } else {
                (
                    scroll_size.width,
                    hitbox.size.width,
                    self.scroll_handle.offset().x,
                )
            };

            // The horizontal scrollbar is set avoid overlapping with the vertical scrollbar, if the vertical scrollbar is visible.
            let margin_end = if has_both && !is_vertical {
                track_width
            } else {
                px(0.)
            };

            // Hide scrollbar, if the scroll area is smaller than the container.
            if scroll_area_size <= container_size {
                has_both = false;
                continue;
            }

            let bounds = Bounds {
                origin: if is_vertical {
                    point(
                        hitbox.origin.x + hitbox.size.width - track_width,
                        hitbox.origin.y,
                    )
                } else {
                    point(
                        hitbox.origin.x,
                        hitbox.origin.y + hitbox.size.height - track_width,
                    )
                },
                size: gpui::Size {
                    width: if is_vertical {
                        track_width
                    } else {
                        hitbox.size.width
                    },
                    height: if is_vertical {
                        hitbox.size.height
                    } else {
                        track_width
                    },
                },
            };

            let is_always_to_show = mode.is_always();
            let is_hover_to_show = mode.is_hover();
            let is_hovered_on_bar = state.get().hovered_axis == Some(axis);
            let is_hovered_on_thumb = state.get().hovered_on_thumb == Some(axis);

            let (thumb_bg, bar_bg, bar_border, mut thumb_width, inset, radius, min_length) =
                if state.get().dragged_axis == Some(axis) {
                    self.style_for_active(cx)
                } else if (is_hover_to_show || mode == ScrollbarMode::Scrolling)
                    && (is_hovered_on_bar || is_hovered_on_thumb)
                {
                    if is_hovered_on_thumb {
                        self.style_for_hovered_thumb(cx)
                    } else {
                        self.style_for_hovered_bar(cx)
                    }
                } else if is_always_to_show && (is_hovered_on_bar || is_hovered_on_thumb) {
                    if is_hovered_on_thumb {
                        self.style_for_hovered_thumb(cx)
                    } else {
                        self.style_for_hovered_bar(cx)
                    }
                } else {
                    self.style_for_normal(cx)
                };

            let mut width_animation = if is_vertical {
                state.get().vertical_width
            } else {
                state.get().horizontal_width
            };
            let (animated_width, running) = width_animation.set_target(thumb_width, expand, now);
            let mut updated = state.get();
            if is_vertical {
                updated.vertical_width = width_animation;
            } else {
                updated.horizontal_width = width_animation;
            }
            state.set(updated);
            thumb_width = animated_width;
            if running {
                window.request_animation_frame();
            }

            let origin = if is_vertical {
                bounds.origin.y
            } else {
                bounds.origin.x
            };
            let geometry = ThumbGeometry::new(
                origin,
                container_size,
                scroll_area_size,
                margin_end,
                inset,
                min_length,
            );
            let (_, _, _, _, active_inset, _, active_min_length) = self.style_for_active(cx);
            let active_geometry = ThumbGeometry::new(
                origin,
                container_size,
                scroll_area_size,
                margin_end,
                active_inset,
                active_min_length,
            );
            let thumb_start = geometry.start(scroll_position) - origin;

            // The clickable area of the thumb
            let thumb_length = geometry.length;
            let thumb_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, thumb_start),
                    size(track_width, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(thumb_start, -inset),
                    size(thumb_length, track_width),
                )
            };

            // The actual render area of the thumb
            let thumb_fill_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, thumb_start),
                    size(thumb_width, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(thumb_start, -inset),
                    size(thumb_length, thumb_width),
                )
            };

            let bar_hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
            });

            states.push(AxisPrepaintState {
                axis,
                bar_hitbox,
                bounds,
                radius,
                bg: bar_bg,
                border: bar_border,
                thumb_bounds,
                thumb_fill_bounds,
                thumb_bg,
                geometry,
                active_geometry,
                track_width,
                visibility_requested: visible,
            })
        }

        // A hidden scrollbar paints nothing. One on its way in or out paints each axis
        // with the opacity and the offset of each frame, which the animation below
        // works out from the clock, and the window asks for those frames until it
        // ends. Nothing notifies this view for them.
        let mut quads = Vec::new();
        if visibility.opacity > 0.0 || visibility.running {
            let duration = if visibility.running {
                inner.visibility.opacity.duration
            } else {
                Duration::ZERO
            };
            let plan = inner.visibility;
            for axis_state in &states {
                let radius = if cx.theme().tokens.radius.md.is_zero() {
                    px(0.)
                } else {
                    axis_state.radius
                };
                let (axis, track_width) = (axis_state.axis, axis_state.track_width);
                let clock = cx.background_executor().clone();
                let painted = state.clone();
                let mut element = ScrollbarQuads {
                    clip: hitbox.bounds,
                    quads: AxisQuads {
                        bounds: axis_state.bounds,
                        track: axis_state.bg,
                        border: axis_state.border,
                        thumb_bounds: axis_state.thumb_fill_bounds,
                        thumb: axis_state.thumb_bg,
                        radius: clamp_thumb_radius(radius, axis_state.thumb_fill_bounds),
                    },
                }
                .with_paint_animation(
                    (
                        if axis.is_vertical() {
                            "scrollbar-fade-vertical"
                        } else {
                            "scrollbar-fade-horizontal"
                        },
                        plan.generation,
                    ),
                    Animation::new(duration),
                    move |progress| {
                        // The last frame lands on the target, however the clocks line up.
                        let now = (progress < 1.0).then(|| clock.now());
                        let effect = plan.effect(axis, track_width, now);
                        let mut inner = painted.get();
                        inner.painted = effect;
                        painted.set(inner);
                        effect
                    },
                )
                .into_any_element();
                element.prepaint_as_root(hitbox.origin, AvailableSpace::min_size(), window, cx);
                quads.push(element);
            }
        }

        PrepaintState {
            hitbox,
            states,
            scrollbar_state: state,
            quads,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scrollbar_state = &prepaint.scrollbar_state;
        let mode = self.mode.unwrap_or(cx.theme().scrollbar.mode());
        let view_id = window.current_view();
        let hitbox_bounds = prepaint.hitbox.bounds;
        let is_hover_to_show = mode.is_hover();

        for quads in &mut prepaint.quads {
            quads.paint(window, cx);
        }

        window.with_content_mask(
            Some(ContentMask {
                bounds: hitbox_bounds,
            }),
            |window| {
                for state in prepaint.states.iter() {
                    let axis = state.axis;
                    let bounds = state.bounds;
                    let thumb_bounds = state.thumb_bounds;
                    let geometry = state.geometry;
                    let active_geometry = state.active_geometry;
                    // The fade ends in a frame no view is laid out for, so whether
                    // the scrollbar still shows is worked out when the event comes.
                    let requested = state.visibility_requested;
                    let is_visible = move |state: &ScrollbarState| {
                        requested || state.get().painted.opacity > 0.0
                    };

                    window.set_cursor_style(CursorStyle::default(), &state.bar_hitbox);

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |event: &ScrollWheelEvent, phase, _, cx| {
                            if phase.bubble() && hitbox_bounds.contains(&event.position) {
                                if scroll_handle.offset() != state.get().last_scroll_offset {
                                    state.set(state.get().with_last_scroll(
                                        scroll_handle.offset(),
                                        Some(cx.background_executor().now()),
                                    ));
                                    cx.notify(view_id);
                                }
                            }
                        }
                    });

                    // A thumb follows the finger, unlike content panning. Claim
                    // this touch before the window turns it into wheel deltas.
                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();
                        let max_fps_duration = Duration::from_secs_f64(1. / self.max_fps as f64);
                        move |event: &TouchDragEvent, phase, window, cx| {
                            if !phase.bubble() {
                                return;
                            }
                            if event.phase == TouchPhase::Started {
                                if !is_visible(&state)
                                    || window.default_prevented()
                                    || !thumb_bounds.contains(&event.start_position)
                                {
                                    return;
                                }
                                scroll_handle.start_drag();
                                state.set(state.get().with_drag_pos(
                                    axis,
                                    event.start_position - thumb_bounds.origin,
                                    cx.background_executor().now(),
                                ));
                            } else if state.get().dragged_axis != Some(axis) {
                                return;
                            }
                            if event.phase != TouchPhase::Cancelled {
                                scroll_handle.set_offset(active_geometry.drag_offset(
                                    axis,
                                    event.position,
                                    state.get().drag_pos,
                                    scroll_handle.offset(),
                                ));
                            }
                            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                                scroll_handle.end_drag();
                                state.set(
                                    state
                                        .get()
                                        .with_unset_drag_pos(cx.background_executor().now()),
                                );
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                            if event.phase == TouchPhase::Moved {
                                state.notify_drag(
                                    cx.background_executor().now(),
                                    max_fps_duration,
                                    view_id,
                                    window,
                                    cx,
                                );
                            } else {
                                cx.notify(view_id);
                            }
                        }
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |event: &MouseDownEvent, phase, _, cx| {
                            if phase.bubble()
                                && is_visible(&state)
                                && bounds.contains(&event.position)
                            {
                                cx.stop_propagation();

                                if thumb_bounds.contains(&event.position) {
                                    // click on the thumb bar, set the drag position
                                    let pos = event.position - thumb_bounds.origin;

                                    scroll_handle.start_drag();
                                    state.set(state.get().with_drag_pos(
                                        axis,
                                        pos,
                                        cx.background_executor().now(),
                                    ));
                                    // Active styling can change the inset or minimum length.
                                    // Keep the original grab point under the pointer on that frame.
                                    scroll_handle.set_offset(active_geometry.drag_offset(
                                        axis,
                                        event.position,
                                        pos,
                                        scroll_handle.offset(),
                                    ));
                                } else {
                                    // click on the scrollbar, jump to the position
                                    // Set the thumb bar center to the click position
                                    let center = point(geometry.length / 2., geometry.length / 2.);
                                    scroll_handle.set_offset(geometry.drag_offset(
                                        axis,
                                        event.position,
                                        center,
                                        scroll_handle.offset(),
                                    ));
                                }

                                cx.notify(view_id);
                            }
                        }
                    });

                    window.on_mouse_event({
                        let scroll_handle = self.scroll_handle.clone();
                        let state = scrollbar_state.clone();
                        let max_fps_duration = Duration::from_secs_f64(1. / self.max_fps as f64);

                        move |event: &MouseMoveEvent, _, window, cx| {
                            let mut notify = false;
                            let now = cx.background_executor().now();
                            let is_visible = is_visible(&state);
                            // When is hover to show mode or it was visible,
                            // we need to update the hovered state and increase the last_scroll_time.
                            let need_hover_to_update = is_hover_to_show || is_visible;
                            // Update hovered state for scrollbar
                            if bounds.contains(&event.position) && need_hover_to_update {
                                let hover_changed = state.get().hovered_axis != Some(axis);
                                state.set(state.get().with_hovered(Some(axis), now));
                                notify |= hover_changed;
                            } else if state.get().hovered_axis == Some(axis) {
                                state.set(state.get().with_hovered(None, now));
                                notify = true;
                            }

                            // Update hovered state for scrollbar thumb
                            if tracks_thumb_hover(mode, is_visible)
                                && thumb_bounds.contains(&event.position)
                            {
                                if state.get().hovered_on_thumb != Some(axis) {
                                    state.set(state.get().with_hovered_on_thumb(Some(axis), now));
                                    notify = true;
                                }
                            } else {
                                if state.get().hovered_on_thumb == Some(axis) {
                                    state.set(state.get().with_hovered_on_thumb(None, now));
                                    notify = true;
                                }
                            }

                            // Move thumb position on dragging
                            if state.get().dragged_axis == Some(axis) && event.dragging() {
                                // Stop the event propagation to avoid selecting text or other side effects.
                                cx.stop_propagation();

                                let offset = active_geometry.drag_offset(
                                    axis,
                                    event.position,
                                    state.get().drag_pos,
                                    scroll_handle.offset(),
                                );
                                if scroll_handle.offset() != offset {
                                    // Coalesce rendering, never discard the newest pointer position.
                                    scroll_handle.set_offset(offset);
                                    notify = true;
                                }
                                if notify {
                                    state.notify_drag(
                                        cx.background_executor().now(),
                                        max_fps_duration,
                                        view_id,
                                        window,
                                        cx,
                                    );
                                }
                                return;
                            }

                            if notify {
                                cx.notify(view_id);
                            }
                        }
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |event: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() && state.get().dragged_axis == Some(axis) {
                                scroll_handle.set_offset(active_geometry.drag_offset(
                                    axis,
                                    event.position,
                                    state.get().drag_pos,
                                    scroll_handle.offset(),
                                ));
                                scroll_handle.end_drag();
                                state.set(
                                    state
                                        .get()
                                        .with_unset_drag_pos(cx.background_executor().now()),
                                );
                                cx.notify(view_id);
                            }
                        }
                    });
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::{Cell, RefCell};

    use gpui::{
        AppContext as _, Context, Entity, Modifiers, MouseButton, ParentElement as _, Render,
        StyleRefinement, Styled as _, TestAppContext, VisualTestContext, div,
    };

    #[test]
    fn thumb_radius_is_limited_by_its_actual_bounds() {
        let vertical_thumb = Bounds::new(Point::default(), size(px(8.), px(80.)));
        let horizontal_thumb = Bounds::new(Point::default(), size(px(80.), px(6.)));

        assert_eq!(clamp_thumb_radius(px(6.), vertical_thumb), px(4.));
        assert_eq!(clamp_thumb_radius(px(6.), horizontal_thumb), px(3.));
        assert_eq!(
            clamp_thumb_radius(Pixels::ZERO, vertical_thumb),
            Pixels::ZERO
        );
    }

    /// Timing standing in for what a styled layer projects. Base itself ships
    /// none of these; see [`motionless_base_snaps_every_transition`].
    const ENTER: Duration = Duration::from_millis(300);
    const EXIT: Duration = Duration::from_millis(500);
    const EXPAND: Duration = Duration::from_millis(300);

    #[test]
    fn visibility_animation_uses_direction_specific_curves_and_durations() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);

        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);
        assert_eq!(animation.sample(start).opacity, 0.0);
        let entering = animation.sample(start + ENTER / 2).position;
        assert!(entering > 0.5, "ease-out must advance quickly");
        let entered = animation.sample(start + ENTER);
        assert_eq!(entered.opacity, 1.0);
        assert_eq!(entered.position, 1.0);

        animation.set_visible(
            false,
            ScrollbarEntrance::SlideAndFade,
            ENTER,
            EXIT,
            start + ENTER,
        );
        let exiting = animation.sample(start + ENTER + EXIT / 2).opacity;
        assert!(exiting > 0.5, "ease-in must remain visible early in exit");
        assert_eq!(animation.sample(start + ENTER + EXIT).opacity, 0.0);
    }

    #[test]
    fn entrance_fades_linearly_while_position_eases_out() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);

        let halfway = animation.sample(start + ENTER / 2);
        assert!((halfway.opacity - 0.5).abs() < 0.001);
        assert!(halfway.position > halfway.opacity);
    }

    #[test]
    fn fade_entrance_snaps_position_and_animates_opacity() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, start);

        let initial = animation.sample(start);
        assert_eq!(initial.opacity, 0.0);
        assert_eq!(initial.position, 1.0);
        let halfway = animation.sample(start + ENTER / 2);
        assert!((halfway.opacity - 0.5).abs() < 0.001);
        assert_eq!(halfway.position, 1.0);
    }

    #[test]
    fn active_visibility_adopts_a_changed_entrance_policy() {
        let start = Instant::now();
        let halfway = start + ENTER / 2;
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);
        let before = animation.sample(halfway);

        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, halfway);
        let after = animation.sample(halfway);

        assert_eq!(
            after.opacity, before.opacity,
            "policy changes must not flash"
        );
        assert_eq!(after.position, 1.0, "fade entrance must stop stale sliding");
    }

    #[test]
    fn base_ships_no_motion_of_its_own() {
        let motion = ScrollbarMotion::default();
        assert_eq!(motion.enter(), Duration::ZERO);
        assert_eq!(motion.exit(), Duration::ZERO);
        assert_eq!(motion.expand(), Duration::ZERO);
        assert_eq!(motion.entrance(), ScrollbarEntrance::Fade);
        assert_eq!(motion.thumb_hover_entrance(), ScrollbarEntrance::Fade);
        assert_eq!(
            motion.idle(),
            DEFAULT_IDLE,
            "the visibility hold is behavior, not motion, and must stay usable"
        );
    }

    #[test]
    fn hover_mode_slides_only_when_the_thumb_is_hovered() {
        let motion = ScrollbarMotion::default()
            .with_entrance(ScrollbarEntrance::Fade)
            .with_thumb_hover_entrance(ScrollbarEntrance::SlideAndFade);

        assert_eq!(
            motion.entrance_for(ScrollbarMode::Hover, false),
            ScrollbarEntrance::Fade
        );
        assert_eq!(
            motion.entrance_for(ScrollbarMode::Hover, true),
            ScrollbarEntrance::SlideAndFade
        );
        assert_eq!(
            motion.entrance_for(ScrollbarMode::Scrolling, true),
            ScrollbarEntrance::Fade
        );
    }

    #[test]
    fn hidden_scrolling_mode_does_not_track_thumb_hover() {
        assert!(!tracks_thumb_hover(ScrollbarMode::Scrolling, false));
        assert!(tracks_thumb_hover(ScrollbarMode::Scrolling, true));
        assert!(tracks_thumb_hover(ScrollbarMode::Hover, false));
    }

    #[test]
    fn visible_scrolling_mode_stays_visible_while_hovered() {
        assert!(!hover_keeps_visible(ScrollbarMode::Scrolling, true, false));
        assert!(hover_keeps_visible(ScrollbarMode::Scrolling, true, true));
        assert!(hover_keeps_visible(ScrollbarMode::Hover, true, false));
    }

    #[test]
    fn motionless_base_snaps_every_transition() {
        let now = Instant::now();
        let motion = ScrollbarMotion::default();
        let mut visibility = VisibilityAnimation::hidden(now);

        visibility.set_visible(true, motion.entrance(), motion.enter(), motion.exit(), now);
        let shown = visibility.sample(now);
        assert_eq!(shown.opacity, 1.0);
        assert_eq!(shown.position, 1.0);
        assert!(!shown.running, "a motionless theme must request no frames");
        assert_eq!(
            visibility_translation(Axis::Vertical, px(16.), shown.position),
            Point::default()
        );

        visibility.set_visible(false, motion.entrance(), motion.enter(), motion.exit(), now);
        let hidden = visibility.sample(now);
        assert_eq!(hidden.opacity, 0.0);
        assert_eq!(hidden.position, 0.0);
        assert!(!hidden.running);

        let mut width = WidthAnimation::new(now);
        assert_eq!(
            width.set_target(px(8.), motion.expand(), now),
            (px(8.), false)
        );
    }

    #[test]
    fn a_zero_duration_settles_a_transition_already_in_flight() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);

        // Reduced motion turns on midway through the entrance.
        let midway = start + ENTER / 2;
        animation.set_visible(
            true,
            ScrollbarEntrance::SlideAndFade,
            Duration::ZERO,
            Duration::ZERO,
            midway,
        );
        let settled = animation.sample(midway);
        assert_eq!(settled.opacity, 1.0);
        assert_eq!(settled.position, 1.0);
        assert!(!settled.running);
    }

    #[test]
    fn thumb_expansion_animates_in_both_directions() {
        let start = Instant::now();
        let mut animation = WidthAnimation::new(start);
        assert_eq!(animation.set_target(px(6.), EXPAND, start), (px(6.), false));

        let (initial, running) = animation.set_target(px(8.), EXPAND, start);
        assert_eq!(initial, px(6.));
        assert!(running);
        let (expanded_halfway, _) = animation.sample(start + EXPAND / 2);
        assert!(expanded_halfway > px(7.));

        let reversal = start + EXPAND / 2;
        let before = animation.sample(reversal).0;
        let (after_reversal, running) = animation.set_target(px(6.), EXPAND, reversal);
        assert_eq!(after_reversal, before);
        assert!(running);
        assert_eq!(animation.sample(reversal + EXPAND).0, px(6.));
    }

    #[test]
    fn reduced_motion_snaps_thumb_expansion() {
        let now = Instant::now();
        let mut animation = WidthAnimation::new(now);
        assert_eq!(
            animation.set_target(px(8.), Duration::ZERO, now),
            (px(8.), false)
        );
    }

    #[test]
    fn visibility_translation_moves_toward_the_nearest_edge() {
        assert_eq!(
            visibility_translation(Axis::Vertical, px(16.), 0.0),
            point(px(16.), px(0.))
        );
        assert_eq!(
            visibility_translation(Axis::Horizontal, px(16.), 0.0),
            point(px(0.), px(16.))
        );
        assert_eq!(
            visibility_translation(Axis::Vertical, px(16.), 1.0),
            Point::default()
        );
    }

    #[test]
    fn visibility_animation_reverses_from_current_progress() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);
        let reversal_time = start + Duration::from_millis(60);
        let before = animation.sample(reversal_time);

        animation.set_visible(
            false,
            ScrollbarEntrance::SlideAndFade,
            ENTER,
            EXIT,
            reversal_time,
        );
        let reversed = animation.sample(reversal_time);
        assert_eq!(reversed.opacity, before.opacity);
        assert_eq!(reversed.position, before.position);
        let after = animation.sample(reversal_time + Duration::from_millis(10));
        assert!(after.opacity < before.opacity);
        assert!(after.position < before.position);
    }

    #[test]
    fn always_hover_drag_and_recent_scroll_request_visibility() {
        let now = Instant::now();
        let idle = DEFAULT_IDLE;
        assert!(wants_visible(
            ScrollbarMode::Always,
            false,
            false,
            None,
            idle,
            now
        ));
        assert!(wants_visible(
            ScrollbarMode::Hover,
            true,
            false,
            None,
            idle,
            now
        ));
        assert!(wants_visible(
            ScrollbarMode::Scrolling,
            false,
            true,
            None,
            idle,
            now
        ));
        assert!(wants_visible(
            ScrollbarMode::Scrolling,
            false,
            false,
            Some(now - idle + Duration::from_millis(1)),
            idle,
            now,
        ));
        assert!(!wants_visible(
            ScrollbarMode::Scrolling,
            false,
            false,
            Some(now - idle),
            idle,
            now,
        ));

        let mut settled = VisibilityAnimation::hidden(now - ENTER);
        settled.set_visible(
            true,
            ScrollbarEntrance::SlideAndFade,
            ENTER,
            EXIT,
            now - ENTER,
        );
        assert!(!settled.sample(now).running, "idle hold must not animate");
    }

    #[test]
    fn leaving_hover_starts_a_fresh_idle_hold() {
        let entered_at = Instant::now();
        let left_at = entered_at + Duration::from_secs(5);
        let state = ScrollbarState::new(entered_at).get();

        let hovered = state.with_hovered(Some(Axis::Vertical), entered_at);
        assert_eq!(hovered.last_scroll_time, Some(entered_at));
        let left = hovered.with_hovered(None, left_at);
        assert_eq!(left.last_scroll_time, Some(left_at));
        assert!(wants_visible(
            ScrollbarMode::Hover,
            false,
            false,
            left.last_scroll_time,
            DEFAULT_IDLE,
            left_at + DEFAULT_IDLE - Duration::from_millis(1),
        ));
    }

    #[test]
    fn idle_boundary_starts_the_exit_without_a_jump() {
        let activity = Instant::now();
        let exit_start = activity + DEFAULT_IDLE;
        let mut animation = VisibilityAnimation::hidden(activity - ENTER);
        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, activity - ENTER);
        assert!(!wants_visible(
            ScrollbarMode::Scrolling,
            false,
            false,
            Some(activity),
            DEFAULT_IDLE,
            exit_start,
        ));

        animation.set_visible(false, ScrollbarEntrance::Fade, ENTER, EXIT, exit_start);
        let start = animation.sample(exit_start);
        assert_eq!(start.opacity, 1.0);
        assert_eq!(start.position, 1.0);
        assert_eq!(animation.sample(exit_start + EXIT).opacity, 0.0);
    }

    #[derive(Clone)]
    struct TestHandle {
        offset: Rc<Cell<Point<Pixels>>>,
        content_size: Size<Pixels>,
        drag_starts: Rc<Cell<usize>>,
        drag_ends: Rc<Cell<usize>>,
    }

    impl TestHandle {
        fn new(content_size: Size<Pixels>) -> Self {
            Self {
                offset: Rc::new(Cell::new(Point::default())),
                content_size,
                drag_starts: Rc::new(Cell::new(0)),
                drag_ends: Rc::new(Cell::new(0)),
            }
        }
    }

    impl ScrollbarHandle for TestHandle {
        fn viewport_bounds(&self) -> Bounds<Pixels> {
            Bounds::new(Point::default(), size(px(100.), px(100.)))
        }

        fn offset(&self) -> Point<Pixels> {
            self.offset.get()
        }

        fn set_offset(&self, offset: Point<Pixels>) {
            self.offset.set(offset);
        }

        fn content_size(&self) -> Size<Pixels> {
            self.content_size
        }

        fn start_drag(&self) {
            self.drag_starts.set(self.drag_starts.get() + 1);
        }

        fn end_drag(&self) {
            self.drag_ends.set(self.drag_ends.get() + 1);
        }
    }

    struct ScrollbarHarness {
        handle: TestHandle,
        axis: ScrollbarAxis,
        mode: ScrollbarMode,
    }

    impl Render for ScrollbarHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size(px(100.))
                .child(Scrollbar::new(&self.handle).axis(self.axis).mode(self.mode))
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        axis: ScrollbarAxis,
        mode: ScrollbarMode,
        content_size: Size<Pixels>,
    ) -> (&mut VisualTestContext, TestHandle) {
        let handle = TestHandle::new(content_size);
        let (_, cx) = cx.add_window_view({
            let handle = handle.clone();
            move |_, _| ScrollbarHarness { handle, axis, mode }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (cx, handle)
    }

    #[test]
    fn each_transition_starts_a_generation_and_a_repeat_does_not() {
        let start = Instant::now();
        let mut animation = VisibilityAnimation::hidden(start);
        assert_eq!(animation.generation, 0);

        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, start);
        assert_eq!(animation.generation, 1);
        let later = start + ENTER / 2;
        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, later);
        assert_eq!(animation.generation, 1, "the transition goes on");

        animation.set_visible(false, ScrollbarEntrance::Fade, ENTER, EXIT, later);
        assert_eq!(animation.generation, 2, "reversing starts another");

        // A motionless policy settles without a run of frames to key.
        animation.set_visible(true, ScrollbarEntrance::Fade, Duration::ZERO, EXIT, later);
        assert_eq!(animation.generation, 2);
    }

    #[test]
    fn the_effect_fades_and_slides_with_the_curves_of_the_transition() {
        let start = Instant::now();
        let width = px(16.);
        let at = |animation: &VisibilityAnimation, time: Duration| {
            animation.effect(Axis::Vertical, width, Some(start + time))
        };

        // A fade in takes its place at once and fades linearly.
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::Fade, ENTER, EXIT, start);
        let halfway = at(&animation, ENTER / 2);
        assert!((halfway.opacity - 0.5).abs() < 0.001);
        assert_eq!(halfway.translation, Point::default());

        // A slide in comes from the edge it leaves by, on a cubic ease-out.
        let mut animation = VisibilityAnimation::hidden(start);
        animation.set_visible(true, ScrollbarEntrance::SlideAndFade, ENTER, EXIT, start);
        let halfway = at(&animation, ENTER / 2);
        assert!((halfway.opacity - 0.5).abs() < 0.001);
        assert!((halfway.translation.x - width * (1. - ease_out_cubic(0.5))).abs() < px(0.001));
        assert_eq!(halfway.translation.y, px(0.));
        assert_eq!(at(&animation, Duration::ZERO).translation.x, width);
        assert_eq!(at(&animation, ENTER).translation.x, px(0.));

        // A fade out slides toward the edge on a cubic ease-in, whatever the entrance was.
        let exit = start + ENTER;
        animation.set_visible(false, ScrollbarEntrance::Fade, ENTER, EXIT, exit);
        let leaving = animation.effect(Axis::Vertical, width, Some(exit + EXIT / 2));
        assert!((leaving.opacity - (1. - ease_in_cubic(0.5))).abs() < 0.001);
        assert!((leaving.translation.x - width * ease_in_cubic(0.5)).abs() < px(0.001));
        // A horizontal bar leaves by the bottom.
        let below = animation.effect(Axis::Horizontal, width, Some(exit + EXIT / 2));
        assert_eq!(below.translation.x, px(0.));
        assert_eq!(below.translation.y, leaving.translation.x);
        // The last frame is the end of the transition, however the clocks line up.
        let ended = animation.effect(Axis::Vertical, width, None);
        assert_eq!((ended.opacity, ended.translation.x), (0., width));
    }

    /// A scrollbar that hands out its state, to read what the last frame painted it with.
    struct StateProbe {
        scrollbar: Scrollbar,
        state: Rc<RefCell<Option<ScrollbarState>>>,
    }

    impl IntoElement for StateProbe {
        type Element = Self;

        fn into_element(self) -> Self {
            self
        }
    }

    impl Element for StateProbe {
        type RequestLayoutState = ();
        type PrepaintState = PrepaintState;

        fn id(&self) -> Option<ElementId> {
            Element::id(&self.scrollbar)
        }

        fn source_location(&self) -> Option<&'static Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            self.scrollbar.request_layout(id, inspector, window, cx)
        }

        fn prepaint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            layout: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) -> PrepaintState {
            let state = self
                .scrollbar
                .prepaint(id, inspector, bounds, layout, window, cx);
            *self.state.borrow_mut() = Some(state.scrollbar_state.clone());
            state
        }

        fn paint(
            &mut self,
            id: Option<&GlobalElementId>,
            inspector: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            layout: &mut (),
            state: &mut PrepaintState,
            window: &mut Window,
            cx: &mut App,
        ) {
            self.scrollbar
                .paint(id, inspector, bounds, layout, state, window, cx);
        }
    }

    /// A view with a scrollbar that counts how often it renders.
    struct FadePane {
        handle: TestHandle,
        mode: ScrollbarMode,
        renders: Rc<Cell<usize>>,
        state: Rc<RefCell<Option<ScrollbarState>>>,
    }

    impl Render for FadePane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().relative().size(px(100.)).child(StateProbe {
                scrollbar: Scrollbar::vertical(&self.handle).mode(self.mode),
                state: self.state.clone(),
            })
        }
    }

    /// A window whose root caches the view of the scrollbar, as an application does for
    /// a long transcript.
    struct FadeWindow {
        pane: Entity<FadePane>,
    }

    impl Render for FadeWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(StyleRefinement::default().size(px(100.)).flex_none()),
            )
        }
    }

    const STEP: Duration = Duration::from_millis(25);

    struct FadeFixture {
        handle: TestHandle,
        pane: Entity<FadePane>,
        renders: Rc<Cell<usize>>,
        state: Rc<RefCell<Option<ScrollbarState>>>,
    }

    impl FadeFixture {
        /// Opens the window with the motion a styled layer projects.
        fn open(
            cx: &mut TestAppContext,
            mode: ScrollbarMode,
            reduce_motion: bool,
        ) -> (Self, &mut VisualTestContext) {
            cx.update(|cx| {
                cx.set_reduce_motion(reduce_motion);
                crate::Theme::global_mut(cx).scrollbar =
                    crate::ScrollbarTheme::new().with_mode(mode).with_motion(
                        ScrollbarMotion::default()
                            .with_idle(DEFAULT_IDLE)
                            .with_enter(ENTER)
                            .with_exit(EXIT)
                            .with_thumb_hover_entrance(ScrollbarEntrance::SlideAndFade),
                    );
            });
            let handle = TestHandle::new(size(px(100.), px(500.)));
            let renders = Rc::new(Cell::new(0));
            let state = Rc::new(RefCell::new(None));
            let (window, cx) = cx.add_window_view({
                let handle = handle.clone();
                let renders = renders.clone();
                let state = state.clone();
                move |_, cx| FadeWindow {
                    pane: cx.new(|_| FadePane {
                        handle,
                        mode,
                        renders,
                        state,
                    }),
                }
            });
            let pane = window.read_with(cx, |root, _| root.pane.clone());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            (
                Self {
                    handle,
                    pane,
                    renders,
                    state,
                },
                cx,
            )
        }

        /// The effect the last frame painted the scrollbar with.
        fn effect(&self) -> PaintEffect {
            self.state.borrow().as_ref().unwrap().get().painted
        }

        /// The opacity the last frame painted the scrollbar with.
        fn painted(&self) -> f32 {
            self.effect().opacity
        }

        /// Scrolls to `y` and renders the view for it, as a wheel event does.
        fn scroll(&self, y: f32, cx: &mut VisualTestContext) {
            self.handle.set_offset(point(px(0.), px(y)));
            self.pane.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
        }

        /// Steps the clock and the display's next frame, and tells how many frames the
        /// window had asked for.
        fn step(cx: &mut VisualTestContext, by: Duration) -> usize {
            cx.executor().advance_clock(by);
            let asked = cx.update(|window, cx| window.simulate_next_frame(cx));
            cx.run_until_parked();
            asked
        }

        /// Steps through `time` in frames that the window has to ask for, and tells what
        /// each painted the scrollbar with.
        fn fade(&self, time: Duration, cx: &mut VisualTestContext) -> Vec<PaintEffect> {
            let mut effects = vec![self.effect()];
            for _ in 0..(time.as_millis() / STEP.as_millis()) {
                assert!(
                    Self::step(cx, STEP) >= 1,
                    "the window asks for the next frame"
                );
                effects.push(self.effect());
            }
            effects
        }
    }

    /// The fade steps through frames the window asks for, and the view that holds the
    /// scrollbar is not rendered for any of them. Rendering it for each step cost a
    /// transcript about 56 renders for each fade, and the session view around it as well.
    #[gpui::test]
    fn the_fade_paints_in_frames_without_rendering_its_view(cx: &mut TestAppContext) {
        let (fixture, cx) = FadeFixture::open(cx, ScrollbarMode::Scrolling, false);
        assert_eq!(fixture.renders.get(), 1);
        assert_eq!(fixture.painted(), 0.0);
        assert_eq!(FadeFixture::step(cx, STEP), 0, "nothing to draw");

        // Scrolling shows the scrollbar, which renders the view once for the new offset.
        fixture.scroll(-20., cx);
        assert_eq!(fixture.renders.get(), 2);
        let effects = fixture.fade(ENTER, cx);
        assert_eq!(fixture.painted(), 1.0, "shown after the fade in");
        // Linear, over the enter time, in place, as the animation that rendered the view was.
        let steps = ENTER.as_millis() as f32 / STEP.as_millis() as f32;
        for (ix, effect) in effects.iter().enumerate() {
            assert!(
                (effect.opacity - ix as f32 / steps).abs() < 0.001,
                "{effects:?}"
            );
            assert_eq!(effect.translation, Point::default(), "{effects:?}");
        }
        assert_eq!(
            fixture.renders.get(),
            2,
            "the fade in never rendered the view"
        );

        // The animation ends in the first frame past its duration, and then it holds
        // with no frames until the idle time is out.
        assert_eq!(FadeFixture::step(cx, STEP), 1);
        assert_eq!(FadeFixture::step(cx, STEP), 0, "nothing asks for a frame");
        assert_eq!(FadeFixture::step(cx, Duration::from_millis(1400)), 0);
        assert_eq!(fixture.painted(), 1.0);
        assert_eq!(fixture.renders.get(), 2, "the hold never rendered the view");

        // The idle timer renders the view once to start the fade out, and the fade
        // goes on in frames. The hold has used 300 + 50 + 1400 ms of the 2 s.
        assert_eq!(FadeFixture::step(cx, Duration::from_millis(249)), 0);
        FadeFixture::step(cx, Duration::from_millis(1));
        assert_eq!(fixture.renders.get(), 3, "the idle time ran out");
        let effects = fixture.fade(EXIT, cx);
        assert_eq!(
            fixture.renders.get(),
            3,
            "the fade out never rendered the view"
        );
        assert_eq!(fixture.painted(), 0.0, "hidden after the fade out");
        // It eases in over the exit time and slides toward the right edge by the width of
        // the track, as the animation that rendered the view did.
        let steps = EXIT.as_millis() as f32 / STEP.as_millis() as f32;
        for (ix, effect) in effects.iter().enumerate() {
            let eased = ease_in_cubic(ix as f32 / steps);
            assert!(
                (effect.opacity - (1.0 - eased)).abs() < 0.001,
                "{effects:?}"
            );
            assert!(
                (effect.translation.x - WIDTH * eased).abs() < px(0.001),
                "{effects:?}"
            );
            assert_eq!(effect.translation.y, px(0.));
        }

        assert_eq!(FadeFixture::step(cx, STEP), 1);
        assert_eq!(FadeFixture::step(cx, STEP), 0, "no frames after the end");
        assert_eq!(fixture.painted(), 0.0);
        assert_eq!(fixture.effect().translation, point(WIDTH, px(0.)));
        assert_eq!(fixture.renders.get(), 3);

        // The scrollbar no longer takes the pointer, though no render followed its end.
        cx.simulate_click(point(px(95.), px(80.)), Modifiers::default());
        assert_eq!(fixture.handle.offset(), point(px(0.), px(-20.)));
    }

    #[gpui::test]
    fn scrolling_in_the_fade_out_fades_back_in_from_where_it_was(cx: &mut TestAppContext) {
        let (fixture, cx) = FadeFixture::open(cx, ScrollbarMode::Scrolling, false);
        fixture.scroll(-20., cx);
        fixture.fade(ENTER, cx);
        FadeFixture::step(cx, DEFAULT_IDLE - ENTER);
        assert_eq!(fixture.renders.get(), 3, "the fade out began");
        let leaving = fixture.fade(EXIT / 2, cx);
        let before = leaving.last().unwrap().opacity;
        assert!(before > 0.5 && before < 1.0, "{leaving:?}");
        assert!(leaving.last().unwrap().translation.x > px(0.));

        // The new offset renders the view, and the opacity goes on from where it was,
        // up to 1 in the part of the enter time that is left to cover. The thumb comes
        // back to its place at once, as a fade in does.
        fixture.scroll(-40., cx);
        assert_eq!(fixture.renders.get(), 4);
        assert!((fixture.painted() - before).abs() < 0.001);
        assert_eq!(fixture.effect().translation, Point::default());
        let entering = fixture.fade(STEP, cx);
        assert!(entering[1].opacity > before, "{entering:?}");
        assert_eq!(FadeFixture::step(cx, STEP), 1);
        assert_eq!(FadeFixture::step(cx, STEP), 0);
        assert_eq!(fixture.painted(), 1.0);
        assert_eq!(
            fixture.renders.get(),
            4,
            "the fade in never rendered the view"
        );
    }

    /// The slide in of a thumb the pointer reaches in Hover mode is a paint effect too, from
    /// the edge on a cubic ease-out, and renders no view.
    #[gpui::test]
    fn the_slide_in_runs_without_rendering_the_view(cx: &mut TestAppContext) {
        let (fixture, cx) = FadeFixture::open(cx, ScrollbarMode::Hover, false);
        cx.simulate_mouse_move(point(px(95.), px(20.)), None, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(fixture.renders.get(), 2, "the pointer reached the thumb");

        let effects = fixture.fade(ENTER, cx);
        let steps = ENTER.as_millis() as f32 / STEP.as_millis() as f32;
        for (ix, effect) in effects.iter().enumerate() {
            let progress = ix as f32 / steps;
            assert!((effect.opacity - progress).abs() < 0.001, "{effects:?}");
            let expected = WIDTH * (1. - ease_out_cubic(progress));
            assert!(
                (effect.translation.x - expected).abs() < px(0.001),
                "{effects:?}"
            );
        }
        assert_eq!(
            fixture.renders.get(),
            2,
            "the slide never rendered the view"
        );
        assert_eq!(FadeFixture::step(cx, STEP), 1);
        assert_eq!(FadeFixture::step(cx, STEP), 0);
        assert_eq!(fixture.effect(), PaintEffect::NONE);
    }

    #[gpui::test]
    fn reduced_motion_shows_and_hides_without_frames(cx: &mut TestAppContext) {
        let (fixture, cx) = FadeFixture::open(cx, ScrollbarMode::Scrolling, true);
        fixture.scroll(-20., cx);
        assert_eq!(fixture.renders.get(), 2);
        assert_eq!(fixture.painted(), 1.0, "shown at once");
        assert_eq!(FadeFixture::step(cx, Duration::from_millis(1900)), 0);
        assert_eq!(fixture.painted(), 1.0, "held until the idle time is out");

        FadeFixture::step(cx, Duration::from_millis(100));
        assert_eq!(fixture.renders.get(), 3, "the idle time renders the view");
        assert_eq!(fixture.painted(), 0.0, "hidden at once");
        assert_eq!(FadeFixture::step(cx, STEP), 0);
        assert_eq!(fixture.renders.get(), 3);
    }

    #[test]
    fn explicit_viewport_bounds_override_handle_bounds() {
        let expected = Bounds::new(point(px(12.), px(24.)), size(px(240.), px(96.)));
        let scrollbar = Scrollbar::vertical(&TestHandle::new(size(px(240.), px(480.))))
            .viewport_bounds(expected);

        assert_eq!(
            scrollbar.resolved_viewport_bounds(Bounds::default()),
            expected
        );
    }

    #[test]
    fn layout_viewport_uses_current_element_bounds() {
        let expected = Bounds::new(point(px(20.), px(30.)), size(px(180.), px(12.)));
        let scrollbar =
            Scrollbar::horizontal(&TestHandle::new(size(px(600.), px(12.)))).viewport_from_layout();

        assert_eq!(scrollbar.resolved_viewport_bounds(expected), expected);
    }

    #[test]
    fn typed_styles_are_fluent_and_include_geometry() {
        let track = gpui::hsla(0.1, 0.2, 0.3, 1.0);
        let border = gpui::hsla(0.2, 0.3, 0.4, 1.0);
        let thumb = gpui::hsla(0.3, 0.4, 0.5, 1.0);
        let hover = gpui::hsla(0.4, 0.5, 0.6, 1.0);
        let active = gpui::hsla(0.5, 0.6, 0.7, 1.0);
        let scrollbar = Scrollbar::new(&TestHandle::new(Size::default())).styles(|styles| {
            styles
                .track(|style| {
                    style
                        .width(px(14.))
                        .bg(track)
                        .border_color(border)
                        .when(false, |style| style.width(px(99.)))
                })
                .thumb(|style| {
                    style
                        .width(px(7.))
                        .inset(px(3.))
                        .radius(px(3.5))
                        .min_length(px(40.))
                        .bg(thumb)
                })
                .thumb_hover(|style| style.width(px(9.)).bg(hover))
                .thumb_active(|style| style.radius(px(4.5)).bg(active))
        });

        assert_eq!(scrollbar.styles.track.background, Some(track));
        assert_eq!(scrollbar.styles.track.border, Some(border));
        assert_eq!(scrollbar.styles.track.width, Some(px(14.)));
        assert!(scrollbar.styles.thumb.background.is_some());
        assert_eq!(scrollbar.styles.thumb.width, Some(px(7.)));
        assert_eq!(scrollbar.styles.thumb.inset, Some(px(3.)));
        assert_eq!(scrollbar.styles.thumb.radius, Some(px(3.5)));
        assert_eq!(scrollbar.styles.thumb.min_length, Some(px(40.)));
        assert!(scrollbar.styles.thumb_hover.background.is_some());
        assert_eq!(scrollbar.styles.thumb_hover.width, Some(px(9.)));
        assert!(scrollbar.styles.thumb_active.background.is_some());
        assert_eq!(scrollbar.styles.thumb_active.radius, Some(px(4.5)));
    }

    #[gpui::test]
    fn unstyled_thumb_follows_the_theme_rather_than_a_fixed_colour(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let scrollbar = Scrollbar::new(&TestHandle::new(Size::default()));

            let light = gpui::hsla(0., 0., 0.04, 1.0);
            crate::Theme::global_mut(cx).tokens.colors.foreground = light;
            let (on_light, ..) = scrollbar.style_for_normal(cx);

            let dark = gpui::hsla(0., 0., 0.98, 1.0);
            crate::Theme::global_mut(cx).tokens.colors.foreground = dark;
            let (on_dark, ..) = scrollbar.style_for_normal(cx);

            assert_eq!(on_light, Background::from(light.alpha(0.35)));
            assert_eq!(on_dark, Background::from(dark.alpha(0.35)));
            // The point of the change: a thumb that never moved with the
            // palette was invisible on one of the two surfaces.
            assert_ne!(on_light, on_dark);
        });
    }

    #[gpui::test]
    fn a_styled_thumb_still_beats_the_theme_derived_default(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let chosen = gpui::hsla(0.6, 0.5, 0.5, 1.0);
            crate::Theme::global_mut(cx).tokens.colors.foreground = gpui::hsla(0., 0., 0.98, 1.0);

            let scrollbar = Scrollbar::new(&TestHandle::new(Size::default()))
                .styles(|styles| styles.thumb(|style| style.bg(chosen)));
            let (thumb, ..) = scrollbar.style_for_normal(cx);

            assert_eq!(thumb, Background::from(chosen));
        });
    }

    #[gpui::test]
    fn instance_styles_override_theme_scrollbar_defaults(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let theme_track = gpui::hsla(0.1, 0.2, 0.3, 1.0);
            let theme_thumb = gpui::hsla(0.2, 0.3, 0.4, 1.0);
            let instance_thumb = gpui::hsla(0.3, 0.4, 0.5, 1.0);

            crate::Theme::global_mut(cx).scrollbar = crate::ScrollbarTheme::new()
                .with_mode(ScrollbarMode::Always)
                .with_motion(ScrollbarMotion::default())
                .with_styles(
                    ScrollbarStyles::default()
                        .track(|style| style.width(px(13.)).bg(theme_track))
                        .thumb(|style| style.width(px(7.)).bg(theme_thumb)),
                );

            let scrollbar = Scrollbar::new(&TestHandle::new(Size::default()))
                .styles(|styles| styles.thumb(|style| style.bg(instance_thumb)));
            let (thumb, track, _, width, _, _, _) = scrollbar.style_for_normal(cx);

            assert_eq!(thumb, Background::from(instance_thumb));
            assert_eq!(track, theme_track);
            assert_eq!(width, px(7.));
            assert_eq!(cx.theme().scrollbar.styles().track.width, Some(px(13.)));
        });
    }

    #[gpui::test]
    fn auto_hide_modes_use_a_six_pixel_resting_thumb(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let handle = TestHandle::new(Size::default());
            let scrolling = Scrollbar::new(&handle).mode(ScrollbarMode::Scrolling);
            let always = Scrollbar::new(&handle).mode(ScrollbarMode::Always);

            assert_eq!(scrolling.style_for_normal(cx).3, px(6.));
            assert_eq!(always.style_for_normal(cx).3, px(6.));
        });
    }

    #[gpui::test]
    fn every_mode_expands_only_for_thumb_hover(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let handle = TestHandle::new(Size::default());
            for mode in [
                ScrollbarMode::Scrolling,
                ScrollbarMode::Hover,
                ScrollbarMode::Always,
            ] {
                let scrollbar = Scrollbar::new(&handle).mode(mode);
                assert_eq!(scrollbar.style_for_normal(cx).3, px(6.));
                assert_eq!(scrollbar.style_for_hovered_bar(cx).3, px(6.));
                assert_eq!(scrollbar.style_for_hovered_thumb(cx).3, px(8.));
            }
        });
    }

    #[gpui::test]
    fn vertical_track_click_updates_vertical_offset(cx: &mut TestAppContext) {
        let (cx, vertical) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Always,
            size(px(100.), px(500.)),
        );
        cx.simulate_click(point(px(95.), px(80.)), Modifiers::default());
        assert!(vertical.offset().y < px(0.));
        assert_eq!(vertical.offset().x, px(0.));
    }

    #[gpui::test]
    fn horizontal_track_click_updates_horizontal_offset(cx: &mut TestAppContext) {
        let (cx, horizontal) = harness(
            cx,
            ScrollbarAxis::Horizontal,
            ScrollbarMode::Always,
            size(px(500.), px(100.)),
        );
        cx.simulate_click(point(px(80.), px(95.)), Modifiers::default());
        assert!(horizontal.offset().x < px(0.));
        assert_eq!(horizontal.offset().y, px(0.));
    }

    #[gpui::test]
    fn no_overflow_has_no_interactive_track(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Both,
            ScrollbarMode::Always,
            size(px(100.), px(100.)),
        );
        cx.simulate_click(point(px(95.), px(80.)), Modifiers::default());
        assert_eq!(handle.offset(), Point::default());
    }

    #[gpui::test]
    fn hidden_hover_scrollbar_ignores_track_click(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Hover,
            size(px(100.), px(500.)),
        );
        cx.simulate_click(point(px(95.), px(80.)), Modifiers::default());
        assert_eq!(handle.offset(), Point::default());
    }

    #[gpui::test]
    fn hidden_hover_scrollbar_ignores_thumb_drag(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Hover,
            size(px(100.), px(500.)),
        );
        cx.simulate_mouse_down(
            point(px(95.), px(20.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            point(px(95.), px(70.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(px(95.), px(70.)),
            MouseButton::Left,
            Modifiers::default(),
        );

        assert_eq!(handle.drag_starts.get(), 0);
        assert_eq!(handle.drag_ends.get(), 0);
        assert_eq!(handle.offset(), Point::default());
    }

    #[gpui::test]
    fn hovering_reveals_scrollbar_for_track_interaction(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Hover,
            size(px(100.), px(500.)),
        );
        // Base ships no motion, so the reveal is immediate.
        cx.simulate_mouse_move(point(px(95.), px(50.)), None, Modifiers::default());
        cx.run_until_parked();

        cx.simulate_click(point(px(95.), px(80.)), Modifiers::default());
        assert!(handle.offset().y < px(0.));
    }

    /// The throttle goes by the window's clock, so a drag that pauses for an
    /// interval of it draws its next move at once.
    #[gpui::test]
    fn a_drag_move_after_a_pause_draws_at_once(cx: &mut TestAppContext) {
        let handle = TestHandle::new(size(px(100.), px(1000.)));
        let (view, cx) = cx.add_window_view({
            let handle = handle.clone();
            move |_, _| ScrollbarHarness {
                handle,
                axis: ScrollbarAxis::Vertical,
                mode: ScrollbarMode::Always,
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let notifications = Rc::new(Cell::new(0));
        let _subscription = cx.update(|_, cx| {
            let notifications = notifications.clone();
            cx.observe(&view, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let thumb = point(px(95.), px(5.));
        cx.simulate_mouse_down(thumb, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            thumb + point(px(0.), px(10.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        // Any trailing draw of the first move happens in the pause.
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        let before = notifications.get();
        let offset = handle.offset().y;

        cx.simulate_mouse_move(
            thumb + point(px(0.), px(20.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        assert!(handle.offset().y < offset);
        assert_eq!(notifications.get(), before + 1);
    }

    #[gpui::test]
    fn drag_notifications_are_throttled_and_deliver_the_latest_offset(cx: &mut TestAppContext) {
        let handle = TestHandle::new(size(px(100.), px(1000.)));
        let (view, cx) = cx.add_window_view({
            let handle = handle.clone();
            move |_, _| ScrollbarHarness {
                handle,
                axis: ScrollbarAxis::Vertical,
                mode: ScrollbarMode::Always,
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = cx.update(|_, cx| {
            let observed = observed.clone();
            let handle = handle.clone();
            cx.observe(&view, move |_, _| {
                observed.borrow_mut().push(handle.offset().y)
            })
        });
        let now = Instant::now();
        let state = ScrollbarState::new(now);
        let interval = Duration::from_secs_f64(1. / 30.);
        let mut inner = state
            .get()
            .with_drag_pos(Axis::Vertical, Point::default(), now);
        inner.last_update = now;
        state.set(inner);
        let update = |offset, elapsed, cx: &mut VisualTestContext| {
            handle.set_offset(point(px(0.), px(offset)));
            cx.update(|window, cx| {
                state.notify_drag(now + elapsed, interval, view.entity_id(), window, cx)
            });
            cx.run_until_parked();
        };
        update(-10., Duration::ZERO, cx);
        cx.executor().advance_clock(Duration::from_millis(16));
        update(-20., Duration::from_millis(16), cx);
        update(-30., Duration::from_millis(16), cx);
        // Rendering at 120 Hz must not bypass the configured 30 Hz timer.
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();
        assert!(observed.borrow().is_empty());
        cx.executor().advance_clock(Duration::from_millis(16));
        cx.run_until_parked();
        assert!(observed.borrow().is_empty());
        cx.executor().advance_clock(Duration::from_millis(2));
        cx.run_until_parked();
        assert_eq!(
            *observed.borrow(),
            vec![px(-30.)],
            "one trailing notification must see the newest offset even when movement stops"
        );

        // A second burst gets its own interval, and release invalidates its timer.
        let next = state.get().last_update;
        handle.set_offset(point(px(0.), px(-40.)));
        cx.update(|window, cx| state.notify_drag(next, interval, view.entity_id(), window, cx));
        cx.run_until_parked();
        state.set(state.get().with_unset_drag_pos(Instant::now()));
        handle.set_offset(point(px(0.), px(-50.)));
        cx.update(|_, cx| cx.notify(view.entity_id()));
        assert_eq!(*observed.borrow(), vec![px(-30.), px(-50.)]);
        cx.executor().advance_clock(interval);
        cx.run_until_parked();
        assert_eq!(
            *observed.borrow(),
            vec![px(-30.), px(-50.)],
            "release must not leave a stale trailing notification"
        );
    }

    #[test]
    fn full_track_thumb_does_not_reset_the_scroll_offset() {
        let geometry = ThumbGeometry::new(px(20.), px(40.), px(500.), px(0.), px(4.), px(64.));
        let offset = point(px(0.), px(-100.));
        assert_eq!(
            geometry.drag_offset(
                Axis::Vertical,
                point(px(0.), px(50.)),
                point(px(0.), px(8.)),
                offset
            ),
            offset
        );
    }

    #[gpui::test]
    fn repeated_touch_and_mouse_drags_keep_the_painted_grab_point(cx: &mut TestAppContext) {
        struct Probe {
            scrollbar: Scrollbar,
            painted: Rc<Cell<Bounds<Pixels>>>,
        }
        impl IntoElement for Probe {
            type Element = Self;
            fn into_element(self) -> Self {
                self
            }
        }
        impl Element for Probe {
            type RequestLayoutState = ();
            type PrepaintState = PrepaintState;
            fn id(&self) -> Option<ElementId> {
                Element::id(&self.scrollbar)
            }
            fn source_location(&self) -> Option<&'static Location<'static>> {
                None
            }
            fn request_layout(
                &mut self,
                id: Option<&GlobalElementId>,
                inspector: Option<&InspectorElementId>,
                window: &mut Window,
                cx: &mut App,
            ) -> (LayoutId, ()) {
                self.scrollbar.request_layout(id, inspector, window, cx)
            }
            fn prepaint(
                &mut self,
                id: Option<&GlobalElementId>,
                inspector: Option<&InspectorElementId>,
                bounds: Bounds<Pixels>,
                layout: &mut (),
                window: &mut Window,
                cx: &mut App,
            ) -> PrepaintState {
                let state = self
                    .scrollbar
                    .prepaint(id, inspector, bounds, layout, window, cx);
                self.painted.set(state.states[0].thumb_fill_bounds);
                state
            }
            fn paint(
                &mut self,
                id: Option<&GlobalElementId>,
                inspector: Option<&InspectorElementId>,
                bounds: Bounds<Pixels>,
                layout: &mut (),
                state: &mut PrepaintState,
                window: &mut Window,
                cx: &mut App,
            ) {
                self.scrollbar
                    .paint(id, inspector, bounds, layout, state, window, cx);
            }
        }
        struct Root {
            handle: TestHandle,
            painted: Rc<Cell<Bounds<Pixels>>>,
            axis: ScrollbarAxis,
            viewport: Bounds<Pixels>,
        }
        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size(px(400.)).child(Probe {
                    scrollbar: Scrollbar::new(&self.handle)
                        .axis(self.axis)
                        .mode(ScrollbarMode::Always)
                        .viewport_bounds(self.viewport)
                        .styles(|style| {
                            style
                                .thumb(|style| style.inset(px(6.)).min_length(px(64.)))
                                .thumb_active(|style| style.inset(px(3.)).min_length(px(72.)))
                        }),
                    painted: self.painted.clone(),
                })
            }
        }
        for touch in [true, false] {
            for vertical in [true, false] {
                for (origin, viewport_size, content_size) in [
                    (Point::default(), 200., 1000.),
                    (point(px(30.), px(40.)), 240., 500.),
                ] {
                    let handle = TestHandle::new(size(px(content_size), px(content_size)));
                    handle.set_offset(point(px(-100.), px(-100.)));
                    let painted = Rc::new(Cell::new(Bounds::default()));
                    let (_, cx) = cx.add_window_view({
                        let handle = handle.clone();
                        let painted = painted.clone();
                        move |_, _| Root {
                            handle,
                            painted,
                            axis: if vertical {
                                ScrollbarAxis::Vertical
                            } else {
                                ScrollbarAxis::Both
                            },
                            viewport: Bounds::new(
                                origin,
                                size(px(viewport_size), px(viewport_size)),
                            ),
                        }
                    });
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    let initial = painted.get();
                    let start = initial.center();
                    let grab = if vertical {
                        start.y - initial.origin.y
                    } else {
                        start.x - initial.origin.x
                    };
                    let axis_delta = |value| {
                        if vertical {
                            point(px(0.), px(value))
                        } else {
                            point(px(value), px(0.))
                        }
                    };
                    let axis_value =
                        |point: Point<Pixels>| if vertical { point.y } else { point.x };
                    let started_position = if touch { start + axis_delta(5.) } else { start };
                    if touch {
                        cx.simulate_event(TouchDragEvent {
                            phase: TouchPhase::Started,
                            start_position: start,
                            position: started_position,
                        });
                    } else {
                        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                    }
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    assert!(
                        (axis_value(painted.get().origin) + grab - axis_value(started_position))
                            .abs()
                            < px(0.1),
                        "starting must preserve the grab point and first displacement"
                    );
                    let mut reference_offset = px(0.);
                    let mut offset_per_pixel = 0.;
                    for delta in [0., 24., -8., 32., -10., 8., 0.] {
                        let position = start + axis_delta(delta);
                        if touch {
                            cx.simulate_event(TouchDragEvent {
                                phase: TouchPhase::Moved,
                                start_position: start,
                                position,
                            });
                        } else {
                            cx.simulate_mouse_move(
                                position,
                                Some(MouseButton::Left),
                                Modifiers::default(),
                            );
                        }
                        cx.update(|window, cx| window.draw(cx).clear(cx));
                        if delta == 0. {
                            reference_offset = axis_value(handle.offset());
                        }
                        if delta == 24. {
                            offset_per_pixel =
                                (axis_value(handle.offset()) - reference_offset) / px(24.);
                        }
                        let actual = if vertical {
                            painted.get().origin.y + grab
                        } else {
                            painted.get().origin.x + grab
                        };
                        let expected = if vertical { position.y } else { position.x };
                        assert!(
                            (actual - expected).abs() < px(0.1),
                            "touch={touch} vertical={vertical} delta={delta}: painted grab {actual:?}, pointer {expected:?}"
                        );
                    }
                    // Release contains a final displacement without an intervening move.
                    let expected_offset = reference_offset + px(12.) * offset_per_pixel;
                    if touch {
                        cx.simulate_event(TouchDragEvent {
                            phase: TouchPhase::Ended,
                            start_position: start,
                            position: start + axis_delta(12.),
                        });
                    } else {
                        cx.simulate_mouse_up(
                            start + axis_delta(12.),
                            MouseButton::Left,
                            Modifiers::default(),
                        );
                    }
                    assert!(
                        (axis_value(handle.offset()) - expected_offset).abs() < px(0.1),
                        "release must flush the last displacement"
                    );
                    let released_offset = handle.offset();
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    assert_eq!(
                        handle.offset(),
                        released_offset,
                        "leaving active styling must not scroll"
                    );
                }
            }
        }
    }

    #[gpui::test]
    fn touch_thumb_drag_moves_down_and_cancel_releases_handle(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Always,
            size(px(100.), px(500.)),
        );
        let start_position = point(px(95.), px(20.));
        for (phase, position) in [
            (TouchPhase::Started, start_position),
            (TouchPhase::Moved, point(px(95.), px(45.))),
            (TouchPhase::Cancelled, point(px(95.), px(45.))),
        ] {
            cx.simulate_event(TouchDragEvent {
                phase,
                start_position,
                position,
            });
        }
        assert!(
            handle.offset().y < px(0.),
            "thumb down must scroll toward later content"
        );
        assert_eq!(handle.offset().x, px(0.));
        assert_eq!(handle.drag_starts.get(), 1);
        assert_eq!(handle.drag_ends.get(), 1);
    }

    #[gpui::test]
    fn thumb_drag_notifies_handle_start_and_end(cx: &mut TestAppContext) {
        let (cx, handle) = harness(
            cx,
            ScrollbarAxis::Vertical,
            ScrollbarMode::Always,
            size(px(100.), px(500.)),
        );
        cx.simulate_mouse_down(
            point(px(95.), px(20.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            point(px(95.), px(70.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(px(95.), px(70.)),
            MouseButton::Left,
            Modifiers::default(),
        );

        assert_eq!(handle.drag_starts.get(), 1);
        assert_eq!(handle.drag_ends.get(), 1);
    }

    /// The height of row `ix` of the list below, from 20px to 578px in no
    /// particular order.
    fn mixed_row_height(ix: usize) -> f32 {
        20. + ((ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 59) as f32 * 18.
    }

    /// A list of 300 rows of mixed heights, 200px tall, with its scrollbar.
    struct ListHarness {
        state: ListState,
    }

    impl Render for ListHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size(px(200.))
                .child(
                    gpui::list(self.state.clone(), |ix, _, _| {
                        div()
                            .h(px(mixed_row_height(ix)))
                            .w_full()
                            .into_any_element()
                    })
                    .size_full(),
                )
                .child(Scrollbar::vertical(&self.state).mode(ScrollbarMode::Always))
        }
    }

    #[gpui::test]
    fn a_lists_thumb_follows_the_pointer_over_rows_not_measured_yet(cx: &mut TestAppContext) {
        let state = ListState::new(300, gpui::ListAlignment::Top, px(50.));
        state.set_follow_mode(gpui::FollowMode::Tail);
        let (_, cx) = cx.add_window_view({
            let state = state.clone();
            move |_, _| ListHarness { state }
        });
        let frame = |cx: &mut VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
        frame(cx);
        let offset = |state: &ListState| -state.scroll_px_offset_for_scrollbar().y;
        let drag = |cx: &mut VisualTestContext, y: f32| {
            cx.simulate_mouse_move(
                point(px(195.), px(y)),
                Some(MouseButton::Left),
                Modifiers::default(),
            );
            frame(cx);
        };

        // The thumb sits at the bottom of the track, at least 48px long.
        let max = state.max_offset_for_scrollbar().y;
        assert_eq!(offset(&state), max);
        let grab = 190.;
        cx.simulate_mouse_down(
            point(px(195.), px(grab)),
            MouseButton::Left,
            Modifiers::default(),
        );
        frame(cx);

        // Up the track a few pixels a frame, while the list measures the rows
        // it scrolls to. The scroll range holds, and each pixel of the track
        // stays the same distance down the list, so the thumb stays under the
        // pointer.
        let mut per_px = None;
        for step in 1..=20 {
            let y = grab - step as f32 * 6.;
            drag(cx, y);
            assert_eq!(state.max_offset_for_scrollbar().y, max);
            let moved = f32::from(max - offset(&state)) / (grab - y);
            let first = *per_px.get_or_insert(moved);
            assert!((moved - first).abs() < 0.01, "{moved} against {first}");
        }

        // Past the top of the track, the list shows its first row.
        drag(cx, -40.);
        cx.simulate_mouse_up(
            point(px(195.), px(-40.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        frame(cx);
        let top = state.logical_scroll_top();
        assert_eq!((top.item_ix, top.offset_in_item), (0, px(0.)));
        assert_eq!(offset(&state), px(0.));

        // Past the bottom of the track, its last row, at the list's bottom.
        cx.simulate_mouse_down(
            point(px(195.), px(10.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        frame(cx);
        for y in [60., 120., 180., 240.] {
            drag(cx, y);
        }
        cx.simulate_mouse_up(
            point(px(195.), px(240.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        frame(cx);
        let last = state.bounds_for_item(299).expect("the last row shows");
        assert_eq!(last.bottom(), state.viewport_bounds().bottom());
        assert_eq!(offset(&state), state.max_offset_for_scrollbar().y);
    }
}
