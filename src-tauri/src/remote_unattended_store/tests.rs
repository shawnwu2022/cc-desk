#![allow(non_snake_case)]
use super::*;
use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
use std::{path::Path, sync::Barrier, thread, time::Duration};
// 只测试公开固定字节，绝不调用真实 DPAPI 或用户密钥。
struct FakeSealer;
impl Sealer for FakeSealer {
    fn seal(&self, b: &[u8]) -> Result<Vec<u8>> {
        Ok([b"TEST_ONLY/".as_slice(), b].concat())
    }
    fn unseal(&self, b: &[u8]) -> Result<Vec<u8>> {
        b.strip_prefix(b"TEST_ONLY/")
            .map(|v| v.to_vec())
            .ok_or(StoreError::Invalid)
    }
}
type TestStore = Store<WindowsUnattendedBackend, FakeSealer>;
fn directory() -> tempfile::TempDir {
    // 仅规范化测试构造路径；生产 NTFS 校验仍拒绝斜杠、UNC 与重解析路径。
    let manifest = env!("CARGO_MANIFEST_DIR").replace('/', "\\");
    let root = Path::new(&manifest)
        .parent()
        .unwrap()
        .join(".audit")
        .join("unattended-store-tests");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("synthetic-")
        .tempdir_in(root)
        .unwrap()
}
fn target() -> Target {
    Target {
        installation: [1; 32],
        device: [2; 32],
        target: [3; 32],
        binding_fingerprint: [4; 32],
    }
}
fn create(path: &Path) -> TestStore {
    Store::new(
        WindowsUnattendedBackend::create_new(path).unwrap(),
        FakeSealer,
    )
    .unwrap()
}
fn reopen(path: &Path) -> TestStore {
    Store::new(
        WindowsUnattendedBackend::open_existing(path).unwrap(),
        FakeSealer,
    )
    .unwrap()
}
fn initial() -> Expected {
    Expected {
        store_id: [1; 16],
        revision: 0,
        target: target(),
    }
}
fn permit(store: &TestStore, expected: Expected, issued_ms: u64, cutoff_ms: u64) -> NativePermit {
    NativePermit {
        instance: store.instance.clone(),
        expected,
        issued_ms,
        cutoff_ms,
        cancelled: Arc::new(AtomicBool::new(false)),
        deadline: Instant::now() + Duration::from_secs(60),
    }
}
fn initialize(store: &TestStore) -> LockedSnapshot {
    store
        .initialize(permit(store, initial(), 1000, 1000))
        .unwrap()
}
fn enable(store: &TestStore, s: &LockedSnapshot) -> LockedSnapshot {
    store
        .enable(permit(store, s.expected(), 1000, 2000), 1000)
        .unwrap()
}
fn raw(store: &TestStore) -> Vec<u8> {
    store.backend.begin().unwrap().read().unwrap().unwrap()
}
fn replace_raw(store: &TestStore, bytes: &[u8]) {
    let mut tx = store.backend.begin().unwrap();
    assert!(tx.read().unwrap().is_some());
    tx.commit(bytes, true, &mut || Ok(())).unwrap();
}
#[test]
fn UnattendedStore_RestartRevokedRemainsLocked_001() {
    let d = directory();
    let s = create(d.path());
    let disabled = initialize(&s);
    let enabled = enable(&s, &disabled);
    let revoked = s.revoke(&enabled.expected(), 1100).unwrap();
    assert_eq!(revoked.0.status, Status::Revoked);
    drop(s);
    let next = reopen(d.path());
    let loaded = next.observe(&revoked.expected(), 1100).unwrap();
    assert_eq!(loaded, revoked);
    assert_eq!(loaded.0.cutoff_ms, 2000);
}
#[test]
fn UnattendedStore_ExactCASAndNativePermit_002() {
    let d = directory();
    let s = create(d.path());
    let snapshot = initialize(&s);
    let other = reopen(d.path());
    let wrong = permit(&other, snapshot.expected(), 1000, 2000);
    assert_eq!(s.enable(wrong, 1000), Err(StoreError::Unauthorized));
    for field in 0..6 {
        let mut expected = snapshot.expected();
        match field {
            0 => expected.store_id[0] ^= 1,
            1 => expected.revision += 1,
            2 => expected.target.device[0] ^= 1,
            3 => expected.target.target[0] ^= 1,
            4 => expected.target.binding_fingerprint[0] ^= 1,
            _ => expected.target.installation[0] ^= 1,
        }
        let reader = reopen(d.path());
        assert_eq!(reader.observe(&expected, 1000), Err(StoreError::Conflict));
        assert_eq!(reader.revoke(&expected, 1000), Err(StoreError::Conflict));
    }
    assert_eq!(raw(&s), raw(&other));
}
#[test]
fn UnattendedStore_CompetingWritersSerialize_003() {
    let d = directory();
    let s = create(d.path());
    drop(s);
    let s = reopen(d.path());
    let snapshot = initialize(&s);
    drop(s);
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = reopen(d.path());
        let expected = snapshot.expected();
        let gate = barrier.clone();
        workers.push(thread::spawn(move || {
            let p = permit(&store, expected, 1000, 2000);
            gate.wait();
            store.enable(p, 1000)
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict | StoreError::Busy)))
            .count(),
        1
    );
}
#[test]
fn UnattendedStore_MalformedDuplicateAndBounds_004() {
    let d = directory();
    let s = create(d.path());
    let snapshot = initialize(&s);
    let original = raw(&s);
    let text = String::from_utf8(original.clone()).unwrap();
    let cases = vec![
        b"TEST_ONLY/{".to_vec(),
        text.replace("\"schema\":1", "\"schema\":1,\"schema\":1")
            .into_bytes(),
        text.replace("\"schema\":1", "\"schema\":1,\"unknown\":true")
            .into_bytes(),
        text.replace(DOMAIN, "cc-desk/native-public-pairing/v1")
            .into_bytes(),
        vec![b'x'; MAX_PLAINTEXT + 1],
    ];
    for bytes in cases {
        replace_raw(&s, &bytes);
        let reader = reopen(d.path());
        assert_eq!(
            reader.observe(&snapshot.expected(), 1000),
            Err(StoreError::Invalid)
        );
    }
    replace_raw(&s, &original);
    assert!(s.decode(&vec![0; MAX_SEALED + 1]).is_err());
}
#[test]
fn UnattendedStore_PrePublishFailureKeepsOldRecord_005() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let bytes = raw(&s);
    s.backend.test_fail_before_publish();
    assert_eq!(
        s.enable(permit(&s, old.expected(), 1000, 2000), 1000),
        Err(StoreError::Io)
    );
    assert_eq!(raw(&s), bytes);
    assert_eq!(
        reopen(d.path()).observe(&old.expected(), 1000).unwrap(),
        old
    );
}
#[test]
fn UnattendedStore_PostPublishUncertainPoisons_006() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let enabled = enable(&s, &old);
    s.backend.test_fail_after_publish();
    assert_eq!(
        s.revoke(&enabled.expected(), 1100),
        Err(StoreError::Unknown)
    );
    assert_eq!(
        s.observe(&enabled.expected(), 1100),
        Err(StoreError::Poisoned)
    );
    let mut committed = enabled.expected();
    committed.revision += 1;
    let loaded = reopen(d.path()).observe(&committed, 1100).unwrap();
    assert_eq!(loaded.0.status, Status::Revoked);
}
#[test]
fn UnattendedStore_MissingNeverAutoInitializes_007() {
    let d = directory();
    assert!(WindowsUnattendedBackend::open_existing(d.path()).is_err());
    let s = create(d.path());
    assert_eq!(s.observe(&initial(), 1000), Err(StoreError::Missing));
    assert!(!d
        .path()
        .join("remote-unattended-v1/delegation.dpapi")
        .exists());
    drop(s);
    let s = reopen(d.path());
    let snapshot = initialize(&s);
    drop(s);
    std::fs::rename(
        d.path().join("remote-unattended-v1/delegation.dpapi"),
        d.path().join("remote-unattended-v1/held-test.dpapi"),
    )
    .unwrap();
    assert_eq!(
        reopen(d.path()).observe(&snapshot.expected(), 1000),
        Err(StoreError::Missing)
    );
}
#[test]
fn UnattendedStore_ClockFloorAndAbsoluteEndSurviveRestart_008() {
    for observed in [1499, 2000] {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        let enabled = enable(&s, &old);
        let advanced = s.observe(&enabled.expected(), 1500).unwrap();
        assert_eq!(advanced.0.cutoff_ms, 2000);
        drop(s);
        let reader = reopen(d.path());
        assert_eq!(
            reader.observe(&advanced.expected(), observed),
            Err(if observed == 1499 {
                StoreError::Clock
            } else {
                StoreError::Expired
            })
        );
        assert_eq!(
            reader.observe(&advanced.expected(), 1500),
            Err(StoreError::Poisoned)
        );
        let mut revoked = advanced.expected();
        revoked.revision += 1;
        let floor = observed.max(1500);
        let loaded = reopen(d.path()).observe(&revoked, floor).unwrap();
        assert_eq!(loaded.0.status, Status::Revoked);
        assert_eq!(loaded.0.observed_wall_ms, floor);
        // 已观察过期并落盘后，重启回拨到旧截止之内也不会恢复 Enabled。
        if observed == 2000 {
            assert_eq!(
                reopen(d.path()).observe(&revoked, 1600),
                Err(StoreError::Clock)
            );
        }
    }
}
#[test]
fn UnattendedStore_CutoffRevisionAndSchemaBounds_009() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    for end in [999, 1000, 1000 + MAX_MS + 1] {
        assert_eq!(
            s.enable(permit(&s, old.expected(), 1000, end), 1000),
            Err(StoreError::Invalid)
        );
    }
    let mut doc = old.0.clone();
    doc.revision = u64::MAX;
    replace_raw(&s, &s.encode(&doc).unwrap());
    let max = LockedSnapshot(doc);
    assert_eq!(
        s.enable(permit(&s, max.expected(), 1000, 2000), 1000),
        Err(StoreError::Invalid)
    );
}
#[test]
fn UnattendedStore_NativeAuthorityFenceAtRename_010() {
    let d = directory();
    let s = create(d.path());
    let p = permit(&s, initial(), 1000, 1000);
    let cancel = p.cancelled.clone();
    let _probe = crate::version_history::windows::files::probe_before_rename(move || {
        cancel.store(true, Ordering::Release)
    });
    assert_eq!(s.initialize(p), Err(StoreError::Unauthorized));
    assert!(!d
        .path()
        .join("remote-unattended-v1/delegation.dpapi")
        .exists());
}
#[test]
fn UnattendedStore_ReplaceChecksAuthorityAgain_011() {
    let d = directory();
    let s = create(d.path());
    initialize(&s);
    let bytes = raw(&s);
    let mut tx = s.backend.begin().unwrap();
    tx.read().unwrap();
    let mut calls = 0;
    let result = tx.commit(&bytes, true, &mut || {
        calls += 1;
        if calls == 2 {
            Err(StoreError::Unauthorized)
        } else {
            Ok(())
        }
    });
    assert_eq!(result, Err(StoreError::Unauthorized));
    assert_eq!(calls, 2);
    drop(tx);
    assert_eq!(raw(&s), bytes);
}
#[test]
fn UnattendedStore_WholeStoreRollbackNotClaimedProtected_012() {
    let d = directory();
    let s = create(d.path());
    let disabled = initialize(&s);
    let enabled = enable(&s, &disabled);
    let old = raw(&s);
    s.revoke(&enabled.expected(), 1100).unwrap();
    replace_raw(&s, &old);
    // 本实例保留撤销闭锁；新实例无可信外部单调存储，旧完整 blob 无法自行识别。
    assert_eq!(
        s.observe(&enabled.expected(), 1100),
        Err(StoreError::Poisoned)
    );
    let restored = reopen(d.path()).observe(&enabled.expected(), 1000).unwrap();
    assert_eq!(restored.0.status, Status::Enabled);
    // 结果仅为 LockedSnapshot，不存在转换为原生批准或运行 owner 的 API。
}
#[test]
fn UnattendedStore_TestBuildCannotUseRealDpapi_013() {
    assert_eq!(
        dpapi::CurrentUserDpapi.seal(b"TEST_ONLY"),
        Err(StoreError::Unauthorized)
    );
}

