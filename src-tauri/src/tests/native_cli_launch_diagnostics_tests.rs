use super::*;
use std::io::Cursor;

// 检查尚未销毁 writer 时即可读取固定阶段，且记录数及文件字节数保持上限。
#[test]
fn LaunchDiag_ImmediateBound_001() {
    let path = std::env::temp_dir().join(format!("cc-desk-diag-{}-bound", std::process::id()));
    let diagnostics = Diagnostics::new(&path, Mode::Ready);
    diagnostics.mark(Code::InitializeStarted);
    let first = snapshot(&path, Mode::Ready);
    assert!(first.contains("sequence=1 elapsed_ms="), "{first}");
    assert!(first.contains("code=initialize_started"), "{first}");
    for _ in 0..100 {
        diagnostics.mark(Code::ReaderJoinStarted);
    }
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 64 * 16);
    let capped = snapshot(&path, Mode::Ready);
    assert!(capped.starts_with("mode=ready diagnostics=capped\n"));
    assert_eq!(capped.lines().count(), 65);
    drop(diagnostics);
    std::fs::remove_file(path).unwrap();
}

// 检查未知错误、环境值、路径、参数、PTY 字节和换行注入均不能进入输出。
#[test]
fn LaunchDiag_Redaction_002() {
    let path = std::env::temp_dir().join(format!("cc-desk-diag-{}-redact", std::process::id()));
    let diagnostics = Diagnostics::new(&path, Mode::Closed);
    for secret in [
        "TOKEN=secret-value",
        "C:\\private\\user",
        "--password=secret-value",
        "\u{1b}[31mraw PTY bytes",
        "EVAL_FAILED\nsecret-value",
    ] {
        diagnostics.failure(secret);
        diagnostics.observation(secret);
    }
    diagnostics.failure("EVAL_FAILED");
    diagnostics.observation("unready-rejected-before-io");
    let output = snapshot(&path, Mode::Closed);
    assert_eq!(output.matches("code=unknown_failure").count(), 5);
    assert_eq!(output.matches("code=unknown_observation").count(), 5);
    assert!(output.contains("code=EVAL_FAILED"));
    assert!(output.contains("code=unready-rejected-before-io"));
    for forbidden in ["secret-value", "private", "password", "PTY", "\u{1b}"] {
        assert!(!output.contains(forbidden), "unexpected payload: {forbidden}");
    }
    drop(diagnostics);
    std::fs::remove_file(path).unwrap();
}

// 检查实际落盘顺序连续，单调耗时不倒退，且两种模式不能混读。
#[test]
fn LaunchDiag_ModeAndOrder_003() {
    for mode in [Mode::Ready, Mode::Closed] {
        let path = std::env::temp_dir().join(format!(
            "cc-desk-diag-{}-{}",
            std::process::id(),
            mode.name()
        ));
        let diagnostics = Diagnostics::new(&path, mode);
        diagnostics.mark(Code::WorkerStarted);
        diagnostics.mark(Code::RunReturnStarted);
        diagnostics.mark(Code::RunReturned);
        let bytes = std::fs::read(&path).unwrap();
        let mut last_elapsed = 0;
        for (index, record) in bytes.chunks_exact(RECORD_BYTES).enumerate() {
            assert_eq!(record[6] as usize, index + 1);
            let elapsed = u64::from_le_bytes(record[8..16].try_into().unwrap());
            assert!(elapsed >= last_elapsed);
            last_elapsed = elapsed;
        }
        assert!(snapshot(&path, mode).contains("diagnostics=ok"));
        let other = if mode == Mode::Ready {
            Mode::Closed
        } else {
            Mode::Ready
        };
        assert_eq!(
            snapshot(&path, other),
            format!("mode={} diagnostics=invalid", other.name())
        );
        drop(diagnostics);
        std::fs::remove_file(path).unwrap();
    }
}

