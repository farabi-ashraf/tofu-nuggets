# Spike: badge layer reparented into the desktop z-band (A2) — VERDICT: NOT VIABLE

Question (docs/V0.1.1.md A2): can the badge layer live in the desktop's own
z-order — above the icons, below every app window — by reparenting under
Progman / SHELLDLL_DefView, killing both alt-tab flicker and over-window bleed?

**Answer on Windows 11 26200 (2026-07-20): no.** The desktop window chain does
not reliably composite foreign windows. Findings, each verified by screenshot
pixel-probing magenta test dots (`cargo run <mode> [defview] [rgn]`):

| Configuration | Renders? |
|---|---|
| `WS_EX_LAYERED` + `UpdateLayeredWindow` (popup or child, Progman or DefView) | never |
| `WS_EX_LAYERED` + `LWA_COLORKEY` / `LWA_ALPHA` | never |
| `WS_CHILD` style (any variant) | never |
| `WS_EX_TRANSPARENT` on a non-layered window | never |
| `SetWindowRgn`-shaped non-layered window | never |
| Plain rect popup, 600×600 | yes (origin AND offset) |
| Plain rect popup, 32–200 px, away from screen edge | no |
| Plain rect popup, 32 px touching top edge | yes |
| Failed configs + forced 200 ms repaint loop | still no |

Interpretation: whatever DWM does for Progman/DefView internals, arbitrary
reparented windows get no per-window composition surface — tiny/offset/layered/
shaped windows simply never reach the screen, and the cases that do render
appear to ride along Explorer's own painting by luck. Behavior is undocumented,
size- and position-dependent, and would differ across Windows builds (the
WorkerW chain itself already changed in Win11 24H2: DefView sits directly under
Progman here, no WorkerW). Not shippable.

Also observed: `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE)` fails
with `E_HANDLE` once the window is parented into the chain (works before).

## Consequence for A2 (plan B — implemented instead)

Keep the existing TOPMOST layered window (proven, per-pixel alpha, composites
everywhere) and fix the two complaints directly:

1. **Over-window bleed**: per-dot occlusion — before drawing a dot, walk the
   top-level windows above the desktop (EnumWindows, visible + non-cloaked +
   rect intersects icon cell); skip dots for covered icons. Dots then never
   draw over an overlapping window even while the desktop is foreground.
2. **Alt-tab flicker**: replace the 2 s foreground-gated show/hide with
   event-driven updates — `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`,
   `EVENT_OBJECT_LOCATIONCHANGE` throttled) so visibility and occlusion react
   in ~10 ms. With per-dot occlusion the layer can stay shown whenever the
   desktop is visible instead of blinking on focus changes.

Perf: hooks are push-based (0% idle), occlusion walk is a handful of rect
tests every refresh/event — within budget.
