//! 粘贴传输反馈环：DevTools 复制的 JSON 粘贴到 Claude CLI 输入框被截断（只剩尾部）。
//!
//! 用真实 ConPTY（portable-pty，与 app 同路径）spawn node 子进程（与 Claude CLI 同运行时），
//! 模拟「Ctrl+V 粘贴」写入 payload，子进程对收到的 stdin 计算长度 + FNV-1a 哈希回报，
//! Rust 侧对比完整 payload 的长度与哈希——顺序敏感、内容敏感，等长替换/重排/换行变形
//! 都会 FAIL。payload 形态对齐生产管线（buildPastePayload）：压缩单行 JSON / 多行 LF /
//! CRLF / bracketed 标记包装。
//!
//! 注意：本测试只证明传输层（ConPTY → 子进程 stdin）字节保真，不覆盖 TS 压缩逻辑
//! （tests/utils/pasteText.test.ts 负责）与 Claude 编辑器行为（paste_claude_e2e.rs 探针）。

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const SENTINEL: &str = "__PASTE_END__";
const BRACKET_OPEN: &str = "\u{1b}[200~";
const BRACKET_CLOSE: &str = "\u{1b}[201~";

/// node 子进程脚本：累积 stdin 到 sentinel，回报长度、FNV-1a 哈希与首尾片段。
/// 若检测到 bracketed 标记（可能被 ConPTY 剥除，也可能被透传）先剥除再计算，
/// 使两种 ConPTY 行为下断言都成立。
const NODE_SCRIPT: &str = r#"
if (process.stdin.isTTY && process.stdin.setRawMode) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stderr.write('RAW:' + (process.stdin.isRaw ? 1 : 0) + '\n');
let acc = '';
process.stdin.on('data', (c) => {
  acc += c;
  if (acc.includes('__PASTE_END__')) {
    const idx = acc.indexOf('__PASTE_END__');
    let body = acc.slice(0, idx);
    if (body.startsWith('\x1b[200~')) body = body.slice(6);
    if (body.endsWith('\x1b[201~')) body = body.slice(0, -6);
    const bytes = Buffer.from(body, 'utf8');
    let h = 2166136261 >>> 0;
    for (const b of bytes) { h ^= b; h = Math.imul(h, 16777619) >>> 0; }
    process.stderr.write('RECV:' + body.length + '\n');
    process.stderr.write('HASH:' + h + '\n');
    process.stderr.write('HEAD:' + JSON.stringify(body.slice(0, 40)) + '\n');
    process.stderr.write('TAIL:' + JSON.stringify(body.slice(-40)) + '\n');
    process.exit(0);
  }
});
setTimeout(() => { process.stderr.write('NODE_TIMEOUT len=' + acc.length + '\n'); process.exit(2); }, 8000);
"#;

/// FNV-1a 32 位（字节域），与 NODE_SCRIPT 内实现一致。
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

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

/// 生成 DevTools「Copy object」风格的多行 JSON：每行 `  "key_N": "AAAA…"`，LF 换行，
/// 合法 JSON（末属性无尾逗号）。
fn devtools_style_json(total_len: usize) -> String {
    let mut body: Vec<String> = Vec::new();
    let mut n = 0usize;
    let mut used = 2usize;
    while used + 4 < total_len {
        let key = format!("key_{}", n);
        let prefix_len = 2 + key.len() + 4;
        let value_len = 60.min(total_len.saturating_sub(used + prefix_len + 2));
        let line = format!("  \"{}\": \"{}\"", key, "A".repeat(value_len));
        used += line.len() + 2; // +",\n"（逗号在 join 时补）
        body.push(line);
        n += 1;
    }
    format!("{{\n{}\n}}", body.join(",\n"))
}

#[cfg(test)]
mod generator_guards {
    // 生成器合法性内建校验：历史上曾产出尾逗号非法 JSON（上轮对抗审查发现），
    // 这里在每个尺寸冒烟锁定 serde 解析通过
    #[test]
    fn devtools_generator_is_valid_json() {
        for size in [256, 1024, 8 * 1024, 128 * 1024] {
            let json = super::devtools_style_json(size);
            serde_json::from_str::<serde_json::Value>(&json)
                .unwrap_or_else(|e| panic!("size {size} 生成了非法 JSON: {e}"));
        }
    }
}

/// 对齐生产管线 compactJsonForPaste：合法 JSON 压缩单行——移除结构性换行
/// （\r\n | \r | \n）及其后继缩进空白，与 TS 端 /(\r\n|\r|\n)[ \t]*/g 等价。
fn compact_json_like_production(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                while matches!(chars.peek(), Some(' ') | Some('\t')) {
                    chars.next();
                }
            }
            '\n' => {
                while matches!(chars.peek(), Some(' ') | Some('\t')) {
                    chars.next();
                }
            }
            _ => out.push(c),
        }
    }
    out
}

struct PasteResult {
    received_len: Option<usize>,
    received_hash: Option<u32>,
    raw: Option<u32>,
    head: String,
    tail: String,
    output_tail: String,
}

