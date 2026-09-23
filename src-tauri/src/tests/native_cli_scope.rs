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
    fs::write(
        &path,
        format!("{{\"cwd\":\"/fixture/project\"}}\n{{\"type\":\"custom-title\",\"customTitle\":\"{name}\"}}\n"),
    ).unwrap();
    File::options().write(true).open(path).unwrap()
        .set_times(FileTimes::new().set_modified(modified)).unwrap();
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
    let left = get_sessions_indexed_at("/fixture/project", std::slice::from_ref(&project), 20, 0, &store, 1, None).unwrap();
    assert_eq!(left.value[0].name, "LEFT-name");
    store.flush_pending(left.pending_flush.unwrap()).unwrap();
    fs::rename(&active, temp.path().join("retired")).unwrap();
    fs::rename(&next, &active).unwrap();
    let right = get_sessions_indexed_at("/fixture/project", &[project], 20, 0, &store, 2, None).unwrap();
    assert_eq!(right.value[0].name, "RIGHTname");
    assert_eq!(right.stats.exact_hits, 0);
    assert_eq!(right.stats.full_rebuilds, 1);
}

// 用独立测试进程检查共享全局缓存，不与其他并发 store fixture 争用状态。
#[test]
fn D12_Mapping_ExplicitRootCannotPoisonLegacy_002() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "tests::native_cli_scope::D12_Mapping_IsolatedWorker", "--nocapture"])
        .stdin(Stdio::null())
        .spawn().unwrap();
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
    let sentinel = ProjectPathMapping::from([("legacy-only".into(), vec![PathBuf::from("legacy-encoded")])]);
    with_project_path_mapping(|cache| *cache = Some(sentinel.clone()));
    let result = get_home_data_indexed_at(&source.join("projects"), 20, 20, "", &[], &index_at(temp.path()), 1, None).unwrap();
    assert_eq!(result.value.recent_sessions[0].name, "explicit");
    assert_eq!(with_project_path_mapping(|cache| cache.clone()), Some(sentinel));
}
