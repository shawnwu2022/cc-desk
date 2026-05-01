mod pty;
mod commands;
mod store;
mod checks;
mod mcp;
mod updater;
mod logger;

use std::sync::LazyLock;
use std::sync::Mutex;

/// 全局缓存环境检查结果（setup 前执行，仅一次）
static CHECK_RESULTS: LazyLock<Mutex<Vec<checks::CheckResult>>> = LazyLock::new(|| {
    let result = checks::run_checks();
    for failed in result.failed_checks() {
        log::error!("[Check Failed] {}: {}", failed.name, failed.message);
    }
    if result.all_passed() {
        log::info!("Environment checks passed");
    }
    Mutex::new(result.checks)
});

/// 获取缓存的检查结果
pub fn get_check_results() -> Vec<checks::CheckResult> {
    CHECK_RESULTS.lock().unwrap().clone()
}

/// 重新运行检查并更新缓存
pub fn rerun_checks() -> Vec<checks::CheckResult> {
    let result = checks::run_checks();
    for failed in result.failed_checks() {
        log::error!("[Check Failed] {}: {}", failed.name, failed.message);
    }
    if result.all_passed() {
        log::info!("Environment checks passed");
    }
    let mut cache = CHECK_RESULTS.lock().unwrap();
    *cache = result.checks.clone();
    result.checks
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                log::info!("Window close requested, cleaning up PTYs...");
                if let Some(manager) = pty::get_pty_manager() {
                    manager.kill_all();
                }
            }
        })
        .setup(|app| {
            logger::init();

            pty::init_pty_manager(app.handle().clone());
            log::info!("PTY manager initialized");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_home_data,
            commands::get_check_results,
            commands::run_checks,
            commands::pty_spawn,
            commands::pty_input,
            commands::pty_resize,
            commands::pty_kill,
            commands::pty_kill_all,
            commands::get_projects,
            commands::get_project_info,
            commands::get_sessions,
            commands::get_session_count,
            commands::get_all_recent_sessions,
            commands::get_session_details,
            commands::search_session_messages,
            commands::get_app_config,
            commands::update_app_config,
            commands::get_default_claude_options,
            commands::save_default_claude_options,
            commands::save_last_project,
            commands::open_in_file_manager,
            commands::get_project_config,
            commands::get_all_agents,
            commands::get_all_skills,
            commands::get_all_mcp_servers,
            commands::get_all_plugins,
            commands::get_mcp_server_detail,
            commands::test_communication,
            commands::check_for_updates,
            commands::get_app_path,
            commands::spawn_new_instance,
            commands::log_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
