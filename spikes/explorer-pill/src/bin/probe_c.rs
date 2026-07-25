//! E0 probe C — pill z-order trick (Win11 26200).
//!
//! Question: a tiny bare Win32 popup owned by an Explorer HWND (via
//! SetWindowLongPtr GWLP_HWNDPARENT). Does it stay above that Explorer window but
//! below other apps when Explorer loses focus? Does it survive Explorer moving /
//! minimizing / closing without crashing? Any interference with Explorer? Fallback
//! (`probe_c hook`): a NON-owned window whose z-order is re-asserted just above the
//! Explorer HWND by a WinEvent hook on EVENT_SYSTEM_FOREGROUND.
//!
//! Method: target the spike's own `probe-folder` window if open (raw EnumWindows by
//! title) so transitions are safe to script; else fall back to the first
//! CabinetWClass in OBSERVE-only mode. Create a 70x34 WS_POPUP over its bottom-right;
//! in owned mode set the owner and let the OS order it. Each timer tick, walk the
//! top-level z-order once and print the ranks of {our window, Explorer, foreground}.
//! When driving our own window it scripts minimize→restore→move→close (targeted
//! Win32 messages to one known HWND — never keystroke injection) and logs that the
//! popup survives each. Guards IsWindow before every Explorer touch so a close can't
//! crash us. Read-only against any real user window (only ever acts on probe-folder).

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, HBRUSH, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

// Shared with the WinEvent hook callback (fallback mode).
static OUR_HWND: AtomicIsize = AtomicIsize::new(0);
static EXPLORER_HWND: AtomicIsize = AtomicIsize::new(0);
static TICKS: AtomicU32 = AtomicU32::new(0);
static HOOK_MODE: AtomicIsize = AtomicIsize::new(0); // 1 = fallback hook mode
static DRIVE: AtomicIsize = AtomicIsize::new(0); // 1 = target is our own temp window; script transitions

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n as usize])
}

// Prefer our own "probe-folder" window (so transitions are safe to script); if it is
// not open, fall back to the first CabinetWClass window in PASSIVE mode (observe only).
extern "system" fn enum_find_ours(hwnd: HWND, _l: LPARAM) -> windows::core::BOOL {
    unsafe {
        if IsWindowVisible(hwnd).as_bool()
            && class_name(hwnd) == "CabinetWClass"
            && window_title(hwnd).contains("probe-folder")
        {
            EXPLORER_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            DRIVE.store(1, Ordering::SeqCst);
            return false.into();
        }
    }
    true.into()
}

extern "system" fn enum_find_explorer(hwnd: HWND, _l: LPARAM) -> windows::core::BOOL {
    unsafe {
        if IsWindowVisible(hwnd).as_bool() && class_name(hwnd) == "CabinetWClass" {
            EXPLORER_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            return false.into(); // stop enumeration
        }
    }
    true.into()
}

fn explorer_hwnd() -> HWND {
    HWND(EXPLORER_HWND.load(Ordering::SeqCst) as *mut _)
}

fn our_hwnd() -> HWND {
    HWND(OUR_HWND.load(Ordering::SeqCst) as *mut _)
}

/// Rank of each hwnd in the top-level z-order (0 = topmost). Returns -1 if not found.
fn zorder_ranks(targets: &[HWND]) -> Vec<i32> {
    let mut ranks = vec![-1i32; targets.len()];
    unsafe {
        let mut cur = GetTopWindow(None).unwrap_or(HWND(std::ptr::null_mut()));
        let mut rank = 0i32;
        while !cur.is_invalid() {
            for (i, t) in targets.iter().enumerate() {
                if cur.0 == t.0 {
                    ranks[i] = rank;
                }
            }
            rank += 1;
            match GetWindow(cur, GW_HWNDNEXT) {
                Ok(next) if !next.is_invalid() => cur = next,
                _ => break,
            }
        }
    }
    ranks
}

