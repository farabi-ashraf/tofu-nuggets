# Tofu Nuggets

> Hover a desktop icon → see your own notes about it.

Tofu Nuggets is a lightweight desktop overlay for Windows and macOS. Hover over any
file or folder icon on your desktop — or inside a **File Explorer** window (Windows)
or a **Finder** window (macOS) — and a glassy panel appears showing context you wrote
yourself — a "nugget." Notes are rich text (todos, links, formatting), created with a
global hotkey, and stored as plain sidecar files next to the item they describe.

It runs quietly in the background: near-zero CPU when idle, hover polling only while
the desktop or a file-manager window (File Explorer / Finder) is in the foreground.

## Status

**v0.3.0 — beta, Windows + macOS (Apple silicon).** Installers (Windows `.exe`,
macOS `.dmg`) are published on the
[Releases page](https://github.com/farabi-ashraf/tofu-nuggets/releases); installed
copies self-update via the tray's "Check for updates…". macOS builds are beta:
ad-hoc signed, not notarized — see [Platform](#platform) for the first-launch
steps.

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

**Hover panel** — rest the cursor on an annotated item for a moment and the glassy
panel appears beside it with your note rendered read-only; checkboxes are live, links
clickable. Move away and it hides. ✕ closes it immediately, ✎ jumps to the editor.
Works on the desktop and inside file-manager windows — **File Explorer** on Windows,
**Finder** browser windows on macOS (list, column, gallery and icon views), including
the active tab of a multi-tab window.

**Badges** — a small dot marks every icon that carries a nugget, so you know what has
notes without hovering. On **Windows** the dots are drawn by the app on the desktop
and hide under any window that overlaps them. On **macOS** the badge is a Finder tag
named "Nugget" that the app adds to the file, so **Finder** draws the dot for you —
on the desktop *and* inside every Finder window, automatically. Either way, badges
can be turned off in Settings; on macOS that removes the "Nugget" tag from your
files (and re-adds it when you switch badges back on). The tag is visible in Get
Info, and because tags sync via iCloud/Dropbox your badges travel between your Macs.
Pick the badge **colour** in Settings — one of seven, shared by both platforms:
Windows paints the dot in it, macOS colours the "Nugget" tag. The very first time
macOS tags a file, a one-time note explains what the tag is and where to change it.

**Explorer count pill** *(Windows)* — every File Explorer window shows a small glassy
chip in the bottom-right of its file list with the number of items in that folder
that carry a note. It follows the window, tracks the active tab, disappears in
folders with no notes, and is switched off by the same Settings badge toggle and by
tray Pause. **Click it** to mark the annotated items: a dot appears on each one you
can currently see. The dots are a quick snapshot — they clear as soon as you scroll,
move the window, switch folders or click away — and clicking the chip again turns
them off. A dot never gets in the way; clicking a file under one still selects it.

**Main window** — tray icon → "Open Tofu Nuggets" (or launching the app again)
lists every nugget with filter and per-row Open / Edit / Delete. The danger zone in
Settings can delete all notes at once.

**Settings** — font size (S–XL), panel scale, dark/light/system theme, hotkey
rebinding, badge toggle, badge colour (7 shared colours), autostart. Reduced
Motion and High Contrast system settings are respected. Changes apply live.

**Tray** — pause/resume hover detection, open main window or settings, toggle
autostart, check for updates, quit.

**Your data** — every note is a small JSON "sidecar" file in a hidden `.nuggets`
folder next to the item it describes (a folder's own note travels inside it).
Sidecars are the single source of truth: the app's SQLite index is only a
rebuildable cache, and uninstalling never deletes your notes.

## How it works

Hover detection uses **UI Automation** (`ElementFromPoint`) on Windows and the
**Accessibility API** (`AXUIElementCopyElementAtPosition`) on macOS — a hit-test under
the cursor, no cross-process memory reads. On both platforms the hit-test is gated to
the foreground surface (desktop or a file-manager window), so it does no work while
another app is in front. The overlay panel is a transparent, never-focused window; the
glass look is CSS. Note content always lives in the sidecar files; the index can be
deleted and rebuilt from them at any time.

## Tech stack

- **Backend:** Rust + [Tauri 2](https://tauri.app/); the `windows` crate for Win32 /
  UI Automation and the macOS Accessibility API behind a platform trait (`DesktopIcons`).
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

Windows 10 and Windows 11, plus macOS 14+ on Apple silicon (beta). The codebase
is single-branch with platform code behind traits/`#[cfg]`; CI compiles and
tests every change on both platforms, and releases ship both installers from
the same tag. The full feature set — hover panel, hotkey capture (under cursor
or selected icon), badges, editor, main list — runs on both; macOS behavior
testing currently covers macOS 26 hardware, earlier versions via CI only.

Beta macOS builds are ad-hoc signed but **not notarized**, so first launch needs
System Settings → Privacy & Security → "Open Anyway". If macOS instead calls the
app "damaged", the copy lost its signature in transit (unzipping a `.app` on a
non-Mac does this) — re-download the `.dmg` and copy the app out of the mounted
image rather than moving an extracted `.app` between machines.

Hover and the note hotkey both need the Accessibility permission (System Settings
→ Privacy & Security → Accessibility): without it the app cannot see desktop
icons and neither feature does anything. Grant it, then quit and reopen the app —
Settings shows the current status. Because beta builds are ad-hoc signed, macOS
treats each new build as a different app, so the permission has to be granted
again after every update.

## Security

Found a vulnerability? Please **don't** open a public issue — see
[docs/SECURITY.md](docs/SECURITY.md) for how to report it and for the project's
secret-hygiene rules (what never to commit, how update-signing keys are handled).

## License

All rights reserved (no license granted yet). Licensing to be decided.
