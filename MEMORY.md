# Project Memory — Handoff for Claude Sessions

> Read this + CLAUDE.md (+ `docs/GLOSSARY.md` for the code map) and continue without
> re-asking settled questions. **Update after every session where decisions are made.**
> Keep it short: *why* a thing was decided lives in the release doc for that version
> (`docs/V0.1.3.md`, `docs/V0.4.0.md`), *how the code works* in module `//!` headers and
> `docs/ARCHITECTURE.md`, and blow-by-blow history in git.

## Status (2026-07-28)

- **Shipped: 0.4.0** — published and live on both platforms (Windows `.exe` + arm64
  `.dmg`), updater serving it. Contains the full file-manager update (notes inside File
  Explorer + Finder, not just the desktop), macOS Finder-tag badges, shared badge colour.
  Work order + rationale: **`docs/V0.4.0.md`**.
- **0.4.1 in flight (macOS keep-alive hotfix)** — 0.4.0 shipped still exiting when the last
  visible window closes on macOS (main window OR overlay-loss). Real fix = `LSUIElement=true`
  in the bundle Info.plist (`app/src-tauri/Info.plist`); see M4c below. Branch
  `wip-macos-keepalive-lsuielement`, awaiting owner PR → tag `v0.4.1`.
- All **E + M phases done and merged** (E0–E3, M3, M4a, M4b, M5). macOS Finder work was
  hardware-verified on this Mini. Tags in git: `v0.4.0` on `9506825`.
- Next macOS scope is a fresh decision (see "Parked features" / MVP non-goals); no
  in-flight release.

## 0.4.0 phase record (all DONE, shipped)

