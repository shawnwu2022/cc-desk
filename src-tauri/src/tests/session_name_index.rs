use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::session_name_index::{
    merge_delta_in_memory, parse_session_name_full, resolve_session_name_at, DirectorySnapshot,
    FileStamp, FlushStage, IndexHealth, IndexLimits, IndexMutation, IndexSnapshot,
    PendingIndexFlush, RawIndexSnapshot, ResolutionKind, SessionNameEntry, SessionNameIndex,
    SessionNameIndexDelta, SessionNameIndexPaths, SessionNameIndexStore, SessionNameResolver,
    SESSION_NAME_INDEX_SCHEMA_VERSION, SESSION_NAME_PARSER_VERSION,
};
use crate::store::{acquire_lock, acquire_lock_with_label};

// 后部 custom-title 必须覆盖此前解析到的用户名称。
#[test]
fn Parser_LateTitle_001() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let content = concat!(
        "{\"type\":\"user\",\"message\":{\"content\":\"First user\"}}\n",
        "{\"type\":\"custom-title\",\"customTitle\":\"Late title\"}\n"
    );
    std::fs::write(&path, content).unwrap();

    let parsed = parse_session_name_full(&path).unwrap();

    assert_eq!(parsed.name, "Late title");
    assert_eq!(parsed.jsonl_bytes_read, content.len() as u64);
}

// 没有 custom-title 时只取第一条有效用户消息。
#[test]
fn Parser_FirstUser_002() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":\"First user\"}}\n",
            "{\"type\":\"user\",\"message\":{\"content\":\"Second user\"}}\n"
        ),
    )
    .unwrap();

    assert_eq!(parse_session_name_full(&path).unwrap().name, "First user");
}

// 无效 UTF-8 与无效 JSON 行不得阻止后续有效名称解析。
#[test]
fn Parser_InvalidLines_003() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut content = vec![0xff, 0xfe, b'\n'];
    content.extend_from_slice(b"not-json\n");
    content.extend_from_slice(b"{\"type\":\"user\",\"message\":{\"content\":\"Recovered\"}}\n");
    std::fs::write(&path, &content).unwrap();

    let parsed = parse_session_name_full(&path).unwrap();

    assert_eq!(parsed.name, "Recovered");
    assert_eq!(parsed.jsonl_bytes_read, content.len() as u64);
}

// 精确指纹命中必须返回缓存名称且读取零 JSONL bytes。
#[test]
fn Resolver_ExactHit_ReadsZero_004() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Disk name\"}}\n",
    )
    .unwrap();
    let stamp = FileStamp::read(&path).unwrap();
    let cached = SessionNameEntry {
        name: "Cached name".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 1_000,
    };

    let result = resolve_session_name_at(&path, stamp, Some(&cached), 2_000);

    assert_eq!(result.name, "Cached name");
    assert_eq!(result.kind, ResolutionKind::ExactHit);
    assert_eq!(result.jsonl_bytes_read, 0);
    assert!(result.replacement.is_none());
}

// 文件增长后不得沿用旧缓存，必须全量解析出新增标题。
#[test]
fn Resolver_Growth_Rebuilds_005() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Old name\"}}\n",
    )
    .unwrap();
    let old_stamp = FileStamp::read(&path).unwrap();
    let cached = SessionNameEntry {
        name: "Old name".to_string(),
        observed_length: old_stamp.observed_length,
        modified_secs: old_stamp.modified_secs,
        modified_nanos: old_stamp.modified_nanos,
        cached_at_ms: 1_000,
    };
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"New title\"}\n")
        .unwrap();
    let current = FileStamp::read(&path).unwrap();

    let result = resolve_session_name_at(&path, current, Some(&cached), 2_000);

    assert_eq!(result.name, "New title");
    assert_eq!(result.kind, ResolutionKind::FullRebuild);
    assert!(result.jsonl_bytes_read > 0);
    assert_eq!(
        result.replacement.unwrap().observed_length,
        current.observed_length
    );
}

// 长度相同但高精度 mtime 不同也必须全量重建。
#[test]
fn Resolver_SameLengthMtime_Rebuilds_006() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Fresh name\"}}\n",
    )
    .unwrap();
    let current = FileStamp::read(&path).unwrap();
    let cached_stamp = FileStamp {
        observed_length: current.observed_length,
        modified_secs: current.modified_secs.saturating_sub(1),
        modified_nanos: current.modified_nanos,
    };
    let cached = SessionNameEntry {
        name: "Stale name".to_string(),
        observed_length: cached_stamp.observed_length,
        modified_secs: cached_stamp.modified_secs,
        modified_nanos: cached_stamp.modified_nanos,
        cached_at_ms: 1_000,
    };

    let result = resolve_session_name_at(&path, current, Some(&cached), 2_000);

    assert_eq!(result.name, "Fresh name");
    assert_eq!(result.kind, ResolutionKind::FullRebuild);
    assert!(result.jsonl_bytes_read > 0);
    assert!(result.replacement.is_some());
}

// 初始指纹已过期时可以返回扫描结果，但不得缓存不稳定候选。
#[test]
fn Resolver_Unstable_NoReplacement_007() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Initial\"}}\n",
    )
    .unwrap();
    let stale_initial = FileStamp::read(&path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"Changed\"}\n")
        .unwrap();

    let result = resolve_session_name_at(&path, stale_initial, None, 2_000);

    assert_eq!(result.name, "Changed");
    assert_eq!(result.kind, ResolutionKind::FullRebuild);
    assert!(result.replacement.is_none());
}

