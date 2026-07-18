# Project Memory — Handoff for Claude Sessions

> Purpose: any new Claude session reads this + CLAUDE.md and can continue without re-asking settled questions. **Update this file after every session where decisions are made.**

## Status

- **Phase**: Milestone 6 complete — settings + accessibility. `settings.json` store, shared `theme.js`, font size / panel scale / dark-light-system theme / High Contrast (solid colors) / Reduced Motion / badge toggle, tray "Settings…". Verified live against real CSS via dev-server webview + 5 new unit tests (15 total).
- **Next step**: Milestone 7 — polish + installer (MSI/NSIS); measure RAM/CPU against ARCHITECTURE budget on a fresh VM. Carry-over TODOs: right-edge panel flip test, editor idle-release (~380 MB persists once opened), autostart-survives-reboot check, title-bar theme sync (light theme still has DWM-dark title bar — cosmetic).
- **Build note**: frontend is Vite-built now — run `npm run build` in `app/ui` BEFORE `cargo build` (assets embed from `ui/dist`).
- **Demo state**: two seeded nuggets live on the real desktop — `Works` folder and `Untitled-1.psd` (sidecars under `OneDrive\Desktop`). Run `app/src-tauri/target/debug/tofu-nuggets.exe`, hover those icons on the desktop.

## Dev environment notes (this machine)

- Rust via rustup. **Default toolchain now `stable-x86_64-pc-windows-msvc`**; VS Build Tools 2022 + Win 11 SDK installed via winget (2026-07-17). App uses `windows` 0.62. The old GNU-toolchain spike keeps `windows` pinned to 0.58 (raw-dylib/dlltool gap) — leave it.
- App transparency stack is subtle — read ARCHITECTURE.md §2 findings before touching overlay window code (`webview2-com` + aliased `windows-core` 0.61 dep is load-bearing).
- Node v24 + npm available (needed for TipTap/Vite in Milestone 3).
- Repo on E: drive (no ownership recording) — `safe.directory` exception added to global git config.
- User's desktop is OneDrive-redirected; icons split across OneDrive Desktop + Public Desktop.

## Settled decisions (do not re-ask)

| Decision | Choice | Where documented |
|---|---|---|
| MVP hover scope | Desktop icons only; Explorer windows post-MVP | docs/MVP.md |
| Stack | Tauri 2: Rust core + webview UI (TipTap editor) | docs/ARCHITECTURE.md |
| Storage | Sidecar JSON in hidden `.nuggets` folders = source of truth; SQLite index = rebuildable cache | docs/ARCHITECTURE.md |
| Pricing | Free MVP, freemium later (research-backed) | docs/FEASIBILITY.md |
| Hover detection | UI Automation `ElementFromPoint`, not ListView memory reads | docs/ARCHITECTURE.md |
| Note capture | Global hotkey (right-click shell menu deferred — needs shell extension) | docs/ARCHITECTURE.md |
| Visual cue | Badge layer: click-through layered window, native-drawn dots on tagged icons | docs/ARCHITECTURE.md §6 |
| Performance | Hard budget: ~0% CPU idle, ~15–20 MB core RAM, WebView2 released after idle timeout, icon count must not affect hover cost | docs/ARCHITECTURE.md |
| Accessibility | MVP includes font size S–XL, panel scale, dark/light/system themes, Reduced Motion + High Contrast respect | docs/ARCHITECTURE.md |
| Settings storage | `settings.json` in app-data dir; serde-default backfill; applied live to all windows via `theme.js` + `settings:changed` | docs/ARCHITECTURE.md (Accessibility & theming) |

## Owner preferences (from conversations)

- Wants app as light as possible — performance budget is a commitment, treat regressions as bugs.
- **Always update the relevant markdown files (docs/, CLAUDE.md, this file) immediately after any question/discussion that adds or changes functionality.**
- Discuss/clarify before building; owner answers scoping questions willingly.

## Session log

