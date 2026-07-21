// Main window: the "all nuggets" list from the SQLite index.

import "./theme.js";
import { hotkeyParts, IS_MAC } from "./hotkeys.js";

// The file manager and the way the app is removed are named differently per
// platform; nothing else in this window is platform-specific.
const REVEAL_LABEL = IS_MAC ? "Reveal in Finder" : "Show in Explorer";
const REMOVAL_PHRASE = IS_MAC
  ? "stay on disk if you move the app to the Trash"
  : "stay on disk after uninstalling";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const listEl = document.getElementById("list");
const emptyEl = document.getElementById("empty");
const filterEl = document.getElementById("filter");
const hotkeyKeysEl = document.getElementById("hotkey-keys");

let entries = [];

// Render the current global hotkey as <kbd> chips in the empty-state hint, so
// it tracks the user's chosen combination instead of the old hardcoded default.
// Labels come from hotkeys.js so the settings field and this hint agree (and
// name macOS modifiers as ⌘/⌥/⌃ rather than Windows ones).
function renderHotkey(combo) {
  if (!hotkeyKeysEl) return;
  const nodes = [];
  hotkeyParts(combo || "ctrl+shift+n").forEach((label, i) => {
    if (i) nodes.push("+");
    const kbd = document.createElement("kbd");
    kbd.textContent = label;
    nodes.push(kbd);
  });
  hotkeyKeysEl.replaceChildren(...nodes);
}

function when(ms) {
  if (!ms) return "";
  const d = new Date(ms);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay
    ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleDateString();
}

function row(entry) {
  const li = document.createElement("li");
  li.className = "nugget-row";
  li.innerHTML = `
    <div class="row-main">
      <div class="row-title">
        <span class="name"></span>
        <span class="when"></span>
      </div>
      <div class="row-path"></div>
      <div class="row-preview"></div>
    </div>
    <div class="row-actions">
      <button data-act="open" title="${REVEAL_LABEL}">Open</button>
      <button data-act="edit" title="Edit note">Edit</button>
      <button data-act="del" class="danger" title="Delete note">Delete</button>
    </div>`;
  li.querySelector(".name").textContent = entry.name;
  li.querySelector(".when").textContent = when(entry.modified_ms);
  const p = li.querySelector(".row-path");
  p.textContent = entry.path;
  p.title = entry.path;
  li.querySelector(".row-preview").textContent = entry.preview || "(empty note)";
  li.querySelector('[data-act="open"]').addEventListener("click", () => {
    invoke("open_in_explorer", { path: entry.path }).catch(() => {});
  });
  li.querySelector('[data-act="edit"]').addEventListener("click", () => {
    invoke("edit_nugget", { path: entry.path }).catch(() => {});
  });
  // Two-step confirm: first click arms the button, second click deletes.
  const delBtn = li.querySelector('[data-act="del"]');
  let armTimer = null;
  delBtn.addEventListener("click", () => {
    if (!delBtn.classList.contains("armed")) {
      delBtn.classList.add("armed");
      delBtn.textContent = "Sure?";
      armTimer = setTimeout(() => {
        delBtn.classList.remove("armed");
        delBtn.textContent = "Delete";
      }, 3000);
      return;
    }
    clearTimeout(armTimer);
    // List refreshes via the nuggets:changed emit.
    invoke("delete_nugget", { path: entry.path }).catch(() => {});
  });
  return li;
}

function render() {
  const q = filterEl.value.trim().toLowerCase();
  const shown = q
    ? entries.filter(
        (e) =>
          e.name.toLowerCase().includes(q) ||
          e.preview.toLowerCase().includes(q) ||
          e.path.toLowerCase().includes(q),
      )
    : entries;
  listEl.replaceChildren(...shown.map(row));
  emptyEl.hidden = entries.length !== 0;
}

async function reload() {
  try {
    entries = await invoke("list_nuggets");
  } catch (e) {
    entries = [];
  }
  render();
}

filterEl.addEventListener("input", render);
listen("nuggets:changed", reload);
reload();

const footHint = document.getElementById("foot-hint");
if (footHint) {
  footHint.textContent =
    `Notes are stored beside your files and ${REMOVAL_PHRASE}. ` +
    "To remove them all, use Settings → Delete all notes.";
}

// Seed the hotkey hint immediately, then sync from settings and keep it live.
renderHotkey();
invoke("get_settings")
  .then((s) => renderHotkey(s.hotkey))
  .catch(() => {});
listen("settings:changed", (e) => renderHotkey(e.payload && e.payload.hotkey));
