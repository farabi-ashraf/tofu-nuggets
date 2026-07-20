//! Main window: the "all nuggets" list, backed by the SQLite index.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::index::{Entry, NuggetIndex};
use crate::{desktop, editor, storage};

pub const LABEL: &str = "main";

#[tauri::command]
pub fn list_nuggets(index: tauri::State<Arc<Mutex<NuggetIndex>>>) -> Result<Vec<Entry>, String> {
    index
        .lock()
        .map_err(|e| e.to_string())?
        .all()
        .map_err(|e| e.to_string())
}

/// Danger-zone "Delete all notes": remove every sidecar — indexed notes (by
/// item path, which also clears redirected sidecars and empty `.nuggets` dirs)
/// plus any strays left in the desktop roots — then clear the index. Sidecars
/// are the source of truth, so this is destructive and irreversible; the UI
/// two-step-confirms before calling. Returns the count of indexed notes removed.
#[tauri::command]
pub fn delete_all_nuggets(
    app: AppHandle,
    index: tauri::State<Arc<Mutex<NuggetIndex>>>,
) -> Result<usize, String> {
    // 1. Delete every indexed note by its item path.
    let entries = index
        .lock()
        .map_err(|e| e.to_string())?
        .all()
        .map_err(|e| e.to_string())?;
    let mut removed = 0usize;
    for e in &entries {
        if storage::delete_nugget(std::path::Path::new(&e.path)).is_ok() {
            removed += 1;
        }
    }

    // 2. Sweep strays: leftover sidecars in each desktop root's `.nuggets`
    //    (including redirected ones) and in its direct child folders' `.nuggets`.
    for root in desktop::desktop_dirs() {
        storage::purge_sidecar_dir(&root.join(storage::SIDECAR_DIR));
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                let d = e.path();
                if d.is_dir()
                    && d.file_name()
                        .map(|f| f != storage::SIDECAR_DIR)
                        .unwrap_or(false)
                {
                    storage::purge_sidecar_dir(&d.join(storage::SIDECAR_DIR));
                }
            }
        }
    }

    // 3. Clear the (now-stale) index and refresh open views.
    index
        .lock()
        .map_err(|e| e.to_string())?
        .clear()
        .map_err(|e| e.to_string())?;
    let _ = app.emit("nuggets:changed", ());
    crate::logfile::log(
        &app,
        &format!("delete all notes: removed {removed} note(s)"),
    );
    Ok(removed)
}

/// Open the editor for a path chosen in the list. Window creation must run on
/// the main thread; commands run on the async runtime, so marshal it over.
#[tauri::command]
pub fn edit_nugget(app: AppHandle, path: String) {
    // WebviewWindowBuilder::build() deadlocks on the async command thread, but
    // works from a plain worker thread (same path the hover engine uses to
    // recreate the overlay). Marshal the open onto one.
    let target = std::path::PathBuf::from(path);
    std::thread::spawn(move || {
        editor::open_for_path(&app, target);
    });
}

/// Open (or focus) the all-nuggets list from the panel's ☰ button. Same
/// marshaling as `edit_nugget`: window creation deadlocks on the async
/// command thread, so hop to a worker thread.
#[tauri::command]
pub fn open_main(app: AppHandle) {
    std::thread::spawn(move || {
        show(&app);
    });
}

pub fn show(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return;
    }
    match create(app) {
        Ok(win) => {
            let _ = win.show();
            let _ = win.set_focus();
        }
        Err(e) => eprintln!("main window create failed: {e}"),
    }
}

fn create(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("main.html".into()))
        .title("Tofu Nuggets")
        .inner_size(720.0, 560.0)
        .min_inner_size(420.0, 320.0)
        .visible(false)
        .build()?;

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
        let hwnd = HWND(win.hwnd()?.0);
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                std::mem::size_of_val(&dark) as u32,
            );
        }
    }

    Ok(win)
}
