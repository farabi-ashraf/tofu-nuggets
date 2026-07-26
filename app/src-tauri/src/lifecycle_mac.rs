//! macOS app-lifetime fix: keep the process alive when no window is visible.
//!
//! macOS terminates an app the moment it has no *visible* window, and that path
//! never raises Tauri's `ExitRequested` — so `prevent_exit`, hide-on-close and
//! `ActivationPolicy::Accessory` (all still in place) cannot see it and cannot
//! stop it (proved repeatedly in tofu.log: `exiting` with no `exit requested`).
//! The documented fix is to answer the AppKit delegate callback
//! `applicationShouldTerminateAfterLastWindowClosed:` with NO.
//!
//! Tauri (via tao/winit) installs its own `NSApplication` delegate and owns that
//! object, so we do NOT replace it — we reach into the delegate's class at
//! startup and add/override this one selector on it. The IMP just returns NO
//! (and logs, so a Mini run proves macOS actually consults it).
//!
//! Because the override is a bare `extern "C"` function it cannot capture the
//! `AppHandle` it needs for logging; the handle is stashed in a process-global
//! `OnceLock` at install time.

use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};
use tauri::AppHandle;

use crate::logfile;

static LOG_APP: OnceLock<AppHandle> = OnceLock::new();

/// `- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)`.
/// Always NO: this app lives in the tray/menu bar and outlives its windows.
extern "C" fn should_terminate_after_last_window_closed(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> Bool {
    if let Some(app) = LOG_APP.get() {
        logfile::log(
            app,
            "delegate: applicationShouldTerminateAfterLastWindowClosed -> false",
        );
    }
    Bool::NO
}

/// Add/override the terminate-after-last-window-closed method on the delegate
/// Tauri already installed. Must run on the main thread (Tauri's `setup` hook
/// is); the delegate is set while the event loop is created, i.e. before setup.
pub fn install(app: &AppHandle) {
    let _ = LOG_APP.set(app.clone());

    unsafe {
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if ns_app.is_null() {
            logfile::log(app, "delegate override: NSApplication nil");
            return;
        }
        let delegate: *mut AnyObject = msg_send![ns_app, delegate];
        if delegate.is_null() {
            // Would mean the fix is inert — surface it rather than fail silent.
            logfile::log(app, "delegate override: NSApp delegate nil at setup");
            return;
        }
        let cls: *mut AnyClass = msg_send![delegate, class];
        if cls.is_null() {
            logfile::log(app, "delegate override: delegate class nil");
            return;
        }
        let cls_ptr = cls as *const objc2::ffi::objc_class;

        // Method type encoding `BOOL(id, SEL, id)`. The runtime uses this only
        // for introspection/forwarding, never for normal dispatch — AppKit
        // calls this selector through a compile-time signature on its side — so
        // the historical `c` for BOOL is accepted on every arch (its return-ABI
        // is identical to arm64's `B`).
        let types = c"c@:@";
        let imp: objc2::ffi::IMP = Some(std::mem::transmute::<
            extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool,
            unsafe extern "C" fn(),
        >(should_terminate_after_last_window_closed));

        // Replace (not add): overrides a pre-existing impl too, and no-ops the
        // add/replace distinction.
        objc2::ffi::class_replaceMethod(
            cls_ptr,
            sel!(applicationShouldTerminateAfterLastWindowClosed:).as_ptr(),
            imp,
            types.as_ptr(),
        );
        logfile::log(
            app,
            "delegate override: applicationShouldTerminateAfterLastWindowClosed installed",
        );
    }
}
