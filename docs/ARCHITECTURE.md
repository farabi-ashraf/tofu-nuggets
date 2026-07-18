# Architecture

## Stack recommendation: Tauri 2 (Rust core + webview UI)

| Concern | Why Tauri fits |
|---|---|
| Hover detection over desktop icons | Needs Win32/UI Automation calls — Rust `windows` crate gives full access from the Tauri backend. Electron would need a native addon anyway. |
| Glassy overlay window | Tauri supports transparent, undecorated, always-on-top, click-through windows; acrylic/mica via `window-vibrancy` crate. |
| Rich text editor | Hardest UI piece — trivial in a webview (TipTap/ProseMirror). Native rich text (WinUI RichEditBox) is far more painful. |
| Footprint | Background app must be light. Tauri idles at ~10–20 MB vs Electron's ~150 MB+. Uses system WebView2, ships small. |
| Future macOS/Linux | Tauri is cross-platform; only the hover-detection layer is per-OS. |

Rejected: **Electron** (heavy for an always-running background app), **fully native WinUI/C++** (rich text editor and iteration speed too costly for MVP).

## Core technical problems (in risk order)

### 1. Detecting which desktop icon is under the cursor

The Windows desktop is a `SysListView32` list-view hosted under `Progman`/`WorkerW`. Two viable approaches:

- **UI Automation (recommended)**: `IUIAutomation::ElementFromPoint` on the desktop list gives the item name + bounding rect under the cursor. No cross-process memory games. Works on Win 10/11.
- Fallback: `LVM_GETITEMPOSITION` / `LVM_GETITEMTEXT` with `VirtualAllocEx` cross-process reads — brittle, avoid unless UIA fails.

Poll cursor position with a low-frequency timer (e.g., 100 ms) + `WM_MOUSEMOVE` low-level hook only while desktop is foreground. Debounce ~400 ms hover before showing the panel.

Resolve icon display name → full path via the desktop folder's shell items (`IShellFolder` enum of `FOLDERID_Desktop` + public desktop), matching by display name.

**Spike result (2026-07-17, `spikes/hover-detect`): GO.** UIA approach validated on Win 11 — 51/51 desktop icons detected via `ElementFromPoint` with correct path resolution; covered icons correctly report the covering window (the production "don't show panel" case). Findings to carry into the real implementation:

- Desktop is often **OneDrive-redirected** (`FOLDERID_Desktop` → `...\OneDrive\Desktop`) and merged with `FOLDERID_PublicDesktop` — always resolve against both. Bonus: sidecar notes on such desktops sync via OneDrive for free.
- **Virtual icons** (This PC, Recycle Bin) have no filesystem path — skip them for annotation in MVP.
- Display-name → path matching must try both full filename and stem (extension hiding).

### 2. Overlay panel

