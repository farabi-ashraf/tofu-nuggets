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

Poll cursor position with a low-frequency timer (e.g., 100 ms). The UIA hit-test runs only when the foreground window is the desktop shell (`Progman`/`WorkerW`) **or a File Explorer content window (`CabinetWClass`)** — any other foreground skips it, so the engine stays idle. Debounce ~400 ms hover before showing the panel.

Resolve icon display name → full path via the desktop folder's shell items (`IShellFolder` enum of `FOLDERID_Desktop` + public desktop), matching by display name. **Inside an Explorer window** the name resolves against that window's current folder, read via `IShellWindows` → `IShellBrowser` → `IFolderView2` (E0 spike `spikes/explorer-pill`); Win11 tabs share one HWND and the active tab is the one whose shell-view window is visible.

**Spike result (2026-07-17, `spikes/hover-detect`): GO.** UIA approach validated on Win 11 — 51/51 desktop icons detected via `ElementFromPoint` with correct path resolution; covered icons correctly report the covering window (the production "don't show panel" case). Findings to carry into the real implementation:

- Desktop is often **OneDrive-redirected** (`FOLDERID_Desktop` → `...\OneDrive\Desktop`) and merged with `FOLDERID_PublicDesktop` — always resolve against both. Bonus: sidecar notes on such desktops sync via OneDrive for free.
- **Virtual icons** (This PC, Recycle Bin) have no filesystem path — skip them for annotation in MVP.
- Display-name → path matching must try both full filename and stem (extension hiding).

**macOS (`desktop_mac.rs`, mirror of the above).** The hit-test is the AX API's `AXUIElementCopyElementAtPosition` (the `ElementFromPoint` analogue), and the same two surfaces are covered: the Finder desktop **and Finder browser windows** (M5).

- **Gate (perf):** before any hit-test, `finder_frontmost` checks that Finder is the focused application (`AXFocusedApplication` pid vs Finder's pid). Any other app frontmost ⇒ no AX work at all, so the engine is idle — the macOS analog of the Windows foreground-surface class check. "Desktop foreground" here means Finder is frontmost with the desktop as the active surface (no key browser window); this is the direct parity with Windows requiring `Progman`/`CabinetWClass` foreground, and desktop hover therefore only fires while the desktop is actually foreground.
- **Route (per hit):** desktop vs Finder window is decided by AX *shape*, not window size — an `AXWindow` in the hit's ancestor chain means a Finder browser window, and no `AXWindow` means the desktop (the Finder desktop is not inside an `AXWindow`). This replaced a display-size heuristic that read a **maximized icon-view window** as the desktop and fired the panel over the empty space between its icons (M5 fix).
- **Finder-window item → path from its `AXURL`.** A Finder item exposes `AXURL` as a *file-reference* URL (`file:///.file/id=…`) that `CFURLCreateFilePathURL`→`CFURLCopyFileSystemPath` resolve to the real path. The **window's** `AXDocument` is empty on Finder folder windows (macOS 26 hardware), and the item URL is better regardless: it names the exact file (no hidden-extension name matching) and it belongs to the active tab, since Finder renders only the active tab (inactive tabs are absent from the AX tree). So a multi-tab window resolves the front tab's items directly — no cursor-in-view guessing, unlike Explorer's live per-tab HWNDs. Where the URL sits varies by view (on the hit in icon/column, a cell's text field in list/details), so `finder_item` climbs from the hit through item-level elements, stopping at content containers (`AXList`/`AXTable`/`AXOutline`/`AXBrowser`/`AXScrollArea`/`AXSplitGroup`) so a hit on the empty space between items — which lands on a container — resolves to nothing.
- The desktop path still resolves its icon name with `icons::resolve_path` (full filename **and** stem, extension-hidden-safe). Everything stays in **points** end to end (never physical pixels — see the units note in the module header).

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

