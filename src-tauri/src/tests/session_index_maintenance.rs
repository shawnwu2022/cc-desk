#![allow(non_snake_case)]
use super::*;

fn caller(epoch: &str) -> CallerIdentity {
    CallerIdentity {
        instance_id: "fixture".into(),
        window_label: "main".into(),
        webview_epoch: crate::cli::types::WireU64::parse(epoch).unwrap(),
    }
}

#[test]
fn SessionIndex_CancelPreservesDiskAndRejectsLateStaging_001() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    std::fs::write(
        temp.path().join("native-history-metadata.json"),
        b"original",
    )
    .unwrap();
    let owner = caller("1");
    let original = manager.route().unwrap();
    assert_eq!(
        manager.apply(&owner, "repair", "begin").unwrap().state,
        "staging"
    );
    let staging = manager.route().unwrap();
    assert_ne!(original, staging);
    assert!(manager.write_guard(&original).is_err());
    let other_process = IndexMaintenance::new(temp.path().to_owned());
    assert!(other_process.write_guard(&original).is_err());
    assert!(other_process
        .apply(&owner, "second-process", "begin")
        .is_err());
    assert_eq!(
        manager.apply(&owner, "repair", "cancel").unwrap().state,
        "cancelled"
    );
    assert_eq!(manager.route().unwrap(), original);
    assert!(manager.write_guard(&staging).is_err());
    assert_eq!(
        std::fs::read(temp.path().join("native-history-metadata.json")).unwrap(),
        b"original"
    );
}

#[test]
fn SessionIndex_StageFailurePreservesPublishedAndRemainsCancellable_003() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    let original = manager.route().unwrap();
    manager.apply(&owner, "repair", "begin").unwrap();
    let staging = manager.route().unwrap();
    manager.record_failure(&staging);
    assert!(manager.apply(&owner, "repair", "commit").is_err());
    assert!(!temp.path().join("session-index-generation.json").exists());
    manager.apply(&owner, "repair", "cancel").unwrap();
    assert_eq!(manager.route().unwrap(), original);
}

#[test]
fn SessionIndex_CommitFencesOldGenerationAndBindsDocument_002() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    let old = manager.route().unwrap();
    manager.apply(&owner, "repair", "begin").unwrap();
    assert!(manager.apply(&caller("2"), "repair", "commit").is_err());
    assert!(manager.apply(&owner, "old-id", "commit").is_err());
    let staging = manager.route().unwrap();
    assert_eq!(
        manager.apply(&owner, "repair", "begin").unwrap().state,
        "staging"
    );
    assert_eq!(
        manager.apply(&owner, "repair", "commit").unwrap().state,
        "committed"
    );
    assert_eq!(
        manager.apply(&owner, "repair", "commit").unwrap().state,
        "committed"
    );
    assert_eq!(manager.route().unwrap(), staging);
    assert!(manager.write_guard(&old).is_err());
    assert!(manager.write_guard(&staging).is_ok());
    let other_process = IndexMaintenance::new(temp.path().to_owned());
    assert_eq!(other_process.route().unwrap(), staging);
}

#[test]
fn SessionIndex_PreservesNativeHistoryAndUserMetadata_004() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let names = [
        "rollout.jsonl",
        "rollout.jsonl.zst",
        "session_index.jsonl",
        "projects.json",
        "config.json",
        "cli-workspace.v1.json",
        "session-name-index.json",
        "native-history-metadata.json",
    ];
    for name in names {
        std::fs::write(temp.path().join(name), name.as_bytes()).unwrap();
    }
    manager.apply(&caller("1"), "repair", "begin").unwrap();
    manager.apply(&caller("1"), "repair", "commit").unwrap();
    for name in names {
        assert_eq!(
            std::fs::read(temp.path().join(name)).unwrap(),
            name.as_bytes()
        );
    }
}

#[test]
fn SessionIndex_CommitRechecksDocumentAndKeepsOriginal_005() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    manager.apply(&owner, "repair", "begin").unwrap();
    let checks = std::cell::Cell::new(0);
    assert!(manager
        .apply_checked(&owner, "repair", "commit", || {
            checks.set(checks.get() + 1);
            if checks.get() > 1 {
                Err(error("FORBIDDEN"))
            } else {
                Ok(())
            }
        })
        .is_err());
    assert!(!temp.path().join("session-index-generation.json").exists());
    manager.release_revoked(|_| false);
    assert_eq!(
        manager
            .apply(&caller("2"), "new-document", "begin")
            .unwrap()
            .state,
        "staging"
    );
}

