//! User settings: accessibility + display preferences, persisted as JSON in
//! the app-data dir and applied live across every window via `settings:changed`.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Font-size preset: "s" | "m" | "l" | "xl" (mapped to a scale in the UI).
    pub font_size: String,
    /// Overlay panel zoom, clamped 1.0–1.5.
    pub panel_scale: f64,
    /// "dark" | "light" | "system".
    pub theme: String,
    /// Force-disable animations (OS Reduced Motion is also honored).
    pub reduced_motion: bool,
    /// Force solid high-contrast colors (OS High Contrast is also honored).
    pub high_contrast: bool,
    /// Draw the badge dots on tagged icons.
    pub badges: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: "m".into(),
            panel_scale: 1.0,
            theme: "system".into(),
            reduced_motion: false,
            high_contrast: false,
            badges: true,
        }
    }
}

impl Settings {
    /// Clamp free-form values from the UI into supported ranges.
    fn normalized(mut self) -> Self {
        self.panel_scale = self.panel_scale.clamp(1.0, 1.5);
        self
    }
}

/// Managed state type; also read directly by the hover engine and badge layer.
pub type Shared = Arc<Mutex<Settings>>;

fn file_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("settings.json"))
}

/// Load from disk, falling back to defaults on missing/corrupt file.
pub fn load(app: &AppHandle) -> Settings {
    file_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(app: &AppHandle, s: &Settings) {
    if let (Some(p), Ok(json)) = (file_path(app), serde_json::to_string_pretty(s)) {
        let _ = std::fs::write(p, json);
    }
}

#[tauri::command]
pub fn get_settings(state: State<Shared>) -> Settings {
    state.lock().map(|g| g.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<Shared>, settings: Settings) {
    let next = settings.normalized();
    if let Ok(mut g) = state.lock() {
        *g = next.clone();
    }
    write(&app, &next);
    // Every window re-applies live (theme.js listener).
    let _ = app.emit("settings:changed", next);
}

/// Open (or focus) the settings window. Called from the tray, which runs on a
/// context where window creation is safe.
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
        Err(e) => eprintln!("settings window create failed: {e}"),
    }
}

fn create(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings.html".into()))
        .title("Tofu Nuggets — Settings")
        .inner_size(440.0, 560.0)
        .resizable(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let d = Settings::default();
        assert_eq!(d.font_size, "m");
        assert_eq!(d.panel_scale, 1.0);
        assert_eq!(d.theme, "system");
        assert!(d.badges);
        assert!(!d.reduced_motion);
        assert!(!d.high_contrast);
    }

    #[test]
    fn missing_fields_backfill_from_default() {
        // A partial/old settings file must not fail to load; #[serde(default)]
        // fills the gaps.
        let s: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert_eq!(s.font_size, "m"); // backfilled
        assert!(s.badges); // backfilled
    }

    #[test]
    fn empty_object_is_all_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.panel_scale, Settings::default().panel_scale);
    }

    #[test]
    fn panel_scale_is_clamped() {
        let low = Settings {
            panel_scale: 0.2,
            ..Settings::default()
        }
        .normalized();
        assert_eq!(low.panel_scale, 1.0);

        let high = Settings {
            panel_scale: 9.0,
            ..Settings::default()
        }
        .normalized();
        assert_eq!(high.panel_scale, 1.5);

        let ok = Settings {
            panel_scale: 1.25,
            ..Settings::default()
        }
        .normalized();
        assert_eq!(ok.panel_scale, 1.25);
    }

    #[test]
    fn roundtrips_through_json() {
        let s = Settings {
            font_size: "xl".into(),
            panel_scale: 1.4,
            theme: "dark".into(),
            reduced_motion: true,
            high_contrast: true,
            badges: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.font_size, "xl");
        assert_eq!(back.panel_scale, 1.4);
        assert_eq!(back.theme, "dark");
        assert!(back.reduced_motion);
        assert!(back.high_contrast);
        assert!(!back.badges);
    }
}
