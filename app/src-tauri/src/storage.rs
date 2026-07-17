//! Sidecar nugget storage (source of truth, see docs/ARCHITECTURE.md §4).
//!
//! A file's nugget lives at `<parent>\.nuggets\<filename>.nugget.json`;
//! a folder's own nugget lives inside it at `<folder>\.nuggets\_self.nugget.json`
//! so it travels when the folder is copied or synced.
//!
//! Milestone 1 only reads nuggets (the editor arrives in Milestone 3); test
//! sidecars can be written by hand or with `write_nugget`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SIDECAR_DIR: &str = ".nuggets";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Nugget {
    pub schema: u32,
    /// Sanitized HTML fragment rendered in the overlay panel.
    pub html: String,
    pub created_ms: u64,
    pub modified_ms: u64,
}

/// Sidecar location for an annotated file or folder.
pub fn sidecar_path(item: &Path) -> Option<PathBuf> {
    if item.is_dir() {
        Some(item.join(SIDECAR_DIR).join("_self.nugget.json"))
    } else {
        let parent = item.parent()?;
        let name = item.file_name()?.to_string_lossy();
        Some(parent.join(SIDECAR_DIR).join(format!("{name}.nugget.json")))
    }
}

pub fn read_nugget(item: &Path) -> Option<Nugget> {
    let sc = sidecar_path(item)?;
    let data = std::fs::read_to_string(sc).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn has_nugget(item: &Path) -> bool {
    sidecar_path(item).map(|p| p.is_file()).unwrap_or(false)
}

#[allow(dead_code)] // used by the editor from Milestone 3; handy for seeding test data now
pub fn write_nugget(item: &Path, nugget: &Nugget) -> std::io::Result<()> {
    let sc = sidecar_path(item)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no sidecar path"))?;
    let dir = sc.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    hide_dir(dir);
    std::fs::write(&sc, serde_json::to_string_pretty(nugget)?)
}

/// Best-effort FILE_ATTRIBUTE_HIDDEN on the .nuggets directory.
fn hide_dir(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES,
    };
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let attrs = GetFileAttributesW(PCWSTR(wide.as_ptr()));
        if attrs != u32::MAX {
            let _ = SetFileAttributesW(
                PCWSTR(wide.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(attrs | FILE_ATTRIBUTE_HIDDEN.0),
            );
        }
    }
}
