use std::io::{Read, Write};

fn phase(value: &str) {
    if let Some(path) = std::env::var_os("CC_DESK_TEST_PHASE") {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(value.as_bytes())
            .unwrap();
    }
}

fn main() {
    // 仅测试夹具捕获合成输入；正常路径直接运行生产 reporter，不初始化桌面。
    if let Some(path) = std::env::var_os("CC_DESK_TEST_STDIN_CAPTURE") {
        phase("reporter-entered;");
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(65_537)
            .read_to_end(&mut bytes)
            .unwrap();
        phase("stdin-read;");
        std::fs::write(path, bytes).unwrap();
        phase("capture-written;");
        return;
    }
    if !d13_observer_core::run_reporter_entry() {
        std::process::exit(2);
    }
}
