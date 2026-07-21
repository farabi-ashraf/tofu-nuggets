//! Windows implementation of `icons::DesktopIcons`: Win32 + UI Automation.
//!
//! Approach validated by spikes/hover-detect (Milestone 0): the desktop is a
//! SysListView32 under Progman (or WorkerW after wallpaper-rotation setups);
//! UIA ElementFromPoint identifies the icon under the cursor, and display
//! names resolve to paths against the (possibly OneDrive-redirected) user
//! desktop plus the public desktop. Portable callers go through `crate::icons`;
//! only the (Windows-only) badge layer uses this module directly.

use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::icons::{DesktopIcons, Icon, IconRect};

fn to_icon_rect(r: RECT) -> IconRect {
    IconRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

pub struct DesktopUia {
    auto: IUIAutomation,
    dirs: Vec<PathBuf>,
}

// IUIAutomation is apartment-bound in principle, but we confine each instance
// to the thread that created it; DesktopUia is not Send/Sync and each worker
// thread creates its own.
impl DesktopUia {
    /// Caller must have initialized COM on this thread.
    pub fn new() -> Result<Self> {
        let auto: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
        Ok(Self {
            auto,
            dirs: desktop_dirs(),
        })
    }

    fn is_desktop_icon(&self, el: &IUIAutomationElement) -> bool {
        unsafe {
            if el
                .CurrentControlType()
                .map(|t| t != UIA_ListItemControlTypeId)
                .unwrap_or(true)
            {
                return false;
            }
            let Ok(walker) = self.auto.ControlViewWalker() else {
                return false;
            };
            let Ok(parent) = walker.GetParentElement(el) else {
                return false;
            };
            parent
                .CurrentClassName()
                .map(|c| c == "SysListView32")
                .unwrap_or(false)
        }
    }
}

impl DesktopIcons for DesktopUia {
    /// Icon under the given screen point, if that point is a desktop icon.
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon> {
        unsafe {
            let el = self.auto.ElementFromPoint(POINT { x, y }).ok()?;
            if !self.is_desktop_icon(&el) {
                return None;
            }
            let name = el.CurrentName().ok()?.to_string();
            let rect = to_icon_rect(el.CurrentBoundingRectangle().ok()?);
            let path = resolve_path(&name, &self.dirs);
            Some(Icon { name, rect, path })
        }
    }

    /// All desktop icons (used by the badge layer).
    fn list_icons(&self) -> std::result::Result<Vec<Icon>, String> {
        let lv = find_desktop_listview().ok_or("desktop SysListView32 not found")?;
        let mut icons = Vec::new();
        let scan = |icons: &mut Vec<Icon>| -> Result<()> {
            unsafe {
                let root = self.auto.ElementFromHandle(lv)?;
                let cond = self.auto.CreateTrueCondition()?;
                let items = root.FindAll(TreeScope_Children, &cond)?;
                for i in 0..items.Length()? {
                    let el = items.GetElement(i)?;
                    if el.CurrentControlType()? != UIA_ListItemControlTypeId {
                        continue;
                    }
                    let name = el.CurrentName()?.to_string();
                    let rect = to_icon_rect(el.CurrentBoundingRectangle()?);
                    let path = resolve_path(&name, &self.dirs);
                    icons.push(Icon { name, rect, path });
                }
            }
            Ok(())
        };
        scan(&mut icons).map_err(|e| e.to_string())?;
        Ok(icons)
    }

    /// Currently selected desktop icon, if any (UIA selection pattern).
    fn selected_icon(&self) -> Option<Icon> {
        let lv = find_desktop_listview()?;
        unsafe {
            let root = self.auto.ElementFromHandle(lv).ok()?;
            let pat: IUIAutomationSelectionPattern =
                root.GetCurrentPatternAs(UIA_SelectionPatternId).ok()?;
            let sel = pat.GetCurrentSelection().ok()?;
            if sel.Length().ok()? < 1 {
                return None;
            }
            let el = sel.GetElement(0).ok()?;
            let name = el.CurrentName().ok()?.to_string();
            let rect = to_icon_rect(el.CurrentBoundingRectangle().ok()?);
            let path = resolve_path(&name, &self.dirs);
            Some(Icon { name, rect, path })
        }
    }
}

pub fn new_icons() -> std::result::Result<DesktopUia, String> {
    DesktopUia::new().map_err(|e| e.to_string())
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    Some((pt.x, pt.y))
}

pub fn virtual_screen_width() -> i32 {
    unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }
}

pub fn init_thread() {
    init_com_for_thread();
}

pub fn find_desktop_listview() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut defview =
            FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();

        if defview.is_none() {
            let mut found: Option<HWND> = None;
            unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
                let mut class = [0u16; 64];
                let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
                if String::from_utf16_lossy(&class[..n]) == "WorkerW" {
                    if let Ok(dv) = unsafe {
                        FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                    } {
                        *found = Some(dv);
                        return BOOL(0);
                    }
                }
                BOOL(1)
            }
            let _ = EnumWindows(Some(enum_cb), LPARAM(&mut found as *mut _ as isize));
            defview = found;
        }

        FindWindowExW(Some(defview?), None, w!("SysListView32"), PCWSTR::null()).ok()
    }
}

/// Suppress the desktop's native icon infotips (the Explorer tooltip that
/// otherwise pops over our panel) by clearing `LVS_EX_INFOTIP` on the desktop
/// ListView. Desktop-only and reverted when Explorer restarts — we re-apply
/// on a timer. Returns whether the listview was found.
///
/// `LVM_SETEXTENDEDLISTVIEWSTYLE = 0x1036`, `LVS_EX_INFOTIP = 0x0400`; passing
/// the mask with a zero value clears just that bit.
pub fn suppress_desktop_infotips() -> bool {
    const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
    const LVS_EX_INFOTIP: usize = 0x0400;
    let Some(lv) = find_desktop_listview() else {
        return false;
    };
    unsafe {
        SendMessageW(
            lv,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(LVS_EX_INFOTIP)),
            Some(LPARAM(0)),
        );
    }
    true
}

pub fn desktop_dirs() -> Vec<PathBuf> {
    unsafe fn known(id: *const GUID) -> Option<PathBuf> {
        let pw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
        let s = unsafe { pw.to_string() }.ok()?;
        unsafe { CoTaskMemFree(Some(pw.as_ptr() as _)) };
        Some(PathBuf::from(s))
    }
    let mut dirs = Vec::new();
    unsafe {
        if let Some(d) = known(&FOLDERID_Desktop) {
            dirs.push(d);
        }
        if let Some(d) = known(&FOLDERID_PublicDesktop) {
            dirs.push(d);
        }
    }
    dirs
}

use crate::icons::resolve_path;

/// UIA needs no permission grant on Windows: `None` means "not applicable",
/// which the settings UI renders as no accessibility row at all.
pub fn accessibility_trusted() -> Option<bool> {
    None
}

pub fn open_accessibility_settings() {}

/// Only macOS needs the "what was actually under the cursor" dump; UIA
/// detection is stable enough not to have needed one.
pub fn debug_cursor_chain() -> Option<String> {
    None
}

pub fn init_com_for_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}
