use super::*;
use serde_json::json;

fn granted<'a>(root: &'a Root, paths: &'a [PathBuf]) -> Vec<ProjectRef<'a>> {
    vec![ProjectRef {
        id: "test-project",
        root,
        paths,
    }]
}

// Large synthetic EOFs must be sparse on NTFS as well as Unix. Mark the same
// held fixture file before extending, then zero only its newly created gap.
// https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_sparse
// https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_zero_data
fn extend_sparse_fixture(file: &std::fs::File, length: u64) {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            FileStandardInfo, GetFileInformationByHandleEx, FILE_STANDARD_INFO,
        };
        use windows::Win32::System::Ioctl::{
            FILE_ZERO_DATA_INFORMATION, FSCTL_SET_SPARSE, FSCTL_SET_ZERO_DATA,
        };
        use windows::Win32::System::IO::DeviceIoControl;

        let previous = file.metadata().unwrap().len();
        assert!(length > previous);
        let handle = HANDLE(file.as_raw_handle());
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_SET_SPARSE,
                None,
                0,
                None,
                0,
                Some(&mut returned),
                None,
            )
            .expect("large history fixture must support explicit sparse marking");
        }
        file.set_len(length).unwrap();
        let gap = FILE_ZERO_DATA_INFORMATION {
            FileOffset: i64::try_from(previous).unwrap(),
            BeyondFinalZero: i64::try_from(length).unwrap(),
        };
        unsafe {
            DeviceIoControl(
                handle,
                FSCTL_SET_ZERO_DATA,
                Some((&gap as *const FILE_ZERO_DATA_INFORMATION).cast()),
                size_of::<FILE_ZERO_DATA_INFORMATION>() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
            .expect("new sparse fixture gap must be zeroed without allocating its logical size");
        }
        let mut standard = FILE_STANDARD_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
            .expect("query allocation through the same sparse fixture handle");
        }
        assert_eq!(standard.EndOfFile, i64::try_from(length).unwrap());
        assert!(
            standard.AllocationSize >= 0 && standard.AllocationSize < standard.EndOfFile,
            "large history fixture must allocate less than its logical EOF"
        );
    }
    #[cfg(not(windows))]
    file.set_len(length).unwrap();
    assert_eq!(file.metadata().unwrap().len(), length);
}

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
    let project_paths = vec![temp.path().to_owned()];
    let catalog = Catalog {
        cli: CliKind::Codex,
        root: &root,
        projects: &granted(&root, &project_paths),
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
        extend_sparse_fixture(&file, length);
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(b"\n").unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let started = std::time::Instant::now();
    let items = read(
        &Catalog {cli: CliKind::Codex, root: &root, projects: &granted(&root, &[temp.path().to_owned()]), user_config: None, check: &|| Ok(()),},
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

// codex 同 thread 的不同 rollout 即使 cwd 不同也不再拒绝整个来源；无归属
// （外国 cwd）的记录不进入代表竞争，已归属的代表保留并可列出。
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
        extend_sparse_fixture(&file, 3 * 1024 * 1024);
    }
    let root = Root::open(temp.path()).unwrap();
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .expect("同 thread 的无归属 rollout 不得让历史读取失败");
    assert_eq!(items.len(), 1);
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(value[0]["cwd"], temp.path().to_str().unwrap());
}

// codex 0.147+ 的 fork/compact 会为同一 thread 追加 rollout 文件，session id
// 继承父 thread（文件名形如 <父id>_<新id>.jsonl）。同 id 多文件是合法 thread
// 模型，最新文件代表 thread 当前状态，与 codex resume 语义一致；不得让整个
// 来源失败，否则任一 fork 都会让全部 codex 历史无法加载。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_SameThreadRolloutsKeepLatest_040() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let cwd = temp.path().to_str().unwrap().to_owned();
    for (name, message, timestamp) in [
        (
            "rollout-2026-10-03T20-14-17-thread.jsonl",
            "before fork",
            "2026-10-03T20:14:17Z",
        ),
        (
            "rollout-2026-10-03T23-35-11-thread_fork.jsonl",
            "after fork",
            "2026-10-03T23:35:11Z",
        ),
    ] {
        std::fs::write(
            directory.join(name),
            format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":"thread","cwd":cwd}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":message},"timestamp":timestamp})
            ),
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .expect("同 thread 的多个 rollout 不得让历史读取失败");
    assert_eq!(items.len(), 1, "同 thread 只保留最新代表");
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(value[0]["title"], "after fork");
    assert_eq!(value[0]["updatedAt"], "2026-10-03T23:35:11Z");
}

