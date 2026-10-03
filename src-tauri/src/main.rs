// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod conpty_runtime;

fn main() {
    #[cfg(windows)]
    match cc_desk::run_version_manager_entry() {
        Ok(true) => return,
        Ok(false) => {}
        Err(code) => {
            show_startup_refusal(&code);
            std::process::exit(3);
        }
    }
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    {
        // Execute before any Tauri/PTY initialization, including development builds.
        if args.get(1).is_some_and(|arg| arg == "--check-conpty") {
            let Some(output) = args.get(2).filter(|_| args.len() == 3) else {
                std::process::exit(2);
            };
            std::process::exit(conpty_runtime::check_report(std::path::Path::new(output)));
        }
    }
    // An ordinary source holds its shared installation lease before any ConPTY,
    // logger, NativeRuntime, repository, or WebView initialization can occur.
    #[cfg(windows)]
    let startup = match cc_desk::admit_desktop_startup() {
        Ok(startup) => startup,
        Err(code) => {
            show_startup_refusal(&code);
            std::process::exit(3);
        }
    };
    #[cfg(windows)]
    {
        if let Err(error) = conpty_runtime::initialize() {
            conpty_runtime::show_error(&error);
            std::process::exit(2);
        }
    }
    let initial_dir = args
        .get(1)
        .filter(|p| std::path::Path::new(p).is_dir())
        .cloned();
    #[cfg(windows)]
    cc_desk::run_admitted(initial_dir, startup);
    #[cfg(not(windows))]
    cc_desk::run(initial_dir);
}

#[cfg(windows)]
fn show_startup_refusal(code: &str) {
    use std::{ffi::c_void, iter, ptr};
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(owner: *mut c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
    }
    let safe_code = match code {
        "HISTORY_RECOVERY_REQUIRED" => "HISTORY_RECOVERY_REQUIRED",
        "HISTORY_SCOPE_UNREGISTERED" => "HISTORY_SCOPE_UNREGISTERED",
        "HISTORY_MANAGER_ENTRY_REQUIRED" => "HISTORY_MANAGER_ENTRY_REQUIRED",
        "HISTORY_MANAGER_ENTRY_INVALID" => "HISTORY_MANAGER_ENTRY_INVALID",
        "HISTORY_HANDOFF_CHANGED" => "HISTORY_HANDOFF_CHANGED",
        _ => "HISTORY_STARTUP_UNAVAILABLE",
    };
    let text: Vec<u16> = format!(
        "CC Desk 无法确认当前安装的数据状态，已在加载应用数据前停止。\n请保留现有文件和历史版本备份，并从版本管理器检查恢复状态。\n\n{safe_code}"
    ).encode_utf16().chain(iter::once(0)).collect();
    let title: Vec<u16> = "CC Desk — 启动检查"
        .encode_utf16()
        .chain(iter::once(0))
        .collect();
    unsafe { MessageBoxW(ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10) };
}
