pub mod commands;
pub mod font_catalog;
pub mod hwpx_package;
pub mod state;

use commands::attachment::*;
use commands::claw_bridge::*;
use commands::document::*;
use commands::fonts::*;
use commands::llm_proxy::*;
use state::AppState;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = match app.path().app_cache_dir() {
                Ok(dir) => AppState::with_staging_root(dir.join("hop-staging")),
                Err(_) => AppState::new(),
            };
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_local_file,
            list_local_fonts,
            read_local_font,
            open_document_session,
            create_blank_session,
            read_file_bytes,
            write_file_bytes,
            commit_staging_to_original,
            get_startup_file,
            get_hwp_default_app_status,
            set_hwp_default_app,
            inspect_document,
            extract_markdown,
            execute_claw_ops,
            call_llm_http,
            cancel_llm_request,
            read_attachment_bytes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
