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
//! name** — `Nugget` — not the colour; the colour is the user's shared
//! `badge_color` setting, mapped to a code by `settings::badge_color_code` and
//! read here through `tag_color`. Changing the setting resyncs every tag
//! (settings.rs), and the read-modify-write below drops the stale
//! `Nugget\n<old>` before writing `Nugget\n<new>`.
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

    use tauri::Manager;

    use super::{parse_entries, serialize_entries, with_tag, without_tag};
    use crate::logfile;

    const XATTR: &str = "com.apple.metadata:_kMDItemUserTags";

    /// The colour code the tag is written with, from the shared `badge_color`
    /// setting. Defaults to orange when the state is unreadable.
    fn tag_color(app: &AppHandle) -> u8 {
        let name = app
            .try_state::<crate::settings::Shared>()
            .and_then(|s| s.lock().ok().map(|g| g.badge_color.clone()))
            .unwrap_or_else(|| crate::settings::DEFAULT_BADGE_COLOR.to_string());
        crate::settings::badge_color_code(&name)
    }

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
    /// Returns `true` only when it actually wrote the xattr.
    fn update(
        app: &AppHandle,
        path: &Path,
        f: impl FnOnce(&[String]) -> Option<Vec<String>>,
    ) -> bool {
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
                return false;
            }
        };
        let Some(next) = f(&cur) else {
            return false; // already correct — no sync-costing write
        };
        match write_current(path, &next) {
            Ok(()) => true,
            Err(e) => {
                logfile::log(app, &format!("tags: write failed {}: {e}", path.display()));
                false
            }
        }
    }

    /// Add/refresh our tag on `path` (note saved). Shows the one-time first-tag
    /// notice the first time a `Nugget` tag is actually written on this profile.
    pub fn set_note_tag(app: &AppHandle, path: &Path) {
        let code = tag_color(app);
        if update(app, path, |cur| with_tag(cur, code)) {
            maybe_show_first_tag_notice(app);
        }
    }

    /// Remove our tag from `path` (note deleted, or emptied).
    pub fn clear_note_tag(app: &AppHandle, path: &Path) {
        let _ = update(app, path, without_tag);
    }

    /// Marker file recording that the first-tag notice has been shown, so it
    /// never shows again. A dedicated file (not a settings field) keeps it out
    /// of the settings the UI round-trips.
    fn first_tag_notice_marker(app: &AppHandle) -> Option<PathBuf> {
        Some(
            crate::paths::data_dir(app)
                .ok()?
                .join("first-tag-notice-shown"),
        )
    }

    /// One time, the first time a `Nugget` tag is written on this profile, tell
    /// the user their annotated files now carry a Finder tag and where to change
    /// its colour. WKWebView has no `alert`/`confirm`, so this uses the native
    /// dialog plugin (as `updater.rs` does), on a detached thread so tagging is
    /// never blocked. The marker is written *before* showing, so a bulk resync
    /// (many `set_note_tag` calls in a row) still shows it at most once.
    fn maybe_show_first_tag_notice(app: &AppHandle) {
        use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

        let Some(marker) = first_tag_notice_marker(app) else {
            return;
        };
        if marker.exists() {
            return;
        }
        if std::fs::write(&marker, b"").is_err() {
            // Could not persist the flag — skip rather than risk showing it on
            // every tag from now on.
            return;
        }
        logfile::log(app, "tags: showing first-tag notice (once)");
        let app = app.clone();
        std::thread::spawn(move || {
            app.dialog()
                .message(
                    "Files with notes now get the macOS tag \u{201C}Nugget\u{201D}, so Finder \
                     shows a coloured dot next to them on the Desktop and in every Finder \
                     window. You can change the dot colour — or turn it off — in Settings.",
                )
                .title("Your notes now tag their files")
                .kind(MessageDialogKind::Info)
                .blocking_show();
        });
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
