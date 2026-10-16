use super::super::super::scoped_fs::Limits;
use super::*;
use serde_json::json;

fn rollout(cwd: &Path, id: &str, title: &str, timestamp: &str) -> String {
    format!(
        "{}\n{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"source":"vscode","thread_source":"user"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":title}]}}),
        json!({"type":"response_item","timestamp":timestamp,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"完成"}]}})
    )
}

fn write_zstd(root: &Path, relative: &str, input: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, zstd::stream::encode_all(input.as_bytes(), 1).unwrap()).unwrap();
}

fn observe(
    root: &Root,
    project: &Path,
    kind: ResourceKind,
    budget: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    let paths = [project.to_owned()];
    let projects = [ProjectRef {
        id: "p",
        root,
        paths: &paths,
    }];
    read(
        &Catalog {
            cli: CliKind::Codex,
            root,
            projects: &projects,
            user_config: None,
            check: &|| Ok(()),
        },
        &Options {
            kind,
            query: Some("连接"),
            session_id: Some("active"),
        },
        budget,
    )
}

// 压缩不是归档：容器决定归档状态，列表读取不解压落盘或修改源文件。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdLiveAndArchived_057() {
    let temp = tempfile::tempdir().unwrap();
    for (path, id) in [
        ("sessions/2026/10/16/active.jsonl.zst", "active"),
        ("archived_sessions/archived.jsonl.zst", "archived"),
    ] {
        write_zstd(
            temp.path(),
            path,
            &rollout(temp.path(), id, "连接主机", "2026-10-16T00:00:00Z"),
        );
    }
    let root = Root::open(temp.path()).unwrap();
    for _ in 0..2 {
        let mut budget = Budget::new(Limits::default());
        let items = observe(&root, temp.path(), ResourceKind::History, &mut budget).unwrap();
        let values = serde_json::to_value(items).unwrap();
        assert_eq!(values.as_array().unwrap().len(), 2);
        for item in values.as_array().unwrap() {
            assert_eq!(item["archived"], item["nativeSessionId"] == "archived");
            assert_eq!(item["title"], "连接主机");
            assert_eq!(item["projectId"], "p");
            assert_eq!(item["updatedAt"], "2026-10-16T00:00:00Z");
        }
        assert!(!budget.history_metadata_incomplete());
        assert!(budget.history_read_failures().is_empty());
    }
    assert!(!temp
        .path()
        .join("sessions/2026/10/16/active.jsonl")
        .exists());
}

// 同一 thread 的压缩旧转录、压缩续写与未压缩副本只能形成一个最新投影。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdRepresentatives_058() {
    let temp = tempfile::tempdir().unwrap();
    write_zstd(
        temp.path(),
        "sessions/old.jsonl.zst",
        &rollout(temp.path(), "active", "旧标题", "2026-10-15T00:00:00Z"),
    );
    let latest = rollout(temp.path(), "active", "新标题", "2026-10-16T00:00:00Z");
    write_zstd(temp.path(), "sessions/new.jsonl.zst", &latest);
    std::fs::write(temp.path().join("sessions/new.jsonl"), latest).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let items = observe(
        &root,
        temp.path(),
        ResourceKind::History,
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let values = serde_json::to_value(items).unwrap();
    assert_eq!(values.as_array().unwrap().len(), 1);
    assert_eq!(values[0]["title"], "新标题");
}

// 历史行对应的消息与搜索仍走同一受限来源，不能把压缩字节当 UTF-8。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdMessagesAndSearch_059() {
    let temp = tempfile::tempdir().unwrap();
    write_zstd(
        temp.path(),
        "sessions/a.jsonl.zst",
        &rollout(temp.path(), "active", "连接主机", "2026-10-16T00:00:00Z"),
    );
    let root = Root::open(temp.path()).unwrap();
    for kind in [ResourceKind::Messages, ResourceKind::Search] {
        let items = observe(
            &root,
            temp.path(),
            kind,
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        let values = serde_json::to_value(items).unwrap();
        assert_eq!(
            values.as_array().unwrap().len(),
            if kind == ResourceKind::Search { 1 } else { 2 }
        );
        assert_eq!(values[0]["text"], "连接主机");
    }
}

// 损坏与被截断的压缩文件按单条失败披露，不能清空其他可读历史。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdCorruptionIsIsolated_060() {
    let temp = tempfile::tempdir().unwrap();
    let input = rollout(temp.path(), "active", "连接主机", "2026-10-16T00:00:00Z");
    write_zstd(temp.path(), "sessions/good.jsonl.zst", &input);
    std::fs::write(
        temp.path().join("sessions/corrupt.jsonl.zst"),
        b"not zstandard",
    )
    .unwrap();
    let mut truncated = zstd::stream::encode_all(input.as_bytes(), 1).unwrap();
    truncated.pop();
    std::fs::write(temp.path().join("sessions/truncated.jsonl.zst"), truncated).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(Limits::default());
    let items = observe(&root, temp.path(), ResourceKind::History, &mut budget).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(budget.history_read_failures(), vec!["SOURCE_INVALID"]);
}

