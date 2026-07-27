//! macOS app-lifetime fix: keep the process alive when no window is visible.
//!
//! macOS terminates an app the moment it has no *visible* window, and that path
//! never raises Tauri's `ExitRequested` — so `prevent_exit`, hide-on-close and
//! `ActivationPolicy::Accessory` (all still in place) cannot see it and cannot
//! stop it (proved repeatedly in tofu.log: `exiting` with no `exit requested`).
//!
//! This module installs TWO defenses; the second is the load-bearing one:
//!
//! 1. Override the AppKit delegate callback
//!    `applicationShouldTerminateAfterLastWindowClosed:` to return NO. This is
//!    the *documented* fix and worth keeping, but on its own it is NOT enough —
//!    Mini-reproduced (2026-07-27, M4a build, once the always-visible badge
//!    window was deleted): closing the last visible window logs this callback
//!    returning `false` and the process still dies ~1s later. tao/winit does not
//!    even implement this selector, so overriding it only re-asserts AppKit's
//!    own default; the kill arrives through the process-lifetime subsystem,
//!    which never consults it.
//! 2. Opt out of automatic *and* sudden termination via `NSProcessInfo`
//!    (`disableSuddenTermination` + `disableAutomaticTermination:`). THIS is what
//!    actually keeps a window-less Accessory app alive — verified on the Mini:
//!    with it, closing the last window survives; Quit (Tauri `app.exit`, a
//!    ControlFlow exit, not `[NSApp terminate:]`) still exits cleanly. Never
//!    re-enabled: the app is meant to outlive its windows for its whole run.
//!
//! Tauri (via tao/winit) installs its own `NSApplication` delegate and owns that
//! object, so we do NOT replace it — we reach into the delegate's class at
//! startup and add/override the one selector on it. The IMP just returns NO
//! (and logs, so a Mini run shows when macOS consults it).
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
        let cls_ptr = cls as *mut objc2::ffi::objc_class;

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

        // The delegate override alone does NOT keep us alive: reproduced on the
        // Mac Mini (2026-07-27, M4a build) that closing the last *visible*
        // window logs `applicationShouldTerminateAfterLastWindowClosed -> false`
        // and the process still dies ~1s later. AppKit tears a window-less
        // Accessory app down through the process-lifetime subsystem, which never
        // consults that selector. Opt out of automatic *and* sudden termination
        // for the whole run so a hidden-window census of zero can't reclaim us.
        // Never re-enabled: this app is meant to outlive its windows, and its
        // Quit path is Tauri's `app.exit` (ControlFlow), not `[NSApp terminate:]`.
        let pi: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        if !pi.is_null() {
            let _: () = msg_send![pi, disableSuddenTermination];
            let reason: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: c"Tofu Nuggets is a menu-bar agent and outlives its windows".as_ptr()
            ];
            let _: () = msg_send![pi, disableAutomaticTermination: reason];
            logfile::log(app, "lifecycle: sudden + automatic termination disabled");
        } else {
            logfile::log(
                app,
                "lifecycle: NSProcessInfo nil; termination not disabled",
            );
        }
    }
}