// 文件已消失时保持现有 Unnamed fallback，且不得生成索引条目。
#[test]
fn Resolver_Missing_Unnamed_008() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.jsonl");
    let missing_stamp = FileStamp {
        observed_length: 0,
        modified_secs: 0,
        modified_nanos: 0,
    };

    let result = resolve_session_name_at(&path, missing_stamp, None, 2_000);

    assert_eq!(result.name, "Unnamed session");
    assert_eq!(result.kind, ResolutionKind::FullRebuild);
    assert_eq!(result.jsonl_bytes_read, 0);
    assert!(result.replacement.is_none());
}

// 索引文件不存在时返回当前版本空快照，不升级为业务错误。
#[test]
fn Index_Missing_Empty_010() {
    let dir = tempfile::tempdir().unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let snapshot = SessionNameIndexStore::new(
        SessionNameIndexPaths {
            data: dir.path().join("session-name-index.json"),
            lock: dir.path().join("session-name-index.json.lock"),
        },
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .read_snapshot();

    assert!(snapshot.index.projects.is_empty());
    assert_eq!(snapshot.raw, RawIndexSnapshot::Missing);
    assert!(!snapshot.parse_attempted);
    assert!(warnings.lock().unwrap().is_empty());
}

// schemaVersion 不匹配时整份索引失效并返回当前版本空索引。
#[test]
fn Index_SchemaVersion_Empty_011() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    std::fs::write(
        &paths.data,
        r#"{"schemaVersion":2,"parserVersion":1,"projects":{"p":{"s.jsonl":{"name":"stale","observedLength":1,"modifiedSecs":1,"modifiedNanos":1,"cachedAtMs":1}}}}"#,
    )
    .unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));

    let snapshot = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .read_snapshot();

    assert!(snapshot.index.projects.is_empty());
    assert!(snapshot.parse_attempted);
    assert!(warnings.lock().unwrap().is_empty());
}

// parserVersion 不匹配时禁止复用旧名称条目。
#[test]
fn Index_ParserVersion_Empty_012() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    std::fs::write(
        &paths.data,
        r#"{"schemaVersion":1,"parserVersion":0,"projects":{"p":{"s.jsonl":{"name":"stale","observedLength":1,"modifiedSecs":1,"modifiedNanos":1,"cachedAtMs":1}}}}"#,
    )
    .unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));

    let snapshot = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .read_snapshot();

    assert!(snapshot.index.projects.is_empty());
    assert!(snapshot.parse_attempted);
}

// 损坏 JSON 必须回退为空索引并记录一次可诊断 warning。
#[test]
fn Index_Corrupt_Fallback_013() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    std::fs::write(&paths.data, b"{not-json").unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));

    let snapshot = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .read_snapshot();

    assert!(snapshot.index.projects.is_empty());
    assert!(snapshot.parse_attempted);
    assert_eq!(warnings.lock().unwrap().len(), 1);
}

// 超过 hard limit 时不得把整份索引读入内存或尝试反序列化。
#[test]
fn Index_HardLimit_Fallback_014() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    std::fs::write(&paths.data, vec![b'x'; 1_025]).unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let limits = IndexLimits {
        target_bytes: 512,
        soft_bytes: 768,
        hard_bytes: 1_024,
    };

    let snapshot = SessionNameIndexStore::new(paths, limits, health, Duration::from_millis(100))
        .read_snapshot();

    assert!(snapshot.index.projects.is_empty());
    assert!(matches!(snapshot.raw, RawIndexSnapshot::Oversized(_)));
    assert!(!snapshot.parse_attempted);
}

// 同一损坏指纹在 30 秒内不得重复 parse 或 warning，指纹变化后立即重试。
#[test]
fn Index_CorruptWarn_Throttles_015() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    std::fs::write(&paths.data, b"{bad-one").unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    );

    let first = store.read_snapshot();
    let second = store.read_snapshot();
    std::fs::write(&paths.data, b"{bad-two-different").unwrap();
    let changed = store.read_snapshot();

    assert!(first.parse_attempted);
    assert!(!second.parse_attempted);
    assert!(changed.parse_attempted);
    assert_eq!(warnings.lock().unwrap().len(), 2);
}

// 写失败后 30 秒内禁止重复调度，窗口到期或成功后恢复。
#[test]
fn Index_WriteFail_BacksOff_016() {
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));

    assert!(health.allows_write());
    health.record_write_failure("disk full");
    health.record_write_failure("disk full again");
    assert!(!health.allows_write());
    assert_eq!(warnings.lock().unwrap().len(), 1);

    now_ms.store(31_000, Ordering::SeqCst);
    assert!(health.allows_write());
    health.record_write_success();
    assert!(health.allows_write());
}

// 通用锁 helper 必须使用调用方资源标签，同时旧 wrapper 仍可正常持锁。
#[test]
fn Index_LockLabel_017() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.lock");
    let first = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    let second = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    acquire_lock(&first, true, Duration::from_secs(1)).unwrap();

    let error = acquire_lock_with_label(
        &second,
        true,
        Duration::from_millis(40),
        "session-name-index.json",
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("session-name-index.json lock timeout"));
}

// 只有精确命中且无清理/压缩时不得生成写回。
#[test]
fn Resolver_ExactHit_NoDelta_020() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("-e-source-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let path = project_dir.join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Disk\"}}\n",
    )
    .unwrap();
    let stamp = FileStamp::read(&path).unwrap();
    let project_key = crate::store::normalize_path_str(&project_dir.to_string_lossy());
    let mut projects = BTreeMap::new();
    projects.insert(
        project_key,
        BTreeMap::from([(
            "session.jsonl".to_string(),
            SessionNameEntry {
                name: "Cached".to_string(),
                observed_length: stamp.observed_length,
                modified_secs: stamp.modified_secs,
                modified_nanos: stamp.modified_nanos,
                cached_at_ms: 1_000,
            },
        )]),
    );
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects,
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    let resolved = resolver.resolve(&project_dir, &path, stamp);
    let stats = resolver.stats();
    let pending = resolver.finish();

    assert_eq!(resolved.name, "Cached");
    assert_eq!(stats.exact_hits, 1);
    assert_eq!(stats.full_rebuilds, 0);
    assert_eq!(stats.jsonl_bytes_read, 0);
    assert!(pending.is_none());
}

