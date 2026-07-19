//! Spike (docs/V0.1.1.md A2): can a window live in the desktop's own z-order —
//! above the icons (SHELLDLL_DefView), below every application window — by
//! reparenting it under Progman/WorkerW?
//!
//! Modes (first CLI arg):
//!   ulw      — per-pixel-alpha layered window via UpdateLayeredWindow (ideal)
//!   child    — same but switched to WS_CHILD after SetParent
//!   alpha    — diagnostic: constant-alpha half-transparent sheet
//!              (SetLayeredWindowAttributes) — "does ANYTHING render there?"
//!   colorkey — GDI WM_PAINT drawing + colorkey transparency (fallback path)
//!
//! Draws bright magenta probe dots at fixed physical screen coordinates so an
//! external screenshot can pixel-test: dots over icons, covered by app
//! windows, no foreground gating. Logs every step. Ctrl+C to quit.

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const DOT_R: i32 = 16;
/// Probe dot centers in *physical* screen coordinates.
const PROBES: [(i32, i32); 3] = [(120, 120), (500, 300), (900, 600)];
const TIMER_ID: usize = 7;
const TIMER_MS: u32 = 200;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ulw,
    Child,
    Alpha,
    Colorkey,
    /// No transparency tricks at all: 600x600 opaque window, magenta fill.
    /// If even this doesn't show, the desktop band ignores foreign windows.
    Plain,
    /// Architecture candidate: full-screen non-layered window shaped by
    /// SetWindowRgn to just the dot circles — transparency without
    /// WS_EX_LAYERED (which the desktop band refuses to composite).
    Region,
    /// Architecture candidate 2: one tiny plain rectangular window per badge
    /// (the only thing the band composites), DWM-rounded corners for shape,
    /// WM_NCHITTEST -> HTTRANSPARENT for click-through.
    Dots,
}

struct Ctx {
    parent: HWND,
    mode: Mode,
}

