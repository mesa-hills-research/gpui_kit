use gpui_base::TestSupportExt as _;

use crate::{
    InteractiveElementExt as _, Selectable, Sizable,
    actions::{Cancel, Confirm, SelectDown, SelectLeft, SelectRight},
    button::{Button, ButtonVariants},
    global_state::GlobalState,
    h_flex,
    menu::PopupMenu,
};
use gpui::{
    App, AppContext as _, ClickEvent, Context, DismissEvent, Entity, FocusHandle, Focusable,
    HighlightStyle, InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, MouseButton,
    OwnedMenu, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    StyledText, Subscription, UnderlineStyle, Window, actions, anchored, canvas, deferred, div,
    prelude::FluentBuilder, px,
};

const CONTEXT: &str = "AppMenuBar";

actions!(
    app_menu_bar,
    [
        /// Move keyboard focus to the window's menu bar, on its first menu,
        /// or back to where it was. Bound to F10 on Windows and Linux.
        FocusAppMenuBar,
        /// A tap of Alt alone. It does what [`FocusAppMenuBar`] does unless
        /// the pointer was pressed or scrolled while Alt was down, or the
        /// window lost focus meanwhile (an Alt+click, or Alt+Tab away and
        /// back, is no tap). Bound to `alt` on Windows and Linux.
        AltTap
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm { secondary: false }, Some(CONTEXT)),
        KeyBinding::new("space", Confirm { secondary: false }, Some(CONTEXT)),
    ]);
    // macOS keeps its menus in the system menu bar.
    if !cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("f10", FocusAppMenuBar, None),
            KeyBinding::new("alt", AltTap, None),
        ]);
    }
}

/// The application menu bar, for Windows and Linux.
///
/// Besides the pointer, the keyboard reaches it the way Windows users
/// expect: F10 or a tap of Alt focuses the first menu's title, Left and
/// Right move between titles, Down, Enter or Space open the focused menu,
/// and Escape goes back. Alt with a menu's underlined letter opens that menu
/// directly, and the letters are underlined while Alt is held or the bar has
/// focus. These need the window's root, which [`gpui_kit::open_window`]
/// mounts.
pub struct AppMenuBar {
    menus: Vec<Entity<AppMenu>>,
    selected_index: Option<usize>,
    action_context: Option<FocusHandle>,
    focus_handle: FocusHandle,
    /// The menu whose title has keyboard focus while no menu is open.
    highlighted_index: Option<usize>,
    /// Each menu's access key: the byte offset of its letter in the name
    /// and the letter, lowercase.
    mnemonics: Vec<Option<(usize, char)>>,
    /// Whether Alt is held, so the access keys are underlined.
    alt_held: bool,
    /// Ends keyboard mode when focus moves out of the bar and its menus.
    _focus_out: Option<Subscription>,
}

/// Each name's access key: its first letter or digit not taken by an earlier
/// name, as `(byte offset, lowercase letter)`.
fn mnemonics<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Option<(usize, char)>> {
    let mut taken = Vec::new();
    names
        .into_iter()
        .map(|name| {
            let found = name
                .char_indices()
                .filter(|(_, ch)| ch.is_alphanumeric())
                .map(|(ix, ch)| (ix, ch.to_lowercase().next().unwrap_or(ch)))
                .find(|(_, ch)| !taken.contains(ch));
            if let Some((_, ch)) = found {
                taken.push(ch);
            }
            found
        })
        .collect()
}

