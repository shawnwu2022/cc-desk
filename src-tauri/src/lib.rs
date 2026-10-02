mod checks;
mod cli;
mod commands;
mod hook_config;
mod hook_events;
mod hook_server;
mod logger;
mod observer_host;
mod observer_http;
mod observer_registry;
mod paste_trace;
mod platform;
mod pty;
mod pty_decoder;
mod run_lifecycle;
mod run_supervisor;
mod session_name_index;
mod store;
mod terminal_input;
mod terminal_transport;
#[cfg(test)]
mod tests;
mod version_history;

#[cfg(target_os = "macos")]
use tauri::menu::MenuBuilder;
use tauri::Emitter;
use tauri::Manager;

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

/// Opaque ordinary-startup capability. main obtains it before loading ConPTY;
/// the Tauri application retains the same shared lease for its whole lifetime.
#[cfg(windows)]
pub struct DesktopStartup {
    admission: std::sync::Arc<version_history::windows::startup::OrdinaryStartup>,
}
#[cfg(windows)]
pub fn admit_desktop_startup() -> Result<DesktopStartup, String> {
    version_history::windows::startup::admit_ordinary()
        .map(|admission| DesktopStartup {
            admission: std::sync::Arc::new(admission),
        })
        .map_err(|failure| failure.code)
}
/// Dispatch the independent manager before ordinary startup and diagnostic DLL
/// loading. A UUID selector alone cannot pass its protected child admission.
#[cfg(windows)]
pub fn run_version_manager_entry() -> Result<bool, String> {
    match version_history::manager_entry::observed_request().map_err(|failure| failure.code)? {
        version_history::manager_entry::DesktopEntryRequest::Ordinary => Ok(false),
        version_history::manager_entry::DesktopEntryRequest::Manager(request) => {
            version_history::manager_runtime::run(request).map_err(|failure| failure.code)?;
            Ok(true)
        }
    }
}

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
pub fn run(initial_dir: Option<String>) {
    #[cfg(windows)]
    {
        let startup =
            admit_desktop_startup().expect("ordinary startup requires maintenance admission");
        run_admitted(initial_dir, startup);
    }
    #[cfg(not(windows))]
    run_ordinary(initial_dir);
}

#[cfg(windows)]
pub fn run_admitted(initial_dir: Option<String>, startup: DesktopStartup) {
    run_ordinary(initial_dir, startup)
}

fn run_ordinary(initial_dir: Option<String>, #[cfg(windows)] startup: DesktopStartup) {
    let mut context = tauri::generate_context!();
    let main_config = cli::native_runtime::take_main_config(context.config_mut())
        .expect("main window configuration unavailable");
    let admission = version_history::maintenance::process_admissions();
    let native_runtime = std::sync::Arc::new(
        cli::native_runtime::NativeRuntime::production(admission.clone())
            .expect("native workspace unavailable"),
    );
    let native_setup = native_runtime.clone();
    let native_shutdown = native_runtime.clone();
    let native_exit_shutdown = native_runtime.clone();
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.manage(startup.admission);
    let app = builder
        .manage(native_runtime)
        .manage(std::sync::Arc::new(
            version_history::commands::HistoryService::default(),
        ))
        .manage(admission.clone())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && native_shutdown
                        .binding()
                        .is_ok_and(|binding| binding.blocks_handoff_exit())
                {
                    api.prevent_close();
                    return;
                }
            }
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                log::info!("Main window close requested, cleaning up PTYs...");
                native_shutdown.shutdown();
                if let Some(manager) = pty::get_pty_manager() {
                    manager.kill_all();
                }
            }
        })
        .setup(move |app| {
            logger::init();
            // Mint and retain document authority before loading the main page.
            // The original window configuration and legacy PTY behavior remain.
            native_setup.initialize_main(app, &main_config)?;

            // macOS: 注册原生 Copy 菜单项，使 Cmd+C 在 WebView 中生效
            #[cfg(target_os = "macos")]
            {
                let menu = MenuBuilder::new(app).copy().build()?;
                let _ = app.set_menu(menu);
            }

            pty::init_pty_manager(app.handle().clone(), admission.clone());
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
                        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
                        use windows_core::Interface;
                        let controller = webview.controller();
                        if let Ok(core_wv) = unsafe { controller.CoreWebView2() } {
                            if let Ok(settings) = unsafe { core_wv.Settings() } {
                                if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() {
                                    if let Err(e) = unsafe {
                                        settings3.SetAreBrowserAcceleratorKeysEnabled(false)
                                    } {
                                        log::warn!(
                                            "Failed to disable browser accelerator keys: {}",
                                            e
                                        );
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
                    log::warn!(
                        "Failed to create plugin files: {}. Hook monitoring may not work.",
                        e
                    );
                }
                // Hook HTTP 服务器
                hook_server::init(handle.clone()).await;

                // 如果通过命令行参数传入了目录（右键菜单打开），延迟 emit 事件给前端
                if let Some(dir) = initial_dir {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = handle.emit("open-directory", dir);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            version_history::commands::list_history,
            version_history::commands::select_history,
            version_history::commands::begin_prepare_history,
            version_history::commands::prepare_history,
            version_history::commands::cancel_prepare_history,
            version_history::commands::begin_switch,
            version_history::commands::inspect_switch,
            commands::native_get_scope,
            commands::native_list_resources,
            cli::commands::cli_start,
            cli::commands::cli_get_launch_status,
            cli::commands::cli_cancel_launch,
            cli::commands::cli_ack_output,
            cli::commands::cli_input_begin,
            cli::commands::cli_input_chunk,
            cli::commands::cli_input_commit,
            cli::commands::cli_input_abort,
            cli::commands::cli_input_protocol,
            cli::commands::cli_resize,
            cli::commands::cli_stop,
            cli::commands::cli_list_profiles,
            cli::commands::cli_patch_profile,
            cli::commands::cli_get_availability,
            cli::project_commands::cli_list_projects,
            cli::project_commands::cli_register_project,
            cli::project_commands::cli_patch_project,
            cli::project_commands::cli_remove_project,
            commands::get_home_data,
            commands::get_check_results,
            commands::run_checks,
            commands::pty_spawn,
            paste_trace::pty_input,
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
            commands::get_projects_state,
            commands::pin_project,
            commands::unpin_project,
            commands::archive_session,
            commands::restore_session,
            commands::set_display_name,
            commands::upsert_session_ui_record,
            commands::remove_session_ui_record,
            commands::set_project_launch_preference,
            commands::delete_sessions,
            commands::get_default_claude_options,
            commands::save_default_claude_options,
            commands::save_last_project,
            commands::open_in_file_manager,
            commands::get_project_config,
            commands::get_all_agents,
            commands::get_all_skills,
            commands::get_all_mcp_servers,
            commands::get_all_plugins,
            commands::test_communication,
            commands::get_app_path,
            commands::spawn_new_instance,
            commands::log_message,
        ])
        .build(context)
        .expect("error while building tauri application");
    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::ExitRequested { ref api, .. } = event {
            if native_exit_shutdown
                .binding()
                .is_ok_and(|binding| binding.blocks_handoff_exit())
            {
                api.prevent_exit();
                return;
            }
        }
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            native_exit_shutdown.shutdown();
        }
    });
}