#[test]
fn SessionIndex_TransactionLockExcludesOtherProcess_006() {
    const ROOT: &str = "CC_DESK_INDEX_TEST_CHILD_ROOT";
    if let Some(root) = std::env::var_os(ROOT) {
        let manager = IndexMaintenance::new(PathBuf::from(root));
        assert!(manager.write_guard(&manager.route().unwrap()).is_err());
        assert!(manager.apply(&caller("1"), "child", "begin").is_err());
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    manager.apply(&caller("1"), "parent", "begin").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "session_index_maintenance::tests::SessionIndex_TransactionLockExcludesOtherProcess_006", "--nocapture"])
        .env(ROOT, temp.path()).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn SessionIndex_CancelThenCommitUsesFreshDirectory_007() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    manager.apply(&owner, "first", "begin").unwrap();
    let cancelled = manager.route().unwrap();
    std::fs::write(cancelled.path("native-history-metadata.json"), b"cancelled").unwrap();
    manager.apply(&owner, "first", "cancel").unwrap();
    manager.apply(&owner, "second", "begin").unwrap();
    let committed = manager.route().unwrap();
    assert_ne!(cancelled, committed);
    std::fs::write(committed.path("native-history-metadata.json"), b"committed").unwrap();
    manager.apply(&owner, "second", "commit").unwrap();
    manager.apply(&owner, "third", "begin").unwrap();
    assert_ne!(manager.route().unwrap(), committed);
    manager.apply(&owner, "third", "cancel").unwrap();
    assert_eq!(manager.route().unwrap(), committed);
    assert_eq!(
        std::fs::read(committed.path("native-history-metadata.json")).unwrap(),
        b"committed"
    );
    assert_eq!(
        std::fs::read(cancelled.path("native-history-metadata.json")).unwrap(),
        b"cancelled"
    );
}

#[test]
fn SessionIndex_ManifestWriteFailureKeepsOldIndexes_008() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    for name in ["session-name-index.json", "native-history-metadata.json"] {
        std::fs::write(temp.path().join(name), b"original").unwrap();
    }
    let original = manager.route().unwrap();
    manager.apply(&owner, "repair", "begin").unwrap();
    // 用真实文件系统冲突阻止 manifest 临时文件写入，不能冒充发布成功。
    std::fs::create_dir(temp.path().join("session-index-generation.json.tmp")).unwrap();
    assert_eq!(
        manager
            .apply(&owner, "repair", "commit")
            .err()
            .unwrap()
            .code,
        "MAINTENANCE_UNAVAILABLE"
    );
    assert_eq!(
        manager.apply(&owner, "repair", "status").unwrap().state,
        "staging"
    );
    manager.apply(&owner, "repair", "cancel").unwrap();
    assert_eq!(
        manager.apply(&owner, "repair", "status").unwrap().state,
        "cancelled"
    );
    assert_eq!(manager.route().unwrap(), original);
    for name in ["session-name-index.json", "native-history-metadata.json"] {
        assert_eq!(std::fs::read(temp.path().join(name)).unwrap(), b"original");
    }
    assert_eq!(
        manager.apply(&owner, "next", "begin").unwrap().state,
        "staging"
    );
}

