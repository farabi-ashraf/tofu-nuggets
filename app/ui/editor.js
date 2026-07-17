// Nugget editor: TipTap over the sidecar HTML. The Rust side opens this
// window with a target item; we pull it on load (same pattern as the
// overlay) and save via the save_nugget command.

import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import Placeholder from "@tiptap/extension-placeholder";

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
    Link.configure({ openOnClick: false }),
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
      const url = window.prompt("Link URL:");
      if (url) chain.setLink({ href: url }).run();
      break;
    }
  }
}

document.querySelector(".toolbar").addEventListener("click", (e) => {
  const btn = e.target.closest("button[data-cmd]");
  if (btn) runCommand(btn.dataset.cmd);
});

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
    await invoke("save_nugget", { path: currentPath, html: editor.getHTML() });
    dirty = false;
    saveState.textContent = "saved";
  } catch (e) {
    saveState.textContent = `save failed: ${e}`;
  }
}

async function saveAndClose() {
  if (dirty) await save();
  getCurrentWindow().close();
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
