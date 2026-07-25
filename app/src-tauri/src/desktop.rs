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
//! current folder, read via the chain IShellWindows → IShellBrowser
//! (SID_STopLevelBrowser) → IFolderView2.
//!
//! Two things the E0 spike could not verify, pinned on real hardware (E1).
//! Apartment: this chain is STA-only — from an MTA thread
//! `IShellBrowser::GetWindow` fails (0x8001010D) and nothing resolves, so the
//! worker threads init COM as STA (`init_com_for_thread`). Hit target:
//! `ElementFromPoint` lands on a child of the row (a column cell / label), not
//! the item, so `item_ancestor` climbs to the ListItem/DataItem.
//!
//! Win11 tabs each surface as their own IShellBrowser sharing one top HWND; the
//! active tab is the one whose view window contains the cursor
//! (`WindowFromPoint`) — visibility does not distinguish them reliably. No
//! tab-switch event exists, so the folder is read fresh on each hit-test.
//!
//! E2 adds `explorer_windows()` for the pill layer, which enumerates the same
//! chain but must name the active tab of a window it is neither hovering nor
//! focusing — so it cannot use the cursor at all. It probes a point in the
//! middle of the frame's content area and walks *down* the child-window tree
//! (`ChildWindowFromPointEx`), which answers "what does this frame put here"
//! independently of z-order, unlike `WindowFromPoint`. Still no tab-switch
//! event, so `pill.rs` polls.
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
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
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

