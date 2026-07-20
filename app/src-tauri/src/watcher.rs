//! Filesystem watcher keeping sidecars + index in step with renames and
//! deletions on the watched roots (desktop dirs for MVP).
//!
//! Windows delivers same-directory renames as a RenameMode::Both event with
//! [from, to] paths; cross-directory moves arrive as remove + create. A move
//! out of watched scope therefore leaves the sidecar behind (stale, harmless,
//! skipped by index rebuilds) — accepted for MVP per ARCHITECTURE.md §4.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::index::NuggetIndex;
use crate::storage;

pub fn spawn(roots: Vec<PathBuf>, index: Arc<Mutex<NuggetIndex>>) {
    std::thread::Builder::new()
        .name("fs-watcher".into())
        .spawn(move || {
            let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
            let mut watcher = match notify::recommended_watcher(tx) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("watcher init failed: {e}");
                    return;
                }
            };
            for root in &roots {
                if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
                    eprintln!("watch {} failed: {e}", root.display());
                }
            }
            // Keep `watcher` alive for the thread's lifetime.
            loop {
                match rx.recv_timeout(Duration::from_secs(3600)) {
                    Ok(Ok(event)) => handle_event(&event, &index),
                    Ok(Err(e)) => eprintln!("watch error: {e}"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("spawn fs watcher");
}

fn handle_event(event: &Event, index: &Arc<Mutex<NuggetIndex>>) {
    match &event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            let (old, new) = (&event.paths[0], &event.paths[1]);
            if is_sidecar_path(old) || is_sidecar_path(new) {
                return;
            }
            let _ = storage::rename_sidecar(old, new);
            if let Ok(idx) = index.lock() {
                idx.rename_item(old, new);
            }
        }
        EventKind::Remove(_) => {
            for p in &event.paths {
                if is_sidecar_path(p) {
                    continue;
                }
                // Sidecar intentionally left on disk (file may come back /
                // was moved elsewhere); only the index entry goes.
                if let Ok(idx) = index.lock() {
                    idx.remove_item(p);
                }
            }
        }
        // Sidecar created/edited (e.g. via editor or sync) -> refresh entry.
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) => {
            for p in &event.paths {
                if let Some(item) = sidecar_to_item(p) {
                    if let Ok(idx) = index.lock() {
                        if item.exists() {
                            idx.upsert_item(&item);
                        } else {
                            idx.remove_item(&item);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn is_sidecar_path(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str() == storage::SIDECAR_DIR)
}

/// Map a sidecar file path back to the item it annotates.
/// `<dir>/.nuggets/_self.nugget.json` -> `<dir>` ;
/// `<parent>/.nuggets/<name>.nugget.json` -> `<parent>/<name>`.
/// A redirected sidecar (unwritable parent) names an item elsewhere; its
/// `target` field is authoritative, so prefer that when the file is readable.
fn sidecar_to_item(p: &Path) -> Option<PathBuf> {
    let fname = p.file_name()?.to_string_lossy();
    let item_name = fname.strip_suffix(".nugget.json")?.to_string();
    let sc_dir = p.parent()?;
    if sc_dir.file_name()? != storage::SIDECAR_DIR {
        return None;
    }
    if let Some(t) = storage::read_sidecar_file(p).and_then(|n| n.target) {
        return Some(PathBuf::from(t));
    }
    let owner = sc_dir.parent()?;
    if item_name == "_self" {
        Some(owner.to_path_buf())
    } else {
        Some(owner.join(item_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, RemoveKind};

    fn index() -> Arc<Mutex<NuggetIndex>> {
        Arc::new(Mutex::new(NuggetIndex::open_in_memory().unwrap()))
    }

    fn nugget(html: &str) -> storage::Nugget {
        storage::Nugget {
            schema: 1,
            html: html.into(),
            created_ms: 1,
            modified_ms: 1,
            target: None,
        }
    }

    #[test]
    fn rename_event_moves_sidecar_and_updates_index() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("todo.txt");
        std::fs::write(&old, b"x").unwrap();
        storage::write_nugget(&old, &nugget("<p>list</p>")).unwrap();

        let idx = index();
        idx.lock().unwrap().upsert_item(&old);

        let new = tmp.path().join("done.txt");
        std::fs::rename(&old, &new).unwrap();

        let ev = Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            paths: vec![old.clone(), new.clone()],
            attrs: Default::default(),
        };
        handle_event(&ev, &idx);

        assert!(
            storage::has_nugget(&new),
            "sidecar should follow the rename"
        );
        assert!(!storage::has_nugget(&old));
        let all = idx.lock().unwrap().all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "done.txt");
    }

    #[test]
    fn remove_event_drops_index_entry_but_keeps_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        storage::write_nugget(&file, &nugget("note")).unwrap();

        let idx = index();
        idx.lock().unwrap().upsert_item(&file);
        std::fs::remove_file(&file).unwrap();

        let ev = Event {
            kind: EventKind::Remove(RemoveKind::File),
            paths: vec![file.clone()],
            attrs: Default::default(),
        };
        handle_event(&ev, &idx);

        assert!(idx.lock().unwrap().all().unwrap().is_empty());
        // Sidecar file stays on disk (stale but harmless).
        assert!(storage::sidecar_path(&file)
            .map(|p| p.is_file())
            .unwrap_or(false));
    }

    #[test]
    fn sidecar_create_event_upserts_owning_item() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("b.txt");
        std::fs::write(&file, b"x").unwrap();
        storage::write_nugget(&file, &nugget("<b>fresh</b>")).unwrap();
        let sc = storage::sidecar_path(&file).unwrap();

        let idx = index();
        let ev = Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![sc],
            attrs: Default::default(),
        };
        handle_event(&ev, &idx);

        let all = idx.lock().unwrap().all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].preview, "fresh");
    }

    #[test]
    fn sidecar_mapping() {
        let p = Path::new("C:/d/.nuggets/file.txt.nugget.json");
        assert_eq!(sidecar_to_item(p).unwrap(), Path::new("C:/d/file.txt"));
        let s = Path::new("C:/d/Folder/.nuggets/_self.nugget.json");
        assert_eq!(sidecar_to_item(s).unwrap(), Path::new("C:/d/Folder"));
        assert!(sidecar_to_item(Path::new("C:/d/other/file.json")).is_none());
    }
}