// 检查记录截断、未知版本/代码、错序、模式混入和耗时倒退全部拒绝。
#[test]
fn LaunchDiag_Malformed_004() {
    let first = encode(Mode::Ready, 1, 10, Code::WorkerStarted);
    let second = encode(Mode::Ready, 2, 20, Code::RunReturnStarted);
    assert!(snapshot_from(Cursor::new([]), Mode::Ready)
        .unwrap()
        .contains("diagnostics=empty"));
    for length in 1..RECORD_BYTES {
        assert_eq!(
            snapshot_from(Cursor::new(&first[..length]), Mode::Ready),
            Err(SnapshotError::Invalid)
        );
    }
    for (offset, value) in [(0, 0), (4, 2), (5, 3), (6, 0), (6, 2), (7, 255)] {
        let mut invalid = first;
        invalid[offset] = value;
        assert_eq!(
            snapshot_from(Cursor::new(invalid), Mode::Ready),
            Err(SnapshotError::Invalid)
        );
    }
    for invalid in [
        encode(Mode::Closed, 2, 20, Code::RunReturnStarted),
        encode(Mode::Ready, 1, 20, Code::RunReturnStarted),
        encode(Mode::Ready, 2, 9, Code::RunReturnStarted),
    ] {
        assert_eq!(
            snapshot_from(Cursor::new([first, invalid].concat()), Mode::Ready),
            Err(SnapshotError::Invalid)
        );
    }
    let mut truncated = [first, second].concat();
    truncated.pop();
    assert_eq!(
        snapshot_from(Cursor::new(truncated), Mode::Ready),
        Err(SnapshotError::Invalid)
    );
}

// 检查超长输入只读取上限加一个探测字节，不把文件内容写入诊断。
#[test]
fn LaunchDiag_BoundedRead_005() {
    let mut input = Cursor::new(vec![b'X'; MAX_BYTES * 10]);
    assert_eq!(
        snapshot_from(&mut input, Mode::Ready),
        Err(SnapshotError::TooLarge)
    );
    assert_eq!(input.position(), (MAX_BYTES + 1) as u64);
}

// 检查读写失败只降低诊断可用性，不抛出原始路径或错误。
#[test]
fn LaunchDiag_IoFailure_006() {
    let missing = std::env::temp_dir().join(format!(
        "cc-desk-diag-{}-missing/secret-value",
        std::process::id()
    ));
    let diagnostics = Diagnostics::new(&missing, Mode::Ready);
    diagnostics.mark(Code::WorkerStarted);
    diagnostics.failure("EVAL_FAILED");
    assert_eq!(
        snapshot(&missing, Mode::Ready),
        "mode=ready diagnostics=unavailable"
    );
    let path = std::env::temp_dir().join(format!("cc-desk-diag-{}-readonly", std::process::id()));
    std::fs::write(&path, []).unwrap();
    let diagnostics = Diagnostics {
        started: Instant::now(),
        mode: Mode::Ready,
        state: Mutex::new(WriterState {
            file: Some(File::open(&path).unwrap()),
            sequence: 0,
        }),
    };
    diagnostics.mark(Code::WorkerStarted);
    diagnostics.mark(Code::RunReturned);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    assert!(diagnostics.state.lock().unwrap().file.is_none());
    drop(diagnostics);
    std::fs::remove_file(path).unwrap();
}

// 检查只写文件的读取失败只返回固定类别。
#[test]
fn LaunchDiag_ReadFailure_007() {
    let path = std::env::temp_dir().join(format!("cc-desk-diag-{}-writeonly", std::process::id()));
    assert_eq!(
        snapshot_from(File::create(&path).unwrap(), Mode::Ready),
        Err(SnapshotError::Unavailable)
    );
    std::fs::remove_file(path).unwrap();
}

// 检查四个并发记录者仍保持连续序号与单调耗时，不能突破 64 条上限。
#[test]
fn LaunchDiag_Concurrent_008() {
    let path = std::env::temp_dir().join(format!("cc-desk-diag-{}-parallel", std::process::id()));
    let diagnostics = std::sync::Arc::new(Diagnostics::new(&path, Mode::Ready));
    let mut threads = vec![];
    for _ in 0..4 {
        let diagnostics = diagnostics.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..32 {
                diagnostics.mark(Code::ReaderJoinStarted);
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    let output = snapshot(&path, Mode::Ready);
    assert!(output.starts_with("mode=ready diagnostics=capped\n"));
    assert_eq!(output.lines().count(), 65);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_BYTES as u64);
    drop(diagnostics);
    std::fs::remove_file(path).unwrap();
}

// 检查模式文本必须精确匹配，环境附加值不能成为模式标记。
#[test]
fn LaunchDiag_ModeAllowlist_009() {
    assert_eq!(Mode::parse("ready"), Some(Mode::Ready));
    assert_eq!(Mode::parse("closed"), Some(Mode::Closed));
    for value in ["", "READY", "closed\nTOKEN=secret", "ready "] {
        assert_eq!(Mode::parse(value), None);
    }
}
