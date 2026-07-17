#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appstate;
mod badges;
mod desktop;
mod editor;
mod hover;
mod index;
mod links;
mod mainwin;
mod overlay;
mod settings;
mod storage;
mod tray;
mod watcher;

use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};

use appstate::Paused;

fn main() {
    let hotkey: Shortcut = "ctrl+shift+n".parse().expect("valid hotkey");
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([hotkey])
                .expect("register hotkey")
                .with_handler(move |app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed && *shortcut == hotkey {
                        editor::open_for_target(app);
                    }
                })
                .build(),
        )
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
