#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appstate;
#[cfg(windows)]
mod badges;
#[cfg(target_os = "macos")]
mod badges_mac;
#[cfg(windows)]
mod desktop;
#[cfg(target_os = "macos")]
mod desktop_mac;
mod editor;
mod hotkey;
mod hover;
mod icons;
mod index;
mod links;
mod logfile;
mod mainwin;
mod overlay;
mod paths;
mod settings;
mod storage;
mod tray;
mod updater;
mod watcher;

use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

use appstate::Paused;

/// No webview window can be built — almost always a missing/broken WebView2
/// Runtime (docs/V0.1.1.md A1). A webview is unavailable by definition, so
/// this must be a native dialog; offer the runtime download page. Windows-only
/// failure mode: macOS's WKWebView is part of the OS.
#[cfg(windows)]
fn webview_missing_alert() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONERROR, MB_YESNO, SHOW_WINDOW_CMD, SW_SHOWNORMAL,
    };
    unsafe {
        let choice = MessageBoxW(
            None,
            w!("Tofu Nuggets could not start because the Microsoft WebView2 Runtime is missing or not working.\n\nInstall the WebView2 Runtime (Evergreen), then start Tofu Nuggets again.\n\nOpen the download page now?"),
            w!("Tofu Nuggets — WebView2 Runtime required"),
            MB_YESNO | MB_ICONERROR,
        );
        if choice == IDYES {
            ShellExecuteW(
                None,
                w!("open"),
                w!("https://developer.microsoft.com/en-us/microsoft-edge/webview2/"),
                PCWSTR::null(),
                PCWSTR::null(),
                SHOW_WINDOW_CMD(SW_SHOWNORMAL.0),
            );
        }
    }
}

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(hover::CurrentNugget::default())
        .manage(editor::CurrentEdit::default())
        .manage(Paused::default())
        .invoke_handler(tauri::generate_handler![
            hover::get_current_nugget,
            icons::accessibility_status,
            icons::open_accessibility_pane,
            editor::get_current_edit,
            editor::save_nugget,
            editor::delete_nugget,
            links::open_in_explorer,
            links::open_external,
            mainwin::list_nuggets,
            mainwin::edit_nugget,
            mainwin::open_main,
            mainwin::delete_all_nuggets,
            overlay::hide_overlay,
            settings::get_settings,
            settings::set_settings
        ])
        .setup(|app| {
            // Tofu Nuggets lives in the tray/menu bar, not the Dock: as a
            // Regular app macOS treats it as window-driven, which both puts a
            // Dock icon on a background utility and ties its lifetime to
            // having windows around. Accessory is the menu-bar-agent policy.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Warm overlay at startup; the hover engine destroys it after
            // idle and recreates it on demand. Failure here is the
            // WebView2-missing signature ("tray alive, all windows dead") —
            // tell the user with a native dialog instead of dying silently.
            match overlay::create(app.handle()) {
                // macOS keeps the panel parked off-screen rather than hidden,
                // because an app with no visible window is terminated (see
                // overlay::park).
                #[cfg(target_os = "macos")]
                Ok(win) => overlay::park(&win),
                #[cfg(not(target_os = "macos"))]
                Ok(_) => {}
                Err(e) => {
                    logfile::log(
                        app.handle(),
                        &format!("startup: overlay create failed: {e}"),
                    );
                    #[cfg(windows)]
                    webview_missing_alert();
                    std::process::exit(1);
                }
            }

            // Silence the desktop's native infotips so our panel is the sole
            // hover surface (re-applied by the badge layer after Explorer
            // restarts).
            icons::suppress_desktop_infotips();

            let roots = icons::desktop_dirs();
            // Redirect sidecars for unwritable parents (Public Desktop) into
            // the user's own desktop `.nuggets` (docs/V0.1.1.md A4). First
            // root is FOLDERID_Desktop.
            storage::set_redirect_root(roots.first().cloned());
            let db_path = paths::data_dir(app.handle())?.join("index.db");
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
            #[cfg(windows)]
            badges::spawn(paused, settings);
            #[cfg(target_os = "macos")]
            badges_mac::spawn(app.handle().clone(), paused, settings);

            tray::build(app.handle())?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Background app: keep running when the overlay window (often the
            // only window) is destroyed for idle release.
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                logfile::log(app, &format!("exit requested (code {code:?})"));
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            // These three exist to tell a clean shutdown apart from a crash in
            // the log: a macOS tester saw the process vanish whenever the
            // panel hid with no other window open, and silence here is what
            // made that ambiguous.
            tauri::RunEvent::Exit => logfile::log(app, "exiting"),
            tauri::RunEvent::WindowEvent { label, event, .. } => match event {
                tauri::WindowEvent::Destroyed => {
                    logfile::log(app, &format!("window '{label}' destroyed"))
                }
                // macOS terminates an app once its last visible window closes,
                // and that path does not raise ExitRequested — the log showed
                // `window 'main' destroyed` followed by `exiting` with no
                // request in between, so `prevent_exit` never got a say. Keep
                // the windows alive and merely hidden, which is also what a
                // Mac user expects from closing a window: the app stays.
                #[cfg(target_os = "macos")]
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(win) = app.get_webview_window(&label) {
                        let _ = win.hide();
                    }
                    logfile::log(app, &format!("window '{label}' hidden instead of closed"));
                }
                #[cfg(not(target_os = "macos"))]
                tauri::WindowEvent::CloseRequested { .. } => {
                    logfile::log(app, &format!("window '{label}' close requested"))
                }
                _ => {}
            },
            _ => {}
        });
}
