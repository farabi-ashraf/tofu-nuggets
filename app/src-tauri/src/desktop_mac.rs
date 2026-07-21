//! macOS implementation of `icons::DesktopIcons`: Accessibility (AX) API.
//!
//! Mirror of the Windows UIA approach: a system-wide AX hit-test
//! (`AXUIElementCopyElementAtPosition`, the `ElementFromPoint` analogue)
//! identifies the element under the cursor; it counts as a desktop icon when
//! it is an `AXImage` inside an `AXScrollArea` whose window covers a whole
//! display — Finder draws the desktop as a borderless full-screen window, so
//! this distinguishes desktop icons from icon-view items in ordinary Finder
//! windows. Display names resolve to paths against `~/Desktop`.
//!
//! Requires the Accessibility permission (System Settings → Privacy &
//! Security → Accessibility). `new_icons` triggers the system prompt via
//! `AXIsProcessTrustedWithOptions`; until granted, AX calls fail and hover
//! stays inert (a grant may need an app restart to take effect).
//!
//! Units: AX and CoreGraphics speak POINTS (global, top-left origin); the
//! `DesktopIcons` contract and the hover engine speak physical PIXELS. All
//! conversion happens here, using the backing scale of the display that
//! contains the coordinate — never let points leak out of this module.
//!
//! FFI is hand-declared (no bindings crate): only simple C functions from
//! the ApplicationServices umbrella framework, kept to the minimum this
//! module actually calls.
//!
//! Not yet implemented (later Route 1 PRs): `selected_icon` (hotkey works
//! via cursor position only), `list_icons` (badge layer is Windows-only).

use std::ffi::c_void;
use std::path::PathBuf;

use crate::icons::{resolve_path, DesktopIcons, Icon, IconRect};

#[allow(non_snake_case, non_upper_case_globals)]
mod ffi {
    use std::ffi::c_void;

    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFAllocatorRef = *const c_void;
    pub type AXUIElementRef = *const c_void;
    pub type AXError = i32;
    pub type CFIndex = isize;
    pub type Boolean = u8;
    pub type CGDirectDisplayID = u32;
    pub type CGError = i32;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }

    pub const kAXErrorSuccess: AXError = 0;
    // AXValue.h AXValueType: 1 = CGPoint, 2 = CGSize.
    pub const kAXValueCGPointType: u32 = 1;
    pub const kAXValueCGSizeType: u32 = 2;
    pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;
        pub static kCFBooleanTrue: CFTypeRef;

        pub fn CFRelease(cf: CFTypeRef);
        pub fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const u8,
            encoding: u32,
        ) -> CFStringRef;
        pub fn CFStringGetCString(
            the_string: CFStringRef,
            buffer: *mut u8,
            buffer_size: CFIndex,
            encoding: u32,
        ) -> Boolean;
        pub fn CFDictionaryCreate(
            alloc: CFAllocatorRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub static kAXTrustedCheckOptionPrompt: CFStringRef;

        pub fn AXIsProcessTrusted() -> Boolean;
        pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
        pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        pub fn AXUIElementCopyElementAtPosition(
            application: AXUIElementRef,
            x: f32,
            y: f32,
            element: *mut AXUIElementRef,
        ) -> AXError;
        pub fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        pub fn AXValueGetValue(value: CFTypeRef, the_type: u32, value_ptr: *mut c_void) -> Boolean;

        pub fn CGGetActiveDisplayList(
            max_displays: u32,
            active_displays: *mut CGDirectDisplayID,
            display_count: *mut u32,
        ) -> CGError;
        pub fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
        pub fn CGDisplayPixelsWide(display: CGDirectDisplayID) -> usize;
        pub fn CGEventCreate(source: *const c_void) -> CFTypeRef;
        pub fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    }
}

use ffi::*;

/// Owned CF/AX object: released on drop. Never wraps a null pointer.
struct CfOwned(CFTypeRef);

impl CfOwned {
    fn new(ptr: CFTypeRef) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

/// CFString from a Rust literal (attribute names are ASCII).
fn cf_string(s: &str) -> Option<CfOwned> {
    let c = format!("{s}\0");
    CfOwned::new(unsafe {
        CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), kCFStringEncodingUTF8)
    })
}

