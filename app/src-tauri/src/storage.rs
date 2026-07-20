//! Sidecar nugget storage (source of truth, see docs/ARCHITECTURE.md §4).
//!
//! A file's nugget lives at `<parent>\.nuggets\<filename>.nugget.json`;
//! a folder's own nugget lives inside it at `<folder>\.nuggets\_self.nugget.json`
//! so it travels when the folder is copied or synced.
//!
//! Unwritable parents (Public Desktop needs elevation — docs/V0.1.1.md A4):
//! the sidecar is *redirected* into the user's own desktop `.nuggets` as
//! `<name>.<pathhash>.nugget.json` with the annotated item's absolute path
//! stored in the `target` field. Reads check the primary location first,
//! then the redirect.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

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
    /// Absolute path of the annotated item — present only in redirected
    /// sidecars, where the filename alone can't identify the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Root whose `.nuggets` dir hosts redirected sidecars (the user's desktop).
/// Set once at startup; tests point it at a tempdir.
static REDIRECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_redirect_root(root: Option<PathBuf>) {
    if let Ok(mut r) = REDIRECT_ROOT.write() {
        *r = root;
    }
}

/// Redirected sidecar location for an item whose own parent is unwritable.
/// The path hash keeps same-named items from different folders apart.
pub fn redirect_sidecar_path(item: &Path) -> Option<PathBuf> {
    let root = REDIRECT_ROOT.read().ok()?.clone()?;
    // Item must not live under the redirect root itself (its sidecars are
    // primary there) — avoids a same-name collision with a primary sidecar.
    if item.parent() == Some(root.as_path()) {
        return None;
    }
    let name = item.file_name()?.to_string_lossy();
    let hash = fnv1a64(item.to_string_lossy().to_lowercase().as_bytes());
    Some(
        root.join(SIDECAR_DIR)
            .join(format!("{name}.{hash:08x}.nugget.json")),
    )
}

/// Stable filename hash — std's DefaultHasher is not guaranteed stable
/// across Rust releases, and these names must never change once written.
fn fnv1a64(bytes: &[u8]) -> u32 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Fold to 32 bits for a short, stable suffix.
    (h ^ (h >> 32)) as u32
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

/// Parse a sidecar file directly (index scans need this for redirected
/// sidecars, whose filename doesn't name their target).
pub fn read_sidecar_file(sc: &Path) -> Option<Nugget> {
    let data = std::fs::read_to_string(sc).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn read_nugget(item: &Path) -> Option<Nugget> {
    if let Some(n) = sidecar_path(item).and_then(|sc| read_sidecar_file(&sc)) {
        return Some(n);
    }
    redirect_sidecar_path(item).and_then(|sc| read_sidecar_file(&sc))
}

pub fn has_nugget(item: &Path) -> bool {
    sidecar_path(item).map(|p| p.is_file()).unwrap_or(false)
        || redirect_sidecar_path(item)
            .map(|p| p.is_file())
            .unwrap_or(false)
}

#[allow(dead_code)] // used by the editor from Milestone 3; handy for seeding test data now
pub fn write_nugget(item: &Path, nugget: &Nugget) -> std::io::Result<()> {
    let sc = sidecar_path(item)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no sidecar path"))?;
    let dir = sc.parent().unwrap();
    let primary = std::fs::create_dir_all(dir).and_then(|()| {
        hide_dir(dir);
        std::fs::write(&sc, serde_json::to_string_pretty(nugget)?)
    });
    match primary {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Unwritable parent (e.g. Public Desktop): redirect the sidecar
            // under the user's own desktop, remembering the target.
            let rsc = redirect_sidecar_path(item).ok_or(e)?;
            let rdir = rsc.parent().unwrap();
            std::fs::create_dir_all(rdir)?;
            hide_dir(rdir);
            let mut n = nugget.clone();
            n.target = Some(item.to_string_lossy().into_owned());
            std::fs::write(&rsc, serde_json::to_string_pretty(&n)?)
        }
        other => other,
    }
}

