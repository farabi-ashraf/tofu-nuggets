// Nugget editor: TipTap over the sidecar HTML. The Rust side opens this
// window with a target item; we pull it on load (same pattern as the
// overlay) and save via the save_nugget command. File/folder links come
// from the picker buttons or OS drag-drop (Tauri drag-drop event; HTML5
// drop never fires) — both feed the same nugget:// insert pipeline.

import "./theme.js";
import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import Placeholder from "@tiptap/extension-placeholder";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const nameEl = document.getElementById("icon-name");
const pathEl = document.getElementById("icon-path");
const saveState = document.getElementById("save-state");

let currentPath = null;
let dirty = false;

const editor = new Editor({
  element: document.getElementById("editor"),
  extensions: [
    StarterKit,
    // nugget:// must be allowlisted or TipTap's URI validation strips file
    // links to href="" the next time a note is opened and saved.
    Link.configure({
      openOnClick: false,
      protocols: ["nugget"],
      isAllowedUri: (url, ctx) => url.startsWith("nugget://") || ctx.defaultValidate(url),
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    Placeholder.configure({
      placeholder: "Why does this file exist? Notes, todos, links…",
    }),
  ],
  content: "",
  onUpdate() {
    dirty = true;
    saveState.textContent = "unsaved changes";
  },
  onTransaction() {
    refreshToolbar();
  },
});

function refreshToolbar() {
  for (const btn of document.querySelectorAll(".toolbar button")) {
    const cmd = btn.dataset.cmd;
    const active =
      (cmd === "bold" && editor.isActive("bold")) ||
      (cmd === "italic" && editor.isActive("italic")) ||
      (cmd === "bulletList" && editor.isActive("bulletList")) ||
      (cmd === "taskList" && editor.isActive("taskList")) ||
      (cmd === "link" && editor.isActive("link"));
    btn.classList.toggle("active", active);
  }
}

function runCommand(cmd) {
  const chain = editor.chain().focus();
  switch (cmd) {
    case "bold":
      chain.toggleBold().run();
      break;
    case "italic":
      chain.toggleItalic().run();
      break;
    case "bulletList":
      chain.toggleBulletList().run();
      break;
    case "taskList":
      chain.toggleTaskList().run();
      break;
    case "link": {
      if (editor.isActive("link")) {
        chain.unsetLink().run();
        break;
      }
      const raw = window.prompt("Link URL:");
      if (!raw) break;
      const url = normalizeUrl(raw);
      if (url) chain.setLink({ href: url }).run();
      else saveState.textContent = `not a valid link: ${raw}`;
      break;
    }
    case "linkFile":
      linkTarget(false);
      break;
    case "linkFolder":
      linkTarget(true);
      break;
  }
}

// Normalize a user-entered web link: bare "example.com" gets https://
// prefixed, anything with a scheme passes through, garbage returns null.
// Without this, scheme-less hrefs are dead in the overlay (it only opens
// http(s) and nugget: links).
function normalizeUrl(raw) {
  const url = raw.trim();
  if (!url) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(url)) return url;
  if (/^\S+\.\S{2,}/.test(url)) return `https://${url}`;
  return null;
}

// Insert a nugget:// link naming a file/folder path; clicking that link
// (in the hover panel) opens Explorer at the target. Shared by the
// picker buttons and drag-drop.
function insertPathLink(path) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  const href = `nugget://open?path=${encodeURIComponent(path)}`;
  editor
    .chain()
    .focus()
    .insertContent([
      { type: "text", text: name, marks: [{ type: "link", attrs: { href } }] },
      { type: "text", text: " " },
    ])
    .run();
}

// Pick a file/folder via the native dialog and link it.
async function linkTarget(directory) {
  let selected;
  try {
    selected = await openDialog({ multiple: false, directory });
  } catch (e) {
    saveState.textContent = `picker failed: ${e}`;
    return;
  }
  if (!selected) return;
  insertPathLink(Array.isArray(selected) ? selected[0] : selected);
}

// Files/folders dropped onto the window become nugget:// links, same
// pipeline as the picker buttons. Tauri intercepts native drag-drop
// (dragDropEnabled default) and delivers OS paths via its own event —
// HTML5 drop events never fire, and this API is identical on macOS,
// so keep this Tauri-only (no platform code here).
const { getCurrentWebview } = window.__TAURI__.webview;
getCurrentWebview().onDragDropEvent((event) => {
  const kind = event.payload.type;
  if (kind === "enter" || kind === "over") {
    document.body.classList.add("drop-target");
  } else if (kind === "leave") {
    document.body.classList.remove("drop-target");
  } else if (kind === "drop") {
    document.body.classList.remove("drop-target");
    for (const path of event.payload.paths || []) insertPathLink(path);
  }
});

document.querySelector(".toolbar").addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-cmd]");
  if (btn) runCommand(btn.dataset.cmd);
});

// Ctrl/Cmd-click a link in the editor to follow it (plain click edits text).
document.getElementById("editor").addEventListener("click", (e) => {
  const a = e.target.closest("a");
  if (!a || !(e.ctrlKey || e.metaKey)) return;
  e.preventDefault();
  openLink(a.getAttribute("href"));
});

function openLink(href) {
  if (!href) return;
  if (href.startsWith("nugget://")) {
    let path = "";
    try {
      path = decodeURIComponent(new URL(href).searchParams.get("path") || "");
    } catch {
      return;
    }
    invoke("open_in_explorer", { path }).catch((err) => {
      saveState.textContent = String(err);
    });
  } else if (/^https?:/i.test(href)) {
    invoke("open_external", { url: href }).catch(() => {});
  } else if (/^\S+\.\S{2,}/.test(href) && !href.includes(":")) {
    invoke("open_external", { url: `https://${href}` }).catch(() => {});
  }
}

function load(payload) {
  currentPath = payload.path;
  nameEl.textContent = payload.name;
  pathEl.textContent = payload.path;
  pathEl.title = payload.path;
  editor.commands.setContent(payload.html || "");
  dirty = false;
  saveState.textContent = "";
  editor.commands.focus("end");
}

async function save() {
  if (!currentPath) return;
  try {
    // Backend treats an empty note as removal (sidecar deleted, badge gone).
    const removed = await invoke("save_nugget", { path: currentPath, html: editor.getHTML() });
    dirty = false;
    saveState.textContent = removed ? "note removed" : "saved";
  } catch (e) {
    saveState.textContent = `save failed: ${e}`;
  }
}

async function saveAndClose() {
  if (dirty) await save();
  // Surface close failures (e.g. a missing window permission) instead of
  // silently staying open.
  getCurrentWindow()
    .close()
    .catch((e) => {
      saveState.textContent = `close failed: ${e}`;
    });
}

document.getElementById("save-btn").addEventListener("click", save);
document.getElementById("close-btn").addEventListener("click", saveAndClose);

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    saveAndClose();
  } else if (e.ctrlKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    save();
  } else if (e.ctrlKey && e.key.toLowerCase() === "k") {
    e.preventDefault();
    runCommand("link");
  }
});

invoke("get_current_edit")
  .then((payload) => {
    if (payload) load(payload);
  })
  .catch(() => {});

listen("edit:show", (event) => load(event.payload));
