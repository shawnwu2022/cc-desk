//! 粘贴帧回归：同时校验起止标记和完整正文，而不是剥除/容忍丢失的标记。
//! Windows 测试使用生产 write_pty_data -> 真 ConPTY -> Node raw stdin。
//! 这验证传输契约，不等同于已验证真实 Claude Code 编辑器或用户剪贴板。

use crate::pty::write_pty_data;

const OPEN: &str = "\x1b[200~";
const CLOSE: &str = "\x1b[201~";

// 普通键盘、图片粘贴键、终端应答和未成帧输入不得被当作粘贴编码。
#[test]
fn PtyPaste_NonPasteInputUnchanged_001() {
    for input in [
        "",
        "plain 中文🎉",
        "\x03",
        "\x16",
        "\x1bv",
        "\x1b[A",
        OPEN,
        CLOSE,
    ] {
        let mut wire = Vec::new();
        write_pty_data(&mut wire, input.as_bytes()).unwrap();
        assert_eq!(wire, input.as_bytes());
    }
}

#[test]
fn PtyPaste_PlatformWire_002() {
    let input = format!("{OPEN}日志\n\x1b[31mred{CLOSE}");
    let mut wire = Vec::new();
    write_pty_data(&mut wire, input.as_bytes()).unwrap();
    #[cfg(windows)]
    let expected = "\x1b[0;0;27;1;0;1_[200~日志\n\x1b[0;0;27;1;0;1_[31mred\x1b[0;0;27;1;0;1_[201~";
    #[cfg(not(windows))]
    let expected = input.as_str();
    assert_eq!(wire, expected.as_bytes());
}

// 下层失败必须上报；不得静默继续或自动重发已经写入的前缀。
#[test]
fn PtyPaste_WriteFailurePropagates_003() {
    struct BrokenWriter;
    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let result = write_pty_data(&mut BrokenWriter, format!("{OPEN}text{CLOSE}").as_bytes());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
}

#[cfg(windows)]
mod native {
    use super::{write_pty_data, CLOSE, OPEN};
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(90);
    const END: &str = "__CC_DESK_NATIVE_PASTE_END__";
    static NATIVE_LOCK: Mutex<()> = Mutex::new(());
    const SCRIPT: &str = r#"
process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
let input = '';
process.stdin.on('data', chunk => {
  input += chunk;
  const end = input.indexOf('__CC_DESK_NATIVE_PASTE_END__');
  if (end < 0) return;
  const value = input.slice(0, end);
  const bytes = Buffer.from(value, 'utf8');
  let hash = 2166136261 >>> 0;
  for (const byte of bytes) { hash ^= byte; hash = Math.imul(hash, 16777619) >>> 0; }
  console.log('PASTE_BYTES:' + bytes.length + ':');
  console.log('PASTE_HASH:' + hash + ':');
  console.log('PASTE_OPEN:' + Number(value.startsWith('\x1b[200~')) + ':');
  console.log('PASTE_CLOSE:' + Number(value.endsWith('\x1b[201~')) + ':');
  console.log('PASTE_RAW:' + Number(process.stdin.isRaw) + ':');
  console.log('CC_DESK_PASTE_DONE');
  process.exit(0);
});
process.stdout.write('\x1b[?2004hCC_DESK_PASTE_READY\r\n');
setTimeout(() => process.exit(2), 85000);
"#;

    fn node_program() -> PathBuf {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .map(|path| path.join("node.exe"))
            .find(|path| path.is_file())
            .expect("Node.js is required for the native paste framing regression")
    }

    fn wait_for(output: &Arc<Mutex<String>>, marker: &str) -> Result<(), String> {
        let start = Instant::now();
        while start.elapsed() < TIMEOUT {
            if output.lock().unwrap().contains(marker) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!("native paste probe timed out waiting for {marker}"))
    }

    fn field(output: &str, key: &str) -> Option<u64> {
        output
            .split_once(key)?
            .1
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    }

    fn hash(bytes: &[u8]) -> u32 {
        bytes.iter().fold(2166136261u32, |h, b| {
            (h ^ u32::from(*b)).wrapping_mul(16777619)
        })
    }

