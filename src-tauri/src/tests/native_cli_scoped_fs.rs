use super::*;
use std::fs;
fn budget() -> Budget {
    Budget::new(Limits::default())
}

// 隔离样例量化 Claude 冷/暖枚举的预算扣减；不把 OS 缓存或标题命中率当作零读取。
#[test]
#[allow(non_snake_case)]
fn HistoryScan_ColdWarmBytes_045() {
    use super::super::catalog::{Catalog, Options, ProjectRef};
    use super::super::wire::ResourceKind;
    use crate::cli::types::CliKind;
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    for index in 0..64 {
        let id = format!("sample-{index}");
        let records = [
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"sessionId":id,"message":{"content":"synthetic task"}}),
            json!({"type":"assistant","sessionId":id,"message":{"content":"a".repeat(96 * 1024)}}),
            json!({"type":"ai-title","sessionId":id,"aiTitle":"synthetic title"}),
        ];
        let input = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        fs::write(temp.path().join(format!("projects/p/{id}.jsonl")), input).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let paths = vec![temp.path().to_path_buf()];
    let projects = [ProjectRef {
        id: "p",
        root: &root,
        paths: &paths,
    }];
    let catalog = Catalog {
        cli: CliKind::Claude,
        root: &root,
        projects: &projects,
        user_config: None,
        check: &|| Ok(()),
    };
    let mut measured = Vec::new();
    for label in ["cold", "warm"] {
        let limits = Limits::default();
        let mut budget = Budget::new(limits);
        let started = Instant::now();
        let items = super::super::catalog::read(
            &catalog,
            &Options {
                kind: ResourceKind::History,
                query: None,
                session_id: None,
            },
            &mut budget,
        )
        .unwrap();
        let charged = limits.total_bytes - budget.bytes_left;
        eprintln!(
            "Claude synthetic {label}: files=64 rows={} charged_bytes={charged} elapsed_ms={}",
            items.len(),
            started.elapsed().as_millis()
        );
        assert_eq!(items.len(), 64);
        assert!(charged > 0 && charged <= 64 * 80 * 1024);
        measured.push(charged);
    }
    assert_eq!(
        measured[0], measured[1],
        "Claude still freshly reads both observations"
    );
}

// Validate the production decoder without asking a filesystem to create an
// invalid name: macOS rejects non-UTF-8 filenames before enumeration can begin.
#[test]
#[cfg(any(unix, windows))]
fn invalid_filename_encoding_fails_closed_without_filesystem_creation() {
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    #[cfg(windows)]
    let name = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    assert_eq!(
        relative(Path::new(&name), false),
        Err("SOURCE_INVALID_TEXT")
    );
}

#[test]
fn same_name_is_read_from_each_held_root() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("same.json"), b"LEFT").unwrap();
    fs::write(b.join("same.json"), b"RIGHT").unwrap();
    let left = Root::open(&a).unwrap();
    let right = Root::open(&b).unwrap();
    assert_ne!(left.key(), right.key());
    assert_eq!(
        left.read(Path::new("same.json"), &mut budget()).unwrap(),
        Some(b"LEFT".to_vec())
    );
    assert_eq!(
        right.read(Path::new("same.json"), &mut budget()).unwrap(),
        Some(b"RIGHT".to_vec())
    );
    left.current().unwrap();
    right.current().unwrap();
}
#[test]
fn absent_root_is_not_defaulted_or_created() {
    let t = tempfile::tempdir().unwrap();
    let missing = t.path().join("missing");
    assert!(Root::open(&missing).is_err());
    assert!(!missing.exists());
    assert!(Root::open(Path::new("relative")).is_err());
}
#[test]
fn missing_optional_file_is_distinct_from_permission_failure() {
    let t = tempfile::tempdir().unwrap();
    let r = Root::open(t.path()).unwrap();
    assert_eq!(
        r.read(Path::new("missing.json"), &mut budget()).unwrap(),
        None
    );
    assert!(r.read(Path::new("../secret"), &mut budget()).is_err());
    assert!(r.read(t.path(), &mut budget()).is_err());
    assert!(r.read(Path::new("x:alternate"), &mut budget()).is_err());
    assert!(r.read(Path::new("x\\y"), &mut budget()).is_err());
}
#[test]
fn file_and_aggregate_budgets_reject_without_truncation() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("x"), b"12345").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = Budget::new(Limits {
        file_bytes: 4,
        total_bytes: 100,
        entries: 10,
    });
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap_err(),
        "SOURCE_TOO_LARGE"
    );
    let mut b = Budget::new(Limits {
        file_bytes: 5,
        total_bytes: 5,
        entries: 10,
    });
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap(),
        Some(b"12345".to_vec())
    );
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap_err(),
        "SOURCE_TOO_LARGE"
    );
}

