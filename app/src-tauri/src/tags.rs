//! macOS Finder-tag badges (D3): the note badge on macOS is a Finder tag named
//! `Nugget`, which Finder draws itself on the Desktop and inside every Finder
//! window, at every zoom, through every reshuffle. There is no overlay window
//! and no pill — the badge half of the file-manager update costs nothing to draw
//! on macOS. Sidecars stay the source of truth; the tag is a derived cue,
//! resynced from them at startup.
//!
//! The tag lives in the `com.apple.metadata:_kMDItemUserTags` xattr as a BINARY
//! plist array of `"Name\nColorCode"` strings (`0` none, `1` gray, `2` green,
//! `3` purple, `4` blue, `5` yellow, `6` red, `7` orange). **Identity is the
//! name** — `Nugget` — not the colour; the colour is user-selectable (M4b), read
//! here through one helper (`tag_color`) so M4b only swaps its source.
//!
//! Write hygiene is mandatory because these are the USER's own tags sitting next
//! to ours (MEMORY / docs/V0.4.0.md D3):
//! - read-modify-write immediately before writing;
//! - preserve every foreign entry verbatim, including order;
//! - **abort and write nothing** if the payload is not a plist array of strings
//!   (never clobber on confusion);
//! - no-op when the array is already correct (tags are xattrs and they SYNC via
//!   iCloud/Dropbox — a pointless write costs the user sync traffic);
//! - match ours by name `Nugget` (any colour), so a recolour replaces the old
//!   `"Nugget\n<code>"` instead of piling a second one up.
//!
//! The pure array transforms (`with_tag`/`without_tag`) and the plist
//! encode/decode are the safety net and are unit-tested on every platform; the
//! xattr syscalls are macOS-only.

/// Finder tag name. Distinctive on purpose: a *name* collision would mean
/// touching a tag the user owns, far worse than a colour clash (D3).
pub const TAG_NAME: &str = "Nugget";

/// Default badge colour code (orange). M4b sources this from the shared
/// `badge_color` setting; until then it is the one constant to change.
#[cfg(target_os = "macos")]
const DEFAULT_COLOR: u8 = 7;

/// The colour code our tag is written with. The single source M4b will swap.
#[cfg(target_os = "macos")]
pub fn tag_color() -> u8 {
    DEFAULT_COLOR
}

/// True when `entry` is one of ours — identity is the name, with or without a
/// colour suffix (`"Nugget"` or `"Nugget\n<code>"`).
fn is_ours(entry: &str) -> bool {
    entry == TAG_NAME
        || entry
            .strip_prefix(TAG_NAME)
            .is_some_and(|rest| rest.starts_with('\n'))
}

/// Entries after ensuring exactly one `Nugget\n<code>`, every foreign entry kept
/// verbatim and in order. `None` when the array is already correct (no write).
fn with_tag(entries: &[String], code: u8) -> Option<Vec<String>> {
    let want = format!("{TAG_NAME}\n{code}");
    let mut ours = entries.iter().filter(|e| is_ours(e));
    if ours.next().map(|e| e == &want).unwrap_or(false) && ours.next().is_none() {
        return None; // exactly one, already the right colour
    }
    let mut out: Vec<String> = entries.iter().filter(|e| !is_ours(e)).cloned().collect();
    out.push(want);
    Some(out)
}

/// Entries with every one of ours removed, foreign entries kept verbatim and in
/// order. `None` when none of ours are present (no write).
fn without_tag(entries: &[String]) -> Option<Vec<String>> {
    if !entries.iter().any(|e| is_ours(e)) {
        return None;
    }
    Some(entries.iter().filter(|e| !is_ours(e)).cloned().collect())
}

/// Parse the xattr payload into the tag strings. `None` means **abort**: the
/// value is not a plist array of strings, so it is not ours to touch.
fn parse_entries(raw: &[u8]) -> Option<Vec<String>> {
    let val = plist::Value::from_reader(std::io::Cursor::new(raw)).ok()?;
    let arr = val.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        out.push(v.as_string()?.to_string());
    }
    Some(out)
}