// 大型高压缩比会话只做有界头观察；后面的超长正文不影响列表身份与标题。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdLargeHistoryStaysVisible_061() {
    let temp = tempfile::tempdir().unwrap();
    let mut input = rollout(temp.path(), "active", "连接主机", "2026-10-16T00:00:00Z");
    input.push_str(&format!("{}\n", json!({"type":"response_item","payload":{"role":"assistant","content":"x".repeat(3*1024*1024)}})));
    write_zstd(temp.path(), "sessions/a.jsonl.zst", &input);
    let root = Root::open(temp.path()).unwrap();
    for _ in 0..2 {
        let mut budget = Budget::new(Limits::default());
        let items = observe(&root, temp.path(), ResourceKind::History, &mut budget).unwrap();
        let values = serde_json::to_value(items).unwrap();
        assert_eq!(values.as_array().unwrap().len(), 1);
        assert_eq!(values[0]["title"], "连接主机");
        assert_eq!(values[0]["metadataIncomplete"], true);
        assert!(budget.history_metadata_incomplete());
        assert!(budget.history_read_failures().is_empty());
    }
    assert_eq!(
        observe(
            &root,
            temp.path(),
            ResourceKind::Messages,
            &mut Budget::new(Limits::default())
        )
        .err(),
        Some("SOURCE_TOO_LARGE")
    );
}

// 允许连续 Zstd frame；完整观察不能只看到第一个 frame 就宣称文件结束。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdConcatenatedFrames_062() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sessions")).unwrap();
    let input = rollout(temp.path(), "active", "连接主机", "2026-10-16T00:00:00Z");
    let split = input.find('\n').unwrap() + 1;
    let mut encoded = zstd::stream::encode_all(&input.as_bytes()[..split], 1).unwrap();
    encoded.extend(zstd::stream::encode_all(&input.as_bytes()[split..], 1).unwrap());
    std::fs::write(temp.path().join("sessions/a.jsonl.zst"), encoded).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let mut budget = Budget::new(Limits::default());
    let values = serde_json::to_value(
        observe(&root, temp.path(), ResourceKind::History, &mut budget).unwrap(),
    )
    .unwrap();
    assert_eq!(values[0]["title"], "连接主机");
    assert_eq!(values[0]["updatedAt"], "2026-10-16T00:00:00Z");
    assert!(!budget.history_metadata_incomplete());
}

// 压缩格式不放宽项目归属、自动来源过滤或暖缓存的当前身份检查。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdSourceAndCacheIdentity_063() {
    let temp = tempfile::tempdir().unwrap();
    let active = rollout(temp.path(), "active", "连接主机", "2026-10-16T00:00:00Z");
    write_zstd(temp.path(), "sessions/a.jsonl.zst", &active);
    write_zstd(
        temp.path(),
        "sessions/foreign.jsonl.zst",
        &rollout(
            Path::new("/foreign"),
            "foreign",
            "其他项目",
            "2026-10-16T00:00:00Z",
        ),
    );
    write_zstd(
        temp.path(),
        "sessions/exec.jsonl.zst",
        &active.replace("\"source\":\"vscode\"", "\"source\":\"exec\""),
    );
    let root = Root::open(temp.path()).unwrap();
    let relative = Path::new("sessions/a.jsonl.zst");
    let (length, modified) = root.stat(relative).unwrap().unwrap();
    cache::with(|cache| {
        cache.record(
            cache::key(root.key(), "codex", relative),
            cache::Stamps::from_system(length, modified).unwrap(),
            cache::CachedMetadata {
                native_id: "stale".into(),
                cwd: Some(temp.path().to_string_lossy().into_owned()),
                title: "过期标题".into(),
                title_priority: Some(0),
                updated: None,
                metadata_incomplete: false,
            },
        )
    });
    let values = serde_json::to_value(
        observe(
            &root,
            temp.path(),
            ResourceKind::History,
            &mut Budget::new(Limits::default()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(values.as_array().unwrap().len(), 1);
    assert_eq!(values[0]["nativeSessionId"], "active");
    assert_eq!(values[0]["title"], "连接主机");
}

// session_meta 自身可超过普通头窗口；只要能在单文件上限内证明身份就应列出。
#[test]
#[allow(non_snake_case)]
fn HistoryCodex_ZstdLongIdentity_064() {
    let temp = tempfile::tempdir().unwrap();
    let header = json!({"type":"session_meta","payload":{"id":"active","cwd":temp.path(),"base_instructions":"中".repeat(80*1024),"source":"vscode"}});
    let input = format!(
        "{header}\n{}\n",
        json!({"type":"event_msg","payload":{"type":"user_message","message":"连接主机"}})
    );
    write_zstd(temp.path(), "sessions/a.jsonl.zst", &input);
    let root = Root::open(temp.path()).unwrap();
    let values = serde_json::to_value(
        observe(
            &root,
            temp.path(),
            ResourceKind::History,
            &mut Budget::new(Limits::default()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(values.as_array().unwrap().len(), 1);
    assert_eq!(values[0]["nativeSessionId"], "active");
}
