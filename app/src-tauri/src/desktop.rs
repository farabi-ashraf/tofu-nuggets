//! Windows implementation of `icons::DesktopIcons`: Win32 + UI Automation.
//!
//! Approach validated by spikes/hover-detect (Milestone 0): the desktop is a
//! SysListView32 under Progman (or WorkerW after wallpaper-rotation setups);
//! UIA ElementFromPoint identifies the icon under the cursor, and display
//! names resolve to paths against the (possibly OneDrive-redirected) user
//! desktop plus the public desktop. Portable callers go through `crate::icons`;
//! only the (Windows-only) badge layer uses this module directly.
//!
//! E1 extends the hit-test to File Explorer content windows (CabinetWClass).
//! Which surface a hit-test runs against is decided up front from the
//! foreground window (`foreground_surface`): desktop shell, one Explorer window,
//! or neither — in the last case no UIA call runs at all, keeping the engine
//! idle (perf budget, docs/ARCHITECTURE.md). For an Explorer window the hovered
//! item resolves to an absolute path from the item's UIA name plus the window's
//! current folder, read via the E0-proven chain IShellWindows → IShellBrowser
//! (SID_STopLevelBrowser) → IFolderView2. Win11 tabs share one top-level HWND;
//! the active tab is the one whose shell-view window `IsWindowVisible` (E0), and
//! no tab-switch event exists so the folder is read fresh on each hit-test.
//!
//! Infotips: the desktop trick (`suppress_desktop_infotips`, clearing
//! `LVS_EX_INFOTIP`) targets a Win32 SysListView32. Modern Explorer content
//! views (Win10 and Win11) are `DirectUIHWND`, not a ListView, so that message
//! does not apply and there is no per-window infotip toggle to hack around it.
//! Our panel is always-on-top and offset to the item's side, so the native
//! infotip (near the cursor) coexists with it rather than hiding it — hence no
//! Explorer infotip suppression is implemented (see the E1 PR notes).

use std::cell::OnceCell;
use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::{HWND, LPARAM, MAX_PATH, POINT, RECT, WPARAM};
use windows::Win32::System::Com::*;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::icons::{DesktopIcons, Icon, IconRect};

/// Service id for the shell browser behind a ShellWindows item (E0).
const SID_STOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4c96be40_915c_11cf_99d3_00aa004ae837);

