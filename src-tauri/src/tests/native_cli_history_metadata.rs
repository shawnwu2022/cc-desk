use super::*;
use serde_json::json;

// 完整历史只返回元数据；保持身份、首个用户标题和完整时间，不携带消息正文。
#[test]
#[allow(non_snake_case)]
fn HistoryParse_SkipBodies_001() {
    let input = format!(
        "{}\n{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"synthetic-session","cwd":"/synthetic/project"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"synthetic task"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"synthetic task"}]}}),
        json!({"type":"response_item","timestamp":"2026-10-10T00:00:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x".repeat(48 * 1024)}]}}),
    );
    let transcript = parse("sessions/rollout-fixture.jsonl", CliKind::Codex, &input).unwrap();
    assert_eq!(transcript.id, "synthetic-session");
    assert_eq!(transcript.cwd.as_deref(), Some("/synthetic/project"));
    assert_eq!(transcript.title, "synthetic task");
    assert_eq!(transcript.updated.as_deref(), Some("2026-10-10T00:00:00Z"));
    assert!(
        transcript.messages.is_empty(),
        "history metadata must not retain user or assistant bodies"
    );
}

// 历史元数据仍拒绝事件与规范用户流的次数差异，不能因省略正文而放宽完整性。
#[test]
#[allow(non_snake_case)]
fn HistoryParse_KeepStreams_002() {
    let header = json!({"type":"session_meta","payload":{"id":"synthetic-session"}});
    let event = json!({"type":"event_msg","payload":{"type":"user_message","message":"same turn"}});
    let canonical = json!({"type":"response_item","payload":{"type":"message","role":"user","content":"same turn"}});
    assert_eq!(
        parse(
            "fixture.jsonl",
            CliKind::Codex,
            &format!("{header}\n{event}\n{event}\n{canonical}\n")
        )
        .err(),
        Some("SOURCE_AMBIGUOUS")
    );
}

// 不提取助手正文时仍校验该记录中的转义 NUL 和损坏 JSON。
#[test]
#[allow(non_snake_case)]
fn HistoryParse_ValidateBodies_003() {
    let header = r#"{"type":"session_meta","payload":{"id":"synthetic-session"}}"#;
    for (record, code) in [
        (
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":"bad\u0000text"}}"#,
            "SOURCE_INVALID_TEXT",
        ),
        ("{broken}", "SOURCE_INVALID"),
    ] {
        assert_eq!(
            parse(
                "fixture.jsonl",
                CliKind::Codex,
                &format!("{header}\n{record}\n")
            )
            .err(),
            Some(code)
        );
    }
}

// Claude 完整历史保留末尾标题、末尾时间和既有 cwd 规则，消息详情继续保留正文。
#[test]
#[allow(non_snake_case)]
fn HistoryParse_ClaudeMetadata_004() {
    let input = concat!(
        "{\"type\":\"user\",\"cwd\":\"/first\",\"message\":{\"content\":\"\"}}\n",
        "{\"type\":\"user\",\"message\":{\"content\":\"first task\"}}\n",
        "{\"type\":\"user\",\"message\":{\"content\":\"second task\"}}\n",
        "{\"type\":\"custom-title\",\"customTitle\":\"old title\"}\n",
        "{\"type\":\"custom-title\",\"customTitle\":\"latest title\"}\n",
        "{\"type\":\"assistant\",\"cwd\":\"/last\",\"timestamp\":\"2026-10-10T01:00:00Z\",\"message\":{\"content\":\"answer\"}}\n",
    );
    let metadata = parse("fixture.jsonl", CliKind::Claude, input).unwrap();
    let full = parse_complete("fixture.jsonl", CliKind::Claude, input, true).unwrap();
    assert_eq!(metadata.title, full.title);
    assert_eq!(metadata.title, "latest title");
    assert_eq!(metadata.cwd, full.cwd);
    assert_eq!(metadata.cwd.as_deref(), Some("/last"));
    assert_eq!(metadata.updated, full.updated);
    assert_eq!(full.messages.len(), 3);
    assert!(metadata.messages.is_empty());
}