/// Whether `child` is `ancestor` or nested under it (walking parent windows).
fn is_descendant(mut child: HWND, ancestor: HWND) -> bool {
    if child.is_invalid() {
        return false;
    }
    for _ in 0..32 {
        if child.0 == ancestor.0 {
            return true;
        }
        match unsafe { GetParent(child) } {
            Ok(p) if !p.is_invalid() => child = p,
            _ => return false,
        }
    }
    false
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
    /// window. Win11 tabs share one top HWND and each tab is its own
    /// IShellBrowser; the active tab is the one whose shell-view window contains
    /// the cursor point (the covered tabs' views never do). Falls back to a
    /// visible matching tab, then any matching tab, so a cursor that is not over
    /// the view (e.g. selection fallback with the pointer on the toolbar) still
    /// resolves.
    fn active_browser(&self, top: HWND, x: i32, y: i32) -> Option<(IShellBrowser, PathBuf)> {
        let shell = self.shell_windows()?;
        let at_cursor = unsafe { WindowFromPoint(POINT { x, y }) };
        unsafe {
            let count = shell.Count().ok()?;
            let mut visible_match = None;
            let mut any_match = None;
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
                if is_descendant(at_cursor, view_hwnd) {
                    return Some((browser, folder)); // cursor is in this tab's view = active tab
                }
                if visible_match.is_none() && IsWindowVisible(view_hwnd).as_bool() {
                    visible_match = Some((browser.clone(), folder.clone()));
                }
                if any_match.is_none() {
                    any_match = Some((browser, folder));
                }
            }
            visible_match.or(any_match)
        }
    }

    /// Nearest ListItem/DataItem at or above `el`. ElementFromPoint returns the
    /// deepest element under the cursor, which in Explorer's modern view is a
    /// child of the row (the label Text or the icon Image), so we climb the
    /// control tree to the item itself. Bounded so a miss can't walk to the root.
    fn item_ancestor(&self, el: IUIAutomationElement) -> Option<IUIAutomationElement> {
        let walker = unsafe { self.auto.ControlViewWalker() }.ok()?;
        let mut cur = el;
        for _ in 0..6 {
            let ct = unsafe { cur.CurrentControlType() }.ok()?;
            if ct == UIA_ListItemControlTypeId || ct == UIA_DataItemControlTypeId {
                return Some(cur);
            }
            cur = unsafe { walker.GetParentElement(&cur) }.ok()?;
        }
        None
    }

    /// Item under the cursor inside an Explorer content window, resolved to an
    /// absolute path (item's UIA name against the active tab's folder).
    fn explorer_icon_at(&self, x: i32, y: i32, top: HWND) -> Option<Icon> {
        unsafe {
            let hit = self.auto.ElementFromPoint(POINT { x, y }).ok()?;
            // Content items are ListItem (icon/list views) or DataItem (details);
            // ElementFromPoint may land on a child, so climb to the item. The
            // nav-pane tree (TreeItem) and breadcrumb (buttons) never reach one.
            let item = self.item_ancestor(hit)?;
            let name = item.CurrentName().ok()?.to_string();
            if name.is_empty() {
                return None;
            }
            let rect = to_icon_rect(item.CurrentBoundingRectangle().ok()?);
            let (_, folder) = self.active_browser(top, x, y)?;
            let path = resolve_path(&name, std::slice::from_ref(&folder));
            Some(Icon { name, rect, path })
        }
    }

    /// Selected item in an Explorer content window (hotkey fallback). Uses the
    /// shell folder view's selection, which yields the filesystem path directly.
    fn explorer_selected(&self, top: HWND) -> Option<Icon> {
        let (x, y) = cursor_pos().unwrap_or((0, 0));
        let (browser, _) = self.active_browser(top, x, y)?;
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

/// One live File Explorer content window, as the pill layer sees it (E2).
pub struct ExplorerWindow {
    /// Top-level CabinetWClass window — the pill's owner.
    pub top: HWND,
    /// Active tab's SHELLDLL_DefView window — the item area proper. NOT
    /// `IShellBrowser::GetWindow`, which returns the whole ShellTabWindowClass
    /// (nav pane and status bar included); a pill placed against that rect
    /// lands on top of the status bar's view-mode buttons.
    pub view: HWND,
    /// Active tab's current folder.
    pub folder: PathBuf,
}

thread_local! {
    /// Per-thread ShellWindows singleton for the free functions below (the
    /// hover path keeps its own inside `DesktopUia`). COM objects are
    /// apartment-bound, so this must not be a process-wide static.
    static SHELL_WINDOWS: OnceCell<IShellWindows> = const { OnceCell::new() };
}

fn with_shell_windows<T>(f: impl FnOnce(&IShellWindows) -> T) -> Option<T> {
    SHELL_WINDOWS.with(|cell| {
        if cell.get().is_none() {
            let s: IShellWindows =
                unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.ok()?;
            let _ = cell.set(s);
        }
        cell.get().map(f)
    })
}

/// Deepest visible child window of `top` at a screen point, found by walking
/// down the parent/child tree rather than by z-order.
///
/// `WindowFromPoint` would answer "which window is on screen there", so any
/// other app covering Explorer breaks it; `ChildWindowFromPointEx` is relative
/// to a parent and ignores unrelated processes entirely, which is what lets the
/// pill identify the active tab of a window it is not hovering.
fn deepest_child_at(top: HWND, pt_screen: POINT) -> HWND {
    let mut cur = top;
    for _ in 0..16 {
        let mut p = pt_screen;
        unsafe {
            if !ScreenToClient(cur, &mut p).as_bool() {
                break;
            }
            let child = ChildWindowFromPointEx(
                cur,
                p,
                CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
            );
            if child.is_invalid() || child.0 == cur.0 {
                break;
            }
            cur = child;
        }
    }
    cur
}

/// A point inside the content area of an Explorer frame: three-quarters across
/// (clear of the navigation pane) and halfway down (clear of the toolbar and
/// the status bar).
fn content_probe_point(top: HWND) -> Option<POINT> {
    unsafe {
        let mut rc = RECT::default();
        GetClientRect(top, &mut rc).ok()?;
        if rc.right <= rc.left || rc.bottom <= rc.top {
            return None;
        }
        let mut pt = POINT {
            x: rc.left + (rc.right - rc.left) * 3 / 4,
            y: rc.top + (rc.bottom - rc.top) / 2,
        };
        if !ClientToScreen(top, &mut pt).as_bool() {
            return None;
        }
        Some(pt)
    }
}

/// Every File Explorer content window with a filesystem folder open, one entry
/// per top-level window, reporting that window's ACTIVE tab.
///
/// Active-tab detection cannot reuse E1's method: that one asks which tab's
/// view contains the cursor, and the pill has no cursor to assume (it must be
/// right in an unfocused, un-hovered window). `IsWindowVisible` on the tab
/// views is not a reliable discriminator either — E1 found covered tabs
/// reporting visible, which resolved the wrong folder. What is used instead is
/// geometric and cursor-free: probe a point in the middle of the frame's
/// content area and walk *down* the child-window tree to whatever occupies it
/// (`deepest_child_at`); only the active tab's view is mapped there, so the
/// browser whose view is that window's ancestor is the active tab. The
/// visible-view and first-match fallbacks below only run if that probe lands
/// nowhere useful (e.g. a frame too small to have a content area).
///
/// No tab-switch or navigation event exists, so callers poll (see `pill.rs`).
/// Non-filesystem folders (This PC, Recycle Bin) yield no path and are skipped
/// — there is nothing there to annotate or count.
pub fn explorer_windows() -> Vec<ExplorerWindow> {
    let mut out: Vec<ExplorerWindow> = Vec::new();
    with_shell_windows(|shell| unsafe {
        let Ok(count) = shell.Count() else {
            return;
        };
        // (top hwnd, best-so-far entry, whether that entry came from the probe)
        let mut best: Vec<(isize, ExplorerWindow, bool)> = Vec::new();
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
            let Ok(tab) = browser.GetWindow() else {
                continue;
            };
            let top = GetAncestor(tab, GA_ROOT);
            if top.is_invalid() || class_of(top) != "CabinetWClass" {
                continue;
            }
            let Some(folder) = folder_path_of(&browser) else {
                continue;
            };
            // Tab identity is the browser's own window (it hosts everything in
            // the tab); the pill's anchor is the narrower item area inside it.
            let view = browser
                .QueryActiveShellView()
                .and_then(|v| v.GetWindow())
                .unwrap_or(tab);
            let active = content_probe_point(top)
                .map(|pt| is_descendant(deepest_child_at(top, pt), tab))
                .unwrap_or(false);
            let entry = ExplorerWindow { top, view, folder };
            match best.iter_mut().find(|(t, _, _)| *t == top.0 as isize) {
                // A probe hit is definitive; anything else is a placeholder
                // that a later probe hit (or nothing) replaces.
                Some(slot) if active && !slot.2 => *slot = (top.0 as isize, entry, true),
                Some(slot) if !slot.2 && IsWindowVisible(view).as_bool() => {
                    slot.1 = entry;
                }
                Some(_) => {}
                None => best.push((top.0 as isize, entry, active)),
            }
        }
        out = best.into_iter().map(|(_, e, _)| e).collect();
    });
    out
}

