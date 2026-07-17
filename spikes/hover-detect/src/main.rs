//! Milestone 0 spike: prove we can detect which desktop icon is under the
//! cursor via UI Automation, and resolve it to a filesystem path.
//!
//! Modes:
//!   scan            enumerate all desktop icons (name, rect, resolved path)
//!   hover [secs]    poll cursor; print icon under cursor (default 15 s)
//!   simtest         move cursor over icons programmatically and verify
//!                   ElementFromPoint returns the right icon (restores cursor)

use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::*;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Icon {
    name: String,
    rect: RECT,
    path: Option<PathBuf>,
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }

    let mode = std::env::args().nth(1).unwrap_or_else(|| "scan".into());
    let auto: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };

    match mode.as_str() {
        "scan" => scan(&auto).map(|_| ()),
        "hover" => {
            let secs = std::env::args()
                .nth(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(15);
            hover(&auto, secs)
        }
        "simtest" => simtest(&auto),
        other => {
            eprintln!("unknown mode: {other} (use scan | hover [secs] | simtest)");
            std::process::exit(2);
        }
    }
}

/// Locate the desktop's SysListView32. Normally Progman > SHELLDLL_DefView >
/// SysListView32; after wallpaper-rotation setups the DefView moves under a
/// WorkerW window instead.
fn find_desktop_listview() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut defview = FindWindowExW(
            progman,
            HWND::default(),
            w!("SHELLDLL_DefView"),
            PCWSTR::null(),
        )
        .ok();

        if defview.is_none() {
            let mut found: Option<HWND> = None;
            unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
                let mut class = [0u16; 64];
                let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
                if String::from_utf16_lossy(&class[..n]) == "WorkerW" {
                    if let Ok(dv) = unsafe {
                        FindWindowExW(
                            hwnd,
                            HWND::default(),
                            w!("SHELLDLL_DefView"),
                            PCWSTR::null(),
                        )
                    } {
                        *found = Some(dv);
                        return BOOL(0); // stop
                    }
                }
                BOOL(1)
            }
            let _ = EnumWindows(Some(enum_cb), LPARAM(&mut found as *mut _ as isize));
            defview = found;
        }

        FindWindowExW(
            defview?,
            HWND::default(),
            w!("SysListView32"),
            PCWSTR::null(),
        )
        .ok()
    }
}

fn desktop_dirs() -> Vec<PathBuf> {
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

/// Explorer shows icon names with extensions possibly hidden, so match the
/// display name against both the full file name and the stem.
fn resolve_path(display_name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let target = display_name.to_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
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

fn list_icons(auto: &IUIAutomation) -> Result<Vec<Icon>> {
    let lv = find_desktop_listview()
        .ok_or_else(|| {
            Error::new(
                windows::Win32::Foundation::E_FAIL,
                "desktop SysListView32 not found",
            )
        })?;
    let dirs = desktop_dirs();
    let mut icons = Vec::new();
    unsafe {
        let root = auto.ElementFromHandle(lv)?;
        let cond = auto.CreateTrueCondition()?;
        let items = root.FindAll(TreeScope_Children, &cond)?;
        for i in 0..items.Length()? {
            let el = items.GetElement(i)?;
            if el.CurrentControlType()? != UIA_ListItemControlTypeId {
                continue;
            }
            let name = el.CurrentName()?.to_string();
            let rect = el.CurrentBoundingRectangle()?;
            let path = resolve_path(&name, &dirs);
            icons.push(Icon { name, rect, path });
        }
    }
    Ok(icons)
}

fn scan(auto: &IUIAutomation) -> Result<Vec<Icon>> {
    let icons = list_icons(auto)?;
    println!("desktop icons found: {}", icons.len());
    for ic in &icons {
        let r = &ic.rect;
        println!(
            "  [{:>4},{:>4} {:>3}x{:>3}] {:<30} -> {}",
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            ic.name,
            ic.path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(no path match)".into())
        );
    }
    Ok(icons)
}

/// True if `el` is a list item whose parent is the desktop list view.
fn is_desktop_icon(auto: &IUIAutomation, el: &IUIAutomationElement) -> bool {
    unsafe {
        if el
            .CurrentControlType()
            .map(|t| t != UIA_ListItemControlTypeId)
            .unwrap_or(true)
        {
            return false;
        }
        let Ok(walker) = auto.ControlViewWalker() else {
            return false;
        };
        let Ok(parent) = walker.GetParentElement(el) else {
            return false;
        };
        parent
            .CurrentClassName()
            .map(|c| c.to_string() == "SysListView32")
            .unwrap_or(false)
    }
}

fn hover(auto: &IUIAutomation, secs: u64) -> Result<()> {
    println!("hover mode: move the mouse over desktop icons ({secs} s)...");
    let dirs = desktop_dirs();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut last: Option<String> = None;
    while Instant::now() < deadline {
        let mut pt = POINT::default();
        unsafe { GetCursorPos(&mut pt)? };
        let el = unsafe { auto.ElementFromPoint(pt) };
        match el {
            Ok(el) if is_desktop_icon(auto, &el) => {
                let name = unsafe { el.CurrentName()? }.to_string();
                if last.as_deref() != Some(&name) {
                    let path = resolve_path(&name, &dirs);
                    println!(
                        "  over: {:<30} -> {}",
                        name,
                        path.map(|p| p.display().to_string())
                            .unwrap_or_else(|| "(no path match)".into())
                    );
                    last = Some(name);
                }
            }
            _ => {
                if last.take().is_some() {
                    println!("  (left icon)");
                }
            }
        }
        sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn simtest(auto: &IUIAutomation) -> Result<()> {
    let icons = list_icons(auto)?;
    if icons.is_empty() {
        println!("SIMTEST SKIP: no desktop icons to test against");
        return Ok(());
    }
    let mut saved = POINT::default();
    unsafe { GetCursorPos(&mut saved)? };

    let (mut pass, mut covered, mut fail) = (0u32, 0u32, 0u32);
    for ic in &icons {
        let cx = (ic.rect.left + ic.rect.right) / 2;
        let cy = (ic.rect.top + ic.rect.bottom) / 2;
        unsafe { SetCursorPos(cx, cy)? };
        sleep(Duration::from_millis(120));
        let el = unsafe { auto.ElementFromPoint(POINT { x: cx, y: cy }) };
        match el {
            Ok(el) if is_desktop_icon(auto, &el) => {
                let got = unsafe { el.CurrentName() }?.to_string();
                if got == ic.name {
                    pass += 1;
                    println!("  PASS    {:<30} at ({cx},{cy})", ic.name);
                } else {
                    fail += 1;
                    println!("  FAIL    {:<30} got '{got}' instead", ic.name);
                }
            }
            Ok(el) => {
                covered += 1;
                let cls = unsafe { el.CurrentClassName() }
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                println!(
                    "  COVERED {:<30} point hits other window (class '{cls}')",
                    ic.name
                );
            }
            Err(e) => {
                fail += 1;
                println!("  FAIL    {:<30} ElementFromPoint error: {e}", ic.name);
            }
        }
    }
    unsafe { SetCursorPos(saved.x, saved.y)? };

    println!("simtest: {pass} pass, {covered} covered by windows, {fail} fail");
    if pass > 0 && fail == 0 {
        println!("RESULT: GO — hover detection works on visible icons");
    } else if pass == 0 && covered > 0 {
        println!("RESULT: INCONCLUSIVE — all icons covered; clear the desktop and rerun");
    } else {
        println!("RESULT: NO-GO — detection failed on visible icons");
    }
    Ok(())
}
