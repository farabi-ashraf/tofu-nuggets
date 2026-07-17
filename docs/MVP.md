# MVP Scope & Milestones

## In scope (MVP)

- Windows 10/11, desktop icons only.
- Hover over annotated desktop icon → glassy panel shows the nugget.
- Badge layer: small visual cue on tagged icons (toggleable) so tagged items are scannable at a glance.
- Global hotkey on selected desktop icon → rich text editor (bold/italic, bullets, checkable todos, clickable URLs).
- File-to-file links: link another file/folder inside a note; clicking opens Explorer at target.
- Sidecar storage (`.nuggets` hidden folders), SQLite cache index.
- Main window: list of all annotated items.
- Accessibility: overlay font size (S/M/L/XL), panel scale, dark/light/system theme, Reduced Motion + High Contrast respect (see ARCHITECTURE.md).
- Tray icon: open, pause, settings (hotkey, autostart, accessibility, badges), quit.
- Performance budget per ARCHITECTURE.md: ~0% CPU idle, core RAM ~15–20 MB, icon count must not affect hover cost.

## Out of scope (MVP) — explicit

- File Explorer window integration (post-MVP, likely Pro tier)
- Right-click shell context menu (needs shell extension — post-MVP)
- Sync, accounts, teams
- macOS/Linux
- Search, tags, link-graph view
- Monetization (free MVP per FEASIBILITY.md)

## Milestones

```
0. Spike: hover detection ✅ GO      → verified 2026-07-17 on Win 11: simtest 51/51 icons
   (UIA ElementFromPoint on desktop)    detected + paths resolved (spikes/hover-detect).
                                        Still pending: Win 10, multi-monitor, DPI ≠ 100%.
1. Overlay panel + badge layer ✅    → verified 2026-07-17 on Win 11: panel shows on hover over
                                        annotated icon (translucent glass, correct position/DPI),
                                        hides on leave; badges render on tagged icons only,
                                        click-through by construction (WS_EX_TRANSPARENT).
                                        Deferred: right-edge flip test, native infotip
                                        suppression, WebView2 idle-release (see ARCHITECTURE
                                        perf notes — measured 379 MB warm, release mandatory).
2. Sidecar storage + index ✅        → verified 2026-07-17: 10 unit tests pass (write/read
                                        roundtrip, rename moves sidecar, cross-dir move, index
                                        rebuild skips stale sidecars, watcher event handling).
                                        Bonus: WebView2 idle-release shipped + live-verified
                                        (6 procs → 0 after idle, recreate on hover ~1 s).
3. Editor (TipTap) + hotkey ✅       → verified 2026-07-17 E2E: Ctrl+Shift+N over icon opens
                                        editor with existing note; typed text saved to sidecar
                                        (created_ms preserved); hover panel shows the edit.
                                        Deferred to M4/M6: todo-check persistence from the
                                        hover panel, URL open-in-browser interception.
4. File links                        → verify: nugget:// link opens Explorer at target;
                                        missing target shows graceful error
5. Main window + tray + autostart    → verify: all annotated items listed; autostart survives
                                        reboot; pause stops hover polling
6. Settings + accessibility          → verify: font size / panel scale / theme apply live;
                                        High Contrast falls back to solid colors; Reduced
                                        Motion disables animations; badges toggle off
7. Polish + installer (MSI/NSIS)     → verify: clean install/uninstall on fresh VM; RAM/CPU
                                        measured against ARCHITECTURE.md performance budget
```

Milestone 0 is a go/no-go gate: if desktop hover detection proves unreliable, fall back to the Explorer infotip shell-extension approach before building anything else.