- **2026-07-18 (2)**: Two user-reported bugs fixed (commit 7ec3086). **(1) File links died**: TipTap Link's URI allowlist rejects `nugget://` → reopening + saving a note stripped hrefs to `""` (link text kept, target destroyed). Fix: `Link.configure({ protocols: ["nugget"], isAllowedUri })`. User's seeded "Suzuki Pitch" link already lost its target — must be re-linked by hand. **(2) Editor never closed**: `core:window:default` is read-only; `close()` needs `core:window:allow-close` (added, + `allow-start-dragging` for titlebar drag) and the rejection was silently swallowed. Overlay/editor now surface link/close errors instead of `.catch(() => {})`. **Debug technique that cracked it (reuse this)**: set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`, launch exe, drive real pages via CDP (`http://127.0.0.1:9223/json` + Node's built-in WebSocket, `Runtime.evaluate` with `awaitPromise`) — full E2E in the live app without GUI clicking; scratchpad `cdp.mjs`/`cdp-errors.mjs` pattern. Caveat: page modules destructure `invoke` at load, so patching `window.__TAURI__.core.invoke` post-hoc can't intercept — probe with `event.defaultPrevented`, direct invokes, and real side effects (sidecar files) instead.
- **2026-07-18**: Milestone 6. `settings.rs` (Settings struct, serde-default backfill, panel_scale clamp 1.0–1.5, get/set_settings, `settings:changed` emit, settings window). Shared `ui/theme.js` applies `--font-scale` / `--panel-scale` + `data-theme`/`data-motion`/`data-contrast` to `<html>`, imported by every entry; effective motion/contrast = user toggle OR OS media query. overlay/editor/main/settings CSS got light + high-contrast (solid `--panel-bg`) + reduced-motion (`animation:none`) blocks. Panel scale is dual: `hover.rs` resizes overlay window by `dpi*panel_scale`, CSS also scales its fonts. `badges.rs` reads `settings.badges` (infotip suppression kept independent). Tray gained "Settings…". Verified against real CSS over the Vite dev-server webview (in-app Browser pane can't run JS on `file://` — serves stale static snapshots; http origin works): font XL×panel1.5 → 14→30.45px, light theme, HC `--panel-bg #000`+white border, reduced-motion animation-name none; settings window layout; app boots clean as background. 15/15 tests, clippy no new warnings. **Note**: 2 pre-existing clippy warnings in index.rs:206 + storage.rs:107 (untouched files) left per surgical rule.
- **2026-07-17**: Premise discussed. Market research done (Notezilla closest competitor; gap confirmed). Docs created: CLAUDE.md, FEASIBILITY, ARCHITECTURE, MVP. Added: performance budget, badge layer, accessibility settings, this memory file.
- **2026-07-17 (2)**: Git init + docs committed. Rust (GNU) installed. Milestone 0 spike built and passed: `spikes/hover-detect` — scan (51 icons, paths resolved incl. OneDrive/Public desktop), simtest 51/51 PASS with desktop visible, covered-window negative case verified. Findings folded into ARCHITECTURE.md.
- **2026-07-17 (7)**: Milestone 5. Main window (mainwin.rs) lists nuggets from index (list_nuggets), filter, Open/Edit per row, reloads on nuggets:changed. Tray (tray.rs): open/pause/autostart/quit. Shared Paused flag (appstate.rs) gates hover+badges. tauri-plugin-autostart. **Key gotcha discovered & documented**: WebviewWindowBuilder::build() DEADLOCKS on the async command thread AND inside run_on_main_thread; only works from a plain std::thread (like the hover engine) or the hotkey handler. edit_nugget spawns a std::thread. Verified Edit→editor via window enumeration + trace (GUI pixel-clicking was unreliable due to multi-window desktop; enumeration is the reliable probe).
- **2026-07-17 (6)**: Milestone 4. links.rs (open_in_explorer via ShellExecuteW /select, open_external for http). tauri-plugin-dialog for file/folder picker; editor 📄/📁 buttons insert nugget:// links. Panel + editor link-click handlers. Fixed the big UX blocker: native desktop infotips were covering the panel — now cleared via LVS_EX_INFOTIP on the desktop listview (re-applied each badge refresh). Panel todo checkboxes persist. E2E: link click opened Explorer (Cabinet 1→2), checkbox toggle wrote data-checked=true to sidecar.
- **2026-07-17 (5)**: Milestone 3. ui/ became npm+Vite package (TipTap deps). Editor window (dark, undecorated, drag-region titlebar, toolbar: bold/italic/bullets/todos/links), global hotkey Ctrl+Shift+N via tauri-plugin-global-shortcut (cursor target → UIA selection fallback), save_nugget command preserves created_ms + upserts index. E2E: SendKeys typed into real editor, saved, panel verified showing edit.
- **2026-07-17 (4)**: Milestone 2. Added rusqlite index (rebuildable cache, app-data DB), notify-based watcher (sidecar follows renames; deletes drop index entry but keep sidecar), preview extraction, 10 unit tests. WebView2 idle-release implemented; two traps: destroying the only window exits Tauri (prevent_exit needed), fresh pages miss emits (get_current_nugget pull on load). Cycle live-verified.
- **2026-07-17 (3)**: Milestone 1 built and verified. VS Build Tools installed, MSVC default. Tauri 2 app in `app/`: hover engine (debounced UIA), glass overlay panel (screenshot-verified show/hide with real transparency), native badge layer (orange dots, click-through). Hard-won transparency findings in ARCHITECTURE.md §2. WebView2 memory measured 379 MB warm → idle-release mandatory. Seeded demo nuggets on Works + Untitled-1.psd.
