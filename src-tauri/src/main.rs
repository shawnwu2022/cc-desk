// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod conpty_runtime;

fn main() {
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
        if let Err(error) = conpty_runtime::initialize() {
            conpty_runtime::show_error(&error);
            std::process::exit(2);
        }
    }
    let initial_dir = args
        .get(1)
        .filter(|p| std::path::Path::new(p).is_dir())
        .cloned();
    cc_desk::run(initial_dir)
}
