# Project Memory — Handoff for Claude Sessions

> Purpose: any new Claude session reads this + CLAUDE.md and can continue without re-asking settled questions. **Update this file after every session where decisions are made.**

## Status

- **Phase**: Milestone 1 complete — overlay panel + badge layer working and verified via screenshots on Win 11.
- **Next step**: Milestone 2 — sidecar storage hardening: rename/move tracking (`ReadDirectoryChangesW`), SQLite cache index, unit tests. Also carry-over TODOs: WebView2 idle-release (mandatory, 379 MB warm), right-edge panel flip test, Explorer infotip suppression.
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
- **2026-07-17 (3)**: Milestone 1 built and verified. VS Build Tools installed, MSVC default. Tauri 2 app in `app/`: hover engine (debounced UIA), glass overlay panel (screenshot-verified show/hide with real transparency), native badge layer (orange dots, click-through). Hard-won transparency findings in ARCHITECTURE.md §2. WebView2 memory measured 379 MB warm → idle-release mandatory. Seeded demo nuggets on Works + Untitled-1.psd.
