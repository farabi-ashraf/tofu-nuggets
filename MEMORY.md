# Project Memory — Handoff for Claude Sessions

> Read this + CLAUDE.md (+ `docs/GLOSSARY.md` for the code map) and continue without
> re-asking settled questions. **Update after every session where decisions are made.**
> Keep it short: *why* a thing was decided lives in the release doc for that version
> (`docs/V0.1.3.md`, `docs/V0.4.0.md`), *how the code works* in module `//!` headers and
> `docs/ARCHITECTURE.md`, and blow-by-blow history in git.

## Status (2026-07-27)

- **Shipped: 0.3.0** — two platforms (Windows `.exe` + arm64 `.dmg`), updater live on
  both, published.
- **In progress: 0.4.0** — badge-layer bug fixes + the file-manager update (works
  inside File Explorer / Finder, not just the desktop) + small UX debts. Full work
  order, decisions and rejected alternatives: **`docs/V0.4.0.md`**.
- Windows side is **complete and owner-verified**; macOS side is next.

## Next step — remaining 0.4.0 phases

| Phase | What | State |
|---|---|---|
| W1, W2, E0–E3 | Taskbar fix, small UX debts, Explorer spike + hover/hotkey + pill + dots | **DONE**, PRs #31–#39, all owner-verified on this machine |
| **M3** | objc2 override of `applicationShouldTerminateAfterLastWindowClosed` + remove panel parking | **DONE — PR #40 merged, Mini-verified 2026-07-27** (survives new/edit-note-then-close, no stray after-sleep window, hover/Quit/updater fine). **Caveat**: the override is *installed but never yet consulted* — Mini log never showed the `-> false` line because the always-visible badge window means AppKit never sees zero visible windows (census `badges=true` at every close). See M4a. |
| **M4a** | Finder-tag engine (`tags.rs`); delete `badges_mac.rs` + `badges.{html,js,css}` + `badges:update` + dead CG helpers | **Code complete, PR open, NOT Mini-verified. This is the REAL test of M3**: the always-visible badge window is now GONE, so the delegate override is the sole keep-alive. Mini MUST show `applicationShouldTerminateAfterLastWindowClosed -> false` + census after a new-note-then-close, and the app must survive; if it dies, M3 needs revisiting (delegate fetched too early / not consulted) before this can merge. Tag engine = write-hygiene xattr of `_kMDItemUserTags`; pure transforms unit-tested on Windows; colour via `tag_color()` (orange) awaiting M4b. |
| **M4b** | Shared `badge_color` setting (both OSes) + one-time first-tag notice | after M4a |
| **M5** | Hover + hotkey inside Finder windows; fix false triggers in icon view; Finder tabs | last before release |
| — | Version bump 0.4.0 → tag → CI draft → owner publishes | |

Rules that produced this order (do not re-litigate):

- **Windows work first, all of it.** macOS behavior can only be tested by the owner
  sideloading a CI `.dmg` onto the Mac Mini, so Windows-testable work never burns a
  Mini cycle.
- **M3 must be Mini-verified BEFORE M4a starts** — M4a deletes the always-visible badge
  window, which is plausibly what keeps the app alive today.
- **M4 is one milestone in two PRs** (Opus 4.8-Low lands bounded diffs better); one Mini
  test round can cover both.
- **One release covering E + M3 + M4 + M5** — the platform-parity gate is now a standing
  policy (`docs/V0.1.3.md`). Accepted cost: macOS 0.3.0 bugs stay in the wild until then.

## Open bugs / parked

- Deleted note reappears after reinstall — needs a repro (does the sidecar survive in
  `~/Desktop/.nuggets/`?).
- Mounted volumes on the macOS desktop are not annotatable (`/Volumes/<name>` is not a
  desktop root) — deliberate no-fix; adding `/Volumes` would pull every disk into the scan.
- Off-desktop rename/delete while running shows only after the next index rebuild
  (watcher is desktop-only by design — `docs/V0.4.0.md` D5).
- Other known behavior gaps: `docs/GLOSSARY.md` "Known behavior gaps" (kept there, not here).
- **Parked features**: last-edit date display (cheap — `modified_ms` already stored),
  edit history (schema change), telemetry (privacy + endpoint decision needed).