// 稳定 miss 必须生成携带 base entry 与 replacement 的单条 mutation。
#[test]
fn Resolver_Miss_CreatesStableDelta_021() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("-e-source-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let path = project_dir.join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Fresh\"}}\n",
    )
    .unwrap();
    let stamp = FileStamp::read(&path).unwrap();
    let stale_stamp = FileStamp {
        observed_length: stamp.observed_length.saturating_sub(1),
        ..stamp
    };
    let stale = SessionNameEntry {
        name: "Stale".to_string(),
        observed_length: stale_stamp.observed_length,
        modified_secs: stale_stamp.modified_secs,
        modified_nanos: stale_stamp.modified_nanos,
        cached_at_ms: 1_000,
    };
    let project_key = crate::store::normalize_path_str(&project_dir.to_string_lossy());
    let mut projects = BTreeMap::new();
    projects.insert(
        project_key.clone(),
        BTreeMap::from([("session.jsonl".to_string(), stale.clone())]),
    );
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects,
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    let resolved = resolver.resolve(&project_dir, &path, stamp);
    let pending = resolver.finish().unwrap();

    assert_eq!(resolved.name, "Fresh");
    assert_eq!(pending.delta.mutations.len(), 1);
    let mutation = &pending.delta.mutations[0];
    assert_eq!(mutation.project_key, project_key);
    assert_eq!(mutation.file_name, "session.jsonl");
    assert_eq!(mutation.base, Some(stale));
    assert_eq!(mutation.replacement.name, "Fresh");
    assert_eq!(mutation.path, path);
}

// 扫描前后指纹不稳定时仍返回业务名称，但不得生成 replacement delta。
#[test]
fn Resolver_Unstable_NoDelta_022() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("-e-source-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let path = project_dir.join("session.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Before\"}}\n",
    )
    .unwrap();
    let stale_stamp = FileStamp::read(&path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"After\"}\n")
        .unwrap();
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects: BTreeMap::new(),
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    let resolved = resolver.resolve(&project_dir, &path, stale_stamp);
    let stats = resolver.stats();

    assert_eq!(resolved.name, "After");
    assert_eq!(stats.full_rebuilds, 1);
    assert!(stats.jsonl_bytes_read > 0);
    assert!(resolver.finish().is_none());
}

// 只有完整枚举的目录才携带 base bucket 与 live 文件集供后续 CAS 清理。
#[test]
fn Resolver_Dirs_PruneComplete_023() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("-e-source-project");
    let project_key = crate::store::normalize_path_str(&project_dir.to_string_lossy());
    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let base_bucket = BTreeMap::from([
        (
            "live.jsonl".to_string(),
            SessionNameEntry {
                name: "Live".to_string(),
                observed_length: stamp.observed_length,
                modified_secs: stamp.modified_secs,
                modified_nanos: stamp.modified_nanos,
                cached_at_ms: 1,
            },
        ),
        (
            "stale.jsonl".to_string(),
            SessionNameEntry {
                name: "Stale".to_string(),
                observed_length: stamp.observed_length,
                modified_secs: stamp.modified_secs,
                modified_nanos: stamp.modified_nanos,
                cached_at_ms: 1,
            },
        ),
    ]);
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects: BTreeMap::from([(project_key.clone(), base_bucket.clone())]),
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    resolver.record_directory(
        &project_dir,
        BTreeSet::from(["live.jsonl".to_string()]),
        true,
    );
    let pending = resolver.finish().unwrap();

    assert_eq!(pending.delta.directories.len(), 1);
    assert_eq!(pending.delta.directories[0].project_key, project_key);
    assert_eq!(pending.delta.directories[0].base_bucket, base_bucket);
    assert_eq!(
        pending.delta.directories[0].live_file_names,
        BTreeSet::from(["live.jsonl".to_string()])
    );
}

// 枚举不完整的目录不得触发删除清理。
#[test]
fn Resolver_Dirs_NoPruneIncomplete_024() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("-e-source-project");
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects: BTreeMap::new(),
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    resolver.record_directory(
        &project_dir,
        BTreeSet::from(["partial.jsonl".to_string()]),
        false,
    );

    assert!(resolver.finish().is_none());
}

// 超过 soft limit 的有效快照即使全命中也必须请求压缩写回。
#[test]
fn Resolver_SoftSize_CompressDelta_025() {
    let resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects: BTreeMap::new(),
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: true,
            parse_attempted: true,
        },
        2_000,
    );

    let pending = resolver.finish().unwrap();

    assert!(pending.delta.request_compaction);
    assert!(pending.delta.mutations.is_empty());
    assert!(pending.delta.directories.is_empty());
}

