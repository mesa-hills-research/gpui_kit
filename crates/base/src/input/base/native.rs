#[cfg(target_os = "macos")]
mod macos {
    use std::{cell::RefCell, collections::HashMap, mem, ptr, sync::Once};

    use gpui::Window;
    use objc2::{
        ffi, msg_send,
        rc::Retained,
        runtime::{AnyObject, AnyProtocol, Imp, Sel},
        sel,
    };
    use objc2_foundation::NSString;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    static INSTALL_TEXT_CONTENT: Once = Once::new();

    thread_local! {
        static CONTENT_TYPES: RefCell<HashMap<usize, Retained<NSString>>> =
            RefCell::new(HashMap::new());
    }

    pub fn set_text_content_type(window: &Window, content_type: Option<&str>) {
        let Some(view) = ns_view(window) else {
            return;
        };

        INSTALL_TEXT_CONTENT.call_once(|| install_text_content(view));
        if view
            .class()
            .instance_method(sel!(setContentType:))
            .is_none()
        {
            return;
        }

        let ns_content_type = content_type.map(NSString::from_str);
        let ns_content_type = ns_content_type.as_ref().map_or(ptr::null_mut(), |value| {
            Retained::as_ptr(value).cast_mut().cast::<AnyObject>()
        });

        unsafe {
            let _: () = msg_send![view, setContentType: ns_content_type];
        }
    }

    fn ns_view(window: &Window) -> Option<&AnyObject> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };

        Some(unsafe { &*(handle.ns_view.as_ptr() as *const AnyObject) })
    }

    fn install_text_content(view: &AnyObject) {
        let class = view.class();
        let class = class as *const _ as *mut _;

        unsafe {
            let protocol = AnyProtocol::get(c"NSTextContent");
            if let Some(protocol) = protocol {
                ffi::class_addProtocol(class, protocol);
            }

            let content_type_imp: Imp = mem::transmute(
                content_type as unsafe extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject,
            );
            let set_content_type_imp: Imp = mem::transmute(
                set_content_type as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject),
            );

            ffi::class_addMethod(class, sel!(contentType), content_type_imp, c"@@:".as_ptr());
            ffi::class_addMethod(
                class,
                sel!(setContentType:),
                set_content_type_imp,
                c"v@:@".as_ptr(),
            );
        }
    }

    unsafe extern "C-unwind" fn content_type(this: &AnyObject, _: Sel) -> *mut AnyObject {
        let key = this as *const _ as usize;
        CONTENT_TYPES.with(|content_types| {
            content_types
                .borrow()
                .get(&key)
                .map_or(ptr::null_mut(), |value| {
                    Retained::as_ptr(value).cast_mut().cast::<AnyObject>()
                })
        })
    }

    unsafe extern "C-unwind" fn set_content_type(this: &AnyObject, _: Sel, value: *mut AnyObject) {
        let key = this as *const _ as usize;
        CONTENT_TYPES.with(|content_types| {
            let mut content_types = content_types.borrow_mut();
            if value.is_null() {
                content_types.remove(&key);
            } else if let Some(value) = unsafe { Retained::retain(value.cast::<NSString>()) } {
                content_types.insert(key, value);
            }
        });
    }
}

#[cfg(target_os = "macos")]
pub use macos::set_text_content_type;

use gpui::{App, FocusHandle, Global, SharedString, Window};

/// Presentation-independent context-menu model produced by the editor.
#[derive(Default)]
pub struct NativeMenu {
    pub items: Vec<NativeMenuItem>,
}

pub enum NativeMenuItem {
    Separator,
    Action {
        label: SharedString,
        disabled: bool,
        action: Box<dyn gpui::Action>,
    },
}

impl NativeMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn gpui::Action>) -> Self {
        self.menu_with_disabled(label, false, action)
    }

    pub fn menu_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        disabled: bool,
        action: Box<dyn gpui::Action>,
    ) -> Self {
        self.items.push(NativeMenuItem::Action {
            label: label.into(),
            disabled,
            action,
        });
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(NativeMenuItem::Separator);
        self
    }
}

/// The text field whose context menu is opening, and the drawn menu that took
/// the focus from it.
struct OpeningContextMenu {
    field: FocusHandle,
    menu: Option<FocusHandle>,
}

impl Global for OpeningContextMenu {}

/// Tell a text field that `menu`, a menu drawn in the window, took the
/// keyboard focus from it as it opened.
///
/// A drawn menu calls this as it takes the focus from `from`. When `from` is
/// a text field whose context menu is opening, the menu holds the focus for
/// the field, as [`crate::input::InputBaseState::is_focused`] says. Otherwise this
/// does nothing.
pub fn drawn_menu_took_focus(from: &FocusHandle, menu: &FocusHandle, cx: &mut App) {
    let opening_from = cx
        .try_global::<OpeningContextMenu>()
        .is_some_and(|opening| opening.field == *from);
    if opening_from {
        cx.global_mut::<OpeningContextMenu>().menu = Some(menu.clone());
    }
}

/// Run `open`, which opens `field`'s context menu, and return the drawn menu
/// that took the focus from the field, if one did.
pub(crate) fn open_context_menu(
    field: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
    open: impl FnOnce(&mut Window, &mut App),
) -> Option<FocusHandle> {
    cx.set_global(OpeningContextMenu {
        field: field.clone(),
        menu: None,
    });
    open(window, cx);
    cx.has_global::<OpeningContextMenu>()
        .then(|| cx.remove_global::<OpeningContextMenu>())
        .and_then(|opening| opening.menu)
}
