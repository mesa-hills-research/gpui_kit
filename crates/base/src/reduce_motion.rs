//! Honors the operating system's reduced-motion preference.
//!
//! Every Base transition, spring, presence and reveal consults
//! [`App::reduce_motion`], but GPUI never reads the platform's setting into
//! that flag: it stays `false` until something sets it. [`init`] reads the
//! setting when Base initializes and writes it into the flag, so an
//! application inherits the user's choice by calling `gpui_base::init` or
//! `gpui_component::init`.
//!
//! The setting is read on the platforms below; everywhere else the flag is
//! left alone.
//!
//! - macOS: `NSWorkspace.accessibilityDisplayShouldReduceMotion`, the
//!   "Reduce motion" switch under Accessibility → Display. Base follows its
//!   changes through `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.
//! - Windows: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`, the
//!   "Animation effects" switch under Accessibility → Visual effects, whose
//!   off state asks for reduced motion. Windows announces a change only to
//!   windows, whose messages GPUI handles, so Base reads it again whenever a
//!   window with a Base [`crate::Root`] becomes active: a change made in the
//!   Settings app applies when the user comes back.
//! - Linux: the `reduced-motion` key of the `org.freedesktop.appearance`
//!   namespace of the XDG desktop portal's Settings interface, which GNOME
//!   and KDE back with their own animation switches. The portal answers over
//!   D-Bus, so the reading lands a moment after `init` returns, and Base keeps
//!   following the portal's change signal for the life of the application.
//!
//! An application owns the flag once it sets it. Base writes the
//! flag only while it still holds what Base last wrote (or GPUI's initial
//! `false`), so an application that calls [`App::set_reduce_motion`] after
//! `init` is never overridden by a later reading, and one that wants to follow
//! the system again calls [`apply_system_reduce_motion`].
//!
//! Under GPUI's test scheduler the platform is never consulted, whichever
//! crate's tests are running: a probe that answers from another thread would
//! break the scheduler's determinism, and a test wanting reduced motion sets
//! the flag itself.

use gpui::{App, Global};

/// What Base last wrote into [`App::set_reduce_motion`], and whether it is
/// already listening for the platform to change its mind.
#[derive(Default)]
struct SystemReduceMotion {
    applied: Option<bool>,
    following: bool,
}

impl Global for SystemReduceMotion {}

/// Applies the system's reduced-motion preference during `gpui_base::init`.
pub(crate) fn init(cx: &mut App) {
    apply_system_reduce_motion(cx);
}

/// Reads the operating system's reduced-motion preference into
/// [`App::set_reduce_motion`].
///
/// `gpui_base::init` calls this once, and a window with a Base
/// [`crate::Root`] calls it again each time it becomes active. Base follows
/// changes on its own on macOS and on Linux; call this to re-read the
/// preference at other moments, such as on Windows without a Base root.
///
/// The application wins over the system: when the flag no longer holds what
/// Base last wrote, the application set it, and this call leaves it alone.
/// Where the platform cannot say (wasm, an unsupported desktop, a Linux
/// session without the portal) the flag is left as it is, and under GPUI's
/// test scheduler the platform is not asked at all.
pub fn apply_system_reduce_motion(cx: &mut App) {
    if is_test_scheduler(cx) {
        #[cfg(test)]
        if let Some(preference) = tests::system_preference() {
            apply_preference(preference, cx);
        }
        return;
    }
    apply_preference(platform::read(), cx);
    let state = cx.default_global::<SystemReduceMotion>();
    if !state.following {
        state.following = true;
        platform::follow(cx);
    }
}

/// Whether the application runs on GPUI's deterministic test scheduler,
/// which must not be woken by a platform answering from its own thread.
fn is_test_scheduler(cx: &App) -> bool {
    cx.background_executor()
        .scheduler_executor()
        .scheduler()
        .as_test()
        .is_some()
}

/// Writes one reading of the system preference into the flag, unless the
/// application has taken the flag over since the reading Base last applied.
fn apply_preference(preference: Option<bool>, cx: &mut App) {
    let Some(reduce) = preference else {
        return;
    };
    let applied = cx
        .try_global::<SystemReduceMotion>()
        .and_then(|state| state.applied);
    if cx.reduce_motion() != applied.unwrap_or(false) {
        return;
    }
    cx.set_reduce_motion(reduce);
    cx.default_global::<SystemReduceMotion>().applied = Some(reduce);
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use futures::StreamExt as _;
    use gpui::App;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
    use objc2_foundation::NSNotification;

    pub(super) fn read() -> Option<bool> {
        Some(NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion())
    }

    /// Reads the preference again whenever the workspace says the
    /// accessibility display options changed.
    pub(super) fn follow(cx: &mut App) {
        let (changed, mut changes) = futures::channel::mpsc::unbounded::<()>();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            let _ = changed.unbounded_send(());
        });
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: the name is AppKit's, no object filters the notifications,
        // and the block only sends on a channel, which any thread may do.
        // The notification center keeps the observer, and the block with it,
        // for the life of the application.
        let _observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                None,
                &block,
            )
        };
        cx.spawn(async move |cx| {
            while changes.next().await.is_some() {
                cx.update(|cx| super::apply_preference(read(), cx));
            }
        })
        .detach();
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use gpui::App;
    use windows::Win32::{
        Foundation::BOOL,
        UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        },
    };

    pub(super) fn read() -> Option<bool> {
        let mut animations_enabled = BOOL(0);
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL through `pvParam`
        // and ignores `uiParam`; the BOOL outlives the call.
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut animations_enabled as *mut BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        Some(!animations_enabled.as_bool())
    }

    pub(super) fn follow(_cx: &mut App) {}
}

