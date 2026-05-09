mod pty;
mod commands;
mod store;
mod checks;
mod mcp;
mod updater;
mod logger;
mod hook_events;
mod hook_server;
mod hook_config;

use tauri::Manager;
use tauri::menu::MenuBuilder;

/// 全局缓存环境检查结果（setup 前执行，仅一次）
use std::sync::LazyLock;
use std::sync::Mutex;
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
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    log::info!("Window close requested, cleaning up PTYs...");
                    if let Some(manager) = pty::get_pty_manager() {
                        manager.kill_all();
                    }
                }
                _ => {}
            }
        })
        .setup(|app| {
            logger::init();

            // macOS: 注册原生 Copy 菜单项，使 Cmd+C 在 WebView 中生效
            #[cfg(target_os = "macos")]
            {
                let menu = MenuBuilder::new(app)
                    .copy()
                    .build()?;
                let _ = app.set_menu(menu);
            }

            pty::init_pty_manager(app.handle().clone());
            log::info!("PTY manager initialized");

            // Windows: 移除原生标题栏（UI 相关，尽早执行）
            #[cfg(target_os = "windows")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_decorations(false);
                }
            }

            // Windows: 禁用 WebView2 浏览器加速键（Ctrl+L/D 等不再被 WebView2 拦截）
            #[cfg(target_os = "windows")]
            {
                if let Some(ww) = app.get_webview_window("main") {
                    let _ = ww.with_webview(|webview| {
                        use windows_core::Interface;
                        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
                        let controller = webview.controller();
                        if let Ok(core_wv) = unsafe { controller.CoreWebView2() } {
                            if let Ok(settings) = unsafe { core_wv.Settings() } {
                                if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() {
                                    if let Err(e) = unsafe { settings3.SetAreBrowserAcceleratorKeysEnabled(false) } {
                                        log::warn!("Failed to disable browser accelerator keys: {}", e);
                                    } else {
                                        log::info!("WebView2 browser accelerator keys disabled");
                                    }
                                }
                            }
                        }
                    });
                }
            }

            // 异步执行非关键初始化（不阻塞 UI 显示）
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Plugin 文件部署（版本匹配时跳过）
                if let Err(e) = hook_config::ensure_plugin_files() {
                    log::warn!("Failed to create plugin files: {}. Hook monitoring may not work.", e);
                }
                // Hook HTTP 服务器
                hook_server::init(handle).await;
            });

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
            commands::download_update,
            commands::install_update,
            commands::get_app_path,
            commands::spawn_new_instance,
            commands::log_message,
            commands::sync_claude_env,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
