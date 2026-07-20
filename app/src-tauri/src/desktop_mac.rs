//! macOS desktop-icon stub (Route 1 scaffolding).
//!
//! Lets the whole app compile and run on macOS with hover and badges inert:
//! `icon_at`/`list_icons`/`selected_icon` return nothing, `cursor_pos` returns
//! `None` so the hover engine idles. The real AX-API (Accessibility)
//! implementation replaces this in a later Route 1 PR. `desktop_dirs` already
//! returns the real `~/Desktop`, so storage, index, watcher, editor and
//! main-window flows are fully functional.

use std::path::PathBuf;

use crate::icons::{DesktopIcons, Icon};

pub struct MacIcons;

impl DesktopIcons for MacIcons {
    fn icon_at(&self, _x: i32, _y: i32) -> Option<Icon> {
        None
    }

    fn list_icons(&self) -> Result<Vec<Icon>, String> {
        Ok(Vec::new())
    }

    fn selected_icon(&self) -> Option<Icon> {
        None
    }
}

pub fn new_icons() -> Result<MacIcons, String> {
    Ok(MacIcons)
}

/// `None` until the AX implementation lands: keeps the hover engine looping
/// without ever hit-testing.
pub fn cursor_pos() -> Option<(i32, i32)> {
    None
}

/// Only consumed by panel placement, which needs a real cursor position
/// first; `MAX` means "never flip left" until then.
pub fn virtual_screen_width() -> i32 {
    i32::MAX
}

pub fn desktop_dirs() -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Desktop"))
        .into_iter()
        .collect()
}

/// Finder has no equivalent of the desktop ListView infotip; nothing to do.
pub fn suppress_desktop_infotips() -> bool {
    false
}

/// No per-thread runtime setup needed on macOS (COM is Windows-only).
pub fn init_thread() {}