// 同 thread 的最新 rollout 归属其自身 cwd 对应的项目：fork 后 thread 在另一
// 项目继续时，条目随最新代表移动，不残留旧项目。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_SameThreadCrossProjectTakesLatest_042() {
    let temp = tempfile::tempdir().unwrap();
    let project_x = temp.path().join("x");
    let project_y = temp.path().join("y");
    std::fs::create_dir_all(&project_x).unwrap();
    std::fs::create_dir_all(&project_y).unwrap();
    let directory = temp.path().join("sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    for (name, cwd, message, timestamp) in [
        (
            "rollout-2026-10-03T20-14-17-thread.jsonl",
            project_x.to_str().unwrap().to_owned(),
            "in project x",
            "2026-10-03T20:14:17Z",
        ),
        (
            "rollout-2026-10-03T23-35-11-thread_fork.jsonl",
            project_y.to_str().unwrap().to_owned(),
            "in project y",
            "2026-10-03T23:35:11Z",
        ),
    ] {
        std::fs::write(
            directory.join(name),
            format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":"thread","cwd":cwd}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":message},"timestamp":timestamp})
            ),
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let projects = [
        ProjectRef {
            id: "project-x",
            root: &root,
            paths: &[project_x.clone()],
        },
        ProjectRef {
            id: "project-y",
            root: &root,
            paths: &[project_y.clone()],
        },
    ];
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &projects,
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(value[0]["projectId"], "project-y");
    assert_eq!(value[0]["title"], "in project y");
}

// codex 自身的归档机制把会话移进 archived_sessions/（与 sessions/ 零重叠）；
// 代表 rollout 的容器位置必须作为 archived 状态输出，供前端隐藏普通树中的行。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ArchivedFlagFromSourceDir_043() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().to_str().unwrap().to_owned();
    for (path, id) in [
        ("archived_sessions/rollout-2026-05-05T19-04-12-archived.jsonl", "archived-id"),
        ("sessions/2026/10/03/rollout-2026-10-03T20-14-17-live.jsonl", "live-id"),
    ] {
        let full = temp.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(
            &full,
            format!(
                "{}\n",
                json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}})
            ),
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .unwrap();
    assert_eq!(items.len(), 2);
    let value = serde_json::to_value(&items).unwrap();
    let by_id: std::collections::BTreeMap<&str, bool> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["nativeSessionId"].as_str().unwrap(), item["archived"].as_bool().unwrap()))
        .collect();
    assert_eq!(by_id["archived-id"], true);
    assert_eq!(by_id["live-id"], false);
}

// 阶段一按 8KiB 块读取首条记录后不得截断缓冲：游标已越过块边界，若把缓冲
// 截回首条记录末端，阶段二会从游标处继续，跨块记录的开头被永久丢弃——轻则
// 标题落到后见记录，重则把多字节字符拦腰切断使整个头窗口变成无效 UTF-8，
// 生产实测曾因此丢弃 200 个 rollout。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_FirstRecordChunkBoundaryKeepsFollowingRecords_044() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions/2026/10/04");
    std::fs::create_dir_all(&directory).unwrap();
    let cwd = temp.path().to_str().unwrap().to_owned();
    // 首条记录约 10KiB（跨过一个 8KiB 块边界）；随后的首条用户记录被该边界
    // 拦腰切断（多字节文本使断缝几乎必然落在字符中间）。
    let input = format!(
        "{}\n{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"thread-a","cwd":cwd,"base_instructions":"i".repeat(10 * 1024)}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"好".repeat(8 * 1024)}]}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"后见标题"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"tail message"},"timestamp":"2026-10-04T02:00:00Z"}),
    );
    std::fs::write(directory.join("rollout-2026-10-04T01-13-08-gap.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .expect("跨块记录不得使整个来源失败");
    assert_eq!(items.len(), 1, "行不得因读取缝隙被丢弃");
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(value[0]["title"], format!("{}…", "好".repeat(96)));
    // 身份只取首条 session_meta；compact/fork 续写内嵌的其他 thread meta 跳过。
    assert_eq!(value[0]["nativeSessionId"], "thread-a");
    assert_eq!(value[0]["updatedAt"], "2026-10-04T02:00:00Z");
}

// 消息读取同样按代表制：同 thread 只读最新 rollout，不同文件的消息不得交错。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_SameThreadMessagesReadRepresentative_041() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("sessions/2026/10/03");
    std::fs::create_dir_all(&directory).unwrap();
    let cwd = temp.path().to_str().unwrap().to_owned();
    for (name, message, timestamp) in [
        (
            "rollout-2026-10-03T20-14-17-thread.jsonl",
            "before fork",
            "2026-10-03T20:14:17Z",
        ),
        (
            "rollout-2026-10-03T23-35-11-thread_fork.jsonl",
            "after fork",
            "2026-10-03T23:35:11Z",
        ),
    ] {
        std::fs::write(
            directory.join(name),
            format!(
                "{}\n{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":"thread","cwd":cwd}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":message},"timestamp":timestamp}),
                json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":format!("{message} answer")}]}})
            ),
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::Messages,
            query: None,
            session_id: Some("thread"),
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
    .expect("同 thread 的多个 rollout 不得让消息读取失败");
    let value = serde_json::to_value(&items).unwrap();
    let bodies: Vec<&str> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        bodies,
        vec!["after fork", "after fork answer"],
        "只读代表 rollout，不含 fork 前消息"
    );
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
    extend_sparse_fixture(&file, 32 * 1024 * 1024);
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &[],
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

// 1677 个 12KiB 身份头（每条记录跨四个 4KiB 块，约 26MiB 计费字节）在 16MiB 显式
// 总量下仍然失败关闭：元数据采样不能扩大或吞掉全局上限。默认上限的量级由
// LimitsDefaultValues 单独钉住，避免该用例为了踩满 64MiB 而在慢机器上先撞 5 秒协作截止。
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
        extend_sparse_fixture(&file, 32 * 1024 * 1024);
    }
    let root = Root::open(temp.path()).unwrap();
    let result = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &[],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits {
            file_bytes: 2 * 1024 * 1024,
            total_bytes: 16 * 1024 * 1024,
            entries: 4096,
        }),
    );
    assert_eq!(result.err(), Some("SOURCE_TOO_LARGE"));
}

