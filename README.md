# Tofu Nuggets

> Hover a desktop icon → see your own notes about it.

Tofu Nuggets is a lightweight desktop overlay for Windows. Hover over any file or
folder icon on your desktop and a glassy panel appears showing context you wrote
yourself — a "nugget." Notes are rich text (todos, links, formatting), created with
a global hotkey, and stored as plain sidecar files next to the item they describe.

It runs quietly in the background: near-zero CPU when idle, hover polling only while
the desktop is the foreground window.

## Status

**v0.1.0 — early, Windows-only, work in progress.** Private preview while core fixes land.

## Features

- **Hover to reveal** — glance a desktop icon, read your note. No clicking.
- **Rich-text nuggets** — todos, links, and formatting via a TipTap editor.
- **Global hotkey** — capture a note without breaking flow.
- **Badge layer** — a subtle marker shows which icons carry a nugget.
- **Sidecar storage** — notes live as files in hidden `.nuggets` folders; they are the
  single source of truth. A SQLite index is only a rebuildable cache.
- **Stays out of the way** — background app with a hard performance budget
  (~0% CPU idle, small RAM footprint, icon count never affects hover cost).

## How it works

Hover detection uses Windows **UI Automation** (`ElementFromPoint`) — no cross-process
memory reads of Explorer's ListView. The overlay panel uses acrylic/vibrancy for the
glassy look. Note content always lives in the sidecar files; the index can be deleted
and rebuilt from them at any time.

## Tech stack

- **Backend:** Rust + [Tauri 2](https://tauri.app/), the `windows` crate for Win32 / UI
  Automation, `window-vibrancy` for acrylic.
- **Frontend:** webview UI with [TipTap](https://tiptap.dev/) for the rich-text editor.
- **Storage:** sidecar JSON files (source of truth) + a SQLite cache index.

## Building from source

Requires [Rust](https://www.rust-lang.org/tools/install), [Node.js](https://nodejs.org/),
and the Tauri prerequisites for Windows (WebView2, MSVC build tools).

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

Windows 10 and Windows 11 only. The hover, DPI, and overlay code is Win32/UIA-specific;
other platforms are out of scope for now.

## License

All rights reserved (no license granted yet). Licensing to be decided.
