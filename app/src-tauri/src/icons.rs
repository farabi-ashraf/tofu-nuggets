//! Portable desktop-icon abstraction (B2: one branch, no per-platform forks).
//!
//! Declares the `DesktopIcons` trait plus the portable `Icon`/`IconRect`
//! types, and re-exports the current platform's implementation and free
//! helpers (`new_icons`, `cursor_pos`, `desktop_dirs`, …). Everything above
//! this layer (hover engine, editor, main wiring) must stay platform-agnostic:
//! no `windows::`/AX imports outside the platform modules (`desktop.rs` on
//! Windows, `desktop_mac.rs` on macOS).

use std::path::PathBuf;

/// Screen-space rectangle in physical pixels. Field-compatible with Win32
/// `RECT` so the Windows implementation converts by field copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IconRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Debug)]
pub struct Icon {
    pub name: String,
    pub rect: IconRect,
    /// `None` for virtual items (This PC, Recycle Bin) — not annotatable.
    pub path: Option<PathBuf>,
}

/// The platform's view of the desktop's icons. Implementations are
/// deliberately not `Send` (Windows UIA is apartment-bound): each worker
/// thread calls `init_thread()` then constructs its own via `new_icons()`.
pub trait DesktopIcons {
    /// Icon under the given screen point (physical px), if that point is a
    /// desktop icon.
    fn icon_at(&self, x: i32, y: i32) -> Option<Icon>;
    /// All desktop icons. Only the (Windows-only) badge layer calls this
    /// today; the macOS badge equivalent will too.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn list_icons(&self) -> Result<Vec<Icon>, String>;
    /// Currently selected desktop icon, if any (hotkey fallback target).
    fn selected_icon(&self) -> Option<Icon>;
}

/// Resolve an icon's display name to a filesystem path against the desktop
/// roots. The file manager may hide extensions (Explorer always for known
/// types, Finder per-file), so match against both the full file name and the
/// stem.
pub fn resolve_path(display_name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let target = display_name.to_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(full) = p.file_name().map(|f| f.to_string_lossy().to_lowercase()) else {
                continue;
            };
            if full == target {
                return Some(p);
            }
            if let Some(stem) = p.file_stem() {
                if stem.to_string_lossy().to_lowercase() == target {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
pub use crate::desktop::{
    accessibility_trusted, cursor_pos, desktop_dirs, init_thread, new_icons,
    open_accessibility_settings, suppress_desktop_infotips, virtual_screen_width,
};
#[cfg(target_os = "macos")]
pub use crate::desktop_mac::{
    accessibility_trusted, cursor_pos, desktop_dirs, init_thread, new_icons,
    open_accessibility_settings, suppress_desktop_infotips, virtual_screen_width,
};

/// `None` where the platform needs no such grant (Windows); `Some(false)`
/// means hover and hotkey targeting cannot work until the user grants it.
#[tauri::command]
pub fn accessibility_status() -> Option<bool> {
    accessibility_trusted()
}

/// Open the OS pane where the user grants the permission.
#[tauri::command]
pub fn open_accessibility_pane() {
    open_accessibility_settings();
}