// 同一真实项目的两个编码目录必须保留为不同一级键。
#[test]
fn Resolver_DuplicateDirs_Separate_026() {
    let dir = tempfile::tempdir().unwrap();
    let first_dir = dir.path().join("-e-source-project-old");
    let second_dir = dir.path().join("-e-source-project-new");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();
    let first_path = first_dir.join("same.jsonl");
    let second_path = second_dir.join("same.jsonl");
    std::fs::write(
        &first_path,
        "{\"type\":\"user\",\"message\":{\"content\":\"First\"}}\n",
    )
    .unwrap();
    std::fs::write(
        &second_path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Second\"}}\n",
    )
    .unwrap();
    let mut resolver = SessionNameResolver::new(
        IndexSnapshot {
            index: SessionNameIndex {
                schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                parser_version: SESSION_NAME_PARSER_VERSION,
                projects: BTreeMap::new(),
            },
            raw: RawIndexSnapshot::Bytes(b"base-index".to_vec()),
            needs_compaction: false,
            parse_attempted: true,
        },
        2_000,
    );

    resolver.resolve(
        &first_dir,
        &first_path,
        FileStamp::read(&first_path).unwrap(),
    );
    resolver.resolve(
        &second_dir,
        &second_path,
        FileStamp::read(&second_path).unwrap(),
    );
    let pending = resolver.finish().unwrap();

    assert_eq!(pending.delta.mutations.len(), 2);
    assert_ne!(
        pending.delta.mutations[0].project_key,
        pending.delta.mutations[1].project_key
    );
}

// 基于同一旧快照的互不相交 mutation 必须能合并，不能后写覆盖先写。
#[test]
fn Flush_Disjoint_MergesBoth_030() {
    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let first = SessionNameEntry {
        name: "First".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 10,
    };
    let second = SessionNameEntry {
        name: "Second".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 20,
    };
    let latest = SessionNameIndex {
        projects: BTreeMap::from([(
            "project".to_string(),
            BTreeMap::from([("first.jsonl".to_string(), first.clone())]),
        )]),
        ..SessionNameIndex::empty()
    };
    let delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "second.jsonl".to_string(),
            path: std::path::Path::new("second.jsonl").to_path_buf(),
            base: None,
            replacement: second.clone(),
        }],
        ..SessionNameIndexDelta::default()
    };

    let merged = merge_delta_in_memory(latest, &delta, IndexLimits::default()).unwrap();

    assert_eq!(merged.index.projects["project"]["first.jsonl"], first);
    assert_eq!(merged.index.projects["project"]["second.jsonl"], second);

    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let initial_bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &initial_bytes).unwrap();
    let first_path = dir.path().join("first.jsonl");
    let second_path = dir.path().join("second.jsonl");
    std::fs::write(&first_path, "first").unwrap();
    std::fs::write(&second_path, "second").unwrap();
    let first_stamp = FileStamp::read(&first_path).unwrap();
    let second_stamp = FileStamp::read(&second_path).unwrap();
    let first_delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "first.jsonl".to_string(),
            path: first_path.to_path_buf(),
            base: None,
            replacement: SessionNameEntry {
                name: "First".to_string(),
                observed_length: first_stamp.observed_length,
                modified_secs: first_stamp.modified_secs,
                modified_nanos: first_stamp.modified_nanos,
                cached_at_ms: 10,
            },
        }],
        ..SessionNameIndexDelta::default()
    };
    let second_delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "second.jsonl".to_string(),
            path: second_path.to_path_buf(),
            base: None,
            replacement: SessionNameEntry {
                name: "Second".to_string(),
                observed_length: second_stamp.observed_length,
                modified_secs: second_stamp.modified_secs,
                modified_nanos: second_stamp.modified_nanos,
                cached_at_ms: 20,
            },
        }],
        ..SessionNameIndexDelta::default()
    };
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    );
    store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(initial_bytes.clone()),
            delta: first_delta,
        })
        .unwrap();
    store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(initial_bytes),
            delta: second_delta,
        })
        .unwrap();

    let persisted: SessionNameIndex =
        serde_json::from_slice(&std::fs::read(&paths.data).unwrap()).unwrap();
    assert_eq!(persisted.projects["project"].len(), 2);
}

// 当前条目不再等于 mutation 的 base 时必须保留新值。
#[test]
fn Flush_StaleBase_PreservesNew_031() {
    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let base = SessionNameEntry {
        name: "Base".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 10,
    };
    let concurrent = SessionNameEntry {
        name: "Concurrent".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 20,
    };
    let replacement = SessionNameEntry {
        name: "Stale replacement".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 30,
    };
    let latest = SessionNameIndex {
        projects: BTreeMap::from([(
            "project".to_string(),
            BTreeMap::from([("same.jsonl".to_string(), concurrent.clone())]),
        )]),
        ..SessionNameIndex::empty()
    };
    let delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "same.jsonl".to_string(),
            path: std::path::Path::new("same.jsonl").to_path_buf(),
            base: Some(base),
            replacement,
        }],
        ..SessionNameIndexDelta::default()
    };

    let merged = merge_delta_in_memory(latest, &delta, IndexLimits::default()).unwrap();

    assert_eq!(merged.index.projects["project"]["same.jsonl"], concurrent);
}