    fn assert_native_frames(name: &str, payloads: &[String]) {
        let _guard = NATIVE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 180,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open ConPTY");
        let mut cmd = CommandBuilder::new(node_program());
        cmd.arg("-e");
        cmd.arg(SCRIPT);
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .expect("spawn raw stdin probe");
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().expect("clone reader");
        let mut writer = pair.master.take_writer().expect("take writer");
        let output = Arc::new(Mutex::new(String::new()));
        let capture = output.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                // 子进程只回报 ASCII 长度/哈希，不把粘贴正文写进日志。
                let mut text = capture.lock().unwrap();
                if text.len() < 64 * 1024 {
                    text.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
            }
        });
        // write_all 也可能阻塞；独立 watchdog 保证失效测试不无限占用 runner。
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let mut killer = child.clone_killer();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                done_rx.recv_timeout(TIMEOUT),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                let _ = killer.kill();
            }
        });
        let result = (|| -> Result<(), String> {
            wait_for(&output, "CC_DESK_PASTE_READY")?;
            for payload in payloads {
                // 必须使用真实生产 writer，不能在测试中复制分块/编码算法。
                write_pty_data(&mut *writer, payload.as_bytes()).map_err(|e| e.to_string())?;
            }
            writer
                .write_all(END.as_bytes())
                .map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
            wait_for(&output, "CC_DESK_PASTE_DONE")
        })();
        let _ = done_tx.send(());
        let _ = child.kill();
        let _ = child.wait();
        drop(writer);
        drop(pair.master);
        reader_thread.join().expect("reader thread");
        watchdog.join().expect("watchdog thread");
        result.expect("native paste transport completed");
        let output = output.lock().unwrap();
        let expected = payloads.concat();
        assert_eq!(
            field(&output, "PASTE_RAW:"),
            Some(1),
            "probe must use raw mode"
        );
        assert_eq!(
            field(&output, "PASTE_BYTES:"),
            Some(expected.len() as u64),
            "paste frame byte length mismatch: {name}"
        );
        assert_eq!(
            field(&output, "PASTE_HASH:"),
            Some(u64::from(hash(expected.as_bytes()))),
            "paste frame content mismatch: {name}"
        );
        assert_eq!(
            field(&output, "PASTE_OPEN:"),
            Some(1),
            "paste start delimiter missing"
        );
        assert_eq!(
            field(&output, "PASTE_CLOSE:"),
            Some(1),
            "paste end delimiter missing"
        );
        assert!(
            output.contains("\x1b[?2004h"),
            "ConPTY must forward the application's bracketed paste mode to xterm"
        );
        println!(
            "[PASS] {name}: {} UTF-8 bytes including intact paste delimiters",
            expected.len()
        );
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        name: String,
        prepared: String,
        repeat: usize,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        separator: String,
    }

    #[test]
    fn PtyPaste_NativeFrames_004() {
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../tests/fixtures/devtools-paste-framing.json"
        ))
        .unwrap();
        for fixture in fixtures {
            let body = vec![fixture.prepared.as_str(); fixture.repeat].join(&fixture.separator);
            let payload = format!("{OPEN}{}{body}{}{CLOSE}", fixture.prefix, fixture.suffix);
            assert_native_frames(&fixture.name, &[payload]);
        }
    }

    #[test]
    fn PtyPaste_NativeChunkBoundary_005() {
        // ESC 位于一个 4 KiB 写块的最后一字节，下一块才有 CSI 参数。
        let payload = format!("{OPEN}{}\x1b[31m中文🎉\nTAIL{CLOSE}", "x".repeat(4075));
        assert_native_frames("split-escape-sequence", &[payload]);
        for padding in 4060..4098 {
            let payload = format!("{OPEN}{}\x1b[31m中文🎉\nTAIL{CLOSE}", "x".repeat(padding));
            assert_native_frames(&format!("escape-offset-{padding}"), &[payload]);
        }
        let payload = format!("{OPEN}{}{CLOSE}", "\x1b[31m色\x1b[0m\n".repeat(400));
        assert_native_frames("dense-literal-escapes", &[payload]);
    }

    #[test]
    fn PtyPaste_NativeConsecutiveFrames_006() {
        let payloads = [
            format!("{OPEN}first\n一{CLOSE}"),
            format!("{OPEN}second\n二{CLOSE}"),
        ];
        assert_native_frames("consecutive-independent-pastes", &payloads);
    }
}