fn main() -> Result<()> {
    let apply_rgn_to_plain = std::env::args().any(|a| a == "rgn");
    let mode = match std::env::args().nth(1).as_deref() {
        None | Some("ulw") => Mode::Ulw,
        Some("child") => Mode::Child,
        Some("alpha") => Mode::Alpha,
        Some("colorkey") => Mode::Colorkey,
        Some("plain") => Mode::Plain,
        Some("region") => Mode::Region,
        Some("dots") => Mode::Dots,
        Some(m) => panic!("unknown mode {m}"),
    };
    // Second arg: which window to reparent into ("progman" default | "defview").
    let into_defview = std::env::args().nth(2).as_deref() == Some("defview");
    unsafe {
        // Physical pixels everywhere; without this GetSystemMetrics returns
        // DPI-virtualized sizes (observed: 1638x1024 on a 2560x1600 @125%).
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        let (parent, defview) = find_desktop_chain().expect("desktop chain not found");
        log_window("parent", parent);
        log_window("defview", defview);

        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let class_name = w!("TofuSpikeBadgeReparent");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        println!("screen {w}x{h}");

        if mode == Mode::Dots {
            let ctx = Box::into_raw(Box::new(Ctx { parent, mode }));
            for (i, (cx, cy)) in PROBES.iter().enumerate() {
                let hwnd = CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class_name,
                    w!("Tofu spike badge dot"),
                    WS_POPUP,
                    cx - DOT_R,
                    cy - DOT_R,
                    DOT_R * 2,
                    DOT_R * 2,
                    None,
                    None,
                    Some(hinstance.into()),
                    None,
                )?;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);
                // Round the corners as far as DWM allows — on a dot-sized
                // window this reads as a circle-ish badge. Must happen while
                // the window is still an ordinary top-level.
                use windows::Win32::Graphics::Dwm::{
                    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                };
                let pref = DWMWCP_ROUND;
                let hr = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref as *const _ as *const core::ffi::c_void,
                    std::mem::size_of_val(&pref) as u32,
                );
                let old = SetParent(hwnd, Some(defview));
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    cx - DOT_R,
                    cy - DOT_R,
                    DOT_R * 2,
                    DOT_R * 2,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )?;
                println!(
                    "dot {i} at ({cx},{cy}): {:?} old_parent={:?} corner_hr={:?}",
                    hwnd, old, hr
                );
            }
            println!("-- sibling chain of {:?} (top to bottom):", defview);
            let mut c = GetWindow(defview, GW_CHILD).ok();
            while let Some(ch) = c {
                log_window("  child", ch);
                c = GetWindow(ch, GW_HWNDNEXT).ok();
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            return Ok(());
        }

        let ex_style = match mode {
            Mode::Plain => WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            // NOTE: WS_EX_TRANSPARENT tested as a composition killer in this
            // band — click-through must come from another mechanism.
            Mode::Region => WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            _ => WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        };
        let env_i32 = |k: &str, d: i32| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        let (win_x, win_y) = if mode == Mode::Plain {
            (env_i32("TOFU_X", 0), env_i32("TOFU_Y", 0))
        } else {
            (0, 0)
        };
        let (win_w, win_h) = if mode == Mode::Plain {
            (env_i32("TOFU_W", 600), env_i32("TOFU_H", 600))
        } else {
            (w, h)
        };
        println!("window geometry ({win_x},{win_y}) {win_w}x{win_h}");
        // Same style recipe as the real badge layer, minus TOPMOST.
        let hwnd = CreateWindowExW(
            ex_style,
            class_name,
            w!("Tofu spike badge layer"),
            WS_POPUP,
            win_x,
            win_y,
            win_w,
            win_h,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        println!("created layered window {:?}", hwnd);

        // The experiment: adopt the desktop's z-band.
        let target = if into_defview { defview } else { parent };
        let old_parent = SetParent(hwnd, Some(target));
        println!(
            "SetParent into {} -> old parent {:?}",
            if into_defview { "defview" } else { "progman" },
            old_parent
        );

        if !matches!(mode, Mode::Ulw | Mode::Plain | Mode::Region) {
            // Proper child style (Win8+ supports WS_EX_LAYERED on children).
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let new_style = (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize;
            SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
            println!("style switched WS_POPUP -> WS_CHILD");
        }

        match mode {
            Mode::Ulw | Mode::Child => {
                paint_ulw(hwnd, w, h)?;
            }
            Mode::Alpha => {
                // Half-transparent white sheet over everything the window
                // covers: pure "does this band composite at all" probe.
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 128, LWA_ALPHA)?;
                println!("SetLayeredWindowAttributes LWA_ALPHA 128");
            }
            Mode::Colorkey => {
                // Black = transparent, dots painted in WM_PAINT.
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY)?;
                println!("SetLayeredWindowAttributes LWA_COLORKEY black");
            }
            Mode::Plain => {
                // Keep WS_POPUP (child-style variant already proven to change
                // nothing); opaque paint happens in WM_PAINT.
                println!("plain mode: 600x600 opaque, no layering");
                if apply_rgn_to_plain {
                    let rgn = CreateRectRgn(0, 0, 0, 0);
                    for (cx, cy) in PROBES {
                        let dot = CreateEllipticRgn(
                            cx - DOT_R,
                            cy - DOT_R,
                            cx + DOT_R + 1,
                            cy + DOT_R + 1,
                        );
                        let _ = CombineRgn(Some(rgn), Some(rgn), Some(dot), RGN_OR);
                        let _ = DeleteObject(dot.into());
                    }
                    let set = SetWindowRgn(hwnd, Some(rgn), true);
                    println!("plain+rgn: SetWindowRgn -> {set}");
                }
            }
            Mode::Dots => unreachable!("handled above"),
            Mode::Region => {
                // Shape the window to the union of the dot circles; SetWindowRgn
                // takes ownership of the region handle.
                let rgn = CreateRectRgn(0, 0, 0, 0);
                for (cx, cy) in PROBES {
                    let dot =
                        CreateEllipticRgn(cx - DOT_R, cy - DOT_R, cx + DOT_R + 1, cy + DOT_R + 1);
                    let _ = CombineRgn(Some(rgn), Some(rgn), Some(dot), RGN_OR);
                    let _ = DeleteObject(dot.into());
                }
                let set = SetWindowRgn(hwnd, Some(rgn), true);
                println!("region mode: SetWindowRgn -> {set}");
            }
        }

        // Top of the sibling chain inside Progman = above SHELLDLL_DefView
        // (the icons) but still below every normal application window,
        // because Progman itself is the bottom of the z-order.
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            win_x,
            win_y,
            win_w,
            win_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        )?;
        println!("SetWindowPos HWND_TOP done");

        // Repaint AFTER reparent: SetParent can invalidate a layered surface.
        if matches!(mode, Mode::Ulw | Mode::Child) {
            paint_ulw(hwnd, w, h)?;
        } else {
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
        log_window("layer", hwnd);
        println!("layer visible={:?}", IsWindowVisible(hwnd).as_bool());
        println!("-- sibling chain of {:?} (top to bottom):", target);
        let mut c = GetWindow(target, GW_CHILD).ok(); // topmost child first
        while let Some(ch) = c {
            log_window("  child", ch);
            c = GetWindow(ch, GW_HWNDNEXT).ok();
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let ctx = Box::into_raw(Box::new(Ctx { parent, mode }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);
        SetTimer(Some(hwnd), TIMER_ID, TIMER_MS, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// Progman (or the WorkerW that hosts SHELLDLL_DefView on wallpaper-slideshow
/// setups) plus the DefView itself. Mirrors app desktop.rs.
unsafe fn find_desktop_chain() -> Option<(HWND, HWND)> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        if let Ok(dv) = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
            return Some((progman, dv));
        }
        let mut found: Option<(HWND, HWND)> = None;
        unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let found = unsafe { &mut *(lparam.0 as *mut Option<(HWND, HWND)>) };
            let mut class = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
            if String::from_utf16_lossy(&class[..n]) == "WorkerW" {
                if let Ok(dv) = unsafe {
                    FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                } {
                    *found = Some((hwnd, dv));
                    return windows::core::BOOL(0);
                }
            }
            windows::core::BOOL(1)
        }
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut found as *mut _ as isize));
        found
    }
}

