//! 隔离 NTFS journal；不覆盖用户历史或更改真实授权。
use super::*;

#[test]
fn UnattendedCheckpoint_RealCapacityReclaimPreservesStateAndContinues_001() {
    let (directory, store, old) = capacity::near_capacity(true, 1);
    let custody = store.backend.claim_owner_custody().unwrap();
    let original_blob = raw(&store);
    let journal_path = path(&store, "commit.journal");
    let original_journal = std::fs::read(&journal_path).unwrap();
    let result = store.checkpoint(&old.expected(), &custody, &mut || Ok(()));
    if result.is_err() {
        assert_eq!(raw(&store), original_blob);
        assert_eq!(std::fs::read(&journal_path).unwrap(), original_journal);
    }
    let compacted = result.expect("verified near-full journal needs a safe physical checkpoint");
    assert_eq!(compacted, old);
    assert_eq!(raw(&store), original_blob);
    let journal = std::fs::read(&journal_path).unwrap();
    assert!(journal.len() < original_journal.len());
    assert!(
        journal::Chain::parse(&journal, false).is_err(),
        "legacy reader must reject checkpoint format"
    );
    custody.check(&store.backend).unwrap();
    let repeated = store
        .checkpoint(&compacted.expected(), &custody, &mut || Ok(()))
        .unwrap();
    assert_eq!(repeated, old);
    assert_eq!(raw(&store), original_blob);
    let next = store.observe(&compacted.expected(), 1100).unwrap();
    assert_eq!(next.0.revision, old.0.revision + 1);
    assert_eq!(next.0.authority_revision, old.0.authority_revision);
    assert_eq!(next.0.observed_wall_ms, 1100);
    let next_blob = raw(&store);
    let again = store
        .checkpoint(&next.expected(), &custody, &mut || Ok(()))
        .unwrap();
    assert_eq!(again, next);
    assert_eq!(raw(&store), next_blob);
    let bytes = std::fs::read(&journal_path).unwrap();
    let size = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let capsule: serde_json::Value =
        serde_json::from_slice(&FakeSealer.unseal(&bytes[64..64 + size]).unwrap()).unwrap();
    assert_eq!(capsule["segment"], 3);
    assert_eq!(capsule["sequence"], next.0.revision * 2);
    let revoked = store.revoke(&next.expected(), 1101).unwrap();
    assert_eq!(revoked.0.status, Status::Revoked);
    drop(custody);
    let fresh = reopen(directory.path());
    assert_eq!(fresh.observe(&revoked.expected(), 1101).unwrap(), revoked);
}

// 仅原 Child 句柄超时终止；子进程独立 registry，直接走 Store 认证读取。
struct ReopenProbe(std::process::Child);
impl Drop for ReopenProbe {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn probe_reopen(root: &Path, complete: bool) {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "remote_unattended_store::tests::checkpoint::UnattendedCheckpoint_ReopenHelper_900",
            "--nocapture",
        ])
        .env("CC_DESK_CHECKPOINT_TEST_ROOT", root)
        .env(
            "CC_DESK_CHECKPOINT_TEST_COMPLETE",
            if complete { "yes" } else { "no" },
        )
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ReopenProbe(child);
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "fresh-process checkpoint classification failed"
            );
            return;
        }
        assert!(
            Instant::now() < end,
            "fresh-process checkpoint probe timed out"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "dedicated isolated checkpoint reopen process"]
