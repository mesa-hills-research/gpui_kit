use crate::{
    ActiveTheme as _, Collapsible, Icon, IconName, Placement, Sizable as _, StyledExt,
    actions::{SelectDown, SelectFirst, SelectLast, SelectLeft, SelectRight, SelectUp},
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt, PopupMenu},
    sidebar::SidebarItem,
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Entity, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, canvas, div, percentage,
    prelude::FluentBuilder,
};
use gpui_base::{RovingFocus, RovingStep, TestSupportExt as _};
use std::rc::Rc;

const CONTEXT: &str = "SidebarMenu";

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
    ]);
}

/// The element id of the `ix`th item under `parent`.
fn item_id(parent: &ElementId, ix: usize) -> ElementId {
    SharedString::from(format!("{}-{}", parent, ix)).into()
}

/// A visible item of a [`SidebarMenu`], in the order the keyboard moves
/// through them, where an item's open submenu follows it.
struct MenuEntry {
    parent: Option<usize>,
    depth: usize,
    enabled: bool,
    active: bool,
    /// The open state of an item whose submenu can open.
    open_state: Option<Entity<bool>>,
    is_open: bool,
}

/// An item's place in the keyboard order of the menu that renders it.
#[derive(Clone)]
struct MenuItemFocus {
    focus: RovingFocus,
    ix: usize,
}

/// Gives every visible item of `items` its place in the keyboard order and
/// its submenu's open state, and lists them in `entries`.
#[allow(clippy::too_many_arguments)]
fn prepare_items(
    items: &mut [SidebarMenuItem],
    parent_id: &ElementId,
    parent: Option<usize>,
    depth: usize,
    collapsed: bool,
    focus: &RovingFocus,
    entries: &mut Vec<MenuEntry>,
    window: &mut Window,
    cx: &mut App,
) {
    for (ix, item) in items.iter_mut().enumerate() {
        let id = item_id(parent_id, ix);
        let flat_ix = entries.len();
        let open_state = item.is_submenu().then(|| {
            let default_open = item.default_open;
            window.use_keyed_state(id.clone(), cx, move |_, _| default_open)
        });
        // A collapsed sidebar shows no submenus.
        let can_open = !collapsed && open_state.is_some();
        let is_open = can_open && open_state.as_ref().is_some_and(|state| *state.read(cx));
        entries.push(MenuEntry {
            parent,
            depth,
            enabled: !item.disabled,
            active: item.active,
            open_state: can_open.then(|| open_state.clone()).flatten(),
            is_open,
        });
        item.focus = Some(MenuItemFocus {
            focus: focus.clone(),
            ix: flat_ix,
        });
        item.open_state = open_state;
        if is_open {
            prepare_items(
                &mut item.children,
                &id,
                Some(flat_ix),
                depth + 1,
                collapsed,
                focus,
                entries,
                window,
                cx,
            );
        }
    }
}

/// Opens the focused item's submenu, or moves into an open one.
fn expand(focus: &RovingFocus, entries: &[MenuEntry], window: &mut Window, cx: &mut App) -> bool {
    let Some(ix) = focus.focused_index(window) else {
        return false;
    };
    let Some(entry) = entries.get(ix) else {
        return false;
    };
    let Some(open_state) = entry.open_state.as_ref() else {
        return false;
    };
    if !entry.is_open {
        open_state.update(cx, |is_open, cx| {
            *is_open = true;
            cx.notify();
        });
        return true;
    }
    let first_child = entries[ix + 1..]
        .iter()
        .take_while(|child| child.depth > entry.depth)
        .position(|child| child.parent == Some(ix) && child.enabled);
    if let Some(offset) = first_child {
        focus.focus(ix + 1 + offset, window, cx);
    }
    true
}

/// Closes the focused item's open submenu, or moves to its parent item.
fn collapse(focus: &RovingFocus, entries: &[MenuEntry], window: &mut Window, cx: &mut App) -> bool {
    let Some(ix) = focus.focused_index(window) else {
        return false;
    };
    let Some(entry) = entries.get(ix) else {
        return false;
    };
    if let (true, Some(open_state)) = (entry.is_open, entry.open_state.as_ref()) {
        open_state.update(cx, |is_open, cx| {
            *is_open = false;
            cx.notify();
        });
        return true;
    }
    match entry.parent {
        Some(parent) => focus.focus(parent, window, cx),
        None => false,
    }
}

