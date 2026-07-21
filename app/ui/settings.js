// Settings window. Loads current settings, reflects them in the controls, and
// on any change writes the whole object back via set_settings — which persists
// it and emits `settings:changed` so every window (including this one, through
// theme.js) updates live.

import "./theme.js";
import { hotkeyFromEvent, prettyHotkey, IS_MAC } from "./hotkeys.js";

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
// macOS names its modifiers differently, and Command combinations are mostly
// already owned by the system or Finder (⌘⇧N makes a new folder), so steer
// towards Control/Option there.
const HOTKEY_HINT = IS_MAC
  ? "Use ⌃ Control or ⌥ Option plus a letter, digit, or F-key. Most ⌘ combinations are already taken by macOS."
  : hotkeyMsg.textContent;
hotkeyMsg.textContent = HOTKEY_HINT;

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
  if (!hotkeyEl.classList.contains("capturing")) {
    hotkeyEl.value = prettyHotkey(settings.hotkey);
  }
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

// --- Hotkey capture: click the field, press a combination (see hotkeys.js for
// the capture rules). Esc cancels.
hotkeyEl.addEventListener("focus", () => {
  hotkeyEl.classList.add("capturing");
  hotkeyEl.value = "press keys…";
  hotkeyMsg.textContent = HOTKEY_HINT;
});

hotkeyEl.addEventListener("blur", () => {
  hotkeyEl.classList.remove("capturing");
  hotkeyEl.value = settings ? prettyHotkey(settings.hotkey) : "";
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

// --- Accessibility permission (macOS). Without it the AX lookups behind
// hover and hotkey targeting all fail, which otherwise looks like the app
// being broken. `null` means the platform needs no grant (Windows) and the
// whole section stays hidden.
const accessGroup = document.getElementById("access-group");
const accessMsg = document.getElementById("access-msg");
const accessOpen = document.getElementById("access-open");

async function refreshAccess() {
  let granted = null;
  try {
    granted = await invoke("accessibility_status");
  } catch {
    return;
  }
  if (granted === null) return;
  accessGroup.hidden = false;
  accessOpen.hidden = granted;
  accessMsg.textContent = granted
    ? "Granted — hover and the note hotkey can find desktop icons."
    : "Not granted. Hover and the note hotkey cannot find desktop icons until " +
      "you allow Tofu Nuggets under Privacy & Security → Accessibility, then " +
      "quit and reopen the app. Beta builds are signed per build, so each new " +
      "build has to be allowed again.";
}

accessOpen.addEventListener("click", () => {
  invoke("open_accessibility_pane").catch(() => {});
});

// The grant happens outside the app, so re-check whenever this window is
// looked at again rather than only on load.
refreshAccess();
window.addEventListener("focus", refreshAccess);

// --- Danger zone: delete all notes. Two-step confirm (arm -> "Sure?", 3 s
// disarm) mirroring the per-row delete in the main window, since this wipes
// every sidecar and cannot be undone.
const deleteAll = document.getElementById("delete-all");
const deleteAllMsg = document.getElementById("delete-all-msg");
const DELETE_HINT = deleteAllMsg.textContent;
let disarm = null;

deleteAll.addEventListener("click", async () => {
  if (!deleteAll.classList.contains("armed")) {
    deleteAll.classList.add("armed");
    deleteAll.textContent = "Delete everything? Click again to confirm";
    disarm = setTimeout(() => {
      deleteAll.classList.remove("armed");
      deleteAll.textContent = "Delete all notes…";
    }, 3000);
    return;
  }
  clearTimeout(disarm);
  deleteAll.classList.remove("armed");
  deleteAll.disabled = true;
  deleteAll.textContent = "Deleting…";
  try {
    const n = await invoke("delete_all_nuggets");
    deleteAllMsg.textContent = `Deleted ${n} note${n === 1 ? "" : "s"}.`;
  } catch (e) {
    deleteAllMsg.textContent = `Could not delete notes: ${e}`;
  } finally {
    deleteAll.disabled = false;
    deleteAll.textContent = "Delete all notes…";
    setTimeout(() => {
      deleteAllMsg.textContent = DELETE_HINT;
    }, 4000);
  }
});

invoke("get_settings")
  .then((s) => {
    settings = s;
    reflect();
  })
  .catch((e) => console.error("get_settings failed", e));