unsafe fn log_window(tag: &str, hwnd: HWND) {
    unsafe {
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        println!(
            "{tag}: {:?} class={} rect=({},{})-({},{})",
            hwnd,
            String::from_utf16_lossy(&class[..n]),
            rect.left,
            rect.top,
            rect.right,
            rect.bottom
        );
    }
}

/// Push the probe dots via UpdateLayeredWindow (premultiplied BGRA DIB).
unsafe fn paint_ulw(hwnd: HWND, w: i32, h: i32) -> Result<()> {
    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(mem_dc, bmp.into());

        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
        px.fill(0);
        for (cx, cy) in PROBES {
            for dy in -DOT_R..=DOT_R {
                for dx in -DOT_R..=DOT_R {
                    if dx * dx + dy * dy > DOT_R * DOT_R {
                        continue;
                    }
                    let (x, y) = (cx + dx, cy + dy);
                    if x >= 0 && y >= 0 && x < w && y < h {
                        // Opaque magenta, premultiplied (alpha 255).
                        px[(y * w + x) as usize] = 0xFFFF00FF;
                    }
                }
            }
        }

        let ok = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&POINT { x: 0, y: 0 }),
            Some(&SIZE { cx: w, cy: h }),
            Some(mem_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            }),
            ULW_ALPHA,
        );
        println!("UpdateLayeredWindow ok={}", ok.is_ok());

        SelectObject(mem_dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
        ok
    }
}

/// Colorkey-mode painting: black background (keyed out), magenta dots on top.
unsafe fn paint_gdi(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let black = CreateSolidBrush(COLORREF(0));
        FillRect(dc, &rc, black);
        let _ = DeleteObject(black.into());

        let magenta = CreateSolidBrush(COLORREF(0x00FF00FF)); // 0x00BBGGRR
        let old = SelectObject(dc, magenta.into());
        for (cx, cy) in PROBES {
            let _ = Ellipse(dc, cx - DOT_R, cy - DOT_R, cx + DOT_R, cy + DOT_R);
        }
        SelectObject(dc, old);
        let _ = DeleteObject(magenta.into());
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ctx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Ctx;
    match msg {
        WM_NCHITTEST => {
            // Dot windows must never eat clicks meant for the icon beneath.
            // TOFU_NO_HITTEST disables this to test whether DWM culls
            // hit-test-transparent windows from the desktop band.
            let dots = !ctx.is_null()
                && unsafe { (*ctx).mode } == Mode::Dots
                && std::env::var_os("TOFU_NO_HITTEST").is_none();
            if dots {
                return LRESULT(HTTRANSPARENT as isize);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_PAINT => {
            let mode = if ctx.is_null() {
                None
            } else {
                Some(unsafe { (*ctx).mode })
            };
            println!("WM_PAINT fired (mode set: {})", mode.is_some());
            {
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            match mode {
                Some(Mode::Colorkey) => {
                    unsafe { paint_gdi(hwnd) };
                    LRESULT(0)
                }
                Some(Mode::Plain) | Some(Mode::Region) | Some(Mode::Dots) => {
                    unsafe {
                        let mut ps = PAINTSTRUCT::default();
                        let dc = BeginPaint(hwnd, &mut ps);
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let magenta = CreateSolidBrush(COLORREF(0x00FF00FF));
                        FillRect(dc, &rc, magenta);
                        let _ = DeleteObject(magenta.into());
                        let _ = EndPaint(hwnd, &ps);
                    }
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
            }
        }
        WM_TIMER if wp.0 == TIMER_ID => {
            // Last-painter-wins diagnostic: keep forcing our own repaint; if
            // the window only stays visible under this, the band has no per-
            // window composition for foreigners — Explorer just overpaints us.
            if std::env::var_os("TOFU_REPAINT").is_some() {
                let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            }
            if !ctx.is_null() {
                let parent_alive = unsafe { IsWindow(Some((*ctx).parent)) }.as_bool();
                if !parent_alive {
                    // Explorer died; children go with it. The real fix will
                    // recreate + re-find here — the spike just reports.
                    println!("timer: parent DEAD (Explorer restart?)");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            println!("WM_DESTROY received (parent likely torn down)");
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
