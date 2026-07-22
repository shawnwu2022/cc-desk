// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let initial_dir = args
        .get(1)
        .filter(|p| std::path::Path::new(p).is_dir())
        .cloned();
    cc_desk::run(initial_dir)
}