#[cfg(target_os = "linux")]
mod platform {
    use ashpd::desktop::settings::{ReducedMotion, Settings};
    use futures::StreamExt as _;
    use gpui::App;

    /// The portal answers asynchronously; [`follow`] applies the first reading.
    pub(super) fn read() -> Option<bool> {
        None
    }

    pub(super) fn follow(cx: &mut App) {
        cx.spawn(async move |cx| {
            let settings = Settings::new().await.ok()?;
            let current = settings.reduced_motion().await.ok()?;
            cx.update(|cx| super::apply_preference(Some(reduces(current)), cx));
            let mut changes = settings.receive_reduced_motion_changed().await.ok()?;
            while let Some(preference) = changes.next().await {
                cx.update(|cx| super::apply_preference(Some(reduces(preference)), cx));
            }
            Some(())
        })
        .detach();
    }

    fn reduces(preference: ReducedMotion) -> bool {
        matches!(preference, ReducedMotion::ReducedMotion)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use gpui::App;

    pub(super) fn read() -> Option<bool> {
        None
    }

    pub(super) fn follow(_cx: &mut App) {}
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    use gpui::{AppContext as _, TestAppContext, VisualTestContext, div};

    use super::{apply_preference, apply_system_reduce_motion};

    thread_local! {
        /// What the system answers in a test, once a test says: under the
        /// test scheduler the platform is never asked.
        static SYSTEM_PREFERENCE: Cell<Option<Option<bool>>> = const { Cell::new(None) };
    }

    /// The preference a test's system holds, if the test set one.
    pub(crate) fn system_preference() -> Option<Option<bool>> {
        SYSTEM_PREFERENCE.with(Cell::get)
    }

    fn set_system_preference(preference: Option<bool>) {
        SYSTEM_PREFERENCE.with(|cell| cell.set(Some(preference)));
    }

    struct Content;

    impl gpui::Render for Content {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            div()
        }
    }

    /// A window with a Base root reads the preference again when it becomes
    /// active, so a change made in the system's settings while the
    /// application was in the background applies when the user comes back.
    #[gpui::test]
    fn a_root_window_reads_the_preference_when_activated(cx: &mut TestAppContext) {
        let window = cx.open_window(gpui::size(gpui::px(200.), gpui::px(100.)), |window, cx| {
            let content = cx.new(|_| Content);
            crate::Root::new(content, window, cx)
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let reduces = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.reduce_motion());
        let activate = |cx: &mut VisualTestContext| {
            cx.deactivate_window();
            cx.update(|window, _| window.activate_window());
            cx.run_until_parked();
        };

        set_system_preference(Some(true));
        activate(&mut cx);
        assert!(reduces(&mut cx));
        set_system_preference(Some(false));
        activate(&mut cx);
        assert!(!reduces(&mut cx));

        // The application's own choice stays.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        activate(&mut cx);
        assert!(reduces(&mut cx));
        set_system_preference(None);
    }

    #[gpui::test]
    fn a_system_preference_for_reduced_motion_sets_the_flag(cx: &mut TestAppContext) {
        cx.update(|cx| {
            apply_preference(Some(true), cx);
            assert!(cx.reduce_motion());
        });
    }

    #[gpui::test]
    fn an_unknown_system_preference_leaves_the_flag_alone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            apply_preference(None, cx);
            assert!(!cx.reduce_motion());

            cx.set_reduce_motion(true);
            apply_preference(None, cx);
            assert!(cx.reduce_motion());
        });
    }

    #[gpui::test]
    fn the_system_drives_the_flag_until_the_application_sets_it(cx: &mut TestAppContext) {
        cx.update(|cx| {
            apply_preference(Some(true), cx);
            apply_preference(Some(false), cx);
            assert!(!cx.reduce_motion());
            apply_preference(Some(true), cx);
            assert!(cx.reduce_motion());

            cx.set_reduce_motion(false);
            apply_preference(Some(true), cx);
            assert!(!cx.reduce_motion());
        });
    }

    #[gpui::test]
    fn the_test_scheduler_is_never_asked_for_the_platform_preference(cx: &mut TestAppContext) {
        cx.update(|cx| {
            apply_system_reduce_motion(cx);
            assert!(!cx.reduce_motion());
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(!cx.reduce_motion()));
    }

    #[gpui::test]
    fn a_flag_the_application_set_before_the_first_reading_is_kept(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            apply_preference(Some(false), cx);
            assert!(cx.reduce_motion());
        });
    }
}
