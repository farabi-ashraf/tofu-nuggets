# Tofu Nuggets

> Hover a desktop icon → see your own notes about it.

Tofu Nuggets is a lightweight desktop overlay for Windows. Hover over any file or
folder icon on your desktop and a glassy panel appears showing context you wrote
yourself — a "nugget." Notes are rich text (todos, links, formatting), created with
a global hotkey, and stored as plain sidecar files next to the item they describe.

It runs quietly in the background: near-zero CPU when idle, hover polling only while
the desktop is the foreground window.

## Status

**v0.2.0 — beta, Windows-only.** Installers are published on the
[Releases page](https://github.com/farabi-ashraf/tofu-nuggets/releases); installed
copies self-update via the tray's "Check for updates…". A macOS port (Apple silicon)
is in progress.

## Using Tofu Nuggets

**Write a note** — press the global hotkey (default `Ctrl+Shift+N`) while the cursor
is over a desktop icon, or with an icon selected. The editor opens for that item.
You can also open any note from the main window's list.

**The editor** — rich text via toolbar or shortcuts: bold, italic, bullet lists,
todo checklists, and links (`Ctrl+K`). `Ctrl+S` saves, `Esc` saves and closes.
Saving an emptied note deletes it (badge and panel disappear with it).

**Link files and folders** — three ways to point a note at another item: the 📄/📁
toolbar buttons open a picker, or just **drag files/folders from Explorer onto the
editor** — each drop inserts a link named after the item. Clicking such a link in
the hover panel opens Explorer at the target; in the editor, `Ctrl+Click` follows
links.

**Hover panel** — rest the cursor on an annotated desktop icon for a moment and the
glassy panel appears beside it with your note rendered read-only; checkboxes are
live, links clickable. Move away and it hides. ✕ closes it immediately, ✎ jumps to
the editor.

**Badges** — a small dot marks every desktop icon that carries a nugget, so you know
what has notes without hovering. Dots hide under any window that overlaps them and
can be turned off in Settings.

**Main window** — tray icon → "Open Tofu Nuggets" (or launching the app again)
lists every nugget with filter and per-row Open / Edit / Delete. The danger zone in
Settings can delete all notes at once.

**Settings** — font size (S–XL), panel scale, dark/light/system theme, hotkey
rebinding, badge toggle, autostart. Reduced Motion and High Contrast system
settings are respected. Changes apply live.

**Tray** — pause/resume hover detection, open main window or settings, toggle
autostart, check for updates, quit.

**Your data** — every note is a small JSON "sidecar" file in a hidden `.nuggets`
folder next to the item it describes (a folder's own note travels inside it).
Sidecars are the single source of truth: the app's SQLite index is only a
rebuildable cache, and uninstalling never deletes your notes.

## How it works

Hover detection uses Windows **UI Automation** (`ElementFromPoint`) — no cross-process
memory reads of Explorer's ListView. The overlay panel is a transparent, never-focused
window; the glass look is CSS. Note content always lives in the sidecar files; the
index can be deleted and rebuilt from them at any time.

## Tech stack

- **Backend:** Rust + [Tauri 2](https://tauri.app/); the `windows` crate for Win32 /
  UI Automation behind a platform trait (`DesktopIcons`).
- **Frontend:** webview UI with [TipTap](https://tiptap.dev/) for the rich-text editor.
- **Storage:** sidecar JSON files (source of truth) + a SQLite cache index.

## Building from source

Requires [Rust](https://www.rust-lang.org/tools/install), [Node.js](https://nodejs.org/),
and the Tauri prerequisites for your platform — Windows: WebView2 + MSVC build
tools; macOS: Xcode Command Line Tools.

```bash
# install UI dependencies
cd app/ui
npm install

# run the app in dev mode (from app/src-tauri)
cd ../src-tauri
cargo tauri dev

# build a release installer
cargo tauri build
```

## Platform

Windows 10 and Windows 11. A macOS port (macOS 14+, Apple silicon) is under way:
the codebase is single-branch with platform code behind traits/`#[cfg]`, and CI
compiles and tests every change on both platforms — each CI run also uploads an
ad-hoc-signed arm64 `.dmg` artifact for testing. Badges and icon-selection
targeting are not ported yet.

Beta macOS builds are ad-hoc signed but **not notarized**, so first launch needs
System Settings → Privacy & Security → "Open Anyway". If macOS instead calls the
app "damaged", the copy lost its signature in transit (unzipping a `.app` on a
non-Mac does this) — re-download the `.dmg` and copy the app out of the mounted
image rather than moving an extracted `.app` between machines. Hover also needs
the Accessibility permission (System Settings → Privacy & Security →
Accessibility), which the app requests on first run; grant it, then relaunch.

## Security

Found a vulnerability? Please **don't** open a public issue — see
[docs/SECURITY.md](docs/SECURITY.md) for how to report it and for the project's
secret-hygiene rules (what never to commit, how update-signing keys are handled).

## License

All rights reserved (no license granted yet). Licensing to be decided.
