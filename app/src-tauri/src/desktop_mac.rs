//! macOS implementation of `icons::DesktopIcons`: Accessibility (AX) API.
//!
//! Mirror of the Windows UIA approach: a system-wide AX hit-test
//! (`AXUIElementCopyElementAtPosition`, the `ElementFromPoint` analogue)
//! identifies the element under the cursor, on the Finder desktop **and inside
//! Finder browser windows** (M5, mirroring the Windows Explorer work in
//! `desktop.rs`). Display/item names resolve to paths (`icons::resolve_path`)
//! against the desktop roots or, in a Finder window, that window's folder.
//!
//! Perf gate (mirrors Windows `foreground_surface`): before any AX hit-test,
//! `finder_frontmost` checks whether Finder is the focused application
//! (`AXFocusedApplication`). When it is not — any other app is frontmost — the
//! hit-test never runs, so the engine stays idle (budget: ~0% CPU when neither
//! the desktop nor a Finder window is foreground, docs/ARCHITECTURE.md). This
//! matches Windows, where the desktop hit-test only runs while `Progman`/a
//! `CabinetWClass` window is foreground: "desktop foreground" on macOS means
//! Finder is frontmost with the desktop as the active surface.
//!
//! Desktop-vs-window routing is by AX *shape*, not window size. The Finder
//! desktop has no `AXWindow` (see below); a Finder browser window is an
//! `AXWindow`. So a hit whose ancestor chain contains an `AXWindow` is resolved
//! against that window's folder, and one without is the desktop. This replaces
//! the earlier display-size heuristic (`covers_a_display` on the hit chain),
//! which mis-classified a *maximized* icon-view Finder window as the desktop and
//! fired the panel over the empty space between its icons (M5 fix). Sizing is
//! still used only to locate the desktop's own container for enumeration.
//!
//! Finder window folder comes from the window's `AXDocument` — a `file://` URL
//! Finder keeps pointed at the **active tab's** folder. Finder renders only the
//! active tab (inactive tabs are not in the AX tree), so a multi-tab window
//! resolves the front tab directly, with no cursor-in-view disambiguation of the
//! kind Explorer needs (there each tab is a live HWND). That is the one place
//! macOS AX is simpler than the Win32 shell chain.
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
//! Two ways in, both landing on the same Finder icon container: the hit-test
//! above (hover, hotkey-under-cursor) and, for `list_icons`/`selected_icon`,
//! a walk down from Finder's application element — found through its pid in
//! the CoreGraphics window list, since those must work with the pointer
//! nowhere near an icon.
//!
//! Shape observed on macOS 26 (from a hardware AX dump, not documentation):
//!
//! ```text
//! AXApplication "Finder"
//!   └ AXScrollArea "desktop"   display-sized, sits directly among the app's
//!     └ AXGroup "Desktop"      children — NOT inside an AXWindow
//!       └ the icon elements    also display-sized
//! ```
//!
//! Both wrappers answer to a name and a frame, so they look like icons unless
//! rejected (`is_container`) — an earlier version reported a phantom "Desktop"
//! icon for bare wallpaper, which also stopped the hotkey ever falling back to
//! the selection. Depths are not hard-coded: the walk descends through
//! display-sized containers until it finds item-shaped children.

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
        pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
        pub fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
        pub fn CFArrayGetValueAtIndex(array: CFTypeRef, idx: CFIndex) -> CFTypeRef;
        pub fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        pub fn CFNumberGetValue(
            number: CFTypeRef,
            the_type: CFIndex,
            value: *mut c_void,
        ) -> Boolean;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub static kAXTrustedCheckOptionPrompt: CFStringRef;

        pub static kCGWindowOwnerName: CFStringRef;
        pub static kCGWindowOwnerPID: CFStringRef;

        pub fn AXIsProcessTrusted() -> Boolean;
        pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
        pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        pub fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
        pub fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
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

