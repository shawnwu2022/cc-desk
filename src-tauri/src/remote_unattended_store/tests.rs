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
        final_control: None,
        mode: None,
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
fn path(store: &TestStore, name: &str) -> std::path::PathBuf {
    Path::new(&store.backend.target_label().unwrap()).join(name)
}
fn raw(store: &TestStore) -> Vec<u8> {
    std::fs::read(path(store, "delegation.dpapi")).unwrap()
}
// 显式模拟可信本地存储被篡改；不冒充一次已批准事务。
fn replace_raw(store: &TestStore, bytes: &[u8]) {
    std::fs::write(path(store, "delegation.dpapi"), bytes).unwrap()
}
fn try_reopen(path: &Path) -> Result<TestStore> {
    Store::new(WindowsUnattendedBackend::open_existing(path)?, FakeSealer)
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
    for mode in 0..5 {
        let d = directory();
        let s = create(d.path());
        let snapshot = initialize(&s);
        let text = String::from_utf8(raw(&s)).unwrap();
        let bytes = match mode {
            0 => b"TEST_ONLY/{".to_vec(),
            1 => text
                .replace("\"schema\":1", "\"schema\":1,\"schema\":1")
                .into_bytes(),
            2 => text
                .replace("\"schema\":1", "\"schema\":1,\"unknown\":true")
                .into_bytes(),
            3 => text.replace(DOMAIN, "wrong-domain").into_bytes(),
            _ => vec![b'x'; MAX_PLAINTEXT + 1],
        };
        assert!(s.decode(&bytes).is_err());
        replace_raw(&s, &bytes);
        assert_eq!(
            s.observe(&snapshot.expected(), 1000),
            Err(StoreError::Unknown)
        );
    }
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
        Err(StoreError::Unknown)
    );
    assert_eq!(raw(&s), bytes);
    assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
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
    assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
}
#[test]
fn UnattendedStore_MissingNeverAutoInitializes_007() {
    let d = directory();
    assert!(WindowsUnattendedBackend::open_existing(d.path()).is_err());
    let s = create(d.path());
    assert_eq!(s.observe(&initial(), 1000), Err(StoreError::Missing));
    assert!(!path(&s, "delegation.dpapi").exists());
    drop(s);
    let reader = reopen(d.path());
    assert_eq!(reader.observe(&initial(), 1000), Err(StoreError::Unknown));
    let d2 = directory();
    let s = create(d2.path());
    let snapshot = initialize(&s);
    let file = path(&s, "delegation.dpapi");
    drop(s);
    std::fs::rename(&file, file.with_extension("held-test")).unwrap();
    assert_eq!(
        reopen(d2.path()).observe(&snapshot.expected(), 1000),
        Err(StoreError::Unknown)
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
    doc.authority_revision = doc.revision + 1;
    assert_eq!(validate(&doc), Err(StoreError::Invalid));
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
    assert_eq!(s.initialize(p), Err(StoreError::Unknown));
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
    let doc = s.decode(&bytes).unwrap();
    let mut next = doc.clone();
    next.revision += 1;
    let next_bytes = s.encode(&next).unwrap();
    let stamp = s.stamp(&next, &next_bytes).unwrap();
    let result = tx.commit(&next_bytes, true, &stamp, &mut || {
        calls += 1;
        if calls == 2 {
            Err(StoreError::Unauthorized)
        } else {
            Ok(())
        }
    });
    assert_eq!(result, Err(StoreError::Unknown));
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
    let old_journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    s.revoke(&enabled.expected(), 1100).unwrap();
    replace_raw(&s, &old);
    std::fs::write(path(&s, "commit.journal"), old_journal).unwrap();
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
    assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Unknown));
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
            Err(StoreError::Unknown)
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
        // 独立验证核心高水位，下层的匹配事务证据也不能允许倒退。
        let mut local = Local {
            last: Some(known.0.clone()),
            ..Local::default()
        };
        assert_eq!(
            check_known(&mut local, &regressed),
            Err(StoreError::Conflict)
        );
        assert!(local.blocked);
        replace_raw(&s, &s.encode(&regressed).unwrap());
        let regressed = LockedSnapshot(regressed);
        assert_eq!(
            s.observe(&regressed.expected(), 1500),
            Err(StoreError::Unknown)
        );
        assert_eq!(
            s.observe(&regressed.expected(), 1500),
            Err(StoreError::Poisoned)
        );
    }
}

