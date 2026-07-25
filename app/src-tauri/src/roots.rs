//! Known roots: folders outside the desktop where a note has been created.
//!
//! The index rebuild at startup scans a fixed list of roots. Before E1 that
//! list was the desktop dirs alone, which was complete because notes could
//! only exist on desktop icons. Explorer note-creation (E1) breaks that: a note
//! written to `D:\work\report.pdf` reaches the main-window list immediately
//! (the editor upserts it), then vanishes on the next restart because nothing
//! rescans `D:\work`.
//!
//! Decision (E2): record the *parent folder* of every annotated item at save
//! time and persist that list as `known_roots.json` in the app-data dir; the
//! startup rebuild scans desktop dirs + known roots. The filesystem watcher
//! stays desktop-only — watching arbitrary user folders is what the perf budget
//! rules out, and the in-session upsert already keeps an open list current.
//! Rationale and the rejected alternatives live in docs/ARCHITECTURE.md §4.
//!
//! The list is a rebuildable cache in the same sense as the SQLite index:
//! losing it costs visibility of old off-desktop notes in the list, never the
//! notes themselves (the sidecars are the source of truth, and hover/badges
//! read them by path regardless).

use std::path::{Path, PathBuf};

const FILE: &str = "known_roots.json";

fn file_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = crate::paths::data_dir(app).ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(FILE))
}

/// Persisted roots that still exist on disk. Vanished folders (unplugged
/// drive, deleted project) are dropped silently — an unreachable root would
/// only slow every rebuild down.
pub fn load(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let stored: Vec<String> = file_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    stored
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

fn save(app: &tauri::AppHandle, roots: &[PathBuf]) {
    let list: Vec<String> = roots.iter().map(|p| p.to_string_lossy().into()).collect();
    if let (Some(p), Ok(json)) = (file_path(app), serde_json::to_string_pretty(&list)) {
        let _ = std::fs::write(p, json);
    }
}

/// Remember the folder a note was just written into. No-op when the folder is
/// already known or is one of the desktop roots (always scanned anyway).
pub fn record(app: &tauri::AppHandle, folder: &Path) {
    let desktop = crate::icons::desktop_dirs();
    if desktop.iter().any(|d| same_folder(d, folder)) {
        return;
    }
    let mut roots = load(app);
    if roots.iter().any(|r| same_folder(r, folder)) {
        return;
    }
    roots.push(folder.to_path_buf());
    save(app, &roots);
}

/// Path equality for the two comparisons above. Windows paths are
/// case-insensitive, so a note saved via `D:\Work` must not add a second root
/// next to `D:\work`.
fn same_folder(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_folder_ignores_case_on_windows() {
        let a = Path::new("D:/Work/Reports");
        let b = Path::new("d:/work/reports");
        assert_eq!(same_folder(a, b), cfg!(windows));
        assert!(!same_folder(a, Path::new("D:/Work/Other")));
    }
}