// 目录清理只在完整 bucket 仍等于 base 时执行，任一并发改动都必须阻止本次清理。
#[test]
fn Flush_Cleanup_CasOnly_034() {
    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let old_unchanged = SessionNameEntry {
        name: "Old unchanged".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 10,
    };
    let old_changed = SessionNameEntry {
        name: "Old changed".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 10,
    };
    let concurrent = SessionNameEntry {
        name: "Concurrent".to_string(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 20,
    };
    let latest = SessionNameIndex {
        projects: BTreeMap::from([(
            "project".to_string(),
            BTreeMap::from([
                ("unchanged.jsonl".to_string(), old_unchanged.clone()),
                ("changed.jsonl".to_string(), concurrent.clone()),
            ]),
        )]),
        ..SessionNameIndex::empty()
    };
    let delta = SessionNameIndexDelta {
        directories: vec![DirectorySnapshot {
            project_key: "project".to_string(),
            base_bucket: BTreeMap::from([
                ("unchanged.jsonl".to_string(), old_unchanged),
                ("changed.jsonl".to_string(), old_changed),
            ]),
            live_file_names: BTreeSet::new(),
        }],
        ..SessionNameIndexDelta::default()
    };

    let diverged = merge_delta_in_memory(latest, &delta, IndexLimits::default()).unwrap();
    assert!(diverged.index.projects["project"].contains_key("unchanged.jsonl"));
    assert_eq!(
        diverged.index.projects["project"]["changed.jsonl"],
        concurrent
    );

    let exact = SessionNameIndex {
        projects: BTreeMap::from([(
            "project".to_string(),
            delta.directories[0].base_bucket.clone(),
        )]),
        ..SessionNameIndex::empty()
    };
    let cleaned = merge_delta_in_memory(exact, &delta, IndexLimits::default()).unwrap();
    assert!(!cleaned.index.projects.contains_key("project"));
    assert_eq!(cleaned.cleaned_entries, 2);
}

// 超过软上限的索引必须批量压缩到目标上限以内。
#[test]
fn Flush_Compaction_ToTarget_036() {
    let delta = SessionNameIndexDelta {
        request_compaction: true,
        ..SessionNameIndexDelta::default()
    };

    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let bucket = (0..40)
        .map(|sequence| {
            (
                format!("session-{sequence:02}.jsonl"),
                SessionNameEntry {
                    name: format!("Session {sequence:02} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
                    observed_length: stamp.observed_length,
                    modified_secs: stamp.modified_secs,
                    modified_nanos: stamp.modified_nanos,
                    cached_at_ms: sequence,
                },
            )
        })
        .collect();
    let limits = IndexLimits {
        target_bytes: 2_000,
        soft_bytes: 2_500,
        hard_bytes: 20_000,
    };

    let merged = merge_delta_in_memory(
        SessionNameIndex {
            projects: BTreeMap::from([("project".to_string(), bucket)]),
            ..SessionNameIndex::empty()
        },
        &delta,
        limits,
    )
    .unwrap();

    assert!(merged.serialized.len() as u64 <= limits.target_bytes);
    assert!(merged.evicted_entries > 1);
}

// 相同输入的淘汰顺序和紧凑 JSON bytes 必须完全确定。
#[test]
fn Flush_Compaction_Deterministic_037() {
    let delta = SessionNameIndexDelta {
        request_compaction: true,
        ..SessionNameIndexDelta::default()
    };

    let first = merge_delta_in_memory(
        {
            let stamp = FileStamp {
                observed_length: 1,
                modified_secs: 1,
                modified_nanos: 1,
            };
            let bucket = (0..40)
                .map(|sequence| {
                    (
                        format!("session-{sequence:02}.jsonl"),
                        SessionNameEntry {
                            name: format!("Session {sequence:02} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
                            observed_length: stamp.observed_length,
                            modified_secs: stamp.modified_secs,
                            modified_nanos: stamp.modified_nanos,
                            cached_at_ms: sequence,
                        },
                    )
                })
                .collect();
            SessionNameIndex {
                projects: BTreeMap::from([("project".to_string(), bucket)]),
                ..SessionNameIndex::empty()
            }
        },
        &delta,
        IndexLimits {
            target_bytes: 2_000,
            soft_bytes: 2_500,
            hard_bytes: 20_000,
        },
    )
    .unwrap();
    let second = merge_delta_in_memory(
        {
            let stamp = FileStamp {
                observed_length: 1,
                modified_secs: 1,
                modified_nanos: 1,
            };
            let bucket = (0..40)
                .map(|sequence| {
                    (
                        format!("session-{sequence:02}.jsonl"),
                        SessionNameEntry {
                            name: format!("Session {sequence:02} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
                            observed_length: stamp.observed_length,
                            modified_secs: stamp.modified_secs,
                            modified_nanos: stamp.modified_nanos,
                            cached_at_ms: sequence,
                        },
                    )
                })
                .collect();
            SessionNameIndex {
                projects: BTreeMap::from([("project".to_string(), bucket)]),
                ..SessionNameIndex::empty()
            }
        },
        &delta,
        IndexLimits {
            target_bytes: 2_000,
            soft_bytes: 2_500,
            hard_bytes: 20_000,
        },
    )
    .unwrap();

    assert_eq!(first.serialized, second.serialized);
    assert_eq!(first.index, second.index);
}

// 容量淘汰必须优先删除 cachedAtMs 最旧的条目。
#[test]
fn Flush_OldEntry_Evictable_039() {
    let delta = SessionNameIndexDelta {
        request_compaction: true,
        ..SessionNameIndexDelta::default()
    };

    let stamp = FileStamp {
        observed_length: 1,
        modified_secs: 1,
        modified_nanos: 1,
    };
    let entries = (0..40)
        .map(|sequence| {
            (
                format!("session-{sequence:02}.jsonl"),
                SessionNameEntry {
                    name: format!("Session {sequence:02} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
                    observed_length: stamp.observed_length,
                    modified_secs: stamp.modified_secs,
                    modified_nanos: stamp.modified_nanos,
                    cached_at_ms: sequence,
                },
            )
        })
        .collect();
    let merged = merge_delta_in_memory(
        SessionNameIndex {
            projects: BTreeMap::from([("project".to_string(), entries)]),
            ..SessionNameIndex::empty()
        },
        &delta,
        IndexLimits {
            target_bytes: 2_000,
            soft_bytes: 2_500,
            hard_bytes: 20_000,
        },
    )
    .unwrap();
    let bucket = &merged.index.projects["project"];

    assert!(!bucket.contains_key("session-00.jsonl"));
    assert!(bucket.contains_key("session-39.jsonl"));
}

// JSONL 在前台解析后继续增长时，后台必须丢弃该 replacement。
#[test]
fn Flush_GrownFile_DropsReplacement_032() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let session_path = dir.path().join("session.jsonl");
    std::fs::write(
        &session_path,
        "{\"type\":\"user\",\"message\":{\"content\":\"Before\"}}\n",
    )
    .unwrap();
    let stamp = FileStamp::read(&session_path).unwrap();
    let delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "session.jsonl".to_string(),
            path: session_path.to_path_buf(),
            base: None,
            replacement: SessionNameEntry {
                name: "Before".to_string(),
                observed_length: stamp.observed_length,
                modified_secs: stamp.modified_secs,
                modified_nanos: stamp.modified_nanos,
                cached_at_ms: 10,
            },
        }],
        ..SessionNameIndexDelta::default()
    };
    std::fs::OpenOptions::new()
        .append(true)
        .open(&session_path)
        .unwrap()
        .write_all(b"{\"type\":\"custom-title\",\"customTitle\":\"After\"}\n")
        .unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    );

    let metrics = store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Missing,
            delta,
        })
        .unwrap();

    assert_eq!(metrics.attempts, 1);
    assert!(!paths.data.exists());
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.to_string_lossy()
                .contains("session-name-index.json.tmp.")
        })
        .collect::<Vec<_>>()
        .is_empty());
}