impl AppMenuBar {
    /// Create a new app menu bar.
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let mut this = Self {
                selected_index: None,
                action_context: None,
                menus: Vec::new(),
                focus_handle: cx.focus_handle(),
                highlighted_index: None,
                mnemonics: Vec::new(),
                alt_held: false,
                _focus_out: None,
            };
            this.reload(cx);
            this
        })
    }

    /// Reload the menus from the app.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let menu_bar = cx.entity();
        let menus: Vec<OwnedMenu> = GlobalState::global(cx)
            .app_menus()
            .iter()
            .cloned()
            .collect();
        self.mnemonics = mnemonics(menus.iter().map(|menu| menu.name.as_ref()));
        self.menus = menus
            .iter()
            .enumerate()
            .map(|(ix, menu)| AppMenu::new(ix, menu, menu_bar.clone(), cx))
            .collect();
        self.selected_index = None;
        self.highlighted_index = None;
        self.action_context = None;
        cx.notify();
    }

    /// Whether the bar has the keyboard: a menu's title is focused or a
    /// menu is open.
    pub fn is_active(&self) -> bool {
        self.selected_index.is_some() || self.highlighted_index.is_some()
    }

    /// The menu whose title has keyboard focus while no menu is open.
    pub fn highlighted_index(&self) -> Option<usize> {
        self.highlighted_index
    }

    /// The menu that is open.
    pub fn open_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Focus the first menu's title from the keyboard, remembering what had
    /// focus. If the bar already has the keyboard, give focus back instead.
    /// What F10 and a tap of Alt do.
    pub fn toggle_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_active() {
            self.leave(window, cx);
        } else if !self.menus.is_empty() {
            self.action_context = window.focused(cx);
            self.focus_handle.focus(window, cx);
            self.highlighted_index = Some(0);
            cx.notify();
        }
    }

    /// Open the menu whose access key is `key`, as Alt with that letter
    /// does. Returns whether one did.
    pub fn open_menu_with_key(
        &mut self,
        key: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = key.to_lowercase().next().unwrap_or(key);
        let Some(ix) = self
            .mnemonics
            .iter()
            .position(|mnemonic| mnemonic.is_some_and(|(_, ch)| ch == key))
        else {
            return false;
        };
        self.highlighted_index = None;
        self.set_selected_index(Some(ix), window, cx);
        true
    }

    /// Underline the access keys while Alt is held.
    pub(crate) fn set_alt_held(&mut self, held: bool, cx: &mut Context<Self>) {
        if self.alt_held != held {
            self.alt_held = held;
            cx.notify();
        }
    }

    /// Focus went somewhere: if it left the bar and its open menu (the user
    /// clicked another control), keyboard mode ends where focus now is.
    fn on_focus_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_active() || self.focus_handle.contains_focused(window, cx) {
            return;
        }
        let in_open_menu = self
            .selected_index
            .and_then(|ix| self.menus.get(ix))
            .and_then(|menu| menu.read(cx).popup_menu.clone())
            .is_some_and(|popup| popup.read(cx).focus_handle(cx).contains_focused(window, cx));
        if in_open_menu {
            return;
        }
        self.highlighted_index = None;
        self.selected_index = None;
        self.action_context = None;
        cx.notify();
    }

    /// Give focus back to what had it and leave keyboard mode.
    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.highlighted_index = None;
        self.set_selected_index(None, window, cx);
    }

    fn on_open_highlighted(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.open_highlighted(window, cx);
    }

    fn on_move_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.open_highlighted(window, cx);
    }

    fn open_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlighted_index.take() {
            self.set_selected_index(Some(ix), window, cx);
        }
    }

    /// While a title has focus, its access key opens its menu.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.highlighted_index.is_none()
            || keystroke.modifiers.modified()
            || keystroke.key.chars().count() != 1
        {
            return;
        }
        if let Some(key) = keystroke.key.chars().next()
            && self.open_menu_with_key(key, window, cx)
        {
            cx.stop_propagation();
        }
    }

    fn on_move_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlighted_index {
            self.highlighted_index = Some(ix.checked_sub(1).unwrap_or(self.menus.len() - 1));
            cx.notify();
            return;
        }
        let Some(selected_index) = self.selected_index else {
            return;
        };

        let new_ix = if selected_index == 0 {
            self.menus.len().saturating_sub(1)
        } else {
            selected_index.saturating_sub(1)
        };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_move_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlighted_index {
            self.highlighted_index = Some((ix + 1) % self.menus.len());
            cx.notify();
            return;
        }
        let Some(selected_index) = self.selected_index else {
            return;
        };

        let new_ix = if selected_index + 1 >= self.menus.len() {
            0
        } else {
            selected_index + 1
        };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.leave(window, cx);
    }

    fn set_selected_index(
        &mut self,
        ix: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keep what had focus before the bar took the keyboard, not the bar.
        if self.selected_index.is_none() && ix.is_some() && self.action_context.is_none() {
            self.action_context = window.focused(cx);
        } else if ix.is_none() {
            match self.action_context.take() {
                Some(action_context) => action_context.focus(window, cx),
                // Nothing had focus before the bar took the keyboard, so
                // nothing has it after.
                None if self.focus_handle.contains_focused(window, cx) => window.blur(cx),
                None => {}
            }
        }

        self.selected_index = ix;
        cx.notify();
    }

    #[inline]
    fn has_activated_menu(&self) -> bool {
        self.selected_index.is_some()
    }
}

