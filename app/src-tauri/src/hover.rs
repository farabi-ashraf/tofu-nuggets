//! Hover engine: watches the cursor, shows the overlay panel over annotated
//! desktop icons, hides it when the cursor leaves.
//!
//! Budget rules (docs/ARCHITECTURE.md): 10 Hz cursor polling only; the
//! platform hit-test fires once per cursor rest (~400 ms), never continuously.
//! While a panel is visible, leave-detection is cheap rect math, not a
//! hit-test. Platform-agnostic by design: all icon/cursor access goes through
//! `crate::icons` (B2) — no `windows::` imports here.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};

use crate::appstate::Paused;
use crate::icons::{self, DesktopIcons, IconRect};
use crate::{overlay, settings, storage};

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

pub fn spawn(app: AppHandle, paused: Paused) {
    std::thread::Builder::new()
        .name("hover-engine".into())
        .spawn(move || {
            icons::init_thread();
            let provider = match icons::new_icons() {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("hover engine: icon provider init failed: {e}");
                    return;
                }
            };
            run(&app, &provider, &paused);
        })
        .expect("spawn hover engine");
}

fn run(app: &AppHandle, provider: &impl DesktopIcons, paused: &Paused) {
    let mut last_pos = (-1, -1);
    let mut rest_since: Option<Instant> = None;
    let mut tested_at_rest = false;
    // Icon + panel rects currently showing, plus when the cursor left them.
    let mut shown: Option<(IconRect, IconRect)> = None;
    let mut outside_since: Option<Instant> = None;
    // Panel-hidden timestamp driving WebView2 idle release.
    let mut idle_since = Instant::now();
    let idle_release = Duration::from_secs(idle_release_secs());

    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));

        // Paused from the tray: hide any panel and do no detection.
        if paused.is_paused() {
            if shown.take().is_some() {
                hide_panel(app);
                outside_since = None;
                idle_since = Instant::now();
            }
            continue;
        }

        // Idle release: destroy the (hidden) overlay window so WebView2's
        // process tree is reclaimed; recreated on next hover. Window teardown
        // must happen on the main thread.
        if shown.is_none() && overlay::exists(app) && idle_since.elapsed() >= idle_release {
            let ah = app.clone();
            let _ = app.run_on_main_thread(move || overlay::destroy(&ah));
        }

        let Some(pt) = icons::cursor_pos() else {
            continue;
        };

        let moved = pt != last_pos;
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

        let Some(icon) = provider.icon_at(pt.0, pt.1) else {
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
fn point_in_hover_zone(pt: (i32, i32), icon: &IconRect, panel: &IconRect) -> bool {
    let pad = 4;
    let (x, y) = pt;
    let in_icon = x >= icon.left - pad
        && x <= icon.right + pad
        && y >= icon.top - pad
        && y <= icon.bottom + pad;
    let in_panel = x >= panel.left && x <= panel.right && y >= panel.top && y <= panel.bottom;
    in_icon || in_panel
}

/// Panel goes to the right of the icon in physical pixels, flipped left when
/// it would run off the virtual screen's right edge.
fn panel_rect(icon: &IconRect, pw: i32, ph: i32, screen_w: i32) -> IconRect {
    let mut left = icon.right + PANEL_GAP;
    if left + pw > screen_w {
        left = icon.left - PANEL_GAP - pw;
    }
    let top = icon.top.max(0);
    IconRect {
        left,
        top,
        right: left + pw,
        bottom: top + ph,
    }
}

/// Returns the panel's physical rect when shown.
fn show_panel(app: &AppHandle, icon_rect: &IconRect, payload: ShowPayload) -> Option<IconRect> {
    let win = overlay::get_or_create(app).ok()?;
    let sf = win.scale_factor().unwrap_or(1.0);
    // User panel zoom (1.0–1.5); the page also scales its font by the same
    // factor (--panel-scale) so the whole panel grows together.
    let zoom = app
        .state::<settings::Shared>()
        .lock()
        .map(|s| s.panel_scale)
        .unwrap_or(1.0);
    let pw = (PANEL_W * sf * zoom).round() as i32;
    let ph = (PANEL_H * sf * zoom).round() as i32;
    let r = panel_rect(icon_rect, pw, ph, icons::virtual_screen_width());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(left: i32, top: i32) -> IconRect {
        IconRect {
            left,
            top,
            right: left + 76,
            bottom: top + 96,
        }
    }

    #[test]
    fn panel_sits_right_of_icon_normally() {
        let r = panel_rect(&icon(100, 200), 340, 240, 1920);
        assert_eq!(r.left, 176 + PANEL_GAP);
        assert_eq!(r.top, 200);
        assert_eq!(r.right - r.left, 340);
        assert_eq!(r.bottom - r.top, 240);
    }

    #[test]
    fn panel_flips_left_at_right_edge() {
        // Icon hugging the right edge of a 1920-wide screen: right side would
        // overflow, so the panel goes to the icon's left.
        let ic = icon(1920 - 80, 300);
        let r = panel_rect(&ic, 340, 240, 1920);
        assert_eq!(r.right, ic.left - PANEL_GAP);
        assert!(r.right <= 1920);
        assert_eq!(r.left, ic.left - PANEL_GAP - 340);
    }

    #[test]
    fn flip_threshold_is_exact() {
        // Exactly fits: no flip.
        let ic = icon(0, 0); // icon.right = 76
        let screen_w = 76 + PANEL_GAP + 340;
        let r = panel_rect(&ic, 340, 240, screen_w);
        assert_eq!(r.left, 76 + PANEL_GAP);
        // One pixel narrower: flips.
        let r2 = panel_rect(&ic, 340, 240, screen_w - 1);
        assert_eq!(r2.right, ic.left - PANEL_GAP);
    }

    #[test]
    fn top_is_clamped_to_screen() {
        let ic = IconRect {
            left: 100,
            top: -30,
            right: 176,
            bottom: 66,
        };
        let r = panel_rect(&ic, 340, 240, 1920);
        assert_eq!(r.top, 0);
    }

    #[test]
    fn scaled_panel_still_flips() {
        // 1.5x panel zoom on a 125% DPI screen.
        let pw = (340.0_f64 * 1.25 * 1.5).round() as i32;
        let ic = icon(2560 - 400, 100);
        let r = panel_rect(&ic, pw, 450, 2560);
        assert_eq!(r.right, ic.left - PANEL_GAP);
    }
}
