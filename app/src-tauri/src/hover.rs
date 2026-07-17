//! Hover engine: watches the cursor, shows the overlay panel over annotated
//! desktop icons, hides it when the cursor leaves.
//!
//! Budget rules (docs/ARCHITECTURE.md): 10 Hz cursor polling only; the UIA
//! hit-test fires once per cursor rest (~400 ms), never continuously. While a
//! panel is visible, leave-detection is cheap rect math, not UIA.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::desktop::{self, DesktopUia};
use crate::storage;

const POLL_MS: u64 = 100;
const DEBOUNCE_MS: u128 = 400;
const LEAVE_GRACE_MS: u128 = 250;
const PANEL_W: i32 = 340;
const PANEL_H: i32 = 240;
const PANEL_GAP: i32 = 8;

#[derive(Clone, Serialize)]
struct ShowPayload {
    name: String,
    path: String,
    html: String,
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
    // Icon rect currently showing a panel, plus when the cursor left it.
    let mut shown_rect: Option<RECT> = None;
    let mut outside_since: Option<Instant> = None;

    loop {
        std::thread::sleep(Duration::from_millis(POLL_MS));

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

        // Leave detection for a visible panel: icon rect + panel strip union.
        if let Some(r) = shown_rect {
            if point_in_hover_zone(pt, &r) {
                outside_since = None;
            } else {
                let out = outside_since.get_or_insert_with(Instant::now);
                if out.elapsed().as_millis() >= LEAVE_GRACE_MS {
                    hide_panel(app);
                    shown_rect = None;
                    outside_since = None;
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

        let Some(icon) = uia.icon_at(pt) else { continue };
        let Some(path) = icon.path.as_ref() else { continue };
        let Some(nugget) = storage::read_nugget(path) else { continue };

        show_panel(
            app,
            &icon.rect,
            ShowPayload {
                name: icon.name.clone(),
                path: path.display().to_string(),
                html: nugget.html,
            },
        );
        shown_rect = Some(icon.rect);
        outside_since = None;
    }
}

/// Icon rect expanded toward the panel so moving onto the panel keeps it open.
fn point_in_hover_zone(pt: POINT, icon: &RECT) -> bool {
    let pad = 4;
    let in_icon = pt.x >= icon.left - pad
        && pt.x <= icon.right + pad
        && pt.y >= icon.top - pad
        && pt.y <= icon.bottom + pad;
    let panel = panel_rect(icon);
    let in_panel = pt.x >= panel.left
        && pt.x <= panel.right
        && pt.y >= panel.top
        && pt.y <= panel.bottom;
    in_icon || in_panel
}

/// Panel goes to the right of the icon, flipped left when it would run off
/// the primary work area's right edge.
fn panel_rect(icon: &RECT) -> RECT {
    let screen_w = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN,
        )
    };
    let mut left = icon.right + PANEL_GAP;
    if left + PANEL_W > screen_w {
        left = icon.left - PANEL_GAP - PANEL_W;
    }
    let top = icon.top.max(0);
    RECT {
        left,
        top,
        right: left + PANEL_W,
        bottom: top + PANEL_H,
    }
}

fn show_panel(app: &AppHandle, icon_rect: &RECT, payload: ShowPayload) {
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let r = panel_rect(icon_rect);
    let _ = app.emit("nugget:show", payload);
    let _ = win.set_position(PhysicalPosition::new(r.left, r.top));
    let _ = win.show();
}

fn hide_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}
