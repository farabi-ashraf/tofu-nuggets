//! Opening link targets from a nugget: a file/folder in Explorer, or a web
//! URL in the default browser. The `nugget://open?path=…` scheme is decoded
//! on the JS side, so these commands receive clean values.

use std::path::Path;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SHOW_WINDOW_CMD, SW_SHOWNORMAL};

/// Open Explorer at a file (selecting it in its parent) or a folder (opening
/// it). Path comes from a `nugget://` link the user authored.
#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Link target no longer exists:\n{path}"));
    }
    if p.is_dir() {
        shell_open("open", &path, None);
    } else {
        // Select the file inside its parent folder.
        shell_open("open", "explorer.exe", Some(&format!("/select,\"{path}\"")));
    }
    Ok(())
}

/// Open an http(s) URL in the default browser.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Only http(s) links can be opened".into());
    }
    shell_open("open", &url, None);
    Ok(())
}

fn shell_open(verb: &str, file: &str, params: Option<&str>) {
    let verb = HSTRING::from(verb);
    let file = HSTRING::from(file);
    let params = params.map(HSTRING::from);
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            params
                .as_ref()
                .map(|p| PCWSTR(p.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            PCWSTR::null(),
            SHOW_WINDOW_CMD(SW_SHOWNORMAL.0),
        );
    }
}
