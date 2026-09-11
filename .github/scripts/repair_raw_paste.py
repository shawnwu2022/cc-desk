from __future__ import annotations

import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TEST_FILE = ROOT / "src-tauri/src/tests/paste_framing.rs"
PTY_FILE = ROOT / "src-tauri/src/pty.rs"

RAW_TEST_NAME = "PtyPaste_WindowsMatchesWindowsTerminalRawWrite_009"
RAW_TEST = r'''
#[cfg(windows)]
#[test]
fn PtyPaste_WindowsMatchesWindowsTerminalRawWrite_009() {
    #[derive(Default)]
    struct RecordingWriter {
        writes: Vec<Vec<u8>>,
        flushes: usize,
    }

    impl std::io::Write for RecordingWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.writes.push(data.to_vec());
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    let payload = format!("{OPEN}{{\n  \\"ok\\": true,\n  \\"text\\": \\"中文🎉\\"\n}}{CLOSE}");
    let mut writer = RecordingWriter::default();
    write_pty_data(&mut writer, payload.as_bytes()).unwrap();

    assert_eq!(
        writer.writes,
        vec![payload.into_bytes()],
        "Windows paste must use one unmodified raw bracketed-paste write"
    );
    assert_eq!(
        writer.flushes, 0,
        "the raw paste path must not inject FlushFileBuffers boundaries"
    );
}

'''


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


def replace_file_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    path.write_text(replace_once(text, old, new, str(path)), encoding="utf-8", newline="\n")


def add_test() -> None:
    text = TEST_FILE.read_text(encoding="utf-8")
    if RAW_TEST_NAME in text:
        raise RuntimeError("raw-frame regression already exists")
    marker = "#[cfg(windows)]\nmod native {"
    text = replace_once(text, marker, RAW_TEST + marker, "native module marker")
    TEST_FILE.write_text(text, encoding="utf-8", newline="\n")


