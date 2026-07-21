//! macOS implementation of `icons::DesktopIcons`: Accessibility (AX) API.
//!
//! Mirror of the Windows UIA approach: a system-wide AX hit-test
//! (`AXUIElementCopyElementAtPosition`, the `ElementFromPoint` analogue)
//! identifies the element under the cursor. A hit counts as a desktop icon
//! when its ancestor chain contains Finder's `AXScrollArea` icon container and
//! any window it reports spans a display. Display names resolve to paths
//! against `~/Desktop`.
//!
//! The element shapes here are NOT contractual — Finder exposes desktop items
//! differently across releases, and the first attempt (exact roles, exact
//! display-sized window) matched nothing on macOS 26. Hence the tolerant walk,
//! and `debug_cursor_chain`, which logs what was actually under the cursor
//! whenever targeting fails.
//!
//! Requires the Accessibility permission (System Settings → Privacy &
//! Security → Accessibility). `new_icons` triggers the system prompt via
//! `AXIsProcessTrustedWithOptions`; until granted, AX calls fail and hover
//! stays inert (a grant may need an app restart to take effect).
//!
//! Units: everything here stays in POINTS (global, top-left origin), which is
//! exactly what Tauri calls a *logical* coordinate, so the hover engine hands
//! these straight to `LogicalPosition`/`LogicalSize`. An earlier version
//! converted to physical pixels with `CGDisplayPixelsWide / CGDisplayBounds`;
//! that ratio is NOT the window backing scale on displays running a scaled
//! resolution (pixels/points can be 1.5 while the backing scale is 2.0), and
//! the panel landed far from its icon. Do not reintroduce the conversion —
//! see `hover::place_panel` for the matching platform split.
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

/// How far up the AX tree to look for the desktop container. Finder nests the
/// desktop a few levels deep and the exact depth is not contractual.
const MAX_DEPTH: usize = 8;

/// The hit element plus its ancestors, nearest first.
fn ancestor_chain(elem: CFTypeRef) -> Vec<CfOwned> {
    let mut chain = Vec::new();
    let mut cur = copy_attr(elem, "AXParent");
    while let Some(node) = cur {
        cur = copy_attr(node.0, "AXParent");
        chain.push(node);
        if chain.len() >= MAX_DEPTH {
            break;
        }
    }
    chain
}

/// First non-empty human name of an element: Finder exposes desktop item names
/// through different attributes depending on the element (the icon image, its
/// label, or the item row).
fn element_name(elem: CFTypeRef) -> Option<String> {
    ["AXTitle", "AXFilename", "AXDescription", "AXValue"]
        .iter()
        .find_map(|a| string_attr(elem, a).filter(|s| !s.is_empty()))
}

/// Window (if any) covers most of a display — the Finder desktop window spans
/// the screen, while ordinary windows normally do not.
///
/// Deliberately permissive: a false positive means a maximized Finder window in
/// icon view can also show notes, which is harmless; a false negative means
/// hover does not exist at all. An exact display-size match was tried first and
/// found nothing on macOS 26, hence the ratio.
fn covers_a_display(win: CFTypeRef) -> bool {
    let Some(f) = frame_pts(win) else {
        return false;
    };
    let area = f.size.width * f.size.height;
    displays()
        .iter()
        .any(|(b, _)| area >= 0.8 * (b.size.width * b.size.height))
}

/// Is this hit inside the desktop's icon container? Requires an `AXScrollArea`
/// ancestor (Finder's icon container) and, when the chain exposes a window at
/// all, one that spans a display. The desktop window is special and does not
/// always answer `AXWindow`, so a missing window is accepted rather than
/// treated as a rejection.
fn chain_is_desktop(chain: &[CfOwned]) -> bool {
    let has_scroll_area = chain
        .iter()
        .any(|e| string_attr(e.0, "AXRole").as_deref() == Some("AXScrollArea"));
    if !has_scroll_area {
        return false;
    }
    match chain.iter().find_map(|e| copy_attr(e.0, "AXWindow")) {
        Some(win) => covers_a_display(win.0),
        None => true,
    }
}

