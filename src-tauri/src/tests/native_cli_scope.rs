use crate::session_name_index::{
    IndexHealth, IndexLimits, SessionNameIndexPaths, SessionNameIndexStore,
};
use crate::store::{
    get_home_data_indexed_at, get_sessions_indexed_at, with_project_path_mapping,
    ProjectPathMapping,
};
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

fn index_at(root: &Path) -> SessionNameIndexStore {
    SessionNameIndexStore::new(
        SessionNameIndexPaths {
            data: root.join("index.json"),
            lock: root.join("index.lock"),
        },
        IndexLimits::default(),
        Arc::new(IndexHealth::new(|| 1000, |_| {})),
        Duration::from_secs(1),
    )
}
fn write_session(root: &Path, name: &str, modified: SystemTime) -> PathBuf {
    let project = root.join("projects/encoded");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("same-id.jsonl");
    let cwd = root.parent().unwrap().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let cwd = serde_json::to_string(cwd.to_str().unwrap()).unwrap();
    fs::write(
        &path,
        format!("{{\"cwd\":{cwd}}}\n{{\"type\":\"custom-title\",\"customTitle\":\"{name}\"}}\n"),
    )
    .unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    project
}

// 同一路径改指向另一个目录，即便文件名/长度/mtime 全相同也不能命中旧根名称。
#[test]
fn D12_Index_RootReplacementInvalidatesExactStamp_001() {
    let temp = tempfile::tempdir().unwrap();
    let active = temp.path().join("active");
    let next = temp.path().join("next");
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let project = write_session(&active, "LEFT-name", time);
    write_session(&next, "RIGHTname", time);
    let store = index_at(temp.path());
    let left = get_sessions_indexed_at(
        "/fixture/project",
        std::slice::from_ref(&project),
        20,
        0,
        &store,
        1,
        None,
    )
    .unwrap();
    assert_eq!(left.value[0].name, "LEFT-name");
    store.flush_pending(left.pending_flush.unwrap()).unwrap();
    fs::rename(&active, temp.path().join("retired")).unwrap();
    fs::rename(&next, &active).unwrap();
    let right =
        get_sessions_indexed_at("/fixture/project", &[project], 20, 0, &store, 2, None).unwrap();
    assert_eq!(right.value[0].name, "RIGHTname");
    assert_eq!(right.stats.exact_hits, 0);
    assert_eq!(right.stats.full_rebuilds, 1);
}

// 用独立测试进程检查共享全局缓存，不与其他并发 store fixture 争用状态。
#[test]
fn D12_Mapping_ExplicitRootCannotPoisonLegacy_002() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "tests::native_cli_scope::D12_Mapping_IsolatedWorker",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "isolated mapping assertion failed");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("isolated mapping assertion timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "executed with a deadline by D12_Mapping_ExplicitRootCannotPoisonLegacy_002"]
fn D12_Mapping_IsolatedWorker() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("explicit");
    write_session(&source, "explicit", SystemTime::now());
    let sentinel =
        ProjectPathMapping::from([("legacy-only".into(), vec![PathBuf::from("legacy-encoded")])]);
    with_project_path_mapping(|cache| *cache = Some(sentinel.clone()));
    let result = get_home_data_indexed_at(
        &source.join("projects"),
        20,
        20,
        "",
        &[],
        &index_at(temp.path()),
        1,
        None,
    )
    .unwrap();
    assert_eq!(result.value.recent_sessions[0].name, "explicit");
    assert_eq!(
        with_project_path_mapping(|cache| cache.clone()),
        Some(sentinel.clone())
    );
    use crate::store::{invalidate_project_path_mapping, with_project_path_mapping_at};
    let first = with_project_path_mapping_at(&source.join("projects"), |cache| cache.clone());
    assert!(first.as_ref().is_some_and(|map| !map.is_empty()));
    let second_root = temp.path().join("second");
    write_session(&second_root, "another", SystemTime::now());
    let second = get_home_data_indexed_at(
        &second_root.join("projects"),
        20,
        20,
        "",
        &[],
        &index_at(temp.path()),
        1,
        None,
    )
    .unwrap();
    assert_eq!(second.value.recent_sessions[0].name, "another");
    assert_eq!(
        with_project_path_mapping_at(&source.join("projects"), |cache| cache.clone()),
        first
    );
    assert_ne!(
        with_project_path_mapping_at(&second_root.join("projects"), |cache| cache.clone()),
        first
    );
    assert_eq!(
        with_project_path_mapping(|cache| cache.clone()),
        Some(sentinel)
    );
    // Unknown/invalid explicit roots never mutate the legacy partition.
    with_project_path_mapping_at(Path::new("relative-root"), |cache| {
        *cache = Some(ProjectPathMapping::new())
    });
    assert!(with_project_path_mapping_at(
        Path::new("relative-root"),
        |cache| cache.is_none()
    ));
    invalidate_project_path_mapping();
    assert!(with_project_path_mapping(|cache| cache.is_none()));
    assert!(with_project_path_mapping_at(
        &source.join("projects"),
        |cache| cache.is_none()
    ));
    assert!(with_project_path_mapping_at(
        &second_root.join("projects"),
        |cache| cache.is_none()
    ));
}