#[test]
fn history_prefix_preserves_hard_budgets_and_does_not_redefine_full_reads() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("x"), b"12345").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = Budget::new(Limits {
        file_bytes: 4,
        total_bytes: 4,
        entries: 10,
    });
    assert_eq!(
        r.history_prefix(Path::new("x"), &mut b).unwrap(),
        Some((b"1234".to_vec(), true))
    );
    assert!(b.history_metadata_incomplete());
    assert_eq!(
        r.history_prefix(Path::new("x"), &mut b).err(),
        Some("SOURCE_TOO_LARGE")
    );
    assert!(r.history_prefix(Path::new("../x"), &mut budget()).is_err());
    let mut b = budget();
    assert_eq!(
        r.history_prefix(Path::new("x"), &mut b).unwrap(),
        Some((b"12345".to_vec(), false))
    );
    assert!(!b.history_metadata_incomplete());
}
#[test]
fn enumeration_is_bounded_and_never_returns_partial_success() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("b"), b"2").unwrap();
    fs::write(t.path().join("a"), b"1").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = Budget::new(Limits {
        file_bytes: 100,
        total_bytes: 100,
        entries: 1,
    });
    assert_eq!(
        r.list(Path::new(""), &mut b).err(),
        Some("SOURCE_TOO_MANY_ENTRIES")
    );
    let entries = r.list(Path::new(""), &mut budget()).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(entries.iter().all(|e| e.is_file && !e.is_dir));
}
#[test]
fn held_root_cannot_silently_follow_replacement() {
    let t = tempfile::tempdir().unwrap();
    let active = t.path().join("active");
    fs::create_dir(&active).unwrap();
    fs::write(active.join("x"), b"old").unwrap();
    let r = Root::open(&active).unwrap();
    match fs::rename(&active, t.path().join("retired")) {
        Ok(()) => {
            fs::create_dir(&active).unwrap();
            fs::write(active.join("x"), b"new").unwrap();
            assert_eq!(r.current().err(), Some("SOURCE_CHANGED"));
            assert!(r.read(Path::new("x"), &mut budget()).is_err());
        }
        Err(e) => {
            // Windows directory capabilities intentionally deny delete-sharing.
            #[cfg(not(windows))]
            panic!("unexpected rename denial: {e}");
            #[cfg(windows)]
            {
                // Delete-sharing denial can surface as ERROR_SHARING_VIOLATION
                // rather than Rust's PermissionDenied classification.
                assert!(
                    matches!(e.raw_os_error(), Some(5 | 32)),
                    "unexpected rename error: {e:?}"
                );
                assert_eq!(
                    r.read(Path::new("x"), &mut budget()).unwrap(),
                    Some(b"old".to_vec())
                );
            }
        }
    }
}
#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let inside = t.path().join("inside");
    fs::create_dir(&inside).unwrap();
    fs::write(t.path().join("secret"), b"secret").unwrap();
    symlink(t.path().join("secret"), inside.join("escape")).unwrap();
    let r = Root::open(&inside).unwrap();
    assert!(r.read(Path::new("escape"), &mut budget()).is_err());
}
#[cfg(unix)]
#[test]
fn invalid_unicode_paths_and_stored_names_are_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let t = tempfile::tempdir().unwrap();
    let r = Root::open(t.path()).unwrap();
    let name = std::ffi::OsString::from_vec(vec![255]);
    // This exercises production rejection even when the filesystem itself
    // rejects creating the filename (observed as EILSEQ on the macOS runner).
    assert_eq!(
        r.read(Path::new(&name), &mut budget()).err(),
        Some("SOURCE_INVALID_TEXT")
    );
    match fs::write(t.path().join(&name), b"bad-name") {
        Ok(()) => assert_eq!(
            r.list(Path::new(""), &mut budget()).err(),
            Some("SOURCE_INVALID_TEXT")
        ),
        Err(error) => {
            #[cfg(target_os = "macos")]
            {
                assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
                assert!(r.list(Path::new(""), &mut budget()).unwrap().is_empty());
            }
            #[cfg(not(target_os = "macos"))]
            panic!("unexpected invalid-name fixture failure: {error}");
        }
    }
}
#[cfg(windows)]
#[test]
fn windows_junction_escape_cannot_read_external_bytes() {
    use std::os::windows::process::CommandExt;
    let t = tempfile::tempdir().unwrap();
    let inside = t.path().join("inside");
    let outside = t.path().join("outside");
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), b"secret").unwrap();
    let status = std::process::Command::new("cmd.exe")
        .creation_flags(0x08000000)
        .args(["/D", "/C", "mklink", "/J"])
        .arg(inside.join("escape"))
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    let r = Root::open(&inside).unwrap();
    assert!(r.read(Path::new("escape/secret"), &mut budget()).is_err());
}