/// A CFString → Rust String.
fn cf_string_value(s: CFTypeRef) -> Option<String> {
    let mut buf = [0u8; 1024];
    let ok = unsafe {
        CFStringGetCString(
            s,
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

/// Read a CFString attribute into a Rust String.
fn string_attr(elem: CFTypeRef, name: &str) -> Option<String> {
    cf_string_value(copy_attr(elem, name)?.0)
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

/// Active display bounds, in points. No pixel dimensions are read anywhere in
/// this module on purpose — see the units note in the module header.
fn displays() -> Vec<CGRect> {
    let mut ids = [0 as CGDirectDisplayID; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return Vec::new();
    }
    ids[..count as usize]
        .iter()
        .map(|&id| unsafe { CGDisplayBounds(id) })
        .collect()
}

/// How far up the AX tree to look for the desktop container. Finder nests the
/// desktop a few levels deep and the exact depth is not contractual.
const MAX_DEPTH: usize = 8;

/// How far *down* from Finder's windows to hunt for the icon container.
const SEARCH_DEPTH: usize = 3;

/// Take our own reference to a CF object we only borrowed (array/dictionary
/// members are owned by their container, so wrapping one in `CfOwned` without
/// retaining it would over-release).
fn retained(ptr: CFTypeRef) -> Option<CfOwned> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CFRetain(ptr) };
    CfOwned::new(ptr)
}

fn array_items(arr: CFTypeRef) -> Vec<CfOwned> {
    let count = unsafe { CFArrayGetCount(arr) };
    (0..count)
        .filter_map(|i| retained(unsafe { CFArrayGetValueAtIndex(arr, i) }))
        .collect()
}

/// Children of an AX element, empty when it has none or is not readable.
fn children(elem: CFTypeRef) -> Vec<CfOwned> {
    copy_attr(elem, "AXChildren")
        .map(|kids| array_items(kids.0))
        .unwrap_or_default()
}

/// Finder's process id, read from the window list — Finder always owns the
/// desktop window. Not cached: Finder can be relaunched, and the callers here
/// run at most once per hotkey press or badge refresh.
fn finder_pid() -> Option<i32> {
    // kCGWindowListOptionAll, kCGNullWindowID.
    let list = CfOwned::new(unsafe { CGWindowListCopyWindowInfo(0, 0) })?;
    for win in array_items(list.0) {
        let owner = unsafe { CFDictionaryGetValue(win.0, kCGWindowOwnerName) };
        if owner.is_null() || cf_string_value(owner).as_deref() != Some("Finder") {
            continue;
        }
        let pid_ref = unsafe { CFDictionaryGetValue(win.0, kCGWindowOwnerPID) };
        if pid_ref.is_null() {
            continue;
        }
        let mut pid: i32 = 0;
        // kCFNumberSInt32Type
        let ok = unsafe { CFNumberGetValue(pid_ref, 3, &mut pid as *mut _ as *mut c_void) };
        if ok != 0 && pid > 0 {
            return Some(pid);
        }
    }
    None
}

/// The element whose children are the desktop icons.
///
/// Observed shape on macOS 26 (hardware, see the module header): the icons sit
/// two levels below Finder's application element, and an earlier version that
/// stopped at the `AXScrollArea` enumerated its single `AXGroup` child instead
/// of any icons. Rather than hard-code that depth, descend through
/// display-sized containers until reaching the element that actually holds
/// item-shaped children — which survives Finder rearranging its tree again.
fn desktop_icon_container() -> Option<CfOwned> {
    let app = CfOwned::new(unsafe { AXUIElementCreateApplication(finder_pid()?) })?;
    children(app.0)
        .into_iter()
        .filter(|top| covers_a_display(top.0))
        .find_map(|top| find_icon_container(top.0, SEARCH_DEPTH))
}

fn find_icon_container(elem: CFTypeRef, depth: usize) -> Option<CfOwned> {
    let kids = children(elem);
    if kids
        .iter()
        .any(|k| !is_container(k.0) && element_name(k.0).is_some())
    {
        return retained(elem);
    }
    if depth == 0 {
        return None;
    }
    kids.into_iter()
        .find_map(|k| find_icon_container(k.0, depth - 1))
}

/// First selected child of `elem` that looks like a desktop item.
fn selection_in(elem: CFTypeRef, dirs: &[PathBuf]) -> Option<Icon> {
    let selected = copy_attr(elem, "AXSelectedChildren")?;
    array_items(selected.0)
        .into_iter()
        .find_map(|kid| icon_from(kid.0, dirs))
}

/// Is this a container (desktop/window chrome) rather than an item?
///
/// On the desktop the container answers `AXTitle` too — "Desktop" — and pointing
/// at bare wallpaper hits it, which produced a phantom icon by that name and,
/// worse, counted as a hit so the hotkey never fell back to the selection.
/// Inside a Finder window the item-holding views (`AXList`/`AXTable`/`AXOutline`
/// in list & details, `AXBrowser` in column view) play the same role: a hit on
/// the empty space between items lands on one of them, and rejecting them is
/// what keeps the panel from firing there (M5). Desktop icons are also never
/// display-sized, so geometry rejects the desktop scroll area/group as well.
fn is_container(elem: CFTypeRef) -> bool {
    matches!(
        string_attr(elem, "AXRole").as_deref(),
        Some("AXScrollArea")
            | Some("AXWindow")
            | Some("AXApplication")
            | Some("AXList")
            | Some("AXTable")
            | Some("AXOutline")
            | Some("AXBrowser")
    ) || covers_a_display(elem)
}

/// An icon element → `Icon`. Items without a name are skipped: they are the
/// container's own scrollbars and decorations, not desktop items.
fn icon_from(elem: CFTypeRef, dirs: &[PathBuf]) -> Option<Icon> {
    if is_container(elem) {
        return None;
    }
    let name = element_name(elem)?;
    let f = frame_pts(elem)?;
    Some(Icon {
        rect: IconRect {
            left: f.origin.x.round() as i32,
            top: f.origin.y.round() as i32,
            right: (f.origin.x + f.size.width).round() as i32,
            bottom: (f.origin.y + f.size.height).round() as i32,
        },
        path: resolve_path(&name, dirs),
        name,
    })
}

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
        .any(|b| area >= 0.8 * (b.size.width * b.size.height))
}

/// Is this hit on the desktop's icon container? Only reached once routing has
/// ruled out a Finder browser window (no `AXWindow` in the chain), so it just
/// confirms the hit is inside Finder's icon `AXScrollArea` — not, say, the menu
/// bar or a Spotlight overlay that also lives window-less under the app. The
/// desktop's `AXScrollArea` is the same container `desktop_icon_container` finds
/// for enumeration.
fn chain_is_desktop(chain: &[CfOwned]) -> bool {
    chain
        .iter()
        .any(|e| string_attr(e.0, "AXRole").as_deref() == Some("AXScrollArea"))
}

/// The nearest `AXWindow` at or above the hit element, marking a hit inside a
/// Finder *browser* window. The desktop has no `AXWindow` (module header), so
/// its absence routes the hit to the desktop instead. Routing by role — not by
/// size — is what stops a maximized icon-view window reading as the desktop.
fn window_in_chain(elem: &CfOwned, chain: &[CfOwned]) -> Option<CfOwned> {
    std::iter::once(elem)
        .chain(chain.iter())
        .find(|e| string_attr(e.0, "AXRole").as_deref() == Some("AXWindow"))
        .and_then(|w| retained(w.0))
}

/// Current folder of a Finder browser window, from its `AXDocument` (a `file://`
/// URL Finder keeps pointed at the active tab's folder). `None` for a window
/// that shows no filesystem folder (Recents, Tags, AirDrop) or has no document.
fn finder_window_folder(win: CFTypeRef) -> Option<PathBuf> {
    file_url_to_path(&string_attr(win, "AXDocument")?)
}

/// A `file://` URL → path, percent-decoding escapes Finder puts in `AXDocument`
/// (spaces become `%20`, and so on). Local file URLs have an empty host —
/// `file:///Users/x` — with an optional `localhost`.
fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    (!decoded.is_empty()).then(|| PathBuf::from(decoded))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Pid of the focused application (the frontmost app), for the perf gate.
fn frontmost_pid(system_wide: CFTypeRef) -> Option<i32> {
    let app = copy_attr(system_wide, "AXFocusedApplication")?;
    let mut pid: i32 = 0;
    let err = unsafe { AXUIElementGetPid(app.0, &mut pid) };
    (err == kAXErrorSuccess && pid > 0).then_some(pid)
}

/// The nearest named, non-container element at or above the hit — the item the
/// cursor is on — resolved against `dirs`. The element actually hit may be the
/// icon image, its text label, or the row wrapping both, and only some carry the
/// name, so take the closest one with both a name and a frame. Container levels
/// (the scroll area / window / table itself) are excluded so bare background
/// between items resolves to nothing instead of the container's own title.
fn item_from_hit(elem: &CfOwned, chain: &[CfOwned], dirs: &[PathBuf]) -> Option<Icon> {
    let (name, f) = std::iter::once(elem)
        .chain(chain.iter().take(2))
        .filter(|e| !is_container(e.0))
        .find_map(|e| Some((element_name(e.0)?, frame_pts(e.0)?)))?;
    Some(Icon {
        rect: IconRect {
            left: f.origin.x.round() as i32,
            top: f.origin.y.round() as i32,
            right: (f.origin.x + f.size.width).round() as i32,
            bottom: (f.origin.y + f.size.height).round() as i32,
        },
        path: resolve_path(&name, dirs),
        name,
    })
}

/// First selected item inside a Finder browser window, resolved against its
/// folder (hotkey selection fallback). Which element answers
/// `AXSelectedChildren` varies by view — the scroll area, or the
/// outline/table/browser inside it — so search a few levels down.
fn window_selection(win: CFTypeRef, depth: usize, dirs: &[PathBuf]) -> Option<Icon> {
    if let Some(icon) = selection_in(win, dirs) {
        return Some(icon);
    }
    if depth == 0 {
        return None;
    }
    children(win)
        .into_iter()
        .find_map(|k| window_selection(k.0, depth - 1, dirs))
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
    // Perf gate + routing verdict: distinguishes "hotkey found nothing because
    // Finder was not frontmost" from "found nothing under the cursor", and shows
    // which surface (desktop vs which Finder window folder) the hit routed to.
    let fpid = frontmost_pid(system_wide.0);
    let finder = finder_pid();
    let gate = matches!((fpid, finder), (Some(a), Some(b)) if a == b);
    let chain = ancestor_chain(elem.0);
    let route = match window_in_chain(&elem, &chain) {
        Some(win) => match finder_window_folder(win.0) {
            Some(folder) => format!("Finder window → folder {}", folder.display()),
            None => "Finder window → no AXDocument folder (Recents/Tags/etc.?)".into(),
        },
        None if chain_is_desktop(&chain) => "desktop".into(),
        None => "neither (no AXWindow, no desktop AXScrollArea)".into(),
    };
    let mut out = format!(
        "gate: frontmost pid={fpid:?} finder pid={finder:?} finder_frontmost={gate}\n\
         route: {route}\n\
         AX chain at ({xp:.0},{yp:.0}) pts:\n  0: {}",
        describe(elem.0)
    );
    for (i, node) in chain.iter().enumerate() {
        out.push_str(&format!("\n  {}: {}", i + 1, describe(node.0)));
    }
    out.push_str(&format!("\n{}", debug_finder_tree()));
    Some(out)
}

/// Finder's window/container shape, for when enumeration comes back empty.
/// The hover hit-test needed two hardware rounds to match Finder's real
/// structure; this exists so enumeration needs fewer.
fn debug_finder_tree() -> String {
    let Some(pid) = finder_pid() else {
        return "Finder tree: process not found in window list".into();
    };
    let Some(app) = CfOwned::new(unsafe { AXUIElementCreateApplication(pid) }) else {
        return format!("Finder tree: no AX element for pid {pid}");
    };
    let mut out = format!("Finder tree (pid {pid}):");
    for (i, win) in children(app.0).iter().enumerate() {
        let role = string_attr(win.0, "AXRole").unwrap_or_else(|| "?".into());
        let title = string_attr(win.0, "AXTitle").unwrap_or_else(|| "-".into());
        let spans = covers_a_display(win.0);
        // AXDocument is where a Finder browser window's active-tab folder comes
        // from; showing it (raw URL + resolved path) is how the tabs path is
        // verified on hardware.
        let doc = string_attr(win.0, "AXDocument")
            .map(|u| {
                let folder = finder_window_folder(win.0)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<unresolved>".into());
                format!("{u} → {folder}")
            })
            .unwrap_or_else(|| "-".into());
        let kids = children(win.0)
            .iter()
            .map(|k| string_attr(k.0, "AXRole").unwrap_or_else(|| "?".into()))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(
            "\n  win {i}: {role} \"{title}\" spans-display={spans} doc={doc} children=[{kids}]"
        ));
    }
    match desktop_icon_container() {
        Some(container) => {
            let kids = children(container.0);
            out.push_str(&format!(
                "\n  container: {} \"{}\" with {} children",
                string_attr(container.0, "AXRole").unwrap_or_else(|| "?".into()),
                string_attr(container.0, "AXTitle").unwrap_or_else(|| "-".into()),
                kids.len()
            ));
            // First few children decide whether enumeration sees real icons.
            for kid in kids.iter().take(3) {
                out.push_str(&format!(
                    "\n    child: {} \"{}\" container={}",
                    string_attr(kid.0, "AXRole").unwrap_or_else(|| "?".into()),
                    element_name(kid.0).unwrap_or_else(|| "-".into()),
                    is_container(kid.0)
                ));
            }
        }
        None => out.push_str("\n  container NOT found"),
    }
    out
}

