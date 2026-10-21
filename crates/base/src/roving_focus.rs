//! Keyboard focus for composite controls whose items share one Tab stop.

use std::{cell::RefCell, rc::Rc};

use gpui::{App, ElementId, FocusHandle, ScrollHandle, Window};

/// Where an arrow key moves focus among the items of a [`RovingFocus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RovingStep {
    /// The previous enabled item. From the first item, the last.
    Previous,
    /// The next enabled item. From the last item, the first.
    Next,
    /// The first enabled item.
    First,
    /// The last enabled item.
    Last,
}

/// Keyboard focus for a composite control, such as a tab list or a sidebar
/// menu, whose items share one Tab stop.
///
/// The control keeps one focus handle per item. Tab and Shift-Tab reach the
/// control on a single item, the current one (a selected tab, an active menu
/// item) or else the first enabled item, and the arrow keys move focus between
/// its items. A focused item that has a click handler activates with Enter or
/// Space, as a focused [`crate::Button`] does. Disabled items are skipped.
///
/// The owner renders the control with these steps.
///
/// 1. [`RovingFocus::keyed`] gives the same state on every frame.
/// 2. [`RovingFocus::set_items`] says how many items there are, which are
///    enabled and which one is current.
/// 3. Each item tracks [`RovingFocus::handle`] for its index.
/// 4. The control's key handlers call [`RovingFocus::move_focus`], and let
///    the key go on to their parent when it returns `None`.
///
/// [`crate::Tabs::roving_focus`] does step 4 for a tab list.
#[derive(Clone, Default)]
pub struct RovingFocus {
    state: Rc<RefCell<RovingFocusState>>,
}

#[derive(Default)]
struct RovingFocusState {
    handles: Vec<FocusHandle>,
    enabled: Vec<bool>,
    tab_stop: Option<usize>,
    scroll_handle: Option<ScrollHandle>,
}