/// Menu for the [`super::Sidebar`]
#[derive(Clone)]
pub struct SidebarMenu {
    style: StyleRefinement,
    collapsed: bool,
    items: Vec<SidebarMenuItem>,
    scroll_handle: Option<ScrollHandle>,
}

impl SidebarMenu {
    /// Create a new SidebarMenu
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            items: Vec::new(),
            collapsed: false,
            scroll_handle: None,
        }
    }

    /// Scroll the item that takes focus from the keyboard into view in the
    /// scroll container that `scroll_handle` tracks.
    ///
    /// A menu in a [`super::Sidebar`] does this without it. Pass the handle
    /// of the container when the menu is used on its own in a scrolling
    /// element of the application. The menu may sit anywhere inside the
    /// container.
    pub fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(scroll_handle.clone());
        self
    }

    /// Add a [`SidebarMenuItem`] child menu item to the sidebar menu.
    ///
    /// See also [`SidebarMenu::children`].
    pub fn child(mut self, child: impl Into<SidebarMenuItem>) -> Self {
        self.items.push(child.into());
        self
    }

    /// Add multiple [`SidebarMenuItem`] child menu items to the sidebar menu.
    pub fn children(
        mut self,
        children: impl IntoIterator<Item = impl Into<SidebarMenuItem>>,
    ) -> Self {
        self.items = children.into_iter().map(Into::into).collect();
        self
    }
}

impl Collapsible for SidebarMenu {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl SidebarItem for SidebarMenu {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let id = id.into();
        let collapsed = self.collapsed;
        let mut items: Vec<_> = self
            .items
            .into_iter()
            .map(|item| item.collapsed(collapsed))
            .collect();

        // The menu is one Tab stop, on its active item.
        let focus = RovingFocus::keyed(SharedString::from(format!("{id}-focus")), window, cx);
        // The item that takes focus scrolls into view, in the application's
        // container or in the sidebar.
        if let Some(scroll_handle) = &self.scroll_handle {
            focus.track_container_scroll(scroll_handle);
        } else if let Some(list_state) = super::current_list() {
            focus.track_list_scroll(&list_state);
        }
        let mut entries = Vec::new();
        prepare_items(
            &mut items,
            &id,
            None,
            0,
            collapsed,
            &focus,
            &mut entries,
            window,
            cx,
        );
        focus.set_items(
            entries.iter().map(|entry| entry.enabled),
            entries.iter().position(|entry| entry.active),
            cx,
        );
        let entries: Rc<[MenuEntry]> = entries.into();
        let step = |step: RovingStep| {
            let focus = focus.clone();
            move |window: &mut Window, cx: &mut App| {
                if focus.move_focus(step, window, cx).is_none() {
                    cx.propagate();
                }
            }
        };
        let (previous, next) = (step(RovingStep::Previous), step(RovingStep::Next));
        let (first, last) = (step(RovingStep::First), step(RovingStep::Last));

        v_flex()
            .gap_2()
            .key_context(CONTEXT)
            .on_action(move |_: &SelectUp, window, cx| previous(window, cx))
            .on_action(move |_: &SelectDown, window, cx| next(window, cx))
            .on_action(move |_: &SelectFirst, window, cx| first(window, cx))
            .on_action(move |_: &SelectLast, window, cx| last(window, cx))
            .on_action({
                let focus = focus.clone();
                let entries = entries.clone();
                move |_: &SelectRight, window, cx| {
                    if !expand(&focus, &entries, window, cx) {
                        cx.propagate();
                    }
                }
            })
            .on_action(move |_: &SelectLeft, window, cx| {
                if !collapse(&focus, &entries, window, cx) {
                    cx.propagate();
                }
            })
            .refine_style(&self.style)
            .children(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(ix, item)| item.render(item_id(&id, ix), window, cx).into_any_element()),
            )
    }
}

impl Styled for SidebarMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Menu item for the [`SidebarMenu`]
#[derive(Clone)]
pub struct SidebarMenuItem {
    icon: Option<Icon>,
    label: SharedString,
    accessibility_label: Option<SharedString>,
    label_style: StyleRefinement,
    style: StyleRefinement,
    handler: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    active: bool,
    default_open: bool,
    click_to_open: bool,
    collapsed: bool,
    click_to_toggle: bool,
    children: Vec<Self>,
    suffix: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    disabled: bool,
    context_menu: Option<Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static>>,
    /// Set by the [`SidebarMenu`] that renders the item.
    focus: Option<MenuItemFocus>,
    open_state: Option<Entity<bool>>,
}