def apply_fix() -> None:
    pty = PTY_FILE.read_text(encoding="utf-8")
    start_marker = "/// Preserve complete bracketed-paste frames for Windows ReadConsoleInputW"
    end_marker = "pub(crate) fn write_pty_data<W: Write + ?Sized>("
    if pty.count(start_marker) != 1:
        raise RuntimeError("unable to locate encoded Windows paste implementation")
    start = pty.index(start_marker)
    end = pty.index(end_marker, start)
    replacement = r'''/// Match Windows Terminal paste semantics on Windows: submit the complete,
/// unmodified bracketed-paste frame through one logical pipe write.
///
/// Earlier CC Desk builds rewrote ESC into Win32 INPUT_RECORD encodings and
/// inserted FlushFileBuffers boundaries. That protocol is only valid after
/// win32-input-mode negotiation and leaked `[201~` on Windows 10 build 19045.
/// Claude Code already enables the input mode it requires; the terminal host
/// must preserve the frame instead of inventing a second keyboard protocol.
#[cfg(windows)]
fn write_conpty_paste<W: Write + ?Sized>(writer: &mut W, data: &[u8]) -> io::Result<()> {
    writer.write_all(data)
}

'''
    pty = pty[:start] + replacement + pty[end:]
    pty = replace_once(
        pty,
        '"windows-conpty-atomic-markers"',
        '"windows-conpty-raw-frame"',
        "transport diagnostic",
    )
    PTY_FILE.write_text(pty, encoding="utf-8", newline="\n")

    tests = TEST_FILE.read_text(encoding="utf-8")
    platform_pattern = re.compile(
        r'\s*#\[cfg\(windows\)\]\n\s*let expected = "\\x1b\[0;0;27;1;0;1_\[200~日志\\n\\x1b\[0;0;27;1;0;1_\[31mred\\x1b\[0;0;27;1;0;1_\[201~";\n'
        r'\s*#\[cfg\(not\(windows\)\)\]\n\s*let expected = input\.as_str\(\);\n\s*assert_eq!\(wire, expected\.as_bytes\(\)\);'
    )
    tests, count = platform_pattern.subn("\n    assert_eq!(wire, input.as_bytes());", tests, count=1)
    if count != 1:
        raise RuntimeError("unable to update platform wire expectation")

    old_start = "#[cfg(windows)]\n#[test]\nfn PtyPaste_Win10MarkersAreNeverFlushedAsStandaloneEscape_007() {"
    new_start = f"#[cfg(windows)]\n#[test]\nfn {RAW_TEST_NAME}()"
    if tests.count(old_start) != 1 or tests.count(new_start) != 1:
        raise RuntimeError("unable to locate old/new Windows paste unit contracts")
    block_start = tests.index(old_start)
    block_end = tests.index(new_start, block_start)
    tests = tests[:block_start] + tests[block_end:]

    for name in (
        "PtyPaste_NativeFrames_004",
        "PtyPaste_NativeChunkBoundary_005",
        "PtyPaste_NativeWin10ReportedShape_008",
        "PtyPaste_NativeConsecutiveFrames_006",
    ):
        needle = f"    #[test]\n    fn {name}()"
        replacement = (
            "    #[test]\n"
            "    #[ignore = \"ReadConsoleInputW probe does not model Claude Code VT input; use paste_cli_submit\"]\n"
            f"    fn {name}()"
        )
        tests = replace_once(tests, needle, replacement, name)
    TEST_FILE.write_text(tests, encoding="utf-8", newline="\n")

    cargo = ROOT / "src-tauri/Cargo.toml"
    replace_file_once(
        cargo,
        'portable-pty = { version = "=0.8.1", path = "vendor/portable-pty" }',
        'portable-pty = "=0.8.1"',
    )

    vendor = ROOT / "src-tauri/vendor/portable-pty"
    if not vendor.is_dir():
        raise RuntimeError("portable-pty local patch directory is missing")
    shutil.rmtree(vendor)

    replace_file_once(ROOT / "package.json", '"version": "0.17.4"', '"version": "0.17.5"')
    replace_file_once(
        ROOT / "package-lock.json",
        '"name": "cc-desk",\n  "version": "0.17.4"',
        '"name": "cc-desk",\n  "version": "0.17.5"',
    )
    replace_file_once(
        ROOT / "package-lock.json",
        '"": {\n      "name": "cc-desk",\n      "version": "0.17.4"',
        '"": {\n      "name": "cc-desk",\n      "version": "0.17.5"',
    )
    replace_file_once(
        cargo,
        'name = "cc-desk"\nversion = "0.17.4"',
        'name = "cc-desk"\nversion = "0.17.5"',
    )
    replace_file_once(
        ROOT / "src-tauri/tauri.conf.json",
        '"version": "0.17.4"',
        '"version": "0.17.5"',
    )

    acceptance = ROOT / ".github/workflows/paste-cli-acceptance.yml"
    text = acceptance.read_text(encoding="utf-8")
    text = text.replace("default: '2.1.267'", "default: '2.1.268'")
    text = text.replace("inputs.claude_version || '2.1.267'", "inputs.claude_version || '2.1.268'")
    text = text.replace(
        "      - src-tauri/vendor/portable-pty/**\n",
        "      - src-tauri/Cargo.toml\n      - src-tauri/Cargo.lock\n",
    )
    acceptance.write_text(text, encoding="utf-8", newline="\n")

    docs = ROOT / "docs/paste-framing.md"
    docs.write_text(
        docs.read_text(encoding="utf-8")
        + """

## 0.17.5 架构修正：停止模拟 Win32 键盘事件

同一类大 JSON 在 Windows Terminal、Windows 10 build 19045、native Claude Code
2.1.267/2.1.268 中会被正确识别为 `Pasted text`；CC Desk 0.17.3 和 0.17.4
则泄漏字面 `[201~`。诊断证明完整原文与两端标记在进入 Rust 前均未丢失，
因此故障来自 CC Desk 自行改写 ConPTY 输入协议。

0.17.5 删除 Win32 INPUT_RECORD ESC 编码、逐段 FlushFileBuffers 以及 portable-pty
本地 flush fork。Windows 粘贴与 Windows Terminal 对齐：将
`ESC[200~ + body + ESC[201~` 作为一个未经改写的逻辑 write_all 提交。

旧 Node raw-stdin 探针没有启用 Claude Code 的控制台输入模式，保留为 ignored
诊断而不再充当发布门禁。正式门禁使用真实 Claude Code、隔离配置和
UserPromptSubmit 全文捕获，对提交正文逐字节比较。
""",
        encoding="utf-8",
        newline="\n",
    )

    agents = ROOT / "AGENTS.md"
    agents.write_text(
        agents.read_text(encoding="utf-8")
        + """

### Windows 粘贴架构约束

- 禁止把 bracketed-paste 的 ESC 改写为 Win32 INPUT_RECORD 序列；该协议要求
  `CSI ? 9001 h` 协商，不能由应用猜测。
- Windows 粘贴必须与 Windows Terminal 一致：完整、未经改写的 frame 通过
  一次逻辑 write_all 提交，不在 marker 或正文中插入 FlushFileBuffers 边界。
- Node raw stdin 不等价于 Claude Code 的控制台输入模式；发布门禁必须使用
  `paste_cli_submit` 捕获真实 UserPromptSubmit 正文。
""",
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in {"add-test", "apply"}:
        raise SystemExit("usage: repair_raw_paste.py add-test|apply")
    if sys.argv[1] == "add-test":
        add_test()
    else:
        apply_fix()
