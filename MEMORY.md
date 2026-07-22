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

**Route 2 SHIPPED, v0.2.0 published (2026-07-21)** — updater live-verified by owner.

**Route 1 STARTED (2026-07-21)**, first PR `wip-route1-scaffold`:
- `DesktopIcons` trait extracted to `icons.rs` (portable `Icon`/`IconRect`); Windows
  impl = `desktop.rs` (UIA), macOS = `desktop_mac.rs` stub (hover/badges inert,
  `cursor_pos` → None; real `~/Desktop` root so storage/editor/list work). Hover
  engine + editor now platform-agnostic (no `windows::` imports).
- Windows deps moved to `[target.'cfg(windows)'.dependencies]`; badges cfg-gated
  (inline no-op stub module in main.rs); links.rs has Finder impl (`open` / `open -R`);
  `hide_dir` no-op on mac (dot prefix); `webview_missing_alert` cfg(windows).
- New `.github/workflows/ci.yml`: PR/push → fmt+clippy(-D warnings)+test matrix on
  windows-latest + macos-latest. **macOS compile status verified by this CI**, not
  locally.
- README rewritten (owner request): v0.2.0, per-function usage guide, macOS-port note.
- Scaffold PR #11 merged 2026-07-21.
- **AX hover PR** (`wip-mac-ax-hover`): `desktop_mac.rs` real `icon_at` via
  system-wide `AXUIElementCopyElementAtPosition`; desktop-icon test = AXImage in
  AXScrollArea whose window is display-sized (heuristic — verify vs Finder icon-view
  windows on Mini); hand-declared FFI (no bindings crates); points↔px via per-display
  backing scale; Accessibility prompt via `AXIsProcessTrustedWithOptions` (grant may
  need restart). `resolve_path` moved to `icons.rs` (shared). `selected_icon` +
  `list_icons` still stubs. **Untested on hardware — CI compile only.**
- AX hover PR #12 merged 2026-07-21 (hardware-untested).
- dmg artifact PR #13 merged 2026-07-21: CI macOS job builds ad-hoc-signed arm64
  `.dmg` (`npx @tauri-apps/cli build --bundles dmg`; beforeBuildCommand cleared in CI
  — the CLI resolves its relative path from a different cwd than tauri-action;
  updater artifacts off — signing key is release-only), uploads as workflow artifact
  (14-day retention, ~5.8 MB verified). `icon.png` added to bundle icons (bundler
  composes the `.icns`). README: per-platform build prereqs + macOS beta/Gatekeeper
  note. upload-artifact bumped v4→v5 (Node 20 deprecation warning).
- **macOS signing gotcha (2026-07-21, cost a test cycle)**: Tauri does NOT sign the
  bundle unless told to. The first artifact was unsigned → Apple silicon refused it
  with *"Tofu Nuggets.app is damaged and can't be opened"* (unsigned + quarantine
  reads as tampering; the "Open Anyway" path never appears). Fix: `bundle.macOS
  .signingIdentity: "-"` (ad-hoc) + `minimumSystemVersion: 14.0`, plus a CI
  `codesign --verify --strict` step so an unsigned bundle fails the build instead of
  the tester's Mac. Also: never move an extracted `.app` between machines (a non-Mac
  unzip strips the signature) — transfer the `.dmg`.
- `actions/upload-artifact`: v5 AND v4 declare `using: node20` upstream → deprecation
  warning regardless of our config; **v7.0.1 is node24** (v6 = Dec 2025, v7 = Feb 2026).
- **First Mini test run (owner, 2026-07-21, macOS 26)**: signed build installs and
  opens after "Open Anyway"; main window + settings work. **Hover and hotkey both
  dead — no note could be created, so hover itself is still UNTESTED.** Hotkey
  findings: many combinations could not be captured at all; ⌘ combinations rejected
  or already owned (⌘⇧N = Finder New Folder); Option+Z stored as `super+z` and shown
  as "Win+Z"; newly set hotkeys never fired.
