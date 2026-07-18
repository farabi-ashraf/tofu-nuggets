#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appstate;
mod badges;
mod desktop;
mod editor;
mod hotkey;
mod hover;
mod index;
mod links;
mod logfile;
mod mainwin;
mod overlay;
mod settings;
mod storage;
mod tray;
mod watcher;

use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

use appstate::Paused;

fn main() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch (autostart + manual, or
        // double-click) hands off to the running instance instead of starting
        // a duplicate hover engine and clashing on the hotkey.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            logfile::log(app, "second launch: opening main window");
            let app = app.clone();
            std::thread::spawn(move || mainwin::show(&app));
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(hover::CurrentNugget::default())
        .manage(editor::CurrentEdit::default())
        .manage(Paused::default())
        .invoke_handler(tauri::generate_handler![
            hover::get_current_nugget,
            editor::get_current_edit,
            editor::save_nugget,
            editor::delete_nugget,
            links::open_in_explorer,
            links::open_external,
            mainwin::list_nuggets,
            mainwin::edit_nugget,
            settings::get_settings,
            settings::set_settings
        ])
        .setup(|app| {
            // Warm overlay at startup; the hover engine destroys it after
            // idle and recreates it on demand.
            overlay::create(app.handle())?;

            // Silence the desktop's native infotips so our panel is the sole
            // hover surface (re-applied by the badge layer after Explorer
            // restarts).
            desktop::suppress_desktop_infotips();

            let roots = desktop::desktop_dirs();
            let db_path = app.path().app_data_dir()?.join("index.db");
            let mut idx = index::NuggetIndex::open(&db_path)?;
            if let Err(e) = idx.rebuild(&roots) {
                eprintln!("index rebuild failed: {e}");
            }
            let idx = Arc::new(Mutex::new(idx));
            watcher::spawn(roots, idx.clone());
            app.manage(idx);

            let settings: settings::Shared = Arc::new(Mutex::new(settings::load(app.handle())));
            app.manage(settings.clone());

            // Hotkey comes from settings; a failed registration (clash with
            // another app) must not kill the app — the user can pick a
            // different combination in Settings.
            let hk = settings
                .lock()
                .map(|s| s.hotkey.clone())
                .unwrap_or_else(|_| "ctrl+shift+n".into());
            match hotkey::register(app.handle(), &hk) {
                Ok(()) => logfile::log(app.handle(), &format!("startup: hotkey '{hk}' registered")),
                Err(e) => logfile::log(app.handle(), &format!("startup: {e}")),
            }

            let paused = app.state::<Paused>().inner().clone();
            hover::spawn(app.handle().clone(), paused.clone());
            badges::spawn(paused, settings);

            tray::build(app.handle())?;

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