fn child_check(parent: &Path, revision: u64, status: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "remote_unattended_store::tests::UnattendedStore_JournalRestartChild_025",
            "--nocapture",
        ])
        .env("CC_DESK_TEST_ONLY_JOURNAL_PARENT", parent)
        .env("CC_DESK_TEST_ONLY_JOURNAL_REVISION", revision.to_string())
        .env("CC_DESK_TEST_ONLY_JOURNAL_STATUS", status)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn UnattendedStore_JournalFaultsSurviveNewBackendAndProcess_018() {
    for fault in [1, 2, 3, 4, 5, 6, 8, 9] {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        s.backend.test_journal_fault(fault);
        assert_eq!(
            s.enable(permit(&s, old.expected(), 1000, 2000), 1000),
            Err(StoreError::Unknown)
        );
        assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
        drop(s);
        child_check(
            d.path(),
            if fault == 8 { 1 } else { 2 },
            match fault {
                5 | 6 => "Enabled",
                8 => "Disabled",
                _ => "Unknown",
            },
        );
    }
}
#[test]
fn UnattendedStore_JournalInitializationPartialFailure_019() {
    for fault in [1, 2, 3, 4, 5, 6, 8, 9] {
        let d = directory();
        let s = create(d.path());
        s.backend.test_journal_fault(fault);
        assert_eq!(
            s.initialize(permit(&s, initial(), 1000, 1000)),
            Err(StoreError::Unknown)
        );
        assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
        drop(s);
        child_check(
            d.path(),
            1,
            if [5, 6].contains(&fault) {
                "Disabled"
            } else {
                "Unknown"
            },
        );
    }
}
#[test]
fn UnattendedStore_JournalStrictChainRejectsMalformed_020() {
    use journal::{Chain, Record};
    for mode in 0..10 {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        let bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
        let chain = Chain::parse(&bytes, false).unwrap();
        let clean = chain.last.as_ref().unwrap();
        let mut pending = Record {
            clean: false,
            sequence: 1,
            transaction: clean.transaction,
            stamp: clean.stamp.clone(),
            previous: [0; 32],
        };
        let mut second = Record {
            clean: true,
            sequence: 2,
            transaction: pending.transaction,
            stamp: pending.stamp.clone(),
            previous: Sha256::digest(pending.bytes()).into(),
        };
        let corrupt = match mode {
            0 => vec![],
            1 => bytes[..100].to_vec(),
            2 => [bytes.as_slice(), b"tail"].concat(),
            3 => {
                pending.clean = true;
                pending.bytes().to_vec()
            }
            4 => {
                second.clean = false;
                [pending.bytes().as_slice(), second.bytes().as_slice()].concat()
            }
            5 => {
                second.stamp.store_id[0] ^= 1;
                [pending.bytes().as_slice(), second.bytes().as_slice()].concat()
            }
            6 => {
                second.sequence = 3;
                [pending.bytes().as_slice(), second.bytes().as_slice()].concat()
            }
            7 => {
                second.previous[0] ^= 1;
                [pending.bytes().as_slice(), second.bytes().as_slice()].concat()
            }
            8 => {
                pending.sequence = 3;
                pending.previous = chain.hash;
                [bytes.as_slice(), pending.bytes().as_slice()].concat()
            }
            _ => {
                let mut bytes = bytes;
                bytes[0] ^= 1;
                bytes
            }
        };
        assert!(Chain::parse(&corrupt, false).is_err());
        std::fs::write(path(&s, "commit.journal"), corrupt).unwrap();
        assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Unknown));
        assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
    }
}
#[test]
fn UnattendedStore_JournalCapacityReservedBeforePending_021() {
    use journal::{Chain, Record, MAX_RECORDS};
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let mut doc = old.0;
    let mut bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
    let mut chain = Chain::parse(&bytes, false).unwrap();
    let mut blob = raw(&s);
    for _ in 1..MAX_RECORDS / 2 {
        doc.revision += 1;
        blob = s.encode(&doc).unwrap();
        let stamp = s.stamp(&doc, &blob).unwrap();
        let pending = chain.pending(&stamp).unwrap();
        let clean = Record {
            clean: true,
            sequence: pending.sequence + 1,
            transaction: pending.transaction,
            stamp,
            previous: Sha256::digest(pending.bytes()).into(),
        };
        bytes.extend_from_slice(&pending.bytes());
        bytes.extend_from_slice(&clean.bytes());
        chain.records += 2;
        chain.hash = Sha256::digest(clean.bytes()).into();
        chain.last = Some(clean);
    }
    std::fs::write(path(&s, "commit.journal"), &bytes).unwrap();
    replace_raw(&s, &blob);
    let snapshot = LockedSnapshot(doc);
    assert!(s.observe(&snapshot.expected(), 1000).is_ok());
    assert_eq!(
        s.enable(permit(&s, snapshot.expected(), 1000, 2000), 1000),
        Err(StoreError::Capacity)
    );
    assert_eq!(raw(&s), blob);
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), bytes);
}
#[test]
fn UnattendedStore_JournalIdentityAndMissingReject_022() {
    for missing in [true, false] {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        let log = path(&s, "commit.journal");
        let bytes = std::fs::read(&log).unwrap();
        std::fs::rename(&log, log.with_extension("held-test")).unwrap();
        if !missing {
            std::fs::write(&log, bytes).unwrap()
        }
        assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Unknown));
        assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
    }
}
#[test]
fn UnattendedStore_CancelBeforePendingDoesNotMutateEvidence_023() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
    let p = permit(&s, old.expected(), 1000, 2000);
    p.cancelled.store(true, Ordering::Release);
    assert_eq!(s.enable(p, 1000), Err(StoreError::Unauthorized));
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), bytes);
    assert_eq!(s.observe(&old.expected(), 1000).unwrap(), old);
}
#[test]
fn UnattendedStore_BlobPublicationFailureLeavesPending_024() {
    for after in [true, false] {
        let d = directory();
        let s = create(d.path());
        let old = initialize(&s);
        if after {
            s.backend.test_fail_after_publish()
        } else {
            s.backend.test_fail_before_publish()
        }
        assert_eq!(
            s.enable(permit(&s, old.expected(), 1000, 2000), 1000),
            Err(StoreError::Unknown)
        );
        drop(s);
        child_check(d.path(), 2, "Unknown");
    }
}
#[test]
fn UnattendedStore_JournalRestartChild_025() {
    let Some(parent) = std::env::var_os("CC_DESK_TEST_ONLY_JOURNAL_PARENT") else {
        return;
    };
    let revision = std::env::var("CC_DESK_TEST_ONLY_JOURNAL_REVISION")
        .unwrap()
        .parse()
        .unwrap();
    let status = std::env::var("CC_DESK_TEST_ONLY_JOURNAL_STATUS").unwrap();
    let result = try_reopen(Path::new(&parent)).and_then(|s| {
        s.observe(
            &Expected {
                revision,
                ..initial()
            },
            1000,
        )
    });
    if status == "Unknown" {
        assert!(result.is_err())
    } else {
        let snapshot = result.unwrap();
        assert_eq!(format!("{:?}", snapshot.0.status), status)
    }
}

