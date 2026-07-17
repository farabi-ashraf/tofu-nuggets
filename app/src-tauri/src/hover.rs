//! Hover engine: watches the cursor, shows the overlay panel over annotated
//! desktop icons, hides it when the cursor leaves.
//!
//! Budget rules (docs/ARCHITECTURE.md): 10 Hz cursor polling only; the UIA
//! hit-test fires once per cursor rest (~400 ms), never continuously. While a
//! panel is visible, leave-detection is cheap rect math, not UIA.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::desktop::{self, DesktopUia};
use crate::{overlay, storage};

const POLL_MS: u64 = 100;
const DEBOUNCE_MS: u128 = 400;
const LEAVE_GRACE_MS: u128 = 250;
// Logical units; scaled by the window's DPI factor at show time.
const PANEL_W: f64 = 340.0;
const PANEL_H: f64 = 240.0;
const PANEL_GAP: i32 = 8;

fn idle_release_secs() -> u64 {
    std::env::var("TOFU_IDLE_RELEASE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
}

#[derive(Clone, Serialize)]
pub struct ShowPayload {
    name: String,
    path: String,
    html: String,
}

/// Last payload sent to the panel; freshly (re)created panel pages pull it
/// on load, since an emit can fire before their listener registers.
#[derive(Default)]
pub struct CurrentNugget(Mutex<Option<ShowPayload>>);

#[tauri::command]
pub fn get_current_nugget(state: State<CurrentNugget>) -> Option<ShowPayload> {
    state.0.lock().ok().and_then(|g| g.clone())
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("hover-engine".into())
        .spawn(move || {
            desktop::init_com_for_thread();
            let uia = match DesktopUia::new() {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("hover engine: UIA init failed: {e}");
                    return;
                }
            };
            run(&app, &uia);
        })
        .expect("spawn hover engine");
}

fn run(app: &AppHandle, uia: &DesktopUia) {
    let mut last_pos = POINT { x: -1, y: -1 };
    let mut rest_since: Option<Instant> = None;
    let mut tested_at_rest = false;
    // Icon + panel rects currently showing, plus when the cursor left them.
    let mut shown: Option<(RECT, RECT)> = None;
    let mut outside_since: Option<Instant> = None;
    // Panel-hidden timestamp driving WebView2 idle release.
    let mut idle_since = Instant::now();
    let idle_release = Duration::from_secs(idle_release_secs());

    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));

        // Idle release: destroy the (hidden) overlay window so WebView2's
        // process tree is reclaimed; recreated on next hover. Window teardown
        // must happen on the main thread.
        if shown.is_none() && overlay::exists(app) && idle_since.elapsed() >= idle_release {
            let ah = app.clone();
            let _ = app.run_on_main_thread(move || overlay::destroy(&ah));
        }

        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            continue;
        }

        let moved = pt.x != last_pos.x || pt.y != last_pos.y;
        if moved {
            last_pos = pt;
            rest_since = Some(Instant::now());
            tested_at_rest = false;
        }

        // Leave detection for a visible panel: icon rect + panel rect union.
        if let Some((icon_r, panel_r)) = shown {
            if point_in_hover_zone(pt, &icon_r, &panel_r) {
                outside_since = None;
            } else {
                let out = outside_since.get_or_insert_with(Instant::now);
                if out.elapsed().as_millis() >= LEAVE_GRACE_MS {
                    hide_panel(app);
                    shown = None;
                    outside_since = None;
                    idle_since = Instant::now();
                }
            }
            continue; // while shown, no new hit-tests needed
        }

        // Debounced single hit-test per rest.
        let Some(rs) = rest_since else { continue };
        if tested_at_rest || rs.elapsed().as_millis() < DEBOUNCE_MS {
            continue;
        }
        tested_at_rest = true;

        let Some(icon) = uia.icon_at(pt) else {
            continue;
        };
        let Some(path) = icon.path.as_ref() else {
            continue;
        };
        let Some(nugget) = storage::read_nugget(path) else {
            continue;
        };

        if let Some(panel_r) = show_panel(
            app,
            &icon.rect,
            ShowPayload {
                name: icon.name.clone(),
                path: path.display().to_string(),
                html: nugget.html,
            },
        ) {
            shown = Some((icon.rect, panel_r));
            outside_since = None;
        }
    }
}

/// Icon rect (padded) or panel rect keeps the panel open.
fn point_in_hover_zone(pt: POINT, icon: &RECT, panel: &RECT) -> bool {
    let pad = 4;
    let in_icon = pt.x >= icon.left - pad
        && pt.x <= icon.right + pad
        && pt.y >= icon.top - pad
        && pt.y <= icon.bottom + pad;
    let in_panel =
        pt.x >= panel.left && pt.x <= panel.right && pt.y >= panel.top && pt.y <= panel.bottom;
    in_icon || in_panel
}

/// Panel goes to the right of the icon in physical pixels, flipped left when
/// it would run off the virtual screen's right edge.
fn panel_rect(icon: &RECT, pw: i32, ph: i32) -> RECT {
    let screen_w = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN,
        )
    };
    let mut left = icon.right + PANEL_GAP;
    if left + pw > screen_w {
        left = icon.left - PANEL_GAP - pw;
    }
    let top = icon.top.max(0);
    RECT {
        left,
        top,
        right: left + pw,
        bottom: top + ph,
    }
}

/// Returns the panel's physical rect when shown.
fn show_panel(app: &AppHandle, icon_rect: &RECT, payload: ShowPayload) -> Option<RECT> {
    let win = overlay::get_or_create(app).ok()?;
    let sf = win.scale_factor().unwrap_or(1.0);
    let pw = (PANEL_W * sf).round() as i32;
    let ph = (PANEL_H * sf).round() as i32;
    let r = panel_rect(icon_rect, pw, ph);
    // Stash for freshly created pages, then emit for already-loaded ones.
    if let Ok(mut cur) = app.state::<CurrentNugget>().0.lock() {
        *cur = Some(payload.clone());
    }
    let _ = app.emit("nugget:show", payload);
    let _ = win.set_size(PhysicalSize::new(pw as u32, ph as u32));
    let _ = win.set_position(PhysicalPosition::new(r.left, r.top));
    let _ = win.show();
    Some(r)
}

fn hide_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(overlay::LABEL) {
        let _ = win.hide();
    }
}