// 多根同时读写同一个派生索引，ID/长度/mtime 相同也只命中各自来源。
#[test]
fn D12_Index_TwoRootsRemainIndependentAfterFlush_003() {
    let temp = tempfile::tempdir().unwrap();
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let left = write_session(&temp.path().join("left"), "LEFT-name", time);
    let right = write_session(&temp.path().join("right"), "RIGHTname", time);
    let store = index_at(temp.path());
    for (dir, title) in [(&left, "LEFT-name"), (&right, "RIGHTname")] {
        let result = get_sessions_indexed_at(
            "/fixture/project",
            std::slice::from_ref(dir),
            20,
            0,
            &store,
            1,
            None,
        )
        .unwrap();
        assert_eq!(result.value[0].name, title);
        assert_eq!(result.stats.exact_hits, 0);
        store.flush_pending(result.pending_flush.unwrap()).unwrap();
    }
    for (dir, title) in [(&right, "RIGHTname"), (&left, "LEFT-name")] {
        let result = get_sessions_indexed_at(
            "/fixture/project",
            std::slice::from_ref(dir),
            20,
            0,
            &store,
            2,
            None,
        )
        .unwrap();
        assert_eq!(result.value[0].name, title);
        assert_eq!(result.stats.exact_hits, 1);
        assert_eq!(result.stats.jsonl_bytes_read, 0);
    }
}

// scope 身份由后端产生，CLI/root/epoch 任一变化都不能复用同一缓存桶。
#[test]
fn D12_Partition_KeysIncludeCliRootAndEpoch_004() {
    use crate::cli::projection::SourcePartition;
    use crate::cli::types::{CliKind, WireU64};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let project = write_session(&root, "title", SystemTime::now());
    let epoch = WireU64::parse("9007199254740993").unwrap();
    let claude = SourcePartition::new(CliKind::Claude, &root, epoch).unwrap();
    let codex = SourcePartition::new(CliKind::Codex, &root, epoch).unwrap();
    let next = SourcePartition::new(
        CliKind::Claude,
        &root,
        WireU64::parse("9007199254740994").unwrap(),
    )
    .unwrap();
    let key = claude.project_index_key(&project).unwrap();
    assert_ne!(key, codex.project_index_key(&project).unwrap());
    assert_ne!(key, next.project_index_key(&project).unwrap());
    assert!(key.contains("9007199254740993"));
    let other_root = temp.path().join("other");
    let other_project = write_session(&other_root, "title", SystemTime::now());
    let other = SourcePartition::new(CliKind::Claude, &other_root, epoch).unwrap();
    assert_ne!(key, other.project_index_key(&other_project).unwrap());
}

// 缺失来源、不支持的 CLI、根之外目录均不退回用户默认目录。
#[test]
fn D12_Partition_UnknownAndOutsideRootFailClosed_005() {
    use crate::cli::projection::SourcePartition;
    use crate::cli::types::{CliKind, WireU64};
    let temp = tempfile::tempdir().unwrap();
    let epoch = WireU64::parse("0").unwrap();
    assert!(SourcePartition::new(CliKind::Claude, &temp.path().join("missing"), epoch).is_err());
    assert!(SourcePartition::new(CliKind::Shell, temp.path(), epoch).is_err());
    let root = temp.path().join("root");
    fs::create_dir_all(&root).unwrap();
    let partition = SourcePartition::new(CliKind::Claude, &root, epoch).unwrap();
    let other = temp.path().join("other");
    fs::create_dir_all(&other).unwrap();
    assert!(partition.project_index_key(&other).is_none());
    fs::write(root.join("file"), b"not a directory").unwrap();
    assert!(partition.project_index_key(&root.join("file")).is_none());
    assert!(SourcePartition::new(CliKind::Claude, &root.join("file"), epoch).is_err());
}

