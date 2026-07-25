//! E0 probe B — UIA item rects across view modes (Win11 26200).
//!
//! Question: for one Explorer window, enumerate item names + bounding rects via UIA
//! in every view mode (extra-large … content). Per mode: rects correct? scrolled-out
//! items absent or present with virtual/clipped rects? Can we tell visible from
//! non-visible (IsOffscreen)? Cost (ms) to enumerate a ~100+ item folder — the pill
//! draws dots from a one-shot snapshot, so this is the click→dots latency floor.
//!
//! Method: pick the CabinetWClass window with the most items (via IShellWindows →
//! IShellBrowser → IFolderView2); read the mode from GetCurrentViewMode. Then UIA:
//! element-from-HWND → locate the items view by CONTROL TYPE (List/DataGrid holding
//! ListItem/DataItem children — skips the breadcrumb + nav tree, no localizable name
//! match) → read Name / BoundingRectangle / IsOffscreen. Times the walk. Default run
//! samples ~35 s and reprints on mode/count change (operator drives Ctrl+Shift+1..8).
//!
//! `cycle` arg drives all 8 view modes itself via SetViewModeAndIconSize — but ONLY
//! on the spike's own temp `probe-folder` window (hard-fenced by folder path); it
//! refuses to modify any real user window. Read-only against everything else.

use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Children, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_DataGridControlTypeId, UIA_DataItemControlTypeId,
    UIA_ListControlTypeId, UIA_ListItemControlTypeId,
};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellWindows, ShellWindows, FOLDERVIEWMODE, FVM_CONTENT,
    FVM_DETAILS, FVM_ICON, FVM_LIST, FVM_SMALLICON, FVM_THUMBNAIL, FVM_THUMBSTRIP, FVM_TILE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GA_ROOT};

const SID_STOP_LEVEL_BROWSER: windows::core::GUID =
    windows::core::GUID::from_u128(0x4c96be40_915c_11cf_99d3_00aa004ae837);

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

fn mode_name(mode: i32) -> &'static str {
    // FOLDERVIEWMODE values (windows crate exposes FVM_* as i32 via u.value).
    match mode {
        m if m == FVM_ICON.0 => "icons (large/xl — size is a slider)",
        m if m == FVM_SMALLICON.0 => "small icons",
        m if m == FVM_LIST.0 => "list",
        m if m == FVM_DETAILS.0 => "details",
        m if m == FVM_THUMBNAIL.0 => "thumbnails",
        m if m == FVM_TILE.0 => "tiles",
        m if m == FVM_THUMBSTRIP.0 => "thumbstrip",
        m if m == FVM_CONTENT.0 => "content",
        _ => "unknown",
    }
}