// 默认聚合上限的量级单独钉住：真实 Codex 根（约 2130 个 rollout，含
// archived_sessions）4KiB 身份采样实测约 47MiB；扩展到 192KiB 头窗口取首条
// 用户消息后实测约 157MiB，256MiB 留出余量。稳定根经元数据缓存命中后不再重读。
#[test]
#[allow(non_snake_case)]
fn HistorySource_DefaultLimitsCoverRealCodexRoots_036() {
    let limits = super::super::super::scoped_fs::Limits::default();
    assert_eq!(limits.file_bytes, 2 * 1024 * 1024);
    assert_eq!(limits.total_bytes, 512 * 1024 * 1024);
    assert_eq!(limits.entries, 16384);
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
            projects: &granted(&root, &[temp.path().to_owned()]),
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

// 头窗口覆盖到首条用户消息时，标题与尾部时间都被观察：codex 列表因此不再只有
// “Untitled”，也不再缺少原生最近时间。窗口未覆盖时仍以未知标题列出（见下方
// 超长身份头用例）。
#[test]
#[allow(non_snake_case)]
fn HistorySource_TitleFromUserMessageWindow_011() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    let title = "seen-title";
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":"unseen-title","cwd":temp.path().to_str().unwrap(),"base_instructions":"i".repeat(1024)}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":title}}),
        json!({"type":"response_item","timestamp":"2026-10-10T03:00:00Z","payload":{"type":"message","role":"assistant","content":"answer"}}),
    );
    std::fs::write(temp.path().join("sessions/title.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[temp.path().to_owned()]),
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
        title: observed_title,
        truncated,
        updated_at,
        ..
    } = &items[0]
    else {
        panic!("history must return only session metadata")
    };
    assert_eq!(native_session_id, "unseen-title");
    assert_eq!(observed_title, title);
    // 完整读取（文件很小）时标记为完整，且时间来自尾部真实时间戳。
    assert!(!truncated);
    assert_eq!(updated_at.as_deref(), Some("2026-10-10T03:00:00Z"));
    assert!(!budget.history_metadata_incomplete());
}

// codex 的自动评审/安全扫描脚手架同样以 role=user 注入，且必须继续向后扫描真实
// 首条消息；没有任何真实用户输入时会话退回 Untitled，而不是把包装文字当标题。
#[test]
#[allow(non_snake_case)]
fn HistorySource_InjectedScaffoldingIsNotATitle_045() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    let cwd = temp.path().to_str().unwrap().to_owned();
    let header = json!({"type":"session_meta","payload":{"id":"auto-review","cwd":cwd}});
    let wrappers = [
        "The following is the Codex agent history whose request action you are assessing.",
        "Review the code changes against the base branch 'main'.",
        "## Code review guidelines:\n# Review Guidelines",
        "IMPORTANT: Do NOT read or execute any files under ~/.codex/",
        "# Files mentioned by the user:",
        "Run one Standard security scan using this exact configuration:",
    ];

    // 只有包装：不得把包装文字当标题。
    let body: String = wrappers
        .iter()
        .map(|text| {
            format!(
                "{}\n",
                json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}})
            )
        })
        .collect();
    std::fs::write(
        temp.path().join("sessions/auto.jsonl"),
        format!("{header}\n{body}"),
    )
    .unwrap();
    let items = read_codex_history_fixture(temp.path()).unwrap();
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(value[0]["title"], "Untitled");
    assert_eq!(value[0]["titleUnknown"], true);

    // 包装之后出现真实用户消息时，标题取该真实消息。
    std::fs::write(
        temp.path().join("sessions/auto.jsonl"),
        format!(
            "{header}\n{body}{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"修复 cc-box 的历史列表"}]}})
        ),
    )
    .unwrap();
    let items = read_codex_history_fixture(temp.path()).unwrap();
    let value = serde_json::to_value(&items).unwrap();
    assert_eq!(value[0]["title"], "修复 cc-box 的历史列表");
}