- **Diagnosis + fix PR** (`wip-mac-hotkey-ux`): (1) capture read `event.key`, but
  macOS composes Option+letter into a character ("Ω") so most combinations were
  silently rejected → now `event.code`; (2) modifier labels were Windows-only
  (`super`→"Win") → per-platform labels ⌘⌥⌃⇧ in new shared `app/ui/hotkeys.js`;
  (3) **the real blocker: with the Accessibility grant missing, every AX call fails,
  so hover finds nothing AND the hotkey's `icon_at` finds nothing — and macOS
  `selected_icon` is still a stub, so the fallback path is dead too. The app said
  nothing** → Settings now shows a permission section (status + "Open Accessibility
  settings"), and the hotkey shows a one-time dialog when the grant is missing.
- **Second Mini run (owner, 2026-07-21, after PR #16)**: permission granted and shown
  as granted; hotkey capture/labels now correct and register fine; **still no editor
  opens on hotkey press**, so hover remains untested. Rules out: permissions, hotkey
  capture, hotkey registration. Also rules out off-main-thread window creation
  (main window + settings open from worker threads on macOS just fine).
- **Remaining suspects, in order**: (1) the AX hit-test finds nothing — the first
  heuristic demanded `AXImage` inside `AXScrollArea` inside an exactly-display-sized
  window, and Finder on macOS 26 evidently does not present that; (2) the global
  shortcut handler never fires at all.
- **Diagnosis PR** (`wip-mac-ax-diag`): tolerant AX walk (ancestor chain up to 8
  levels, any `AXScrollArea` ancestor, window must cover ≥80% of a display, missing
  `AXWindow` accepted, name from AXTitle/AXFilename/AXDescription/AXValue on the item
  levels only) + **`debug_cursor_chain()` dumps roles/subroles/names/frames to
  tofu.log when targeting fails** + a one-time dialog when the hotkey fires but finds
  no icon. **The dialog is the discriminator: if it appears, the handler runs and the
  AX walk is wrong (log has the chain); if nothing appears at all, the shortcut never
  fires and the next suspect is tauri-plugin-global-shortcut on macOS.**
- macOS log path: `~/Library/Application Support/Tofu Nuggets/tofu.log` (renamed — see
  the data-dir bug below).

## macOS status after PR #21 (2026-07-21): app survives every tested case

Startup, hover cycle, edit-note, and new-note-then-close all confirmed stable on the
Mini. Follow-ups from that run:
- **Web-link entry was dead on macOS** — it used `window.prompt`, which WKWebView does
  not implement (WebView2 does, hence Windows worked). Replaced with an in-page link
  bar (`wip-mac-link-bar`); ⌘S/⌘K now work alongside Ctrl. **Never use
  prompt/alert/confirm in this UI.**
- **External SSD on the desktop is not annotatable** (logged as "virtual icon"):
  volumes live at `/Volumes/<name>` but name→path resolution only searches desktop
  roots. Owner filed as future reference, not fixed — adding `/Volumes` as a root
  would pull every mounted disk into the index scan.
- Still open from earlier: deleted note reappearing after reinstall (needs a repro:
  does the sidecar survive in `~/Desktop/.nuggets/`?).
- Link fix verified on BOTH platforms (2026-07-21), PR #22.

## Route 1 leftovers (in progress)

0. **Phantom "Desktop" icon (found 2026-07-21, same branch)**: pointing at bare
   wallpaper made `icon_at` return an icon named "Desktop" — the tolerant walk from
   PR #17 skipped `AXScrollArea` ancestors but accepted the desktop *window* above
   it, which has a name and frame. Logged as "'Desktop' has no filesystem path
   (virtual icon)". Worse than cosmetic: it counted as a hit, so the hotkey never
   reached the `selected_icon` fallback (which is why selection targeting appeared
   dead and no Finder-tree dump was ever written). Fix: `is_container()` rejects
   `AXScrollArea`/`AXWindow`/`AXApplication` roles **and anything display-sized**
   (icons never are), applied in both `icon_at` and `icon_from`.

0b. **Finder's real AX shape (macOS 26, from the hardware dump — no longer a guess)**:
   `AXApplication "Finder"` → `AXScrollArea "desktop"` (display-sized, **directly
   among the app's children, not inside an AXWindow**) → `AXGroup "Desktop"` (also
   display-sized) → the icon elements. First enumeration attempt stopped at the
   scroll area and enumerated its single AXGroup child ⇒ zero icons ("container
   found, 1 children" in the log). Now the walk descends through display-sized
   containers until it finds item-shaped children, so the depth is not hard-coded.

1. **Icon enumeration — DONE, PR #23 merged (2026-07-21, Mini-verified)**: macOS
   `list_icons` + `selected_icon` walk down from Finder's application element — pid
   from `CGWindowListCopyWindowInfo` (also the API the future badge occlusion pass
   needs), then `find_icon_container()` descends through display-sized containers
   (per the real shape in 0b) until item-shaped children appear. `selected_icon`
   asks the container then its parent via `selection_in()` helper. Owner confirmed:
   select icon → pointer on bare wallpaper → hotkey opens that icon's note.
   `debug_finder_tree()` diagnostic stays (prints container role/title + first
   three children) for future Finder-shape drift. Unblocks badges.
2. **macOS badge layer — PR `wip-mac-badges` (2026-07-22, hardware-UNtested)**:
   new `badges_mac.rs` — transparent click-through always-on-top **webview**
   window (`badges` label, `badges.html`) spanning the display bounding box;
   dots = positioned divs pushed via `badges:update` each 2 s tick (emit
   unconditional — covers page-load race; page skips unchanged payloads).
   Occlusion per-dot from `CGWindowListCopyWindowInfo` (new
   `desktop_mac::onscreen_window_rects` + `display_bounds_pts`; CG not AX, no
   permission needed; own pid + alpha-0 excluded, desktop elements excluded by
   flag). AX walk skipped while every display is covered. Window rules
   honored: built on plain std::thread, AppKit calls via run_on_main_thread,
   Logical/points only, never `hide()` — dots vanish by emptying the page.
   main.rs stub replaced with cfg split (`badges::spawn` win /
   `badges_mac::spawn(app, …)` mac). Mini checklist: dots appear on annotated
   icons, disappear under overlapping app windows / when paused / badges-off,
   click-through (dot doesn't eat desktop clicks), position correct on scaled
   resolution.

   **First Mini run (owner, 2026-07-22, PR #25 merged): two failures.**
   (a) **No dots ever drawn.** Diagnosis: `onscreen_window_rects` counted every
   on-screen window — macOS keeps screen-covering agent windows (Notification
   Center overlay etc.) on-screen at high `kCGWindowLayer` at all times, so the
   "all displays covered" short-circuit always fired. Fix in
   `wip-mac-badge-diag`: occluders = layer-0 windows only (menu bar/Dock no
   longer occlude — matches Windows). Plus tofu.log diagnostics: badge window
   create result + per-tick state summary (icons/annotated/occluders/dots or
   why empty), logged on change only.
   (b) **Exit regression: new-note-then-close kills the app again** when no
   main/settings window open — despite parked panel AND visible badge window
   (log: `editor hidden` → `exiting` 1 s later, no `exit requested`; same
   no-visible-window signature as PR #20/21). Same flow was verified stable
   after PR #21, so the badge window changed the equation somehow, or macOS
   doesn't count either window. Discriminator added: CloseRequested now logs a
   visible-window census (`label=is_visible` for every window). If census shows
   panel/badges visible=true at kill time ⇒ AppKit doesn't count them; next
   step is the documented fallback (override
   `applicationShouldTerminateAfterLastWindowClosed` via objc2). If false ⇒
   find who hid them.

   **Second Mini run (owner, 2026-07-22, PR #26 merged): badge layer VERIFIED.**
   Dots appear on annotated icons, clear on pause, hide behind Finder windows —
   the layer-0 occluder filter was the fix. **Exit regression did NOT reproduce**:
   new-note-then-close survived, census showed `overlay=true badges=true` after
   the editor hid — so the always-visible badge window plausibly cured it as a
   side effect (macOS may not have counted the *off-screen parked* panel; the
   badge window is on-screen). Watch: if exit ever returns, census lines are in
   place; objc2 delegate override remains the fallback. Startup log noise
   `badges: list_icons failed: desktop icon container not found` once, ~2 s
   after launch = Finder AX not ready yet; recovers next tick; harmless.
   Leftover #2 CLOSED. Owner decision: staying on Windows dev machine (no move
   to the Mac).
3. **Release workflow macOS entry — PR `wip-release-mac` (2026-07-22)**:
   release.yml now a fail-fast:false matrix (windows-latest + macos-latest);
   both legs attach to the same draft, tauri-action merges platform entries
   into one latest.json (updater gains darwin-aarch64). Release body carries
   per-OS install notes (Gatekeeper "Open Anyway" for mac). Version bumped to
   **0.3.0** (owner approved "go ahead with the release"). README: status +
   platform sections rewritten for two-platform beta. After merge: tag
   `v0.3.0` → CI builds draft → owner publishes.

## macOS hover: WORKS as of the third Mini run (2026-07-21)

Hotkey opens the editor, note saved, **hover panel appears over a desktop icon** —
the tolerant AX walk (PR #17) was the fix. Three new bugs found in that run, all
fixed in `wip-mac-hover-fixes`:

1. **Panel drawn far left of the icon.** `desktop_mac` converted points→physical px
   using `CGDisplayPixelsWide / CGDisplayBounds`; that ratio is NOT the window
   backing scale on displays running a *scaled* resolution (can be 1.5 while backing
   scale is 2.0). Fix: macOS keeps everything in POINTS end to end and the panel is
   placed with `LogicalPosition`/`LogicalSize`; Windows stays physical-px +
   `PhysicalPosition`. **Never reintroduce the conversion.**
**REAL ROOT CAUSE (sixth Mini run, log from PR #20 build)**: `prevent_close` + hide
did NOT stop it. Log shows `exiting` ~6 s after a launch where **no window was ever
opened or closed**, and ~1 s after the last window hid. So macOS terminates the app
whenever it has **no VISIBLE window** — hidden windows do not count, `Accessory`
policy does not change it, and the termination never raises `ExitRequested`.
Fix in `wip-mac-panel-park`: the panel is **parked off-screen (still ordered in)**
instead of hidden — `overlay::park`, used by startup, `hover::hide_panel`, the
panel's ✕ command, **and `editor::open_editor`** (that last one was missed in the
first pass: opening the editor hid the parked panel, so closing the editor left
nothing visible — which is exactly why a *new* note quit the app while editing an
existing one from the main window did not). **Rule: on macOS never call `hide()` on
the panel; park it.** Parking verified good for startup, hover cycle and edit-note. **If the app still exits, the next step is overriding
`applicationShouldTerminateAfterLastWindowClosed` on the NSApp delegate via objc2**
(more invasive; only if parking fails, e.g. if AppKit constrains the parked window
back on-screen). Confirmed working in that run: tray Quit, editor keyboard focus
under Accessory policy, all cosmetic wording.

**Earlier (fifth Mini run, log from PR #19 build)**: the log ends
`window 'main' destroyed` → `exiting` with **no `exit requested` line**, so
`RunEvent::Exit` arrives without `ExitRequested` — macOS terminates the app when its
last *visible* window closes, and that path never consults `prevent_exit` (the
Accessory policy did not change it). Fix in `wip-mac-window-close`: on macOS
`CloseRequested` → `api.prevent_close()` + `win.hide()`, so windows are only ever
hidden. Matches Mac convention (closing a window ≠ quitting) and windows are reused
on next open (`mainwin::show`/`editor::get_or_create` already show existing windows).
Same PR: tray label "Start with Windows" → "Open at Login" on macOS; row action
tooltip → "Reveal in Finder"; main-window footer wording per platform (moved into
`main.js`).

**Fourth Mini run (2026-07-21, after PR #18)**: placement fixed — panel now appears
beside the icon. **Process still dies after the panel hides, BUT only when no other
app window is open** (main list or settings window open ⇒ survives). That rules the
hover/AX code out as the direct cause and points at last-window teardown. Response
(`wip-mac-window-lifetime`): log `ExitRequested`(+code)/`Exit`/window
`Destroyed`/`CloseRequested` so a clean exit is distinguishable from a crash in
tofu.log, and set macOS `ActivationPolicy::Accessory` (correct for a menu-bar/tray
app anyway; Regular ties lifetime to windows and adds a Dock icon). **If the log
shows "exit requested" before death it is a graceful exit path; if it shows nothing,
it is a hard crash and the macOS crash report in `~/Library/Logs/DiagnosticReports/`
names the faulting call.** Watch for a regression: Accessory apps must still take
keyboard focus in the editor window.

2. **App died a few seconds after the panel appeared.** The hover thread called
   `show`/`hide`/`set_position` directly — legal on Win32, fatal on macOS, where all
   AppKit window calls must be on the main thread. Fix: macOS marshals every panel
   call through `run_on_main_thread`. Idle release (destroy+recreate the overlay) is
   now Windows-only: it exists for WebView2's process tree, and WKWebView has no
   equivalent cost.
3. **Data folder unopenable** ("damaged or incomplete"): the identifier
   `com.tofunuggets.app` makes `~/Library/Application Support/com.tofunuggets.app`
   look like an app bundle to Finder, hiding the log. Fix: new `paths.rs` — macOS
   uses `Tofu Nuggets`; **Windows keeps the identifier dir** (shipped installs store
   settings/index there and would be stranded by a rename).
- Still open once hover runs: retina rect alignment (panel offset ×2 = unit bug),
  hidden-extension name resolution, false hover triggers in Finder icon-view windows,
  macOS `selected_icon` + `list_icons` still stubs.
- Ad-hoc signing means macOS keys the Accessibility grant to each build's signature:
  **every new CI build must be granted again** (stale entries accumulate in the list).
- Remaining Route 1 work after AX hover verified on Mini: macOS overlay/panel look,
  badge equivalent (needs list_icons via Finder AX tree), selected_icon, hotkey/tray/
  updater verification, release.yml macOS matrix + Gatekeeper docs at mac launch.

**Route 1 test strategy CONFIRMED (owner, 2026-07-21)**:
- **CI matrix from day 1**: every PR compiles + unit-tests on Apple-silicon macOS
  runner (`macos-14`/`15`), .dmg/.app artifacts attached for download. CI is the
  compile/test gate only — it can NOT test behavior (Accessibility/AX permission needs
  a GUI grant; hover/overlay/badges need eyes).
- **Behavior testing on owner's work M4 Mac Mini**: self-managed (owner is admin, no
  IT/MDM, no workplace policy issue), runs **macOS 26 Tahoe**. Owner sideloads CI
  artifacts during work hours and runs a per-build manual checklist (hover, overlay,
  badges, editor, drag-drop, settings, Gatekeeper "Open Anyway" flow).
- Hardware covers macOS 26 only; macOS 14/15 coverage = CI compile + invited beta
  testers later.

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