// 当前 exact stamp 与 replacement 不同，即使长度相同也不得写回。
#[test]
fn Flush_ChangedStamp_DropsReplacement_033() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let session_path = dir.path().join("session.jsonl");
    std::fs::write(&session_path, "same-length").unwrap();
    let current = FileStamp::read(&session_path).unwrap();
    let stale = FileStamp {
        modified_secs: current.modified_secs.saturating_sub(1),
        ..current
    };
    let delta = SessionNameIndexDelta {
        mutations: vec![IndexMutation {
            project_key: "project".to_string(),
            file_name: "session.jsonl".to_string(),
            path: session_path.to_path_buf(),
            base: None,
            replacement: SessionNameEntry {
                name: "Stale".to_string(),
                observed_length: stale.observed_length,
                modified_secs: stale.modified_secs,
                modified_nanos: stale.modified_nanos,
                cached_at_ms: 10,
            },
        }],
        ..SessionNameIndexDelta::default()
    };
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    );

    store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Missing,
            delta,
        })
        .unwrap();

    assert!(!paths.data.exists());
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.to_string_lossy()
                .contains("session-name-index.json.tmp.")
        })
        .collect::<Vec<_>>()
        .is_empty());
}

// serde/merge/compaction/serialize/temp/sync 阶段都不得观察到排他锁已持有。
#[test]
fn Flush_NoExpensiveWork_UnderLock_035() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &bytes).unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let probe = Arc::new(move |stage, held| captured.lock().unwrap().push((stage, held)));
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .with_flush_test_config(Duration::from_secs(1), 4, Some(probe), None, None);

    store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(bytes),
            delta: SessionNameIndexDelta {
                request_compaction: true,
                ..SessionNameIndexDelta::default()
            },
        })
        .unwrap();

    let expensive = [
        FlushStage::Deserialize,
        FlushStage::Merge,
        FlushStage::Compaction,
        FlushStage::Serialize,
        FlushStage::TempWrite,
        FlushStage::Sync,
    ];
    let observations = observations.lock().unwrap();
    assert!(observations
        .iter()
        .filter(|(stage, _)| expensive.contains(stage))
        .all(|(_, held)| !held));
}

// 原子替换失败必须保留旧索引并清理本次唯一临时文件。
#[test]
fn Flush_AtomicFail_KeepsOld_038() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &bytes).unwrap();
    let fail_replace = Arc::new(|_: &std::path::Path, _: &std::path::Path| {
        Err(std::io::Error::other("injected replace failure"))
    });
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths.clone(),
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .with_flush_test_config(Duration::from_secs(1), 4, None, None, Some(fail_replace));

    let error = store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(bytes.clone()),
            delta: SessionNameIndexDelta {
                request_compaction: true,
                ..SessionNameIndexDelta::default()
            },
        })
        .unwrap_err();

    assert!(error.to_string().contains("injected replace failure"));
    assert_eq!(std::fs::read(&paths.data).unwrap(), bytes);
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.to_string_lossy()
                .contains("session-name-index.json.tmp.")
        })
        .collect::<Vec<_>>()
        .is_empty());
    assert_eq!(warnings.lock().unwrap().len(), 1);
}

// 探针必须同时看到锁外准备阶段和仅限锁内的 raw compare/replace 阶段。
#[test]
fn Flush_Probe_AllPreparationUnlocked_040() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &bytes).unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let probe = Arc::new(move |stage, held| captured.lock().unwrap().push((stage, held)));
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .with_flush_test_config(Duration::from_secs(1), 4, Some(probe), None, None);

    store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(bytes),
            delta: SessionNameIndexDelta {
                request_compaction: true,
                ..SessionNameIndexDelta::default()
            },
        })
        .unwrap();

    let observations = observations.lock().unwrap();
    assert!(observations.contains(&(FlushStage::TempWrite, false)));
    assert!(observations.contains(&(FlushStage::Sync, false)));
    assert!(observations.contains(&(FlushStage::LockedRawCompare, true)));
    assert!(observations.contains(&(FlushStage::Replace, true)));
}

