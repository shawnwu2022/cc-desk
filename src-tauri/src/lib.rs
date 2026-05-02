mod pty;
mod commands;
mod store;
mod checks;
mod mcp;
mod updater;
mod logger;

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use tauri::Emitter;
use tauri::Manager;
use raw_window_handle::HasWindowHandle;

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

/// 原生窗口焦点状态（非 Windows 平台使用）
#[cfg(not(target_os = "windows"))]
static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(true);

/// 窗口 HWND（Windows 平台使用 GetForegroundWindow 检查）
#[cfg(target_os = "windows")]
static OUR_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

#[cfg(target_os = "windows")]
extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
}

/// 检查本窗口是否在前台
#[cfg(target_os = "windows")]
fn is_window_foreground() -> bool {
    let hwnd = OUR_HWND.load(Ordering::SeqCst);
    if hwnd.is_null() {
        return true;
    }
    unsafe { GetForegroundWindow() == hwnd }
}

#[cfg(not(target_os = "windows"))]
fn is_window_foreground() -> bool {
    WINDOW_FOCUSED.load(Ordering::SeqCst)
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
                #[cfg(not(target_os = "windows"))]
                tauri::WindowEvent::Focused(focused) => {
                    WINDOW_FOCUSED.store(*focused, Ordering::SeqCst);
                }
                _ => {}
            }
        })
        .setup(|app| {
            logger::init();

            pty::init_pty_manager(app.handle().clone());
            log::info!("PTY manager initialized");

            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};

                const CMD_OR_CTRL: Modifiers = {
                    #[cfg(target_os = "macos")]
                    {
                        Modifiers::SUPER
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        Modifiers::CONTROL
                    }
                };

                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_shortcuts([
                            "CmdOrCtrl+Comma",
                            "CmdOrCtrl+Shift+N",
                            "CmdOrCtrl+Shift+Left",
                            "CmdOrCtrl+Shift+Right",
                            "CmdOrCtrl+Shift+R",
                            "CmdOrCtrl+Shift+H",
                            "CmdOrCtrl+Equal",
                            "CmdOrCtrl+Shift+Equal",
                            "CmdOrCtrl+Minus",
                            "CmdOrCtrl+0",
                            "Alt+N",
                            "Alt+R",
                            "Alt+Up",
                            "Alt+Down",
                        ])?
                        .with_handler(|app, shortcut, event| {
                            if event.state != ShortcutState::Pressed {
                                return;
                            }

                            if !is_window_foreground() {
                                return;
                            }

                            let m = shortcut.mods;
                            let k = shortcut.key;

                            let event_name = match (m, k) {
                                (m, Code::Comma)      if m == CMD_OR_CTRL => "shortcut:toggle-settings",
                                (m, Code::KeyN)       if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:new-instance",
                                (m, Code::ArrowLeft)  if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:snap-left",
                                (m, Code::ArrowRight) if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:snap-right",
                                (m, Code::KeyR)       if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:restart-app",
                                (m, Code::KeyH)       if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:back-to-projects",
                                (m, Code::Equal)      if m == CMD_OR_CTRL => "shortcut:font-increase",
                                (m, Code::Equal)      if m == CMD_OR_CTRL | Modifiers::SHIFT => "shortcut:font-increase",
                                (m, Code::Minus)      if m == CMD_OR_CTRL => "shortcut:font-decrease",
                                (m, Code::Digit0)     if m == CMD_OR_CTRL => "shortcut:font-reset",
                                (m, Code::KeyN)       if m == Modifiers::ALT => "shortcut:new-session",
                                (m, Code::KeyR)       if m == Modifiers::ALT => "shortcut:restart-session",
                                (m, Code::ArrowUp)    if m == Modifiers::ALT => "shortcut:tab-prev",
                                (m, Code::ArrowDown)  if m == Modifiers::ALT => "shortcut:tab-next",
                                _ => return,
                            };

                            let _ = app.emit(event_name, ());
                        })
                        .build(),
                )?;

                log::info!("Global shortcuts registered");
            }

            // Windows: 缓存窗口 HWND 用于前台检查
            #[cfg(target_os = "windows")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    if let Ok(handle) = win.window_handle() {
                        match handle.as_raw() {
                            raw_window_handle::RawWindowHandle::Win32(h) => {
                                OUR_HWND.store(h.hwnd.get() as *mut c_void, Ordering::SeqCst);
                            }
                            _ => {}
                        }
                    }
                }
            }

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
