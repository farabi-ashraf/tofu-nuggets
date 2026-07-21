# Glossary & Code Map

> **Maintenance is mandatory**: any change that adds/renames a module, command, event,
> term, or storage location updates this file in the same PR. This is the entry point
> for any coding agent; the code itself is the source of truth — every `.rs` file opens
> with a `//!` header stating its responsibility and invariants. Read those headers
> before this file when working inside a module.

## Terms

| Term | Meaning |
|---|---|
| **Nugget** | One user note attached to a file/folder. Rich-text HTML fragment + timestamps, stored as a sidecar JSON. |
| **Sidecar** | The JSON file holding a nugget: `<parent>\.nuggets\<filename>.nugget.json`; a folder's own note is `<folder>\.nuggets\_self.nugget.json` (travels with the folder). Source of truth. |
| **Redirected sidecar** | Sidecar for an item whose parent is unwritable (e.g. Public Desktop): lands in the user-desktop `.nuggets` as `<name>.<pathhash>.nugget.json` with the item's abs path in a `target` field. |
| **Index** | SQLite cache (app-data dir) powering the main-window list. Always rebuildable from sidecars; never the only copy of anything. |
| **Overlay / panel** | The glassy hover panel window showing a nugget. Transparent, undecorated, never-focusable. |
| **Badge layer** | Full-desktop click-through layered window drawing dots on annotated icons (GDI, no webview). |
| **Hover engine** | Polling loop (cursor + UIA hit-test) deciding when to show/hide the panel. |
| **Main window** | "All nuggets" list (filter, Open/Edit/Delete rows). |
| **Editor** | TipTap rich-text window opened by hotkey or Edit. |
| **`nugget://` link** | Editor link scheme for file/folder targets, resolved by `links.rs` via ShellExecute. |
| **Idle release** | Destroying the overlay window after inactivity so WebView2's process tree exits (RAM back to core baseline); recreated on next hover. |
| **Virtual icon** | Desktop item with no filesystem path (This PC, Recycle Bin) — not annotatable. |
| **`DesktopIcons` trait** | Portable icon-provider abstraction in `icons.rs` (B2). Windows impl = `desktop.rs` (UIA); macOS = `desktop_mac.rs` (stub until AX-API impl lands). Hover engine, editor, and main wiring only touch `crate::icons`. |

## Code map — `app/src-tauri/src/`

| File | Owns | Key entry points |
|---|---|---|
| `main.rs` | App wiring: plugins, managed state, command registry, startup (WebView2 guard, index rebuild, watcher, hotkey, hover, badges, tray) | `main`, `webview_missing_alert` |
| `hover.rs` | Hover engine + panel show/hide/position (DPI, edge flip); platform-agnostic via `icons` | `spawn`, `get_current_nugget` |
| `icons.rs` | `DesktopIcons` trait + portable `Icon`/`IconRect` types + shared display-name→path resolution; re-exports the platform impl (`new_icons`, `cursor_pos`, `desktop_dirs`, …); accessibility-permission commands (`None` = platform needs no grant) | `DesktopIcons`, `new_icons`, `resolve_path`, `accessibility_status`, `open_accessibility_pane` |
| `desktop.rs` | **Windows** `DesktopIcons` impl: UIA icon detection, display-name→path resolution, desktop roots, infotip suppression | `DesktopUia`, `desktop_dirs`, `suppress_desktop_infotips` |
| `desktop_mac.rs` | **macOS** `DesktopIcons` impl: AX-API hit-test hover (hand-declared FFI, points↔pixels conversion, Accessibility-permission prompt/status) + `debug_cursor_chain` AX dump for the log. `selected_icon`/`list_icons` still stubs (badges Windows-only) | `MacIcons`, `debug_cursor_chain` |
| `overlay.rs` | Overlay window creation (transparency stack) | `create`, `hide_overlay` |
| `badges.rs` | Badge layer: dot painting, per-dot occlusion, WinEvent-driven refresh. Windows-only (no-op stub on macOS, see main.rs) | `spawn` |
| `storage.rs` | Sidecar read/write/delete/rename, redirect logic, HTML preview/empty checks, bulk purge | `write_nugget`, `read_nugget`, `delete_nugget`, `rename_sidecar`, `purge_sidecar_dir` |
| `index.rs` | SQLite cache: rebuild scan, upsert/remove/rename, list, clear | `NuggetIndex`, `scan_root` |
| `watcher.rs` | FS watcher keeping sidecars+index in step with renames/deletes on watched roots | `spawn`, `handle_event` |
| `editor.rs` | Editor window + save/delete commands | `open_for_path`, `save_nugget`, `delete_nugget` |
| `mainwin.rs` | Main window + list/edit/open/delete-all commands | `show`, `list_nuggets`, `delete_all_nuggets` |
| `settings.rs` | Settings model (serde-default backfill), persistence, live apply via event | `Settings`, `get_settings`, `set_settings` |
| `hotkey.rs` | Global hotkey registration/rebinding (failure non-fatal) | `register`, `reregister` |
| `tray.rs` | Tray icon + menu (open/pause/settings/autostart/updates/quit) | `build` |
| `updater.rs` | "Check for updates" flow (check → confirm dialog → install → restart) | `check` |
| `links.rs` | Opening targets: Explorer select, external browser | `open_in_explorer`, `open_external` |
| `logfile.rs` | Append log in the per-user data dir (512 KB cap) | `log` |
| `paths.rs` | Per-user data dir: Tauri's identifier dir on Windows, `~/Library/Application Support/Tofu Nuggets` on macOS (an identifier ending in `.app` reads as an app bundle to Finder) | `data_dir` |
| `appstate.rs` | Shared pause flag | `Paused` |

