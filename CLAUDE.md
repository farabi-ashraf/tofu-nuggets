# Tofu Nuggets

Desktop overlay app for Windows: hover over a file/folder icon on the desktop → glassy panel shows user-written context ("nugget"). Notes are rich text (todos, links) created via global hotkey, stored as sidecar files. Current release: **0.1.3**, public repo, shipped via GitHub Releases + in-app updater.

## Key docs — read before large changes

- `MEMORY.md` — session handoff: settled decisions, status, next step. **Read first in a new session.**
- `docs/GLOSSARY.md` — terms + code map + cross-window contracts. **Entry point for finding anything; updating it is mandatory when modules/commands/events/terms change.**
- `docs/ARCHITECTURE.md` — stack, hover detection, badge layer, performance budget, accessibility, storage design, threading rules
- `docs/V0.1.3.md` — 0.1.1→0.1.3 release record + standing policies (one-branch rule, release process)
- `docs/MVP.md` — original scope, explicit non-goals, milestone record
- `docs/SECURITY.md` — secret hygiene, updater key flow, incident steps
- `README.md` — public-facing readme; keep accurate on feature/build changes

## Code is the source of truth

Docs lag; code must stand alone. Standing conventions:

- Every `.rs` module keeps a `//!` header stating its responsibility, invariants, and gotchas — **update it in the same change that alters the behavior it describes**.
- Comments explain *why* (constraints, traps, rejected alternatives), not *what*.
- Tests document behavior: name them after the behavior they pin.
- Docs are only for cross-cutting content code can't express: architecture rationale, budgets, workflows, release/security process.
- `docs/GLOSSARY.md` is the map between the two — mandatory to keep current.

## Stack

- **Backend**: Rust (Tauri 2), `windows` crate for Win32/UI Automation, `window-vibrancy` for acrylic
- **Frontend**: webview UI, TipTap for rich text editor (only large UI dependency budgeted)
- **Storage**: sidecar JSON files in hidden `.nuggets` folders (source of truth) + SQLite cache index (rebuildable)

## Project rules

- Hover scope is desktop icons (Explorer integration, sync, tags, shell extensions deliberately deferred — see `docs/MVP.md` non-goals; revisit only as an explicit owner decision).
- Sidecar files are the source of truth; the SQLite index must always be rebuildable from them. Never store note content only in the index.
- Hover detection uses UI Automation (`ElementFromPoint`), not cross-process ListView memory reads.
- Background app: performance budget in `docs/ARCHITECTURE.md` is a hard requirement — ~0% CPU idle, ~15–20 MB core RAM, icon count must never affect hover cost. Treat regressions as bugs.
- Accessibility is core scope, not polish: font size, panel scale, themes, Reduced Motion / High Contrast respect.
- Features are done when their verification criteria pass, not when code compiles.
- **After any discussion that adds/changes functionality or decisions: update the affected docs and `MEMORY.md` in the same turn.** (Owner instruction, standing.)

## Workflow & conventions

- GitHub: single **public** repo, default branch `main`, branch ruleset `protect-main` (PR required, no force-push). Work on `wip-*` branches; owner merges PRs after verifying. Never re-introduce per-platform branches (see `docs/V0.1.3.md` B2).
- Release: bump version in `tauri.conf.json` + `Cargo.toml` (PR) → tag `v*` → CI builds/signs a **draft** release → owner publishes. Updater picks it up from `releases/latest/download/latest.json`.
- Rust: `cargo fmt` + `cargo clippy` clean, tests green before commit.
- Frontend is Vite-built: run `npm run build` in `app/ui` BEFORE `cargo build` (assets embed from `ui/dist`).
- Don't commit `.claude/settings.local.json` / `.claude/launch.json` (gitignored; leaked once, purged).
- Test on both Win 10 and Win 11 for anything touching UIA, DPI, or the overlay window.
