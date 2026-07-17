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