/// Pick the CabinetWClass Explorer window whose folder has the MOST items (so the
/// latency measurement lands on the 100+ item test folder, not a small one).
fn find_explorer(shell_windows: &IShellWindows) -> Option<(IShellBrowser, HWND)> {
    unsafe {
        let count = shell_windows.Count().ok()?;
        let mut best: Option<(IShellBrowser, HWND)> = None;
        let mut best_items = -1i32;
        for i in 0..count {
            let idx = VARIANT::from(i);
            let Ok(disp) = shell_windows.Item(&idx) else {
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
            let top = GetAncestor(view_hwnd, GA_ROOT);
            if class_name(top) != "CabinetWClass" {
                continue;
            }
            let n = item_count(&browser).unwrap_or(0);
            if n > best_items {
                best_items = n;
                best = Some((browser, top));
            }
        }
        best
    }
}

fn current_mode(browser: &IShellBrowser) -> Option<i32> {
    unsafe {
        let view = browser.QueryActiveShellView().ok()?;
        let fv: IFolderView2 = view.cast().ok()?;
        fv.GetCurrentViewMode().ok().map(|m| m as i32)
    }
}

fn item_count(browser: &IShellBrowser) -> Option<i32> {
    unsafe {
        let view = browser.QueryActiveShellView().ok()?;
        let fv: IFolderView2 = view.cast().ok()?;
        // SVGIO_ALLVIEW = 0 in the shell headers.
        fv.ItemCount(windows::Win32::UI::Shell::_SVGIO(0)).ok()
    }
}

/// Current folder path of a browser (used to fence the `cycle` mode to our own temp
/// window so we never modify a real user Explorer window's view state).
fn folder_path(browser: &IShellBrowser) -> Option<String> {
    unsafe {
        let view = browser.QueryActiveShellView().ok()?;
        let fv: IFolderView2 = view.cast().ok()?;
        let pf: windows::Win32::UI::Shell::IPersistFolder2 = fv.GetFolder().ok()?;
        let pidl = pf.GetCurFolder().ok()?;
        let mut buf = [0u16; 260];
        let ok = windows::Win32::UI::Shell::SHGetPathFromIDListW(pidl, &mut buf);
        windows::Win32::System::Com::CoTaskMemFree(Some(pidl as *const _));
        if ok.as_bool() {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        } else {
            None
        }
    }
}

/// Set the active view mode of a browser's folder view. Only ever called on our own
/// temp window (guarded by folder_path in main).
fn set_view(browser: &IShellBrowser, mode: FOLDERVIEWMODE, icon_size: i32) {
    unsafe {
        if let Ok(view) = browser.QueryActiveShellView() {
            if let Ok(fv) = view.cast::<IFolderView2>() {
                let _ = fv.SetViewModeAndIconSize(mode, icon_size);
            }
        }
    }
}

struct ItemRow {
    name: String,
    l: f64,
    t: f64,
    w: f64,
    h: f64,
    offscreen: bool,
}

/// Locate the folder's items view under the Explorer HWND and enumerate its items.
/// Returns (rows, elapsed_ms, container_found).
///
/// The file list is a List (icon/list views) or DataGrid (details) whose item
/// children are ListItem/DataItem. We match by control type, not name/AutomationId
/// (those localize / drift across builds), and pick the List/DataGrid holding the
/// most item children — that skips the address-bar breadcrumb and the nav-pane tree.
fn enumerate_uia(
    uia: &IUIAutomation,
    top: HWND,
) -> windows::core::Result<(Vec<ItemRow>, f64, bool)> {
    unsafe {
        let start = Instant::now();
        let root = uia.ElementFromHandle(top)?;
        let ct = UIA_ControlTypePropertyId;
        let cond_container = uia.CreateOrCondition(
            &uia.CreatePropertyCondition(ct, &VARIANT::from(UIA_ListControlTypeId.0))?,
            &uia.CreatePropertyCondition(ct, &VARIANT::from(UIA_DataGridControlTypeId.0))?,
        )?;
        let cond_item = uia.CreateOrCondition(
            &uia.CreatePropertyCondition(ct, &VARIANT::from(UIA_ListItemControlTypeId.0))?,
            &uia.CreatePropertyCondition(ct, &VARIANT::from(UIA_DataItemControlTypeId.0))?,
        )?;

        let containers = root.FindAll(TreeScope_Descendants, &cond_container)?;
        let mut best_items: Option<windows::Win32::UI::Accessibility::IUIAutomationElementArray> =
            None;
        let mut best_n = -1i32;
        for i in 0..containers.Length()? {
            let c = containers.GetElement(i)?;
            let items = c.FindAll(TreeScope_Children, &cond_item)?;
            let n = items.Length()?;
            if n > best_n {
                best_n = n;
                best_items = Some(items);
            }
        }

        let mut rows = Vec::new();
        let container_found = best_items.is_some();
        if let Some(items) = best_items {
            for i in 0..items.Length()? {
                let it = items.GetElement(i)?;
                let name = it.CurrentName().map(|s| s.to_string()).unwrap_or_default();
                let (l, t, w, h) = bounding_rect(&it);
                let offscreen = it
                    .CurrentIsOffscreen()
                    .map(|b| b.as_bool())
                    .unwrap_or(false);
                rows.push(ItemRow {
                    name,
                    l,
                    t,
                    w,
                    h,
                    offscreen,
                });
            }
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok((rows, ms, container_found))
    }
}

fn bounding_rect(el: &IUIAutomationElement) -> (f64, f64, f64, f64) {
    unsafe {
        if let Ok(r) = el.CurrentBoundingRectangle() {
            (
                r.left as f64,
                r.top as f64,
                (r.right - r.left) as f64,
                (r.bottom - r.top) as f64,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    }
}

fn report(rows: &[ItemRow], ms: f64, folder_count: Option<i32>, container_found: bool) {
    if !container_found {
        println!("  (items container not found under this HWND)");
        return;
    }
    let onscreen = rows.iter().filter(|r| !r.offscreen).count();
    let offscreen = rows.iter().filter(|r| r.offscreen).count();
    let zero_rect = rows.iter().filter(|r| r.w == 0.0 && r.h == 0.0).count();
    println!(
        "  UIA items: {} (onscreen={onscreen} offscreen={offscreen} zero-rect={zero_rect}); \
         folder ItemCount={}; walk={:.1} ms",
        rows.len(),
        folder_count.map(|c| c.to_string()).unwrap_or("?".into()),
        ms
    );
    // Show a few samples incl. any offscreen ones to see if rects are virtual.
    for r in rows.iter().take(3) {
        println!(
            "    e.g. name={:?} rect=({:.0},{:.0} {:.0}x{:.0}) offscreen={}",
            r.name, r.l, r.t, r.w, r.h, r.offscreen
        );
    }
    if let Some(off) = rows.iter().find(|r| r.offscreen) {
        println!(
            "    offscreen sample: name={:?} rect=({:.0},{:.0} {:.0}x{:.0})",
            off.name, off.l, off.t, off.w, off.h
        );
    }
    if let Some(fc) = folder_count {
        if (fc as usize) > rows.len() {
            println!(
                "    NOTE: folder has {fc} items but UIA exposed {} — virtualization is",
                rows.len()
            );
            println!("          dropping scrolled-out items from the tree entirely.");
        }
    }
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    println!("== probe_b: UIA item rects across view modes ==");
    println!("SETUP: open ONE Explorer window on a folder with 100+ items. During the");
    println!("35 s sampling phase, cycle view modes with Ctrl+Shift+1..8 (xl icons,");
    println!("large, medium, small, list, details, tiles, content). Each mode/change");
    println!("reprints a fresh enumeration.\n");

    let shell_windows: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.expect("ShellWindows");
    let uia: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.expect("CUIAutomation");

    let Some((browser, top)) = find_explorer(&shell_windows) else {
        println!("No CabinetWClass Explorer window found. Open one and rerun.");
        return;
    };
    println!("Target Explorer top_hwnd={:?}\n", top.0);

    // Warm-up timing: the real app keeps one IUIAutomation alive, so per-click cost is
    // a WARM enumeration (ElementFromHandle + FindAll), not the cold first call which
    // pays UIA/tree init. Measure both so the README quotes the honest click latency.
    if let Ok((_, cold_ms, _)) = enumerate_uia(&uia, top) {
        if let Ok((_, warm_ms, _)) = enumerate_uia(&uia, top) {
            println!("timing: cold walk = {cold_ms:.1} ms, warm walk = {warm_ms:.1} ms (warm = per-click cost)\n");
        }
    }

    // `cycle` mode: programmatically set each view mode and enumerate. Hard-fenced to
    // our own temp probe-folder — refuse on any real user window (read-only rule).
    if std::env::args().any(|a| a == "cycle") {
        let path = folder_path(&browser).unwrap_or_default();
        if !path.contains("probe-folder") {
            println!("cycle refused: target folder is {path:?}, not our temp probe-folder.");
            println!("(cycle only ever modifies the spike's own throwaway window.)");
            return;
        }
        println!("cycle: driving all view modes on our own temp window {path:?}\n");
        let modes: &[(&str, FOLDERVIEWMODE, i32)] = &[
            ("extra large icons", FVM_ICON, 256),
            ("large icons", FVM_ICON, 96),
            ("medium icons", FVM_ICON, 48),
            ("small icons", FVM_SMALLICON, 16),
            ("list", FVM_LIST, 16),
            ("details", FVM_DETAILS, 16),
            ("tiles", FVM_TILE, 48),
            ("content", FVM_CONTENT, 32),
        ];
        for (label, mode, size) in modes {
            set_view(&browser, *mode, *size);
            std::thread::sleep(Duration::from_millis(500)); // let the view relayout
            let m = current_mode(&browser).unwrap_or(-1);
            let fc = item_count(&browser);
            println!(
                "-- {label} (icon={size}px): reported mode={m} ({}) --",
                mode_name(m)
            );
            match enumerate_uia(&uia, top) {
                Ok((rows, ms, found)) => report(&rows, ms, fc, found),
                Err(e) => println!("  UIA error: {e:?}"),
            }
            println!();
        }
        println!("-- cycle done --");
        return;
    }

    let sample = |label: &str| {
        let mode = current_mode(&browser).unwrap_or(-1);
        let fc = item_count(&browser);
        println!("-- {label}: view mode = {} ({}) --", mode, mode_name(mode));
        match enumerate_uia(&uia, top) {
            Ok((rows, ms, found)) => report(&rows, ms, fc, found),
            Err(e) => println!("  UIA error: {e:?}"),
        }
    };

    sample("initial");

    let start = Instant::now();
    let mut last_mode = current_mode(&browser).unwrap_or(-1);
    let mut last_count = item_count(&browser).unwrap_or(-1);
    while start.elapsed() < Duration::from_secs(35) {
        std::thread::sleep(Duration::from_millis(500));
        let m = current_mode(&browser).unwrap_or(-1);
        let c = item_count(&browser).unwrap_or(-1);
        if m != last_mode || c != last_count {
            println!();
            sample(&format!("t={:>4.1}s change", start.elapsed().as_secs_f32()));
            last_mode = m;
            last_count = c;
        }
    }
    println!("\n-- done --");
}
