//! System tray icon + menu: open the main window, pause hover, toggle
//! autostart, quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::appstate::Paused;
use crate::{mainwin, settings};

const OPEN: &str = "open";
const PAUSE: &str = "pause";
const SETTINGS: &str = "settings";
const AUTOSTART: &str = "autostart";
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
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[&open, &sep1, &pause, &settings, &autostart, &sep2, &quit],
    )?;

    TrayIconBuilder::with_id("tofu-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Tofu Nuggets")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            OPEN => mainwin::show(app),
            PAUSE => {
                let paused = app.state::<Paused>().toggle();
                eprintln!("hover {}", if paused { "paused" } else { "resumed" });
            }
            SETTINGS => settings::show(app),
            AUTOSTART => {
                let mgr = app.autolaunch();
                let now_on = mgr.is_enabled().unwrap_or(false);
                let _ = if now_on { mgr.disable() } else { mgr.enable() };
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
                mainwin::show(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
