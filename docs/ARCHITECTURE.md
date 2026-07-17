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

### 3. Note capture (editor window)

- Global hotkey (default e.g. `Ctrl+Shift+N`) while a desktop icon is selected, plus tray-menu entry. True right-click context-menu entry on desktop icons requires a shell extension — defer post-MVP; hotkey first.
- Editor: TipTap in a Tauri window. MVP marks: bold/italic, bullet list, checkable todo list, hyperlinks.
- File links: special link scheme (`nugget://open?path=...`) → backend opens Explorer via `explorer /select,"<path>"`.

### 4. Storage — sidecar files (user decision)

- Per-directory hidden folder: `<dir>\.nuggets\<filename>.nugget.json` (set FILE_ATTRIBUTE_HIDDEN). One JSON per annotated item: rich text content (TipTap JSON), created/modified timestamps, outbound links, schema version.
- Folder notes: `<dir>\.nuggets\_self.nugget.json` inside the folder itself → note travels when the folder is copied/synced.
- Rename/move within a watched dir: track via `ReadDirectoryChangesW` (rename events) and update sidecar names. Moves out of watched scope: note travels only if `.nuggets` travels (folder notes do; file notes in a different parent don't — accept for MVP, document it).
- App maintains a lightweight SQLite index (path → nugget) purely as a cache for the "show all tagged items" main window; sidecars are the source of truth, index is rebuildable.

### 5. Main window ("all nuggets" view)

Standard Tauri window listing all indexed nuggets: name, path, note preview, last edited. Click → open Explorer at item; edit button → editor window.

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

Settings (persisted in app config, applied to overlay panel, editor, and main window):

- **Font size**: S / M / L / XL for overlay panel text (editor and main window follow).
- **Panel scale**: overlay panel size adjustable up to a cap (~1.5×), keeps on-screen positioning logic.
- **Themes**: dark / light / follow-system for MVP; theme system designed so more themes slot in later (CSS custom properties, one theme file per theme).
- **System respect**: honor Windows Reduced Motion (disable panel fade/slide), High Contrast mode (fall back from acrylic to solid high-contrast colors), WCAG AA contrast minimum in both themes.
- **Keyboard access**: global hotkey flow means notes are creatable/editable without mouse; editor and main window fully keyboard-navigable.
- Badge size follows font-size setting so the cue stays visible at higher scales.

## Process model

Single background process, tray icon, autostart (registry Run key, user-toggleable). Tray menu: open main window, pause overlay, settings, quit.