fn read_codex_history_fixture(root_path: &Path) -> ReadResult<Vec<ResourceItem>> {
    let root = Root::open(root_path).unwrap();
    read(
        &Catalog {
            cli: CliKind::Codex,
            root: &root,
            projects: &granted(&root, &[root_path.to_owned()]),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut Budget::new(super::super::super::scoped_fs::Limits::default()),
    )
}

fn read_claude_history_fixture(
    root: &Root,
    paths: &[PathBuf],
    budget: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    read(
        &Catalog {
            cli: CliKind::Claude,
            root,
            projects: &granted(root, paths),
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        budget,
    )
}

// Anthropic SDK 0.2.165 / bundled CLI 2.1.296 supports a leading permission-mode
// record without cwd. A following large first user record must remain discoverable.
// https://github.com/anthropics/claude-agent-sdk-python/blob/b6e9d12fe1cc98dde988ab7b7713c1feeee50c6c/tests/test_sessions.py#L1399-L1426
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_LeadingMetadataLargeUser_012() {
    for metadata in [
        json!({"type":"permission-mode","permissionMode":"acceptEdits"}),
        json!({"type":"file-history-snapshot","snapshot":{}}),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let input = format!(
            "{metadata}\n{}\n{}\n",
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"synthetic task ".repeat(8000)}}),
            json!({"type":"assistant","timestamp":"2026-10-10T04:00:00Z","message":{"content":"a".repeat(96 * 1024)}}),
        );
        let complete = parse("fixture.jsonl", CliKind::Claude, &input).unwrap();
        assert_eq!(complete.cwd.as_deref(), temp.path().to_str());
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        assert_eq!(
            items.len(),
            1,
            "a complete supported cwd record follows metadata"
        );
        let ResourceItem::Session {
            title,
            cwd,
            truncated,
            updated_at,
            ..
        } = &items[0]
        else {
            panic!("history must return session metadata")
        };
        assert!(title.starts_with("synthetic task "));
        assert!(title.len() <= 512);
        assert_eq!(cwd.as_deref(), temp.path().to_str());
        assert!(*truncated);
        assert!(updated_at.is_none(), "unread tail time must stay unknown");
        assert!(budget.history_read_failures().is_empty());
    }
}

// The first supported cwd record itself may exceed many chunks; retain the
// observed title and never parse an incomplete UTF-8 or JSON record as a whole.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_LongFirstRecord_013() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let input = format!(
        "{}\n{}\n",
        json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"中".repeat(80 * 1024)}}),
        json!({"type":"assistant","message":{"content":"a".repeat(96 * 1024)}}),
    );
    std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
    assert_eq!(items.len(), 1);
    let ResourceItem::Session {
        title,
        truncated,
        updated_at,
        ..
    } = &items[0]
    else {
        panic!("history must return session metadata")
    };
    assert!(!title.is_empty());
    assert!(title.len() <= 512);
    assert!(title.chars().all(|ch| ch == '中'));
    assert!(*truncated);
    assert!(updated_at.is_none());
}

// 1251 complete synthetic Claude transcripts with 96KiB assistant bodies must
// fit the 64MiB default aggregate observation cap without reading those bodies.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_Load1251_014() {
    let temp = tempfile::tempdir().unwrap();
    let assistant = json!({"type":"assistant","timestamp":"2026-10-10T05:00:00Z","message":{"content":"a".repeat(96 * 1024)}}).to_string();
    for index in 0..1251 {
        let directory = temp.path().join(format!("projects/p{:03}", index % 159));
        std::fs::create_dir_all(&directory).unwrap();
        let input = format!(
            "{}\n{assistant}\n",
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"synthetic task"}}),
        );
        std::fs::write(directory.join(format!("session-{index:04}.jsonl")), input).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget)
        .expect("1251 short cwd observations must fit the unchanged byte/entry caps");
    assert_eq!(items.len(), 1251);
    assert!(budget.history_metadata_incomplete());
    assert!(budget.history_read_failures().is_empty());
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
            panic!("history must return session metadata")
        };
        assert!(ids.insert(native_session_id));
        assert_eq!(title, "synthetic task");
        assert!(truncated);
        assert!(updated_at.is_none());
    }
}

// A sampled metadata record without cwd is insufficient for project identity;
// exhausting the per-file cap must be diagnosed instead of silently filtering it.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_UnobservedCwdCap_015() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let input = format!(
        "{}\n{}\n",
        json!({"type":"file-history-snapshot","snapshot":{}}),
        json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"u".repeat(3 * 1024 * 1024)}}),
    );
    std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
    assert!(items.is_empty());
    assert_eq!(budget.history_read_failures(), vec!["SOURCE_TOO_LARGE"]);
    assert!(budget.history_metadata_incomplete());
}

