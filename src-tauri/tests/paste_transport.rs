//! 粘贴传输反馈环：DevTools 复制的 JSON 粘贴到 Claude CLI 输入框被截断（只剩尾部）。
//!
//! 用真实 ConPTY（portable-pty，与 app 同路径）spawn node 子进程（与 Claude CLI 同运行时），
//! 模拟「Ctrl+V 粘贴」写入 payload，子进程把收到的 stdin 原始字节回报回 master 输出流，
//! 以此断言「写入 ConPTY 的字节 == 应用收到的字节」。
//! RED 判据：字节数不匹配（丢头部 → 只剩尾部）或换行语义变形。

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const SENTINEL: &str = "__PASTE_END__";

/// node 子进程脚本：raw/cooked 可选；累积 stdin 到 sentinel，回报 `RECV:<净字节数>`
/// 和首尾片段（定位丢失位置）；8s 未等到 sentinel 则报 `NODE_TIMEOUT len=<n>`。
const NODE_SCRIPT: &str = r#"
if (process.env.CC_PROBE_RAW === '1') {
  if (process.stdin.isTTY && process.stdin.setRawMode) process.stdin.setRawMode(true);
}
process.stdin.setEncoding('utf8');
let acc = '';
process.stdin.on('data', (c) => {
  acc += c;
  if (acc.includes('__PASTE_END__')) {
    const body = acc.slice(0, acc.indexOf('__PASTE_END__'));
    process.stderr.write('RECV:' + body.length + '\n');
    process.stderr.write('HEAD:' + JSON.stringify(body.slice(0, 50)) + '\n');
    process.stderr.write('TAIL:' + JSON.stringify(body.slice(-50)) + '\n');
    process.exit(0);
  }
});
setTimeout(() => { process.stderr.write('NODE_TIMEOUT len=' + acc.length + '\n'); process.exit(2); }, 8000);
"#;

/// portable-pty 在 Windows 上不会经过 shell 解析 PATH；显式解析 node.exe，
/// 避免 nvm 等 PATH shim 在 CreateProcessW 中无法定位可执行文件。
fn node_program() -> OsString {
    let candidates = if cfg!(windows) {
        ["node.exe", "node.cmd", "node"]
    } else {
        ["node", "node.exe", "node.cmd"]
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in candidates {
                let candidate = PathBuf::from(&dir).join(name);
                if candidate.is_file() {
                    return candidate.into_os_string();
                }
            }
        }
    }
    OsString::from("node")
}

/// 生成 DevTools「Copy object」风格的多行 JSON：每行 `  "key_N": "AAAA…",`，LF 换行。
fn devtools_style_json(total_len: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("{".to_string());
    let mut n = 0usize;
    let mut used = 2usize;
    while used + 4 < total_len {
        let key = format!("key_{}", n);
        let prefix_len = 2 + key.len() + 4; // 缩进 + "key": "
        let value_len = 60.min(total_len.saturating_sub(used + prefix_len + 3));
        let line = format!("  \"{}\": \"{}\",", key, "A".repeat(value_len));
        used += line.len() + 1;
        lines.push(line);
        n += 1;
    }
    lines.push("}".to_string());
    lines.join("\n")
}

/// spawn node 探针 → 写 payload → 等待 node 超时自退 → 返回 master 输出全量文本。
fn run_probe(payload: &str, node_raw: bool) -> String {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(node_program());
    cmd.arg("-e");
    cmd.arg(NODE_SCRIPT);
    if node_raw {
        cmd.env("CC_PROBE_RAW", "1");
    }
    let mut child = pair.slave.spawn_command(cmd).expect("spawn node");
    drop(pair.slave);

    // 等待子进程完成 stdin/console 初始化，避免启动竞态丢掉首批粘贴字节。
    std::thread::sleep(Duration::from_millis(300));

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_in_thread = Arc::clone(&collected);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            collected_in_thread
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });

    let mut writer = pair.master.take_writer().expect("take writer");
    writer.write_all(payload.as_bytes()).expect("write payload");
    writer.write_all(SENTINEL.as_bytes()).expect("write sentinel");
    writer.flush().expect("flush");

    // 等 RECV 回报或 node 超时标记
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        let done = {
            let text = collected.lock().unwrap();
            text.contains("RECV:") || text.contains("NODE_TIMEOUT")
        };
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let text = collected.lock().unwrap().clone();
    text
}

/// 提取 node 回报的 `RECV:<n>`（未收到则 None）。
fn parse_recv(output: &str) -> Option<usize> {
    let pos = output.find("RECV:")?;
    let rest = &output[pos + 5..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<usize>().ok()
}

/// 核心断言：payload 完整送达（字节数一致），失败时打印 node 看到的首尾片段。
fn assert_paste_intact(case: &str, payload: &str, node_raw: bool) {
    let output = run_probe(&format!("{}{}", payload, SENTINEL), node_raw);
    match parse_recv(&output) {
        Some(n) if n == payload.len() => {
            println!("[PASS] {:<24} {} bytes intact", case, payload.len());
        }
        Some(n) => panic!(
            "[RED] {} 发送 {} 字节，node 收到 {} 字节（丢 {}）\nHEAD: {}\nTAIL: {}",
            case,
            payload.len(),
            n,
            payload.len() - n,
            output.lines().find(|l| l.contains("HEAD:")).unwrap_or(""),
            output.lines().find(|l| l.contains("TAIL:")).unwrap_or(""),
        ),
        None => panic!(
            "[RED] {} node 未在限期内读到 sentinel\n{}",
            case,
            output.lines().filter(|l| l.contains("NODE_TIMEOUT") || l.contains("HEAD:") || l.contains("bad option")).next().unwrap_or(&output[output.len().saturating_sub(300)..])
        ),
    }
}

#[test]
fn probe_raw_bracketed_small_json() {
    // 环校准探针：raw mode 下小 payload 必须能走通（此前已确认 ConPTY 会吞 bracketed 标记）
    let payload = "{\n  \"key_0\": \"AAAA\"\n}";
    assert_paste_intact("probe-raw-small", payload, true);
}

#[test]
fn baseline_raw_singleline_4k() {
    assert_paste_intact("baseline-raw-4k", &"B".repeat(4096), true);
}

#[test]
fn devtools_json_raw_1k() {
    assert_paste_intact("json-raw-1k", &devtools_style_json(1024), true);
}

#[test]
fn devtools_json_raw_4k() {
    assert_paste_intact("json-raw-4k", &devtools_style_json(4 * 1024), true);
}

#[test]
fn devtools_json_raw_16k() {
    assert_paste_intact("json-raw-16k", &devtools_style_json(16 * 1024), true);
}

#[test]
fn devtools_json_raw_64k() {
    assert_paste_intact("json-raw-64k", &devtools_style_json(64 * 1024), true);
}

#[test]
fn devtools_json_raw_128k() {
    assert_paste_intact("json-raw-128k", &devtools_style_json(128 * 1024), true);
}

#[test]
fn devtools_json_raw_8k_crlf() {
    // 剪贴板原始形态是 CRLF（preparePasteText 之前的输入）
    let crlf = devtools_style_json(8 * 1024).replace('\n', "\r\n");
    assert_paste_intact("json-raw-crlf-8k", &crlf, true);
}