/// The surface a cursor hit-test should run against, from the foreground window.
/// `None` means neither the desktop nor an Explorer window is foreground, so no
/// hit-test runs — the engine stays idle (perf budget).
enum Surface {
    Desktop,
    /// Top-level CabinetWClass window under the foreground.
    Explorer(HWND),
    None,
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Classify the foreground window. The desktop shell is Progman (or a WorkerW
/// hosting SHELLDLL_DefView after wallpaper-slideshow setups); a File Explorer
/// content window is CabinetWClass. Common Save/Open dialogs are #32770 and fall
/// through to `None` (they are also absent from IShellWindows — E0).
fn foreground_surface() -> Surface {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return Surface::None;
    }
    match class_of(fg).as_str() {
        "Progman" | "WorkerW" => Surface::Desktop,
        "CabinetWClass" => Surface::Explorer(fg),
        _ => Surface::None,
    }
}

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
    /// Running-Explorer ShellWindows singleton, connected lazily on the first
    /// Explorer hit-test and reused (each hit-test only marshals reads).
    shell: OnceCell<IShellWindows>,
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
            shell: OnceCell::new(),
        })
    }

    /// The ShellWindows singleton, connected once and cached. `None` only if
    /// Explorer is not running (then there are no Explorer windows anyway).
    fn shell_windows(&self) -> Option<&IShellWindows> {
        if let Some(s) = self.shell.get() {
            return Some(s);
        }
        let s: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.ok()?;
        let _ = self.shell.set(s);
        self.shell.get()
    }

    /// Active tab's shell browser + current folder for a top-level CabinetWClass
    /// window. Win11 tabs share one top HWND; the active tab is the one whose
    /// shell-view window is visible (E0), with any matching tab as fallback.
    fn active_browser(&self, top: HWND) -> Option<(IShellBrowser, PathBuf)> {
        let shell = self.shell_windows()?;
        unsafe {
            let count = shell.Count().ok()?;
            let mut fallback = None;
            for i in 0..count {
                let Ok(disp) = shell.Item(&VARIANT::from(i)) else {
                    continue;
                };
                let Ok(sp) = disp.cast::<IServiceProvider>() else {
                    continue;
                };
                let Ok(browser) = sp.QueryService::<IShellBrowser>(&SID_STOP_LEVEL_BROWSER) else {
                    continue;
                };
                let Ok(view_hwnd) = browser.GetWindow() else {
                    continue;
                };
                if GetAncestor(view_hwnd, GA_ROOT).0 != top.0 {
                    continue;
                }
                let Some(folder) = folder_path_of(&browser) else {
                    continue;
                };
                if IsWindowVisible(view_hwnd).as_bool() {
                    return Some((browser, folder));
                }
                fallback = Some((browser, folder));
            }
            fallback
        }
    }

    /// Item under the cursor inside an Explorer content window, resolved to an
    /// absolute path (item's UIA name against the active tab's folder).
    fn explorer_icon_at(&self, x: i32, y: i32, top: HWND) -> Option<Icon> {
        unsafe {
            let el = self.auto.ElementFromPoint(POINT { x, y }).ok()?;
            // Content items are ListItem (icon/list views) or DataItem (details);
            // the nav-pane tree (TreeItem) and breadcrumb (buttons) are excluded.
            let ct = el.CurrentControlType().ok()?;
            if ct != UIA_ListItemControlTypeId && ct != UIA_DataItemControlTypeId {
                return None;
            }
            let name = el.CurrentName().ok()?.to_string();
            if name.is_empty() {
                return None;
            }
            let rect = to_icon_rect(el.CurrentBoundingRectangle().ok()?);
            let (_, folder) = self.active_browser(top)?;
            let path = resolve_path(&name, std::slice::from_ref(&folder));
            Some(Icon { name, rect, path })
        }
    }

    /// Selected item in an Explorer content window (hotkey fallback). Uses the
    /// shell folder view's selection, which yields the filesystem path directly.
    fn explorer_selected(&self, top: HWND) -> Option<Icon> {
        let (browser, _) = self.active_browser(top)?;
        unsafe {
            let view = browser.QueryActiveShellView().ok()?;
            let fv: IFolderView2 = view.cast().ok()?;
            let items: IShellItemArray = fv.Items(_SVGIO(1)).ok()?; // SVGIO_SELECTION
            if items.GetCount().ok()? < 1 {
                return None;
            }
            let it = items.GetItemAt(0).ok()?;
            let disp = it.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let s = disp.to_string().ok()?;
            CoTaskMemFree(Some(disp.0 as *const _));
            let path = PathBuf::from(s);
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(Icon {
                name,
                rect: IconRect::default(),
                path: Some(path),
            })
        }
    }

    /// Existing desktop hit-test (SysListView32 under Progman/WorkerW).
    fn desktop_icon_at(&self, x: i32, y: i32) -> Option<Icon> {
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
    /// Item under the given screen point — a desktop icon or a File Explorer
    /// content item, per the foreground window. Neither foreground → `None`, and
    /// no UIA hit-test runs at all (perf budget).
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon> {
        match foreground_surface() {
            Surface::Desktop => self.desktop_icon_at(x, y),
            Surface::Explorer(top) => self.explorer_icon_at(x, y, top),
            Surface::None => None,
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

    /// Selected item (hotkey fallback): the active Explorer window's selection
    /// when one is foreground, else the selected desktop icon. Preserves the
    /// desktop-selection fallback even when some other app is foreground.
    fn selected_icon(&self) -> Option<Icon> {
        if let Surface::Explorer(top) = foreground_surface() {
            return self.explorer_selected(top);
        }
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

/// Current folder of a shell browser's active view, as a filesystem path.
/// `None` for non-filesystem folders (This PC, Recycle Bin). The PIDL comes from
/// IFolderView2::GetFolder(IPersistFolder2) — casting IShellView straight to
/// IPersistFolder2 fails (E0).
fn folder_path_of(browser: &IShellBrowser) -> Option<PathBuf> {
    unsafe {
        let view = browser.QueryActiveShellView().ok()?;
        let fv: IFolderView2 = view.cast().ok()?;
        let pf: IPersistFolder2 = fv.GetFolder().ok()?;
        let pidl = pf.GetCurFolder().ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let ok = SHGetPathFromIDListW(pidl, &mut buf);
        CoTaskMemFree(Some(pidl as *const _));
        if !ok.as_bool() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
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