#[test]
fn UnattendedStore_JournalRejectsNonAdjacentTransactionReuse_026() {
    use journal::{Chain, Record};
    let d = directory();
    let s = create(d.path());
    initialize(&s);
    let mut bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
    let first = Chain::parse(&bytes, false).unwrap().last.unwrap();
    for revision in [2, 3] {
        let chain = Chain::parse(&bytes, false).unwrap();
        let mut stamp = first.stamp.clone();
        stamp.revision = revision;
        let pending = Record {
            clean: false,
            sequence: chain.records as u64 + 1,
            transaction: if revision == 3 {
                first.transaction
            } else {
                [8; 16]
            },
            stamp: stamp.clone(),
            previous: chain.hash,
        };
        let clean = Record {
            clean: true,
            sequence: pending.sequence + 1,
            transaction: pending.transaction,
            stamp,
            previous: Sha256::digest(pending.bytes()).into(),
        };
        bytes.extend_from_slice(&pending.bytes());
        bytes.extend_from_slice(&clean.bytes());
    }
    assert!(Chain::parse(&bytes, false).is_err());
}

#[test]
fn UnattendedStore_KnownUnknownFencedAfterLeaseAcquisition_027() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let b = reopen(d.path());
    let expected = old.expected();
    let (entered, seen) = std::sync::mpsc::sync_channel(0);
    let (release, resume) = std::sync::mpsc::sync_channel(0);
    let reader = thread::spawn(move || {
        crate::version_history::windows::unattended_store::test_before_control(move || {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        });
        b.observe(&expected, 1000)
    });
    seen.recv().unwrap();
    s.backend.test_journal_fault(5);
    assert_eq!(
        s.enable(permit(&s, old.expected(), 1000, 2000), 1000),
        Err(StoreError::Unknown)
    );
    release.send(()).unwrap();
    assert_eq!(reader.join().unwrap(), Err(StoreError::Poisoned));
}

#[test]
fn UnattendedStore_CompletedTransactionCannotCommitTwice_028() {
    let d = directory();
    let s = create(d.path());
    let old = initialize(&s);
    let mut tx = s.backend.begin().unwrap();
    tx.read().unwrap();
    let mut next = old.0;
    next.revision += 1;
    let bytes = s.encode(&next).unwrap();
    let stamp = s.stamp(&next, &bytes).unwrap();
    tx.commit(&bytes, true, &stamp, &mut || Ok(())).unwrap();
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    assert_eq!(
        tx.commit(&bytes, true, &stamp, &mut || Ok(())),
        Err(StoreError::Invalid)
    );
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}

#[test]
fn UnattendedStore_ReopenBeforeInitializeNeverResumesCreation_029() {
    let d = directory();
    drop(create(d.path()));
    let s = reopen(d.path());
    assert_eq!(
        s.initialize(permit(&s, initial(), 1000, 1000)),
        Err(StoreError::Unknown)
    );
    assert!(!path(&s, "delegation.dpapi").exists());
    assert!(matches!(try_reopen(d.path()), Err(StoreError::Poisoned)));
}

#[path = "custody_tests.rs"]
mod custody;

#[path = "capacity_tests.rs"]
mod capacity;

#[path = "single_run_tests.rs"]
mod single_run_tests;
#[path = "checkpoint_tests.rs"]
mod checkpoint;