| Phase | What | State |
|---|---|---|
| W1, W2, E0–E3 | Taskbar fix, small UX debts, Explorer spike + hover/hotkey + pill + dots | **DONE**, PRs #31–#39, all owner-verified on this machine |
| **M3** | objc2 override of `applicationShouldTerminateAfterLastWindowClosed` + remove panel parking | **DONE — PR #40 merged.** ⚠️ The delegate override was NOT the real fix — see M4a. It masked the bug only because the always-visible badge window meant AppKit never saw zero visible windows. |
| **M4a** | Finder-tag engine (`tags.rs`); delete `badges_mac.rs` + `badges.{html,js,css}` + `badges:update` + dead CG helpers + **fix the keep-alive M3 only appeared to fix** | **Mini-verified 2026-07-27 (local build, this machine).** Deleting the badge window exposed that M3's delegate override does NOT keep the app alive: reproduced that closing the last visible window logs `applicationShouldTerminateAfterLastWindowClosed -> false` and the process still dies ~1s later. Root cause = AppKit reclaiming a window-less Accessory app via the process-lifetime subsystem, which never consults that selector (tao doesn't even implement it). **Fix (SUPERSEDED by M4c — did NOT fix the bundle): `lifecycle_mac::install` also calls `NSProcessInfo disableSuddenTermination` + `disableAutomaticTermination:`.** ⚠️ The "all green" matrix was run against the bare binary (immune for free); the shipped .app still died. Real fix is `LSUIElement` (M4c). Tag engine = write-hygiene xattr of `_kMDItemUserTags`; pure transforms unit-tested on Windows; colour via `tag_color()` (orange) awaiting M4b. |
| **M4b** | Shared `badge_color` setting (both OSes) + one-time first-tag notice | **macOS-verified 2026-07-27 (local build); Windows pending owner.** Branch `wip-m4b-badge-color`. One shared `badge_color` (7 names, default orange) in `settings.rs`; `badge_color_code` (mac tag code, unit-tested) + `badge_rgb` (Win dot). `tags.rs` sources the code from the setting; `set_settings` resyncs every tag on colour change (mac) and pokes `badges::wake()`/`pill::wake()` (Win). First-tag notice = one-time `Info` dialog gated by `first-tag-notice-shown` marker. Swatch picker in Settings (ring+checkmark selection, `forced-color-adjust:none`, high-contrast uses system highlight). **Mini-verified:** pick colour → every tag switches, exactly one `Nugget`, foreign tags untouched; notice fires once then never again. Windows checklist (dots + pill repaint, HC/XL) still owner-to-verify. |
| **M5** | Hover + hotkey inside Finder windows; fix false triggers in icon view; Finder tabs | **DONE — merged (#45) + shipped in 0.4.0; hardware-verified on the Mini.** All in `desktop_mac.rs`. Perf gate `finder_frontmost` (`AXFocusedApplication` pid vs Finder); measured 0.0% CPU with a non-Finder app front. Routing desktop-vs-window by **`AXWindow` in hit chain**, not size — icon-view false-trigger fix (verified on a maximized window). Item path = **item `AXURL`** (CFURL-resolved); window `AXDocument` is EMPTY on folder windows (hardware finding). Only the active tab is in the AX tree → tabs resolve the front tab. Hotkey: under-cursor → `AXSelectedChildren` → desktop selection fallback. Full checklist passed live (4 views, maximized window, 2-tab window, selection fallback, hotkey-create writes sidecar+tag, desktop unregressed). ⚠️ Post-mortem: PR #44 first merged the wrong (`AXDocument`) build — a bad `git add` pathspec silently unstaged the rewrite so `--amend` kept old content; **#45 corrected it**. Guardrail: verify `git diff --cached --stat` before committing. |
| — | Version bump 0.4.0 → tag → CI draft → owner publishes | **DONE — #46 bumped, `v0.4.0` tagged, draft built, owner published. Live.** |
| **M4c** | **Real macOS keep-alive: `LSUIElement=true` in bundle Info.plist** (`app/src-tauri/Info.plist`, merged by Tauri) — hotfix in **0.4.1** | **Mini-verified 2026-07-28.** ⚠️ M4a's "fix" was WRONG: `NSProcessInfo disableSudden/AutomaticTermination` does NOT keep the shipped bundle alive; v0.4.0 still died when the last visible window closed (main OR overlay-loss). **Trap:** M4a was verified against the bare `target/*/tofu-nuggets` binary, which has no Info.plist so LaunchServices never lifetime-manages it and it survives windowless for free — the real .app is managed and gets killed. Runtime `setActivationPolicy(Accessory)` is NOT a substitute for the launch-time declaration. A/B proof: two identical bundles — no-LSUIElement dies on main-close (`exiting` in tofu.log), LSUIElement survives. Delegate override (M3) + NSProcessInfo (M4a) kept as belt-and-suspenders only. **ALWAYS verify keep-alive against the BUNDLE, never the bare binary.** |

Rules that produced this order (do not re-litigate):

- **macOS behavior is now verified locally on the Mac Mini via Claude** — build from
  source (`npm run build` in `app/ui` → `cargo build`/`cargo run` in `app/src-tauri`) and
  run the app right here. Sideloading a CI `.dmg` is no longer the verification path; it
  remains only for owner acceptance / release-signing checks. Claude drives the build,
  runs it, and reads `tofu.log` (and tao's stderr) directly. Windows work no longer has to
  come first to avoid burning Mini cycles — a Mini cycle is now a local build.
- ~~M3 must be Mini-verified BEFORE M4a starts~~ — moot: verification is now local
  (above), and M4a is where the keep-alive was actually diagnosed and fixed.
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
- **Behavior testing = local build+run on the Mac Mini (macOS 26), driven by Claude.**
  Build from source and run in place; no CI `.dmg` sideload needed to verify behavior.
  Hardware covers macOS 26 only; 14/15 = CI compile + beta testers later. Never claim
  "should work" for macOS code that hasn't actually been run here.
- **Accessibility grant**: a locally-run debug build must be granted Accessibility once
  (System Settings → Privacy & Security → Accessibility). A rebuilt binary at the same
  path generally keeps its grant; a CI `.dmg` re-prompts because ad-hoc signing keys the
  grant to the signature.
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

- AppKit reclaims a window-less **Accessory** app, and that path skips `ExitRequested`
  entirely — `prevent_exit`, hide-on-close and `Accessory` policy do not stop it. The
  **real** keep-alive is `NSProcessInfo disableSuddenTermination` +
  `disableAutomaticTermination:` (`lifecycle_mac.rs`), Mini-verified 2026-07-27. The
  delegate override of `applicationShouldTerminateAfterLastWindowClosed:` (M3) is NOT
  sufficient and never was — it returns NO and the process still dies on last-window
  close; it only *looked* fixed while the always-visible badge window kept a visible
  window around. tao doesn't implement that selector, so overriding it just re-asserts
  AppKit's default. Parking the panel off-screen was the older workaround, removed in M4a.
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
- **(M5) Desktop vs Finder browser window = `AXWindow` in the hit chain, NOT window
  size.** The desktop has no `AXWindow`; a browser window is one. The old `covers_a_display`
  test on the hit chain read a *maximized* icon-view window as the desktop and fired the
  panel over the empty space between icons — the M5 false-trigger bug. Route by role; keep
  size only for locating the desktop's own enumeration container.
- **(M5) A Finder-window item's path = its own `AXURL`, NOT the window's folder.** The
  window's `AXDocument` is EMPTY on Finder folder windows (macOS 26 — verified live; the
  a-priori "read AXDocument for the folder" plan was wrong). Each item's `AXURL` is a
  file-*reference* URL (`file:///.file/id=…`) resolved with `CFURLCreateFilePathURL` +
  `CFURLCopyFileSystemPath`; it names the exact file (no hidden-extension matching) and
  belongs to the active tab. The URL sits on the hit element in icon/column views but on a
  cell's child text field in list/details, so `finder_item` climbs from the hit through
  item-level elements, stopping at content containers (`is_search_barrier`:
  `AXList`/`AXTable`/`AXOutline`/`AXBrowser`/`AXScrollArea`/`AXSplitGroup`) so empty space
  resolves to nothing. **Finder keeps only the active tab in the AX tree**, so a multi-tab
  window needs no cursor-in-view disambiguation — the opposite of Explorer's live per-tab
  HWNDs. (`AXSplitGroup` had to be a barrier: a hit on the window's non-content margin
  lands directly on it.)
- **(M5) Perf gate = frontmost app is Finder** (`AXFocusedApplication` pid vs Finder's),
  mirroring Windows' `foreground_surface`. Consequence to state to the owner: macOS desktop
  hover now only fires while the desktop is actually foreground (Finder frontmost, no key
  window) — same posture as Windows needing `Progman` foreground, and the reason idle CPU is
  ~0 when another app is in front.
- Idle release (destroy/recreate the overlay) is Windows-only — it exists for WebView2's
  process tree, which WKWebView does not have.
