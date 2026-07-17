// Main window: the "all nuggets" list from the SQLite index.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const listEl = document.getElementById("list");
const emptyEl = document.getElementById("empty");
const filterEl = document.getElementById("filter");

let entries = [];

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
      <button data-act="open" title="Show in Explorer">Open</button>
      <button data-act="edit" title="Edit note">Edit</button>
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
