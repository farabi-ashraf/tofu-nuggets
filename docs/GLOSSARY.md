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
| **Badge layer** | Full-desktop click-through window drawing dots on annotated icons. Windows: GDI layered window, no webview (`badges.rs`). macOS: transparent webview window fed by `badges:update` (`badges_mac.rs`). |
| **Pill** | Small glassy count chip drawn per File Explorer window (`pill.rs`): bottom-right of the content area, showing how many items in that window's active-tab folder carry a note. Hidden at zero. Owned by (not TOPMOST over) its Explorer window. Clicking it toggles the dots snapshot (E3) and the pill shows an active, accent-bordered state while dots are up. |
| **Dots (Explorer)** | The E3 on-click reveal: a one-shot UIA snapshot (`desktop::annotated_item_rects`) of the annotated *visible* items in a pill's window, drawn as desktop-style badge dots in a click-through (`WS_EX_TRANSPARENT`) layered window owned by the Explorer top HWND. **A snapshot, never live-tracked** — dismissed (not repositioned) on the first view change: scroll, focus loss, move/resize, folder/tab change (`DOTS_DISMISS`). Per-window; clicking the pill again toggles off. |
| **Known roots** | Folders outside the desktop where a note has been created, persisted as `known_roots.json` and scanned by the startup index rebuild alongside the desktop roots (`roots.rs`). The FS watcher stays desktop-only. |
| **Hover engine** | Polling loop (cursor + UIA hit-test) deciding when to show/hide the panel. Scope is the desktop **and** File Explorer content windows (E1); the hit-test is gated to run only while one of those is foreground. |
| **Main window** | "All nuggets" list (filter, Open/Edit/Delete rows). |
| **Editor** | TipTap rich-text window opened by hotkey or Edit. |
| **`nugget://` link** | Editor link scheme for file/folder targets, resolved by `links.rs` via ShellExecute. |
| **Idle release** | Destroying the overlay window after inactivity so WebView2's process tree exits (RAM back to core baseline); recreated on next hover. |
| **Virtual icon** | Desktop item with no filesystem path (This PC, Recycle Bin) — not annotatable. |
| **`DesktopIcons` trait** | Portable icon-provider abstraction in `icons.rs` (B2). Windows impl = `desktop.rs` (UIA; resolves desktop icons **and** File Explorer items, E1); macOS = `desktop_mac.rs` (stub until AX-API impl lands). Hover engine, editor, and main wiring only touch `crate::icons`. |
| **Active tab (Explorer)** | Win11 tabs share one top-level HWND, one `IShellBrowser` each, with no switch event. Hover (E1) picks the tab whose view contains the cursor; the pill (E2) has no cursor, so it probes a point in the middle of the content area and walks *down* the child-window tree (`ChildWindowFromPointEx`), which is unaffected by other apps covering the window. `IsWindowVisible` is NOT a reliable discriminator (E1 finding). The pill catches a folder/tab change from the frame's `EVENT_OBJECT_NAMECHANGE` (so it updates even when Explorer is not focused) and otherwise re-reads on its foreground poll. |
| **Foreground surface** | Which surface the Windows hit-test targets, from the foreground window class: desktop shell (`Progman`/`WorkerW`), an Explorer content window (`CabinetWClass`), or neither (no hit-test — engine idle). Save/Open dialogs (`#32770`) fall through to neither. |

## Code map — `app/src-tauri/src/`

