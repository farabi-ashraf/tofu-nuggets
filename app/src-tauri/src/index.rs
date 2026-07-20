//! SQLite cache index over sidecar nuggets (docs/ARCHITECTURE.md §4).
//!
//! Sidecars are the source of truth; this index only accelerates the
//! "show all nuggets" view and must always be rebuildable from disk.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::storage;

pub struct NuggetIndex {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub preview: String,
    pub modified_ms: u64,
}

impl NuggetIndex {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = db_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nuggets (
                path        TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                preview     TEXT NOT NULL,
                modified_ms INTEGER NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nuggets (
                path        TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                preview     TEXT NOT NULL,
                modified_ms INTEGER NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    /// Drop everything and re-scan the given roots for sidecars.
    pub fn rebuild(&mut self, roots: &[PathBuf]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM nuggets", [])?;
        for root in roots {
            for (item, nugget) in scan_root(root) {
                upsert_tx(&tx, &item, &nugget)?;
            }
        }
        tx.commit()
    }

    pub fn upsert_item(&self, item: &Path) {
        if let Some(n) = storage::read_nugget(item) {
            let _ = upsert_tx(&self.conn, item, &n);
        }
    }

    pub fn remove_item(&self, item: &Path) {
        let _ = self.conn.execute(
            "DELETE FROM nuggets WHERE path = ?1",
            params![item.to_string_lossy()],
        );
    }

    pub fn rename_item(&self, old: &Path, new: &Path) {
        let _ = self.conn.execute(
            "UPDATE nuggets SET path = ?1, name = ?2 WHERE path = ?3",
            params![
                new.to_string_lossy(),
                new.file_name().unwrap_or_default().to_string_lossy(),
                old.to_string_lossy()
            ],
        );
    }

    pub fn all(&self) -> rusqlite::Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, name, preview, modified_ms FROM nuggets ORDER BY modified_ms DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Entry {
                path: r.get(0)?,
                name: r.get(1)?,
                preview: r.get(2)?,
                modified_ms: r.get::<_, i64>(3)? as u64,
            })
        })?;
        rows.collect()
    }
}

fn upsert_tx(conn: &Connection, item: &Path, nugget: &storage::Nugget) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO nuggets (path, name, preview, modified_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET name=?2, preview=?3, modified_ms=?4",
        params![
            item.to_string_lossy(),
            item.file_name().unwrap_or_default().to_string_lossy(),
            storage::preview_text(&nugget.html),
            nugget.modified_ms as i64
        ],
    )?;
    Ok(())
}

/// All (item, nugget) pairs under one root: file sidecars in `root/.nuggets/`
/// plus each direct child folder's own `_self` nugget.
fn scan_root(root: &Path) -> Vec<(PathBuf, storage::Nugget)> {
    let mut out = Vec::new();

    // File nuggets: root/.nuggets/<filename>.nugget.json -> root/<filename>.
    // Redirected sidecars (unwritable parents, e.g. Public Desktop) carry a
    // `target` field and a `<name>.<hash>.nugget.json` filename; they point
    // at an item outside this root, so trust the embedded target instead.
    let sc_dir = root.join(storage::SIDECAR_DIR);
    if let Ok(entries) = std::fs::read_dir(&sc_dir) {
        for e in entries.flatten() {
            let sc = e.path();
            let Some(fname) = sc.file_name().map(|f| f.to_string_lossy().to_string()) else {
                continue;
            };
            let Some(item_name) = fname.strip_suffix(".nugget.json") else {
                continue;
            };
            if item_name == "_self" {
                continue;
            }
            let Some(n) = storage::read_sidecar_file(&sc) else {
                continue;
            };
            let item = match &n.target {
                Some(t) => PathBuf::from(t),
                None => root.join(item_name),
            };
            if !item.exists() {
                continue; // stale sidecar; keep file, skip index
            }
            out.push((item, n));
        }
    }

    // Folder nuggets: root/<dir>/.nuggets/_self.nugget.json -> root/<dir>
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir()
                || dir
                    .file_name()
                    .map(|f| f == storage::SIDECAR_DIR)
                    .unwrap_or(true)
            {
                continue;
            }
            if let Some(n) = storage::read_nugget(&dir) {
                out.push((dir, n));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{write_nugget, Nugget};

    fn nugget(html: &str, t: u64) -> Nugget {
        Nugget {
            schema: 1,
            html: html.into(),
            created_ms: t,
            modified_ms: t,
            target: None,
        }
    }

    #[test]
    fn rebuild_finds_file_and_folder_nuggets() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let file = root.join("report.pdf");
        std::fs::write(&file, b"x").unwrap();
        write_nugget(&file, &nugget("<p>quarterly report</p>", 100)).unwrap();

        let folder = root.join("Projects");
        std::fs::create_dir(&folder).unwrap();
        write_nugget(&folder, &nugget("<b>client work</b>", 200)).unwrap();

        // Stale sidecar for a deleted file must be skipped.
        let ghost = root.join("gone.txt");
        std::fs::write(&ghost, b"x").unwrap();
        write_nugget(&ghost, &nugget("stale", 50)).unwrap();
        std::fs::remove_file(&ghost).unwrap();

        let mut idx = NuggetIndex::open_in_memory().unwrap();
        idx.rebuild(std::slice::from_ref(&root)).unwrap();

        let all = idx.all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "Projects"); // newest first
        assert_eq!(all[0].preview, "client work");
        assert_eq!(all[1].name, "report.pdf");
        assert_eq!(all[1].preview, "quarterly report");
    }

    #[test]
    fn rebuild_resolves_redirected_sidecar_via_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        // A real item elsewhere on disk (stand-in for Public Desktop).
        let elsewhere = tmp.path().join("public");
        std::fs::create_dir(&elsewhere).unwrap();
        let item = elsewhere.join("Logitech G HUB.lnk");
        std::fs::write(&item, b"x").unwrap();

        // A redirected sidecar under the scanned root, hashed filename, with
        // the real target embedded.
        let sc_dir = root.join(storage::SIDECAR_DIR);
        std::fs::create_dir_all(&sc_dir).unwrap();
        let n = storage::Nugget {
            schema: 1,
            html: "<p>gaming</p>".into(),
            created_ms: 5,
            modified_ms: 5,
            target: Some(item.to_string_lossy().into_owned()),
        };
        std::fs::write(
            sc_dir.join("Logitech G HUB.lnk.deadbeef.nugget.json"),
            serde_json::to_string_pretty(&n).unwrap(),
        )
        .unwrap();

        let mut idx = NuggetIndex::open_in_memory().unwrap();
        idx.rebuild(std::slice::from_ref(&root)).unwrap();

        let all = idx.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Logitech G HUB.lnk");
        assert_eq!(all[0].preview, "gaming");
        assert!(all[0].path.ends_with("Logitech G HUB.lnk"));
    }

    #[test]
    fn upsert_remove_rename_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let file = root.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        write_nugget(&file, &nugget("<p>note</p>", 10)).unwrap();

        let idx = NuggetIndex::open_in_memory().unwrap();
        idx.upsert_item(&file);
        assert_eq!(idx.all().unwrap().len(), 1);

        let renamed = root.join("b.txt");
        idx.rename_item(&file, &renamed);
        let all = idx.all().unwrap();
        assert_eq!(all[0].name, "b.txt");
        assert!(all[0].path.ends_with("b.txt"));

        idx.remove_item(&renamed);
        assert!(idx.all().unwrap().is_empty());
    }
}
