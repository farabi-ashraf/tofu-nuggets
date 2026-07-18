// Settings window. Loads current settings, reflects them in the controls, and
// on any change writes the whole object back via set_settings — which persists
// it and emits `settings:changed` so every window (including this one, through
// theme.js) updates live.

import "./theme.js";

const { invoke } = window.__TAURI__.core;

const segFont = document.getElementById("font-size");
const segTheme = document.getElementById("theme");
const panelScale = document.getElementById("panel-scale");
const panelScaleVal = document.getElementById("panel-scale-val");
const badges = document.getElementById("badges");
const reducedMotion = document.getElementById("reduced-motion");
const highContrast = document.getElementById("high-contrast");
const hotkeyEl = document.getElementById("hotkey");
const hotkeyMsg = document.getElementById("hotkey-msg");
const HOTKEY_HINT = hotkeyMsg.textContent;

let settings = null;

function reflect() {
  for (const b of segFont.children) {
    b.classList.toggle("active", b.dataset.value === settings.font_size);
  }
  for (const b of segTheme.children) {
    b.classList.toggle("active", b.dataset.value === settings.theme);
  }
  panelScale.value = settings.panel_scale;
  panelScaleVal.textContent = `${Number(settings.panel_scale).toFixed(2)}×`;
  badges.checked = settings.badges;
  reducedMotion.checked = settings.reduced_motion;
  highContrast.checked = settings.high_contrast;
  if (!hotkeyEl.classList.contains("capturing")) hotkeyEl.value = settings.hotkey;
}

async function commit() {
  reflect();
  try {
    await invoke("set_settings", { settings });
  } catch (e) {
    // Backend refused (e.g. hotkey already taken elsewhere): resync from the
    // still-active stored settings and surface the reason.
    hotkeyMsg.textContent = String(e);
    try {
      settings = await invoke("get_settings");
    } catch {}
    reflect();
  }
}

segFont.addEventListener("click", (e) => {
  const b = e.target.closest("button[data-value]");
  if (!b) return;
  settings.font_size = b.dataset.value;
  commit();
});

segTheme.addEventListener("click", (e) => {
  const b = e.target.closest("button[data-value]");
  if (!b) return;
  settings.theme = b.dataset.value;
  commit();
});

panelScale.addEventListener("input", () => {
  settings.panel_scale = parseFloat(panelScale.value);
  panelScaleVal.textContent = `${settings.panel_scale.toFixed(2)}×`;
});
panelScale.addEventListener("change", () => {
  settings.panel_scale = parseFloat(panelScale.value);
  commit();
});

badges.addEventListener("change", () => {
  settings.badges = badges.checked;
  commit();
});
reducedMotion.addEventListener("change", () => {
  settings.reduced_motion = reducedMotion.checked;
  commit();
});
highContrast.addEventListener("change", () => {
  settings.high_contrast = highContrast.checked;
  commit();
});

// --- Hotkey capture: click the field, press a combination. Needs Ctrl, Alt,
// or Win plus a normal key so it stays a sane global shortcut. Esc cancels.
function hotkeyFromEvent(e) {
  const k = e.key.toLowerCase();
  if (["control", "shift", "alt", "meta"].includes(k)) return null; // modifier alone
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return null;
  let key = null;
  if (/^[a-z0-9]$/.test(k)) key = k;
  else if (/^f([1-9]|1[0-2])$/.test(k)) key = k;
  else if (k === " ") key = "space";
  if (!key) return null;
  const mods = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.metaKey) mods.push("super");
  if (e.shiftKey) mods.push("shift");
  return [...mods, key].join("+");
}

hotkeyEl.addEventListener("focus", () => {
  hotkeyEl.classList.add("capturing");
  hotkeyEl.value = "press keys…";
  hotkeyMsg.textContent = HOTKEY_HINT;
});

hotkeyEl.addEventListener("blur", () => {
  hotkeyEl.classList.remove("capturing");
  hotkeyEl.value = settings ? settings.hotkey : "";
});

hotkeyEl.addEventListener("keydown", (e) => {
  e.preventDefault();
  if (e.key === "Escape") {
    hotkeyEl.blur();
    return;
  }
  const combo = hotkeyFromEvent(e);
  if (!combo) return; // keep capturing until a valid combination lands
  settings.hotkey = combo;
  hotkeyEl.classList.remove("capturing");
  hotkeyEl.blur();
  commit();
});

invoke("get_settings")
  .then((s) => {
    settings = s;
    reflect();
  })
  .catch((e) => console.error("get_settings failed", e));
