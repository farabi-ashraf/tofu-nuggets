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
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(not(windows))]
use tauri::{LogicalPosition, LogicalSize};
#[cfg(windows)]
use tauri::{PhysicalPosition, PhysicalSize};

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

#[cfg(windows)]
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
    // Panel-hidden timestamp driving WebView2 idle release (Windows-only, as
    // is the release itself).
    #[cfg(windows)]
    let mut idle_since = Instant::now();
    #[cfg(windows)]
    let idle_release = Duration::from_secs(idle_release_secs());

    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));

        // Paused from the tray: hide any panel and do no detection.
        if paused.is_paused() {
            if shown.take().is_some() {
                hide_panel(app);
                outside_since = None;
                #[cfg(windows)]
                {
                    idle_since = Instant::now();
                }
            }
            continue;
        }

        // Idle release: destroy the (hidden) overlay window so WebView2's
        // process tree is reclaimed; recreated on next hover. Window teardown
        // must happen on the main thread. Windows-only: this exists for
        // WebView2's ~380 MB process tree, while WKWebView has no equivalent
        // cost, and recreating an AppKit window per hover is a crash risk we
        // have no reason to take.
        #[cfg(windows)]
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
                    #[cfg(windows)]
                    {
                        idle_since = Instant::now();
                    }
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
            pt,
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

/// Widest an anchor item may be before the panel anchors to the cursor instead
/// of the item's edges. Desktop icon cells are small; Explorer rows in
/// details/list/content span most of the window, and anchoring to their far
/// right shoves the panel off-screen — so wide items anchor at the cursor.
const MAX_ANCHOR_W: i32 = 220;

/// Place the panel beside the item (physical pixels), flipping to the other
/// side / edge when it would run off the virtual screen, and clamping so it is
/// always fully on-screen. Origin is assumed (0,0) — the same single-/primary-
/// monitor assumption the rest of the engine already makes.
fn panel_rect(
    icon: &IconRect,
    cursor: (i32, i32),
    pw: i32,
    ph: i32,
    screen_w: i32,
    screen_h: i32,
) -> IconRect {
    // Anchor horizontally to the item for narrow icons, to the cursor for wide
    // rows (so column width can't push the panel to the far edge).
    let (aleft, aright) = if icon.right - icon.left > MAX_ANCHOR_W {
        (cursor.0, cursor.0)
    } else {
        (icon.left, icon.right)
    };
    let mut left = aright + PANEL_GAP;
    if left + pw > screen_w {
        left = aleft - PANEL_GAP - pw; // flip to the left side
    }
    left = left.clamp(0, (screen_w - pw).max(0));

    let mut top = icon.top;
    if top + ph > screen_h {
        top = icon.bottom - ph; // flip above the item from the bottom edge
    }
    top = top.clamp(0, (screen_h - ph).max(0));

    IconRect {
        left,
        top,
        right: left + pw,
        bottom: top + ph,
    }
}

/// Returns the panel's rect (engine units) when shown.
fn show_panel(
    app: &AppHandle,
    icon_rect: &IconRect,
    cursor: (i32, i32),
    payload: ShowPayload,
) -> Option<IconRect> {
    let win = overlay::get_or_create(app).ok()?;
    // Engine units are physical pixels on Windows, so the panel is sized in
    // them; macOS works in points, which already absorb the display scale.
    #[cfg(windows)]
    let sf = win.scale_factor().unwrap_or(1.0);
    #[cfg(not(windows))]
    let sf = 1.0;
    // User panel zoom (1.0–1.5); the page also scales its font by the same
    // factor (--panel-scale) so the whole panel grows together.
    let zoom = app
        .state::<settings::Shared>()
        .lock()
        .map(|s| s.panel_scale)
        .unwrap_or(1.0);
    let pw = (PANEL_W * sf * zoom).round() as i32;
    let ph = (PANEL_H * sf * zoom).round() as i32;
    let r = panel_rect(
        icon_rect,
        cursor,
        pw,
        ph,
        icons::virtual_screen_width(),
        icons::virtual_screen_height(),
    );
    // Stash for freshly created pages, then emit for already-loaded ones.
    if let Ok(mut cur) = app.state::<CurrentNugget>().0.lock() {
        *cur = Some(payload.clone());
    }
    let _ = app.emit("nugget:show", payload);
    place_panel(app, &win, pw, ph, &r);
    Some(r)
}

