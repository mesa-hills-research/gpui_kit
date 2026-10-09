//! Wheel scrolling over a scroll area that sits inside another one.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    DispatchPhase, HitboxBehavior, IntoElement, Pixels, Point, ScrollWheelEvent, Styled as _,
    TouchPhase, canvas, point, px,
};

use super::ScrollbarHandle;

/// A pause between wheel events this long starts a new gesture. Browsers keep
/// a wheel gesture with the scroll area it started in for about as long.
const GESTURE_GAP: Duration = Duration::from_millis(500);

/// What [`nested_scroll`] remembers between wheel events. Keep one per scroll
/// area, for as long as the area is shown.
#[derive(Clone, Default)]
pub(crate) struct NestedScroll(Rc<RefCell<NestedScrollState>>);

#[derive(Default)]
struct NestedScrollState {
    /// The area's offset before it handled the event being dispatched.
    before: Option<Point<Pixels>>,
    /// When the last wheel event over the area arrived.
    last_event: Option<Instant>,
    /// Whether the area moved during the current gesture.
    moved_in_gesture: bool,
}

/// Keeps wheel and trackpad scrolling over a scroll area from moving the
/// scroll areas around it, such as a list inside a scrolling page.
///
/// GPUI gives each wheel event to every scroll area under the pointer, so
/// without this the page moves together with the list. Place the element
/// before the scroll area, inside the same parent and over the same bounds:
/// it reads the area's offset before the area handles an event, and once the
/// area has moved, the event goes no further.
///
/// At the area's end the event goes on to the page, unless the area moved
/// earlier in the same gesture. The gesture stays with the list, and the page
/// takes over with the next one, the way a browser chains scrolling.
pub(crate) fn nested_scroll<H: ScrollbarHandle + Clone>(
    state: &NestedScroll,
    handle: &H,
) -> impl IntoElement {
    let state = state.0.clone();
    let handle = handle.clone();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                let mut state = state.borrow_mut();
                if !hitbox.should_handle_scroll(window) {
                    // The pointer is elsewhere, so the area's gesture is over.
                    state.last_event = None;
                    state.moved_in_gesture = false;
                    return;
                }
                match phase {
                    DispatchPhase::Capture => state.before = Some(clamped_offset(&handle)),
                    DispatchPhase::Bubble => {
                        let Some(before) = state.before.take() else {
                            return;
                        };
                        let now = cx.background_executor().now();
                        let new_gesture = event.touch_phase == TouchPhase::Started
                            || state
                                .last_event
                                .is_none_or(|at| now.saturating_duration_since(at) >= GESTURE_GAP);
                        if new_gesture {
                            state.moved_in_gesture = false;
                        }
                        state.last_event = Some(now);
                        if clamped_offset(&handle) != before {
                            state.moved_in_gesture = true;
                        }
                        if state.moved_in_gesture {
                            cx.stop_propagation();
                        }
                    }
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

/// The handle's offset within its range. A div adds a wheel delta to its
/// offset straight away and clamps it when it is next laid out, so a delta
/// at the end would otherwise look like movement.
fn clamped_offset(handle: &impl ScrollbarHandle) -> Point<Pixels> {
    let content = handle.content_size();
    let viewport = handle.viewport_bounds().size;
    let max_x = (content.width - viewport.width).max(px(0.));
    let max_y = (content.height - viewport.height).max(px(0.));
    let offset = handle.offset();
    point(
        offset.x.clamp(-max_x, px(0.)),
        offset.y.clamp(-max_y, px(0.)),
    )
}