#[test]
fn UnattendedStore_RepairedCorruptionDoesNotReopenSameInstance_014() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let bytes = raw(&s);
    replace_raw(&s, b"TEST_ONLY/{");
    assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Invalid));
    replace_raw(&s, &bytes);
    assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Poisoned));
}
#[test]
fn UnattendedStore_ExpiryPublicationFailureStaysBlocked_015() {
    for uncertain in [false, true] {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        let enabled = enable(&s, &old);
        if uncertain {
            s.backend.test_fail_after_publish()
        } else {
            s.backend.test_fail_before_publish()
        }
        assert_eq!(
            s.observe(&enabled.expected(), 2000),
            Err(if uncertain {
                StoreError::Unknown
            } else {
                StoreError::Io
            })
        );
        assert_eq!(
            s.observe(&enabled.expected(), 1500),
            Err(StoreError::Poisoned)
        );
    }
}

#[test]
fn UnattendedStore_WallObservationDoesNotChangeAuthorityRevision_016() {
    let d = directory();
    let s = create(d.path());
    let disabled = initialize(&s);
    assert_eq!(disabled.0.authority_revision, 1);
    let enabled = enable(&s, &disabled);
    assert_eq!(enabled.0.authority_revision, 2);
    let observed = s.observe(&enabled.expected(), 1100).unwrap();
    assert_eq!(observed.0.revision, enabled.0.revision + 1);
    assert_eq!(observed.0.authority_revision, enabled.0.authority_revision);
    let revoked = s.revoke(&observed.expected(), 1200).unwrap();
    assert_eq!(
        revoked.0.authority_revision,
        observed.0.authority_revision + 1
    );
    assert_eq!(revoked.0.revision, observed.0.revision + 1);
}

#[test]
fn UnattendedStore_HigherStorageRevisionCannotLowerKnownFloors_017() {
    for lower_authority in [true, false] {
        let d = directory();
        let s = create(d.path());
        let disabled = initialize(&s);
        let enabled = enable(&s, &disabled);
        let known = s.observe(&enabled.expected(), 1500).unwrap();
        let mut regressed = known.0.clone();
        regressed.revision += 1;
        if lower_authority {
            regressed.authority_revision -= 1
        } else {
            regressed.observed_wall_ms -= 1
        }
        replace_raw(&s, &s.encode(&regressed).unwrap());
        let regressed = LockedSnapshot(regressed);
        assert_eq!(
            s.observe(&regressed.expected(), 1500),
            Err(StoreError::Conflict)
        );
        assert_eq!(
            s.observe(&regressed.expected(), 1500),
            Err(StoreError::Poisoned)
        );
    }
}
