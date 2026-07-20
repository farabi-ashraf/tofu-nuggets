//! Global note hotkey, re-registerable at runtime from settings so users can
//! resolve clashes with shortcuts already taken on their machine.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::{editor, icons, logfile};

pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let sc: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("invalid hotkey '{hotkey}': {e:?}"))?;
    app.global_shortcut()
        .on_shortcut(sc, |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                logfile::log(app, "hotkey pressed");
                // Window creation is only safe from a plain thread (see
                // ARCHITECTURE.md); Windows UIA needs COM on that thread.
                let app = app.clone();
                std::thread::spawn(move || {
                    icons::init_thread();
                    editor::open_for_target(&app);
                });
            }
        })
        .map_err(|e| format!("could not register '{hotkey}': {e}"))
}

/// Swap the registered hotkey. On failure the old binding is restored so the
/// app is never left without a hotkey.
pub fn reregister(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    if old == new {
        return Ok(());
    }
    if let Ok(sc) = old.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(sc);
    }
    register(app, new).inspect_err(|_| {
        let _ = register(app, old);
    })
}