/// Delete an item's nugget: remove the sidecar (primary and redirect) and,
/// when that leaves a `.nuggets` dir empty, the dir itself. Missing sidecar
/// is not an error.
pub fn delete_nugget(item: &Path) -> std::io::Result<()> {
    let mut result = Ok(());
    for sc in [sidecar_path(item), redirect_sidecar_path(item)]
        .into_iter()
        .flatten()
    {
        match std::fs::remove_file(&sc) {
            Ok(()) => {
                if let Some(dir) = sc.parent() {
                    if std::fs::read_dir(dir)
                        .map(|mut d| d.next().is_none())
                        .unwrap_or(false)
                    {
                        let _ = std::fs::remove_dir(dir);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => result = Err(e),
        }
    }
    result
}

/// Remove every `*.nugget.json` from a `.nuggets` directory, then the dir
/// itself if that leaves it empty. Used by the danger-zone "delete all notes"
/// to sweep strays (stale sidecars for items that no longer exist never enter
/// the index). Missing dir is a no-op.
pub fn purge_sidecar_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.file_name()
            .map(|f| f.to_string_lossy().ends_with(".nugget.json"))
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&p);
        }
    }
    if std::fs::read_dir(dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir(dir);
    }
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
    // Redirected sidecar: the filename hash and embedded target both encode
    // the item path, so rewrite rather than rename.
    if let Some(old_rsc) = redirect_sidecar_path(old_item).filter(|p| p.is_file()) {
        if let Some(mut n) = read_sidecar_file(&old_rsc) {
            n.target = Some(new_item.to_string_lossy().into_owned());
            if let Some(new_rsc) = redirect_sidecar_path(new_item) {
                std::fs::write(&new_rsc, serde_json::to_string_pretty(&n)?)?;
                let _ = std::fs::remove_file(&old_rsc);
                return Ok(());
            }
        }
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

    // Redirect tests mutate the process-global REDIRECT_ROOT; serialize them
    // so parallel runs don't clobber each other's root.
    static REDIRECT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn nugget(html: &str) -> Nugget {
        Nugget {
            schema: SCHEMA_VERSION,
            html: html.into(),
            created_ms: 1,
            modified_ms: 1,
            target: None,
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
    fn purge_sidecar_dir_removes_nuggets_and_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("doc.txt");
        std::fs::write(&file, b"x").unwrap();
        write_nugget(&file, &nugget("<p>hi</p>")).unwrap();
        let sc_dir = tmp.path().join(SIDECAR_DIR);
        assert!(sc_dir.exists());

        purge_sidecar_dir(&sc_dir);
        // Sole sidecar removed -> dir gone.
        assert!(!sc_dir.exists());
        // Missing dir is a no-op.
        purge_sidecar_dir(&sc_dir);
    }

    #[test]
    fn purge_sidecar_dir_keeps_dir_with_foreign_files() {
        let tmp = tempfile::tempdir().unwrap();
        let sc_dir = tmp.path().join(SIDECAR_DIR);
        std::fs::create_dir_all(&sc_dir).unwrap();
        std::fs::write(sc_dir.join("a.nugget.json"), b"{}").unwrap();
        // A non-nugget file must survive, so the dir stays.
        std::fs::write(sc_dir.join("keep.txt"), b"x").unwrap();

        purge_sidecar_dir(&sc_dir);
        assert!(!sc_dir.join("a.nugget.json").exists());
        assert!(sc_dir.join("keep.txt").exists());
        assert!(sc_dir.exists());
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

    #[test]
    fn redirect_path_shape_and_root_exclusion() {
        let _guard = REDIRECT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        set_redirect_root(Some(root.clone()));

        // Item under an unwritable elsewhere-dir gets a hashed name in the
        // redirect root's .nuggets.
        let item = Path::new("C:/Users/Public/Desktop/Logitech G HUB.lnk");
        let rsc = redirect_sidecar_path(item).unwrap();
        assert_eq!(rsc.parent().unwrap(), root.join(SIDECAR_DIR));
        let fname = rsc.file_name().unwrap().to_string_lossy();
        assert!(fname.starts_with("Logitech G HUB.lnk."));
        assert!(fname.ends_with(".nugget.json"));

        // An item that already lives directly under the redirect root has a
        // normal primary sidecar there — no redirect (would collide).
        let native = root.join("thing.txt");
        assert!(redirect_sidecar_path(&native).is_none());

        set_redirect_root(None);
    }

    #[test]
    fn redirect_roundtrip_read_has_delete() {
        let _guard = REDIRECT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        set_redirect_root(Some(root.clone()));

        // Simulate a redirected save: the item's real parent is unwritable,
        // so the sidecar lands under the redirect root with `target` set.
        let item = Path::new("C:/Users/Public/Desktop/Logitech G HUB.lnk");
        let rsc = redirect_sidecar_path(item).unwrap();
        std::fs::create_dir_all(rsc.parent().unwrap()).unwrap();
        let mut n = nugget("<p>gaming</p>");
        n.target = Some(item.to_string_lossy().into_owned());
        std::fs::write(&rsc, serde_json::to_string_pretty(&n).unwrap()).unwrap();

        // Reads resolve via the redirect, keyed by item path.
        assert!(has_nugget(item));
        assert_eq!(read_nugget(item).unwrap().html, "<p>gaming</p>");

        // Delete clears the redirected sidecar too.
        delete_nugget(item).unwrap();
        assert!(!has_nugget(item));
        assert!(!rsc.is_file());

        set_redirect_root(None);
    }
}
