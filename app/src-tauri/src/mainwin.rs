//! Main window: the "all nuggets" list, backed by the SQLite index.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::editor;
use crate::index::{Entry, NuggetIndex};

pub const LABEL: &str = "main";

#[tauri::command]
pub fn list_nuggets(index: tauri::State<Arc<Mutex<NuggetIndex>>>) -> Result<Vec<Entry>, String> {
    index
        .lock()
        .map_err(|e| e.to_string())?
        .all()
        .map_err(|e| e.to_string())
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
