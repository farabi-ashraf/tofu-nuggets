#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod badges;
mod desktop;
mod hover;
mod index;
mod overlay;
mod storage;
mod watcher;

use std::sync::{Arc, Mutex};

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(hover::CurrentNugget::default())
        .invoke_handler(tauri::generate_handler![hover::get_current_nugget])
        .setup(|app| {
            // Warm overlay at startup; the hover engine destroys it after
            // idle and recreates it on demand.
            overlay::create(app.handle())?;

            let roots = desktop::desktop_dirs();
            let db_path = app.path().app_data_dir()?.join("index.db");
            let mut idx = index::NuggetIndex::open(&db_path)?;
            if let Err(e) = idx.rebuild(&roots) {
                eprintln!("index rebuild failed: {e}");
            }
            let idx = Arc::new(Mutex::new(idx));
            watcher::spawn(roots, idx.clone());
            app.manage(idx);

            hover::spawn(app.handle().clone());
            badges::spawn();

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Background app: keep running when the overlay window (often the
            // only window) is destroyed for idle release.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