#[test]
fn SessionIndex_ProductionReadersRouteAndFenceBothCaches_009() {
    const CHILD: &str = "CC_DESK_INDEX_TEST_ROUTING_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session_index_maintenance::tests::SessionIndex_ProductionReadersRouteAndFenceBothCaches_009", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use crate::cli::native_projection::cache;
    use crate::session_name_index::{FileStamp, SessionNameIndexStore, SessionNameResolver};
    let manager = production();
    let owner = caller("1");
    let source = tempfile::tempdir().unwrap();
    let transcript = source.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        b"{\"type\":\"user\",\"message\":{\"content\":\"Original title\"}}\n",
    )
    .unwrap();
    let old = manager.route().unwrap();
    let store = SessionNameIndexStore::production().unwrap();
    let mut resolver = SessionNameResolver::new(store.read_snapshot(), 1);
    resolver.resolve(
        source.path(),
        &transcript,
        FileStamp::read(&transcript).unwrap(),
    );
    let late_old = resolver.finish().unwrap();
    store.flush_pending(late_old.clone()).unwrap();
    let stamps = cache::Stamps {
        len: 1,
        secs: 1,
        nanos: 0,
    };
    let metadata = |id: &str| cache::CachedMetadata {
        native_id: id.into(),
        cwd: None,
        title: id.into(),
        title_priority: Some(0),
        updated: None,
        metadata_incomplete: false,
    };
    cache::with_route(Some(&old), |cache| {
        cache.record("key".into(), stamps.clone(), metadata("original"));
        cache.flush().unwrap();
    })
    .unwrap();
    let old_native = std::fs::read(old.path("native-history-metadata.json")).unwrap();
    let old_legacy = std::fs::read(old.path("session-name-index.json")).unwrap();
    manager.apply(&owner, "cancelled", "begin").unwrap();
    let stage = manager.route().unwrap();
    let stage_store = SessionNameIndexStore::production().unwrap();
    assert!(stage_store.read_snapshot().index.projects.is_empty());
    assert!(stage_store.flush_pending(late_old.clone()).is_err());
    let mut stage_resolver = SessionNameResolver::new(stage_store.read_snapshot(), 2);
    stage_resolver.resolve(
        source.path(),
        &transcript,
        FileStamp::read(&transcript).unwrap(),
    );
    let late_stage = stage_resolver.finish().unwrap();
    stage_store.flush_pending(late_stage.clone()).unwrap();
    cache::with_route(Some(&stage), |cache| {
        cache.record("key".into(), stamps.clone(), metadata("staging"));
        cache.flush().unwrap();
    })
    .unwrap();
    manager.apply(&owner, "cancelled", "cancel").unwrap();
    assert!(stage_store.flush_pending(late_stage).is_err());
    assert_eq!(
        cache::with_route(Some(&old), |cache| cache
            .lookup("key", &stamps)
            .unwrap()
            .native_id
            .clone())
        .unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read(old.path("native-history-metadata.json")).unwrap(),
        old_native
    );
    assert_eq!(
        std::fs::read(old.path("session-name-index.json")).unwrap(),
        old_legacy
    );
    manager.apply(&owner, "committed", "begin").unwrap();
    manager.apply(&owner, "committed", "commit").unwrap();
    assert!(SessionNameIndexStore::production()
        .unwrap()
        .flush_pending(late_old)
        .is_err());
    assert!(cache::with_route(Some(&old), |_| ()).is_none());
}

#[test]
fn SessionIndex_CorruptManifestBypassesCacheAndKeepsBusiness_010() {
    const CHILD: &str = "CC_DESK_INDEX_TEST_CORRUPT_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session_index_maintenance::tests::SessionIndex_CorruptManifestBypassesCacheAndKeepsBusiness_010", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use crate::session_name_index::SessionNameIndexStore;
    let source = tempfile::tempdir().unwrap();
    let transcript = source.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        b"{\"type\":\"user\",\"message\":{\"content\":\"Actual disk title\"}}\n",
    )
    .unwrap();
    let route = production().route().unwrap();
    std::fs::create_dir_all(&route.root).unwrap();
    std::fs::write(route.path("session-name-index.json"), b"old cache bytes").unwrap();
    std::fs::write(
        route.root.join("session-index-generation.json"),
        b"{invalid",
    )
    .unwrap();
    let store = SessionNameIndexStore::production().unwrap();
    let result = crate::store::get_sessions_indexed_at(
        source.path().to_str().unwrap(),
        &[source.path().to_owned()],
        10,
        0,
        &store,
        1,
        None,
    )
    .unwrap();
    assert_eq!(result.value.len(), 1);
    assert_eq!(result.value[0].name, "Actual disk title");
    assert!(
        result.pending_flush.is_none(),
        "禁写快照不能产生可迟到的新代写回"
    );
    if let Some(pending) = result.pending_flush {
        store.flush_pending(pending).unwrap();
    }
    assert_eq!(
        std::fs::read(route.path("session-name-index.json")).unwrap(),
        b"old cache bytes"
    );
    assert_eq!(
        std::fs::read(route.root.join("session-index-generation.json")).unwrap(),
        b"{invalid"
    );
}