// 锁竞争必须受同一个累计 flush budget 约束，不能每次重置等待窗口。
#[test]
fn Flush_RetryBudget_Cumulative_041() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &bytes).unwrap();
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&paths.lock)
        .unwrap();
    acquire_lock_with_label(
        &holder,
        true,
        Duration::from_secs(1),
        "session-name-index.json",
    )
    .unwrap();
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .with_flush_test_config(Duration::from_millis(60), 4, None, None, None);

    let started = Instant::now();
    let error = store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(bytes),
            delta: SessionNameIndexDelta {
                request_compaction: true,
                ..SessionNameIndexDelta::default()
            },
        })
        .unwrap_err();
    let elapsed = started.elapsed();
    let _ = holder.unlock();

    assert!(error.to_string().contains("lock timeout"));
    assert!(elapsed < Duration::from_millis(250), "elapsed={elapsed:?}");
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.to_string_lossy()
                .contains("session-name-index.json.tmp.")
        })
        .collect::<Vec<_>>()
        .is_empty());
}

// 四次 whole-file CAS 都失配后必须停止并清理所有尝试的临时文件。
#[test]
fn Flush_CasExhausted_CleansTemps_042() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SessionNameIndexPaths {
        data: dir.path().join("session-name-index.json"),
        lock: dir.path().join("session-name-index.json.lock"),
    };
    let bytes = serde_json::to_vec(&SessionNameIndex::empty()).unwrap();
    std::fs::write(&paths.data, &bytes).unwrap();
    let attempts = Arc::new(AtomicU64::new(0));
    let captured_attempts = Arc::clone(&attempts);
    let data_path = paths.data.clone();
    let before_exclusive = Arc::new(move |attempt: usize| {
        captured_attempts.fetch_add(1, Ordering::SeqCst);
        let mut index = SessionNameIndex::empty();
        index.projects.insert(
            format!("concurrent-{attempt}"),
            BTreeMap::from([(
                "session.jsonl".to_string(),
                SessionNameEntry {
                    name: format!("Concurrent {attempt}"),
                    observed_length: attempt as u64,
                    modified_secs: 1,
                    modified_nanos: 1,
                    cached_at_ms: attempt as u64,
                },
            )]),
        );
        std::fs::write(&data_path, serde_json::to_vec(&index).unwrap()).unwrap();
    });
    let now_ms = Arc::new(AtomicU64::new(1_000));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let clock_state = Arc::clone(&now_ms);
    let warning_state = Arc::clone(&warnings);
    let health = Arc::new(IndexHealth::new(
        move || clock_state.load(Ordering::SeqCst),
        move |message| warning_state.lock().unwrap().push(message),
    ));
    let store = SessionNameIndexStore::new(
        paths,
        IndexLimits::default(),
        health,
        Duration::from_millis(100),
    )
    .with_flush_test_config(
        Duration::from_secs(1),
        4,
        None,
        Some(before_exclusive),
        None,
    );

    let error = store
        .flush_pending(PendingIndexFlush {
            base_raw: RawIndexSnapshot::Bytes(bytes),
            delta: SessionNameIndexDelta {
                request_compaction: true,
                ..SessionNameIndexDelta::default()
            },
        })
        .unwrap_err();

    assert!(error.to_string().contains("CAS exhausted"));
    assert_eq!(attempts.load(Ordering::SeqCst), 4);
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.to_string_lossy()
                .contains("session-name-index.json.tmp.")
        })
        .collect::<Vec<_>>()
        .is_empty());
}