fn UnattendedCheckpoint_ReopenHelper_900() {
    let root = std::env::var_os("CC_DESK_CHECKPOINT_TEST_ROOT").unwrap();
    let complete = std::env::var("CC_DESK_CHECKPOINT_TEST_COMPLETE").unwrap() == "yes";
    let s = reopen(Path::new(&root));
    let original_blob = raw(&s);
    let expected = s.decode(&original_blob).unwrap();
    let mut local = s.lock().unwrap();
    let mut tx = s.backend.begin().unwrap();
    let read = s.read(&mut tx, &mut local);
    if complete {
        assert_eq!(read.unwrap(), expected);
    } else {
        assert!(
            read.is_err(),
            "partial checkpoint must not fall back to old tail"
        );
    }
    assert_eq!(raw(&s), original_blob);
}
#[test]
fn UnattendedCheckpoint_ActualWriteCutsPoisonAndFreshReaderFailsClosed_002() {
    for fault in 21..=29 {
        let (d, s, old) = capacity::near_capacity(true, 1);
        let custody = s.backend.claim_owner_custody().unwrap();
        let blob = raw(&s);
        s.backend.test_journal_fault(fault);
        assert_eq!(
            s.checkpoint(&old.expected(), &custody, &mut || Ok(())),
            Err(StoreError::Unknown),
            "fault {fault}"
        );
        assert!(s.observe(&old.expected(), 1000).is_err());
        assert!(
            s.backend.begin().is_err(),
            "poison must prevent append rescue"
        );
        assert_eq!(raw(&s), blob);
        drop(custody);
        drop(s);
        // 最后完整头已落盘的两个读/确认故障可读完整新格式，但原实例仍 Unknown。
        probe_reopen(d.path(), fault >= 27);
    }
}

#[test]
fn UnattendedCheckpoint_CancelBeforeWriteAndBeforeFinalHeader_003() {
    for at in [1, 2, 3] {
        let (d, s, old) = capacity::near_capacity(true, 1);
        let custody = s.backend.claim_owner_custody().unwrap();
        let blob = raw(&s);
        let before = std::fs::read(path(&s, "commit.journal")).unwrap();
        let mut calls = 0;
        let result = s.checkpoint(&old.expected(), &custody, &mut || {
            calls += 1;
            if calls == at {
                Err(StoreError::Unauthorized)
            } else {
                Ok(())
            }
        });
        assert_eq!(calls, at);
        assert_eq!(raw(&s), blob);
        if at < 3 {
            assert_eq!(result, Err(StoreError::Unauthorized));
            assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), before);
        } else {
            assert_eq!(result, Err(StoreError::Unknown));
            assert!(s.backend.begin().is_err());
            drop(custody);
            drop(s);
            probe_reopen(d.path(), false);
        }
    }
}
#[test]
fn UnattendedCheckpoint_TamperedCapsuleAndOldTailRejected_004() {
    for mode in 0..3 {
        let (d, s, old) = capacity::near_capacity(true, 1);
        let custody = s.backend.claim_owner_custody().unwrap();
        let journal_path = path(&s, "commit.journal");
        let old_journal = std::fs::read(&journal_path).unwrap();
        s.checkpoint(&old.expected(), &custody, &mut || Ok(()))
            .unwrap();
        let mut journal = std::fs::read(&journal_path).unwrap();
        match mode {
            0 => journal[64] ^= 1,
            1 => journal.extend_from_slice(&old_journal[old_journal.len() - 448..]),
            _ => {
                journal.pop();
            }
        }
        drop(custody);
        drop(s);
        std::fs::write(journal_path, journal).unwrap();
        probe_reopen(d.path(), false);
    }
}