// 过期的五秒协作预算使用独立固定码，不能与文件、总字节或条目上限混淆。
#[test]
#[allow(non_snake_case)]
fn SourceBudget_DeadlineCode_001() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("x"), b"{}").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = budget();
    b.started = Instant::now() - Duration::from_secs(121);
    assert_eq!(
        r.history_prefix(Path::new("x"), &mut b).err(),
        Some("SOURCE_BUDGET_EXCEEDED")
    );
    assert!(!b.omit_history_entry("SOURCE_BUDGET_EXCEEDED"));
}

// Claude 首行换行位于第九个 64KiB 块时仍继续读取，随后保持前缀未完整与原总量扣减。
#[test]
#[allow(non_snake_case)]
fn HistoryPrefix_LongFirstLine_001() {
    let t = tempfile::tempdir().unwrap();
    let mut bytes = vec![b'x'; 8 * 64 * 1024];
    bytes.extend_from_slice(b"\n{}");
    bytes.extend(vec![b'y'; 64 * 1024]);
    fs::write(t.path().join("x"), &bytes).unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = budget();
    let (observed, incomplete) = r.history_prefix(Path::new("x"), &mut b).unwrap().unwrap();
    assert_eq!(observed.len(), 9 * 64 * 1024);
    assert!(observed == bytes[..observed.len()]);
    assert!(incomplete);
    assert!(b.history_metadata_incomplete());
    assert_eq!(b.bytes_left, Limits::default().total_bytes - observed.len());
}

// Codex 完整首行位于首个 4KiB 块时只扣减实际读取块，Messages 的完整文件限制保持不变。
#[test]
#[allow(non_snake_case)]
fn CodexHeader_ChargeSample_002() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let mut file = fs::File::create(t.path().join("x")).unwrap();
    file.write_all(b"{}\n").unwrap();
    file.set_len(3 * 1024 * 1024).unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = budget();
    let (observed, incomplete) = r.history_header(Path::new("x"), &mut b).unwrap().unwrap();
    assert_eq!(observed.len(), 4 * 1024);
    assert!(observed.starts_with(b"{}\n"));
    assert!(incomplete);
    assert!(b.history_metadata_incomplete());
    assert_eq!(b.bytes_left, Limits::default().total_bytes - observed.len());
    assert_eq!(
        r.read(Path::new("x"), &mut budget()).err(),
        Some("SOURCE_TOO_LARGE")
    );
}

// 压缩输入与解压输出分别计入同一聚合预算，恰好用尽后不能再次读取。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_ChargeInputAndOutput_003() {
    let temp = tempfile::tempdir().unwrap();
    let input = b"hello";
    let encoded = zstd::stream::encode_all(&input[..], 1).unwrap();
    let total = encoded.len() + input.len();
    fs::write(temp.path().join("x.jsonl.zst"), &encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(Limits {
        file_bytes: 1024,
        total_bytes: total,
        entries: 10,
    });
    assert_eq!(
        root.read(Path::new("x.jsonl.zst"), &mut budget).unwrap(),
        Some(input.to_vec())
    );
    assert_eq!(budget.bytes_left, 0);
    assert_eq!(
        root.read(Path::new("x.jsonl.zst"), &mut budget).err(),
        Some("SOURCE_TOO_LARGE")
    );
    assert_eq!(budget.bytes_left, 0);
}

