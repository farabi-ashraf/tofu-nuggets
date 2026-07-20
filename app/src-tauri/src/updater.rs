//! "Check for updates" flow (docs/V0.1.1.md B1). Driven from the tray in
//! Rust so it needs no webview window and no extra capability surface. Checks
//! the signed `latest.json` on the GitHub release, confirms via a native
//! dialog, downloads + installs the update, then restarts.
//!
//! Window/dialog work is only reliable off the async command threads, so the
//! whole flow runs on a plain worker thread (same rule as `tray::on_worker`);
//! the async updater calls are driven with `block_on`.

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::logfile;

/// Tray entry point. `user_initiated` = the user clicked "Check for updates…",
/// so surface the up-to-date and error outcomes in a dialog. A background
/// check would pass `false` to stay silent unless an update is found.
pub fn check(app: &AppHandle, user_initiated: bool) {
    let app = app.clone();
    std::thread::spawn(move || run(&app, user_initiated));
}

fn run(app: &AppHandle, user_initiated: bool) {
    match tauri::async_runtime::block_on(async { app.updater()?.check().await }) {
        Ok(Some(update)) => prompt_and_install(app, update),
        Ok(None) => {
            logfile::log(app, "updater: up to date");
            if user_initiated {
                let v = app.package_info().version.to_string();
                notify(
                    app,
                    "Up to date",
                    &format!("Tofu Nuggets {v} is the latest version."),
                    MessageDialogKind::Info,
                );
            }
        }
        Err(e) => {
            logfile::log(app, &format!("updater: check failed: {e}"));
            if user_initiated {
                notify(
                    app,
                    "Update check failed",
                    &format!("Could not check for updates:\n{e}"),
                    MessageDialogKind::Error,
                );
            }
        }
    }
}

fn prompt_and_install(app: &AppHandle, update: Update) {
    logfile::log(app, &format!("updater: {} available", update.version));
    let notes = update.body.clone().unwrap_or_default();
    let msg = format!(
        "Tofu Nuggets {} is available (you have {}).\n\n{}\n\nInstall now? The app will restart.",
        update.version,
        update.current_version,
        notes.trim(),
    );
    let install = app
        .dialog()
        .message(msg)
        .title("Update available")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".into(),
            "Later".into(),
        ))
        .blocking_show();
    if !install {
        logfile::log(app, "updater: user declined");
        return;
    }

    match tauri::async_runtime::block_on(async {
        update.download_and_install(|_, _| {}, || {}).await
    }) {
        Ok(()) => {
            logfile::log(app, "updater: installed, restarting");
            app.restart();
        }
        Err(e) => {
            logfile::log(app, &format!("updater: install failed: {e}"));
            notify(
                app,
                "Update failed",
                &format!("The update could not be installed:\n{e}"),
                MessageDialogKind::Error,
            );
        }
    }
}

fn notify(app: &AppHandle, title: &str, msg: &str, kind: MessageDialogKind) {
    app.dialog()
        .message(msg)
        .title(title)
        .kind(kind)
        .blocking_show();
}
