// Learn more about Tauri commands at https://tauri.app/v2/guides/features/command/

mod pty;
mod commands;
mod store;
mod checks;
mod mcp;
mod updater;

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
            // 初始化日志（开发模式下输出到控制台）
            #[cfg(debug_assertions)]
            {
                log::set_max_level(log::LevelFilter::Debug);
                // 简单的日志输出到 stderr
                log::set_logger(&SimpleLogger).ok();
                log::info!("Application starting in debug mode");
            }

            // 初始化 PTY 管理器
            pty::init_pty_manager(app.handle().clone());
            log::info!("PTY manager initialized");

            // 检查环境
            let checks_result = checks::run_checks();
            if !checks_result.all_passed() {
                log::warn!("Environment checks failed: {:?}", checks_result.failed_checks());
            } else {
                log::info!("Environment checks passed");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 简单的日志输出器（输出到 stderr）
#[cfg(debug_assertions)]
struct SimpleLogger;

#[cfg(debug_assertions)]
impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}][{}] {}", record.target(), record.level(), record.args());
        }
    }

    fn flush(&self) {}
}