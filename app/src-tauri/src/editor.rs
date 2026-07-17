//! Nugget editor window + save pipeline.
//!
//! Opened by the global hotkey for the icon under the cursor (or the selected
//! icon). Same freshly-created-page pattern as the overlay: payload stashed in
//! state, pulled by the page on load, also emitted for a warm window.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::desktop::DesktopUia;
use crate::index::NuggetIndex;
use crate::{overlay, storage};

pub const LABEL: &str = "editor";

#[derive(Clone, Serialize)]
pub struct EditPayload {
    name: String,
    path: String,
    html: String,
}

#[derive(Default)]
pub struct CurrentEdit(Mutex<Option<EditPayload>>);

#[tauri::command]
pub fn get_current_edit(state: State<CurrentEdit>) -> Option<EditPayload> {
    state.0.lock().ok().and_then(|g| g.clone())
}

#[tauri::command]
pub fn save_nugget(
    path: String,
    html: String,
    index: State<Arc<Mutex<NuggetIndex>>>,
) -> Result<(), String> {
    let item = std::path::PathBuf::from(&path);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let created_ms = storage::read_nugget(&item)
        .map(|n| n.created_ms)
        .unwrap_or(now);
    let nugget = storage::Nugget {
        schema: storage::SCHEMA_VERSION,
        html,
        created_ms,
        modified_ms: now,
    };
    storage::write_nugget(&item, &nugget).map_err(|e| e.to_string())?;
    if let Ok(idx) = index.lock() {
        idx.upsert_item(&item);
    }
    Ok(())
}

/// Hotkey entry: open the editor for the icon under the cursor, falling back
/// to the selected desktop icon. Runs on the main thread (shortcut handler).
pub fn open_for_target(app: &AppHandle) {
    let Ok(uia) = DesktopUia::new() else {
        eprintln!("editor: UIA init failed");
        return;
    };

    let mut pt = windows::Win32::Foundation::POINT::default();
    let under_cursor = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt)
            .ok()
            .and_then(|_| uia.icon_at(pt))
    };
    let target = under_cursor.or_else(|| uia.selected_icon());

    let Some(icon) = target else {
        eprintln!("editor: no desktop icon under cursor or selected");
        return;
    };
    let Some(path) = icon.path.clone() else {
        eprintln!(
            "editor: '{}' has no filesystem path (virtual icon)",
            icon.name
        );
        return;
    };

    // The quick-view panel would fight the editor visually.
    if let Some(win) = app.get_webview_window(overlay::LABEL) {
        let _ = win.hide();
    }

    let html = storage::read_nugget(&path)
        .map(|n| n.html)
        .unwrap_or_default();
    let payload = EditPayload {
        name: icon.name.clone(),
        path: path.display().to_string(),
        html,
    };
    if let Ok(mut cur) = app.state::<CurrentEdit>().0.lock() {
        *cur = Some(payload.clone());
    }
    let _ = app.emit("edit:show", payload);

    match get_or_create(app) {
        Ok(win) => {
            let _ = win.show();
            let _ = win.set_focus();
        }
        Err(e) => eprintln!("editor: create failed: {e}"),
    }
}

fn get_or_create(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(win) = app.get_webview_window(LABEL) {
        return Ok(win);
    }
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("editor.html".into()))
        .title("Edit Nugget")
        .inner_size(480.0, 440.0)
        .min_inner_size(360.0, 300.0)
        .decorations(false)
        .shadow(true)
        .center()
        .visible(false)
        .build()?;

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
            DWMWCP_ROUND,
        };
        let hwnd = HWND(win.hwnd()?.0);
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                std::mem::size_of_val(&dark) as u32,
            );
            let corners = DWMWCP_ROUND.0;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corners as *const _ as _,
                std::mem::size_of_val(&corners) as u32,
            );
        }
    }

    Ok(win)
}