| File | Owns | Key entry points |
|---|---|---|
| `main.rs` | App wiring: plugins, managed state, command registry, startup (WebView2 guard, index rebuild, watcher, hotkey, hover, badges, tray) | `main`, `webview_missing_alert` |
| `hover.rs` | Hover engine + panel show/hide/position (DPI, edge flip); platform-agnostic via `icons` | `spawn`, `get_current_nugget` |
| `icons.rs` | `DesktopIcons` trait + portable `Icon`/`IconRect` types + shared display-name→path resolution; re-exports the platform impl (`new_icons`, `cursor_pos`, `desktop_dirs`, …); accessibility-permission commands (`None` = platform needs no grant) | `DesktopIcons`, `new_icons`, `resolve_path`, `accessibility_status`, `open_accessibility_pane` |
| `desktop.rs` | **Windows** `DesktopIcons` impl: UIA icon detection over the desktop **and** File Explorer windows (`foreground_surface` gate; Explorer folder via `IShellWindows`→`IShellBrowser`→`IFolderView2`, active tab by cursor), display-name→path resolution, desktop roots, desktop infotip suppression; cursor-free Explorer window/active-tab enumeration for the pill; one-shot annotated-visible-item snapshot for the dots | `DesktopUia`, `desktop_dirs`, `suppress_desktop_infotips`, `explorer_windows`, `explorer_is_foreground`, `annotated_item_rects` |
| `desktop_mac.rs` | **macOS** `DesktopIcons` impl: AX hit-test hover + `list_icons`/`selected_icon` by walking down from Finder's app element (pid from the CG window list); CG window-list helpers for the badge layer (`onscreen_window_rects`, `display_bounds_pts`); hand-declared FFI, points throughout, Accessibility prompt/status, `debug_cursor_chain` + `debug_finder_tree` dumps for the log | `MacIcons`, `debug_cursor_chain` |
| `overlay.rs` | Overlay window creation (transparency stack) | `create`, `hide_overlay` |
| `badges.rs` | **Windows** badge layer: GDI dot painting, per-dot occlusion, WinEvent-driven refresh | `spawn` |
| `pill.rs` | **Windows** Explorer pill + dots: one owned GDI layered chip per Explorer window (count mode), hand-composited; per-window create/track/destroy, `LOCATIONCHANGE` reposition, foreground-gated polling, accessibility styling. Click toggles a click-through dots overlay from a one-shot UIA snapshot, dismissed on any view change (E3) | `spawn`, `notes_changed`, `wake` |
| `roots.rs` | Known-roots list: records the parent folder of each saved note, loaded by the startup index rebuild | `load`, `record` |
| `badges_mac.rs` | **macOS** badge layer: click-through always-on-top webview window over all displays; per-dot occlusion from the CG window list; 2 s poll (no WinEvent equivalent); dots pushed via `badges:update` | `spawn` |
| `lifecycle_mac.rs` | **macOS** process-survival fix: adds `applicationShouldTerminateAfterLastWindowClosed:` → NO to Tauri's NSApp delegate at setup, so hidden windows no longer end the app | `install` |
| `storage.rs` | Sidecar read/write/delete/rename, redirect logic, HTML preview/empty checks, bulk purge, per-folder note count (the pill's number) | `write_nugget`, `read_nugget`, `delete_nugget`, `rename_sidecar`, `purge_sidecar_dir`, `count_notes_in_folder` |
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
| `badges.html/js/css` | macOS badge dots (absolutely-positioned divs; skips DOM work on unchanged payloads) |
| `editor.html/js/css` | TipTap editor, toolbar, link insertion/normalization, drag-drop of files/folders → `nugget://` links (Tauri drag-drop event, portable) |
| `main.html/js/css` | All-nuggets list, filter, row actions, hotkey hint, data-lifecycle footer, app version + report-bugs notice |
| `settings.html/js/css` | Settings controls, hotkey capture, danger zone (delete all) |
| `hotkeys.js` | Hotkey capture (`event.code`, not `event.key` — Option+letter composes characters on macOS) + per-platform modifier labels (⌘⌥⌃⇧ vs Ctrl/Alt/Win); shared by settings capture and main-window hint |
| `theme.js` | Single applier of font-scale/panel-scale/theme/motion/contrast to `<html>`; imported by every entry |

## Other locations

| Where | What |
|---|---|
| `app/src-tauri/tauri.conf.json` | Version (bump here + Cargo.toml to release), updater endpoint+pubkey, NSIS config |
| `app/src-tauri/nsis/hooks.nsh` | Uninstaller message (notes stay on disk) |
| `app/src-tauri/capabilities/default.json` | Webview permission grants (write-ops need explicit allows) |
| `.github/workflows/release.yml` | Tag `v*` → build+sign on Windows AND macOS matrix → draft release with `.exe` + arm64 `.dmg` + merged `latest.json` |
| `.github/workflows/ci.yml` | PR/push to main → fmt+clippy+test on Windows AND macOS runners (B2 matrix; compile/test gate only, no behavior tests). macOS job also uploads an ad-hoc-signed arm64 `.dmg` artifact (14-day retention) for hardware testing |
| `spikes/` | Historical go/no-go spikes (hover-detect GO; badge-reparent NO-GO) with findings in their READMEs |
| `%APPDATA%\com.tofunuggets.app\` (Windows) / `~/Library/Application Support/Tofu Nuggets/` (macOS) | settings.json, index.db, known_roots.json, tofu.log (per-user runtime data; see `paths.rs`) |

## Events & commands (cross-window contracts)

| Name | Kind | Contract |
|---|---|---|
| `nuggets:changed` | emit → all windows | Note set changed; main window reloads list. Emitted by editor save/delete and delete-all (NOT by the watcher — known gap). |
| `settings:changed` | emit → all windows | Full `Settings` payload; `theme.js` + windows re-apply live. |
| `nugget:show` | emit → overlay | Panel payload; fresh pages pull via `get_current_nugget` instead (emit can beat page load). |
| `badges:update` | emit → badges (macOS) | Array of `{x,y}` dot centers (points, window-relative). Emitted every refresh tick unconditionally (covers page-load race); page skips unchanged payloads. |

## Platform behavior differences

- **macOS ends the process whenever no window is VISIBLE** — hidden ones do not
  count, and the termination skips `ExitRequested`, so `prevent_exit` never sees it
  (proved by tofu.log: `exiting` with no `exit requested`, including ~6 s after a
  launch where no window was ever opened). The fix is the delegate override
  `applicationShouldTerminateAfterLastWindowClosed:` → NO (`lifecycle_mac::install`,
  added to Tauri's own NSApp delegate at setup). With that in place the panel
  **just hides like everywhere else** — the earlier off-screen "parking" of the
  panel is gone (it was a workaround, and the parked window reappeared mid-screen
  after display sleep when AppKit constrained it back on-screen). Windows still
  hide instead of closing on macOS (`main.rs` run handler, the platform
  convention); `CloseRequested` still logs a visible-window census as the
  discriminator if the exit bug ever returns.
- **Idle release is Windows-only**: it reclaims WebView2's process tree; WKWebView has
  no equivalent cost and per-hover AppKit window recreation is a needless risk.
- **Activation policy**: macOS runs as `Accessory` (menu-bar agent, no Dock icon).
- **Badge layer machinery differs entirely**: Windows = GDI layered window +
  WinEvent hooks (push-based occlusion within ~100 ms); macOS = transparent
  click-through webview window + CG window list polled on the 2 s tick (no
  cheap cross-process window-move hook exists). Same 2 s icon/sidecar cadence
  and per-dot occlusion model on both.
- Per-platform wording lives next to the code that shows it: tray autostart label
  (`tray.rs`), file-manager + app-removal wording (`main.js`), modifier labels
  (`hotkeys.js`).

## Known behavior gaps (candidates, not bugs-by-surprise)

- Watcher rename/move updates the index but doesn't emit `nuggets:changed` → open main window shows stale name until reopened.
- Rename while app not running orphans the sidecar (old filename no longer matches; note preserved on disk, unlisted). Renaming back relinks.
- Item moved off the desktop then back: hover+badge relink immediately (sidecar re-read), main list only after next index rebuild (restart).
- **Off-desktop notes are listed, but only their folder is re-scanned, and nothing watches it** (E2 decision, docs/ARCHITECTURE.md §4): saving a note records its parent folder in the known-roots list, which the startup rebuild scans. So a note made on a File Explorer item survives a restart in the main list. What is NOT covered: renaming or deleting such an item while the app runs (no watcher outside the desktop) — the index catches up at the next rebuild, and the sidecar is never lost either way.
- **Mounted volumes on the macOS desktop are not annotatable**: an external disk shows on the desktop but lives at `/Volumes/<name>`, while name→path resolution only searches the desktop roots, so it is reported as a virtual icon ("has no filesystem path"). Adding `/Volumes` as a root would also pull every mounted disk into the index scan — deliberate decision needed before changing it.
- **No `window.prompt`/`alert`/`confirm` in UI code**: WKWebView does not implement them (they silently do nothing on macOS), which is why link entry is an in-page bar in the editor. Keep new UI in-page.
