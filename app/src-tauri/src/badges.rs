//! Badge layer: click-through overlay marking annotated desktop icons with a
//! small dot (docs/ARCHITECTURE.md §6, reworked per docs/V0.1.1.md A2).
//!
//! One native layered TOPMOST window spans the virtual screen; badges are
//! drawn into a 32-bit premultiplied DIB and pushed with UpdateLayeredWindow.
//! No webview involved.
//!
//! A2 model (spikes/badge-reparent ruled out reparenting into the desktop
//! z-band): the layer stays shown, and instead each dot is individually
//! occlusion-tested — a dot is skipped while any visible top-level window
//! overlaps its pixels, so dots never draw over applications. WinEvent hooks
//! (foreground changes + window moves, throttled) keep the set current within
//! ~100 ms; a 2 s timer handles icon-position/sidecar drift while the desktop
//! is foreground. Zero work while nothing changes.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::appstate::Paused;
use crate::desktop::{self, DesktopUia};
use crate::{settings, storage};

const REFRESH_TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 2000;
/// One-shot coalescing timer armed by WinEvent callbacks.
const EVENT_TIMER_ID: usize = 2;
const EVENT_DELAY_MS: u32 = 80;
const BADGE_R: i32 = 6; // radius in px
                        // Warm accent, premultiplied at full alpha below.
const BADGE_RGBA: (u8, u8, u8, u8) = (0xF5, 0x8F, 0x3C, 0xE6);

/// Badge window handle for the WinEvent callbacks (single instance).
static BADGE_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn spawn(paused: Paused, settings: settings::Shared) {
    std::thread::Builder::new()
        .name("badge-layer".into())
        .spawn(move || {
            desktop::init_com_for_thread();
            let uia = match DesktopUia::new() {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("badge layer: UIA init failed: {e}");
                    return;
                }
            };
            if let Err(e) = run(uia, paused, settings) {
                eprintln!("badge layer failed: {e}");
            }
        })
        .expect("spawn badge layer");
}

struct Ctx {
    uia: DesktopUia,
    visible: bool,
    paused: Paused,
    settings: settings::Shared,
    /// Dot centers (bitmap-relative to the virtual-screen origin) of every
    /// badged icon, refreshed by the UIA walk.
    badged: Vec<(i32, i32)>,
    /// The subset actually drawn last push (post-occlusion); repaints are
    /// skipped while this is unchanged.
    drawn: Vec<(i32, i32)>,
}

