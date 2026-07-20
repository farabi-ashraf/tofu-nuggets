# Project Memory — Handoff for Claude Sessions

> Any new session reads this + CLAUDE.md (+ docs/GLOSSARY.md for the code map) and
> continues without re-asking settled questions. **Update after every session where
> decisions are made.** Detail older than the current line lives in git history.

## Status (2026-07-20)

- **Current release: 0.1.3** — shipped through the CI pipeline, updater live-verified.
  Whole 0.1.x work order complete (`docs/V0.1.3.md`): A1–A4 fixes, B1 updater+CI,
  B3 delete-all + uninstall messaging, B4 publish gate. B2 (one branch, CI matrix) is a
  standing policy.
- **Repo PUBLIC** at `https://github.com/farabi-ashraf/tofu-nuggets` after security gate:
  history rewritten with `git filter-repo` (purged FEASIBILITY/GIT-GUIDE docs, `.claude`
  local files, username leak; emails → `farabigithub@gmail.com`) + force-push — **any
  older clone must re-clone, never pull**. Protections on: secret scanning + push
  protection, Dependabot, least-priv Actions, ruleset `protect-main` (**PR required for
  main** — all work on `wip-*` branches; owner merges).
- **Updater keys**: private key + password ONLY in GitHub Actions secrets
  (`TAURI_SIGNING_PRIVATE_KEY`, `..._PASSWORD`) + owner's password manager. Never on disk
  here. Losing them = can never sign updates again (would need new keypair + re-ship).
- **Docs overhauled (2026-07-20)**: V0.1.1→`docs/V0.1.3.md` (release record + policies),
  new `docs/GLOSSARY.md` (code map — **mandatory to update** as codebase changes), new
  conventions in CLAUDE.md: code is source of truth (module `//!` headers current in the
  same change; docs only for cross-cutting).
- **Pending verification (hardware-bound)**: fresh-VM install, Win 10, multi-monitor,
  DPI≠100%, autostart-after-reboot. Cosmetic: title-bar theme sync.

## Next step — CONFIRMED plan (owner, 2026-07-20): Route 2 → stable → Route 1; Route 3 deferred

**Route 2 SHIPPED (2026-07-21)**: PR #8 merged, owner tested drag-drop on desktop —
works. Declared stable → **0.2.0 release in progress** (bump PR `wip-bump-0.2.0`;
minor bump because drag-drop is a feature). After 0.2.0 ships: begin Route 1.

Implementation record: dropping files/folders
onto the open editor inserts `nugget://` links — same `insertPathLink` pipeline as the
📄/📁 picker buttons. Webview-side only (`editor.js` via Tauri `onDragDropEvent` — API
identical on macOS, no platform code; HTML5 drop never fires because Tauri intercepts).
Accent-ring drop cue in `editor.css`. No new commands/events/permissions; no Rust
change. After owner confirms stable → declare stable version → begin Route 1
(macOS 14–26, Apple silicon; B2: extract `DesktopIcons` trait at port start, never
per-platform branches).

**macOS distribution decision**: owner will NOT pay for an Apple Developer account yet.
Beta distribution = unsigned GitHub Releases downloads: Tauri applies free ad-hoc
signing automatically (mandatory on Apple silicon), but no notarization → Gatekeeper
blocks first launch; testers need the documented bypass (System Settings → Privacy &
Security → "Open Anyway"; on macOS 15+ the old right-click→Open bypass is gone).
Acceptable for invited beta testers with instructions in the README/release notes.
Revisit the $99/yr account (real signing + notarization) before any public macOS launch.
The in-app updater's own signature (minisign keypair) is independent of Apple signing
and keeps working.

Original route analysis (for context):

1. **Route 1 — macOS port** (mac 14–26, Apple-silicon testers waiting): heaviest.
   `DesktopIcons` trait extraction per B2, AX-API hover, Finder specifics, overlay/badge
   port, Apple signing + notarization ($99/yr dev account), CI matrix.
2. **Route 2 — drag-drop file/folder links onto open editor** (recommended first):
   SMALL, not heavy — Tauri webview drag-drop event → insert `nugget://` links.
   Webview-side code carries to macOS for free. Fast capture win.
3. **Route 3 — Explorer-window hover integration**: the actually-heavy one (UIA over
   every Explorer window incl. Win11 tabs, per-window infotip suppression, positioning,
   perf budget) and Windows-only while mac testers wait. Defer; revisit post-macOS.