impl SidebarMenuItem {
    /// Create a new [`SidebarMenuItem`] with a label.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            label: label.into(),
            accessibility_label: None,
            label_style: StyleRefinement::default(),
            style: StyleRefinement::default(),
            handler: Rc::new(|_, _, _| {}),
            active: false,
            collapsed: false,
            default_open: false,
            click_to_open: false,
            click_to_toggle: false,
            children: Vec::new(),
            suffix: None,
            disabled: false,
            context_menu: None,
            focus: None,
            open_state: None,
        }
    }

    /// Set the icon for the menu item
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set the style for the label
    pub fn label_style(mut self, style: StyleRefinement) -> Self {
        self.label_style = style;
        self
    }

    /// Set the accessibility label of the item row.
    ///
    /// Defaults to the visible label text.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// Set the active state of the menu item
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Add a click handler to the menu item
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.handler = Rc::new(handler);
        self
    }

    /// Set the collapsed state of the menu item
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Set the default open state of the Submenu, default is `false`.
    ///
    /// This only used on initial render, the internal state will be used afterwards.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Set whether clicking the menu item open the submenu.
    ///
    /// Default is `false`.
    ///
    /// If `false` we only handle open/close via the caret button.
    pub fn click_to_open(mut self, click_to_open: bool) -> Self {
        self.click_to_open = click_to_open;
        self
    }

    /// Set whether clicking the menu item toggles the submenu.
    ///
    /// If click_to_open is `true`, this has no effect.
    ///
    /// Default is `false`.
    pub fn click_to_toggle(mut self, click_to_toggle: bool) -> Self {
        self.click_to_toggle = click_to_toggle;
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl Into<Self>>) -> Self {
        self.children = children.into_iter().map(Into::into).collect();
        self
    }

    /// Set the suffix for the menu item.
    pub fn suffix<F, E>(mut self, builder: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.suffix = Some(Rc::new(move |window, cx| {
            builder(window, cx).into_any_element()
        }));
        self
    }

    /// Set disabled flat for menu item.
    pub fn disable(mut self, disable: bool) -> Self {
        self.disabled = disable;
        self
    }

    fn is_submenu(&self) -> bool {
        self.children.len() > 0
    }

    fn collapsed_tooltip(&self) -> Option<SharedString> {
        (self.collapsed && self.icon.is_some()).then(|| self.label.clone())
    }

    /// Set the context menu for the menu item.
    pub fn context_menu(
        mut self,
        f: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.context_menu = Some(Rc::new(f));
        self
    }
}

impl FluentBuilder for SidebarMenuItem {}

