// ===================== PTY 输出解码回归测试 =====================
// 历史 bug：PTY reader 用 String::from_utf8_lossy 把非 UTF-8 字节
// （如 Windows cmd.exe / 某些 git 输出的 GBK 字节）替换为 U+FFFD，
// 导致终端出现黑色方块乱码。修复后改用 decode_output（贪心扫描：
// UTF-8 优先 + GBK 双字节兜底）。
//
// utf8_complete_boundary / utf8_seq_len 已被 PtyDecoder 替代，
// 边界与跨 read 行为由 tests/pty_decoder.rs 覆盖。

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
