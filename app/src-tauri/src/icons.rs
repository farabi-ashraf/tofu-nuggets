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

#[cfg(windows)]
pub use crate::desktop::{
    cursor_pos, desktop_dirs, init_thread, new_icons, suppress_desktop_infotips,
    virtual_screen_width,
};
#[cfg(target_os = "macos")]
pub use crate::desktop_mac::{
    cursor_pos, desktop_dirs, init_thread, new_icons, suppress_desktop_infotips,
    virtual_screen_width,
};