// 隔离 FakeSealer 允许构造密封层已通过的反例；不是伪造真实 DPAPI 的证明。
fn rewrite_test_capsule(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let size = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let mut capsule: serde_json::Value =
        serde_json::from_slice(&FakeSealer.unseal(&bytes[64..64 + size]).unwrap()).unwrap();
    change(&mut capsule);
    let sealed = FakeSealer
        .seal(&serde_json::to_vec(&capsule).unwrap())
        .unwrap();
    let mut result = bytes[..64].to_vec();
    result[8..16].copy_from_slice(&(sealed.len() as u64).to_le_bytes());
    result[16..48].copy_from_slice(&Sha256::digest(&sealed));
    result.extend_from_slice(&sealed);
    result
}
#[test]
fn UnattendedCheckpoint_AuthenticatedAnchorFieldsAndAbsoluteOverflow_005() {
    let (_d, s, old) = capacity::near_capacity(true, 1);
    let custody = s.backend.claim_owner_custody().unwrap();
    let blob = raw(&s);
    s.checkpoint(&old.expected(), &custody, &mut || Ok(()))
        .unwrap();
    let bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
    for field in [
        "domain",
        "schema",
        "segment",
        "sequence",
        "blob",
        "document_digest",
    ] {
        let altered = rewrite_test_capsule(&bytes, |v| {
            v[field] = match field {
                "domain" => serde_json::json!("foreign"),
                "blob" | "document_digest" => serde_json::json!([0; 32].to_vec()),
                _ => serde_json::json!(0),
            };
        });
        assert!(
            journal::Chain::parse_authenticated(&altered, false, &FakeSealer).is_err(),
            "{field}"
        );
    }
    let altered = rewrite_test_capsule(&bytes, |v| {
        v["document"]["observed_wall_ms"] = serde_json::json!(1001);
        let document: Document = serde_json::from_value(v["document"].clone()).unwrap();
        v["document_digest"] =
            serde_json::json!(Sha256::digest(serde_json::to_vec(&document).unwrap()).to_vec());
    });
    let chain = journal::Chain::parse_authenticated(&altered, false, &FakeSealer).unwrap();
    assert!(chain.verify_checkpoint_blob(&blob, &FakeSealer).is_err());
    let exhausted =
        rewrite_test_capsule(&bytes, |v| v["sequence"] = serde_json::json!(u64::MAX - 1));
    let chain = journal::Chain::parse_authenticated(&exhausted, false, &FakeSealer).unwrap();
    let mut next = old.0.clone();
    next.revision += 1;
    assert!(chain.pending(&s.stamp(&next, &blob).unwrap()).is_err());
    let exhausted = rewrite_test_capsule(&bytes, |v| v["segment"] = serde_json::json!(u64::MAX));
    let chain = journal::Chain::parse_authenticated(&exhausted, false, &FakeSealer).unwrap();
    assert!(journal::seal_checkpoint(
        &old.0,
        &blob,
        chain.checkpoint_basis().unwrap(),
        &FakeSealer
    )
    .is_err());
    let chain = journal::Chain::parse_authenticated(&bytes, false, &FakeSealer).unwrap();
    assert_eq!(
        chain.remaining_commits().unwrap(),
        (journal::MAX_JOURNAL - bytes.len()) / 448
    );
    assert!(chain.remaining_commits().unwrap() < journal::MAX_RECORDS / 2);
}

#[test]
fn UnattendedCheckpoint_SealUnknownBeforeWriteLocksInstance_006() {
    struct UnknownSeal;
    impl Sealer for UnknownSeal {
        fn seal(&self, _: &[u8]) -> Result<Vec<u8>> {
            Err(StoreError::Unknown)
        }
        fn unseal(&self, b: &[u8]) -> Result<Vec<u8>> {
            FakeSealer.unseal(b)
        }
    }
    let (d, s, old) = capacity::near_capacity(true, 1);
    let journal_path = path(&s, "commit.journal");
    let blob_path = path(&s, "delegation.dpapi");
    let before = std::fs::read(&journal_path).unwrap();
    let blob = raw(&s);
    drop(s);
    let store = Store::new(
        WindowsUnattendedBackend::open_existing(d.path()).unwrap(),
        UnknownSeal,
    )
    .unwrap();
    let custody = store.backend.claim_owner_custody().unwrap();
    assert_eq!(
        store.checkpoint(&old.expected(), &custody, &mut || Ok(())),
        Err(StoreError::Unknown)
    );
    assert!(store.observe(&old.expected(), 1000).is_err());
    assert_eq!(
        std::fs::read(journal_path).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(blob_path).unwrap(),
        blob
    );
    drop(custody);
    drop(store);
    probe_reopen(d.path(), true);
}