// 256 个带 8KiB 头及 48KiB 助手正文的合成会话，列表只读元数据并明确标记未读取正文。
#[test]
#[allow(non_snake_case)]
fn HistorySource_Load256_005() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions/2026/10/10");
    std::fs::create_dir_all(&directory).unwrap();
    for index in 0..256 {
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":format!("synthetic-{index:03}"),"cwd":temp.path().to_str().unwrap(),"base_instructions":"i".repeat(8 * 1024)}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"synthetic task"}}),
            json!({"type":"response_item","timestamp":"2026-10-10T01:00:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"a".repeat(48 * 1024)}]}}),
        );
        assert!(input.len() < 64 * 1024);
        std::fs::write(directory.join(format!("rollout-{index:03}.jsonl")), input).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let catalog = Catalog {
        cli: CliKind::Codex,
        root: &root,
        project: Some(&root),
        project_paths: &[temp.path().to_owned()],
        user_config: None,
        check: &|| Ok(()),
    };
    let items = read(
        &catalog,
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .unwrap();
    assert_eq!(items.len(), 256);
    for item in items {
        let ResourceItem::Session {
            title,
            truncated,
            updated_at,
            ..
        } = item
        else {
            panic!("history must return only session metadata")
        };
        assert_eq!(title, "synthetic task");
        assert!(truncated);
        assert!(updated_at.is_none());
    }
}

// 1677 个稀疏大文件分布在 159 个目录中；历史只读取完整元数据记录，原 16MiB 总量预算内保留全部身份。
#[test]
#[allow(non_snake_case)]
fn HistorySource_Sparse1677_006() {
    use std::io::{Seek, SeekFrom, Write};
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("archived_sessions")).unwrap();
    for month in 1..=12 {
        for day in 1..=12 {
            std::fs::create_dir_all(
                temp.path()
                    .join(format!("sessions/2026/{month:02}/{day:02}")),
            )
            .unwrap();
        }
    }
    for index in 0..1677 {
        let month = index % 144 / 12 + 1;
        let day = index % 12 + 1;
        let path = temp.path().join(format!(
            "sessions/2026/{month:02}/{day:02}/rollout-{index:04}.jsonl"
        ));
        let input = format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":format!("synthetic-{index:04}"),"cwd":temp.path().to_str().unwrap(),"base_instructions":"i".repeat(1024)}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"synthetic task"}}),
        );
        assert!(
            input.len() < 4 * 1024,
            "both complete metadata records fit the first sample"
        );
        let length = match index {
            0 => 284_000_000,
            1..=63 => 10 * 1024 * 1024,
            _ => 1_887_437,
        };
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(input.as_bytes()).unwrap();
        file.set_len(length).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(b"\n").unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let started = std::time::Instant::now();
    let items = read(
        &Catalog {cli: CliKind::Codex, root: &root, project: Some(&root), project_paths: &[temp.path().to_owned()], user_config: None, check: &|| Ok(())},
        &Options {kind: ResourceKind::History, query: None, session_id: None},
        &mut budget,
    ).expect("metadata sampling must fit 1677 sparse histories within the existing aggregate and entry caps");
    eprintln!(
        "synthetic 1677 identity observations: {:?}; each first sample = 4096 charged bytes",
        started.elapsed()
    );
    assert_eq!(items.len(), 1677);
    assert!(budget.history_metadata_incomplete());
    let mut ids = BTreeSet::new();
    for item in items {
        let ResourceItem::Session {
            native_session_id,
            title,
            truncated,
            updated_at,
            ..
        } = item
        else {
            panic!("history must return only session metadata")
        };
        assert!(ids.insert(native_session_id));
        assert_eq!(title, "synthetic task");
        assert!(truncated);
        assert!(updated_at.is_none());
    }
}

// 项目筛选移除另一条记录前，采样头中重复的 Codex 身份仍拒绝整个来源。
#[test]
#[allow(non_snake_case)]
fn HistorySource_DuplicateHeaders_007() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions");
    std::fs::create_dir_all(&directory).unwrap();
    for (name, cwd) in [
        ("a", temp.path().to_str().unwrap()),
        ("b", "/synthetic/foreign"),
    ] {
        let mut file = std::fs::File::create(directory.join(format!("{name}.jsonl"))).unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"id":"duplicate","cwd":cwd}})
        )
        .unwrap();
        file.set_len(3 * 1024 * 1024).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let result = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            project: Some(&root),
            project_paths: &[temp.path().to_owned()],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    );
    assert_eq!(result.err(), Some("SOURCE_AMBIGUOUS"));
}

