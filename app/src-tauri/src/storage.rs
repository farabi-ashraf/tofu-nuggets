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
#[allow(dead_code)] // stamped by the editor from Milestone 3
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

/// Delete an item's nugget: remove the sidecar and, when that leaves the
/// `.nuggets` dir empty, the dir itself. Missing sidecar is not an error.
pub fn delete_nugget(item: &Path) -> std::io::Result<()> {
    let Some(sc) = sidecar_path(item) else {
        return Ok(());
    };
    match std::fs::remove_file(&sc) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    if let Some(dir) = sc.parent() {
        if std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Ok(())
}

/// An "empty" note (no visible text once tags are stripped) counts as
/// deleted: saving one removes the nugget instead of storing markup husks.
pub fn is_empty_html(html: &str) -> bool {
    preview_text(html).trim().is_empty()
}

/// Plain-text preview of a nugget's HTML for list views: tags stripped,
/// whitespace collapsed, clipped to 120 chars.
pub fn preview_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(120).collect()
}

/// Keep the sidecar in step when its item is renamed (same parent) or moved.
/// Folder nuggets travel inside the folder, so only files need this.
pub fn rename_sidecar(old_item: &Path, new_item: &Path) -> std::io::Result<()> {
    if new_item.is_dir() {
        return Ok(());
    }
    let (Some(old_sc), Some(new_sc)) = (sidecar_path_for_file(old_item), sidecar_path(new_item))
    else {
        return Ok(());
    };
    if !old_sc.is_file() {
        return Ok(());
    }
    if let Some(dir) = new_sc.parent() {
        std::fs::create_dir_all(dir)?;
        hide_dir(dir);
    }
    std::fs::rename(old_sc, new_sc)
}

/// Sidecar location assuming the item is (was) a file — used when the old
/// path no longer exists so `is_dir()` can't be asked.
fn sidecar_path_for_file(item: &Path) -> Option<PathBuf> {
    let parent = item.parent()?;
    let name = item.file_name()?.to_string_lossy();
    Some(parent.join(SIDECAR_DIR).join(format!("{name}.nugget.json")))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn nugget(html: &str) -> Nugget {
        Nugget {
            schema: SCHEMA_VERSION,
            html: html.into(),
            created_ms: 1,
            modified_ms: 1,
        }
    }

    #[test]
    fn write_read_roundtrip_file_and_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("doc.txt");
        std::fs::write(&file, b"x").unwrap();
        write_nugget(&file, &nugget("<p>hi</p>")).unwrap();
        assert!(has_nugget(&file));
        assert_eq!(read_nugget(&file).unwrap().html, "<p>hi</p>");

        let folder = tmp.path().join("stuff");
        std::fs::create_dir(&folder).unwrap();
        write_nugget(&folder, &nugget("folder note")).unwrap();
        // Folder sidecar lives inside the folder -> travels with it.
        assert!(folder.join(SIDECAR_DIR).join("_self.nugget.json").is_file());
        assert_eq!(read_nugget(&folder).unwrap().html, "folder note");
    }

    #[test]
    fn rename_moves_file_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old.txt");
        std::fs::write(&old, b"x").unwrap();
        write_nugget(&old, &nugget("keep me")).unwrap();

        let new = tmp.path().join("new.txt");
        std::fs::rename(&old, &new).unwrap();
        rename_sidecar(&old, &new).unwrap();

        assert!(!has_nugget(&old));
        assert_eq!(read_nugget(&new).unwrap().html, "keep me");
    }

    #[test]
    fn rename_into_other_dir_moves_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let old = tmp.path().join("a.txt");
        std::fs::write(&old, b"x").unwrap();
        write_nugget(&old, &nugget("travels")).unwrap();

        let new = sub.join("a.txt");
        std::fs::rename(&old, &new).unwrap();
        rename_sidecar(&old, &new).unwrap();
        assert_eq!(read_nugget(&new).unwrap().html, "travels");
    }

    #[test]
    fn delete_removes_sidecar_and_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("doc.txt");
        std::fs::write(&file, b"x").unwrap();
        write_nugget(&file, &nugget("<p>hi</p>")).unwrap();
        assert!(has_nugget(&file));

        delete_nugget(&file).unwrap();
        assert!(!has_nugget(&file));
        // Sole sidecar gone -> .nuggets dir cleaned up too.
        assert!(!tmp.path().join(SIDECAR_DIR).exists());
        // Deleting again is a no-op, not an error.
        delete_nugget(&file).unwrap();
    }

    #[test]
    fn delete_keeps_dir_with_other_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        for f in [&a, &b] {
            std::fs::write(f, b"x").unwrap();
            write_nugget(f, &nugget("<p>hi</p>")).unwrap();
        }
        delete_nugget(&a).unwrap();
        assert!(!has_nugget(&a));
        assert!(has_nugget(&b));
        assert!(tmp.path().join(SIDECAR_DIR).exists());
    }

    #[test]
    fn empty_html_detection() {
        assert!(is_empty_html(""));
        assert!(is_empty_html("<p></p>"));
        assert!(is_empty_html("<p> </p><ul><li></li></ul>"));
        assert!(!is_empty_html("<p>note</p>"));
        assert!(!is_empty_html(
            "<ul data-type=\"taskList\"><li><input type=\"checkbox\"><div>todo</div></li></ul>"
        ));
    }

    #[test]
    fn preview_strips_tags_and_clips() {
        assert_eq!(
            preview_text("<p><b>Hello</b> world</p><ul><li>item</li></ul>"),
            "Hello world item"
        );
        let long = format!("<p>{}</p>", "x".repeat(300));
        assert_eq!(preview_text(&long).chars().count(), 120);
    }
}