#[test]
fn D12_Partition_PreviousRootIdentityCannotSurviveReplacement_006() {
    use crate::cli::projection::SourcePartition;
    use crate::cli::types::{CliKind, WireU64};
    let temp = tempfile::tempdir().unwrap();
    let active = temp.path().join("active");
    let project = write_session(&active, "first", SystemTime::now());
    write_session(&temp.path().join("next"), "other", SystemTime::now());
    let original =
        SourcePartition::new(CliKind::Claude, &active, WireU64::parse("0").unwrap()).unwrap();
    assert!(original.project_index_key(&project).is_some());
    fs::rename(&active, temp.path().join("retired")).unwrap();
    fs::rename(temp.path().join("next"), &active).unwrap();
    assert!(original.project_index_key(&project).is_none());
}

#[cfg(unix)]
#[test]
fn D12_Partition_SymlinkOutsideAndCaseDistinct_007() {
    use crate::cli::projection::SourcePartition;
    use crate::cli::types::{CliKind, WireU64};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("A")).unwrap();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(temp.path().join("outside")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("outside"), root.join("escape")).unwrap();
    std::os::unix::fs::symlink(root.join("A"), root.join("alias")).unwrap();
    let source =
        SourcePartition::new(CliKind::Claude, &root, WireU64::parse("0").unwrap()).unwrap();
    assert!(source.project_index_key(&root.join("escape")).is_none());
    assert_eq!(
        source.project_index_key(&root.join("A")),
        source.project_index_key(&root.join("alias"))
    );
    // On case-insensitive volumes A and a are the same physical directory.
    let a = crate::cli::source_scope::resolve_path_key(&root.join("A")).unwrap();
    let b = crate::cli::source_scope::resolve_path_key(&root.join("a")).unwrap();
    if a.key != b.key {
        assert_ne!(
            source.project_index_key(&root.join("A")),
            source.project_index_key(&root.join("a"))
        );
    }
}

#[cfg(windows)]
#[test]
fn D12_Partition_WindowsJunctionCannotCrossRoot_008() {
    use crate::cli::projection::SourcePartition;
    use crate::cli::types::{CliKind, WireU64};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let project = root.join("actual");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let alias = root.join("alias");
    let escape = root.join("escape");
    for (link, target) in [(&alias, &project), (&escape, &outside)] {
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    let source =
        SourcePartition::new(CliKind::Claude, &root, WireU64::parse("0").unwrap()).unwrap();
    assert!(source.project_index_key(&escape).is_none());
    assert_eq!(
        source.project_index_key(&alias),
        source.project_index_key(&project)
    );
    fs::remove_dir(alias).unwrap();
    fs::remove_dir(escape).unwrap();
}

#[test]
fn D12_Index_V1CacheRebuildsWithoutReusingUnscopedTitle_009() {
    use crate::session_name_index::{FileStamp, SessionNameEntry, SessionNameIndex};
    use std::collections::BTreeMap;
    let temp = tempfile::tempdir().unwrap();
    let project = write_session(&temp.path().join("root"), "native-title", SystemTime::now());
    let file = project.join("same-id.jsonl");
    let stamp = FileStamp::read(&file).unwrap();
    let entry = SessionNameEntry {
        name: "unscoped-wrong-title".into(),
        observed_length: stamp.observed_length,
        modified_secs: stamp.modified_secs,
        modified_nanos: stamp.modified_nanos,
        cached_at_ms: 1,
    };
    let mut old = SessionNameIndex::empty();
    old.schema_version = 1;
    old.projects.insert(
        crate::store::normalize_path_str(&project.to_string_lossy()),
        BTreeMap::from([("same-id.jsonl".into(), entry)]),
    );
    fs::write(
        temp.path().join("index.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    let original = fs::read(&file).unwrap();
    let store = index_at(temp.path());
    let result =
        get_sessions_indexed_at("/fixture/project", &[project], 20, 0, &store, 2, None).unwrap();
    assert_eq!(result.value[0].name, "native-title");
    assert_eq!(result.stats.exact_hits, 0);
    store.flush_pending(result.pending_flush.unwrap()).unwrap();
    assert_eq!(store.read_snapshot().index.schema_version, 2);
    assert_eq!(fs::read(file).unwrap(), original);
}
