# Project Memory — Handoff for Claude Sessions

> Purpose: any new Claude session reads this + CLAUDE.md and can continue without re-asking settled questions. **Update this file after every session where decisions are made.**

## Status

- **Phase**: Milestone 4 complete — file links E2E-verified (editor picker → nugget:// link → panel click opens Explorer). Also shipped: infotip suppression, panel todo-checkbox persistence, panel/editor web-link open.
- **Next step**: Milestone 5 — main window (all-nuggets list from the index), tray icon (open/pause/quit), autostart. Carry-over TODO: right-edge panel flip test.
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

## Owner preferences (from conversations)

- Wants app as light as possible — performance budget is a commitment, treat regressions as bugs.
- **Always update the relevant markdown files (docs/, CLAUDE.md, this file) immediately after any question/discussion that adds or changes functionality.**
- Discuss/clarify before building; owner answers scoping questions willingly.

## Session log

- **2026-07-17**: Premise discussed. Market research done (Notezilla closest competitor; gap confirmed). Docs created: CLAUDE.md, FEASIBILITY, ARCHITECTURE, MVP. Added: performance budget, badge layer, accessibility settings, this memory file.
- **2026-07-17 (2)**: Git init + docs committed. Rust (GNU) installed. Milestone 0 spike built and passed: `spikes/hover-detect` — scan (51 icons, paths resolved incl. OneDrive/Public desktop), simtest 51/51 PASS with desktop visible, covered-window negative case verified. Findings folded into ARCHITECTURE.md.
- **2026-07-17 (6)**: Milestone 4. links.rs (open_in_explorer via ShellExecuteW /select, open_external for http). tauri-plugin-dialog for file/folder picker; editor 📄/📁 buttons insert nugget:// links. Panel + editor link-click handlers. Fixed the big UX blocker: native desktop infotips were covering the panel — now cleared via LVS_EX_INFOTIP on the desktop listview (re-applied each badge refresh). Panel todo checkboxes persist. E2E: link click opened Explorer (Cabinet 1→2), checkbox toggle wrote data-checked=true to sidecar.
- **2026-07-17 (5)**: Milestone 3. ui/ became npm+Vite package (TipTap deps). Editor window (dark, undecorated, drag-region titlebar, toolbar: bold/italic/bullets/todos/links), global hotkey Ctrl+Shift+N via tauri-plugin-global-shortcut (cursor target → UIA selection fallback), save_nugget command preserves created_ms + upserts index. E2E: SendKeys typed into real editor, saved, panel verified showing edit.
- **2026-07-17 (4)**: Milestone 2. Added rusqlite index (rebuildable cache, app-data DB), notify-based watcher (sidecar follows renames; deletes drop index entry but keep sidecar), preview extraction, 10 unit tests. WebView2 idle-release implemented; two traps: destroying the only window exits Tauri (prevent_exit needed), fresh pages miss emits (get_current_nugget pull on load). Cycle live-verified.
- **2026-07-17 (3)**: Milestone 1 built and verified. VS Build Tools installed, MSVC default. Tauri 2 app in `app/`: hover engine (debounced UIA), glass overlay panel (screenshot-verified show/hide with real transparency), native badge layer (orange dots, click-through). Hard-won transparency findings in ARCHITECTURE.md §2. WebView2 memory measured 379 MB warm → idle-release mandatory. Seeded demo nuggets on Works + Untitled-1.psd.