- Transparent, undecorated, always-on-top Tauri window, hidden by default, never focusable (`WS_EX_NOACTIVATE` via `set_focusable(false)`).
- Position near icon bounding rect in physical pixels, scaled by `scale_factor()`; flip side near screen edges.
- Dismiss on cursor leave (icon + panel union) with small grace period.
- **Transparency/glass findings (Milestone 1, Win 11 26200):**
  - Tauri `transparent(true)` alone is NOT enough — WebView2 still paints an opaque theme-colored canvas. Must also set `ICoreWebView2Controller2::put_DefaultBackgroundColor` to alpha 0 (via `webview2-com`, which drags in a second `windows-core` version — aliased dep).
  - **OS blur is unavailable for never-activated windows**: DWM system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`) and SWCA acrylic (window-vibrancy) both render a solid grey fill when the window is inactive. Glass look is therefore pure CSS (translucent gradient + border) over a genuinely transparent window. Revisit real blur later via DirectComposition backdrop brush if ever worth it.
  - Rounded corners + dark mode via `DwmSetWindowAttribute` (`DWMWCP_ROUND`, `DWMWA_USE_IMMERSIVE_DARK_MODE`) work fine.
  - Tauri v2 events need a `capabilities/default.json` granting `core:default` to the window, else JS `listen` silently never fires.
  - Explorer's own hover infotip can overlap our panel — suppress or offset later (polish).

### 3. Note capture (editor window) — implemented (Milestone 3)

- Global hotkey `Ctrl+Shift+N` (`tauri-plugin-global-shortcut`): targets the icon under the cursor, falls back to the UIA-selected icon. Tray-menu entry comes with M5; shell context menu stays post-MVP.
- Editor: TipTap (StarterKit + Link + TaskList/TaskItem + Placeholder) in a dark undecorated Tauri window, Vite-built (`ui/` is now an npm package; `npm run build` must run before `cargo build` since assets embed from `ui/dist`). Marks: bold/italic, bullets, checkable todos, hyperlinks. Ctrl+S saves, Esc saves-and-closes.
- `save_nugget` command writes the sidecar (preserving `created_ms`) and upserts the index; badges pick the change up on their next 2 s refresh.
- File links (implemented, Milestone 4): editor 📄/📁 buttons use `tauri-plugin-dialog` to pick a file/folder, inserting a TipTap link with href `nugget://open?path=<encoded abs path>` and the basename as text. JS decodes the path and calls backend commands (`links.rs`): `open_in_explorer` (folder → open it, file → `explorer /select`) and `open_external` (http(s) → default browser), both via `ShellExecuteW`. Panel intercepts link clicks (it can't navigate); the editor follows links on Ctrl+click.
- **Gotcha — TipTap strips custom protocols**: TipTap Link's URI validation only allows an http(s)/mailto/… allowlist. Without `Link.configure({ protocols: ["nugget"], isAllowedUri: (url, ctx) => url.startsWith("nugget://") || ctx.defaultValidate(url) })`, any note *reopened* in the editor and saved has its `nugget://` hrefs silently stripped to `href=""` — the link text survives, the target is destroyed (found post-M6; insertion works, the loss happens on re-parse). Overlay link failures now flash a message in the panel's path line instead of being swallowed.
- **Gotcha — window write-ops need explicit capabilities**: `core:default` / `core:window:default` grant only *read* permissions. `getCurrentWindow().close()` and the `data-tauri-drag-region` titlebar need `core:window:allow-close` / `core:window:allow-start-dragging` in `capabilities/default.json`, and a missing permission surfaces only as an unhandled promise rejection (the editor simply "wouldn't close").

### Desktop infotip suppression (Milestone 4)

Explorer's native icon infotip (folder-contents / file-type tooltip) pops *over* our panel — unusable for folders. `desktop::suppress_desktop_infotips()` clears `LVS_EX_INFOTIP` (0x0400) on the desktop `SysListView32` via `LVM_SETEXTENDEDLISTVIEWSTYLE`. Desktop-only, reverts on Explorer restart, so it's re-applied on each 2 s badge refresh. Now the panel is the sole hover surface.

### Todo checkboxes in the panel (Milestone 4)

TaskList checkboxes render live in the panel; toggling one reflects `data-checked` into the markup and calls `save_nugget`, so the change round-trips through the sidecar and index. The panel receives clicks despite being non-activating (`set_focusable(false)` blocks keyboard focus, not mouse input).

### 4. Storage — sidecar files (user decision)

- Per-directory hidden folder: `<dir>\.nuggets\<filename>.nugget.json` (set FILE_ATTRIBUTE_HIDDEN). One JSON per annotated item: rich text content (TipTap JSON), created/modified timestamps, outbound links, schema version.
- Folder notes: `<dir>\.nuggets\_self.nugget.json` inside the folder itself → note travels when the folder is copied/synced.
- Rename/move within a watched dir: `notify` crate watcher (wraps `ReadDirectoryChangesW`) renames the sidecar and updates the index. Windows delivers same-dir renames as one two-path event; cross-dir moves arrive as remove+create, so a move out of watched scope leaves a stale sidecar behind (harmless: index rebuilds skip sidecars whose item is missing). Folder notes always travel inside their folder.
- App maintains a lightweight SQLite index (`rusqlite`, DB in app-data dir) purely as a cache for the "show all tagged items" main window; sidecars are the source of truth, index is rebuilt from a full scan at startup and kept fresh by the watcher.
- **Implemented + tested (Milestone 2)**: `storage.rs` / `index.rs` / `watcher.rs`, 10 unit tests.