impl Render for AppMenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window's root routes F10, Alt and Alt with a letter here.
        let this = cx.entity().downgrade();
        crate::root::WindowState::try_update(window, cx, |state, window, cx| {
            state.register_app_menu_bar(this, window, cx)
        });
        if self._focus_out.is_none() {
            self._focus_out = Some(cx.on_focus_out(
                &self.focus_handle.clone(),
                window,
                |this, _, window, cx| this.on_focus_out(window, cx),
            ));
        }
        h_flex()
            .id("app-menu-bar")
            .overflow_x_scroll()
            .lock_scroll_axis()
            .test_support()
            .track_focus(&self.focus_handle)
            .role(Role::MenuBar)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_open_highlighted))
            .on_action(cx.listener(Self::on_cancel))
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .gap_x_1()
            .children(self.menus.clone())
    }
}

/// A menu in the menu bar.
pub(super) struct AppMenu {
    menu_bar: Entity<AppMenuBar>,
    ix: usize,
    name: SharedString,
    menu: OwnedMenu,
    popup_menu: Option<Entity<PopupMenu>>,
    /// Registered on the menu's title and never focused, so the popup can look
    /// its shortcuts up through the menu bar's key contexts on the frame it
    /// opens, when nothing has focus or focus is outside those contexts. GPUI
    /// finds a handle in the previously rendered frame, where the popup's own
    /// element is not yet.
    trigger_focus: FocusHandle,

    _subscription: Option<Subscription>,
}

impl AppMenu {
    pub(super) fn new(
        ix: usize,
        menu: &OwnedMenu,
        menu_bar: Entity<AppMenuBar>,
        cx: &mut App,
    ) -> Entity<Self> {
        let name = menu.name.clone();
        cx.new(|cx| Self {
            ix,
            menu_bar,
            name,
            menu: menu.clone(),
            popup_menu: None,
            trigger_focus: cx.focus_handle(),
            _subscription: None,
        })
    }

    /// Whether this menu's popup is the one the menu bar currently has open.
    fn is_open(&self, cx: &App) -> bool {
        self.menu_bar.read(cx).selected_index == Some(self.ix)
    }

    fn build_popup_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<PopupMenu> {
        let action_context = self.menu_bar.read(cx).action_context.clone();
        let popup_menu = match self.popup_menu.as_ref() {
            None => {
                let items = self.menu.items.clone();
                let popup_menu = PopupMenu::build(window, cx, |menu, window, cx| {
                    menu.with_menu_items(items, window, cx)
                });
                let trigger_focus = self.trigger_focus.clone();
                popup_menu.update(cx, |menu, cx| {
                    menu.set_action_context(action_context.clone(), cx);
                    menu.set_trigger_focus(Some(trigger_focus), cx);
                });
                self._subscription =
                    Some(cx.subscribe_in(&popup_menu, window, Self::handle_dismiss));
                self.popup_menu = Some(popup_menu.clone());

                popup_menu
            }
            Some(menu) => {
                menu.update(cx, |menu, cx| {
                    menu.set_action_context(action_context.clone(), cx);
                });
                menu.clone()
            }
        };

        let focus_handle = popup_menu.read(cx).focus_handle(cx);
        if !focus_handle.contains_focused(window, cx) {
            focus_handle.focus(window, cx);
        }

        popup_menu
    }

