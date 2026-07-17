//! Badge layer: click-through overlay marking annotated desktop icons with a
//! small dot (docs/ARCHITECTURE.md §6).
//!
//! One native layered window spans the virtual screen; badges are drawn into
//! a 32-bit premultiplied DIB and pushed with UpdateLayeredWindow. The window
//! is only shown while the desktop is foreground, refreshed on a 2 s timer.
//! No webview involved.

use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::appstate::Paused;
use crate::desktop::{self, DesktopUia};
use crate::storage;

const REFRESH_TIMER_ID: usize = 1;
const REFRESH_MS: u32 = 2000;
const BADGE_R: i32 = 6; // radius in px
                        // Warm accent, premultiplied at full alpha below.
const BADGE_RGBA: (u8, u8, u8, u8) = (0xF5, 0x8F, 0x3C, 0xE6);

pub fn spawn(paused: Paused) {
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
            if let Err(e) = run(uia, paused) {
                eprintln!("badge layer failed: {e}");
            }
        })
        .expect("spawn badge layer");
}

struct Ctx {
    uia: DesktopUia,
    visible: bool,
    paused: Paused,
}

fn run(uia: DesktopUia, paused: Paused) -> Result<()> {
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

        // Lives for the whole process; the window procedure owns it via
        // GWLP_USERDATA.
        let ctx = Box::into_raw(Box::new(Ctx {
            uia,
            visible: false,
            paused,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as isize);

        SetTimer(Some(hwnd), REFRESH_TIMER_ID, REFRESH_MS, None);
        refresh(hwnd, &mut *ctx);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == REFRESH_TIMER_ID => {
            let ctx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Ctx;
            if !ctx.is_null() {
                refresh(hwnd, unsafe { &mut *ctx });
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

fn refresh(hwnd: HWND, ctx: &mut Ctx) {
    // Badges only while the desktop is foreground and not paused; hidden
    // otherwise so the topmost layer never draws over other applications.
    if ctx.paused.is_paused() || !desktop::desktop_is_foreground() {
        if ctx.visible {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            ctx.visible = false;
        }
        return;
    }

    // Cheap re-apply so infotips stay off across Explorer restarts.
    desktop::suppress_desktop_infotips();

    let Ok(icons) = ctx.uia.list_icons() else {
        return;
    };
    let badged: Vec<_> = icons
        .iter()
        .filter(|ic| {
            ic.path
                .as_ref()
                .map(|p| storage::has_nugget(p))
                .unwrap_or(false)
        })
        .collect();

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

        for ic in &badged {
            // Badge center: top-right corner of the icon cell, nudged inward.
            let cx = ic.rect.right - vx - BADGE_R - 4;
            let cy = ic.rect.top - vy + BADGE_R + 4;
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
