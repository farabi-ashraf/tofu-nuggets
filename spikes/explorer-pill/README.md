# Spike: Explorer "pill" (E0) — VERDICT: GO on all three

Question set (MEMORY.md "Explorer update — the pill design", E-phase breakdown).
Three probes, each printing evidence to stdout, run on **Windows 11 26200
(2026-07-25)**. Read-only against Explorer — probes only ever created, drove and
closed the spike's own throwaway `probe-folder` window (title-fenced); no user
window or file was touched.

```
cargo run --bin probe_a          # window + tab enumeration
cargo run --bin probe_b          # UIA rects, current view mode (35 s sampler)
cargo run --bin probe_b -- cycle # UIA rects across ALL 8 view modes (temp window only)
cargo run --bin probe_c          # z-order trick (auto-scripts on our temp window)
cargo run --bin probe_c -- hook  # WinEvent fallback variant
```

Evidence below was gathered against a self-created temp folder of **120 `.txt`
files** opened in Explorer.

---

## A. Explorer window + tab enumeration — **GO**

`IShellWindows` (CLSID_ShellWindows) → per item `IServiceProvider` →
`IShellBrowser` (SID_STopLevelBrowser). From the browser: top HWND via
`GetAncestor(view, GA_ROOT)` + class; current folder path via active
`IShellView` → `IFolderView2::GetFolder(IPersistFolder2)` → `GetCurFolder` PIDL →
`SHGetPathFromIDListW`.

Measured:

```
[0] top_hwnd=0x90776 class=CabinetWClass view_visible=true path=F:\...\target\release
[1] top_hwnd=0x1708f8 class=CabinetWClass view_visible=true path=C:\...\probe-folder
=> 2 IShellWindows entries across 2 distinct top-level window(s)
```

- **Enumeration + per-window folder path: works** (once the PIDL comes from
  `IFolderView2::GetFolder`, not a cast of `IShellView` — that cast fails and was
  the initial `<err>`). Non-filesystem folders (This PC, etc.) return no path;
  handled.
- **Content window vs Save/Open dialog**: `IShellWindows` tracks only real
  browser windows — common `IFileDialog` Save/Open dialogs (`#32770`) never
  appear in the enumeration at all, so they are excluded for free. The `class`
  column is the secondary discriminator: Explorer content windows are
  `CabinetWClass`.

### Win11 tabs — the three sub-unknowns

The machine-driven run above had one tab per window, so the multi-tab specifics
below are **reasoned from the API + the probe's built-in grouping/visibility
logic and flagged OWNER-CONFIRM** (keyboard automation to spawn tabs proved
unsafe — `SendKeys` leaked keystrokes to other windows — so it was abandoned;
open a 3-tab window and rerun `probe_a`, switching tabs during the 20 s sampler,
to confirm):

1. **Separate entries?** Expected: each tab hosts its own `IShellBrowser`, so
   `IShellWindows` lists **one entry per tab, all sharing the same top-level
   HWND**. `probe_a` already groups by `top_hwnd` and prints
   "N entries share it (TABS)" to prove this at a glance.
2. **Active tab's folder?** Candidate discriminator, implemented: the inactive
   tabs' shell-view windows are hidden, so `IsWindowVisible(view_hwnd)` should be
   **true for exactly the active tab**. `probe_a` prints `view_visible` per entry
   — the active tab is the visible one, and its folder path reads out normally.
3. **What fires on tab switch?** No documented shell event is assumed. `probe_a`
   **polls** (400 ms) and reprints on any change to the
   {path, visible-view} set. Design consequence: the pill's count refresh on tab
   switch should be **poll/`LOCATIONCHANGE`-driven off the active view**, not
   event-subscription — same posture as `badges.rs`.

**Verdict A: GO.** Enumeration, per-window path, and dialog exclusion are proven.
The tab model has a working read path (visible-view = active tab, polled); the
one-line owner confirmation is a formality, not a risk.

---

## B. UIA item rects across view modes — **GO**

Target = the CabinetWClass window with the most items. Items view located by
UIA **control type** (List/DataGrid container → ListItem/DataItem children),
which skips the address-bar breadcrumb and nav-pane tree without matching
localizable names/AutomationIds. `cycle` sets each of the 8 modes via
`IFolderView2::SetViewModeAndIconSize` (temp window only) and re-enumerates.

Folder = 120 items. Per mode (UIA exposed / onscreen / offscreen, walk ms):

| View mode | UIA items | onscreen | offscreen | walk |
|---|---|---|---|---|
| extra large icons (256px) | 10 | 8 | 2 | 90 ms |
| large icons (96px) | 32 | 32 | 0 | 97 ms |
| medium icons (48px) | 66 | 66 | 0 | 109 ms |
| small icons | 43 | 41 | 2 | 106 ms |
| list | **120** | 120 | 0 | 90 ms |
| details | 41 | 40 | 1 | 125 ms |
| tiles | 17 | 16 | 1 | 93 ms |
| content | 20 | 19 | 1 | 89 ms |

- **Rects correct in every mode** — real client-area coordinates; item size
  scales with the mode (e.g. 218×226 xl-icon cells, 598×22 details rows). Zero
  zero-rect items.