// 五档紧凑索引只量化锁外 serde parse，不把 CPU 成本混入排他持锁时间。
#[test]
#[ignore = "synthetic 64 KiB to 16 MiB index parse benchmark"]
fn BenchmarkIndexParseCost_002() {
    let targets = [
        64 * 1024usize,
        1024 * 1024,
        6 * 1024 * 1024,
        8 * 1024 * 1024,
        16 * 1024 * 1024,
    ];
    let payloads = targets
        .into_iter()
        .map(|target| {
            let mut entry_count = (target / 180).max(1);
            let mut closest = Vec::new();
            for _ in 0..8 {
                let mut index = SessionNameIndex {
                    schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                    parser_version: SESSION_NAME_PARSER_VERSION,
                    projects: Default::default(),
                };
                let bucket = index
                    .projects
                    .entry("E:/synthetic".to_string())
                    .or_default();
                for sequence in 0..entry_count {
                    bucket.insert(
                        format!("session-{sequence:08}.jsonl"),
                        SessionNameEntry {
                            name: format!("Synthetic benchmark session {sequence:08} xxxxxxxxxx"),
                            observed_length: 1_000_000 + sequence as u64,
                            modified_secs: 1_786_380_000,
                            modified_nanos: sequence as u32,
                            cached_at_ms: 1_786_380_000_000 + sequence as u64,
                        },
                    );
                }
                let bytes = serde_json::to_vec(&index).unwrap();
                let actual_bytes = bytes.len();
                let distance = bytes.len().abs_diff(target);
                if closest.is_empty() || distance < closest.len().abs_diff(target) {
                    closest = bytes;
                }
                if distance <= target / 50 {
                    break;
                }
                entry_count = entry_count
                    .saturating_mul(target)
                    .checked_div(actual_bytes.max(1))
                    .unwrap_or(1)
                    .max(1);
            }
            (target, closest)
        })
        .collect::<Vec<_>>();
    let mut samples = std::collections::BTreeMap::<usize, Vec<f64>>::new();

    for round in 0..7 {
        for offset in 0..payloads.len() {
            let (target, bytes) = &payloads[(round + offset) % payloads.len()];
            let started = Instant::now();
            let parsed = serde_json::from_slice::<SessionNameIndex>(bytes).unwrap();
            black_box(parsed);
            samples
                .entry(*target)
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    for (target, bytes) in &payloads {
        let values = samples.get_mut(target).unwrap();
        let raw_samples = values.clone();
        values.sort_by(f64::total_cmp);
        eprintln!(
            "target_bytes={target}; actual_bytes={}; parse_samples_ms={raw_samples:?}; p50_ms={:.3}; p95_ms={:.3}",
            bytes.len(),
            values[3],
            values[6],
        );
    }
}

// 五档完整 flush 基准量化每个阶段，所有索引和临时文件均位于测试临时目录。
#[test]
#[ignore = "synthetic 64 KiB to 16 MiB full flush benchmark"]
fn BenchmarkIndexSizes_001() {
    let targets = [
        64 * 1024usize,
        1024 * 1024,
        6 * 1024 * 1024,
        8 * 1024 * 1024,
        16 * 1024 * 1024,
    ];
    let payloads = targets
        .into_iter()
        .map(|target| {
            let mut entry_count = (target / 180).max(1);
            let mut closest = Vec::new();
            for _ in 0..8 {
                let mut index = SessionNameIndex {
                    schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
                    parser_version: SESSION_NAME_PARSER_VERSION,
                    projects: Default::default(),
                };
                let bucket = index
                    .projects
                    .entry("E:/synthetic".to_string())
                    .or_default();
                for sequence in 0..entry_count {
                    bucket.insert(
                        format!("session-{sequence:08}.jsonl"),
                        SessionNameEntry {
                            name: format!("Synthetic benchmark session {sequence:08} xxxxxxxxxx"),
                            observed_length: 1_000_000 + sequence as u64,
                            modified_secs: 1_786_380_000,
                            modified_nanos: sequence as u32,
                            cached_at_ms: 1_786_380_000_000 + sequence as u64,
                        },
                    );
                }
                let bytes = serde_json::to_vec(&index).unwrap();
                let actual_bytes = bytes.len();
                let distance = bytes.len().abs_diff(target);
                if closest.is_empty() || distance < closest.len().abs_diff(target) {
                    closest = bytes;
                }
                if distance <= target / 50 {
                    break;
                }
                entry_count = entry_count
                    .saturating_mul(target)
                    .checked_div(actual_bytes.max(1))
                    .unwrap_or(1)
                    .max(1);
            }
            (target, closest)
        })
        .collect::<Vec<_>>();

    for (target, bytes) in payloads {
        let mut stages = BTreeMap::<&'static str, Vec<f64>>::new();
        let mut attempts = Vec::new();
        let mut output_bytes = Vec::new();
        for _ in 0..7 {
            let dir = tempfile::tempdir().unwrap();
            let paths = SessionNameIndexPaths {
                data: dir.path().join("session-name-index.json"),
                lock: dir.path().join("session-name-index.json.lock"),
            };
            std::fs::write(&paths.data, &bytes).unwrap();
            let now_ms = Arc::new(AtomicU64::new(1_000));
            let warnings = Arc::new(Mutex::new(Vec::new()));
            let clock_state = Arc::clone(&now_ms);
            let warning_state = Arc::clone(&warnings);
            let health = Arc::new(IndexHealth::new(
                move || clock_state.load(Ordering::SeqCst),
                move |message| warning_state.lock().unwrap().push(message),
            ));
            let store = SessionNameIndexStore::new(
                paths,
                IndexLimits::default(),
                health,
                Duration::from_millis(100),
            );
            let metrics = store
                .flush_pending(PendingIndexFlush {
                    base_raw: RawIndexSnapshot::Bytes(bytes.clone()),
                    delta: SessionNameIndexDelta {
                        request_compaction: true,
                        ..SessionNameIndexDelta::default()
                    },
                })
                .unwrap();
            let stage_timings = {
                let mut clone = crate::session_name_index::FlushMetrics {
                    revalidate: metrics.revalidate,
                    raw_read: metrics.raw_read,
                    deserialize: metrics.deserialize,
                    merge: metrics.merge,
                    compaction: metrics.compaction,
                    serialize: metrics.serialize,
                    temp_write: metrics.temp_write,
                    sync: metrics.sync,
                    lock_wait: metrics.lock_wait,
                    locked_raw_compare: metrics.locked_raw_compare,
                    replace: metrics.replace,
                    exclusive_hold: metrics.exclusive_hold,
                    ..Default::default()
                };
                FlushStage::ALL.map(|stage| {
                    let elapsed = std::mem::take(stage.field_mut(&mut clone));
                    (stage.label(), elapsed.as_secs_f64() * 1000.0)
                })
            };
            for (stage, elapsed_ms) in stage_timings {
                stages.entry(stage).or_default().push(elapsed_ms);
            }
            attempts.push(metrics.attempts);
            output_bytes.push(metrics.output_bytes);
            assert!(std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.to_string_lossy()
                        .contains("session-name-index.json.tmp.")
                })
                .collect::<Vec<_>>()
                .is_empty());
        }

        eprintln!(
            "index_target_bytes={target}; input_bytes={}; attempts={attempts:?}; output_bytes={output_bytes:?}",
            bytes.len()
        );
        for (stage, samples) in &mut stages {
            let raw = samples.clone();
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "stage={stage}; samples_ms={raw:?}; p50_ms={:.3}; p95_ms={:.3}",
                samples[3], samples[6]
            );
        }

        if target == 8 * 1024 * 1024 {
            assert!(output_bytes.iter().all(|bytes| *bytes <= 6 * 1024 * 1024));
        }
        if target == 16 * 1024 * 1024 {
            assert!(output_bytes.iter().all(|bytes| *bytes <= 6 * 1024 * 1024));
        }
    }
}
