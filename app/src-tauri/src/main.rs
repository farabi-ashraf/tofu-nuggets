#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod badges;
mod desktop;
mod hover;
mod storage;

use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let overlay = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("overlay.html".into()),
            )
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

            // Rounded corners + dark titlebar hints via DWM (Win11).
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::Graphics::Dwm::{
                    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
                    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
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

            hover::spawn(app.handle().clone());
            badges::spawn();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
