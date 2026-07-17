# Project Memory — Handoff for Claude Sessions

> Purpose: any new Claude session reads this + CLAUDE.md and can continue without re-asking settled questions. **Update this file after every session where decisions are made.**

## Status

- **Phase**: pre-code. Docs complete, no source files yet.
- **Next step**: Milestone 0 spike — Rust program proving desktop-icon hover detection via UIA `ElementFromPoint` (go/no-go gate, see `docs/MVP.md`).

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
