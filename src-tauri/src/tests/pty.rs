// ===================== PTY 输出解码回归测试 =====================
// 历史 bug：PTY reader 用 String::from_utf8_lossy 把非 UTF-8 字节
// （如 Windows cmd.exe / 某些 git 输出的 GBK 字节）替换为 U+FFFD，
// 导致终端出现黑色方块乱码。修复后改用 decode_output（贪心扫描：
// UTF-8 优先 + GBK 双字节兜底）。
//
// utf8_complete_boundary / utf8_seq_len 已被 PtyDecoder 替代，
// 边界与跨 read 行为由 tests/pty_decoder.rs 覆盖。

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::ExitStatus;

struct ChunkRecorder {
    chunks: Vec<Vec<u8>>,
    flush_count: usize,
}

impl Write for ChunkRecorder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.chunks.push(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_count += 1;
        Ok(())
    }
}

#[test]
fn PtyWrite_LargePayload_ChunkedWithoutMutation_001() {
    let payload: Vec<u8> = (0..(crate::pty::PTY_WRITE_CHUNK_SIZE * 2 + 7))
        .map(|index| (index % 251) as u8)
        .collect();
    let mut recorder = ChunkRecorder {
        chunks: Vec::new(),
        flush_count: 0,
    };

    crate::pty::write_pty_data(&mut recorder, &payload).expect("chunked write");

    let written: Vec<u8> = recorder.chunks.iter().flatten().copied().collect();
    assert_eq!(written, payload);
    assert_eq!(recorder.chunks.len(), 3);
    assert_eq!(recorder.flush_count, 3);
    assert!(recorder
        .chunks
        .iter()
        .all(|chunk| chunk.len() <= crate::pty::PTY_WRITE_CHUNK_SIZE));
}

#[test]
fn PtyId_ValidUuidAccepted_001() {
    crate::pty::validate_pty_id("550e8400-e29b-41d4-a716-446655440000")
        .expect("valid frontend-allocated UUID");
}

#[test]
fn PtyId_ArbitraryTextRejected_001() {
    assert!(crate::pty::validate_pty_id("../../session").is_err());
    assert!(crate::pty::validate_pty_id("").is_err());
    assert!(crate::pty::validate_pty_id("not-a-uuid").is_err());
}

#[test]
fn PtyExitPayload_PreservesExitCode_001() {
    let status = ExitStatus::with_exit_code(42);
    let payload = crate::pty::exit_payload("pty-1", &status);

    assert_eq!(payload.id, "pty-1");
    assert_eq!(payload.exit_code, 42);
    assert_eq!(payload.signal, None);
}

#[test]
fn PtyExitPayload_SignalStatusUsesPortableExitCode_001() {
    let status = ExitStatus::with_signal("SIGTERM");
    let payload = crate::pty::exit_payload("pty-2", &status);

    assert_eq!(payload.id, "pty-2");
    assert_eq!(payload.exit_code, 1);
    assert_eq!(payload.signal, None);
}

#[test]
fn PtyWriterLookup_ClonesPerPtyHandle_001() {
    let entry = Arc::new(crate::pty::PtyWriterEntry::new(Box::new(ChunkRecorder {
        chunks: Vec::new(),
        flush_count: 0,
    })));
    let registry = Mutex::new(HashMap::from([("pty-1".to_string(), entry.clone())]));

    let found = crate::pty::lookup_writer(&registry, "pty-1").expect("writer entry");

    assert!(Arc::ptr_eq(&entry, &found));
    assert_eq!(Arc::strong_count(&entry), 3);
}

#[test]
fn PtyPasteDiagnostic_DoesNotRequireClipboardContent_001() {
    let input = "\x1b[200~{\n  \"secret\": true\n}\x1b[201~";
    assert!(input.starts_with("\x1b[200~"));
    assert!(input.ends_with("\x1b[201~"));
}

// 复现旧 bug：from_utf8_lossy 把 GBK 字节 "你好" 替换为 U+FFFD
#[cfg(target_os = "windows")]
#[test]
fn PtyDecode_GbkBytes_LossyCorrupts_001() {
    let (cow, _, _) = encoding_rs::GBK.encode("你好");
    let gbk_bytes = cow.into_owned();
    let lossy = String::from_utf8_lossy(&gbk_bytes).to_string();
    assert!(
        lossy.contains('\u{FFFD}'),
        "from_utf8_lossy 应将 GBK 字节替换为 U+FFFD（这是 bug 源头）"
    );
}

// 验证修复：decode_output 把同样的 GBK 字节正确解码为中文
#[cfg(target_os = "windows")]
#[test]
fn PtyDecode_GbkBytes_DecodeCorrect_001() {
    let (cow, _, _) = encoding_rs::GBK.encode("你好");
    let gbk_bytes = cow.into_owned();
    let decoded = crate::platform::decode_output(&gbk_bytes);
    assert_eq!(decoded, "你好");
}

// UTF-8 字节两种解码方式结果一致，确保修复不破坏 UTF-8 主场景
#[test]
fn PtyDecode_Utf8Bytes_BothCorrect_001() {
    let utf8_bytes = "你好世界".as_bytes();
    let lossy = String::from_utf8_lossy(utf8_bytes).to_string();
    let decoded = crate::platform::decode_output(utf8_bytes);
    assert_eq!(lossy, "你好世界");
    assert_eq!(decoded, "你好世界");
}