/// Whether a File Explorer content window is the foreground window. The pill
/// layer polls its folder/count only while this holds (perf budget).
pub fn explorer_is_foreground() -> bool {
    matches!(foreground_surface(), Surface::Explorer(_))
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

pub fn virtual_screen_height() -> i32 {
    unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }
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

/// Dump what UIA sees under the cursor + the foreground surface + (for Explorer)
/// the resolved active folder. Logged by the editor when the hotkey found no
/// target, so an Explorer miss is diagnosable from tofu.log instead of guessed.
pub fn debug_cursor_chain() -> Option<String> {
    let (x, y) = cursor_pos()?;
    let fg = unsafe { GetForegroundWindow() };
    let fclass = class_of(fg);
    let mut out = format!("cursor chain @({x},{y}) fg_class={fclass:?}\n");
    unsafe {
        let Ok(auto) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        else {
            out.push_str("  (IUIAutomation create failed)\n");
            return Some(out);
        };
        if let Ok(el) = auto.ElementFromPoint(POINT { x, y }) {
            if let Ok(walker) = auto.ControlViewWalker() {
                let mut cur = Some(el);
                let mut depth = 0;
                while let Some(c) = cur {
                    if depth > 8 {
                        break;
                    }
                    let ct = c.CurrentControlType().map(|t| t.0).unwrap_or(-1);
                    let name = c.CurrentName().map(|s| s.to_string()).unwrap_or_default();
                    let cls = c
                        .CurrentClassName()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "  [{depth}] ct={ct} class={cls:?} name={name:?}\n"
                    ));
                    cur = walker.GetParentElement(&c).ok();
                    depth += 1;
                }
            }
        } else {
            out.push_str("  (ElementFromPoint failed)\n");
        }
    }
    if fclass == "CabinetWClass" {
        out.push_str(&dump_shell_tabs(fg, x, y));
    }
    Some(out)
}

/// Diagnostic: every IShellWindows entry that belongs to the foreground
/// Explorer window, with its folder, view HWND, visibility, and whether the
/// cursor is inside that tab's view. Shows whether Win11 tabs surface as
/// separate entries and which one active_browser should pick.
fn dump_shell_tabs(top: HWND, x: i32, y: i32) -> String {
    let mut out = String::new();
    let at_cursor = unsafe { WindowFromPoint(POINT { x, y }) };
    out.push_str(&format!(
        "  WindowFromPoint({x},{y}) = {:?} class={:?}\n",
        at_cursor.0,
        class_of(at_cursor)
    ));
    let shell: IShellWindows = match unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) } {
        Ok(s) => s,
        Err(e) => return format!("  ShellWindows create failed: {e}\n"),
    };
    unsafe {
        let count = shell.Count().unwrap_or(0);
        out.push_str(&format!("  ShellWindows.Count = {count}\n"));
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
            let view = browser.GetWindow().ok();
            let vtop = view.map(|v| GetAncestor(v, GA_ROOT).0 as isize);
            let matches = vtop == Some(top.0 as isize);
            let visible = view.map(|v| IsWindowVisible(v).as_bool());
            let cursor_in = view.map(|v| is_descendant(at_cursor, v)).unwrap_or(false);
            let folder = folder_path_of(&browser)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".into());
            out.push_str(&format!(
                "  [{i}] match_fg={matches} view={:?} visible={visible:?} cursor_in_view={cursor_in} folder={folder}\n",
                view.map(|v| v.0)
            ));
        }
    }
    out
}

/// COM apartment for the UIA/shell worker threads (hover, hotkey, badge layer).
///
/// Single-threaded (STA), not multithreaded. The Explorer chain is
/// apartment-affine: from an MTA thread `IShellBrowser::GetWindow` fails with
/// `RPC_E_CANTCALLOUT_ININPUTSYNCCALL` (0x8001010D), so the active tab's window
/// and folder never resolve and Explorer hover/hotkey find nothing (E1 spike
/// probe_e1: same call returns Ok under STA). UIA clients run fine in an STA,
/// and the badge layer already pumps a message loop, so STA suits all three.
pub fn init_com_for_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}
