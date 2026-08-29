//! 粘贴端到端反馈环（重型，#[ignore] 默认跳过）：真实 claude CLI 跑在与 app 相同的
//! portable-pty/ConPTY 路径里，复现「DevTools JSON 粘贴到输入框只显尾部」。
//!
//! 运行：cargo test --test paste_claude_e2e -- --ignored --test-threads=1 --nocapture
//!
//! 安全护栏：探针自身不发送 Enter；真实 Claude Code 多行粘贴不会逐行自动提交
//! （用户症状即输入框截断而非消息被发出），故裸配置（用户真实 provider）下运行无请求风险。

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// 生成 DevTools「Copy object」风格的多行 JSON：每行 `  "key_N": "AAAA…",`，LF 换行。
fn devtools_style_json(total_len: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("{".to_string());
    let mut n = 0usize;
    let mut used = 2usize;
    let mut body: Vec<String> = Vec::new();
    while used + 4 < total_len {
        let key = format!("key_{}", n);
        let prefix_len = 2 + key.len() + 4;
        let value_len = 60.min(total_len.saturating_sub(used + prefix_len + 3));
        let line = format!("  \"{}\": \"{}\"", key, "A".repeat(value_len));
        used += line.len() + 2; // +",\n"（末行逗号在 join 时补）
        body.push(line);
        n += 1;
    }
    lines.push(body.join(",\n"));
    lines.push("}".to_string());
    lines.join("\n")
}

struct ClaudeSession {
    collected: Arc<Mutex<String>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    reader_dead: Arc<std::sync::atomic::AtomicBool>,
}

fn spawn_claude(_cwd: &str, dump_out: &str) -> ClaudeSession {
    // 重试式 spawn：本机 conhost 资源紧张时新会话可能秒死（reader 立即 EOF），
    // 丢弃死会话重试，用就绪标记确认拿到健康实例。
    for attempt in 1..=8 {
        println!("[e2e] 第 {} 次尝试 spawn claude", attempt);
        let session = spawn_claude_once(dump_out);
        // 快速探活：3s 内 reader EOF = 会话秒死，立即弃；否则给足渲染时间
        let dead = session.wait_for_dead(Duration::from_secs(3));
        if dead {
            let len = session.collected.lock().unwrap().len();
            println!("[e2e] 第 {} 次会话秒死（len={}），弃", attempt, len);
            drop(session);
            std::thread::sleep(Duration::from_millis(2000));
            continue;
        }
        let ready = session.wait_for("shift+tab", Duration::from_secs(20));
        if ready {
            println!("[e2e] 会话就绪");
            return session;
        }
        let len = session.collected.lock().unwrap().len();
        println!("[e2e] 第 {} 次会话存活但未就绪（len={}），重试", attempt, len);
    }
    panic!("[e2e] 连续 5 次 spawn 均未就绪");
}

/// 显式解析 node.exe（EDITER 转储助手需要），与 paste_transport.rs 同逻辑。
fn node_program() -> OsString {
    let mut cmd_name = OsString::from("node");
    if let Some(path) = std::env::var_os("PATH") {
        'outer: for dir in std::env::split_paths(&path) {
            for name in ["node.exe", "node.cmd", "node"] {
                let candidate = PathBuf::from(&dir).join(name);
                if candidate.is_file() {
                    cmd_name = candidate.into_os_string();
                    break 'outer;
                }
            }
        }
    }
    cmd_name
}

fn spawn_claude_once(dump_out: &str) -> ClaudeSession {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let program = std::env::var_os("CC_E2E_CLAUDE_PATH")
        .unwrap_or_else(|| OsString::from("C:/Users/wusha/.local/bin/claude.exe"));
    println!("[e2e] 使用 claude 程序: {:?}", program);
    let mut cmd = CommandBuilder::new(program);
    cmd.cwd("E:/source/github/cc-box");
    // EDITOR 指向转储助手：Ctrl+G 触发外部编辑器时，助手把编辑器缓冲原样落盘后退出，
    // 探针据此回读真实编辑器内容做逐字节判定（排除 VT 渲染流视口/批处理失真）
    let helper = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/dump_editor.js"
    );
    let node = node_program().to_string_lossy().to_string();
    cmd.env("EDITOR", format!("\"{}\" \"{}\"", node, helper));
    cmd.env("CC_DUMP_OUT", dump_out);
    let child = pair.slave.spawn_command(cmd).expect("spawn claude");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_in_thread = Arc::clone(&collected);
    // reader EOF = 输出流关闭（进程秒死的信号）
    let reader_dead = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader_dead_in_thread = Arc::clone(&reader_dead);
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
        reader_dead_in_thread.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let master = pair.master;
    ClaudeSession {
        collected,
        master,
        writer: None,
        child,
        reader_dead,
    }
}

