# Project Memory — Handoff for Claude Sessions

> Any new session reads this + CLAUDE.md (+ docs/GLOSSARY.md for the code map) and
> continues without re-asking settled questions. **Update after every session where
> decisions are made.** Detail older than the current line lives in git history.

## Status (2026-07-22)

- **Current release: 0.3.0 — PUBLISHED. First two-platform release; Route 1
  (macOS port) COMPLETE.** Windows `.exe` + arm64 `.dmg` + updater artifacts
  for both platforms from one tag. Owner updated existing installs on both
  OSes (macOS via the in-app updater from the last test build — first live
  macOS updater run, worked). All Route 1 leftovers closed: icon enumeration
  (PR #23), badge layer (PRs #25/#26, Mini-verified), release matrix
  (PRs #27/#28).
- **Next-release backlog (owner, 2026-07-22) — ALL THREE CLOSED by v0.4.0 phase
  W2 (PR #33): 1 and 3 shipped as code, 2 became the standing release-notes rule
  (the dialog already showed the body; the gap was what we wrote in it).**
  1. App version shown on the main window.
  2. Update dialog lists new features with a short description of each
     (today it only asks install yes/no).
  3. Main window: small "report bugs / request features" text pointing at
     owner's email (ASK OWNER which address to publish before implementing);
     clicking it shows a reminder that the app is in active development, so
     some breakage is expected.
- Open bugs parked: deleted-note-reappears (needs repro), external SSD
  volumes not annotatable (deliberate no-fix), macOS exit regression
  not reproduced since PR #26 (census logging stays in place).

## Next step — CONFIRMED v0.4.0 plan (owner, 2026-07-25 planning session)

Fixes the 0.3.0 badge-layer bugs (Windows autohide taskbar blocked; macOS stray
window after display sleep + dots traveling on Show Desktop) plus the small-updates
backlog. **Windows work lands and is verified first; macOS code only after** (owner
token-economy instruction). Coding sessions run on Opus 4.8-Low → keep PRs small,
each with an explicit verification checklist.

**Decisions (owner-confirmed):**

1. **Windows badges — Option A, keep the layer**: TOPMOST layered window stays.
   Bug cause: monitor-covering TOPMOST window trips the shell's "fullscreen app"
   heuristic → autohide-taskbar reveal suppressed. Fix ladder: (1) shrink layer
   1 px from each monitor edge; (2) if insufficient, carve autohide reveal strips
   from the window bounds (`SHAppBarMessage` ABM_GETSTATE/ABM_GETTASKBARPOS, all
   edges/monitors). HWND_BOTTOM rework (drop TOPMOST, delete occlusion machinery)
   was considered and NOT chosen. `IShellIconOverlayIdentifier` re-rejected
   (15 slots; one overlay per file — clashes with OneDrive on owner's redirected
   desktop; HKLM COM DLL inside explorer.exe; no accessibility control).
   Verify: autohide taskbar reveals with badges on; dots render + occlude; Win+D ok.
   **DONE — PR #31 merged, owner-verified 2026-07-25. Ladder step 1 was enough**:
   new `layer_rect()` in `badges.rs` = virtual screen inset `EDGE_INSET` (1 px)
   all sides; all dot coords are relative to that rect's origin (badged set,
   occlusion pass, `UpdateLayeredWindow` push all read the one helper). TOPMOST,
   occlusion machinery and parentage untouched. Step 2 (`SHAppBarMessage` reveal
   strips) NOT needed and NOT implemented. **Known limit left standing**: only the
   outer virtual-screen edges are inset, so a monitor in the *interior* of a
   multi-monitor arrangement is still fully covered — if the taskbar bug ever
   shows up there, step 2 is the follow-up.
2. **macOS badges — Finder tags replace the webview layer**: write/remove tag
   `"Nugget"` (orange) via xattr `com.apple.metadata:_kMDItemUserTags` (binary
   plist array; append/remove ONLY our tag, never touch user tags). Finder draws
   the dot natively on Desktop AND in Finder windows (next big update gets badges
   free). Sidecars stay source of truth; tags are a derived cue, resynced at
   startup. Deletes `badges_mac.rs` + `badges.html` machinery. FinderSync appex
   deferred (Xcode target, bundler can't embed, user must enable, ad-hoc-signing
   unknowns) — revisit only if tags insufficient or when Apple $99 account bought.
   Tradeoffs accepted: tag visible in Get Info/sidebar; dot per Finder convention
   (not our styled corner dot); badge size/corner settings become Windows-only.
3. **macOS lifecycle — real fix, delete the hacks**: override
   `applicationShouldTerminateAfterLastWindowClosed` → false on the NSApp delegate
   via objc2 (the documented fallback; `prevent_exit`/hide-on-close/Accessory are
   all already in place and proven insufficient — termination skips
   `ExitRequested`). Then panel goes back to plain `hide()` (parking removed) —
   the stray after-sleep window IS the parked panel constrained back on-screen.
   Census logging stays as discriminator. **ORDER HAZARD: the always-visible badge
   webview window is plausibly what keeps the app alive today — the delegate
   override must land and be Mini-verified BEFORE the badge window is deleted.**
4. **Small updates (ship with v0.4.0, all Windows-testable)**: checklist done
   items get strikethrough (CSS on `data-checked="true"` in editor/overlay/main
   preview); app version shown on main window; main-window "report bugs / request
   features" text → `farabigithub@gmail.com` (owner-confirmed address), click
   shows active-development/breakage-expected reminder. Update dialog ALREADY
   shows release-note body (`updater.rs`) — the gap is process: **standing release
   rule: every release body lists new features with a one-line description each.**
   **DONE — PR #33 merged, owner-verified 2026-07-25.** Strikethrough is CSS only
   (`li[data-checked="true"] > div` in `editor.css` + `overlay.css`; the dim uses
   `--fg-dim`, which equals `--fg` under high contrast so the line alone carries
   the state there). **The main-window row preview needed no change — `main.js`
   sets it with `textContent`, so it has no checked markup to style.** Version
   comes from `window.__TAURI__.app.getVersion()`; **no capability entry was
   needed — `core:default` → `core:app:default` already grants
   `core:app:allow-version`** (verified against `gen/schemas/acl-manifests.json`).
   Report-bugs is a footer button toggling in-page text (never alert/prompt).
   Release rule now written into `docs/V0.1.3.md` "Release process" and the
   settled-decisions table below.
5. **macOS pause/badges-off semantics with tags (owner-confirmed)**: tray Pause
   leaves tags in place (pauses hover only on macOS — dots can't be cheaply
   hidden when Finder draws them; documented platform difference); persistent
   badges-off setting strips our tag from all files, re-tags on re-enable.
   Note-delete always removes the file's tag.

**Phases (REORDERED by owner, 2026-07-25)**: ~~W1 taskbar-fix PR~~ (DONE, PR #31)
→ ~~W2 small-updates PR~~ (DONE, PR #33) → **E-phases (Explorer-on-Windows
update, pulled BEFORE the macOS phases — owner wants it built and tested on
Windows now)** → M3 delegate-override + unpark PR → M4 Finder-tags PR (batch
Mini test runs — every CI build re-prompts the Accessibility grant under ad-hoc
signing). **Release packaging OPEN**: ship a Windows-only release after E vs
hold one release for E+M3+M4 — owner decides once E is verified. Note: macOS
0.3.0 bugs (stray window after sleep, dots traveling on Show Desktop) keep
hurting until M3/M4 land — argument against delaying them long.

## Explorer update — the pill design (owner concept, confirmed 2026-07-25)

Persistent full-desktop dot layer stays **desktop-only**. Inside Explorer
windows a live-tracked overlay is deliberately rejected (scroll jitter,
viewport clipping, Win11 tabs/multi-window churn). Instead, per Explorer
window: a **pill** — small glassy acrylic toggle, styled like the app.

- **Inactive**: shows the count of notes in the window's current folder
  (on-demand read of that folder's `.nuggets` + redirected sidecars — no
  watcher expansion needed for the count).
- **Click**: draws dots over annotated *visible* items — a one-shot UIA
  snapshot clipped to the client area, never live-tracked.
- **Dots dismiss on ANY of**: scroll, focus loss, window move/resize,
  folder/tab change. Snapshot model is the point — no tracking headaches.
- **Placement (Claude decision, owner delegated)**: bottom-right of the
  content area — above the status bar, left of the vertical scrollbar,
  ~12 px inset (title-bar row rejected: Win11 tabs/search/ribbon states).
  Follows the window via WinEvent LOCATIONCHANGE hook (same pattern as
  badges.rs); hides on minimize/occlusion/foreground-loss.
- **Z-order**: NOT global-TOPMOST (taskbar lesson). Candidate: set pill owner
  to the Explorer hwnd (`SetWindowLongPtr GWL_HWNDPARENT`) so it rides above
  that window only — cross-process ownership is quirky, spike it (E0).
- **Scope assumption (flag to owner if wrong)**: "work inside Explorer" also
  includes hover panel + hotkey note-creation on Explorer items (Route 3
  core); the pill covers the badge half only.

**E-phase breakdown (Opus-Low sized, each with verification checklist):**

- **E0 spike (go/no-go, findings recorded in `spikes/`)**: (a) IShellWindows
  enumeration + Win11 tab behavior (per-tab folder path; tabs may or may not
  appear as separate entries — verify); (b) UIA item bounding rects across ALL
  view modes (extra-large icons → details/list) + visible-item filtering;
  (c) the GWL_HWNDPARENT pill z-order trick on Win11 26200 (+ Win10 later).
  **DONE — `spikes/explorer-pill/`, PR pending; ALL THREE GO (Win11 26200,
  2026-07-25). Read the README before E1.** Verdicts:
  - **A GO**: `IShellWindows` → `IServiceProvider` → `IShellBrowser`
    (SID_STopLevelBrowser) enumerates windows; folder path via active
    `IShellView` → `IFolderView2::GetFolder(IPersistFolder2)` → `GetCurFolder`
    PIDL (NOT a cast of IShellView — that fails). Save/Open dialogs (#32770) are
    absent from `IShellWindows` for free; content windows are `CabinetWClass`.
    Win11 tabs: designed read path = one entry per tab sharing the top HWND,
    active tab = the one with `IsWindowVisible(view_hwnd)`; no switch event, so
    POLL. Multi-tab active-folder readout is the one cosmetic OWNER-CONFIRM
    (SendKeys tab-spawn was abandoned — leaked keystrokes).
  - **B GO**: items view found by UIA control type (List/DataGrid → ListItem/
    DataItem — not localizable names). Rects correct in all 8 modes; scrolled-out
    items are ABSENT (virtualized), tree gives visible set + a 1-row `IsOffscreen`
    fringe (list mode materializes all). Visible filter = `IsOffscreen==false` +
    rect in client area. Cost ~90–145 ms for 120 items (cold≈warm; whole-window
    `FindAll` dominates) = click→dots latency; E3 can shrink it by scoping
    `ElementFromHandle` to the shell-view HWND.
  - **C GO (owned)**: `SetWindowLongPtr(GWLP_HWNDPARENT)=Explorer` — popup stays
    above owner, auto-hides on owner minimize, does NOT follow owner move (needs
    `LOCATIONCHANGE` reposition, like badges.rs), SURVIVES owner close (no crash),
    no Explorer interference. "Below an unrelated app" = standard owned semantics,
    cosmetic OWNER-CONFIRM. WinEvent fallback built but unused (naive
    `SetWindowPos(our, ex)` places BELOW — would need insert-above-predecessor).
- **E1**: hover + hotkey inside Explorer windows — extend the UIA hit-test
  path, per-window infotip suppression, polling gate becomes "desktop OR
  Explorer window foreground" (README + ARCHITECTURE budget line update in
  the same PR). **DONE — `wip-explorer-hover`, PR #36, OWNER-VERIFIED on this
  machine 2026-07-26** (hover panel, editor, hotkey, selection fallback,
  multi-tab incl. different-path tabs, details+large+list+content views, desktop
  unregressed). Core in `desktop.rs` (hotkey.rs untouched; hover.rs got panel
  placement; editor.rs got a diagnostic log): `foreground_surface()` classifies
  the foreground window (Progman/WorkerW = desktop, CabinetWClass = Explorer,
  else None → no UIA hit-test). Explorer item → path = UIA name resolved against
  the active tab's folder. **Three bugs the E0 spike missed, found + fixed on
  hardware:** (1) **COM apartment** — the shell chain is STA-only; the worker
  threads ran MTA where `IShellBrowser::GetWindow` fails (0x8001010D) so nothing
  resolved → `init_com_for_thread` now uses `COINIT_APARTMENTTHREADED` (badges
  unaffected, they pump a message loop); (2) **ElementFromPoint** lands on a row
  child (column cell/label), not the item → `item_ancestor` climbs to the
  ListItem/DataItem; (3) **active tab** detection by `IsWindowVisible` was
  unreliable (different-path tabs resolved the wrong folder) → now the tab whose
  view contains the cursor (`WindowFromPoint` + `is_descendant`). Panel placement
  reworked: wide Explorer rows anchor to the cursor (not the row's far edge),
  flips on both axes + clamps on-screen (`virtual_screen_height` added). Hotkey
  selection fallback uses `IFolderView2::Items(SVGIO_SELECTION)` → filesystem
  path. `debug_cursor_chain`/`dump_shell_tabs` (Windows) log the cursor chain +
  all Explorer tabs on a hotkey miss. **Infotip suppression NOT implemented for
  Explorer**: modern
  content view is `DirectUIHWND`, not SysListView32, so `LVS_EX_INFOTIP` has no
  target — panel is always-on-top + side-offset, native tooltip coexists
  (documented, not hacked). Storage already folder-general (new round-trip test).
  **Watcher/index NOT expanded** — Explorer-created notes don't show in the main
  list yet (deferred to E2). Cargo features gained `Win32_System_Ole/_Variant/
  _UI_Shell_Common`. Accepted minor regression: desktop hover now gated to
  desktop-foreground (app-focused hover over a visible desktop gap no longer
  fires) — matches the documented "only while desktop foreground" intent.
- **E2**: pill in count mode — create/track/destroy per window, glassy
  styling, accessibility scaling, count refresh on navigation/tab switch.
  **DONE — `wip-explorer-pill`, PR pending owner verification.** New
  `pill.rs` (Windows-only) + `roots.rs`; `desktop.rs` gained
  `explorer_windows()`/`explorer_is_foreground()`; `storage.rs` gained
  `count_notes_in_folder`. Decisions taken while building:
  - **GDI layered window per Explorer window, NOT a webview** — a WebView2
    process tree per pill (times N windows) breaks the RAM budget. Cost of the
    choice is hand-composited everything (rounded rect, border, accent dot,
    digits via a GDI text coverage mask, since GDI writes no alpha).
    **Measured: ~80 KB per pill** (two pills: WS 57.36 → 57.52 MB). Consequence
    accepted: the "glass" is a translucent fill, not real acrylic —
    `UpdateLayeredWindow` and DWM blur-behind don't compose.
  - **Anchor is SHELLDLL_DefView, not `IShellBrowser::GetWindow`.** The latter
    returns the whole ShellTabWindowClass (nav pane + status bar included) and
    the first build put the pill on top of the status bar's view-mode buttons.
    Fixed via `IShellView::GetWindow`; inset 12 logical px, minus `SM_CXVSCROLL`.
  - **Active tab without a cursor (the E2 unknown)**: probe a point at 3/4 width,
    1/2 height of the frame and walk *down* with `ChildWindowFromPointEx`, which
    is unaffected by other apps covering the window (`WindowFromPoint` is not).
    Falls back to visible-view then first-match.
  - **Zero notes ⇒ pill hidden**, not "0".
  - **Idle cost**: no Explorer window ⇒ no timer at all; foreground Explorer ⇒
    700 ms tick (shell enumeration + folder read) plus a short grace window
    after Explorer gains focus; unfocused Explorer ⇒ 2 s tick with no shell
    call and no disk read. Nothing is missed because navigation/tab switch need
    focus, new/closed windows are caught by a class-only `EnumWindows` scan,
    and the editor calls `pill::notes_changed()` on save/delete.
  - **Perf-measurement trap (cost a cycle)**: whole-process CPU is dominated by
    the BADGE layer's 2 s UIA walk, which short-circuits only while a window
    covers the whole virtual screen — the same build reads 0 ms or 250 ms per
    25 s depending only on whether the desktop is covered. The pill's real cost
    was isolated by building with `pill::spawn` removed: **250 ms / 25 s without
    it vs 218–234 ms with it** — inside the noise. Always isolate this way
    before making a budget claim.
  - **Three bugs found by the owner on the first hardware run, all fixed in the
    same branch:**
    1. **No pill after the main window's Open button** until you clicked away
       and back. A window that just opened is not enumerable through
       `IShellWindows` for a moment, so the first sync found nothing — and the
       tick cadence was keyed off *pills existing*, so it disarmed and never
       retried. Fixed three ways: cadence now keys off Explorer windows
       existing (cheap class-only `EnumWindows`, which sees a window instantly);
       a `settle` grace window keeps polling for ~5 ticks after Explorer gains
       focus; and the full path also runs whenever a window has no pill yet.
    2. **New tab (Ctrl+T, opens on This PC) kept the previous tab's count.**
       `explorer_windows()` dropped entries with no filesystem folder, which let
       the *inactive* tab win the fallback. Now `folder: Option<PathBuf>` and a
       non-filesystem active tab hides the pill.
    3. **Lag when moving/resizing.** Every `LOCATIONCHANGE` reset the coalescing
       timer, so during a continuous drag it never fired; and each fire ran the
       full shell enumeration. Now a `MOVE_ARMED` flag lets it fire throughout
       the drag at 30 ms, and the move path does placement only — no shell call.
    **Second hardware run — two more wake-up gaps (same root: the layer only
    wakes on WinEvents + its own timer, and it kills the timer when zero
    Explorer windows exist or when paused):** (a) Open button from the main
    list still no pill — `ShellExecute` from an already-foreground Tofu window
    opens Explorer in the BACKGROUND (Windows foreground-lock), so no
    `CabinetWClass` foreground event fires and, with no other Explorer window
    open, no live timer notices it. (b) Unpause left no pill until the window
    was refreshed — pause tears pills down and kills the timer, resume only
    flips the shared atomic. Fix: `pill::wake()` (FORCE_FULL + arm_full),
    called from `open_in_explorer` (links.rs) and the tray Pause toggle
    (tray.rs). CDP-verified: Open on a folder from a cold layer → pill in
    0.8 s. Design note: do NOT try to fix this with a global window-creation
    hook — the app knows when it opens Explorer and when it unpauses, so an
    explicit poke is cheaper and has no perf-budget cost.
    **Third hardware run — navigation in an UNFOCUSED window missed.** Open a
    new (windowed) Explorer on Home/This PC (folder None, pill correctly
    hidden, pill struct created), then navigate to a folder-with-notes while
    Explorer is not the focused window → no pill until refocus/refresh. Cause:
    the poll only re-reads a window's folder on the FAST (foreground) tick;
    `cheap_pass` (unfocused) never re-enumerates, and a pill already existed for
    that top window so the class scan saw nothing new. Fix: hook
    `EVENT_OBJECT_NAMECHANGE` — a folder nav / tab switch renames the frame — 
    filtered to top-level CabinetWClass, forcing a full sync. Low volume, so
    unconditional (unlike LOCATIONCHANGE). Verified (Explorer unfocused, driven
    via Shell.Application.Navigate2): This PC→hidden, notes folder→visible 0.7 s,
    C:\Windows→hidden, back→visible. Idle CPU unchanged (265 ms/25 s, inside
    the badge-walk noise band). NOT the desktop dots showing behind (owner's
    guess) — the badge layer is transparent; this was pure nav detection.
    **Fourth hardware run — pill DRAWN but invisible (z-order).** tofu.log
    showed `pills=[4:true]` (count 4, drawn) for the Desktop folder yet owner
    saw nothing until refresh/refocus. Root cause: the pill is an owned popup,
    and an owned popup sits above its owner only until the owner is
    re-activated — a pill drawn while its Explorer window was already foreground
    got stacked BEHIND that window. Every automation "repro" had masked it by
    calling SetForegroundWindow (which restacks owned windows). Also `render()`
    early-returned on unchanged bitmap, so it never re-raised. Fix: on every
    render, insert the pill at the slot just above its owner
    (`SetWindowPos(pill, GetWindow(owner, GW_HWNDPREV), SWP_NOACTIVATE)`) —
    NOT `HWND_TOP`, which would float it over unrelated apps. Verified by z-order
    probe: pill directly above Explorer (idx 64<65); Notepad raised → Notepad
    top, pill+Explorer below it (pill still just above its own owner); refocus
    restores. This was the real bug behind every "reveals after refresh" report;
    the always-full-sync change (prior commit) was necessary too but not
    sufficient. **Owner-confirmed working 2026-07-26; the temporary `pill:`
    tofu.log diagnostic was then stripped.**
    Regression-tested after the fixes: new window gets its pill within 1.2 s
    with no refocus; pill-to-window gap stays constant (35/36 px) across a
    15-step drag; This PC window's pill hidden while the folder window's stays
    visible; minimize/restore; all-closed leaves zero pill HWNDs.
  - Verified on hardware this session: pill renders with the right count,
    minimize hides it / restore brings it back, two windows = two pills, an
    empty folder's pill stays hidden, closing both windows leaves zero pill
    HWNDs and the app alive. Owner confirmed on the first run: count correct,
    follows move + resize, themes/contrast/settings/restart all fine.
- **E3**: pill click → on-demand dots + all dismissal rules. **DONE —
  `wip-explorer-dots`, PR pending owner verification. E-PHASES COMPLETE (Explorer
  update fully built on Windows).** Clicking a pill toggles a one-shot UIA
  snapshot of the annotated *visible* items and draws a desktop-style badge dot
  over each, in a second owned layered window that is `WS_EX_TRANSPARENT` so a
  click where a dot sits falls through to the file (click-through requirement).
  Snapshot in new `desktop::annotated_item_rects(view, folder)`: `ElementFromHandle`
  on the shell-view HWND (E0's latency scope) → `FindAll` ListItem/DataItem →
  filter `IsOffscreen==false` + rect intersects the content area + name resolves
  to a `has_nugget` path. Dots are **snapshot, never live-tracked** — dismissed
  (not repositioned) on the first view change: scroll/resize (item
  `LOCATIONCHANGE` inside the frame, caught by folding a pre-filter check into the
  existing `move_event`), focus loss (`fg_event`), folder/tab change
  (`nav_event`/NAMECHANGE). Dismissal is **global** (any qualifying change drops
  every window's dots) — chosen over per-window bookkeeping because the snapshot
  is momentary; passes the whole dismissal checklist. Two atomics: `DOTS_ACTIVE`
  (gate — all dismiss hooks are no-ops while zero dots shown, so zero idle cost),
  `DOTS_DISMISS` (signal drained at the top of `sync`). Pill shows an
  accent-bordered active state while its dots are up (threaded `active` bool
  through render/draw_pill/compose + the `drawn` cache tuple). **Hardware bug #1
  (fixed, owner-confirmed 2026-07-26): scroll did not dismiss.** Modern Explorer
  content is a DirectUIHWND — list items are not real child windows, so wheel
  scroll fires no `EVENT_OBJECT_LOCATIONCHANGE` (the move-hook path caught
  nothing). Fix: a `WH_MOUSE_LL` hook (`wheel_proc`) installed only while dots are
  shown (`update_wheel_hook`, tied to `DOTS_ACTIVE` → zero idle cost) dismisses on
  `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`; plus `EVENT_SYSTEM_SCROLLINGSTART` for
  scrollbar-drag / keyboard scroll. Both gated on dots + scoped to CabinetWClass.
  Snapshot latency is
  logged (`dots: N annotated ... snapshot M ms`) so the owner can report the
  100+-item number. Dot appearance/scale reuse the desktop badge (same warm
  accent + white rim, top-right corner nudge, scaled by the pill's accessibility
  `Style`). Respects pause + badges-off (both destroy dots with the pills).
  Design guards written into the `pill.rs`/`desktop.rs` `//!` headers +
  ARCHITECTURE §7: **do not "improve" the snapshot into a tracked overlay.**
  fmt/clippy clean, 38 tests (added a `draw_dot` clip/centering test).
  **OWNER-VERIFIED on this machine 2026-07-26 — full checklist passed** (dots on
  exactly the annotated visible items in details/large/list/content views;
  dismiss on scroll/focus-loss/move/resize/nav/tab; toggle off; click-through;
  two windows independent; 100+ latency; pause+badges-off inert; E1/E2/desktop
  badges unregressed; ~0% CPU dots-off). **PR #38 open, based on
  `wip-explorer-pill` (E2/#37) so its diff is E3-only — RETARGET to `main` once
  #37 merges.** E-PHASES DONE; release packaging decision (Windows-only after E
  vs hold for E+M3+M4) is the owner's, per the reordered-phases note above.
- **Deferred design decision — DECIDED at E2 (implemented in the same PR)**:
  main-window list/index scope for notes outside the desktop. **Chosen: known
  roots.** `roots.rs` records the parent folder of every saved note in
  `known_roots.json` (app-data); the startup index rebuild scans desktop dirs +
  known roots; roots whose folder no longer exists are dropped on load. **The FS
  watcher stays desktop-only** — an unbounded watched set is exactly the
  background cost the budget forbids, and all it would buy is live index updates
  for renames/deletes outside the desktop, which the next rebuild fixes.
  Rejected: watching every known root (cost), indexing on hover (turns a read
  path into a write), full-disk sidecar scan (minutes; indexes other people's
  folders on shared machines). Rationale written into ARCHITECTURE §4. Residual
  gap (documented in GLOSSARY): rename/delete of an off-desktop annotated item
  while the app runs is only reflected after the next rebuild.

macOS side of the big update unchanged: tags (M4) already cover Finder-window
badges; AX hover inside Finder windows is a later phase (false-trigger bug
already on file). Shell icon overlays stay rejected. **Parked further
updates**: last-edit date display (cheap — `modified_ms` already stored),
edit history (schema change, design later), telemetry (privacy + endpoint
decision needed).

## Older status (2026-07-20)

- **Release 0.1.3** — shipped through the CI pipeline, updater live-verified.
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

   **First v0.3.0 tag run FAILED on macOS leg (2026-07-22)**: `bundle.targets`
   was `["nsis"]` — Windows-only, so the mac leg compiled but bundled NOTHING
   and tauri-action errored "No artifacts were found" (the CI dmg job never
   caught this because it passes `--bundles dmg` explicitly). Windows leg
   succeeded and attached its assets to the draft. Fix PR
   `wip-release-mac-targets`: targets `["nsis", "app", "dmg"]` (Tauri skips
   non-native targets per platform; `app` produces the updater `.app.tar.gz`).
   Recovery: merge fix → delete draft release + delete tag → re-tag v0.3.0.

   **Re-tag succeeded (2026-07-22)**: draft v0.3.0 complete — `.exe` + sig,
   arm64 `.dmg`, `.app.tar.gz` + sig, and one merged latest.json with
   `windows-x86_64` + `darwin-aarch64`. Awaiting owner review + publish.
   **Route 1 leftovers all CLOSED once published.** Next-release reminder:
   macOS in-app updater (0.3.0 → next) gets its first live test then; also
   remember every macOS update re-prompts for the Accessibility grant
   (ad-hoc signing).

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
| Release notes | **Binding from v0.4.0**: every release body lists each new feature with a one-line description — `updater.rs` shows the body verbatim in the update dialog (docs/V0.1.3.md) |
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