/// 跑一次粘贴：spawn node PTY → 写 payload+sentinel → 等回报。
fn run_paste_case(payload: &str) -> PasteResult {
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

    let start = Instant::now();
    let mut writer = pair.master.take_writer().expect("take writer");
    writer.write_all(payload.as_bytes()).expect("write payload");
    writer
        .write_all(SENTINEL.as_bytes())
        .expect("write sentinel");
    // raw 模式下此回车是 sentinel 之后的额外字节（不计入 body）；若降级 cooked 行模式
    // 则充当行结束符，保证 node 能收到数据
    writer.write_all(b"\r").expect("write terminator");
    writer.flush().expect("flush");

    // 等 RECV 回报或 node 超时标记
    while start.elapsed() < Duration::from_secs(15) {
        let done = {
            let text = collected.lock().unwrap();
            (text.contains("RECV:") && text.contains("HASH:")) || text.contains("NODE_TIMEOUT")
        };
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();

    let text = collected.lock().unwrap().clone();
    // ConPTY 渲染会把回报行与 ANSI 序列（清屏/擦除）混在同一“行”里，
    // 因此按任意位置定位标记、取其后前导数字解析
    let value_after = |marker: &str| -> Option<String> {
        let pos = text.find(marker)?;
        let rest = &text[pos + marker.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            None
        } else {
            Some(digits)
        }
    };
    let received_len = value_after("RECV:").and_then(|d| d.parse::<usize>().ok());
    let received_hash = value_after("HASH:").and_then(|d| d.parse::<u32>().ok());
    let raw = value_after("RAW:").and_then(|d| d.parse::<u32>().ok());
    let head = text
        .lines()
        .find(|l| l.contains("HEAD:"))
        .unwrap_or("")
        .to_string();
    let tail = text
        .lines()
        .find(|l| l.contains("TAIL:"))
        .unwrap_or("")
        .to_string();
    let output_tail = text
        .chars()
        .rev()
        .take(400)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    PasteResult {
        received_len,
        received_hash,
        raw,
        head,
        tail,
        output_tail,
    }
}

/// 核心断言：长度与 FNV-1a 哈希双比对（内容+顺序敏感），失败打印首尾片段。
fn assert_paste_intact(case: &str, expected_body: &str, payload: &str) {
    let expected_len = expected_body.len();
    let expected_hash = fnv1a(expected_body.as_bytes());
    let result = run_paste_case(payload);
    // 探针必须运行在 raw mode：cooked 行模式攒行不吐数据，测的不是 Claude 实际输入路径
    assert_eq!(result.raw, Some(1), "{} 探针未运行在 raw mode", case);
    match (result.received_len, result.received_hash) {
        (Some(n), Some(h)) if n == expected_len && h == expected_hash => {
            println!("[PASS] {:<28} {} bytes, hash {:#010x}", case, expected_len, expected_hash);
        }
        (Some(n), Some(h)) => panic!(
            "[RED] {} 期望 {} 字节/hash {:#010x}，实际 {} 字节/hash {:#010x}\nHEAD: {}\nTAIL: {}\n输出尾部: {:?}",
            case, expected_len, expected_hash, n, h, result.head, result.tail, result.output_tail
        ),
        _ => panic!(
            "[RED] {} 未在限期内收到完整回报\n输出尾部: {:?}",
            case, result.output_tail
        ),
    }
}

#[test]
fn baseline_raw_singleline_4k() {
    // 基线：无换行纯文本，链路本身必须通（不通则反馈环自身失效）
    let payload = "B".repeat(4096);
    assert_paste_intact("baseline-raw-4k", &payload, &payload);
}

#[test]
fn devtools_json_multiline_lf_8k() {
    // 多行 LF JSON：非 JSON 管线路径（不压缩）的传输保真
    let json = devtools_style_json(8 * 1024);
    assert_paste_intact("json-multiline-lf-8k", &json, &json);
}

#[test]
fn devtools_json_compact_singleline_8k() {
    // 生产 JSON 管线形态：压缩后的单行（buildPastePayload 实际写入的字节形态）
    let json = devtools_style_json(8 * 1024);
    let compact = compact_json_like_production(&json);
    assert_paste_intact("json-compact-8k", &compact, &compact);
}

#[test]
fn production_pipeline_fixture_shape() {
    // 共享 fixture 贯通：compact_json_like_production 对仓库样本的压缩输出必须与
    // 黄金文件逐字节一致（TS 侧 PasteJson_ProductionEquivalence_012 对同一样本断言
    // compactJsonForPaste 输出 === 同一黄金文件），Rust 构造的 payload 由此与生产
    // buildPastePayload 的正文逐字节一致，传输保真结论直接覆盖生产字节形态
    let pretty =
        std::fs::read_to_string("tests/fixtures/paste_sample.pretty.json").expect("read fixture");
    let golden =
        std::fs::read_to_string("tests/fixtures/paste_sample.compacted.txt").expect("read golden");
    serde_json::from_str::<serde_json::Value>(&pretty).expect("fixture 必须是合法 JSON");
    assert!(!golden.contains('\n'), "黄金压缩输出必须单行");
    let compact = compact_json_like_production(&pretty);
    assert_eq!(compact, golden, "Rust 压缩与黄金文件不一致");
    assert_paste_intact("json-fixture-compact", &compact, &compact);
}

#[test]
fn devtools_json_multiline_crlf_8k() {
    // 剪贴板原始形态是 CRLF
    let crlf = devtools_style_json(8 * 1024).replace("\n", "\r\n");
    assert_paste_intact("json-multiline-crlf-8k", &crlf, &crlf);
}

#[test]
fn devtools_json_bracketed_wrapped_8k() {
    // bracketed 标记包装形态：ConPTY 可能剥除标记（Win10 实测）也可能透传（未来版本），
    // 两种行为下正文都必须完整到达（node 侧对标记做了容错剥除）
    let json = devtools_style_json(8 * 1024);
    let payload = format!("{}{}{}", BRACKET_OPEN, json, BRACKET_CLOSE);
    assert_paste_intact("json-bracketed-8k", &json, &payload);
}

#[test]
fn devtools_json_compact_128k() {
    // DevTools 大对象压缩后的量级
    let json = devtools_style_json(128 * 1024);
    let compact = compact_json_like_production(&json);
    assert_paste_intact("json-compact-128k", &compact, &compact);
}
