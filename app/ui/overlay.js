// Overlay panel renderer. The hover engine (Rust) emits `nugget:show` with
// { name, path, html } right before it positions and shows this window.

const nameEl = document.getElementById("icon-name");
const pathEl = document.getElementById("icon-path");
const noteEl = document.getElementById("note-content");

if (!window.__TAURI__) {
  nameEl.textContent = "tauri api missing";
  throw new Error("__TAURI__ not injected");
}
const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

function render({ name, path, html }) {
  currentPath = path;
  nameEl.textContent = name;
  pathEl.textContent = path;
  pathEl.title = path;
  noteEl.replaceChildren(sanitize(html));
}

// The window may be created lazily right before a show event that fires
// while this page is still loading — pull the current payload on startup.
invoke("get_current_nugget")
  .then((payload) => {
    if (payload) render(payload);
  })
  .catch(() => {});

let currentPath = null;

// Click a link in the note: nugget:// opens Explorer at the target, http(s)
// opens the browser. Both are backend commands (the panel can't navigate).
noteEl.addEventListener("click", (e) => {
  const a = e.target.closest("a");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href") || "";
  if (href.startsWith("nugget://")) {
    let path = "";
    try {
      path = decodeURIComponent(new URL(href).searchParams.get("path") || "");
    } catch {
      return;
    }
    invoke("open_in_explorer", { path }).catch(() => {});
  } else if (/^https?:/i.test(href)) {
    invoke("open_external", { url: href }).catch(() => {});
  }
});

// Todo checkboxes are interactive in the panel; persist toggles back to disk.
noteEl.addEventListener("change", (e) => {
  if (e.target.matches('input[type="checkbox"]')) {
    persistCurrentHtml();
  }
});

function persistCurrentHtml() {
  if (!currentPath) return;
  // Reflect the checkbox DOM state into the markup TipTap will re-parse.
  noteEl.querySelectorAll('input[type="checkbox"]').forEach((box) => {
    const li = box.closest("li");
    if (li) li.setAttribute("data-checked", box.checked ? "true" : "false");
    if (box.checked) box.setAttribute("checked", "checked");
    else box.removeAttribute("checked");
  });
  invoke("save_nugget", { path: currentPath, html: noteEl.innerHTML }).catch(() => {});
}

// Nugget HTML comes from the user's own sidecar files, but sanitize anyway:
// strip script/style/iframe and inline event handlers.
function sanitize(html) {
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  tpl.content.querySelectorAll("script, style, iframe, object, embed").forEach((n) => n.remove());
  tpl.content.querySelectorAll("*").forEach((el) => {
    [...el.attributes].forEach((attr) => {
      if (attr.name.startsWith("on") || (attr.name === "href" && attr.value.trim().toLowerCase().startsWith("javascript:"))) {
        el.removeAttribute(attr.name);
      }
    });
  });
  return tpl.content;
}

listen("nugget:show", (event) => render(event.payload));
