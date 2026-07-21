// Badge layer renderer (macOS). The Rust badge thread emits `badges:update`
// every refresh tick with an array of { x, y } dot centers in CSS px
// (= points, window-relative). The emit is unconditional — this page may
// still be loading when the first set is computed — so unchanged payloads
// are skipped here instead of in Rust.

if (!window.__TAURI__) {
  throw new Error("__TAURI__ not injected");
}
const { listen } = window.__TAURI__.event;

const layer = document.getElementById("layer");
let last = "";

listen("badges:update", ({ payload }) => {
  const key = JSON.stringify(payload);
  if (key === last) return;
  last = key;
  layer.replaceChildren(
    ...payload.map(({ x, y }) => {
      const d = document.createElement("div");
      d.className = "dot";
      d.style.left = `${x}px`;
      d.style.top = `${y}px`;
      return d;
    }),
  );
});