### WebView2 idle release (implemented, Milestone 2)

The overlay window is destroyed after `TOFU_IDLE_RELEASE_SECS` (default 300) without a panel shown, dropping the ~380 MB WebView2 process tree to zero; it is recreated on the next hover (~1 s cold start). Traps discovered:
- Destroying the app's only window triggers Tauri's exit-on-all-windows-closed — a background app must intercept `RunEvent::ExitRequested` (with `code.is_none()`) and `prevent_exit()`.
- Window creation works from a worker thread, but a freshly created page can miss a `nugget:show` emit — the page pulls the current payload via a `get_current_nugget` command on load (state stashed before emit).

### 5. Main window ("all nuggets" view) — implemented (Milestone 5)

Tauri window (`mainwin.rs`) listing indexed nuggets via `list_nuggets`: name, path, preview, last-edited, with a live text filter. Each row: **Open** (`open_in_explorer`) and **Edit** (`edit_nugget`). Reloads on the `nuggets:changed` event emitted by `save_nugget`. Reachable from the tray.

**Threading trap (important):** `WebviewWindowBuilder::build()` *deadlocks* when called from a Tauri async command thread, and also from inside a `run_on_main_thread` closure. It works from a plain worker `std::thread` (the same context the hover engine uses to recreate the overlay). So `edit_nugget` spawns a short-lived `std::thread` to open the editor. The global-hotkey path builds directly (its handler thread is build-safe).

Follow-up: the editor window currently persists once created (~380 MB WebView2). Add editor idle-release like the overlay later; out of scope for M5.

### Tray, pause, autostart (Milestone 5)

- `tray.rs`: tray icon + menu (Open / Pause hover / Start with Windows / Quit). Left-click opens the main window.
- Pause: a shared `Paused` (`AtomicBool` in `appstate.rs`) checked by the hover engine (hides panel, skips detection) and the badge layer (hides badges). Toggled from the tray.
- Autostart: `tauri-plugin-autostart` (registry Run key), toggled from the tray; state read back to check the menu item.
- Background app: no window at startup; `RunEvent::ExitRequested` already prevents exit when windows close (added in M2 for idle release), so closing the main window leaves the app in the tray.

### 6. Badge layer (visual cue for tagged icons)

Small dot/glyph on a corner of each tagged icon so users spot annotated items at a glance.

