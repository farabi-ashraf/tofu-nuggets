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
}

async function commit() {
  reflect();
  try {
    await invoke("set_settings", { settings });
  } catch (e) {
    console.error("set_settings failed", e);
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

invoke("get_settings")
  .then((s) => {
    settings = s;
    reflect();
  })
  .catch((e) => console.error("get_settings failed", e));