    fn handle_dismiss(
        &mut self,
        _: &Entity<PopupMenu>,
        _: &DismissEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscription.take();
        self.popup_menu.take();
        self.menu_bar.update(cx, |state, cx| {
            state.on_cancel(&Cancel, window, cx);
        });
    }

    fn handle_trigger_click(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, ClickEvent::Mouse(_)) {
            return;
        }

        self.toggle(window, cx);
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_open = self.is_open(cx);
        _ = self.menu_bar.update(cx, |state, cx| {
            let new_ix = if is_open { None } else { Some(self.ix) };
            state.set_selected_index(new_ix, window, cx);
        });
    }

    fn handle_hover(&mut self, hovered: &bool, window: &mut Window, cx: &mut Context<Self>) {
        if !*hovered {
            return;
        }

        let has_activated_menu = self.menu_bar.read(cx).has_activated_menu();
        if !has_activated_menu {
            return;
        }

        _ = self.menu_bar.update(cx, |state, cx| {
            state.set_selected_index(Some(self.ix), window, cx);
        });
    }
}

impl Render for AppMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_open = self.is_open(cx);
        let bar = self.menu_bar.read(cx);
        let is_highlighted = bar.highlighted_index == Some(self.ix);
        let show_mnemonic = bar.alt_held || bar.is_active();
        let mnemonic = bar.mnemonics.get(self.ix).copied().flatten();

        let trigger_focus = self.trigger_focus.clone();
        div()
            .id(self.ix)
            .relative()
            // Registers the handle on this path without making the title
            // focusable.
            .child(
                canvas(
                    move |_, window, cx| window.set_focus_handle(&trigger_focus, cx),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_0(),
            )
            .child(
                Button::new("menu")
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .map(|this| match mnemonic.filter(|_| show_mnemonic) {
                        Some((offset, _)) => {
                            let end = offset
                                + self.name[offset..].chars().next().map_or(1, char::len_utf8);
                            this.accessibility_label(self.name.clone()).child(
                                StyledText::new(self.name.clone()).with_highlights([(
                                    offset..end,
                                    HighlightStyle {
                                        underline: Some(UnderlineStyle {
                                            thickness: px(1.),
                                            ..Default::default()
                                        }),
                                        ..Default::default()
                                    },
                                )]),
                            )
                        }
                        None => this.label(self.name.clone()),
                    })
                    .selected(is_highlighted)
                    .open(is_open)
                    .on_mouse_down(
                        MouseButton::Left,
                        window.listener_for(&cx.entity(), move |this, _, window, cx| {
                            // Stop propagation to avoid dragging the window.
                            window.prevent_default();
                            cx.stop_propagation();
                            this.toggle(window, cx);
                        }),
                    )
                    .on_click(cx.listener(Self::handle_trigger_click)),
            )
            .on_hover(cx.listener(Self::handle_hover))
            .when(is_open, |this| {
                this.child(deferred(
                    anchored()
                        .anchor(gpui::Anchor::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .size_full()
                                .occlude()
                                .top_1()
                                .child(self.build_popup_menu(window, cx)),
                        ),
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use gpui::TestAppContext;

    gpui::actions!(app_menu_bar_tests, [SaveDoc]);

    /// A document under a menu bar, inside the key context its shortcuts are
    /// bound in, the way an application lays out its window.
    struct DocRoot {
        menu_bar: Entity<AppMenuBar>,
        doc: FocusHandle,
    }

    impl Render for DocRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context("Doc")
                .on_action(|_: &SaveDoc, _, _| {})
                .size_full()
                .child(div().h(px(30.)).child(self.menu_bar.clone()))
                .child(div().id("doc").track_focus(&self.doc).size_full())
        }
    }

    /// The menu opens complete: its first frame lays out the shortcuts beside
    /// the items, at the width it keeps, whether or not the document has
    /// focus.
    #[gpui::test]
    fn a_menu_opens_with_its_shortcuts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.bind_keys([KeyBinding::new("ctrl-alt-shift-s", SaveDoc, Some("Doc"))]);
            GlobalState::global_mut(cx).set_app_menus(vec![
                gpui::Menu::new("File")
                    .items([gpui::MenuItem::action("Save a Copy", SaveDoc)])
                    .owned(),
            ]);
        });
        for focused in [true, false] {
            let (root, cx) = cx.add_window_view(|window, cx| {
                let doc = cx.focus_handle();
                if focused {
                    doc.focus(window, cx);
                }
                DocRoot {
                    menu_bar: AppMenuBar::new(cx),
                    doc,
                }
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let menu_bar = root.read_with(cx, |root, _| root.menu_bar.clone());
            let popup = move |cx: &App| {
                menu_bar.read(cx).menus[0]
                    .read(cx)
                    .popup_menu
                    .clone()
                    .expect("the menu is open")
            };

            cx.simulate_mouse_down(
                gpui::point(px(10.), px(10.)),
                MouseButton::Left,
                Default::default(),
            );
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert!(cx.debug_bounds("kbd:ctrl-alt-shift-s").is_some());
            let (first, settled) = cx.update(|_, cx| popup(cx).read(cx).first_and_last_bounds());
            assert_eq!(
                first.size.width, settled.size.width,
                "focused: {focused}, the menu must open at its width"
            );
        }
    }

    struct TestRoot {
        menu_bar: Entity<AppMenuBar>,
        first_focus: FocusHandle,
        second_focus: FocusHandle,
    }

    impl Render for TestRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(div().id("first").track_focus(&self.first_focus))
                .child(div().id("second").track_focus(&self.second_focus))
                .child(self.menu_bar.clone())
        }
    }

    /// A window with File and Edit menus, with the document focused or
    /// nothing focused, and the window active.
    #[cfg(not(target_os = "macos"))]
    fn open_with_menus(
        focused: bool,
        cx: &mut TestAppContext,
    ) -> (Entity<AppMenuBar>, &mut gpui::VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            GlobalState::global_mut(cx).set_app_menus(vec![
                gpui::Menu::new("File")
                    .items([gpui::MenuItem::action("Save a Copy", SaveDoc)])
                    .owned(),
                gpui::Menu::new("Edit")
                    .items([gpui::MenuItem::action("Undo", SaveDoc)])
                    .owned(),
            ]);
        });
        let menu_bar = cx.update(AppMenuBar::new);
        let (_, cx) = cx.add_window_view({
            let menu_bar = menu_bar.clone();
            move |window, cx| {
                let doc = cx.focus_handle();
                if focused {
                    doc.focus(window, cx);
                }
                let view = cx.new(|_| DocRoot { menu_bar, doc });
                crate::Root::new(view, window, cx)
            }
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|window, _| window.is_window_active()));
        assert_eq!(
            cx.update(|window, cx| window.focused(cx).is_some()),
            focused
        );
        (menu_bar, cx)
    }

    /// Reports to the window that these modifiers are down.
    #[cfg(not(target_os = "macos"))]
    fn hold_modifiers(cx: &mut gpui::VisualTestContext, modifiers: gpui::Modifiers) {
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::ModifiersChanged(gpui::ModifiersChangedEvent {
                    modifiers,
                    capslock: gpui::Capslock { on: false },
                }),
                cx,
            );
        });
    }

    /// Lets the interval in which the window's modifiers are checked pass.
    #[cfg(not(target_os = "macos"))]
    fn let_modifiers_be_checked(cx: &mut gpui::VisualTestContext) {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(crate::root::ALT_WATCH_INTERVAL * 2);
        cx.run_until_parked();
    }

    /// With nothing focused, GPUI sends modifier changes to no element of
    /// the window. Alt held still underlines the access keys, and letting go
    /// of it hides them again.
    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn alt_underlines_the_access_keys_with_nothing_focused(cx: &mut TestAppContext) {
        let (menu_bar, cx) = open_with_menus(false, cx);
        let alt_held =
            |cx: &mut gpui::VisualTestContext| menu_bar.read_with(cx, |bar, _| bar.alt_held);
        assert!(!alt_held(cx));

        hold_modifiers(cx, gpui::Modifiers::alt());
        let_modifiers_be_checked(cx);
        assert!(alt_held(cx), "Alt went down");

        hold_modifiers(cx, gpui::Modifiers::default());
        let_modifiers_be_checked(cx);
        assert!(!alt_held(cx), "Alt was let go");

        // Alt held with Shift is no Alt press.
        hold_modifiers(
            cx,
            gpui::Modifiers {
                alt: true,
                shift: true,
                ..Default::default()
            },
        );
        let_modifiers_be_checked(cx);
        assert!(!alt_held(cx));
        hold_modifiers(cx, gpui::Modifiers::default());

        // It goes on after the bar was used with the keyboard.
        cx.simulate_keystrokes("f10 escape");
        let_modifiers_be_checked(cx);
        assert!(!alt_held(cx), "the bar closed");
        hold_modifiers(cx, gpui::Modifiers::alt());
        let_modifiers_be_checked(cx);
        assert!(alt_held(cx), "Alt went down again");
        hold_modifiers(cx, gpui::Modifiers::default());
        let_modifiers_be_checked(cx);
        assert!(!alt_held(cx));
    }

    /// The access keys stay hidden while the window isn't the active one.
    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn alt_in_an_inactive_window_underlines_nothing(cx: &mut TestAppContext) {
        let (menu_bar, cx) = open_with_menus(false, cx);
        cx.deactivate_window();
        let_modifiers_be_checked(cx);
        hold_modifiers(cx, gpui::Modifiers::alt());
        let_modifiers_be_checked(cx);
        assert!(!menu_bar.read_with(cx, |bar, _| bar.alt_held));
    }

    /// With something focused the window's surface hears Alt itself, at once.
    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn alt_underlines_the_access_keys_at_once_with_a_focus(cx: &mut TestAppContext) {
        let (menu_bar, cx) = open_with_menus(true, cx);
        hold_modifiers(cx, gpui::Modifiers::alt());
        assert!(menu_bar.read_with(cx, |bar, _| bar.alt_held));
        hold_modifiers(cx, gpui::Modifiers::default());
        assert!(!menu_bar.read_with(cx, |bar, _| bar.alt_held));
    }

    #[test]
    fn access_keys_take_each_names_first_free_letter() {
        assert_eq!(
            mnemonics(["File", "Edit", "Format", "Fx", "Ünicode", "123", "--"]),
            vec![
                Some((0, 'f')),
                Some((0, 'e')),
                Some((1, 'o')),
                Some((1, 'x')),
                Some((0, 'ü')),
                Some((0, '1')),
                None
            ]
        );
    }

    #[gpui::test]
    fn preserves_action_context_while_switching_menus(cx: &mut TestAppContext) {
        let (root, cx) = cx.add_window_view(|window, cx| {
            let first_focus = cx.focus_handle();
            let second_focus = cx.focus_handle();
            first_focus.focus(window, cx);

            TestRoot {
                menu_bar: cx.new(|cx| AppMenuBar {
                    menus: Vec::new(),
                    selected_index: None,
                    action_context: None,
                    focus_handle: cx.focus_handle(),
                    highlighted_index: None,
                    mnemonics: Vec::new(),
                    alt_held: false,
                    _focus_out: None,
                }),
                first_focus,
                second_focus,
            }
        });

        let (menu_bar, first_focus, second_focus) = root.read_with(cx, |root, _| {
            (
                root.menu_bar.clone(),
                root.first_focus.clone(),
                root.second_focus.clone(),
            )
        });

        menu_bar.update_in(cx, |menu_bar, window, cx| {
            menu_bar.set_selected_index(Some(0), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&first_focus));

            second_focus.focus(window, cx);
            menu_bar.set_selected_index(Some(1), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&first_focus));

            menu_bar.set_selected_index(None, window, cx);
            assert!(menu_bar.action_context.is_none());
            assert_eq!(window.focused(cx).as_ref(), Some(&first_focus));

            second_focus.focus(window, cx);
            menu_bar.set_selected_index(Some(0), window, cx);
            assert_eq!(menu_bar.action_context.as_ref(), Some(&second_focus));
        });
    }
}
