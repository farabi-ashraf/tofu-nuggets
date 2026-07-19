# Tofu Nuggets

Desktop overlay app for Windows: hover over a file/folder icon on the desktop → glassy panel shows user-written context ("nugget"). Notes are rich text (todos, links) created via global hotkey, stored as sidecar files.

## Key docs — read before large changes

- `MEMORY.md` — session handoff: settled decisions, status, next step. **Read first in a new session.**
- `docs/V0.1.1.md` — current work order: 0.1.1 fixes + updater/multi-platform/uninstall plans (owner instructions)
- `docs/FEASIBILITY.md` — market research, free-vs-paid decision, differentiators
- `docs/ARCHITECTURE.md` — stack (Tauri 2 + Rust), hover detection, badge layer, performance budget, accessibility, storage design
- `docs/MVP.md` — scope, explicit non-goals, milestones with verification criteria
- `docs/GIT-GUIDE.md` — owner-facing git/session workflow guide; keep in sync with practice
- `README.md` — public-facing project readme (published to GitHub); keep accurate on feature/build changes

## Stack

- **Backend**: Rust (Tauri 2), `windows` crate for Win32/UI Automation, `window-vibrancy` for acrylic
- **Frontend**: webview UI, TipTap for rich text editor
- **Storage**: sidecar JSON files in hidden `.nuggets` folders (source of truth) + SQLite cache index (rebuildable)

## Project rules

- MVP is desktop icons only. Do not add File Explorer integration, sync, tags, or shell extensions — they are explicitly post-MVP (see `docs/MVP.md`).
- Sidecar files are the source of truth; the SQLite index must always be rebuildable from them. Never store note content only in the index.
- Hover detection uses UI Automation (`ElementFromPoint`), not cross-process ListView memory reads.
- Background app: performance budget in `docs/ARCHITECTURE.md` is a hard requirement — ~0% CPU idle, ~15–20 MB core RAM, hover polling only while desktop is foreground, icon count must never affect hover cost. Treat regressions as bugs.
- Accessibility is MVP scope, not polish: font size, panel scale, themes, Reduced Motion / High Contrast respect.
- Every milestone has verification criteria in `docs/MVP.md` — a milestone is done when its check passes, not when code compiles.
- **After any discussion that adds/changes functionality or decisions: update the affected docs and `MEMORY.md` in the same turn.** (Owner instruction, standing.)

## Conventions

- Source on GitHub: single repo, default branch `main`, currently **private**. Never re-introduce per-platform branches (see `docs/V0.1.1.md` B2). Don't commit `.claude/settings.local.json` (gitignored).
- Rust: `cargo fmt` + `cargo clippy` clean before commit.
- Frontend: keep dependencies minimal; TipTap is the only large UI dependency budgeted.
- Test on both Win 10 and Win 11 for anything touching UIA, DPI, or the overlay window.
