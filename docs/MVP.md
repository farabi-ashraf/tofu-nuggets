# MVP Scope & Milestone Record

> MVP (0.1.0) completed 2026-07-18; hardened + shipped as the 0.1.x line
> (`docs/V0.1.3.md`). This file records what was in/out of scope and that every
> milestone passed its verification gate. Per-milestone verification detail lives in
> git history and MEMORY.md lessons.

## MVP scope (all shipped)

- Windows 10/11, desktop icons only.
- Hover over annotated icon → glassy panel shows the nugget; badge dots mark tagged icons.
- Global hotkey → TipTap editor (bold/italic, bullets, checkable todos, links).
- File-to-file `nugget://` links; click opens Explorer at target.
- Sidecar storage (`.nuggets` hidden folders) + rebuildable SQLite index.
- Main window: all nuggets, filter, per-row Open/Edit/Delete (two-step confirm);
  saving an emptied note removes it.
- Accessibility: font size S–XL, panel scale, dark/light/system theme, Reduced
  Motion + High Contrast respect.
- Tray: open, pause, settings (hotkey rebind, autostart, accessibility, badges), quit.
- Performance budget (hard): ~0% CPU idle, core RAM ~15–20 MB, icon count must not
  affect hover cost. Measured on release build: idle 0.00% CPU, 6.3 MB private,
  WebView2 6→0 procs after idle release.

## Explicitly deferred (revisit only as owner decision)

- File Explorer window integration (Route 3 discussion pending — see MEMORY.md)
- Right-click shell context menu (needs shell extension)
- Sync, accounts, teams; search, tags, link-graph view
- macOS/Linux (macOS port is a candidate next step)
- Monetization (free while in beta; freemium later per owner's market research)

## Milestone record (all ✅, each passed its verification gate)

| # | Milestone | Gate that passed |
|---|---|---|
| 0 | Hover-detection spike | 51/51 desktop icons detected + paths resolved (`spikes/hover-detect`) — was the go/no-go for the whole approach |
| 1 | Overlay panel + badge layer | Panel shows/hides on hover with real transparency; dots on tagged icons only, click-through |
| 2 | Sidecar storage + index + watcher | Unit tests: roundtrip, rename follows, stale-skip; WebView2 idle-release live-verified |
| 3 | Editor + global hotkey | E2E: hotkey over icon → editor → save → panel shows edit |
| 4 | File links + infotip suppression | Link click opened Explorer at target; native infotips suppressed; todo toggles persist |
| 5 | Main window + tray + autostart | List/filter/Edit verified; background app; pause wired |
| 6 | Settings + accessibility | Live apply verified against real CSS (font/panel scale, themes, HC, RM) |
| 7 | Installer + deletion + polish | NSIS per-user install/uninstall cycle clean; budget measured; deletion E2E |

Post-MVP hardening (first external Win 10 install): web-link normalization, hotkey
customizer with non-fatal registration, single-instance guard, worker-thread window
creation, `tofu.log` diagnostics. Root cause of the field failure: missing WebView2
Runtime → fixed for good in 0.1.1 A1.