// 230KiB 的完整有效身份头无需标题即可列出；未知正文不产生虚构标题、时间或完整性。
#[test]
#[allow(non_snake_case)]
fn HistorySource_LongNoTitle_008() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    let mut file = std::fs::File::create(temp.path().join("sessions/long.jsonl")).unwrap();
    writeln!(file, "{}", json!({"type":"session_meta","payload":{"id":"long-no-title","cwd":temp.path().to_str().unwrap(),"base_instructions":{"text":"i".repeat(230 * 1024)}}})).unwrap();
    file.set_len(32 * 1024 * 1024).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            project: None,
            project_paths: &[],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut budget,
    )
    .unwrap();
    assert!(budget.history_metadata_incomplete());
    assert_eq!(items.len(), 1);
    let ResourceItem::Session {
        native_session_id,
        title,
        truncated,
        updated_at,
        ..
    } = &items[0]
    else {
        panic!("history must return only session metadata")
    };
    assert_eq!(native_session_id, "long-no-title");
    assert_eq!(title, "Untitled");
    assert!(*truncated);
    assert!(updated_at.is_none());
}

// 1677 个 12KiB 身份头仍受原 16MiB 总量限制，元数据采样不能扩大或吞掉全局上限。
#[test]
#[allow(non_snake_case)]
fn HistorySource_KeepHeaderCap_009() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    for index in 0..1677 {
        let mut file =
            std::fs::File::create(temp.path().join(format!("sessions/{index:04}.jsonl"))).unwrap();
        writeln!(file, "{}", json!({"type":"session_meta","payload":{"id":format!("synthetic-{index:04}"),"base_instructions":"i".repeat(12 * 1024)}})).unwrap();
        file.set_len(32 * 1024 * 1024).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let result = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            project: None,
            project_paths: &[],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    );
    assert_eq!(result.err(), Some("SOURCE_TOO_LARGE"));
}

// Claude 首个快照无 cwd 时仍扫描旧 64KiB 窗口，保留随后用户记录的项目、标题与完整原生时间。
#[test]
#[allow(non_snake_case)]
fn HistorySource_ClaudeWindow_010() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let input = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        json!({"type":"file-history-snapshot","snapshot":{}}),
        json!({"type":"system","detail":"i".repeat(24 * 1024)}),
        json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"claude task"}}),
        json!({"type":"custom-title","customTitle":"latest title"}),
        json!({"type":"assistant","timestamp":"2026-10-10T02:00:00Z","message":{"content":"answer"}}),
    );
    std::fs::write(temp.path().join("projects/p/claude-fixture.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            project: Some(&root),
            project_paths: &[temp.path().to_owned()],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut budget,
    )
    .unwrap();
    assert_eq!(
        items.len(),
        1,
        "Claude project history must keep cwd observed later in the existing metadata window"
    );
    let ResourceItem::Session {
        title,
        truncated,
        updated_at,
        ..
    } = &items[0]
    else {
        panic!("history must return only session metadata")
    };
    assert_eq!(title, "latest title");
    assert!(!truncated);
    assert_eq!(updated_at.as_deref(), Some("2026-10-10T02:00:00Z"));
    assert!(!budget.history_metadata_incomplete());
}

// 首个用户标题记录在采样块之外时保持未知；未读取的原生末尾时间不能替换成文件时间。
#[test]
#[allow(non_snake_case)]
fn HistorySource_UnseenTitle_011() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"unseen-title","cwd":temp.path().to_str().unwrap(),"base_instructions":"i".repeat(1024)}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"u".repeat(6 * 1024)}}),
        json!({"type":"response_item","timestamp":"2026-10-10T03:00:00Z","payload":{"type":"message","role":"assistant","content":"answer"}}),
    );
    std::fs::write(temp.path().join("sessions/title.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            project: Some(&root),
            project_paths: &[temp.path().to_owned()],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut budget,
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    let ResourceItem::Session {
        native_session_id,
        title,
        truncated,
        updated_at,
        ..
    } = &items[0]
    else {
        panic!("history must return only session metadata")
    };
    assert_eq!(native_session_id, "unseen-title");
    assert_eq!(title, "Untitled");
    assert!(*truncated);
    assert!(updated_at.is_none());
    assert!(budget.history_metadata_incomplete());
}
