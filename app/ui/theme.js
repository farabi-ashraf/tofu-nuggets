// Shared accessibility/theme applier, imported by every window's entry script.
// Reads persisted settings, applies them to <html>, and re-applies live when
// the backend emits `settings:changed` or the OS media preferences change.
//
// Effective rule for motion/contrast: user toggle OR the matching OS setting —
// so we honor the OS, and the toggle can additionally force it on.

const FONT_SCALE = { s: 0.85, m: 1.0, l: 1.2, xl: 1.45 };

const mqDark = matchMedia("(prefers-color-scheme: dark)");
const mqMotion = matchMedia("(prefers-reduced-motion: reduce)");
const mqContrast = matchMedia("(prefers-contrast: more)");
const mqForced = matchMedia("(forced-colors: active)");

let current = null;

function apply(s) {
  current = s;
  const root = document.documentElement;

  root.style.setProperty("--font-scale", FONT_SCALE[s.font_size] ?? 1);
  root.style.setProperty("--panel-scale", s.panel_scale || 1);

  const theme =
    s.theme === "system" ? (mqDark.matches ? "dark" : "light") : s.theme;
  root.setAttribute("data-theme", theme);

  const motion = s.reduced_motion || mqMotion.matches;
  root.setAttribute("data-motion", motion ? "reduced" : "full");

  const contrast = s.high_contrast || mqContrast.matches || mqForced.matches;
  root.setAttribute("data-contrast", contrast ? "high" : "normal");
}

function reapply() {
  if (current) apply(current);
}

// OS-level changes (system theme flip, Reduced Motion, High Contrast).
for (const mq of [mqDark, mqMotion, mqContrast, mqForced]) {
  mq.addEventListener("change", reapply);
}

const tauri = window.__TAURI__;
if (tauri) {
  tauri.core.invoke("get_settings").then(apply).catch(() => {});
  tauri.event.listen("settings:changed", (e) => apply(e.payload));
}

export { apply };