/// Encode tag strings as a binary plist array, the way Finder stores them.
fn serialize_entries(entries: &[String]) -> Vec<u8> {
    let arr = plist::Value::Array(entries.iter().cloned().map(plist::Value::String).collect());
    let mut buf = Vec::new();
    let _ = arr.to_writer_binary(&mut buf);
    buf
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    use tauri::AppHandle;

    use super::{parse_entries, serialize_entries, tag_color, with_tag, without_tag};
    use crate::logfile;

    const XATTR: &str = "com.apple.metadata:_kMDItemUserTags";

    /// Current tags, or `Err(())` = abort: the attr could not be read or held
    /// something that is not a plist array of strings. A missing attr is not an
    /// error — it is an empty tag set.
    fn read_current(path: &Path) -> Result<Vec<String>, ()> {
        match xattr::get(path, XATTR) {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(raw)) => parse_entries(&raw).ok_or(()),
            Err(_) => Err(()),
        }
    }

    /// Write the tag set, removing the attr entirely when it is empty (an empty
    /// `_kMDItemUserTags` is not how Finder represents "no tags").
    fn write_current(path: &Path, entries: &[String]) -> std::io::Result<()> {
        if entries.is_empty() {
            match xattr::remove(path, XATTR) {
                Ok(()) => Ok(()),
                // Nothing to remove is success as far as we care.
                Err(e) if e.raw_os_error() == Some(libc::ENOATTR) => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            xattr::set(path, XATTR, &serialize_entries(entries))
        }
    }

    /// Read-modify-write with the mandated hygiene. `f` returns the new array or
    /// `None` for "already correct". Any read/parse failure aborts the write.
    fn update(app: &AppHandle, path: &Path, f: impl FnOnce(&[String]) -> Option<Vec<String>>) {
        let cur = match read_current(path) {
            Ok(c) => c,
            Err(()) => {
                logfile::log(
                    app,
                    &format!(
                        "tags: abort (unreadable or non-tag xattr) {}",
                        path.display()
                    ),
                );
                return;
            }
        };
        let Some(next) = f(&cur) else {
            return; // already correct — no sync-costing write
        };
        if let Err(e) = write_current(path, &next) {
            logfile::log(app, &format!("tags: write failed {}: {e}", path.display()));
        }
    }

    /// Add/refresh our tag on `path` (note saved).
    pub fn set_note_tag(app: &AppHandle, path: &Path) {
        update(app, path, |cur| with_tag(cur, tag_color()));
    }

    /// Remove our tag from `path` (note deleted, or emptied).
    pub fn clear_note_tag(app: &AppHandle, path: &Path) {
        update(app, path, without_tag);
    }

    /// Bulk resync: tag or untag every annotated item. Used at startup (from the
    /// index, the source-of-truth resync) and when the badges setting toggles.
    /// Runs on a worker thread — it is one xattr syscall per item.
    pub fn resync(app: &AppHandle, paths: Vec<PathBuf>, want_tagged: bool) {
        for p in &paths {
            if want_tagged {
                set_note_tag(app, p);
            } else {
                clear_note_tag(app, p);
            }
        }
        logfile::log(
            app,
            &format!(
                "tags: resync {} item(s) -> {}",
                paths.len(),
                if want_tagged { "tagged" } else { "untagged" }
            ),
        );
    }
}

#[cfg(target_os = "macos")]
pub use imp::{clear_note_tag, resync, set_note_tag};

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_to_empty_writes_our_tag() {
        assert_eq!(with_tag(&[], 7), Some(v(&["Nugget\n7"])));
    }

    #[test]
    fn add_preserves_foreign_entries_and_order() {
        let cur = v(&["Red\n6", "Work\n4"]);
        assert_eq!(
            with_tag(&cur, 7),
            Some(v(&["Red\n6", "Work\n4", "Nugget\n7"]))
        );
    }

    #[test]
    fn add_is_noop_when_already_correct() {
        assert_eq!(with_tag(&v(&["Work\n4", "Nugget\n7"]), 7), None);
    }

    #[test]
    fn recolour_removes_the_stale_entry_first() {
        // A different colour, plus a foreign tag that must survive untouched.
        let cur = v(&["Nugget\n5", "Work\n4"]);
        assert_eq!(with_tag(&cur, 7), Some(v(&["Work\n4", "Nugget\n7"])));
    }

    #[test]
    fn bare_name_without_colour_is_ours() {
        // A "Nugget" with no colour code still counts as ours by name.
        assert_eq!(with_tag(&v(&["Nugget"]), 7), Some(v(&["Nugget\n7"])));
        assert_eq!(without_tag(&v(&["Nugget"])), Some(v(&[])));
    }

    #[test]
    fn remove_drops_ours_keeps_foreign() {
        let cur = v(&["Work\n4", "Nugget\n7", "Ideas\n2"]);
        assert_eq!(without_tag(&cur), Some(v(&["Work\n4", "Ideas\n2"])));
    }

    #[test]
    fn remove_is_noop_when_not_present() {
        assert_eq!(without_tag(&v(&["Work\n4"])), None);
    }

    #[test]
    fn a_similarly_named_tag_is_not_ours() {
        // Guard the name match: "Nuggets" / "Nugget Ideas" belong to the user.
        assert!(!is_ours("Nuggets\n7"));
        assert!(!is_ours("Nugget Ideas\n7"));
        assert!(is_ours("Nugget\n7"));
        assert!(is_ours("Nugget"));
    }

    #[test]
    fn plist_binary_roundtrip() {
        let entries = v(&["Work\n4", "Nugget\n7"]);
        let raw = serialize_entries(&entries);
        assert_eq!(parse_entries(&raw), Some(entries));
    }

    #[test]
    fn parse_aborts_on_non_array_plist() {
        // A dict, or a scalar, is "unexpected shape" -> abort (None).
        let dict = plist::Value::Dictionary(plist::Dictionary::new());
        let mut raw = Vec::new();
        dict.to_writer_binary(&mut raw).unwrap();
        assert_eq!(parse_entries(&raw), None);
    }

    #[test]
    fn parse_aborts_on_non_string_element() {
        let arr = plist::Value::Array(vec![
            plist::Value::String("Work\n4".into()),
            plist::Value::Integer(3.into()),
        ]);
        let mut raw = Vec::new();
        arr.to_writer_binary(&mut raw).unwrap();
        assert_eq!(parse_entries(&raw), None);
    }

    #[test]
    fn parse_aborts_on_garbage_bytes() {
        assert_eq!(parse_entries(b"not a plist at all"), None);
    }
}