impl MacIcons {
    /// Perf gate: is Finder the frontmost application? Any other app frontmost
    /// means neither the desktop nor a Finder window is the active surface, so
    /// no hit-test runs (budget). Mirrors the Windows `foreground_surface`
    /// class check.
    fn finder_frontmost(&self) -> bool {
        matches!(
            (frontmost_pid(self.system_wide.0), finder_pid()),
            (Some(a), Some(b)) if a == b
        )
    }

    /// AX element under the cursor (points), or `None` when the hit-test fails
    /// (permission missing, or nothing there).
    fn hit(&self, x: i32, y: i32) -> Option<CfOwned> {
        let mut raw: AXUIElementRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyElementAtPosition(self.system_wide.0, x as f32, y as f32, &mut raw)
        };
        (err == kAXErrorSuccess)
            .then(|| CfOwned::new(raw))
            .flatten()
    }

    /// The frontmost Finder browser window and its folder, if one is focused.
    /// The desktop is not an `AXWindow`, so when the desktop is the active
    /// surface Finder reports no focused window (or a non-`AXWindow`) and this
    /// returns `None` — the caller then uses the desktop selection instead.
    fn focused_finder_window(&self) -> Option<(CfOwned, PathBuf)> {
        if !self.finder_frontmost() {
            return None;
        }
        let app = CfOwned::new(unsafe { AXUIElementCreateApplication(finder_pid()?) })?;
        let win = copy_attr(app.0, "AXFocusedWindow")?;
        if string_attr(win.0, "AXRole").as_deref() != Some("AXWindow") {
            return None;
        }
        let folder = finder_window_folder(win.0)?;
        Some((win, folder))
    }
}

