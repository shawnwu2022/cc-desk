//! 粘贴端到端反馈环（重型，#[ignore] 默认跳过）：真实 claude CLI 跑在与 app 相同的
//! portable-pty/ConPTY 路径里，复现「DevTools JSON 粘贴到输入框只显尾部」。
//!
//! 运行：cargo test --test paste_claude_e2e -- --ignored --test-threads=1 --nocapture
//!
//! 安全护栏：探针自身不发送 Enter；真实 Claude Code 多行粘贴不会逐行自动提交
//! （用户症状即输入框截断而非消息被发出），故裸配置（用户真实 provider）下运行无请求风险。

use std::ffi::OsString;
use std::io::{Read, Write};
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

fn spawn_claude(_cwd: &str) -> ClaudeSession {
    // 重试式 spawn：本机 conhost 资源紧张时新会话可能秒死（reader 立即 EOF），
    // 丢弃死会话重试，用就绪标记确认拿到健康实例。
    for attempt in 1..=8 {
        println!("[e2e] 第 {} 次尝试 spawn claude", attempt);
        let session = spawn_claude_once();
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

fn spawn_claude_once() -> ClaudeSession {
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

/// 核心探针：粘贴后截取最终屏幕区域，报告编辑器内容里可见的首/尾 key。
fn run_case(case: &str, payload: &str, chunk_size: usize, delay_ms: u64) {
    println!("=== [{}] payload {} 字节，chunk={} delay={}ms ===", case, payload.len(), chunk_size, delay_ms);
    let mut session = spawn_claude("E:/source/github/cc-box");

    // spawn_claude 已等待就绪标记；这里再留一点稳定时间
    std::thread::sleep(Duration::from_millis(1500));

    session.write_chunked(payload, chunk_size, delay_ms);
    session.wait_quiet(1200, Duration::from_secs(20));

    // 打最终屏幕快照的纯文本行（过滤纯控制序列行），供人工读取编辑器内容
    let tail = session.snapshot_tail(14000);
    let visible: String = tail
        .chars()
        .filter(|c| *c != '\u{1b}')
        .collect();
    println!("---- [{}] 最终屏幕尾部（剥离 ESC） ----", case);
    println!("{}", visible);
    println!("---- end ----");

    let has_head = tail.contains("key_0");
    let last_key = {
        // 找 payload 中最大的 key_N
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
    };
    let has_tail = tail.contains(&last_key);
    let has_chip = tail.contains("Pasted text") || tail.contains("pasted text");
    let has_hint = tail.contains("paste again to expand");
    // 整流判定：累积 VT 流里渲染过的行都在。全流含 head+tail = 内容曾完整进入编辑器，
    // 终屏缺 head 只是滚出视口；全流缺 head = 真截断。
    let full = session.snapshot_all();
    let full_head = full.contains("key_0");
    let full_tail = full.contains(&last_key);
    println!(
        "[{}] 结果: 终屏 head={} tail({})={} chip={} hint={} | 全流 head={} tail={}",
        case, has_head, last_key, has_tail, has_chip, has_hint, full_head, full_tail
    );
    if has_tail && !has_head && !has_chip && !full_head {
        panic!("[RED] {} 复现用户症状：全流与终屏均无头部（真截断）", case);
    }
    if has_tail && !has_head && !has_chip && full_head {
        println!("[NOTE] {} 终屏缺头但全流有头：视口滚动，非截断", case);
    }
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
    let value: serde_json::Value = serde_json::from_str(&pretty).expect("payload 必须是合法 JSON");
    let payload = serde_json::to_string(&value).expect("minify");
    assert!(!payload.contains('\n'), "minify 后必须单行");
    let tail_key = last_key_of(&payload);
    for rep in 1..=3 {
        let mut session = spawn_claude("E:/source/github/cc-box");
        std::thread::sleep(Duration::from_millis(1500));
        session.write_chunked(&payload, 0, 0);
        session.wait_quiet(1200, Duration::from_secs(20));
        let full = session.snapshot_all();
        let chip = full.contains("Pasted text") || full.contains("paste again to expand");
        // 单行未折叠时光标在末尾，尾部必在渲染区（key_0 会折行滚出视口，不能用）；
        // tail_key 缺失 = 尾部真丢失。chip=true = 粘贴被识别接管，内容由 chip 保存。
        let tail_ok = full.contains(&tail_key);
        println!(
            "[minified-rep-{}] chip={} head={} tail={}",
            rep,
            chip,
            full.contains("key_0"),
            tail_ok
        );
        assert!(
            chip || tail_ok,
            "[RED] minified-rep-{} 尾部丢失且未被 chip 识别",
            rep
        );
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