fn run(uia: DesktopUia, paused: Paused, settings: settings::Shared) -> Result<()> {
    unsafe {
        let class_name = w!("TofuNuggetsBadgeLayer");
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name,
            w!("Tofu Nuggets badges"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        BADGE_HWND.store(hwnd.0 as isize, Ordering::Release);

        // Lives for the whole process; the window procedure owns it via
        // GWLP_USERDATA.
        let ctx = Box::into_raw(Box::new(Ctx {
            uia,
            visible: false,
            paused,
            settings,
            badged: Vec::new(),
            drawn: Vec::new(),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);

        // Push-based updates: foreground switches and window moves re-run the
        // occlusion pass (coalesced via EVENT_TIMER). Hooks belong to this
        // thread's message loop; own-process windows are skipped.
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let _fg: HWINEVENTHOOK = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event),
            0,
            0,
            flags,
        );
        let _loc: HWINEVENTHOOK = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(win_event),
            0,
            0,
            flags,
        );

        SetTimer(Some(hwnd), REFRESH_TIMER_ID, REFRESH_MS, None);
        full_refresh(hwnd, &mut *ctx);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// WinEvent callback: any top-level window moving/resizing or the foreground
/// changing can change dot occlusion. Coalesce bursts (drags fire dozens of
/// LOCATIONCHANGEs per second) into one occlusion pass via a one-shot timer;
/// re-arming an armed timer just resets it.
unsafe extern "system" fn win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    idchild: i32,
    _thread: u32,
    _time: u32,
) {
    if idobject != OBJID_WINDOW.0 || idchild != 0 || hwnd.is_invalid() {
        return;
    }
    // Only top-level windows can occlude the desktop.
    if event == EVENT_OBJECT_LOCATIONCHANGE && unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
        return;
    }
    let badge = BADGE_HWND.load(Ordering::Acquire);
    if badge != 0 {
        unsafe {
            SetTimer(
                Some(HWND(badge as *mut core::ffi::c_void)),
                EVENT_TIMER_ID,
                EVENT_DELAY_MS,
                None,
            );
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == REFRESH_TIMER_ID => {
            let ctx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Ctx;
            if !ctx.is_null() {
                full_refresh(hwnd, unsafe { &mut *ctx });
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == EVENT_TIMER_ID => {
            unsafe {
                let _ = KillTimer(Some(hwnd), EVENT_TIMER_ID);
            }
            let ctx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Ctx;
            if !ctx.is_null() {
                let ctx = unsafe { &mut *ctx };
                // Foreground flips can mean the desktop just appeared with a
                // stale badge set — cheap occlusion pass first, and the 2 s
                // timer keeps content fresh.
                let occluders = occluder_rects();
                occlusion_pass(hwnd, ctx, &occluders);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn hide(hwnd: HWND, ctx: &mut Ctx) {
    if ctx.visible {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        ctx.visible = false;
    }
    ctx.drawn.clear();
}

/// Full pass: sidecar/icon state via UIA, then redraw. The UIA walk is
/// skipped while a maximized/fullscreen window hides the whole desktop —
/// every dot is occluded then, so only the cheap occlusion pass runs.
fn full_refresh(hwnd: HWND, ctx: &mut Ctx) {
    if ctx.paused.is_paused() {
        hide(hwnd, ctx);
        return;
    }

    let occluders = occluder_rects();
    let vs = virtual_screen();
    if occluders.iter().any(|r| covers(r, &vs)) {
        occlusion_pass(hwnd, ctx, &occluders);
        return;
    }

    // Cheap re-apply so infotips stay off across Explorer restarts. Kept
    // independent of the badge toggle: the panel must stay the sole hover
    // surface even when dots are switched off.
    desktop::suppress_desktop_infotips();

    // Badge dots disabled in settings: keep the layer hidden but leave
    // infotip suppression (above) running.
    let badges_on = ctx.settings.lock().map(|s| s.badges).unwrap_or(true);
    if !badges_on {
        ctx.badged.clear();
        hide(hwnd, ctx);
        return;
    }

    let Ok(icons) = ctx.uia.list_icons() else {
        return;
    };
    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    ctx.badged = icons
        .iter()
        .filter(|ic| {
            ic.path
                .as_ref()
                .map(|p| storage::has_nugget(p))
                .unwrap_or(false)
        })
        .map(|ic| {
            // Badge center: top-right corner of the icon cell, nudged inward.
            (
                ic.rect.right - vx - BADGE_R - 4,
                ic.rect.top - vy + BADGE_R + 4,
            )
        })
        .collect();

    occlusion_pass(hwnd, ctx, &occluders);
}

fn virtual_screen() -> RECT {
    unsafe {
        RECT {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            right: GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: GetSystemMetrics(SM_YVIRTUALSCREEN) + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// `outer` fully contains `inner`.
fn covers(outer: &RECT, inner: &RECT) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Rects (screen coords) of every window that can cover a dot: visible,
/// non-minimized, non-cloaked top-level windows of other processes, excluding
/// the desktop chain itself.
fn occluder_rects() -> Vec<RECT> {
    struct EnumState {
        rects: Vec<RECT>,
        own_pid: u32,
    }
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut EnumState) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == state.own_pid {
                return BOOL(1);
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            // The desktop band itself never occludes dots. Everything below
            // it in z-order is invisible anyway, so stop the walk there.
            if class == "Progman" || class == "WorkerW" {
                return BOOL(0);
            }
            // UWP ghosts: alive, "visible", but cloaked by DWM.
            let mut cloaked = 0u32;
            let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            if cloaked != 0 {
                return BOOL(1);
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                state.rects.push(rect);
            }
        }
        BOOL(1)
    }
    let mut state = EnumState {
        rects: Vec::new(),
        own_pid: std::process::id(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut state as *mut _ as isize));
    }
    state.rects
}

fn intersects(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Filter the badged set to dots whose pixels no window covers, and push a
/// repaint only when that set changed since the last push.
fn occlusion_pass(hwnd: HWND, ctx: &mut Ctx, occluders: &[RECT]) {
    if ctx.paused.is_paused() {
        hide(hwnd, ctx);
        return;
    }
    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };

    let visible_dots: Vec<(i32, i32)> = ctx
        .badged
        .iter()
        .copied()
        .filter(|&(cx, cy)| {
            let dot = RECT {
                left: vx + cx - BADGE_R - 1,
                top: vy + cy - BADGE_R - 1,
                right: vx + cx + BADGE_R + 1,
                bottom: vy + cy + BADGE_R + 1,
            };
            !occluders.iter().any(|w| intersects(&dot, w))
        })
        .collect();

    if visible_dots == ctx.drawn && ctx.visible {
        return;
    }
    draw(hwnd, ctx, visible_dots);
}

fn draw(hwnd: HWND, ctx: &mut Ctx, dots: Vec<(i32, i32)>) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if vw <= 0 || vh <= 0 {
            return;
        }

        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: vw,
                biHeight: -vh, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return;
        };
        let old = SelectObject(mem_dc, bmp.into());

        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (vw * vh) as usize);
        px.fill(0); // fully transparent

        for &(cx, cy) in &dots {
            draw_dot(px, vw, vh, cx, cy);
        }

        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&POINT { x: vx, y: vy }),
            Some(&SIZE { cx: vw, cy: vh }),
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

        SelectObject(mem_dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        ctx.drawn = dots;
        if !ctx.visible {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            ctx.visible = true;
        }
    }
}

/// Filled anti-aliased dot with a subtle white ring, premultiplied BGRA.
fn draw_dot(px: &mut [u32], w: i32, h: i32, cx: i32, cy: i32) {
    let (r8, g8, b8, a8) = BADGE_RGBA;
    let rad = BADGE_R as f32;
    for dy in -BADGE_R - 1..=BADGE_R + 1 {
        for dx in -BADGE_R - 1..=BADGE_R + 1 {
            let x = cx + dx;
            let y = cy + dy;
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            // 1 px anti-aliasing falloff at the rim.
            let coverage = (rad + 0.5 - dist).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let a = (a8 as f32 * coverage) as u32;
            // Ring: lighten the outer 1.5 px for contrast on dark wallpapers.
            let (r, g, b) = if dist > rad - 1.5 {
                (0xFFu32, 0xFFu32, 0xFFu32)
            } else {
                (r8 as u32, g8 as u32, b8 as u32)
            };
            // Premultiply.
            let pr = r * a / 255;
            let pg = g * a / 255;
            let pb = b * a / 255;
            px[(y * w + x) as usize] = (a << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }
}