pub struct MacIcons {
    system_wide: CfOwned,
    dirs: Vec<PathBuf>,
}

fn copy_attr(elem: CFTypeRef, name: &str) -> Option<CfOwned> {
    let attr = cf_string(name)?;
    let mut out: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(elem, attr.0, &mut out) };
    if err != kAXErrorSuccess {
        return None;
    }
    CfOwned::new(out)
}

/// Read a CFString attribute into a Rust String.
fn string_attr(elem: CFTypeRef, name: &str) -> Option<String> {
    let val = copy_attr(elem, name)?;
    let mut buf = [0u8; 1024];
    let ok = unsafe {
        CFStringGetCString(
            val.0,
            buf.as_mut_ptr(),
            buf.len() as CFIndex,
            kCFStringEncodingUTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

/// Element frame in POINTS from its AXPosition + AXSize.
fn frame_pts(elem: CFTypeRef) -> Option<CGRect> {
    let pos_val = copy_attr(elem, "AXPosition")?;
    let size_val = copy_attr(elem, "AXSize")?;
    let mut origin = CGPoint::default();
    let mut size = CGSize::default();
    unsafe {
        if AXValueGetValue(
            pos_val.0,
            kAXValueCGPointType,
            &mut origin as *mut _ as *mut c_void,
        ) == 0
            || AXValueGetValue(
                size_val.0,
                kAXValueCGSizeType,
                &mut size as *mut _ as *mut c_void,
            ) == 0
        {
            return None;
        }
    }
    Some(CGRect { origin, size })
}

/// Active displays as (bounds in points, backing scale).
fn displays() -> Vec<(CGRect, f64)> {
    let mut ids = [0 as CGDirectDisplayID; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return Vec::new();
    }
    ids[..count as usize]
        .iter()
        .map(|&id| {
            let bounds = unsafe { CGDisplayBounds(id) };
            let px_wide = unsafe { CGDisplayPixelsWide(id) } as f64;
            let scale = if bounds.size.width > 0.0 {
                px_wide / bounds.size.width
            } else {
                1.0
            };
            (bounds, scale)
        })
        .collect()
}

/// Backing scale of the display containing the point; 1.0 when unknown.
fn scale_at_pts(x: f64, y: f64) -> f64 {
    for (b, scale) in displays() {
        if x >= b.origin.x
            && x < b.origin.x + b.size.width
            && y >= b.origin.y
            && y < b.origin.y + b.size.height
        {
            return scale;
        }
    }
    1.0
}

/// The window covers a whole display (couple-of-points tolerance) — Finder's
/// borderless desktop window signature. Heuristic to verify on hardware:
/// distinguishes desktop icons from icon-view items in normal Finder windows,
/// which are practically never exactly display-sized at the display origin.
fn window_is_desktop(win: CFTypeRef) -> bool {
    let Some(f) = frame_pts(win) else {
        return false;
    };
    const TOL: f64 = 2.0;
    displays().iter().any(|(b, _)| {
        (f.origin.x - b.origin.x).abs() <= TOL
            && (f.origin.y - b.origin.y).abs() <= TOL
            && (f.size.width - b.size.width).abs() <= TOL
            && (f.size.height - b.size.height).abs() <= TOL
    })
}

impl DesktopIcons for MacIcons {
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon> {
        // Engine pixels → AX points. The conversion needs the display's
        // scale, but the display lookup itself wants points — so probe
        // twice: first with the raw pixel values (correct whenever the
        // point's display sits at the origin or scales are uniform), then
        // re-probe with that estimate. Verify on mixed-DPI multi-monitor
        // hardware.
        let scale = scale_at_pts(x as f64, y as f64);
        let (xp, yp) = (x as f64 / scale, y as f64 / scale);
        let scale = scale_at_pts(xp, yp);
        let (xp, yp) = (x as f64 / scale, y as f64 / scale);

        let mut raw: AXUIElementRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyElementAtPosition(self.system_wide.0, xp as f32, yp as f32, &mut raw)
        };
        if err != kAXErrorSuccess {
            return None;
        }
        let elem = CfOwned::new(raw)?;

        if string_attr(elem.0, "AXRole")? != "AXImage" {
            return None;
        }
        let parent = copy_attr(elem.0, "AXParent")?;
        if string_attr(parent.0, "AXRole")? != "AXScrollArea" {
            return None;
        }
        let window = copy_attr(elem.0, "AXWindow")?;
        if !window_is_desktop(window.0) {
            return None;
        }

        let name = string_attr(elem.0, "AXTitle")
            .filter(|s| !s.is_empty())
            .or_else(|| string_attr(elem.0, "AXDescription").filter(|s| !s.is_empty()))?;
        let f = frame_pts(elem.0)?;
        let s = scale_at_pts(f.origin.x, f.origin.y);
        let rect = IconRect {
            left: (f.origin.x * s).round() as i32,
            top: (f.origin.y * s).round() as i32,
            right: ((f.origin.x + f.size.width) * s).round() as i32,
            bottom: ((f.origin.y + f.size.height) * s).round() as i32,
        };
        let path = resolve_path(&name, &self.dirs);
        Some(Icon { name, rect, path })
    }

    /// Badge layer is Windows-only for now; implemented with the macOS badge
    /// equivalent (Route 1).
    fn list_icons(&self) -> Result<Vec<Icon>, String> {
        Ok(Vec::new())
    }

    /// Not implemented yet: the hotkey falls back to this only when the
    /// cursor is not over an icon; cursor targeting already works.
    fn selected_icon(&self) -> Option<Icon> {
        None
    }
}

pub fn new_icons() -> Result<MacIcons, String> {
    // Trigger the system Accessibility prompt (also registers the app in the
    // System Settings list). Proceed even when not yet trusted: AX calls
    // fail cleanly and start working once the user grants (+ app restart).
    unsafe {
        let key = kAXTrustedCheckOptionPrompt;
        let val = kCFBooleanTrue;
        let opts = CFDictionaryCreate(
            std::ptr::null(),
            &key as *const CFTypeRef,
            &val as *const CFTypeRef,
            1,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let trusted = AXIsProcessTrustedWithOptions(opts);
        if !opts.is_null() {
            CFRelease(opts);
        }
        if trusted == 0 {
            eprintln!(
                "mac icons: Accessibility permission not granted — hover stays \
                 inert (System Settings → Privacy & Security → Accessibility, \
                 then restart the app)"
            );
        }
    }
    let system_wide =
        CfOwned::new(unsafe { AXUIElementCreateSystemWide() }).ok_or("AX system-wide element")?;
    Ok(MacIcons {
        system_wide,
        dirs: desktop_dirs(),
    })
}

/// Cursor position in physical pixels (CGEvent speaks points).
pub fn cursor_pos() -> Option<(i32, i32)> {
    let ev = CfOwned::new(unsafe { CGEventCreate(std::ptr::null()) })?;
    let p = unsafe { CGEventGetLocation(ev.0) };
    let s = scale_at_pts(p.x, p.y);
    Some(((p.x * s).round() as i32, (p.y * s).round() as i32))
}

/// Right-most physical-pixel edge across displays (panel edge-flip bound).
pub fn virtual_screen_width() -> i32 {
    displays()
        .iter()
        .map(|(b, s)| ((b.origin.x + b.size.width) * s).round() as i32)
        .max()
        .unwrap_or(i32::MAX)
}

pub fn desktop_dirs() -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Desktop"))
        .into_iter()
        .collect()
}

/// Whether the Accessibility permission is currently granted. Without it every
/// AX call fails, so hover and the hotkey's icon targeting do nothing at all —
/// the UI asks for this to explain that instead of looking broken.
///
/// Beta builds are ad-hoc signed, and macOS keys this permission to the code
/// signature: every new CI build counts as a different app and must be granted
/// again (old entries pile up in the list and can be removed).
pub fn accessibility_trusted() -> Option<bool> {
    Some(unsafe { AXIsProcessTrusted() } != 0)
}

pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// Finder has no equivalent of the desktop ListView infotip; nothing to do.
pub fn suppress_desktop_infotips() -> bool {
    false
}

/// No per-thread runtime setup needed on macOS (COM is Windows-only).
pub fn init_thread() {}