// Validate all complete records in each observed chunk, including records after
// a positive cwd and records before a later cwd. Arbitrary metadata cannot prove cwd.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_ValidateObservedRecords_016() {
    for (extra, code) in [
        ("{broken}", "SOURCE_INVALID"),
        (
            r#"{"type":"future","value":"bad\u0000text"}"#,
            "SOURCE_INVALID_TEXT",
        ),
    ] {
        for after in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
            let user = json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"task"}});
            let input = if after {
                format!("{user}\n{extra}\n")
            } else {
                format!("{extra}\n{user}\n")
            };
            std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
            let root = Root::open(temp.path()).unwrap();
            let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
            let items =
                read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
            assert!(items.is_empty());
            assert_eq!(budget.history_read_failures(), vec![code]);
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_UnknownAndMetadataOnlySemantics_017() {
    for (input, expected_failure) in [
        ("", Some("SOURCE_UNSUPPORTED")),
        (
            "{\"type\":\"permission-mode\",\"permissionMode\":\"acceptEdits\"}\n",
            Some("SOURCE_UNSUPPORTED"),
        ),
        (
            "{\"type\":\"future\",\"cwd\":\"/synthetic/project\"}\n",
            Some("SOURCE_UNSUPPORTED"),
        ),
        (
            "{\"type\":\"custom-title\",\"customTitle\":\"known\"}\n",
            None,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        assert!(items.is_empty());
        assert_eq!(
            budget.history_read_failures(),
            expected_failure.into_iter().collect::<Vec<_>>()
        );
    }
}

// Adaptive chunks must debit the original shared aggregate cap even when each
// file reaches a supported cwd before its unread assistant body.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_KeepAggregateCap_018() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    for index in 0..3 {
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"type":"permission-mode","permissionMode":"acceptEdits"}),
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"u".repeat(12 * 1024)}}),
            json!({"type":"assistant","message":{"content":"a".repeat(24 * 1024)}}),
        );
        std::fs::write(temp.path().join(format!("projects/p/{index}.jsonl")), input).unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits {
        file_bytes: 16 * 1024,
        total_bytes: 32 * 1024,
        entries: 4096,
    });
    assert_eq!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).err(),
        Some("SOURCE_TOO_LARGE")
    );
}

// Claude filenames remain identities across the whole held source. Project
// filtering cannot hide duplicates discovered through different cwd records.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_DuplicateBeforeProjectFilter_019() {
    let temp = tempfile::tempdir().unwrap();
    for (directory, cwd) in [
        ("a", temp.path().to_str().unwrap()),
        ("b", "/synthetic/foreign"),
    ] {
        std::fs::create_dir_all(temp.path().join(format!("projects/{directory}"))).unwrap();
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"type":"permission-mode","permissionMode":"acceptEdits"}),
            json!({"type":"user","cwd":cwd,"message":{"content":"task"}}),
            json!({"type":"assistant","message":{"content":"a".repeat(96 * 1024)}}),
        );
        std::fs::write(
            temp.path()
                .join(format!("projects/{directory}/same-id.jsonl")),
            input,
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    assert_eq!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).err(),
        Some("SOURCE_AMBIGUOUS")
    );
}

// Preserve titles and native time from a complete observed file. A partial file
// can retain observed explicit titles but never promote its observed time to latest.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_ObservedTitleAndTime_020() {
    for partial in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let mut input = format!(
            "{}\n{}\n{}\n{}\n",
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"timestamp":"2026-10-10T06:00:00Z","message":{"content":"original task"}}),
            json!({"type":"custom-title","customTitle":"old title"}),
            json!({"type":"custom-title","customTitle":"observed latest title"}),
            json!({"type":"assistant","timestamp":"2026-10-10T07:00:00Z","message":{"content":"answer"}}),
        );
        if partial {
            input.push_str(&format!(
                "{}\n",
                json!({"type":"assistant","timestamp":"2026-10-10T08:00:00Z","message":{"content":"a".repeat(96 * 1024)}}),
            ));
        }
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        let ResourceItem::Session {
            title,
            truncated,
            updated_at,
            ..
        } = &items[0]
        else {
            panic!("history must return session metadata")
        };
        assert_eq!(title, "observed latest title");
        assert_eq!(*truncated, partial);
        assert_eq!(
            updated_at.as_deref(),
            if partial {
                None
            } else {
                Some("2026-10-10T07:00:00Z")
            }
        );
    }
}

// Arbitrary metadata with a cwd cannot terminate sampling before the actual
// supported record. It does not establish an authorized project directory.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_UnknownCwdDoesNotEndObservation_021() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"type":"future","cwd":"/synthetic/foreign"}),
        json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"task ".repeat(20 * 1024)}}),
        json!({"type":"assistant","message":{"content":"a".repeat(96 * 1024)}}),
    );
    std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    assert_eq!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).err(),
        Some("SOURCE_AMBIGUOUS"),
        "observed conflicting cwd records keep their existing fail-closed semantics"
    );
}

#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_RejectMainTranscriptLink_022() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("source.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"cwd\":\"/synthetic/project\"}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&path, temp.path().join("projects/p/fixture.jsonl")).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    assert_eq!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).err(),
        Some("SOURCE_NOT_REGULAR")
    );
}