- Global hotkey (`tauri-plugin-global-shortcut`, default `Ctrl+Shift+N`): targets the icon under the cursor, falls back to the UIA-selected icon. Tray-menu entry comes with M5; shell context menu stays post-MVP.
- **Hotkey is settings-driven and re-registerable** (`hotkey.rs`, post-M7): registered in setup from `settings.hotkey`; a registration failure (combination already taken by another app — a real support case) is logged and non-fatal so the user can rebind in Settings. `set_settings` re-registers before persisting and restores the old binding on failure. The handler marshals onto a plain `std::thread` (with COM init for UIA) before opening the editor — same safe-window-creation rule as everywhere else.
- **Diagnostics**: `logfile.rs` appends to `%APPDATA%\com.tofunuggets.app\tofu.log` (capped ~512 KB). The app is headless in the tray on installed machines, so this file is the only way remote users can report silent failures (hotkey clash, UIA misses, window-creation errors, tray clicks are all logged).
- Editor: TipTap (StarterKit + Link + TaskList/TaskItem + Placeholder) in a dark undecorated Tauri window, Vite-built (`ui/` is now an npm package; `npm run build` must run before `cargo build` since assets embed from `ui/dist`). Marks: bold/italic, bullets, checkable todos, hyperlinks. Ctrl+S saves, Esc saves-and-closes.
- `save_nugget` command writes the sidecar (preserving `created_ms`) and upserts the index; badges pick the change up on their next 2 s refresh.
- File links (implemented, Milestone 4): editor 📄/📁 buttons use `tauri-plugin-dialog` to pick a file/folder, inserting a TipTap link with href `nugget://open?path=<encoded abs path>` and the basename as text. JS decodes the path and calls backend commands (`links.rs`): `open_in_explorer` (folder → open it, file → `explorer /select`) and `open_external` (http(s) → default browser), both via `ShellExecuteW`. Panel intercepts link clicks (it can't navigate); the editor follows links on Ctrl+click.
- **Gotcha — TipTap strips custom protocols**: TipTap Link's URI validation only allows an http(s)/mailto/… allowlist. Without `Link.configure({ protocols: ["nugget"], isAllowedUri: (url, ctx) => url.startsWith("nugget://") || ctx.defaultValidate(url) })`, any note *reopened* in the editor and saved has its `nugget://` hrefs silently stripped to `href=""` — the link text survives, the target is destroyed (found post-M6; insertion works, the loss happens on re-parse). Overlay link failures now flash a message in the panel's path line instead of being swallowed.
- **Gotcha — window write-ops need explicit capabilities**: `core:default` / `core:window:default` grant only *read* permissions. `getCurrentWindow().close()` and the `data-tauri-drag-region` titlebar need `core:window:allow-close` / `core:window:allow-start-dragging` in `capabilities/default.json`, and a missing permission surfaces only as an unhandled promise rejection (the editor simply "wouldn't close").

### Desktop infotip suppression (Milestone 4)

Explorer's native icon infotip (folder-contents / file-type tooltip) pops *over* our panel — unusable for folders. `desktop::suppress_desktop_infotips()` clears `LVS_EX_INFOTIP` (0x0400) on the desktop `SysListView32` via `LVM_SETEXTENDEDLISTVIEWSTYLE`. Desktop-only, reverts on Explorer restart, so it's re-applied on each 2 s badge refresh. Now the panel is the sole hover surface.

**Inside Explorer content windows (E1) the same trick does not apply**: those views are `DirectUIHWND`, not a Win32 `SysListView32`, so the ListView message has no target and there is no per-window infotip toggle to clear. The panel is always-on-top and offset to the item's side, so the native infotip (which appears at the cursor) coexists with it rather than hiding it — no Explorer infotip suppression is implemented, deliberately (not hacked around).

### Todo checkboxes in the panel (Milestone 4)

TaskList checkboxes render live in the panel; toggling one reflects `data-checked` into the markup and calls `save_nugget`, so the change round-trips through the sidecar and index. The panel receives clicks despite being non-activating (`set_focusable(false)` blocks keyboard focus, not mouse input).

### 4. Storage — sidecar files (user decision)

- Per-directory hidden folder: `<dir>\.nuggets\<filename>.nugget.json` (set FILE_ATTRIBUTE_HIDDEN). One JSON per annotated item: rich text content (TipTap JSON), created/modified timestamps, outbound links, schema version.
- Folder notes: `<dir>\.nuggets\_self.nugget.json` inside the folder itself → note travels when the folder is copied/synced.
- Rename/move within a watched dir: `notify` crate watcher (wraps `ReadDirectoryChangesW`) renames the sidecar and updates the index. Windows delivers same-dir renames as one two-path event; cross-dir moves arrive as remove+create, so a move out of watched scope leaves a stale sidecar behind (harmless: index rebuilds skip sidecars whose item is missing). Folder notes always travel inside their folder.
- **Unwritable parents — sidecar redirect (0.1.1 A4)**: some desktop items live where a standard user can't create `.nuggets` (e.g. `C:\Users\Public\Desktop` — Logitech G HUB and other all-users installer shortcuts). When the primary write returns `PermissionDenied`, the sidecar is redirected into the user's own desktop `.nuggets` as `<name>.<pathhash>.nugget.json`, with the item's absolute path stored in a `target` field (the hash keeps same-named items from different folders apart). `read_nugget`/`has_nugget` fall back to the redirect after the primary; `index::scan_root` and the watcher's `sidecar_to_item` prefer `target` when present. Notes stay user documents beside the user's files (survive reinstall/uninstall), just relocated to the writable desktop. Redirect root = `desktop_dirs()[0]`, set once at startup via `storage::set_redirect_root`.
- **Index scope for notes outside the desktop (E2 decision)**: the startup rebuild scans a root list, and before E1 that list was the desktop dirs — complete, because notes could only exist on desktop icons. Explorer note-creation broke that: the in-session `upsert_item` puts the note in the main-window list immediately, but nothing rescanned that folder on the next launch, so it silently vanished from the list (the sidecar was never at risk). Decision: `roots.rs` records the **parent folder of every saved note** in `known_roots.json`, and the startup rebuild scans desktop dirs + known roots; entries whose folder no longer exists are dropped on load. The **FS watcher stays desktop-only** — an unbounded set of watched user folders is exactly the background cost the budget below rules out, and the only thing it would buy is live index updates for renames/deletes outside the desktop, which the next rebuild fixes anyway. Rejected: watching every known root (cost), indexing on hover (makes a read path write), and a full-disk sidecar scan (minutes, and it would index other people's folders on shared machines). Like the index itself, the list is a rebuildable cache: losing it costs list visibility of old off-desktop notes, never a note.
- App maintains a lightweight SQLite index (`rusqlite`, DB in app-data dir) purely as a cache for the "show all tagged items" main window; sidecars are the source of truth, index is rebuilt from a full scan at startup and kept fresh by the watcher.
- **Implemented + tested (Milestone 2)**: `storage.rs` / `index.rs` / `watcher.rs`, 10 unit tests.
- **Deletion (added post-M7)**: two paths, same backend. Saving a note with no visible text (`storage::is_empty_html` — tags stripped, trimmed) counts as removal: `save_nugget` returns `removed=true`, deletes the sidecar (and the `.nuggets` dir when that leaves it empty), drops the index row, and emits `nuggets:changed`; the badge dot and hover panel disappear on their next refresh since both re-read the sidecar. The main window's per-row Delete button calls the explicit `delete_nugget` command (same removal helper) behind a two-step in-row confirm ("Delete" → "Sure?", 3 s auto-disarm) — deliberately not a native dialog: no extra capability, no focus steal, automatable.

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
- Pause: a shared `Paused` (`AtomicBool` in `appstate.rs`) checked by the hover engine (hides panel, skips detection) and, on Windows, the badge layer (hides dots). On macOS pause affects hover only — the badges are Finder tags Finder draws, so they stay. Toggled from the tray.
- Autostart: `tauri-plugin-autostart` (registry Run key), toggled from the tray; state read back to check the menu item.
- Background app: no window at startup; `RunEvent::ExitRequested` already prevents exit when windows close (added in M2 for idle release), so closing the main window leaves the app in the tray.

### 6. Badge (visual cue for annotated icons)

Small dot on a corner of each annotated icon so users spot them at a glance. The
two platforms reach it by completely different mechanisms.

#### Windows — our own badge layer

- One full-desktop, click-through layered TOPMOST window (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`), per-pixel alpha via `UpdateLayeredWindow`, drawn natively (GDI) — no webview involved, near-zero cost between redraws.
- Badge positions come from tagged icons' bounding rects (UIA).
- **Occlusion model (0.1.1 A2)**: the layer stays shown; each dot is individually occlusion-tested against visible, non-cloaked, non-minimized top-level windows (`EnumWindows` + `DWMWA_CLOAKED`) and skipped while any window overlaps its pixels — dots never draw over applications and persist whenever the desktop is visible (no show/hide on focus changes). `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND` + `EVENT_OBJECT_LOCATIONCHANGE`, coalesced via an 80 ms one-shot timer) re-runs the occlusion pass within ~100 ms of any window move; the 2 s timer refreshes icon/sidecar state, skipping the UIA walk while one window covers the whole virtual screen. Repaints are skipped when the visible-dot set is unchanged.
- Rejected (0.1.1 A2 spike, `spikes/badge-reparent`): reparenting the layer into the desktop z-band (Progman/`SHELLDLL_DefView`) — Win11 26200 does not composite foreign windows there (layered/child/shaped windows never render; plain ones only erratically).
- Settings: toggle on/off, badge corner, badge size (tied to accessibility scale).
- Rejected: `IShellIconOverlayIdentifier` shell overlays — only 15 system-wide slots (Dropbox/OneDrive contention), requires shell extension, affects Explorer too.

#### macOS — Finder tags (D3, M4a)

macOS draws no window of ours. The app writes a Finder tag named **`Nugget`** to
each annotated file's `com.apple.metadata:_kMDItemUserTags` xattr (`tags.rs`), and
Finder draws (and occludes) the dot itself — on the Desktop *and* inside every
Finder window, at every zoom, through every reshuffle. So macOS has **no overlay
window, no pill, no occlusion pass, and no per-tick UIA/CG walk** for badges; the
badge half of the file-manager update costs nothing to draw there. The old
transparent webview badge layer (`badges_mac.rs`, PRs #25/#26) is deleted.

- Sidecars stay the source of truth; the tag is a derived cue. It is written when
  a note is saved, removed when a note is deleted or emptied, and resynced from
  the index (itself rebuilt from sidecars) at startup so third-party drift heals.
- The tag xattr is a binary-plist array of `"Name\nColorCode"` strings. **Identity
  is the name**, so write hygiene is mandatory: read-modify-write immediately
  before writing, preserve every foreign entry verbatim, abort and write nothing
  if the payload is not a plist array of strings, and no-op when already correct
  (tags sync via iCloud/Dropbox — pointless writes cost the user traffic). Pure
  transforms unit-tested on every platform; the xattr syscalls are macOS-only.
- Colour comes from the shared cross-platform `badge_color` setting (default
  orange), so Windows and macOS honour one control (M4b). The seven names map to
  a Finder tag colour code (`settings::badge_color_code`, unit-tested) on macOS
  and to an RGB (`settings::badge_rgb`) on Windows. Changing it live-recolours:
  `set_settings` resyncs every `Nugget` tag on macOS (the read-modify-write drops
  the stale `Nugget\n<old>` before writing `Nugget\n<new>`) and pokes the Windows
  dot painters to repaint. An unknown name normalises back to orange.
- **First-tag notice (macOS):** the first time a `Nugget` tag is actually
  written on a profile, a one-time `Info` dialog (the dialog plugin, never
  `window.alert` — WKWebView has none) tells the user their notes now tag their
  files and where to change the colour. A marker file (`first-tag-notice-shown`)
  makes it fire at most once, even across a bulk startup resync. No onboarding
  screen — deliberately rejected as scope creep and a worse first launch.
- **Rejected: `FIFinderSync` extension.** The "official" API, but it needs an
  Xcode appex target the Tauri bundler cannot embed, a custom CI codesign path, a
  user visit to System Settings to enable it, and has unverified interactions with
  ad-hoc signing. Revisit if tags prove insufficient or an Apple Developer account
  is bought. Shell icon overlays have no macOS analogue we use either — tags are
  the whole mechanism.

### 7. Explorer pill + dots (E2/E3, Windows)

The desktop gets a persistent dot layer; File Explorer windows deliberately do
not (scroll jitter, viewport clipping, Win11 tab churn — MEMORY.md, the pill
design). Instead each Explorer window gets one small glassy chip at the
bottom-right of its content area showing how many items in that window's active
folder carry a note, and clicking that chip reveals the dots on demand (E3).

- **GDI, not a webview.** One `WebviewWindowBuilder` pill per Explorer window
  would start a WebView2 process tree each (tens of MB, and a user can have five
  windows open), which alone breaks the RAM budget below. So a pill is a layered
  window pushed with `UpdateLayeredWindow` exactly like the badge layer, and the
  chip — rounded rect, border, accent dot, digits — is composited per pixel in
  `pill.rs`. Text is rendered by GDI into a second DIB and read back as a
  coverage mask, because GDI writes no alpha. **Measured cost: ~80 KB per pill**
  (two pills moved the process working set 57.36 → 57.52 MB).
- **The "glass" is a translucent fill, not acrylic**: `UpdateLayeredWindow` and
  DWM blur-behind do not compose, and real blur means buying a webview back. Fill
  alpha, border and accent match the overlay panel's tokens.
- **Z-order is ownership, not TOPMOST** (E0 verdict C; also the 0.1.4 taskbar
  lesson): `SetWindowLongPtrW(GWLP_HWNDPARENT)` = the Explorer window. The pill
  then sits above that window only, auto-hides when it is minimized, and stays
  below any unrelated app raised over Explorer — which is the desired behavior,
  not a limitation. An owned popup does **not** follow its owner's moves and does
  **not** die with it, so an `EVENT_OBJECT_LOCATIONCHANGE` hook (coalesced,
  60 ms) re-places it and a liveness check destroys it once the owner is gone.
- **Z-order upkeep**: an owned popup sits above its owner only until the owner is re-activated, which then stacks the owner on top of it — so a pill drawn while its Explorer window was already foreground rendered behind that window and looked absent until a refocus. Every render re-inserts the pill at the slot immediately above its owner (`SetWindowPos` after `GetWindow(owner, GW_HWNDPREV)`, `SWP_NOACTIVATE`), never `HWND_TOP` — so it stays glued above its own Explorer window yet below any unrelated app raised over that window.
- **Placement**: bottom-right of the SHELLDLL_DefView rect — the item area
  proper, which already excludes the toolbar, the navigation pane and the status
  bar — inset 12 logical px, minus `SM_CXVSCROLL` on the x axis to clear the
  scrollbar. Note this is NOT `IShellBrowser::GetWindow`, which returns the whole
  ShellTabWindowClass; a pill placed against that rect lands on the status bar's
  view-mode buttons.
- **Active tab without a cursor**: E1's hover picks the tab whose view contains
  the cursor, which the pill cannot do (it must be correct in an unfocused,
  un-hovered window), and `IsWindowVisible` on tab views is not a reliable
  discriminator (E1 found it resolving the wrong folder). The pill probes a point
  in the middle of the frame's content area and walks *down* the child-window
  tree with `ChildWindowFromPointEx`, which answers "what does this frame put
  here" regardless of what covers the window — unlike `WindowFromPoint`.
- **The count is a folder read, never a tree walk**: `storage::count_notes_in_folder`
  lists that folder's `.nuggets`, the folder itself (for sub-folders carrying
  their own `_self` note) and the redirect root's `.nuggets` (items whose parent
  was unwritable). Cost is independent of how many notes exist elsewhere. No UIA
  is involved in count mode at all — that is the dots' cost, paid on click.
- **Zero notes = no pill** (hidden, not "0"): a chip in every Explorer window
  would be noise, and in count mode there is nothing behind it to reveal.
- **Click → dots, a snapshot (E3)**: clicking the pill takes a one-shot UIA
  snapshot of the annotated *visible* items (`desktop::annotated_item_rects`,
  scoped to the shell-view HWND with `ElementFromHandle` to stay in the
  ~90–145 ms band E0 measured) and draws a desktop-style badge dot over each in a
  second owned layered window — this one `WS_EX_TRANSPARENT`, so a click where a
  dot sits falls through to the file. Visible = `IsOffscreen == false` AND the
  rect intersects the content area (virtualized views drop scrolled-out items and
  leave a one-row fringe; list view materializes everything, so the rect test
  carries there). **This is a snapshot and must stay one.** It is deliberately
  *not* live-tracked — no scroll-sync, no viewport clipping math, no per-tab
  reposition state, which is the entire reason the Explorer surface is a
  click-to-reveal pill and not a persistent overlay. Instead the dots are
  *dismissed* on the first thing that could invalidate them: a scroll or resize
  (item `EVENT_OBJECT_LOCATIONCHANGE` inside the frame), focus loss
  (`EVENT_SYSTEM_FOREGROUND`), a window move, a folder navigation or tab switch
  (`EVENT_OBJECT_NAMECHANGE`). Dismissal is global (any qualifying change drops
  every window's dots) — simpler than per-window bookkeeping and momentary by
  design. The pill shows an accent-bordered active state while its dots are up;
  clicking again toggles off. All the dismissal hooks are gated on dots actually
  being shown (`DOTS_ACTIVE`), so this machinery costs nothing in the common case
  and does not touch the idle budget.
- Accessibility: font-size preset, panel scale, theme and high contrast are read
  from settings on every redraw and per-monitor DPI from the owner window; high
  contrast switches to opaque system colors (`GetSysColor`), and the dot uses the
  system highlight colour there instead of `badge_color` (accessibility wins).
  Reduced Motion needs nothing — the pill has no animation by construction. It
  also respects the badges on/off setting and tray Pause, both of which destroy
  every pill.
- The Settings badge-colour picker is seven swatches: selection is shown by a
  ring **and** a checkmark (never colour alone, so it survives High Contrast and
  colour-blindness), and each chip keeps its colour under forced-colors
  (`forced-color-adjust: none`) so the palette stays usable. The explainer beside
  it names the mechanism per platform (Finder tag on macOS, dot on Windows).

## Performance budget (hard requirements)

The pitch is "light layer on top of the desktop" — these are commitments, not aspirations:

| State | CPU | RAM |
|---|---|---|
| Idle, neither desktop nor Explorer foreground | ~0% (10 Hz cursor poll runs but the UIA hit-test is gated off) | ~15–20 MB (core process) |
| Desktop or File Explorer foreground, watching | <0.1% (10 Hz cursor timer; UIA hit-test only after ~400 ms hover debounce) | core + badge layer (negligible) |
| Explorer window open but NOT foreground | a 2 s tick that re-enumerates the shell (a couple of windows, sub-ms to low-ms) and re-reads the active folder; below the badge-walk noise in whole-process CPU (see the isolation measurement below) | core + ~80 KB per pill |
| No Explorer window open | pill has no timer at all and its hooks return immediately | core |
| Panel/editor visible (WebView2 warm) | UI-bound only | +60–80 MB while warm |

- **Icon count does not affect hover cost**: detection is a single `ElementFromPoint` hit-test at the cursor, not per-icon scanning. 100 icons and 1000 icons cost the same — the Explorer path adds only a per-hit folder-path read (no per-item scan). Badge refresh enumerates tagged-icon rects only — a few ms every few seconds, only while desktop is foreground.
- **Foreground gate**: the UIA hit-test runs only when the foreground window is the desktop shell or a `CabinetWClass` Explorer window; otherwise the engine does no UIA work (measured 0 ms CPU over 3 s with neither foreground, E1). **macOS (M5)** has the same gate: `finder_frontmost` (one `AXFocusedApplication` read) short-circuits `icon_at` whenever Finder is not the frontmost app, so with any other app foreground the engine does no AX work — ~0% CPU when neither the desktop nor a Finder window is foreground. There is no macOS badge layer to add cost (Finder draws the tags), so the core process is the whole idle footprint.
- **Pill gate (E2)**: with no Explorer window open there is no pill and no timer at all — the state the budget cares about. Once any Explorer window exists the tick re-enumerates the shell and re-reads the active folder every time (700 ms while one is foreground, 2 s otherwise). An earlier version skipped the shell read on unfocused ticks and relied on WinEvents to heal folder changes; a change no event delivered then left a stale count until a manual refocus (owner hit this several times). Enumerating a couple of windows is sub-ms to low-ms, bounded by the 2 s cadence, so correctness won over saving it. The move/resize path is the one that does not re-enumerate. Nothing that can change a count while the user looks elsewhere is missed: a folder navigation or tab switch renames the frame and is caught by an `EVENT_OBJECT_NAMECHANGE` hook (low volume, so processed unconditionally), a new or closed window by a class-only `EnumWindows` scan, and `pill::wake()` is called explicitly for the cases no WinEvent reports to a sleeping layer — the editor on save/delete (`notes_changed`), `open_in_explorer` (a window opened from our own foreground window does not steal focus, so it opens in the background), and the tray Pause toggle on resume (pause kills the layer's timer). **The foreground hook filters on `CabinetWClass` in the callback**: an unfiltered version opened the grace window on every foreground change on the desktop and that alone was measurable.
- **Measuring this is easy to get wrong.** Whole-process CPU is dominated by the badge layer's 2 s UIA walk, which short-circuits only while some window covers the whole virtual screen — so the same build reads 0 ms or 250 ms per 25 s depending on nothing but whether the desktop is covered. The pill's cost was isolated by building with `pill::spawn` removed: **250 ms / 25 s without the pill layer vs 218–234 ms with it**, i.e. the pill is inside the noise. Use that method, not a bare before/after reading, for any future budget claim here.
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
- **Badge toggle**: `settings.badges`. Windows: read by the badge layer each 2 s
  refresh; when off the layer hides but infotip suppression keeps running (the
  panel must stay the sole hover surface even with dots off). macOS: toggling it
  strips our Finder tag from every annotated file (and re-adds it when switched
  back on) — Finder has no live flag to read, so the state change is a one-shot
  resync (`tags::resync`).
- **Settings window**: opened from the tray (`Settings…`), same build path as the
  main window; itself imports `theme.js` so it previews changes live.
- **Keyboard access**: global hotkey flow means notes are creatable/editable
  without mouse; editor and main window fully keyboard-navigable.
- **Known follow-up**: window title bars stay DWM dark regardless of light theme
  (content themes correctly); syncing the immersive-dark attribute to theme is
  deferred cosmetic polish.

## Process model

Single background process, tray icon, autostart (registry Run key, user-toggleable). Tray menu: open main window, pause overlay, settings, quit.

- **Single instance** (`tauri-plugin-single-instance`, registered first): a second launch (autostart + manual start, double-click) hands off to the running instance — which opens the main window — instead of spawning a duplicate hover engine that clashes on the global hotkey. This exact duplicate-instance state was observed in the wild before the guard existed.
- **Tray handlers must not build windows on their own thread**: `WebviewWindowBuilder::build()` is only reliable from a plain worker thread (see §2 findings / M5 deadlock notes), so the tray's Open/Settings handlers and the single-instance callback all `std::thread::spawn` before calling `mainwin::show` / `settings::show`. Verified: main window opens from this path.