impl DesktopIcons for MacIcons {
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon> {
        // Perf gate: no AX hit-test unless Finder is the active surface.
        if !self.finder_frontmost() {
            return None;
        }
        let elem = self.hit(x, y)?;
        let chain = ancestor_chain(elem.0);

        // Route by shape: an AXWindow ancestor = a Finder browser window
        // (resolve against its folder); none = the desktop (resolve against the
        // desktop roots). See the module header for why this is not a size test.
        match window_in_chain(&elem, &chain) {
            Some(win) => {
                let folder = finder_window_folder(win.0)?;
                item_from_hit(&elem, &chain, std::slice::from_ref(&folder))
            }
            None => {
                if !chain_is_desktop(&chain) {
                    return None;
                }
                item_from_hit(&elem, &chain, &self.dirs)
            }
        }
    }

    fn list_icons(&self) -> Result<Vec<Icon>, String> {
        let container = desktop_icon_container().ok_or("desktop icon container not found")?;
        Ok(children(container.0)
            .into_iter()
            .filter_map(|kid| icon_from(kid.0, &self.dirs))
            .collect())
    }

    fn selected_icon(&self) -> Option<Icon> {
        // A Finder browser window in front → its selection, resolved against its
        // folder. Mirrors the Windows `selected_icon` Explorer branch.
        if let Some((win, folder)) = self.focused_finder_window() {
            if let Some(icon) =
                window_selection(win.0, SEARCH_DEPTH + 1, std::slice::from_ref(&folder))
            {
                return Some(icon);
            }
        }
        // Desktop selection fallback, kept regardless of foreground (as on
        // Windows): the hotkey over empty desktop still targets the selection.
        let container = desktop_icon_container()?;
        if let Some(icon) = selection_in(container.0, &self.dirs) {
            return Some(icon);
        }
        // Which element answers AXSelectedChildren is not contractual — the
        // group holding the icons, or the scroll area above it.
        let parent = copy_attr(container.0, "AXParent")?;
        selection_in(parent.0, &self.dirs)
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
        .map(|b| (b.origin.x + b.size.width).round() as i32)
        .max()
        .unwrap_or(i32::MAX)
}

/// Bottom-most edge across displays, in points (panel bottom-flip bound).
pub fn virtual_screen_height() -> i32 {
    displays()
        .iter()
        .map(|b| (b.origin.y + b.size.height).round() as i32)
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

#[cfg(test)]
mod tests {
    use super::{file_url_to_path, percent_decode};
    use std::path::PathBuf;

    #[test]
    fn plain_file_url_becomes_path() {
        assert_eq!(
            file_url_to_path("file:///Users/x/Documents"),
            Some(PathBuf::from("/Users/x/Documents"))
        );
    }

    #[test]
    fn trailing_slash_is_kept_but_harmless() {
        // Finder's AXDocument on a folder ends in a slash; resolve_path reads the
        // dir either way, so the exact trailing slash does not matter.
        assert_eq!(
            file_url_to_path("file:///Users/x/Documents/"),
            Some(PathBuf::from("/Users/x/Documents/"))
        );
    }

    #[test]
    fn percent_escapes_are_decoded() {
        // Spaces and other escapes Finder puts in the URL come back as bytes.
        assert_eq!(
            file_url_to_path("file:///Users/x/My%20Great%20Folder"),
            Some(PathBuf::from("/Users/x/My Great Folder"))
        );
    }

    #[test]
    fn localhost_host_is_stripped() {
        assert_eq!(
            file_url_to_path("file://localhost/Users/x"),
            Some(PathBuf::from("/Users/x"))
        );
    }

    #[test]
    fn non_file_scheme_is_rejected() {
        assert_eq!(file_url_to_path("x-apple.finder:///Recents"), None);
        assert_eq!(file_url_to_path(""), None);
    }

    #[test]
    fn unicode_percent_bytes_round_trip() {
        // "café" → the é is UTF-8 %C3%A9; decoding must reassemble the bytes.
        assert_eq!(
            file_url_to_path("file:///Users/x/caf%C3%A9"),
            Some(PathBuf::from("/Users/x/café"))
        );
    }

    #[test]
    fn dangling_percent_is_left_literal() {
        // A stray '%' with no two hex digits after it is passed through as-is
        // rather than dropped or panicking.
        assert_eq!(percent_decode("abc%"), "abc%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zz"), "a%zz");
    }
}