- **Scrolled-out items**: **absent** from the UIA tree (virtualization), not
  present-with-virtual-rects. The count materialized ≈ what fits the viewport
  **plus a one-row buffer** whose `IsOffscreen=true` and whose rect sits just
  below the client area (e.g. details `item_041` at y=1117). So the tree gives
  you exactly the visible set + a fringe row; the deep tail (79 of 120 here) is
  simply not enumerable.
  - **Exception: list view materialized all 120** (`IsOffscreen=false` for every
    one) — list mode is not virtualized the same way. Harmless for the pill
    (rects are still correct); just don't assume the "≈viewport" count in list.
- **Visible vs non-visible is cleanly detectable**: `IsOffscreen` +
  rect-inside-client-area. The pill draws dots only where `IsOffscreen==false`
  and the rect intersects the content area.
- **Cost**: cold ≈ warm ≈ **90–145 ms** for 120 items. The whole-window
  `FindAll(Descendants)` for the container dominates, so keeping the
  `IUIAutomation` object alive barely helps. This is the **click→dots latency
  floor**. E3 optimization noted below (scope the search to the shell-view HWND).

**Verdict B: GO.** One-shot snapshot is accurate in all modes and the visible-set
filter is trivial. ~100 ms latency is fine for a manual toggle; optimizable.

---

## C. Pill z-order trick — **GO (owned window)**

Bare `WS_POPUP` (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`), owner set to the
Explorer HWND via `SetWindowLongPtrW(GWLP_HWNDPARENT)`. Probe auto-scripts
minimize/restore/move/close **on its own probe-folder window** and logs the
top-level z-order rank of {our window, Explorer, foreground} each second (lower
rank = higher in z-order).

```
SetWindowLongPtr(GWLP_HWNDPARENT) prev=0 err=0; owner now=0x1708f8
tick 1  explorer(alive iconic=false) our_vis=true  our ABOVE explorer
>> minimize
tick 3  explorer(alive iconic=true)  our_vis=false our ABOVE explorer   # popup auto-hid
>> restore
tick 6  explorer(alive iconic=false) our_vis=true  our ABOVE explorer   # reappeared
>> move (+40,+40)
tick 9  explorer(alive iconic=false) our_vis=true  our ABOVE explorer   # popup did NOT follow
>> close (WM_CLOSE)
tick 13 explorer(alive=false)        our_vis=true                        # popup survived, no crash
```

- **Stays above its owner**: yes, consistently (`our ABOVE explorer` every tick
  the owner is alive).
- **Follows owner minimize/restore**: the owned popup **auto-hides when Explorer
  is minimized** and reappears on restore — desirable (no orphan pill over an
  empty desktop).
- **Owner move**: the popup does **not** track the move — expected; the pill must
  reposition via a `LOCATIONCHANGE` WinEvent hook (already the plan, same as
  `badges.rs`).
- **Owner close**: cross-process ownership does **not** tear the popup down; it
  survived with no crash. The pill will destroy itself on the window-gone event
  anyway.
- **No interference** with Explorer observed (Explorer kept behaving normally).
- **"Below other apps when Explorer loses focus"**: owned semantics pin the popup
  above its owner but a *different* app raised above Explorer also lands above the
  pill. Not exercised by the scripted (API-driven) run — **OWNER-CONFIRM with a
  manual click-test** — but this is standard owned-window behavior and aligns
  with the design's "hide on foreground-loss/occlusion" rule.

**Fallback** (`probe_c -- hook`, non-owned + `EVENT_SYSTEM_FOREGROUND`
re-assert): runs clean, no crash, but the naive `SetWindowPos(our, ex, …)` places
the popup **behind** Explorer (inserting *after* a window = below it). A working
fallback must insert above Explorer's predecessor (`GetWindow(ex, GW_HWNDPREV)`)
or raise on the Explorer-foreground event specifically. **Not needed** — the
owned trick works.

**Verdict C: GO** with the owned-window approach; fallback documented but unused.

---

## Implications for E1–E3

All three foundations hold, so the pill design is buildable as scoped. **E1**
(hover/hotkey inside Explorer) gets its window + active-folder path from the
probe-A path (`IShellWindows` → `IShellBrowser` → `IFolderView2`), and the
polling gate keys off "desktop OR CabinetWClass foreground". **E2** (pill in count
mode) parents each pill to its Explorer HWND via `GWLP_HWNDPARENT` (auto-hides on
minimize, survives close, repositioned by a `LOCATIONCHANGE` hook exactly like
`badges.rs`); the per-tab count reads the active (visible-view) tab's folder,
refreshed by polling on navigation/tab-switch since no switch event exists.
**E3** (click → dots) is a one-shot UIA snapshot filtered to `IsOffscreen==false`
items clipped to the client rect — accurate in every view mode at a ~90–145 ms
cost, which E3 can cut by calling `ElementFromHandle` on the shell-view HWND
(from `IShellBrowser::GetWindow`) instead of the top window to shrink the
`FindAll` tree. The only open confirmations are cosmetic owner click-tests
(multi-tab active-folder readout; pill sitting below an unrelated app), neither of
which gates the build.