// 高压缩比不能绕过解压输出上限；失败仍扣除已经读取的压缩输入与解压输出。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_DecodedLimit_004() {
    let temp = tempfile::tempdir().unwrap();
    let encoded = zstd::stream::encode_all(&vec![b'x'; 64 * 1024][..], 1).unwrap();
    fs::write(temp.path().join("x.jsonl.zst"), &encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(Limits {
        file_bytes: 1024,
        total_bytes: 10000,
        entries: 10,
    });
    assert_eq!(
        root.read(Path::new("x.jsonl.zst"), &mut budget).err(),
        Some("SOURCE_TOO_LARGE")
    );
    assert_eq!(budget.bytes_left, 10000 - encoded.len() - 1024);
}

// 头窗口受解压后的字节限制，并且不因源文件压缩得很小而变成完整观察。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_BoundedMetadata_005() {
    let temp = tempfile::tempdir().unwrap();
    let input = format!("{{}}\n{}", "x".repeat(3 * 1024 * 1024));
    let encoded = zstd::stream::encode_all(input.as_bytes(), 1).unwrap();
    fs::write(temp.path().join("x.jsonl.zst"), &encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = budget();
    let (head, _, incomplete) = root
        .history_codex_metadata(Path::new("x.jsonl.zst"), &mut budget, &mut |bytes| {
            Ok(bytes.starts_with(b"{}\n"))
        })
        .unwrap()
        .unwrap();
    assert!(head.starts_with(b"{}\n"));
    assert!(head.len() <= CODEX_HEAD_WINDOW);
    assert!(incomplete);
    assert!(budget.history_metadata_incomplete());
    assert!(Limits::default().total_bytes - budget.bytes_left <= CODEX_HEAD_WINDOW + encoded.len());
    assert_eq!(
        root.read(Path::new("../x.jsonl.zst"), &mut budget).err(),
        Some("SOURCE_PATH_REJECTED")
    );
}

// 合法压缩帧也不能要求超出产品上限的解码窗口，避免输出很小时仍大额分配内存。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_WindowLimit_006() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.window_log(25).unwrap();
    encoder.include_contentsize(false).unwrap();
    encoder.write_all(b"small valid payload").unwrap();
    let encoded = encoder.finish().unwrap();
    assert_eq!(
        zstd::stream::decode_all(&encoded[..]).unwrap(),
        b"small valid payload"
    );
    fs::write(temp.path().join("x.jsonl.zst"), &encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    assert_eq!(
        root.read(Path::new("x.jsonl.zst"), &mut budget()).err(),
        Some("SOURCE_INVALID")
    );
}

// 短首 frame 会使输出块不对齐；即使一直未找到标题也不能越过 192KiB 头窗口。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_UnalignedFrameWindow_007() {
    let temp = tempfile::tempdir().unwrap();
    let mut encoded = zstd::stream::encode_all(&b"{}\n"[..], 1).unwrap();
    encoded.extend(zstd::stream::encode_all("record\n".repeat(40 * 1024).as_bytes(), 1).unwrap());
    assert!(encoded.len() < 8192);
    fs::write(temp.path().join("x.jsonl.zst"), &encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = budget();
    let (head, _, incomplete) = root
        .history_codex_metadata(Path::new("x.jsonl.zst"), &mut budget, &mut |_| Ok(false))
        .unwrap()
        .unwrap();
    assert!(incomplete);
    assert!(head.len() <= CODEX_HEAD_WINDOW);
    assert_eq!(
        Limits::default().total_bytes - budget.bytes_left,
        encoded.len() + CODEX_HEAD_WINDOW
    );
}

// 元数据丢弃末尾不完整行后，压缩帧已读完也不能宣称完整观察。
#[test]
#[allow(non_snake_case)]
fn ZstdRead_TruncatedRecordIsIncomplete_008() {
    let temp = tempfile::tempdir().unwrap();
    let input = b"{}\n{\"partial\": true}";
    let encoded = zstd::stream::encode_all(&input[..], 1).unwrap();
    fs::write(temp.path().join("x.jsonl"), input).unwrap();
    fs::write(temp.path().join("x.jsonl.zst"), encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    for path in ["x.jsonl", "x.jsonl.zst"] {
        let mut budget = budget();
        let (head, _, incomplete) = root
            .history_codex_metadata(Path::new(path), &mut budget, &mut |_| Ok(false))
            .unwrap()
            .unwrap();
        assert_eq!(head, b"{}\n");
        assert!(incomplete, "{path}");
        assert!(budget.history_metadata_incomplete(), "{path}");
    }
}
