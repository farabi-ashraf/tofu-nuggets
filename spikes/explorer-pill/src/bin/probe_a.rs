//! E0 probe A — Explorer window + tab enumeration (Win11 26200).
//!
//! Question: enumerate open Explorer windows; per window get the current folder
//! path + top-level HWND. Critical unknown = Win11 TABS: do tabs show as separate
//! `IShellWindows` entries, can we read the ACTIVE tab's folder, and what changes
//! when the user switches tabs. Also: how to tell an Explorer content window from a
//! Save/Open dialog reusing the same view classes.
//!
//! Method: `IShellWindows` (CLSID_ShellWindows) → per item, `IServiceProvider` →
//! `IShellBrowser` (SID_STopLevelBrowser). From the browser: the shell-view HWND
//! (→ GA_ROOT top window + class), and the active shell view → `IFolderView2` →
//! current PIDL → path. Candidate active-tab discriminator: the shell-view window of
//! an inactive tab is hidden, so `IsWindowVisible` on the view HWND should flag the
//! active tab. Prints an initial snapshot, then samples for 20 s so the operator can
//! switch tabs and we log what changed (no event assumed — polled).
//!
//! Read-only: never moves/closes/modifies Explorer. Prints evidence to stdout.

use std::time::{Duration, Instant};
use windows::core::{Interface, GUID};
use windows::Win32::Foundation::{HWND, MAX_PATH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView2, IPersistFolder2, IShellBrowser, IShellWindows, SHGetPathFromIDListW, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, IsWindowVisible, GA_ROOT,
};

// SID_STopLevelBrowser — service id for the shell browser behind a ShellWindows item.
const SID_STOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4c96be40_915c_11cf_99d3_00aa004ae837);

struct WinInfo {
    top_hwnd: HWND,
    view_hwnd: HWND,
    class: String,
    path: String,
    view_visible: bool,
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

fn path_from_browser(browser: &IShellBrowser) -> windows::core::Result<String> {
    unsafe {
        let view = browser.QueryActiveShellView()?;
        // The current folder's PIDL comes from the view's IFolderView2, not the view
        // itself (IShellView does not implement IPersistFolder2).
        let fv: IFolderView2 = view.cast()?;
        let pf: IPersistFolder2 = fv.GetFolder()?;
        let pidl: *mut ITEMIDLIST = pf.GetCurFolder()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let ok = SHGetPathFromIDListW(pidl, &mut buf);
        // Free the PIDL that GetCurFolder allocated.
        windows::Win32::System::Com::CoTaskMemFree(Some(pidl as *const _));
        if ok.as_bool() {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Ok(String::from_utf16_lossy(&buf[..len]))
        } else {
            // Non-filesystem folder (This PC, Recycle Bin, etc.) — no path.
            Ok("<non-filesystem>".to_string())
        }
    }
}

fn enumerate(shell_windows: &IShellWindows) -> Vec<WinInfo> {
    let mut out = Vec::new();
    unsafe {
        let count = shell_windows.Count().unwrap_or(0);
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
            let top_hwnd = GetAncestor(view_hwnd, GA_ROOT);
            let class = class_name(top_hwnd);
            let path = path_from_browser(&browser).unwrap_or_else(|_| "<err>".into());
            let view_visible = IsWindowVisible(view_hwnd).as_bool();
            out.push(WinInfo {
                top_hwnd,
                view_hwnd,
                class,
                path,
                view_visible,
            });
        }
    }
    out
}

fn signature(list: &[WinInfo]) -> String {
    list.iter()
        .map(|w| {
            format!(
                "{:?}|{}|{}|{}",
                w.top_hwnd.0, w.class, w.path, w.view_visible
            )
        })
        .collect::<Vec<_>>()
        .join("  ;  ")
}

fn print_snapshot(list: &[WinInfo]) {
    if list.is_empty() {
        println!("  (no Explorer/IShellWindows entries)");
        return;
    }
    for (i, w) in list.iter().enumerate() {
        println!(
            "  [{i}] top_hwnd={:?} class={:<16} view_hwnd={:?} view_visible={:<5} path={}",
            w.top_hwnd.0, w.class, w.view_hwnd.0, w.view_visible, w.path
        );
    }
    // Group by top-level HWND — multiple entries sharing one HWND == Win11 tabs.
    let mut hwnds: Vec<isize> = list.iter().map(|w| w.top_hwnd.0 as isize).collect();
    hwnds.sort_unstable();
    hwnds.dedup();
    println!(
        "  => {} IShellWindows entries across {} distinct top-level window(s)",
        list.len(),
        hwnds.len()
    );
    for h in &hwnds {
        let shared = list
            .iter()
            .filter(|w| w.top_hwnd.0 as isize == *h)
            .collect::<Vec<_>>();
        if shared.len() > 1 {
            let visible = shared.iter().filter(|w| w.view_visible).count();
            println!(
                "     hwnd {h:#x}: {} entries share it (TABS) — {visible} report view_visible=true",
                shared.len()
            );
        }
    }
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    println!("== probe_a: Explorer window + tab enumeration ==");
    println!("SETUP: open at least one File Explorer window. To test tabs, open ONE");
    println!("window with 3 tabs on 3 different folders, then switch tabs during the");
    println!("20 s sampling phase below.\n");

    let shell_windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }
        .expect("CoCreateInstance(ShellWindows)");

    let first = enumerate(&shell_windows);
    println!("-- initial snapshot --");
    print_snapshot(&first);

    // Dialog-vs-Explorer note: common Save/Open dialogs (#32770) are NOT tracked by
    // IShellWindows, so they never appear here. Any entry above is a real Explorer
    // browser; the class column proves it (CabinetWClass on Win11).
    println!("\n-- Save/Open dialog check --");
    println!("Open a Save/Open dialog now if you want; IShellWindows will still only");
    println!("list real Explorer windows (dialogs are #32770, not tracked). Classes");
    println!("above should all read CabinetWClass.\n");

    println!("-- sampling 20 s for tab-switch changes (switch tabs now) --");
    let mut last_sig = signature(&first);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(400));
        let now = enumerate(&shell_windows);
        let sig = signature(&now);
        if sig != last_sig {
            println!(
                "\n[t={:>5.1}s] CHANGE detected:",
                start.elapsed().as_secs_f32()
            );
            print_snapshot(&now);
            last_sig = sig;
        }
    }
    println!("\n-- done. If nothing printed during sampling, tab switches produced no");
    println!("   change in enumeration/paths/visibility (polling only; no event). --");
}
