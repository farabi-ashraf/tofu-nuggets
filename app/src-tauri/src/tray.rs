//! System tray icon + menu: open the main window, pause hover, toggle
//! autostart, quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::appstate::Paused;
use crate::{logfile, mainwin, settings};

/// Window creation is only reliable from a plain worker thread (see
/// ARCHITECTURE.md build() deadlock notes) — tray handlers marshal onto one.
fn on_worker(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || f(&app));
}

const OPEN: &str = "open";
const PAUSE: &str = "pause";
const SETTINGS: &str = "settings";
const AUTOSTART: &str = "autostart";
const UPDATES: &str = "updates";
const QUIT: &str = "quit";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, "Open Tofu Nuggets", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, PAUSE, "Pause hover", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS, "Settings…", true, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        AUTOSTART,
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let updates = MenuItem::with_id(app, UPDATES, "Check for updates…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &open, &sep1, &pause, &settings, &autostart, &updates, &sep2, &quit,
        ],
    )?;

    TrayIconBuilder::with_id("tofu-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Tofu Nuggets")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            OPEN => {
                logfile::log(app, "tray: open clicked");
                on_worker(app, mainwin::show);
            }
            PAUSE => {
                let paused = app.state::<Paused>().toggle();
                eprintln!("hover {}", if paused { "paused" } else { "resumed" });
            }
            SETTINGS => {
                logfile::log(app, "tray: settings clicked");
                on_worker(app, settings::show);
            }
            AUTOSTART => {
                let mgr = app.autolaunch();
                let now_on = mgr.is_enabled().unwrap_or(false);
                let _ = if now_on { mgr.disable() } else { mgr.enable() };
            }
            UPDATES => {
                logfile::log(app, "tray: check for updates clicked");
                crate::updater::check(app, true);
            }
            QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                on_worker(tray.app_handle(), mainwin::show);
            }
        })
        .build(app)?;

    Ok(())
}