/// Move, size and show the panel. Both the units and the thread differ by
/// platform: Windows takes physical pixels and tolerates these calls from the
/// hover thread, while macOS speaks points and requires every AppKit window
/// call on the main thread — doing it here killed the process a few seconds
/// into the first successful hover.
#[cfg(windows)]
fn place_panel(_app: &AppHandle, win: &tauri::WebviewWindow, pw: i32, ph: i32, r: &IconRect) {
    let _ = win.set_size(PhysicalSize::new(pw as u32, ph as u32));
    let _ = win.set_position(PhysicalPosition::new(r.left, r.top));
    let _ = win.show();
}

#[cfg(not(windows))]
fn place_panel(app: &AppHandle, win: &tauri::WebviewWindow, pw: i32, ph: i32, r: &IconRect) {
    let win = win.clone();
    let (left, top) = (r.left, r.top);
    let _ = app.run_on_main_thread(move || {
        let _ = win.set_size(LogicalSize::new(pw as f64, ph as f64));
        let _ = win.set_position(LogicalPosition::new(left as f64, top as f64));
        let _ = win.show();
    });
}

#[cfg(windows)]
fn hide_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(overlay::LABEL) {
        let _ = win.hide();
    }
}

/// macOS parks the panel off-screen instead of hiding it: an app with no
/// visible window gets terminated, and the panel is usually the only window
/// there is (see `overlay::park`).
#[cfg(target_os = "macos")]
fn hide_panel(app: &AppHandle) {
    let Some(win) = app.get_webview_window(overlay::LABEL) else {
        return;
    };
    let _ = app.run_on_main_thread(move || overlay::park(&win));
}

#[cfg(not(any(windows, target_os = "macos")))]
fn hide_panel(app: &AppHandle) {
    let Some(win) = app.get_webview_window(overlay::LABEL) else {
        return;
    };
    let _ = app.run_on_main_thread(move || {
        let _ = win.hide();
    });
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

    // Cursor inside the icon; irrelevant for narrow icons (item-anchored).
    fn cur(ic: &IconRect) -> (i32, i32) {
        ((ic.left + ic.right) / 2, (ic.top + ic.bottom) / 2)
    }

    #[test]
    fn panel_sits_right_of_icon_normally() {
        let ic = icon(100, 200);
        let r = panel_rect(&ic, cur(&ic), 340, 240, 1920, 1080);
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
        let r = panel_rect(&ic, cur(&ic), 340, 240, 1920, 1080);
        assert_eq!(r.right, ic.left - PANEL_GAP);
        assert!(r.right <= 1920);
        assert_eq!(r.left, ic.left - PANEL_GAP - 340);
    }

    #[test]
    fn flip_threshold_is_exact() {
        // Icon.left = 400 leaves room for a left-flip to land fully on-screen.
        let ic = icon(400, 0); // icon.right = 476
        let screen_w = 476 + PANEL_GAP + 340;
        // Exactly fits: no flip.
        let r = panel_rect(&ic, cur(&ic), 340, 240, screen_w, 1080);
        assert_eq!(r.left, 476 + PANEL_GAP);
        // One pixel narrower: flips to the icon's left (still on-screen here).
        let r2 = panel_rect(&ic, cur(&ic), 340, 240, screen_w - 1, 1080);
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
        let r = panel_rect(&ic, cur(&ic), 340, 240, 1920, 1080);
        assert_eq!(r.top, 0);
    }

    #[test]
    fn scaled_panel_still_flips() {
        // 1.5x panel zoom on a 125% DPI screen.
        let pw = (340.0_f64 * 1.25 * 1.5).round() as i32;
        let ic = icon(2560 - 400, 100);
        let r = panel_rect(&ic, cur(&ic), pw, 450, 2560, 1440);
        assert_eq!(r.right, ic.left - PANEL_GAP);
    }

    #[test]
    fn wide_row_anchors_to_cursor_not_far_right() {
        // Explorer details/list/content row spanning most of the window: the
        // panel must sit beside the CURSOR, not the row's far-right edge.
        let ic = IconRect {
            left: 250,
            top: 300,
            right: 1250, // 1000 px wide -> wide-item path
            bottom: 322,
        };
        let r = panel_rect(&ic, (500, 311), 340, 240, 1920, 1080);
        assert_eq!(r.left, 500 + PANEL_GAP);
    }

    #[test]
    fn panel_flips_up_at_bottom_edge() {
        // Item near the bottom: the panel flips up and stays fully on-screen.
        let ic = icon(100, 1000); // bottom = 1096, off a 1080 screen
        let r = panel_rect(&ic, cur(&ic), 340, 240, 1920, 1080);
        assert!(r.bottom <= 1080);
        assert_eq!(r.top, 1080 - 240);
    }
}
