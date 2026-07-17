//! Desktop icon interop via Win32 + UI Automation.
//!
//! Approach validated by spikes/hover-detect (Milestone 0): the desktop is a
//! SysListView32 under Progman (or WorkerW after wallpaper-rotation setups);
//! UIA ElementFromPoint identifies the icon under the cursor, and display
//! names resolve to paths against the (possibly OneDrive-redirected) user
//! desktop plus the public desktop.

use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug)]
pub struct DesktopIcon {
    pub name: String,
    pub rect: RECT,
    pub path: Option<PathBuf>,
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

    /// Icon under the given screen point, if that point is a desktop icon.
    pub fn icon_at(&self, pt: POINT) -> Option<DesktopIcon> {
        unsafe {
            let el = self.auto.ElementFromPoint(pt).ok()?;
            if !self.is_desktop_icon(&el) {
                return None;
            }
            let name = el.CurrentName().ok()?.to_string();
            let rect = el.CurrentBoundingRectangle().ok()?;
            let path = resolve_path(&name, &self.dirs);
            Some(DesktopIcon { name, rect, path })
        }
    }

    /// All desktop icons (used by the badge layer).
    pub fn list_icons(&self) -> Result<Vec<DesktopIcon>> {
        let lv = find_desktop_listview().ok_or_else(|| {
            Error::new(
                windows::Win32::Foundation::E_FAIL,
                "desktop SysListView32 not found",
            )
        })?;
        let mut icons = Vec::new();
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
                let rect = el.CurrentBoundingRectangle()?;
                let path = resolve_path(&name, &self.dirs);
                icons.push(DesktopIcon { name, rect, path });
            }
        }
        Ok(icons)
    }

    /// Currently selected desktop icon, if any (UIA selection pattern).
    pub fn selected_icon(&self) -> Option<DesktopIcon> {
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
            let rect = el.CurrentBoundingRectangle().ok()?;
            let path = resolve_path(&name, &self.dirs);
            Some(DesktopIcon { name, rect, path })
        }
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

/// True while the desktop itself is the foreground window (icons visible and
/// interactive) — the gate for badge visibility and fast hover polling.
pub fn desktop_is_foreground() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(fg, &mut class) as usize;
        matches!(
            String::from_utf16_lossy(&class[..n]).as_str(),
            "Progman" | "WorkerW"
        )
    }
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

/// Explorer may hide extensions, so match the display name against both the
/// full file name and the stem.
pub fn resolve_path(display_name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let target = display_name.to_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(full) = p.file_name().map(|f| f.to_string_lossy().to_lowercase()) else {
                continue;
            };
            if full == target {
                return Some(p);
            }
            if let Some(stem) = p.file_stem() {
                if stem.to_string_lossy().to_lowercase() == target {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn init_com_for_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}