fn reassert_zorder() {
    // Fallback: place our window immediately above Explorer in z-order.
    let ex = explorer_hwnd();
    let our = our_hwnd();
    unsafe {
        if IsWindow(Some(ex)).as_bool() && IsWindow(Some(our)).as_bool() {
            let _ = SetWindowPos(
                our,
                Some(ex),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _idobject: i32,
    _idchild: i32,
    _thread: u32,
    _time: u32,
) {
    reassert_zorder();
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let brush: HBRUSH = CreateSolidBrush(COLORREF(0x00AA5500)); // BGR: orange-ish
                FillRect(hdc, &ps.rcPaint, brush);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let n = TICKS.fetch_add(1, Ordering::SeqCst) + 1;
                if HOOK_MODE.load(Ordering::SeqCst) == 1 {
                    reassert_zorder();
                }
                if DRIVE.load(Ordering::SeqCst) == 1 {
                    scripted_action(n);
                }
                sample(n);
                let limit = if DRIVE.load(Ordering::SeqCst) == 1 {
                    16
                } else {
                    25
                };
                if n >= limit {
                    let _ = KillTimer(Some(hwnd), 1);
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// Scripted transitions on OUR OWN probe-folder window only, to test whether the
// owned popup survives minimize / restore / move / close without crashing. Targeted
// Win32 calls to one known HWND — no keystroke injection, no user window touched.
fn scripted_action(tick: u32) {
    let ex = explorer_hwnd();
    unsafe {
        if !IsWindow(Some(ex)).as_bool() {
            return;
        }
        match tick {
            3 => {
                println!("  >> ACTION: minimize our Explorer window");
                let _ = ShowWindow(ex, SW_MINIMIZE);
            }
            6 => {
                println!("  >> ACTION: restore our Explorer window");
                let _ = ShowWindow(ex, SW_RESTORE);
            }
            9 => {
                println!("  >> ACTION: move our Explorer window (+40,+40)");
                let mut r = RECT::default();
                let _ = GetWindowRect(ex, &mut r);
                let _ = SetWindowPos(
                    ex,
                    None,
                    r.left + 40,
                    r.top + 40,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            12 => {
                println!("  >> ACTION: close our Explorer window (WM_CLOSE)");
                let _ = PostMessageW(Some(ex), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            _ => {}
        }
    }
}

fn sample(n: u32) {
    let ex = explorer_hwnd();
    let our = our_hwnd();
    unsafe {
        let ex_alive = IsWindow(Some(ex)).as_bool();
        let ex_iconic = ex_alive && IsIconic(ex).as_bool();
        let our_vis = IsWindowVisible(our).as_bool();
        let fg = GetForegroundWindow();
        let ranks = zorder_ranks(&[our, ex, fg]);
        let (our_r, ex_r, fg_r) = (ranks[0], ranks[1], ranks[2]);
        let above = if our_r >= 0 && ex_r >= 0 {
            if our_r < ex_r {
                "our ABOVE explorer"
            } else {
                "our BELOW explorer"
            }
        } else {
            "n/a"
        };
        let fg_class = if fg.is_invalid() {
            "-".into()
        } else {
            class_name(fg)
        };
        println!(
            "[tick {n:>2}] explorer(alive={ex_alive} iconic={ex_iconic}) our_vis={our_vis} \
             ranks[our={our_r} explorer={ex_r} fg={fg_r}] {above} | fg_class={fg_class}"
        );
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let hook_mode = mode == "hook";
    HOOK_MODE.store(if hook_mode { 1 } else { 0 }, Ordering::SeqCst);

    println!("== probe_c: pill z-order trick ==");
    println!(
        "mode: {}",
        if hook_mode {
            "FALLBACK (non-owned + WinEvent EVENT_SYSTEM_FOREGROUND re-assert)"
        } else {
            "OWNED (SetWindowLongPtr GWLP_HWNDPARENT = Explorer)"
        }
    );
    println!("SETUP: prefers our own 'probe-folder' window (then it scripts");
    println!("minimize/restore/move/close on it automatically). If that window is not");
    println!("open it falls back to the first Explorer window in OBSERVE-only mode —");
    println!("then drive the transitions yourself and watch ranks + no-crash.\n");

    unsafe {
        // Pass 1: our own temp window (safe to drive). Pass 2: any Explorer (passive).
        let _ = EnumWindows(Some(enum_find_ours), LPARAM(0));
        if EXPLORER_HWND.load(Ordering::SeqCst) == 0 {
            let _ = EnumWindows(Some(enum_find_explorer), LPARAM(0));
        }
    }
    let ex = explorer_hwnd();
    if ex.is_invalid() {
        println!("No CabinetWClass Explorer window found. Open one and rerun.");
        return;
    }
    println!("Target Explorer hwnd={:?}", ex.0);

    unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        let cls = w!("ExplorerPillProbeC");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: cls,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Position over Explorer's bottom-right corner.
        let mut r = RECT::default();
        let _ = GetWindowRect(ex, &mut r);
        let (px, py) = (r.right - 90, r.bottom - 60);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            cls,
            w!("pill"),
            WS_POPUP,
            px,
            py,
            70,
            34,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .expect("CreateWindowExW");
        OUR_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        if !hook_mode {
            // OWNED mode: set the owner to the Explorer window.
            windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::WIN32_ERROR(0));
            let prev = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, ex.0 as isize);
            let err = windows::Win32::Foundation::GetLastError();
            println!(
                "SetWindowLongPtr(GWLP_HWNDPARENT) prev={prev} err={:?}; owner now={:?}",
                err,
                GetWindow(hwnd, GW_OWNER)
                    .map(|h| h.0)
                    .unwrap_or(std::ptr::null_mut())
            );
        }

        let mut hook = HWINEVENTHOOK::default();
        if hook_mode {
            hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            println!("WinEvent hook installed: {:?}", hook.0);
        }

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), 1, 1000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if hook_mode && !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
    }
    println!("\n-- done (clean exit; Explorer untouched) --");
}