impl RovingFocus {
    /// The roving focus kept for `id` in the window's element state.
    ///
    /// Call it while rendering, with an id that is stable across frames, such
    /// as one derived from the control's own element id.
    pub fn keyed(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Self {
        window
            .use_keyed_state(id.into(), cx, |_, _| Self::default())
            .read(cx)
            .clone()
    }

    /// Sets the control's items for this frame, with one `bool` per item
    /// (`true` for an item that can take focus) and the index of the current
    /// item.
    ///
    /// Focus handles are kept for items that stay, so focus stays on an item
    /// across frames. The current item is the Tab stop if it's enabled.
    /// Otherwise the first enabled item is.
    pub fn set_items(
        &self,
        enabled: impl IntoIterator<Item = bool>,
        current: Option<usize>,
        cx: &mut App,
    ) {
        let mut state = self.state.borrow_mut();
        state.enabled.clear();
        state.enabled.extend(enabled);
        let len = state.enabled.len();
        state.handles.truncate(len);
        while state.handles.len() < len {
            state.handles.push(cx.focus_handle());
        }
        state.tab_stop = current
            .filter(|&ix| state.enabled.get(ix).copied().unwrap_or(false))
            .or_else(|| state.enabled.iter().position(|&enabled| enabled));
    }

    /// Reveals the item that takes focus in `scroll_handle`, whose scroll
    /// container holds one child per item, in item order.
    pub fn track_scroll(&self, scroll_handle: &ScrollHandle) {
        self.state.borrow_mut().scroll_handle = Some(scroll_handle.clone());
    }

    /// The number of items given to [`RovingFocus::set_items`].
    pub fn len(&self) -> usize {
        self.state.borrow().enabled.len()
    }

    /// Whether the control has no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The focus handle for item `ix`, a Tab stop only for the item Tab
    /// reaches. `None` past the last item.
    pub fn handle(&self, ix: usize) -> Option<FocusHandle> {
        let state = self.state.borrow();
        let tab_stop = state.tab_stop == Some(ix);
        state
            .handles
            .get(ix)
            .map(|handle| handle.clone().tab_stop(tab_stop))
    }

    /// The index of the item Tab reaches, if any item is enabled.
    pub fn tab_stop_index(&self) -> Option<usize> {
        self.state.borrow().tab_stop
    }

    /// The index of the item that has focus.
    pub fn focused_index(&self, window: &Window) -> Option<usize> {
        self.state
            .borrow()
            .handles
            .iter()
            .position(|handle| handle.is_focused(window))
    }

    /// Whether one of the items has focus.
    pub fn contains_focused(&self, window: &Window) -> bool {
        self.focused_index(window).is_some()
    }

    /// Moves focus to item `ix`, if it's enabled.
    pub fn focus(&self, ix: usize, window: &mut Window, cx: &mut App) -> bool {
        let (handle, scroll_handle) = {
            let state = self.state.borrow();
            if !state.enabled.get(ix).copied().unwrap_or(false) {
                return false;
            }
            (state.handles[ix].clone(), state.scroll_handle.clone())
        };
        window.focus(&handle, cx);
        if let Some(scroll_handle) = scroll_handle {
            scroll_handle.scroll_to_item(ix);
        }
        true
    }

    /// Moves focus from the focused item by `step` and returns the item that
    /// has focus afterwards. Returns `None`, and leaves focus alone, when no
    /// item has focus, so the key can go on to the control's parent.
    pub fn move_focus(&self, step: RovingStep, window: &mut Window, cx: &mut App) -> Option<usize> {
        let from = self.focused_index(window)?;
        let target = step_target(&self.state.borrow().enabled, Some(from), step)?;
        if target != from {
            self.focus(target, window, cx);
        }
        Some(target)
    }
}

/// The enabled item `step` lands on from `from`.
fn step_target(enabled: &[bool], from: Option<usize>, step: RovingStep) -> Option<usize> {
    let len = enabled.len();
    let is_enabled = |ix: &usize| enabled[*ix];
    match step {
        RovingStep::First => (0..len).find(is_enabled),
        RovingStep::Last => (0..len).rev().find(is_enabled),
        RovingStep::Next => {
            let start = from.map_or(0, |from| from + 1);
            (0..len)
                .map(|offset| (start + offset) % len)
                .find(is_enabled)
        }
        RovingStep::Previous => {
            let start = from.unwrap_or(0) + len;
            (1..=len)
                .map(|offset| (start - offset) % len)
                .find(is_enabled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_skip_disabled_items_and_wrap() {
        let enabled = [true, false, true, true, false];

        assert_eq!(step_target(&enabled, Some(0), RovingStep::Next), Some(2));
        assert_eq!(step_target(&enabled, Some(3), RovingStep::Next), Some(0));
        assert_eq!(
            step_target(&enabled, Some(2), RovingStep::Previous),
            Some(0)
        );
        assert_eq!(
            step_target(&enabled, Some(0), RovingStep::Previous),
            Some(3)
        );
        assert_eq!(step_target(&enabled, Some(2), RovingStep::First), Some(0));
        assert_eq!(step_target(&enabled, Some(0), RovingStep::Last), Some(3));
    }

    #[test]
    fn a_single_enabled_item_keeps_focus() {
        let enabled = [false, true, false];

        assert_eq!(step_target(&enabled, Some(1), RovingStep::Next), Some(1));
        assert_eq!(
            step_target(&enabled, Some(1), RovingStep::Previous),
            Some(1)
        );
    }

    #[test]
    fn no_enabled_item_has_no_target() {
        assert_eq!(step_target(&[], None, RovingStep::Next), None);
        assert_eq!(
            step_target(&[false, false], Some(0), RovingStep::Last),
            None
        );
    }

    #[gpui::test]
    fn the_current_item_is_the_tab_stop_unless_disabled(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let focus = RovingFocus::default();
            focus.set_items([true, true, true], Some(1), cx);
            assert_eq!(focus.tab_stop_index(), Some(1));
            assert!(focus.handle(1).unwrap().tab_stop);
            assert!(!focus.handle(0).unwrap().tab_stop);

            focus.set_items([false, true, false], Some(2), cx);
            assert_eq!(focus.tab_stop_index(), Some(1));

            focus.set_items([false, false], None, cx);
            assert_eq!(focus.tab_stop_index(), None);
            assert!(focus.handle(2).is_none());
        });
    }

    #[gpui::test]
    fn items_that_stay_keep_their_focus_handles(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let focus = RovingFocus::default();
            focus.set_items([true, true], None, cx);
            let first = focus.handle(0).unwrap();
            focus.set_items([true, true, true], None, cx);
            assert_eq!(focus.handle(0).unwrap(), first);
            assert_eq!(focus.len(), 3);
        });
    }
}
