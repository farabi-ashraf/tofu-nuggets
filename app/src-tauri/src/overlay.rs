//! Overlay panel window: created on demand, destroyed after idle to release
//! WebView2's ~380 MB process tree (docs/ARCHITECTURE.md performance budget).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "overlay";

/// Hide the panel immediately (✕ button / after ✎ Edit). A command instead of
/// the `core:window:allow-hide` permission keeps the capability surface small
/// (docs/V0.1.1.md A3).
#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
}

pub fn get_or_create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(win) = app.get_webview_window(LABEL) {
        return Ok(win);
    }
    match create(app) {
        Ok(w) => Ok(w),
        Err(e) => {
            eprintln!("overlay: create failed: {e}");
            Err(e)
        }
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let overlay = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("Tofu Nuggets Overlay")
        .inner_size(340.0, 240.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .background_color(tauri::utils::config::Color(0, 0, 0, 0))
        .build()?;

    // Never steal focus from whatever the user is doing.
    overlay.set_focusable(false)?;

    // Force the WebView2 canvas fully transparent; Tauri's transparent
    // flag alone leaves an opaque theme-colored background behind the
    // page (observed on WebView2 / Win11).
    #[cfg(target_os = "windows")]
    overlay.with_webview(|webview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Controller2, COREWEBVIEW2_COLOR,
        };
        use windows_core_061::Interface;
        let controller = webview.controller();
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            });
        }
    })?;

    // Rounded corners + dark titlebar hints via DWM (Win11). OS blur is
    // impossible for never-activated windows (see ARCHITECTURE.md §2) —
    // the glass look is CSS over genuine transparency.
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
            DWMWCP_ROUND,
        };
        let hwnd = HWND(overlay.hwnd()?.0);
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                std::mem::size_of_val(&dark) as u32,
            );
            let corners = DWMWCP_ROUND.0;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corners as *const _ as _,
                std::mem::size_of_val(&corners) as u32,
            );
        }
    }

    eprintln!("overlay: window created");
    Ok(overlay)
}

/// Destroy the overlay window (and with it the WebView2 processes).
pub fn destroy(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        match win.destroy() {
            Ok(()) => eprintln!("overlay: destroyed (idle release)"),
            Err(e) => eprintln!("overlay: destroy failed: {e}"),
        }
    }
}

pub fn exists(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).is_some()
}
