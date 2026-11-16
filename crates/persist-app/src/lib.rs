//! Persist desktop application (Tauri 2 backend).
//!
//! The desktop app is a thin shell around the same [`persist_core::session::Session`]
//! the CLI uses. All logic lives in `persist-core`; this crate only exposes it
//! to the webview as commands and forwards progress as events.

mod commands;
mod progress;
mod state;

pub use state::AppState;

use tauri::Manager;

/// Entry point used by both `main.rs` and the mobile entry point.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Log to stderr. Honour RUST_LOG, defaulting to a quiet level.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "persist_app_lib=info,persist_core=info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new().map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_config,
            commands::set_backend,
            commands::set_drive_folder,
            commands::get_settings,
            commands::set_settings,
            commands::account,
            commands::connect_google,
            commands::disconnect_google,
            commands::init_repository,
            commands::clone_repository,
            commands::list_roots,
            commands::add_root,
            commands::remove_root,
            commands::update_root,
            commands::plan_backup,
            commands::run_backup,
            commands::list_snapshots,
            commands::get_snapshot,
            commands::diff_local,
            commands::diff_snapshots,
            commands::plan_restore,
            commands::run_restore,
            commands::verify,
            commands::storage_info,
            commands::list_activity,
            commands::cancel_operation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Persist application");
}