## Settled decisions (do not re-ask)

| Decision | Choice |
|---|---|
| Hover scope | Desktop icons + File Explorer windows (0.4.0) + Finder windows (0.4.0 M5). Sync/shell extensions still out |
| Stack | Tauri 2: Rust core + webview UI (TipTap editor) |
| Storage | Sidecar JSON in hidden `.nuggets` = source of truth; SQLite index = rebuildable cache |
| Index scope | Desktop dirs + `known_roots.json` (parent folder of every saved note); watcher stays desktop-only |
| Hover detection | UIA `ElementFromPoint` (Windows) / AX hit-test (macOS) — never ListView memory reads |
| Note capture | Global hotkey (shell context menu deferred — needs a shell extension) |
| Visual cue — desktop | Windows: GDI click-through layered window, per-dot occlusion. macOS: Finder tags (0.4.0) |
| Visual cue — file manager | Windows: per-window pill + one-shot dot snapshot, never live-tracked. macOS: nothing to build, Finder draws the tags |
| Badge color | One shared 7-color `badge_color` setting, default orange: Windows paints its dot, macOS maps it to the tag color |
| Performance | Hard budget: ~0% CPU idle, ~15–20 MB core RAM, icon count never affects hover cost |
| Accessibility | Font size S–XL, panel scale, dark/light/system, Reduced Motion + High Contrast |
| Settings | `settings.json` in app-data; serde-default backfill; live via `settings:changed` |
| Pricing | Free MVP, freemium later |
| Hosting | GitHub, single public repo, `main` + `wip-*` branches, PR-only merges, owner merges |
| Releases | Version-bump PR → tag `v*` → CI signed draft → owner publishes (`docs/V0.1.3.md`) |
| Platform parity | No release until both OSes reach the same usability bar; different mechanisms per OS are fine |
| Release notes | Binding from 0.4.0: every release body lists each new feature with a one-line description (`updater.rs` shows the body verbatim) |
| Platform strategy | B2: never per-platform branches; `#[cfg]`/traits + CI matrix |
| Docs strategy | Code = source of truth (`//!` headers); GLOSSARY.md mandatory-current |

## macOS: how testing and distribution work

- **CI is a compile/test gate only** (both legs on every PR). It cannot test behavior —
  the Accessibility grant needs a GUI, and hover/badges need eyes.
- **Behavior testing = owner's M4 Mac Mini, macOS 26**, sideloading the CI `.dmg`.
  Hardware covers macOS 26 only; 14/15 = CI compile + beta testers later.
- **Every new build re-prompts the Accessibility grant** (ad-hoc signing keys it to the
  signature) — so batch changes into few Mini runs, and never claim "should work" for
  unverified macOS code.
- **No Apple Developer account** (owner decision): ad-hoc signing only, no notarization,
  so first launch needs System Settings → Privacy & Security → "Open Anyway" (the old
  right-click→Open bypass is gone on 15+). The updater's own minisign signature is
  independent of Apple signing. Revisit the $99/yr account before any public macOS launch.
- Log: `~/Library/Application Support/Tofu Nuggets/tofu.log` (the identifier dir looked
  like an app bundle to Finder — see `paths.rs`).
- **Updater keys**: private key + password ONLY in GitHub Actions secrets
  (`TAURI_SIGNING_PRIVATE_KEY`, `..._PASSWORD`) + owner's password manager, never on
  disk here. Losing them = no signed updates ever again.
- Repo history was rewritten once (secret purge, 2026-07-20) — **any clone older than
  that must re-clone, never pull.**

## Dev environment (this machine)

- Repo at `F:\Claude\tofu-nuggets` (add `safe.directory` for F: if git complains).
- Rust stable-msvc via rustup; VS Build Tools 2022 + Win11 SDK; `windows` 0.62 (the old
  GNU spike pins 0.58 — leave it).
- Node v24 + npm. **`npm run build` in `app/ui` BEFORE `cargo build`** (assets embed
  from `ui/dist`).
- Transparency stack is subtle — read ARCHITECTURE.md §2 before touching overlay code
  (`webview2-com` + aliased `windows-core` 0.61 is load-bearing).
