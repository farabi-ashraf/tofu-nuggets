//! Where per-user runtime data lives (settings.json, index.db, tofu.log).
//!
//! Windows keeps Tauri's identifier-named directory
//! (`%APPDATA%\com.tofunuggets.app`) — shipped installs already store settings
//! and the index there, and renaming it would strand them.
//!
//! macOS cannot use that name: the identifier ends in `.app`, so Finder treats
//! `~/Library/Application Support/com.tofunuggets.app` as an application
//! bundle and refuses to open it ("damaged or incomplete"), which also hides
//! the log from anyone trying to send it in. macOS has no shipped users yet,
//! so it gets a plain human-readable folder instead.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
const MACOS_DIR_NAME: &str = "Tofu Nuggets";

pub fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let dir = app
            .path()
            .home_dir()?
            .join("Library/Application Support")
            .join(MACOS_DIR_NAME);
        Ok(dir)
    }
    #[cfg(not(target_os = "macos"))]
    {
        app.path().app_data_dir()
    }
}
