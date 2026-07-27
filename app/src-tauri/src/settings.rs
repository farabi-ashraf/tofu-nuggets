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
    /// One shared badge colour for both platforms, by name (see `BADGE_COLORS`).
    /// Windows paints its dot in it; macOS maps it to the Finder tag colour code.
    pub badge_color: String,
    /// Global note hotkey, tauri shortcut syntax (e.g. "ctrl+shift+n").
    pub hotkey: String,
}

/// The seven selectable badge colours, in UI (swatch) order. The stored
/// `badge_color` is one of these names; anything else falls back to the default.
/// Shared across platforms on purpose (platform-parity, docs/V0.1.3.md): one
/// setting, one palette, the same choice on Windows and macOS.
pub const BADGE_COLORS: [&str; 7] = ["gray", "green", "purple", "blue", "yellow", "red", "orange"];

/// Default badge colour name — orange, the product accent.
pub const DEFAULT_BADGE_COLOR: &str = "orange";

/// macOS Finder tag colour code for a badge colour name (`0` none, `1` gray,
/// `2` green, `3` purple, `4` blue, `5` yellow, `6` red, `7` orange — the codes
/// Finder stores in `_kMDItemUserTags`). Unknown/empty names map to orange (7),
/// matching the setting default, so a corrupt value never writes a "no colour"
/// tag. This is the mapping `tags.rs` writes; unit-tested here (cross-platform).
/// Only macOS (`tags.rs`) reads it; kept compiled everywhere so it type-checks.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn badge_color_code(name: &str) -> u8 {
    match name {
        "gray" => 1,
        "green" => 2,
        "purple" => 3,
        "blue" => 4,
        "yellow" => 5,
        "red" => 6,
        "orange" => 7,
        _ => 7,
    }
}

/// Straight-alpha RGB the Windows badge dot and Explorer pill dot paint for a
/// badge colour name. Chosen to read like the matching macOS Finder tag colour
/// so the two platforms look like one product. Unknown names map to orange (the
/// original warm accent), matching the setting default. Only the Windows dot
/// painters read this; kept compiled on every platform so it still type-checks.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn badge_rgb(name: &str) -> (u8, u8, u8) {
    match name {
        "gray" => (0x8E, 0x8E, 0x93),
        "green" => (0x5B, 0xC2, 0x36),
        "purple" => (0xC0, 0x6B, 0xDE),
        "blue" => (0x3B, 0x8E, 0xF3),
        "yellow" => (0xF4, 0xC8, 0x1F),
        "red" => (0xF5, 0x4E, 0x4E),
        // orange + fallback: the warm accent the app shipped with.
        _ => (0xF5, 0x8F, 0x3C),
    }
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
            badge_color: DEFAULT_BADGE_COLOR.into(),
            hotkey: "ctrl+shift+n".into(),
        }
    }
}

impl Settings {
    /// Clamp free-form values from the UI into supported ranges.
    fn normalized(mut self) -> Self {
        self.panel_scale = self.panel_scale.clamp(1.0, 1.5);
        // An unknown badge colour (hand-edited file, older/newer build) would
        // otherwise paint nothing on Windows and write a "no colour" tag on
        // macOS — snap it back to the default instead.
        if !BADGE_COLORS.contains(&self.badge_color.as_str()) {
            self.badge_color = DEFAULT_BADGE_COLOR.into();
        }
        self
    }
}

/// Managed state type; also read directly by the hover engine and badge layer.
pub type Shared = Arc<Mutex<Settings>>;

