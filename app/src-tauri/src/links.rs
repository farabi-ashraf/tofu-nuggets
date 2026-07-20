//! Opening link targets from a nugget: a file/folder in the system file
//! manager (Explorer/Finder), or a web URL in the default browser. The
//! `nugget://open?path=…` scheme is decoded on the JS side, so these commands
//! receive clean values. Command names keep the historical `explorer` wording
//! on all platforms — they are a frontend contract (see GLOSSARY).

use std::path::Path;

#[cfg(windows)]
use windows::core::{HSTRING, PCWSTR};
#[cfg(windows)]
use windows::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{SHOW_WINDOW_CMD, SW_SHOWNORMAL};

/// Open the file manager at a file (selecting it in its parent) or a folder
/// (opening it). Path comes from a `nugget://` link the user authored.
#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Link target no longer exists:\n{path}"));
    }
    #[cfg(windows)]
    if p.is_dir() {
        shell_open("open", &path, None);
    } else {
        // Select the file inside its parent folder.
        shell_open("open", "explorer.exe", Some(&format!("/select,\"{path}\"")));
    }
    #[cfg(target_os = "macos")]
    {
        // `open <dir>` opens Finder there; `open -R <file>` reveals the file
        // selected in its parent — the /select, equivalent.
        let mut cmd = std::process::Command::new("open");
        if p.is_dir() {
            cmd.arg(&path);
        } else {
            cmd.arg("-R").arg(&path);
        }
        cmd.spawn().map_err(|e| e.to_string())?;
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
    #[cfg(windows)]
    shell_open("open", &url, None);
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
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