impl ClaudeSession {
    fn write_all(&mut self, data: &str) {
        if self.writer.is_none() {
            // 就绪后再取 writer：spawn 毫秒级取 input 管道句柄会撞 conhost 初始化导致会话秒死
            self.writer = Some(self.master.take_writer().expect("take writer"));
        }
        let w = self.writer.as_mut().unwrap();
        w.write_all(data.as_bytes()).expect("write");
        w.flush().expect("flush");
    }

    /// 分块写入：模拟 pacing（chunk 字节/每 delay_ms），chunk_size=0 表示一次性写入。
    fn write_chunked(&mut self, data: &str, chunk_size: usize, delay_ms: u64) {
        if self.writer.is_none() {
            self.writer = Some(self.master.take_writer().expect("take writer"));
        }
        if chunk_size == 0 {
            self.write_all(data);
            return;
        }
        let bytes = data.as_bytes();
        for chunk in bytes.chunks(chunk_size) {
            let w = self.writer.as_mut().unwrap();
            w.write_all(chunk).expect("write chunk");
            w.flush().expect("flush chunk");
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }

    fn wait_for(&self, needle: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            {
                let text = self.collected.lock().unwrap();
                if text.contains(needle) {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }

    /// 等待会话死亡信号（reader EOF），超时返回 false（视为存活）。
    fn wait_for_dead(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.reader_dead.load(std::sync::atomic::Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn wait_quiet(&self, quiet_ms: u64, timeout: Duration) {
        let start = Instant::now();
        let mut last_len = 0usize;
        let mut last_change = Instant::now();
        while start.elapsed() < timeout {
            let len = self.collected.lock().unwrap().len();
            if len != last_len {
                last_len = len;
                last_change = Instant::now();
            } else if last_change.elapsed() >= Duration::from_millis(quiet_ms) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn snapshot_tail(&self, n: usize) -> String {
        let text = self.collected.lock().unwrap().clone();
        text.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()
    }

    fn snapshot_all(&self) -> String {
        self.collected.lock().unwrap().clone()
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// 增量完整性共享判定（run_case 与监测探针同强度使用）：
/// chip 识别接管（内容折叠保存，不渲染属预期）→ SAFE；
/// 否则三哨兵 key_0/middle/tail 必须在增量内按顺序出现，返回首个异常哨兵。
/// 单行折行渲染同样会把全部哨兵写入累积流，缺失即真截断，不存在视口豁免。
fn verify_increment(increment: &str, payload: &str) -> Result<(), String> {
    let chip = increment.contains("Pasted text") || increment.contains("paste again to expand");
    if chip {
        return Ok(());
    }
    let last_key = last_key_of(payload);
    let middle_key = middle_key_of(payload);
    let mut search_from = 0usize;
    for k in ["key_0", middle_key.as_str(), last_key.as_str()] {
        match increment[search_from..].find(k) {
            Some(p) => search_from += p + k.len(),
            None => return Err(k.to_string()),
        }
    }
    Ok(())
}

/// 核心探针：粘贴后判定内容完整性。
/// 主判定 = 回读编辑器缓冲：Ctrl+G 触发 EDITOR 转储助手，把 Claude 输入编辑器的
/// 真实内容落盘，与 payload 逐字节比对（只容忍末尾换行）——渲染层彻底出局，
/// 哨兵缺失不再可能是视口/批处理假象。chip 识别接管（内容折叠）时跳过回读。
/// 次判定 = 输出增量内的有序三哨兵（保留上游行为画像）。
fn run_case(case: &str, payload: &str, chunk_size: usize, delay_ms: u64) {
    println!("=== [{}] payload {} 字节，chunk={} delay={}ms ===", case, payload.len(), chunk_size, delay_ms);
    let dump_out = std::env::temp_dir().join(format!(
        "cc_desk_paste_dump_{}_{}.txt",
        case,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dump_out);
    let mut session = spawn_claude("E:/source/github/cc-box", &dump_out.to_string_lossy());

    // spawn_claude 已等待就绪标记；这里再留一点稳定时间
    std::thread::sleep(Duration::from_millis(1500));

    let baseline_len = session.snapshot_all().len();
    session.write_chunked(payload, chunk_size, delay_ms);
    session.wait_quiet(1200, Duration::from_secs(20));

    // 打最终屏幕快照（剥离 ESC），供人工读取编辑器内容
    let tail = session.snapshot_tail(14000);
    let visible: String = tail.chars().filter(|c| *c != '\u{1b}').collect();
    println!("---- [{}] 最终屏幕尾部（剥离 ESC） ----", case);
    println!("{}", visible);
    println!("---- end ----");

    let full = session.snapshot_all();
    let increment = &full[baseline_len..];
    let chip = increment.contains("Pasted text") || increment.contains("paste again to expand");
    let verdict = verify_increment(increment, payload);
    println!(
        "[{}] 结果: 基线={}B 增量={}B chip={} 哨兵判定={}",
        case,
        baseline_len,
        increment.len(),
        chip,
        if verdict.is_ok() { "SAFE" } else { "RED" }
    );

    if chip {
        println!("[SAFE] {} 粘贴被 chip 识别接管（内容折叠保存，不回读）", case);
        return;
    }

    // 主判定：Ctrl+G（\x07）回读编辑器真实缓冲
    session.write_all("\u{7}");
    let mut dumped = None;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(content) = std::fs::read_to_string(&dump_out) {
            dumped = Some(content);
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    match dumped {
        Some(content) => {
            // 编辑器可能补一个末尾换行；其余字节必须与 payload 完全一致
            let normalized = content.trim_end_matches(['\n', '\r']);
            if normalized == payload {
                println!("[SAFE] {} 编辑器缓冲逐字节一致（{} 字节）", case, normalized.len());
            } else {
                panic!(
                    "[RED] {} 编辑器缓冲与 payload 不一致：缓冲 {} 字节 / 期望 {} 字节（已排除渲染失真，实为内容丢失）",
                    case,
                    normalized.len(),
                    payload.len()
                );
            }
        }
        None => panic!("[RED] {} Ctrl+G 转储超时：EDITOR 助手未生效或会话异常", case),
    }
}

/// 与生产 TS compactJsonForPaste 等价的压缩：移除 (\r\n | \r | \n) 及后继 [ \t]*。
/// 与 paste_transport.rs 中同名函数同算法；二者与 TS 的逐字节等价由共享 fixture
/// （tests/fixtures/paste_sample.*）上的 production_pipeline_fixture_shape 与
/// PasteJson_ProductionEquivalence_012 双向锁定。
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

/// 取 payload 中最后一个 key_N（全流完整性判定的尾部标记）。
fn last_key_of(payload: &str) -> String {
    let mut last = String::new();
    let mut search = 0usize;
    while let Some(pos) = payload[search..].find("key_") {
        let abs = search + pos;
        let rest = &payload[abs + 4..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num.is_empty() {
            last = format!("key_{}", num);
        }
        search = abs + 4;
    }
    last
}

/// 取 payload 最大学号的一半对应的 key_N（中段哨兵）。
fn middle_key_of(payload: &str) -> String {
    let last = last_key_of(payload);
    let num: String = last[4..].chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = num.parse().unwrap_or(0);
    format!("key_{}", n / 2)
}

#[test]
#[ignore]
fn claude_small_lf_json_control() {
    // 对照组：小 payload 粘贴应完整（验证环本身没坏）
    let payload = devtools_style_json(1024);
    run_case("lf-1k-single", &payload, 0, 0);
}

#[test]
#[ignore]
fn claude_json_singleline_paste_monitor() {
    // 上游可靠性监测探针：app 对合法 JSON 的实际写入形态是压缩后的单行（见
    // src/utils/pasteText.ts compactJsonForPaste）。单行形态相比多行显著降低丢失率，
    // 但大 burst（≥4KB）在 Claude Code 未识别为粘贴时仍可能被截尾——上游 #49673/#49337
    // 未修，Win10 ConPTY 又吞掉 bracketed 标记，带内无 100% 可靠方案。本测试 RED =
    // 上游仍不可靠的信号；上游修复（或换用可透传标记的系统 conhost）后应转绿。
    let pretty = devtools_style_json(8 * 1024);
    // payload 用与生产逐字节等价的方式构造：compact_json_like_production 与 TS
    // compactJsonForPaste 是同一算法，二者在共享 fixture
    // tests/fixtures/paste_sample.* 上的输出逐字节一致（transport 套件
    // production_pipeline_fixture_shape 与 TS 用例 PasteJson_ProductionEquivalence_012
    // 双向锁定），因此本 payload 即生产 buildPastePayload 的正文形态。
    serde_json::from_str::<serde_json::Value>(&pretty).expect("payload 必须是合法 JSON");
    let payload = compact_json_like_production(&pretty);
    assert!(!payload.contains('\n'), "压缩后必须单行");
    for rep in 1..=3 {
        let dump_out = std::env::temp_dir().join(format!(
            "cc_desk_paste_dump_monitor_rep{}_{}.txt",
            rep,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&dump_out);
        let mut session = spawn_claude("E:/source/github/cc-box", &dump_out.to_string_lossy());
        std::thread::sleep(Duration::from_millis(1500));
        // 增量绑定：只在本轮粘贴之后的输出里判定 chip 与哨兵
        let baseline_len = session.snapshot_all().len();
        session.write_chunked(&payload, 0, 0);
        session.wait_quiet(1200, Duration::from_secs(20));
        let full = session.snapshot_all();
        let increment = &full[baseline_len..];
        let chip = increment.contains("Pasted text") || increment.contains("paste again to expand");
        let verdict = verify_increment(increment, &payload);
        println!(
            "[minified-rep-{}] 基线={}B 增量={}B chip={} 哨兵判定={}",
            rep,
            baseline_len,
            increment.len(),
            chip,
            if verdict.is_ok() { "SAFE" } else { "RED" }
        );
        if chip {
            println!("[SAFE] minified-rep-{} 粘贴被 chip 识别接管（内容折叠保存，不回读）", rep);
            continue;
        }
        // 主判定：Ctrl+G 回读编辑器真实缓冲（与 run_case 同强度）
        session.write_all("\u{7}");
        let mut dumped = None;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(12) {
            if let Ok(content) = std::fs::read_to_string(&dump_out) {
                dumped = Some(content);
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        match dumped {
            Some(content) => {
                let normalized = content.trim_end_matches(['\n', '\r']);
                if normalized == payload {
                    println!("[SAFE] minified-rep-{} 编辑器缓冲逐字节一致（{} 字节）", rep, normalized.len());
                } else {
                    panic!(
                        "[RED] minified-rep-{} 编辑器缓冲与 payload 不一致：缓冲 {} 字节 / 期望 {} 字节（已排除渲染失真）",
                        rep,
                        normalized.len(),
                        payload.len()
                    );
                }
            }
            None => panic!("[RED] minified-rep-{} Ctrl+G 转储超时", rep),
        }
    }
}

#[test]
#[ignore]
fn claude_multiline_json_upstream_bug_repro() {
    // 上游 bug 探针（anthropics/claude-code#49673/#49337，官方关闭不修）：
    // 多行无标记 burst（ConPTY 吞掉 bracketed 标记后 Claude 实际收到的形态）会
    // 间歇性丢失头部或尾部。此测试会间歇 RED——用于人工监测上游是否修复，
    // 不是常规回归断言；修复后 CC Desk 可移除 compactJsonForPaste 的压缩前置。
    let payload = devtools_style_json(8 * 1024);
    run_case("lf-8k-multiline", &payload, 0, 0);
}