Known watcher gaps (from owner Q&A, candidates if testers hit them — see
GLOSSARY "Known behavior gaps"): watcher doesn't emit `nuggets:changed` (stale open
list on rename); rename-while-app-closed orphans sidecar (relinks if renamed back);
move-off-desktop-and-back relinks hover/badge instantly but list only after restart.

## Settled decisions (do not re-ask)

| Decision | Choice |
|---|---|
| Hover scope | Desktop icons; Explorer windows deferred (Route 3 discussion pending) |
| Stack | Tauri 2: Rust core + webview UI (TipTap editor) |
| Storage | Sidecar JSON in hidden `.nuggets` = source of truth; SQLite index = rebuildable cache |
| Hover detection | UIA `ElementFromPoint`, not ListView memory reads |
| Note capture | Global hotkey (shell context menu deferred — needs shell extension) |
| Visual cue | Badge layer: click-through layered window, per-dot occlusion (reparent ruled out) |
| Performance | Hard budget: ~0% CPU idle, ~15–20 MB core RAM, icon count never affects hover cost |
| Accessibility | Font size S–XL, panel scale, dark/light/system, Reduced Motion + High Contrast |
| Settings | `settings.json` app-data; serde-default backfill; live via `settings:changed` |
| Pricing | Free MVP, freemium later (owner's market research, 2026-07-17) |
| Hosting | GitHub, single public repo, `main` + `wip-*` branches, PR-only merges |
| Releases | Version-bump PR → tag `v*` → CI signed draft → owner publishes (docs/V0.1.3.md) |
| Platform strategy | B2: never per-platform branches; `#[cfg]`/traits + CI matrix |
| Docs strategy | Code = source of truth (`//!` headers); GLOSSARY.md mandatory-current |

## Dev environment (this machine)

- Repo at `F:\Claude\tofu-nuggets` (E: drive died 2026-07-20; add `safe.directory` for F: if git complains).
- Rust stable-msvc via rustup; VS Build Tools 2022 + Win11 SDK. `windows` 0.62 (old GNU spike pins 0.58 — leave it).
- Node v24 + npm. **`npm run build` in `app/ui` BEFORE `cargo build`** (assets embed from `ui/dist`).
- Transparency stack is subtle — read ARCHITECTURE.md §2 before touching overlay code (`webview2-com` + aliased `windows-core` 0.61 is load-bearing).
- Owner's desktop is OneDrive-redirected; icons split across OneDrive Desktop + Public Desktop. Owner has real nuggets (not demo data).

## Owner preferences

- App as light as possible — budget is a commitment; regressions are bugs.
- **Update relevant docs + this file immediately after any decision/functionality change.**
- Discuss/clarify before building; answers scoping questions willingly.
- Owner is new to GitHub web UI — give click-by-click paths for web-UI steps; Claude handles CLI.

## Hard-won lessons (keep; cost real debugging time)

- `WebviewWindowBuilder::build()` deadlocks on async command threads AND in `run_on_main_thread` — only plain `std::thread` workers (tray/single-instance/commands all marshal).
- Kill the installed running instance before runtime tests — single-instance plugin silently hands off, tests hit the wrong build.
- CDP E2E technique: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` + `Runtime.evaluate` (awaitPromise) drives the real app; page modules destructure `invoke` at load (post-hoc patching fails — probe via real side effects); path args must go through `JSON.stringify` (hand-escaped backslashes collapse).
- PowerShell is DPI-virtualized (logical px = physical/1.25); `FindWindowW` from PS Add-Type flaky → EnumWindows or screenshots for window-state truth; hidden consoles can hold foreground; spawn probes `-WindowStyle Hidden`.
- Hover E2E recipe: cursor ≥400 ms on icon after ≥250 ms outside.
- `REDIRECT_ROOT` is process-global → redirect unit tests serialize via a Mutex.
- "Tray alive, all windows dead" = missing WebView2 Runtime signature.
- Don't close a bug by testing the wrong symptom (A4: verified editor *open* when the bug was *save*).
- TipTap Link strips non-allowlisted protocols on re-parse — `protocols: ["nugget"]` + `isAllowedUri` required or hrefs silently die.
- Tauri capabilities: `core:default` is read-only; window `close()`/drag need explicit allows; missing permission = silent promise rejection.
- Owner `git pull` on fresh-checkout main can fail (no upstream) — Claude does tag/release CLI steps.
