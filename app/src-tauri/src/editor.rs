//! Nugget editor window + save pipeline.
//!
//! Opened by the global hotkey for the icon under the cursor (or the selected
//! icon). Same freshly-created-page pattern as the overlay: payload stashed in
//! state, pulled by the page on load, also emitted for a warm window.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::icons::{self, DesktopIcons};
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

/// Save a note. An empty note (no visible text) counts as removal: the
/// sidecar is deleted, so the badge dot and hover panel disappear with it.
/// Returns `true` when the nugget was removed instead of written.
#[tauri::command]
pub fn save_nugget(
    app: AppHandle,
    path: String,
    html: String,
    index: State<Arc<Mutex<NuggetIndex>>>,
) -> Result<bool, String> {
    let item = std::path::PathBuf::from(&path);
    if storage::is_empty_html(&html) {
        remove_nugget(&app, &item, &index)?;
        return Ok(true);
    }
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
        // write_nugget stamps this itself when it redirects; irrelevant to a
        // primary sidecar (the path names the target).
        target: None,
    };
    storage::write_nugget(&item, &nugget).map_err(|e| e.to_string())?;
    if let Ok(idx) = index.lock() {
        idx.upsert_item(&item);
    }
    // Let an open main window refresh its list.
    let _ = app.emit("nuggets:changed", ());
    Ok(false)
}

/// Explicit delete from the main window's list.
#[tauri::command]
pub fn delete_nugget(
    app: AppHandle,
    path: String,
    index: State<Arc<Mutex<NuggetIndex>>>,
) -> Result<(), String> {
    remove_nugget(&app, std::path::Path::new(&path), &index)
}

fn remove_nugget(
    app: &AppHandle,
    item: &std::path::Path,
    index: &State<Arc<Mutex<NuggetIndex>>>,
) -> Result<(), String> {
    storage::delete_nugget(item).map_err(|e| e.to_string())?;
    if let Ok(idx) = index.lock() {
        idx.remove_item(item);
    }
    let _ = app.emit("nuggets:changed", ());
    Ok(())
}

/// Hotkey entry: open the editor for the icon under the cursor, falling back
/// to the selected desktop icon. Runs on the main thread (shortcut handler).
pub fn open_for_target(app: &AppHandle) {
    let provider = match icons::new_icons() {
        Ok(u) => u,
        Err(e) => {
            crate::logfile::log(app, &format!("editor: icon provider init failed: {e}"));
            return;
        }
    };

    let under_cursor = icons::cursor_pos().and_then(|(x, y)| provider.icon_at(x, y));
    let target = under_cursor.or_else(|| provider.selected_icon());

    let Some(icon) = target else {
        crate::logfile::log(app, "editor: no desktop icon under cursor or selected");
        return;
    };
    let Some(path) = icon.path.clone() else {
        crate::logfile::log(
            app,
            &format!(
                "editor: '{}' has no filesystem path (virtual icon)",
                icon.name
            ),
        );
        return;
    };
    crate::logfile::log(app, &format!("editor: opening for '{}'", icon.name));
    open_editor(app, &icon.name, path);
}

/// Open the editor for an explicit path (from the main window list).
pub fn open_for_path(app: &AppHandle, path: std::path::PathBuf) {
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    open_editor(app, &name, path);
}

fn open_editor(app: &AppHandle, name: &str, path: std::path::PathBuf) {
    // The quick-view panel would fight the editor visually.
    if let Some(win) = app.get_webview_window(overlay::LABEL) {
        let _ = win.hide();
    }

    let html = storage::read_nugget(&path)
        .map(|n| n.html)
        .unwrap_or_default();
    let payload = EditPayload {
        name: name.to_string(),
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
        Err(e) => crate::logfile::log(app, &format!("editor: create failed: {e}")),
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