- Owner's desktop is OneDrive-redirected; icons split across OneDrive Desktop + Public
  Desktop. Owner has real nuggets on it, not demo data.

## Owner preferences

- App as light as possible — the budget is a commitment; regressions are bugs.
- **Update the relevant docs + this file immediately after any decision or functionality
  change.**
- Discuss and clarify before building; scoping questions get answered.
- New to the GitHub web UI — give click-by-click paths for web steps; Claude does CLI.

## Hard-won lessons (each cost real debugging time)

**Tauri / cross-platform**

- `WebviewWindowBuilder::build()` deadlocks on async command threads AND inside
  `run_on_main_thread` — plain `std::thread` workers only.
- Tauri capabilities: `core:default` is read-only; window `close()`/drag need explicit
  allows; a missing permission surfaces only as a silent promise rejection.
- TipTap Link strips non-allowlisted protocols on re-parse — `protocols: ["nugget"]` +
  `isAllowedUri` or `nugget://` hrefs silently die.
- Kill the installed running instance before runtime tests — the single-instance plugin
  hands off silently and the test hits the wrong build.
- Don't close a bug by testing the wrong symptom (verified the editor *opened* when the
  bug was *save*).

**Windows**

- Modern Explorer content is a `DirectUIHWND`: list items are not real child windows, so
  `LVS_EX_INFOTIP` suppression and wheel-scroll window events both do nothing there.
- An owned popup sits above its owner only until the owner is re-activated — re-insert it
  just above the owner on every render, never `HWND_TOP`. Automation repros mask this by
  calling `SetForegroundWindow`.
- The shell COM chain (`IShellWindows`/`IShellBrowser`) is **STA-only** — MTA threads get
  `0x8001010D` and resolve nothing.
- Isolate before claiming a perf number: whole-process CPU is dominated by the badge
  layer's UIA walk, so the same build reads 0 ms or 250 ms per 25 s depending only on
  whether the desktop is covered.
- CDP E2E: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` +
  `Runtime.evaluate` (awaitPromise) drives the real app; page modules destructure
  `invoke` at load (patching post-hoc fails — probe via real side effects); path args
  must go through `JSON.stringify`.
- PowerShell is DPI-virtualized (logical px = physical/1.25); `FindWindowW` via Add-Type
  is flaky → use EnumWindows or screenshots; hidden consoles can hold foreground.
- Hover E2E recipe: cursor ≥400 ms on the icon after ≥250 ms outside.
- `REDIRECT_ROOT` is process-global → redirect unit tests serialize on a Mutex.
- "Tray alive, all windows dead" = missing WebView2 Runtime.

**macOS**

- AppKit terminates the app when **no window is VISIBLE**, and that path skips
  `ExitRequested` entirely — `prevent_exit`, hide-on-close and `Accessory` policy do not
  stop it. The delegate override (M3) is the real fix; parking the panel off-screen was
  the workaround being removed.
- All AppKit window calls must happen on the main thread (`run_on_main_thread`); calling
  `show`/`hide`/`set_position` from a worker is legal on Win32 and fatal here.
- Keep everything in **points**, end to end. The `CGDisplayPixelsWide / CGDisplayBounds`
  ratio is NOT the backing scale on scaled resolutions — never reintroduce that conversion.
- No `window.prompt` / `alert` / `confirm` — WKWebView does not implement them; they fail
  silently. Use in-page UI or the dialog plugin.
- Tauri does not sign the bundle unless told: `bundle.macOS.signingIdentity: "-"` plus a
  CI `codesign --verify --strict` step, or Apple silicon reports "app is damaged". Never
  move an extracted `.app` between machines — transfer the `.dmg`.
- `bundle.targets` must list every platform's targets (`["nsis", "app", "dmg"]`); a
  Windows-only list makes the macOS release leg compile and bundle nothing.
- Finder's desktop AX shape has no `AXWindow`: app → display-sized `AXScrollArea` →
  `AXGroup` → items. Walk down through display-sized containers; never hard-code depth
  (details in the `desktop_mac.rs` header).
- Idle release (destroy/recreate the overlay) is Windows-only — it exists for WebView2's
  process tree, which WKWebView does not have.
