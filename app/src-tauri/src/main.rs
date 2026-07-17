#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod badges;
mod desktop;
mod hover;
mod storage;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

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
            .build()?;

            // Never steal focus from whatever the user is doing.
            overlay.set_focusable(false)?;

            #[cfg(target_os = "windows")]
            window_vibrancy::apply_acrylic(&overlay, Some((30, 30, 40, 160)))
                .expect("acrylic unsupported");

            hover::spawn(app.handle().clone());
            badges::spawn();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
