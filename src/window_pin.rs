//! View > Always on Top: keeps editor windows above other windows. GPUI has
//! no API for it, so this talks to the platform window directly. Linux has
//! no portable way (Wayland forbids it), so the item is hidden there.

use gpui::{App, Global, Window};

/// Whether this platform can pin windows on top.
pub(crate) const SUPPORTED: bool = cfg!(any(target_os = "windows", target_os = "macos"));

/// App-wide Always on Top state; applies to every editor window.
#[derive(Default)]
pub(crate) struct AlwaysOnTop(pub(crate) bool);

impl Global for AlwaysOnTop {}

impl AlwaysOnTop {
    pub(crate) fn enabled(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|state| state.0)
    }
}

/// Puts `window` above (or back among) the other windows.
pub(crate) fn set_always_on_top(window: &Window, on_top: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    match handle.as_raw() {
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(handle) => {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
            };
            let hwnd = HWND(handle.hwnd.get() as *mut _);
            let after = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
            // SAFETY: the handle belongs to a live GPUI window.
            let result = unsafe {
                SetWindowPos(
                    hwnd,
                    Some(after),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
            if let Err(err) = result {
                eprintln!("failed to change always-on-top: {err}");
            }
        }
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => {
            use objc::runtime::Object;
            use objc::{msg_send, sel, sel_impl};
            // NSFloatingWindowLevel / NSNormalWindowLevel.
            let level: isize = if on_top { 3 } else { 0 };
            let view = handle.ns_view.as_ptr() as *mut Object;
            // SAFETY: the view belongs to a live GPUI window.
            unsafe {
                let ns_window: *mut Object = msg_send![view, window];
                if !ns_window.is_null() {
                    let _: () = msg_send![ns_window, setLevel: level];
                }
            }
        }
        _ => {
            let _ = on_top;
        }
    }
}
