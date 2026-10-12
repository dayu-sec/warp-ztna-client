//! macOS chrome: Activity Monitor–sized continuous corners, close button only.

#![allow(unsafe_code)]

use std::ffi::c_void;

use objc2::class;
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::runtime::AnyObject;
use tauri::WebviewWindow;

/// Matches current macOS app windows (Activity Monitor / Tahoe).
const CORNER_RADIUS: f64 = 26.0;
const CLOSE_BUTTON: usize = 0;
const MINIATURIZE_BUTTON: usize = 1;
const ZOOM_BUTTON: usize = 2;

#[derive(Clone, Copy)]
#[repr(C)]
struct NSSize {
    width: f64,
    height: f64,
}

type CGFloat = f64;

// SAFETY: `NSSize` is `repr(C)` and mirrors AppKit's `{CGSize=dd}` encoding on 64-bit macOS.
unsafe impl Encode for NSSize {
    const ENCODING: Encoding = Encoding::Struct("NSSize", &[CGFloat::ENCODING, CGFloat::ENCODING]);
}

pub fn apply(window: &WebviewWindow) {
    let _ = window.set_decorations(true);
    let _ = window.set_title_bar_style(tauri::TitleBarStyle::Overlay);
    let _ = window.set_shadow(true);
    let _ = window.set_minimizable(false);
    let _ = window.set_maximizable(false);
    if let Ok(ns_window) = window.ns_window() {
        hide_min_zoom(ns_window);
        // SAFETY: `ns_window` is a live `NSWindow` pointer from Tauri.
        unsafe {
            enlarge_close_button(ns_window);
        }
        apply_continuous_radius(ns_window, CORNER_RADIUS);
    }
}

pub fn activate() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
    }
}

pub fn hide_min_zoom(ns_window: *mut c_void) {
    if ns_window.is_null() {
        return;
    }
    // SAFETY: pointer comes from Tauri `ns_window()` and is a live `NSWindow`.
    unsafe {
        let win = ns_window.cast::<AnyObject>();
        hide_button(win, MINIATURIZE_BUTTON);
        hide_button(win, ZOOM_BUTTON);
    }
}

unsafe fn hide_button(win: *const AnyObject, button: usize) {
    let btn: *mut AnyObject = msg_send![win, standardWindowButton: button];
    if !btn.is_null() {
        let _: () = msg_send![btn, setHidden: true];
    }
}

unsafe fn enlarge_close_button(win: *mut c_void) {
    let window = win.cast::<AnyObject>();
    let btn: *mut AnyObject = msg_send![window, standardWindowButton: CLOSE_BUTTON];
    if !btn.is_null() {
        let _: () = msg_send![
            btn,
            setFrameSize: NSSize { width: 24.0, height: 24.0 }
        ];
        let _: () = msg_send![btn, setNeedsDisplay: true];
    }
}

fn apply_continuous_radius(ns_window: *mut c_void, radius: f64) {
    if ns_window.is_null() {
        return;
    }
    // SAFETY: same `NSWindow` as above; we only set public CALayer properties.
    unsafe {
        let win = ns_window.cast::<AnyObject>();
        let _: () = msg_send![win, _setCornerRadius: radius];
        let content: *mut AnyObject = msg_send![win, contentView];
        if content.is_null() {
            return;
        }
        let _: () = msg_send![content, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![content, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMaskedCorners: 15u64];
        let _: () = msg_send![layer, setMasksToBounds: true];
    }
}