## Code map — `app/ui/` (Vite package; `npm run build` BEFORE `cargo build`)

| File | Owns |
|---|---|
| `overlay.html/js/css` | Hover panel rendering, link/checkbox handling |
| `editor.html/js/css` | TipTap editor, toolbar, link insertion/normalization, drag-drop of files/folders → `nugget://` links (Tauri drag-drop event, portable) |
| `main.html/js/css` | All-nuggets list, filter, row actions, hotkey hint, data-lifecycle footer |
| `settings.html/js/css` | Settings controls, hotkey capture, danger zone (delete all) |
| `hotkeys.js` | Hotkey capture (`event.code`, not `event.key` — Option+letter composes characters on macOS) + per-platform modifier labels (⌘⌥⌃⇧ vs Ctrl/Alt/Win); shared by settings capture and main-window hint |
| `theme.js` | Single applier of font-scale/panel-scale/theme/motion/contrast to `<html>`; imported by every entry |

## Other locations

| Where | What |
|---|---|
| `app/src-tauri/tauri.conf.json` | Version (bump here + Cargo.toml to release), updater endpoint+pubkey, NSIS config |
| `app/src-tauri/nsis/hooks.nsh` | Uninstaller message (notes stay on disk) |
| `app/src-tauri/capabilities/default.json` | Webview permission grants (write-ops need explicit allows) |
| `.github/workflows/release.yml` | Tag `v*` → build+sign → draft release + `latest.json` |
| `.github/workflows/ci.yml` | PR/push to main → fmt+clippy+test on Windows AND macOS runners (B2 matrix; compile/test gate only, no behavior tests). macOS job also uploads an ad-hoc-signed arm64 `.dmg` artifact (14-day retention) for hardware testing |
| `spikes/` | Historical go/no-go spikes (hover-detect GO; badge-reparent NO-GO) with findings in their READMEs |
| `%APPDATA%\com.tofunuggets.app\` (Windows) / `~/Library/Application Support/Tofu Nuggets/` (macOS) | settings.json, index.db, tofu.log (per-user runtime data; see `paths.rs`) |

## Events & commands (cross-window contracts)

| Name | Kind | Contract |
|---|---|---|
| `nuggets:changed` | emit → all windows | Note set changed; main window reloads list. Emitted by editor save/delete and delete-all (NOT by the watcher — known gap). |
| `settings:changed` | emit → all windows | Full `Settings` payload; `theme.js` + windows re-apply live. |
| `nugget:show` | emit → overlay | Panel payload; fresh pages pull via `get_current_nugget` instead (emit can beat page load). |

## Known behavior gaps (candidates, not bugs-by-surprise)

- Watcher rename/move updates the index but doesn't emit `nuggets:changed` → open main window shows stale name until reopened.
- Rename while app not running orphans the sidecar (old filename no longer matches; note preserved on disk, unlisted). Renaming back relinks.
- Item moved off the desktop then back: hover+badge relink immediately (sidecar re-read), main list only after next index rebuild (restart).
