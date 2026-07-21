//! Append-only debug log in the app-data dir. The app runs headless in the
//! tray on installed machines, so eprintln is invisible there — this file is
//! what a remote user can actually send back when something silently fails.

use std::io::Write;

use tauri::AppHandle;

pub fn log(app: &AppHandle, msg: &str) {
    eprintln!("{msg}");
    let Ok(dir) = crate::paths::data_dir(app) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tofu.log");
    // Cap growth: start over past ~512 KB.
    if path
        .metadata()
        .map(|m| m.len() > 512 * 1024)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(&path);
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}