fn file_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = crate::paths::data_dir(app).ok()?;
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
pub fn set_settings(
    app: AppHandle,
    state: State<Shared>,
    settings: Settings,
) -> Result<(), String> {
    let next = settings.normalized();
    // A changed hotkey must actually register before it is persisted; on
    // failure the old binding stays live and the caller gets the error.
    let (old_hotkey, _old_badges, _old_color) = state
        .lock()
        .map(|g| (g.hotkey.clone(), g.badges, g.badge_color.clone()))
        .unwrap_or_else(|_| ("ctrl+shift+n".into(), true, DEFAULT_BADGE_COLOR.into()));
    if next.hotkey != old_hotkey {
        crate::hotkey::reregister(&app, &old_hotkey, &next.hotkey)?;
        crate::logfile::log(&app, &format!("hotkey changed to '{}'", next.hotkey));
    }
    if let Ok(mut g) = state.lock() {
        *g = next.clone();
    }
    write(&app, &next);

    // macOS badges are Finder tags: turning badges off strips our tag from every
    // annotated file, turning them back on re-tags them (D3). Windows draws its
    // dots live from this flag, so it needs no such sweep. Off the main thread —
    // one xattr syscall per note.
    #[cfg(target_os = "macos")]
    if next.badges != _old_badges {
        let app2 = app.clone();
        let want_tagged = next.badges;
        let index = app
            .state::<Arc<Mutex<crate::index::NuggetIndex>>>()
            .inner()
            .clone();
        std::thread::spawn(move || {
            let paths: Vec<std::path::PathBuf> = index
                .lock()
                .ok()
                .and_then(|i| i.all().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|e| std::path::PathBuf::from(e.path))
                .collect();
            crate::tags::resync(&app2, paths, want_tagged);
        });
    }

    // macOS: a new badge colour rewrites the `Nugget` tag on every annotated
    // file so Finder redraws the dot in the chosen colour. `set_note_tag`'s
    // read-modify-write drops the stale `Nugget\n<old>` and writes `Nugget\n<new>`
    // (tags.rs), so this is just a resync at the new colour. No-op when badges
    // are off (nothing is tagged). Skipped when badges also toggled this call —
    // that branch already resynced. Off the main thread: one xattr per note.
    #[cfg(target_os = "macos")]
    if next.badge_color != _old_color && next.badges && next.badges == _old_badges {
        let app2 = app.clone();
        let index = app
            .state::<Arc<Mutex<crate::index::NuggetIndex>>>()
            .inner()
            .clone();
        std::thread::spawn(move || {
            let paths: Vec<std::path::PathBuf> = index
                .lock()
                .ok()
                .and_then(|i| i.all().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|e| std::path::PathBuf::from(e.path))
                .collect();
            crate::tags::resync(&app2, paths, true);
        });
    }

    // Windows draws its dots live from the setting; a colour change moves no
    // icons, so nudge both dot surfaces to repaint now instead of waiting for
    // the next refresh tick.
    #[cfg(windows)]
    if next.badge_color != _old_color {
        crate::badges::wake();
        crate::pill::wake();
    }

    // Every window re-applies live (theme.js listener).
    let _ = app.emit("settings:changed", next);
    Ok(())
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
        Err(e) => crate::logfile::log(app, &format!("settings window create failed: {e}")),
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
        assert_eq!(d.badge_color, "orange");
        assert_eq!(d.hotkey, "ctrl+shift+n");
    }

    #[test]
    fn badge_color_names_map_to_finder_codes() {
        // The full palette, in the order Finder assigns its colour codes.
        assert_eq!(badge_color_code("gray"), 1);
        assert_eq!(badge_color_code("green"), 2);
        assert_eq!(badge_color_code("purple"), 3);
        assert_eq!(badge_color_code("blue"), 4);
        assert_eq!(badge_color_code("yellow"), 5);
        assert_eq!(badge_color_code("red"), 6);
        assert_eq!(badge_color_code("orange"), 7);
    }

    #[test]
    fn every_palette_name_has_a_nonzero_code() {
        // A "no colour" (0) tag would show a dot-less grey pill in Finder —
        // never valid for one of our colours.
        for name in BADGE_COLORS {
            assert_ne!(badge_color_code(name), 0, "{name} mapped to code 0");
        }
    }

    #[test]
    fn unknown_badge_color_falls_back_to_orange_code() {
        assert_eq!(badge_color_code("chartreuse"), badge_color_code("orange"));
        assert_eq!(badge_color_code(""), 7);
    }

    #[test]
    fn unknown_badge_color_normalizes_to_default() {
        let s = Settings {
            badge_color: "chartreuse".into(),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(s.badge_color, "orange");
    }

    #[test]
    fn known_badge_color_survives_normalization() {
        let s = Settings {
            badge_color: "blue".into(),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(s.badge_color, "blue");
    }

    #[test]
    fn missing_fields_backfill_from_default() {
        // A partial/old settings file must not fail to load; #[serde(default)]
        // fills the gaps.
        let s: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert_eq!(s.font_size, "m"); // backfilled
        assert!(s.badges); // backfilled
        assert_eq!(s.badge_color, "orange"); // backfilled
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
            badge_color: "blue".into(),
            hotkey: "ctrl+alt+j".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.font_size, "xl");
        assert_eq!(back.panel_scale, 1.4);
        assert_eq!(back.theme, "dark");
        assert!(back.reduced_motion);
        assert!(back.high_contrast);
        assert!(!back.badges);
        assert_eq!(back.badge_color, "blue");
        assert_eq!(back.hotkey, "ctrl+alt+j");
    }
}
