//! Each platform's keys, so a case can check macOS, Windows and Linux from
//! any one of them.
//!
//! The text keymaps are built for a platform given as a value, so a test can
//! bind another platform's tables wherever it runs. [`each`] runs a case once
//! for every platform with that platform's tables bound, and the case presses
//! the platform's keys through [`Keys`] and checks the platform's results.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use gpui_kit::{
    App, Global, KeyBinding, TestAppContext,
    component::input::{Keymap, KeymapPlatform},
};

/// Marks a test's app as initialized, so [`init`] runs `gpui_kit::init` once.
struct Initialized;

impl Global for Initialized {}

/// Initialize the kit, the first time for the test's app, and bind the text
/// keymaps of `platform`.
pub fn init(platform: KeymapPlatform, cx: &mut TestAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<Initialized>() {
            gpui_kit::init(cx);
            cx.set_global(Initialized);
        }
        bind_keymaps(platform, cx);
    });
}

/// Bind the text keymaps (CUA, Emacs and Vim) of `platform` in place of the
/// ones bound now. They keep their place among the other bindings, where
/// `gpui_kit::init` put them, since the order decides between bindings of
/// the same keys.
fn bind_keymaps(platform: KeymapPlatform, cx: &mut App) {
    let is_keymap = |binding: &KeyBinding| {
        binding.predicate().is_some_and(|predicate| {
            let predicate = predicate.to_string();
            Keymap::ALL
                .iter()
                .any(|keymap| predicate.contains(&format!("keymap == {}", keymap.name())))
        })
    };
    let bindings: Vec<KeyBinding> = cx.key_bindings().borrow().bindings().cloned().collect();
    let first = bindings
        .iter()
        .position(is_keymap)
        .expect("gpui_kit::init binds the text keymaps");
    let mut rebound = Vec::with_capacity(bindings.len());
    for (index, binding) in bindings.into_iter().enumerate() {
        if index == first {
            rebound.extend(
                Keymap::ALL
                    .into_iter()
                    .flat_map(|keymap| keymap.bindings(platform)),
            );
        }
        if !is_keymap(&binding) {
            rebound.push(binding);
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(rebound);
}

/// Run `case` once for each platform, with that platform's [`Keys`]. A
/// failure names the platform after the assertion's own message.
pub fn each(cx: &mut TestAppContext, mut case: impl FnMut(Keys, &mut TestAppContext)) {
    for platform in KeymapPlatform::ALL {
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| case(Keys(platform), cx))) {
            eprintln!("The case above failed with the {platform:?} keymap.");
            resume_unwind(panic);
        }
    }
}

/// The keys a case presses on one platform's CUA keymap.
#[derive(Clone, Copy, Debug)]
pub struct Keys(pub KeymapPlatform);

impl Keys {
    /// The keys of the platform the test runs on.
    pub fn host() -> Self {
        Self(KeymapPlatform::current())
    }

    pub fn platform(self) -> KeymapPlatform {
        self.0
    }

    pub fn is_windows(self) -> bool {
        self.0 == KeymapPlatform::Windows
    }

    fn pick(self, macos: &'static str, other: &'static str) -> &'static str {
        if self.0.is_macos() { macos } else { other }
    }

    /// `key` with the platform's shortcut modifier, such as `cmd-v` on macOS
    /// and `ctrl-v` elsewhere.
    pub fn primary(self, key: &str) -> String {
        format!("{}-{key}", self.0.primary())
    }

    /// `key` with the modifier that moves and deletes by word, Option on
    /// macOS and Ctrl elsewhere, such as `alt-left` or `ctrl-backspace`.
    pub fn word(self, key: &str) -> String {
        format!("{}-{key}", self.pick("alt", "ctrl"))
    }

    /// To the start of the line: Cmd-Left on macOS, where Home only scrolls,
    /// and Home elsewhere.
    pub fn line_start(self) -> &'static str {
        self.pick("cmd-left", "home")
    }

    /// To the end of the line: Cmd-Right on macOS, where End only scrolls,
    /// and End elsewhere.
    pub fn line_end(self) -> &'static str {
        self.pick("cmd-right", "end")
    }

    /// To the start of the text: Cmd-Up on macOS and Ctrl-Home elsewhere.
    pub fn text_start(self) -> &'static str {
        self.pick("cmd-up", "ctrl-home")
    }

    /// To the end of the text: Cmd-Down on macOS and Ctrl-End elsewhere.
    pub fn text_end(self) -> &'static str {
        self.pick("cmd-down", "ctrl-end")
    }

    /// A page up: Option-Page Up on macOS, where Page Up only scrolls, and
    /// Page Up elsewhere.
    pub fn page_up(self) -> &'static str {
        self.pick("alt-pageup", "pageup")
    }

    /// A page down: Option-Page Down on macOS, where Page Down only scrolls,
    /// and Page Down elsewhere.
    pub fn page_down(self) -> &'static str {
        self.pick("alt-pagedown", "pagedown")
    }

    /// Redo: Cmd-Shift-Z on macOS and Ctrl-Y elsewhere.
    pub fn redo(self) -> &'static str {
        self.pick("cmd-shift-z", "ctrl-y")
    }

    /// Add a cursor on the line above.
    pub fn add_cursor_above(self) -> &'static str {
        match self.0 {
            KeymapPlatform::MacOS => "cmd-alt-up",
            KeymapPlatform::Windows => "ctrl-alt-up",
            KeymapPlatform::Linux => "shift-alt-up",
        }
    }

    /// Add a cursor on the line below.
    pub fn add_cursor_below(self) -> &'static str {
        match self.0 {
            KeymapPlatform::MacOS => "cmd-alt-down",
            KeymapPlatform::Windows => "ctrl-alt-down",
            KeymapPlatform::Linux => "shift-alt-down",
        }
    }
}