// A complete recognized metadata file without cwd is globally observable, but
// cannot prove absence under a particular registered project. No directory-name inference.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_UnassociatedMetadataIsPartial_023() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    std::fs::write(
        temp.path().join("projects/p/fixture.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"known title\"}\n",
    )
    .unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let scoped =
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
    assert!(scoped.is_empty());
    assert!(
        budget.history_metadata_incomplete(),
        "missing association cannot establish complete project absence"
    );
    assert!(budget.history_read_failures().is_empty());

    let mut global_budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let global = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            projects: &[],
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind: ResourceKind::History,
            query: None,
            session_id: None,
        },
        &mut global_budget,
    )
    .unwrap();
    assert_eq!(global.len(), 1);
    let ResourceItem::Session { title, cwd, .. } = &global[0] else {
        panic!("history must return session metadata")
    };
    assert_eq!(title, "known title");
    assert!(cwd.is_none());
    assert!(!global_budget.history_metadata_incomplete());

    std::fs::write(
        temp.path().join("projects/p/fixture.jsonl"),
        "{\"type\":\"user\",\"cwd\":\"/synthetic/foreign\",\"message\":{\"content\":\"foreign\"}}\n",
    )
    .unwrap();
    let mut foreign_budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    assert!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut foreign_budget)
            .unwrap()
            .is_empty()
    );
    assert!(!foreign_budget.history_metadata_incomplete());
    assert!(foreign_budget.history_read_failures().is_empty());
}

// Official SDK 0.2.165 prefers customTitle, then aiTitle, then a user prompt.
// https://github.com/anthropics/claude-agent-sdk-python/blob/b6e9d12fe1cc98dde988ab7b7713c1feeee50c6c/src/claude_agent_sdk/_internal/sessions.py#L441-L458
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_AiTitleSchemaAndPrecedence_024() {
    let user =
        json!({"type":"user","cwd":"/synthetic/project","message":{"content":"first prompt"}});
    let ai = json!({"type":"ai-title","aiTitle":"generated title","sessionId":"fixture"});
    assert_eq!(
        parse("fixture.jsonl", CliKind::Claude, &format!("{user}\n{ai}\n"))
            .unwrap()
            .title,
        "generated title"
    );
    for input in [
        format!("{user}\n{{\"type\":\"custom-title\",\"customTitle\":\"manual\"}}\n{ai}\n"),
        format!("{user}\n{ai}\n{{\"type\":\"custom-title\",\"customTitle\":\"manual\"}}\n"),
    ] {
        assert_eq!(
            parse("fixture.jsonl", CliKind::Claude, &input)
                .unwrap()
                .title,
            "manual"
        );
        assert_eq!(
            parse_metadata("fixture.jsonl", CliKind::Claude, &input)
                .unwrap()
                .title,
            "manual"
        );
    }
    for (value, code) in [
        (
            json!({"type":"ai-title","aiTitle":"generated","sessionId":"other"}),
            "SOURCE_AMBIGUOUS",
        ),
        (
            json!({"type":"ai-title","aiTitle":"generated"}),
            "SOURCE_INVALID",
        ),
        (
            json!({"type":"ai-title","aiTitle":"","sessionId":"fixture"}),
            "SOURCE_INVALID",
        ),
        (
            json!({"type":"ai-title","aiTitle":42,"sessionId":"fixture"}),
            "SOURCE_INVALID",
        ),
    ] {
        let input = format!("{user}\n{value}\n");
        assert_eq!(
            parse("fixture.jsonl", CliKind::Claude, &input).err(),
            Some(code)
        );
        assert_eq!(
            parse_metadata("fixture.jsonl", CliKind::Claude, &input).err(),
            Some(code)
        );
    }
}

