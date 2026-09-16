mod agent_scan;
mod autosync;
mod commands;
mod db;
mod embed;
mod import;
mod keywords;
mod mcp;
mod models;

/// Thin re-exports used by integration tests to drive the import pipeline directly.
pub mod test_support {
    pub use crate::db::open as open_for_test;
    pub use crate::import::import_zip as import_for_test;
    pub use crate::mcp::dispatch_tool as mcp_tool_for_test;
    pub use crate::agent_scan::import_agent_sessions as scan_agents_for_test;
}

use db::DbState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let path = db::db_path(&data_dir);
            let conn = db::open(&path).expect("failed to open database");
            // Backfill: fix conversations whose updated_at was wrongly set to
            // export/import time instead of actual last-message time.
            let _ = conn.execute(
                "UPDATE conversations
                 SET
                   created_at = COALESCE(
                     created_at,
                     (SELECT MIN(m.created_at) FROM messages m
                      WHERE m.conversation_id = conversations.id AND m.created_at IS NOT NULL)
                   ),
                   updated_at = COALESCE(
                     (SELECT m.created_at FROM messages m
                      WHERE m.conversation_id = conversations.id AND m.created_at IS NOT NULL
                      ORDER BY m.seq DESC LIMIT 1),
                     updated_at
                   )
                 WHERE EXISTS (
                   SELECT 1 FROM messages m WHERE m.conversation_id = conversations.id
                 )",
                [],
            );
            app.manage(DbState(Mutex::new(conn)));
            app.manage(mcp::McpState::default());
            app.manage(autosync::AutoSyncState::default());
            // Comes back on its own if the user left the MCP server switched on.
            mcp::start_if_enabled(&app.handle().clone());
            // Keeps the archive — and so everything MCP serves — from going
            // stale between manual scans.
            autosync::start_if_enabled(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_zip_file,
            commands::list_conversations,
            commands::count_conversations,
            commands::list_models,
            commands::get_conversation,
            commands::list_conversations_by_path,
            commands::search_conversations_by_path,
            commands::delete_conversation,
            commands::list_import_batches,
            commands::delete_import_batch,
            commands::search_all,
            commands::create_favorite,
            commands::list_favorites,
            commands::delete_favorite,
            commands::list_tags,
            commands::create_tag,
            commands::set_favorite_tags,
            commands::suggest_tags,
            commands::auto_organize_favorites,
            commands::search_favorites,
            commands::fix_conversation_timestamps,
            commands::test_embed_connection,
            commands::get_setting,
            commands::set_setting,
            commands::get_embedding_stats,
            commands::generate_embeddings,
            commands::semantic_search,
            commands::scan_agent_sources,
            commands::import_agent_sessions,
            commands::launch_resume_terminal,
            commands::terminal_env,
            mcp::mcp_status,
            mcp::mcp_set_enabled,
            mcp::mcp_set_port,
            mcp::mcp_regenerate_token,
            mcp::mcp_open_enrollment,
            mcp::mcp_cancel_enrollment,
            mcp::mcp_activity,
            autosync::autosync_status,
            autosync::autosync_set_enabled,
            autosync::autosync_set_interval,
            autosync::autosync_run_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
