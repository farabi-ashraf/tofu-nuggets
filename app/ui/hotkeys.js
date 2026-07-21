// Hotkey capture and display, shared by the settings window (capture + field)
// and the main window (hint chips) so both name the same combination the same
// way.
//
// Two macOS traps drove this module out of settings.js:
//   - Option+<letter> produces a composed character in `event.key` ("Ω" for
//     Option+Z), so key-based capture silently rejected most combinations.
//     `event.code` is the physical key and survives every modifier.
//   - The same modifier has different names per platform: `super` is the
//     Windows key on Windows and Command on macOS, so one label table is
//     always wrong somewhere.
//
// The stored value stays platform-neutral tauri shortcut syntax
// ("ctrl+shift+n"); only the presentation differs.

const IS_MAC = navigator.userAgent.includes("Mac");

const LABELS = IS_MAC
  ? {
      ctrl: "⌃",
      control: "⌃",
      shift: "⇧",
      alt: "⌥",
      super: "⌘",
      meta: "⌘",
      space: "Space",
    }
  : {
      ctrl: "Ctrl",
      control: "Ctrl",
      shift: "Shift",
      alt: "Alt",
      super: "Win",
      meta: "Win",
      space: "Space",
    };

/// Display labels for each element of a stored combination.
export function hotkeyParts(combo) {
  return (combo || "")
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => {
      const low = p.toLowerCase();
      return (
        LABELS[low] || (low.length === 1 ? low.toUpperCase() : p[0].toUpperCase() + p.slice(1))
      );
    });
}

/// One-line form: macOS writes ⌃⇧N with no separators, Windows Ctrl+Shift+N.
export function prettyHotkey(combo) {
  return hotkeyParts(combo).join(IS_MAC ? "" : "+");
}

/// Physical key → the name tauri's shortcut parser expects. Returns null for
/// keys that are not usable on their own (modifiers, punctuation that differs
/// per layout).
function keyFromCode(code) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-2])$/.test(code)) return code.toLowerCase();
  if (code === "Space") return "space";
  return null;
}

/// A keydown event → stored combination, or null while the combination is not
/// yet valid (modifier alone, no modifier, unusable key). Requires a
/// non-shift modifier so the result is a sane global shortcut.
export function hotkeyFromEvent(e) {
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return null;
  const key = keyFromCode(e.code);
  if (!key) return null;
  const mods = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.metaKey) mods.push("super");
  if (e.shiftKey) mods.push("shift");
  return [...mods, key].join("+");
}

export { IS_MAC };