fn claude_tail_input(cwd: &Path, tail: &str) -> String {
    format!(
        "{}\n{}\n{tail}",
        json!({"type":"user","cwd":cwd.to_str().unwrap(),"sessionId":"fixture","message":{"content":"first prompt"}}),
        json!({"type":"assistant","timestamp":"2026-10-10T09:00:00Z","message":{"content":"a".repeat(150 * 1024)}}),
    )
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_BoundedTailTitle_025() {
    for (tail, expected) in [
        ("{\"type\":\"ai-title\",\"aiTitle\":\"generated title\",\"sessionId\":\"fixture\"}\n", "generated title"),
        ("{\"type\":\"custom-title\",\"customTitle\":\"manual\",\"sessionId\":\"fixture\"}\n{\"type\":\"ai-title\",\"aiTitle\":\"generated title\",\"sessionId\":\"fixture\"}\n", "manual"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), claude_tail_input(temp.path(), tail)).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items = read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        let ResourceItem::Session { title, updated_at, truncated, .. } = &items[0] else { panic!("session metadata") };
        assert_eq!(title, expected);
        assert!(updated_at.is_none(), "tail display enrichment is not complete activity evidence");
        assert!(*truncated);
        assert!(budget.history_metadata_incomplete());
    }
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_TailFailureRetainsProvenRow_026() {
    for (tail, code) in [
        ("{broken}\n", "SOURCE_INVALID"),
        (
            "{\"type\":\"future\",\"value\":\"bad\\u0000text\"}\n",
            "SOURCE_INVALID_TEXT",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        std::fs::write(
            temp.path().join("projects/p/fixture.jsonl"),
            claude_tail_input(temp.path(), tail),
        )
        .unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        assert_eq!(items.len(), 1);
        let ResourceItem::Session { title, .. } = &items[0] else {
            panic!("session metadata")
        };
        assert_eq!(title, "first prompt");
        assert_eq!(budget.history_read_failures(), vec![code]);
    }
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_TailIdMismatchFailsClosed_027() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    std::fs::write(
        temp.path().join("projects/p/fixture.jsonl"),
        claude_tail_input(
            temp.path(),
            "{\"type\":\"ai-title\",\"aiTitle\":\"wrong session\",\"sessionId\":\"other\"}\n",
        ),
    )
    .unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    assert_eq!(
        read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).err(),
        Some("SOURCE_AMBIGUOUS")
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_UnreadMiddleAndLiteralUntitled_028() {
    for (prompt, expect_unknown) in [("", true), ("Untitled", false)] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let input = format!(
            "{}\n{}\n{}\n{}\n",
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":prompt}}),
            json!({"type":"assistant","message":{"content":"a".repeat(150 * 1024)}}),
            json!({"type":"ai-title","aiTitle":"unread middle title","sessionId":"fixture"}),
            json!({"type":"assistant","message":{"content":"b".repeat(150 * 1024)}}),
        );
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        let row = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(row["title"], "Untitled");
        assert_eq!(
            row.get("titleUnknown").and_then(Value::as_bool),
            expect_unknown.then_some(true)
        );
    }
}

// Required headers finish before optional tails spend any aggregate bytes.
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_OptionalTitlesCannotStarveHeaders_029() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    for index in 0..8 {
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"type":"user","cwd":temp.path().to_str().unwrap(),"message":{"content":"task"}}),
            json!({"type":"assistant","message":{"content":"a".repeat(96 * 1024)}}),
            json!({"type":"ai-title","aiTitle":"optional","sessionId":format!("{index:02}")})
        );
        std::fs::write(
            temp.path().join(format!("projects/p/{index:02}.jsonl")),
            input,
        )
        .unwrap();
    }
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits {
        file_bytes: 2 * 1024 * 1024,
        total_bytes: 8 * 4096 + 8192,
        entries: 4096,
    });
    let items = read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
    assert_eq!(items.len(), 8);
    let enriched = items
        .iter()
        .filter(|item| matches!(item, ResourceItem::Session { title, .. } if title == "optional"))
        .count();
    assert_eq!(
        enriched, 1,
        "only one optional header+tail fits after all eight required headers"
    );
    assert!(budget.history_read_failures().is_empty());
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_ObservedSessionIdsAgree_030() {
    for record in [
        json!({"type":"user","sessionId":"other","cwd":"/synthetic/project","message":{"content":"task"}}),
        json!({"type":"assistant","sessionId":"other","cwd":"/synthetic/project"}),
        json!({"type":"custom-title","sessionId":"other","customTitle":"manual"}),
    ] {
        let input = format!("{record}\n");
        assert_eq!(
            parse("fixture.jsonl", CliKind::Claude, &input).err(),
            Some("SOURCE_AMBIGUOUS")
        );
        assert_eq!(
            parse_metadata("fixture.jsonl", CliKind::Claude, &input).err(),
            Some("SOURCE_AMBIGUOUS")
        );
    }
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_OptionalHeaderMustRemainProven_031() {
    for replacement in [
        None,
        Some("{broken}\n"),
        Some("{\"type\":\"user\",\"cwd\":\"/synthetic/changed\"}\n"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let path = temp.path().join("projects/p/fixture.jsonl");
        let input = claude_tail_input(
            temp.path(),
            "{\"type\":\"ai-title\",\"aiTitle\":\"generated\",\"sessionId\":\"fixture\"}\n",
        );
        std::fs::write(&path, &input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let end = input[..4096].rfind('\n').unwrap();
        let mut transcript =
            parse_metadata("projects/p/fixture.jsonl", CliKind::Claude, &input[..=end]).unwrap();
        transcript.observation_bytes = 4096;
        match replacement {
            None => std::fs::remove_file(&path).unwrap(),
            Some(header) => {
                std::fs::write(&path, format!("{header}{}", " ".repeat(input.len()))).unwrap()
            }
        }
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let project_paths = vec![temp.path().to_owned()];
        let catalog = Catalog {
            cli: CliKind::Claude,
            root: &root,
            projects: &granted(&root, &project_paths),
            user_config: None,
            check: &|| Ok(()),
        };
        assert_eq!(
            enrich_claude_title(&catalog, &transcript, &mut budget).err(),
            Some("SOURCE_CHANGED")
        );
    }
}

#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_OptionalLinkFailsClosed_032() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let input = claude_tail_input(temp.path(), "");
    let path = temp.path().join("projects/p/fixture.jsonl");
    std::fs::write(&path, &input).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let end = input[..4096].rfind('\n').unwrap();
    let mut transcript =
        parse_metadata("projects/p/fixture.jsonl", CliKind::Claude, &input[..=end]).unwrap();
    transcript.observation_bytes = 4096;
    std::fs::remove_file(&path).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.jsonl"), &input).unwrap();
    std::os::unix::fs::symlink(outside.path().join("outside.jsonl"), &path).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let project_paths = vec![temp.path().to_owned()];
    let catalog = Catalog {
        cli: CliKind::Claude,
        root: &root,
        projects: &granted(&root, &project_paths),
        user_config: None,
        check: &|| Ok(()),
    };
    assert_eq!(
        enrich_claude_title(&catalog, &transcript, &mut budget).err(),
        Some("SOURCE_NOT_REGULAR")
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_OptionalTitleBudgetAndPriority_033() {
    for (custom, file_bytes, expected) in [
        (true, 2 * 1024 * 1024, "manual"),
        (false, 8192, "first prompt"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let mut input = claude_tail_input(
            temp.path(),
            "{\"type\":\"ai-title\",\"aiTitle\":\"generated\",\"sessionId\":\"fixture\"}\n",
        );
        if custom {
            input.insert_str(0, "{\"type\":\"custom-title\",\"customTitle\":\"manual\",\"sessionId\":\"fixture\"}\n");
        }
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits {
            file_bytes,
            total_bytes: 16 * 1024 * 1024,
            entries: 4096,
        });
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        let ResourceItem::Session { title, .. } = &items[0] else {
            panic!("session metadata")
        };
        assert_eq!(title, expected);
        assert!(budget.history_read_failures().is_empty());
    }
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_GlobalHistoryDoesNotSampleTail_034() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    std::fs::write(
        temp.path().join("projects/p/fixture.jsonl"),
        claude_tail_input(temp.path(), "{broken}\n"),
    )
    .unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
    let items = read(
        &Catalog {
            cli: CliKind::Claude,
            root: &root,
            projects: &[],
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
    assert!(budget.history_metadata_incomplete());
    assert!(
        budget.history_read_failures().is_empty(),
        "optional tail requires positive requested-project association"
    );
}

#[test]
#[allow(non_snake_case)]
fn HistoryClaude_TitleProvenanceWire_035() {
    for (record, expected) in [
        (
            json!({"type":"user","message":{"content":"task"}}),
            Some("prompt"),
        ),
        (
            json!({"type":"ai-title","aiTitle":"generated","sessionId":"fixture"}),
            Some("ai"),
        ),
        (
            json!({"type":"custom-title","customTitle":"manual"}),
            Some("custom"),
        ),
        (json!({"type":"file-history-snapshot"}), None),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        let input = format!(
            "{}\n{record}\n",
            json!({"type":"system","cwd":temp.path().to_str().unwrap()})
        );
        std::fs::write(temp.path().join("projects/p/fixture.jsonl"), input).unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        let row = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(row.get("titleSource").and_then(Value::as_str), expected);
        assert!(row.get("metadataIncomplete").is_none());
        assert_eq!(
            row.get("titleUnknown").and_then(Value::as_bool),
            expected.is_none().then_some(true)
        );
    }
}

// 尾段记录携带的 cwd 与首段观察不同（会话进入子目录、或仅大小写/分隔符差异）不是
// 身份歧义：行归属由首段 cwd 决定，尾段只提取标题，其身份由 sessionId 一致性约束。
// 真机复现：46 个 Claude 转录中 7 个因此被误判 SOURCE_AMBIGUOUS，整源不可用。
#[test]
#[allow(non_snake_case)]
fn HistoryClaude_TailCwdVariationKeepsRow_037() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("web").to_str().unwrap().to_string();
    for tail in [
        format!(
            "{}\n{}\n",
            json!({"type":"user","cwd":nested,"sessionId":"fixture","message":{"content":"later turn"}}),
            json!({"type":"ai-title","aiTitle":"generated title","sessionId":"fixture"}),
        ),
        format!(
            "{}\n",
            json!({"type":"ai-title","aiTitle":"generated title","sessionId":"fixture","cwd":"/synthetic/unrelated"}),
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
        std::fs::write(
            temp.path().join("projects/p/fixture.jsonl"),
            claude_tail_input(temp.path(), &tail),
        )
        .unwrap();
        let root = Root::open(temp.path()).unwrap();
        let mut budget = Budget::new(super::super::super::scoped_fs::Limits::default());
        let items =
            read_claude_history_fixture(&root, &[temp.path().to_owned()], &mut budget).unwrap();
        assert_eq!(items.len(), 1, "cwd variation in the tail is not ambiguity");
        let ResourceItem::Session { title, cwd, .. } = &items[0] else {
            panic!("history must return session metadata")
        };
        assert_eq!(title, "generated title");
        assert_eq!(cwd.as_deref(), Some(temp.path().to_str().unwrap()));
    }
}
