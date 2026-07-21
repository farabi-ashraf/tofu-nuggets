//! macOS badge layer: marks annotated desktop icons with a small dot.
//!
//! Same job as `badges.rs` on Windows, different machinery: none of the GDI
//! layered-window or WinEvent-hook code ports, so this is a Tauri webview
//! window instead — transparent, always-on-top, click-through
//! (`set_ignore_cursor_events`), never focusable, spanning the bounding box
//! of all displays. Dots are plain absolutely-positioned divs pushed to the
//! page via the `badges:update` event; WKWebView is part of the OS, so a
//! resident webview window has none of WebView2's process-tree cost (the
//! reason idle release stays Windows-only).
//!
//! Occlusion is per-dot like the Windows A2 model, but sourced from the
//! CoreGraphics window list (`desktop_mac::onscreen_window_rects`) rather
//! than WinEvent hooks + EnumWindows: macOS has no cheap cross-process
//! window-move hook, so a dot's visibility is re-checked on the same 2 s
//! cadence as icon/sidecar drift. The AX icon walk is skipped entirely while
//! every display is covered by a window (all dots would be occluded anyway),
//! keeping fullscreen work at one CG list call per tick.
//!
//! macOS window rules apply (see MEMORY.md, hard-won): every AppKit call —
//! position, size, ignore-cursor — is marshalled through
//! `run_on_main_thread`; the window is built on a plain `std::thread` worker
//! (builder deadlocks on async command threads); geometry is POINTS via
//! `Logical*` types, never converted to pixels. The window stays visible for
//! the whole run — dots disappear by emptying the page, never by `hide()`
//! (an app whose windows are all hidden gets terminated; the parked panel
//! usually keeps one visible, but the badge layer must not depend on that).
//!
//! The emit is unconditional every tick: the page may still be loading when
//! the first dots are computed, and re-sending a tiny array each 2 s is
//! cheaper than a pull command + stash. The page itself skips DOM work when
//! the payload is unchanged.

use std::time::Duration;

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::appstate::Paused;
use crate::desktop_mac;
use crate::icons::{DesktopIcons, IconRect};
use crate::{settings, storage};

pub const LABEL: &str = "badges";

const REFRESH: Duration = Duration::from_secs(2);
const BADGE_R: i32 = 6; // dot radius in points, matches the Windows layer

/// Dot center in points, relative to the badge window's top-left corner.
#[derive(Serialize, Clone, PartialEq)]
struct Dot {
    x: i32,
    y: i32,
}

pub fn spawn(app: AppHandle, paused: Paused, settings: settings::Shared) {
    std::thread::Builder::new()
        .name("badge-layer".into())
        .spawn(move || {
            let icons = match desktop_mac::new_icons() {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("badge layer: AX init failed: {e}");
                    return;
                }
            };
            if let Err(e) = run(&app, &icons, &paused, &settings) {
                eprintln!("badge layer failed: {e}");
            }
        })
        .expect("spawn badge layer");
}

fn run(
    app: &AppHandle,
    icons: &desktop_mac::MacIcons,
    paused: &Paused,
    settings: &settings::Shared,
) -> tauri::Result<()> {
    let win = create(app)?;
    let mut screen = IconRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    loop {
        std::thread::sleep(REFRESH);

        let displays = desktop_mac::display_bounds_pts();
        let bounds = bounding_box(&displays);
        if bounds != screen {
            screen = bounds;
            place(app, &win, &screen);
        }

        let dots = compute_dots(icons, paused, settings, &displays, &screen);
        let _ = app.emit("badges:update", dots);
    }
}

fn compute_dots(
    icons: &desktop_mac::MacIcons,
    paused: &Paused,
    settings: &settings::Shared,
    displays: &[IconRect],
    screen: &IconRect,
) -> Vec<Dot> {
    if paused.is_paused() {
        return Vec::new();
    }
    let badges_on = settings.lock().map(|s| s.badges).unwrap_or(true);
    if !badges_on {
        return Vec::new();
    }

    let occluders = desktop_mac::onscreen_window_rects();
    // Every display covered (fullscreen apps everywhere): every dot would be
    // occluded, so skip the AX walk entirely.
    if !displays.is_empty()
        && displays
            .iter()
            .all(|d| occluders.iter().any(|w| covers(w, d)))
    {
        return Vec::new();
    }

    let Ok(list) = icons.list_icons() else {
        return Vec::new();
    };
    list.iter()
        .filter(|ic| {
            ic.path
                .as_ref()
                .map(|p| storage::has_nugget(p))
                .unwrap_or(false)
        })
        .filter_map(|ic| {
            // Badge center: top-right corner of the icon cell, nudged inward
            // (same placement as the Windows layer).
            let cx = ic.rect.right - BADGE_R - 4;
            let cy = ic.rect.top + BADGE_R + 4;
            let dot = IconRect {
                left: cx - BADGE_R - 1,
                top: cy - BADGE_R - 1,
                right: cx + BADGE_R + 1,
                bottom: cy + BADGE_R + 1,
            };
            let occluded = occluders.iter().any(|w| intersects(&dot, w));
            (!occluded).then_some(Dot {
                x: cx - screen.left,
                y: cy - screen.top,
            })
        })
        .collect()
}

fn bounding_box(displays: &[IconRect]) -> IconRect {
    let mut it = displays.iter();
    let Some(first) = it.next() else {
        return IconRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
    };
    it.fold(*first, |acc, r| IconRect {
        left: acc.left.min(r.left),
        top: acc.top.min(r.top),
        right: acc.right.max(r.right),
        bottom: acc.bottom.max(r.bottom),
    })
}

fn covers(outer: &IconRect, inner: &IconRect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

fn intersects(a: &IconRect, b: &IconRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(win) = app.get_webview_window(LABEL) {
        return Ok(win);
    }
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("badges.html".into()))
        .title("Tofu Nuggets Badges")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(true)
        .background_color(tauri::utils::config::Color(0, 0, 0, 0))
        .build()?;
    let w = win.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = w.set_focusable(false);
        let _ = w.set_ignore_cursor_events(true);
    });
    Ok(win)
}

/// Move/size the badge window over the display bounding box. AppKit calls,
/// so main thread + logical (point) coordinates.
fn place(app: &AppHandle, win: &WebviewWindow, screen: &IconRect) {
    let win = win.clone();
    let screen = *screen;
    let _ = app.run_on_main_thread(move || {
        let _ = win.set_position(LogicalPosition::new(
            f64::from(screen.left),
            f64::from(screen.top),
        ));
        let _ = win.set_size(LogicalSize::new(
            f64::from(screen.right - screen.left),
            f64::from(screen.bottom - screen.top),
        ));
        let _ = win.show();
    });
}