impl Collapsible for SidebarMenuItem {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl SidebarItem for SidebarMenuItem {
    fn render(
        mut self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let click_to_open = self.click_to_open;
        let click_to_toggle = self.click_to_toggle;
        let default_open = self.default_open;
        let collapsed_tooltip = self.collapsed_tooltip();
        let id = id.into();
        let label_text = self.label.clone();
        let accessibility_label = self.accessibility_label.clone();
        let is_submenu = self.is_submenu();
        let open_state = match self.open_state.take() {
            Some(open_state) => Some(open_state),
            None if is_submenu => Some(window.use_keyed_state(id.clone(), cx, |_, _| default_open)),
            None => None,
        };
        let handler = self.handler.clone();
        let is_collapsed = self.collapsed;
        let is_active = self.active;
        let is_hoverable = !is_active && !self.disabled;
        let is_disabled = self.disabled;
        let is_open = open_state
            .as_ref()
            .map_or(false, |s| !is_collapsed && *s.read(cx));
        let focus_handle = self
            .focus
            .as_ref()
            .filter(|_| !is_disabled)
            .and_then(|item| item.focus.handle(item.ix));
        // Pressing an item focuses it too, but only the keyboard shows a ring.
        let focus_visible = window.last_input_was_keyboard()
            && focus_handle
                .as_ref()
                .is_some_and(|focus_handle| focus_handle.is_focused(window));
        let item_focus = self.focus.clone().filter(|_| !is_disabled);

        div()
            .id(id.clone())
            .test_support()
            .w_full()
            .child(
                h_flex()
                    .size_full()
                    .id("item")
                    .role(Role::TreeItem)
                    .aria_label(accessibility_label.unwrap_or_else(|| label_text.clone()))
                    .aria_selected(is_active)
                    .when(is_submenu && !is_collapsed, |this| {
                        this.aria_expanded(is_open)
                    })
                    .when_some(focus_handle, |this, focus_handle| {
                        this.track_focus(&focus_handle)
                    })
                    .overflow_x_hidden()
                    .flex_shrink_0()
                    .p_2()
                    .gap_x_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .refine_style(&self.style)
                    .when(is_hoverable, |this| {
                        this.hover(|this| {
                            this.bg(cx.theme().sidebar_accent.opacity(0.8))
                                .text_color(cx.theme().sidebar_accent_foreground)
                        })
                    })
                    .when(is_active, |this| {
                        this.font_medium()
                            .bg(cx.theme().tokens.sidebar_accent)
                            .text_color(cx.theme().sidebar_accent_foreground)
                    })
                    .when_some(self.icon.clone(), |this, icon| this.child(icon))
                    .when(is_collapsed, |this| {
                        this.justify_center().when(is_active, |this| {
                            this.bg(cx.theme().tokens.sidebar_accent)
                                .text_color(cx.theme().sidebar_accent_foreground)
                        })
                    })
                    .when(!is_collapsed, |this| {
                        this.h_7()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .gap_x_2()
                                    .justify_between()
                                    .overflow_x_hidden()
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .overflow_x_hidden()
                                            .refine_style(&self.label_style)
                                            .child(self.label.clone()),
                                    )
                                    .when_some(self.suffix.clone(), |this, suffix| {
                                        this.child(suffix(window, cx).into_any_element())
                                    }),
                            )
                            .when_some(open_state.clone(), |this, open_state| {
                                this.child(
                                    Button::new("caret")
                                        .xsmall()
                                        .ghost()
                                        .icon(
                                            Icon::new(IconName::ChevronRight)
                                                .size_4()
                                                .when(is_open, |this| {
                                                    this.rotate(percentage(90. / 360.))
                                                }),
                                        )
                                        .on_click({
                                            move |_, _, cx| {
                                                // Avoid trigger item click, just expand/collapse submenu
                                                cx.stop_propagation();
                                                open_state.update(cx, |is_open, cx| {
                                                    *is_open = !*is_open;
                                                    cx.notify();
                                                })
                                            }
                                        }),
                                )
                            })
                    })
                    .when(is_disabled, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    // Tells the menu where the row is, to scroll it into view
                    // when it takes focus.
                    .when_some(item_focus, |this, item| {
                        this.child(
                            canvas(
                                move |bounds, window, cx| {
                                    item.focus.item_prepainted(item.ix, bounds, window, cx)
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                    })
                    // Inside the row, which clips what's outside it.
                    .when(focus_visible, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(cx.theme().radius)
                                .border_2()
                                .border_color(cx.theme().ring),
                        )
                    })
                    .when(!is_disabled, |this| {
                        this.on_click({
                            let open_state = open_state.clone();
                            move |ev, window, cx| {
                                if click_to_open {
                                    if let Some(ref s) = open_state {
                                        s.update(cx, |is_open: &mut bool, cx| {
                                            *is_open = true;
                                            cx.notify();
                                        });
                                    }
                                } else if click_to_toggle {
                                    if let Some(ref s) = open_state {
                                        s.update(cx, |is_open: &mut bool, cx| {
                                            *is_open = !*is_open;
                                            cx.notify();
                                        });
                                    }
                                }
                                handler(ev, window, cx)
                            }
                        })
                    })
                    .map(|this| {
                        if let Some(tooltip) = collapsed_tooltip {
                            this.managed_tooltip_at(Placement::Right, move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                        } else {
                            this
                        }
                    })
                    .map(|this| {
                        if let Some(context_menu) = self.context_menu {
                            this.context_menu(move |menu, window, cx| {
                                context_menu(menu, window, cx)
                            })
                            .into_any_element()
                        } else {
                            this.into_any_element()
                        }
                    }),
            )
            .when(is_open, |this| {
                this.child(
                    v_flex()
                        .id("submenu")
                        .border_l_1()
                        .border_color(cx.theme().sidebar_border)
                        .gap_1()
                        .ml_3p5()
                        .pl_2p5()
                        .py_0p5()
                        .children(self.children.into_iter().enumerate().map(|(ix, item)| {
                            item.render(item_id(&id, ix), window, cx).into_any_element()
                        })),
                )
            })
    }
}

impl Styled for SidebarMenuItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_icon_item_uses_label_as_tooltip() {
        let item = SidebarMenuItem::new("Projects")
            .icon(Icon::default())
            .collapsed(true);