- One full-desktop, click-through layered window (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`), per-pixel alpha via `UpdateLayeredWindow`, drawn natively (GDI+/Direct2D) — no webview involved, near-zero cost between redraws.
- Badge positions come from tagged icons' bounding rects (UIA). Refresh only while desktop is foreground: on focus gain, every few seconds, and after our own note create/delete. Hidden entirely when desktop not foreground.
- Settings: toggle on/off, badge corner, badge size (tied to accessibility scale).
- Rejected: `IShellIconOverlayIdentifier` shell overlays — only 15 system-wide slots (Dropbox/OneDrive contention), requires shell extension, affects Explorer too.

## Performance budget (hard requirements)

The pitch is "light layer on top of the desktop" — these are commitments, not aspirations:

| State | CPU | RAM |
|---|---|---|
| Idle, desktop not foreground | ~0% (hooks/timers off) | ~15–20 MB (core process) |
| Desktop foreground, watching | <0.1% (10 Hz cursor timer; UIA hit-test only after ~400 ms hover debounce) | core + badge layer (negligible) |
| Panel/editor visible (WebView2 warm) | UI-bound only | +60–80 MB while warm |

- **Icon count does not affect hover cost**: detection is a single `ElementFromPoint` hit-test at the cursor, not per-icon scanning. 100 icons and 1000 icons cost the same. Badge refresh enumerates tagged-icon rects only — a few ms every few seconds, only while desktop is foreground.
- **WebView2 lifecycle**: spawned on first panel/editor show, released after idle timeout (default 5 min, configurable) so RAM returns to core baseline. Cost: first hover after release pays ~300–500 ms cold start; warm hovers render <150 ms.
- **Measured (Milestone 1, debug build)**: main process 51 MB (release build will shrink), WebView2 warm = **379 MB across 6 processes** — far above the original 60–80 MB estimate. The idle-release mechanism is mandatory to meet budget; implement by destroying/recreating the overlay webview window rather than hiding it.
- Disk: installer ~10 MB; nuggets 1–5 KB each; SQLite index <1 MB for hundreds of nuggets.

## Accessibility & theming

**Implemented in Milestone 6.** Settings live in `settings.json` in the app-data
dir (source of truth), modeled by `settings.rs::Settings` with `#[serde(default)]`
so a partial/old file backfills from defaults rather than failing to load.
`panel_scale` is clamped to 1.0–1.5 in `Settings::normalized()`. Two commands:
`get_settings` (pull on load) and `set_settings` (persist + `emit("settings:changed")`).

- **Live apply**: a shared `ui/theme.js`, imported by every window entry, is the
  single applier. On load (and on each `settings:changed`) it writes to `<html>`:
  `--font-scale`, `--panel-scale`, and the attributes `data-theme` (dark|light),
  `data-motion` (full|reduced), `data-contrast` (normal|high). All window CSS is
  authored as `:root` variable defaults + `:root[data-theme="light"]` /
  `[data-contrast="high"]` overrides + a `[data-motion="reduced"] *` rule that
  kills `animation`/`transition`.
- **Font size**: S/M/L/XL → scale 0.85/1.0/1.2/1.45 (mapping lives in `theme.js`;
  Rust never needs the numeric). Applies to overlay, editor, main, settings.
- **Panel scale (overlay only, dual knob)**: `hover.rs::show_panel` sizes the
  window `PANEL_W/H * dpi * panel_scale`, and overlay CSS multiplies its fonts by
  `var(--panel-scale)` too, so the whole panel zooms together. Positioning/edge-flip
  logic is unchanged (operates on the final physical rect).
- **Themes**: dark / light / system. `system` is resolved in `theme.js` via
  `matchMedia('(prefers-color-scheme: dark)')` and re-resolves on OS change. More
  themes slot in as additional `data-theme` blocks.
- **System respect + override**: effective reduced-motion / high-contrast =
  user toggle **OR** the matching OS media query (`prefers-reduced-motion`,
  `prefers-contrast: more`, `forced-colors: active`). So the OS setting is honored
  and the toggle can additionally force it on. High contrast drops the translucent
  glass for solid colors (`--panel-bg: #000` / `#fff`, opaque border).
- **Badge toggle**: `settings.badges`, read by the badge layer each 2 s refresh;
  when off the layer hides but infotip suppression keeps running (the panel must
  stay the sole hover surface even with dots off).
- **Settings window**: opened from the tray (`Settings…`), same build path as the
  main window; itself imports `theme.js` so it previews changes live.
- **Keyboard access**: global hotkey flow means notes are creatable/editable
  without mouse; editor and main window fully keyboard-navigable.
- **Known follow-up**: window title bars stay DWM dark regardless of light theme
  (content themes correctly); syncing the immersive-dark attribute to theme is
  deferred cosmetic polish.

## Process model

Single background process, tray icon, autostart (registry Run key, user-toggleable). Tray menu: open main window, pause overlay, settings, quit.