/// Human-readable dump of what sits under the cursor, written to the log when
/// targeting fails. Without it a failed lookup is indistinguishable from the
/// hotkey never firing, which cost a full hardware test round.
pub fn debug_cursor_chain() -> Option<String> {
    let (x, y) = cursor_pos()?;
    let (xp, yp) = (x as f64, y as f64);
    let system_wide = CfOwned::new(unsafe { AXUIElementCreateSystemWide() })?;
    let mut raw: AXUIElementRef = std::ptr::null();
    let err =
        unsafe { AXUIElementCopyElementAtPosition(system_wide.0, xp as f32, yp as f32, &mut raw) };
    if err != kAXErrorSuccess {
        return Some(format!(
            "AX hit-test at ({xp:.0},{yp:.0}) pts failed with AXError {err} \
             (-25204 = API disabled: permission missing or not yet applied to \
             this build)"
        ));
    }
    let elem = CfOwned::new(raw)?;
    let describe = |e: CFTypeRef| {
        let role = string_attr(e, "AXRole").unwrap_or_else(|| "?".into());
        let sub = string_attr(e, "AXSubrole").unwrap_or_else(|| "-".into());
        let name = element_name(e).unwrap_or_else(|| "-".into());
        let f = frame_pts(e)
            .map(|f| {
                format!(
                    "{:.0},{:.0} {:.0}x{:.0}",
                    f.origin.x, f.origin.y, f.size.width, f.size.height
                )
            })
            .unwrap_or_else(|| "no frame".into());
        format!("{role}/{sub} \"{name}\" [{f}]")
    };
    let mut out = format!(
        "AX chain at ({xp:.0},{yp:.0}) pts:\n  0: {}",
        describe(elem.0)
    );
    for (i, node) in ancestor_chain(elem.0).iter().enumerate() {
        out.push_str(&format!("\n  {}: {}", i + 1, describe(node.0)));
    }
    Some(out)
}

impl DesktopIcons for MacIcons {
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon> {
        let (xp, yp) = (x as f64, y as f64);
        let mut raw: AXUIElementRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyElementAtPosition(self.system_wide.0, xp as f32, yp as f32, &mut raw)
        };
        if err != kAXErrorSuccess {
            return None;
        }
        let elem = CfOwned::new(raw)?;

        let chain = ancestor_chain(elem.0);
        if !chain_is_desktop(&chain) {
            return None;
        }

        // The element actually hit may be the icon image, its text label, or
        // the item wrapping both, and only some of those carry the name — so
        // take the nearest one that has both a name and a frame. Bounded to
        // the item's own levels so a miss cannot fall through to the whole
        // scroll area and report the desktop itself as an icon.
        let (name, f) = std::iter::once(&elem)
            .chain(chain.iter().take(2))
            .filter(|e| string_attr(e.0, "AXRole").as_deref() != Some("AXScrollArea"))
            .find_map(|e| Some((element_name(e.0)?, frame_pts(e.0)?)))?;
        let rect = IconRect {
            left: f.origin.x.round() as i32,
            top: f.origin.y.round() as i32,
            right: (f.origin.x + f.size.width).round() as i32,
            bottom: (f.origin.y + f.size.height).round() as i32,
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

/// Cursor position in points, matching every other coordinate here.
pub fn cursor_pos() -> Option<(i32, i32)> {
    let ev = CfOwned::new(unsafe { CGEventCreate(std::ptr::null()) })?;
    let p = unsafe { CGEventGetLocation(ev.0) };
    Some((p.x.round() as i32, p.y.round() as i32))
}

/// Right-most edge across displays, in points (panel edge-flip bound).
pub fn virtual_screen_width() -> i32 {
    displays()
        .iter()
        .map(|(b, _)| (b.origin.x + b.size.width).round() as i32)
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