        assert_eq!(item.collapsed_tooltip().as_deref(), Some("Projects"));
    }

    #[test]
    fn expanded_or_iconless_item_has_no_collapsed_tooltip() {
        let expanded = SidebarMenuItem::new("Projects").icon(Icon::default());
        let iconless = SidebarMenuItem::new("Projects").collapsed(true);

        assert!(expanded.collapsed_tooltip().is_none());
        assert!(iconless.collapsed_tooltip().is_none());
    }

    mod keyboard {
        use super::*;
        use gpui::{Context, KeyDownEvent, KeyUpEvent, Keystroke, Render, TestAppContext};
        use std::cell::RefCell;

        type Clicks = Rc<RefCell<Vec<&'static str>>>;

        /// Inbox (active), Projects with Alpha and Beta, Archive (disabled), Trash.
        struct MenuHarness {
            clicks: Clicks,
        }

        fn item(label: &'static str, clicks: &Clicks) -> SidebarMenuItem {
            let clicks = clicks.clone();
            SidebarMenuItem::new(label).on_click(move |_, _, _| clicks.borrow_mut().push(label))
        }

        impl Render for MenuHarness {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let clicks = &self.clicks;
                div().w(gpui::px(240.)).child(
                    SidebarMenu::new()
                        .child(item("Inbox", clicks).active(true))
                        .child(
                            item("Projects", clicks)
                                .children([item("Alpha", clicks), item("Beta", clicks)]),
                        )
                        .child(item("Archive", clicks).disable(true))
                        .child(item("Trash", clicks))
                        .render("menu", window, cx),
                )
            }
        }

        fn harness(cx: &mut TestAppContext) -> (&mut gpui::VisualTestContext, Clicks) {
            cx.update(crate::init);
            let clicks = Clicks::default();
            let (_, cx) = cx.add_window_view({
                let clicks = clicks.clone();
                move |_, _| MenuHarness { clicks }
            });
            draw(cx);
            cx.update(|window, cx| window.focus_next(cx));
            (cx, clicks)
        }

        fn draw(cx: &mut gpui::VisualTestContext) {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        /// Presses Enter on the focused item and returns what it clicked.
        fn activate(cx: &mut gpui::VisualTestContext, clicks: &Clicks) -> Option<&'static str> {
            let keystroke = Keystroke::parse("enter").unwrap();
            cx.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(KeyUpEvent { keystroke });
            clicks.borrow_mut().pop()
        }

        #[gpui::test]
        fn tab_reaches_the_active_item_and_arrows_skip_disabled_items(cx: &mut TestAppContext) {
            let (cx, clicks) = harness(cx);
            assert_eq!(activate(cx, &clicks), Some("Inbox"));

            cx.simulate_keystrokes("down");
            assert_eq!(activate(cx, &clicks), Some("Projects"));
            cx.simulate_keystrokes("down");
            assert_eq!(activate(cx, &clicks), Some("Trash"), "Archive is disabled");
            cx.simulate_keystrokes("down");
            assert_eq!(activate(cx, &clicks), Some("Inbox"), "focus wraps");
            cx.simulate_keystrokes("up");
            assert_eq!(activate(cx, &clicks), Some("Trash"));
            cx.simulate_keystrokes("home");
            assert_eq!(activate(cx, &clicks), Some("Inbox"));
            cx.simulate_keystrokes("end");
            assert_eq!(activate(cx, &clicks), Some("Trash"));
        }

        #[gpui::test]
        fn right_and_left_open_enter_leave_and_close_a_submenu(cx: &mut TestAppContext) {
            let (cx, clicks) = harness(cx);
            cx.simulate_keystrokes("down right");
            draw(cx);
            cx.simulate_keystrokes("down");
            assert_eq!(
                activate(cx, &clicks),
                Some("Alpha"),
                "Right opened Projects"
            );

            cx.simulate_keystrokes("up right");
            assert_eq!(activate(cx, &clicks), Some("Alpha"), "Right moved into it");
            cx.simulate_keystrokes("down");
            assert_eq!(activate(cx, &clicks), Some("Beta"));

            cx.simulate_keystrokes("left");
            assert_eq!(activate(cx, &clicks), Some("Projects"), "Left moved out");
            cx.simulate_keystrokes("left");
            draw(cx);
            cx.simulate_keystrokes("down");
            assert_eq!(activate(cx, &clicks), Some("Trash"), "Left closed Projects");
        }
    }
}