#[test]
fn SessionIndex_UnreadableLegacyNameFailsOnlyMaintenance_011() {
    const CHILD: &str = "CC_DESK_INDEX_TEST_READFAIL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session_index_maintenance::tests::SessionIndex_UnreadableLegacyNameFailsOnlyMaintenance_011", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use crate::session_name_index::SessionNameIndexStore;
    let source = tempfile::tempdir().unwrap();
    let transcript = source.path().join("session.jsonl");
    let owner = caller("1");
    for maintenance in [false, true] {
        std::fs::write(
            &transcript,
            b"{\"type\":\"user\",\"message\":{\"content\":\"Disk title\"}}\n",
        )
        .unwrap();
        if maintenance {
            production().apply(&owner, "repair", "begin").unwrap();
        }
        let remove = transcript.clone();
        let result = crate::store::get_sessions_indexed_at(
            source.path().to_str().unwrap(),
            &[source.path().to_owned()],
            10,
            0,
            &SessionNameIndexStore::production().unwrap(),
            1,
            Some(std::sync::Arc::new(move || {
                std::fs::remove_file(&remove).unwrap();
            })),
        );
        if maintenance {
            assert!(result.is_err());
            assert_eq!(
                production()
                    .apply(&owner, "repair", "commit")
                    .err()
                    .unwrap()
                    .code,
                "MAINTENANCE_UNAVAILABLE"
            );
            assert_eq!(
                production()
                    .apply(&owner, "repair", "status")
                    .unwrap()
                    .state,
                "staging"
            );
            production().apply(&owner, "repair", "cancel").unwrap();
        } else {
            assert_eq!(result.unwrap().value[0].name, "Unnamed session");
        }
    }
}

#[test]
fn SessionIndex_AbsentStatusDoesNotCreateOrCancel_012() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    assert_eq!(
        manager
            .apply(&owner, "never-began", "status")
            .unwrap()
            .state,
        "absent"
    );
    manager.apply(&owner, "active", "begin").unwrap();
    let active = manager.route().unwrap();
    assert_eq!(
        manager
            .apply(&owner, "begin-rejected", "status")
            .unwrap()
            .state,
        "absent"
    );
    assert!(manager.apply(&caller("2"), "active", "status").is_err());
    assert_eq!(manager.route().unwrap(), active);
}

#[test]
fn SessionIndex_UnchangedCorruptManifestCanReconcileAndCancel_013() {
    let temp = tempfile::tempdir().unwrap();
    let manager = IndexMaintenance::new(temp.path().to_owned());
    let owner = caller("1");
    std::fs::write(
        temp.path().join("session-index-generation.json"),
        b"{broken",
    )
    .unwrap();
    manager.apply(&owner, "repair", "begin").unwrap();
    assert_eq!(
        manager.apply(&owner, "repair", "status").unwrap().state,
        "staging"
    );
    manager.apply(&owner, "repair", "cancel").unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("session-index-generation.json")).unwrap(),
        b"{broken"
    );
    assert_eq!(
        manager.apply(&owner, "repair", "status").unwrap().state,
        "cancelled"
    );
    manager.apply(&owner, "again", "begin").unwrap();
    manager.apply(&owner, "again", "commit").unwrap();
    assert_eq!(
        manager.apply(&owner, "again", "status").unwrap().state,
        "committed"
    );
}

#[test]
fn SessionIndex_CancelAbsentFencesDelayedBeginAcrossOtherReceipts_014() {
    let temp = tempfile::tempdir().unwrap();
    let manager = std::sync::Arc::new(IndexMaintenance::new(temp.path().to_owned()));
    let owner = caller("1");
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let delayed = manager.clone();
    let delayed_owner = owner.clone();
    let begin = std::thread::spawn(move || {
        release_rx.recv().unwrap();
        delayed
            .apply(&delayed_owner, "delayed", "begin")
            .unwrap()
            .state
    });
    assert_eq!(
        manager.apply(&owner, "delayed", "status").unwrap().state,
        "absent"
    );
    assert_eq!(
        manager.apply(&owner, "delayed", "cancel").unwrap().state,
        "cancelled"
    );
    manager.apply(&owner, "other", "begin").unwrap();
    manager.apply(&owner, "other", "commit").unwrap();
    let published = manager.route().unwrap();
    release_tx.send(()).unwrap();
    assert_eq!(begin.join().unwrap(), "cancelled");
    assert_eq!(manager.route().unwrap(), published);
    assert!(manager.state.lock().active.is_none());
}
