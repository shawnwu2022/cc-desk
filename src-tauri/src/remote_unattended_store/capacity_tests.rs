//! 隔离 NTFS 近满历史，固定原上限；不重置日志或修改真实授权。
use super::*;
fn near_capacity(
    enabled: bool,
    remaining: usize,
) -> (tempfile::TempDir, TestStore, LockedSnapshot) {
    use journal::{Chain, Record, MAX_RECORDS};
    let d = directory();
    let s = create(d.path());
    let first = initialize(&s);
    let first = if enabled { enable(&s, &first) } else { first };
    let mut doc = first.0;
    let mut bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
    let mut chain = Chain::parse(&bytes, false).unwrap();
    let mut blob = raw(&s);
    // TEST_ONLY：构造完整合法历史，不把这种构造当原批准或真实4096次写盘。
    for _ in chain.records / 2..MAX_RECORDS / 2 - remaining {
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
    (d, s, LockedSnapshot(doc))
}
fn reopened_revoked(d: &Path, s: &TestStore, floor: u64) {
    let doc = s.decode(&raw(s)).unwrap();
    assert_eq!(doc.status, Status::Revoked);
    assert_eq!(doc.observed_wall_ms, floor);
    let expected = LockedSnapshot(doc).expected();
    let loaded = reopen(d).observe(&expected, floor).unwrap();
    assert_eq!(loaded.0.status, Status::Revoked);
    assert_eq!(
        std::fs::metadata(path(s, "commit.journal")).unwrap().len(),
        journal::MAX_JOURNAL as u64
    );
}
#[test]
fn UnattendedCapacity_LastPairClosesInsteadOfAdvancingEnabled_001() {
    let (d, s, old) = near_capacity(true, 1);
    assert_eq!(
        s.observe(&old.expected(), 1100),
        Err(StoreError::CapacityRevoked)
    );
    reopened_revoked(d.path(), &s, 1100);
}
#[test]
fn UnattendedCapacity_LastPairStillPersistsExplicitRevoke_002() {
    let (d, s, old) = near_capacity(true, 1);
    let revoked = s.revoke(&old.expected(), 1100).unwrap();
    assert_eq!(revoked.0.authority_revision, old.0.authority_revision + 1);
    reopened_revoked(d.path(), &s, 1100);
}
#[test]
fn UnattendedCapacity_EnableCannotSpendRevocationReserve_003() {
    let (_d, s, old) = near_capacity(false, 1);
    let blob = raw(&s);
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    assert_eq!(
        s.enable(permit(&s, old.expected(), 1000, 2000), 1000),
        Err(StoreError::Capacity)
    );
    assert_eq!(raw(&s), blob);
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}
#[test]
fn UnattendedCapacity_OldFullEnabledNeverReturnsSuccessfulObservation_004() {
    let (d, s, old) = near_capacity(true, 0);
    let blob = raw(&s);
    let journal = std::fs::read(path(&s, "commit.journal")).unwrap();
    assert_eq!(s.observe(&old.expected(), 1000), Err(StoreError::Capacity));
    assert_eq!(s.revoke(&old.expected(), 1000), Err(StoreError::Capacity));
    assert_eq!(
        reopen(d.path()).observe(&old.expected(), 1000),
        Err(StoreError::Capacity)
    );
    assert_eq!(raw(&s), blob);
    assert_eq!(std::fs::read(path(&s, "commit.journal")).unwrap(), journal);
}
#[test]
fn UnattendedCapacity_ClockAndExpiryKeepOriginalClosureReason_005() {
    for (now, reason, floor) in [
        (900, StoreError::Clock, 1000),
        (2000, StoreError::Expired, 2000),
    ] {
        let (d, s, old) = near_capacity(true, 1);
        assert_eq!(s.observe(&old.expected(), now), Err(reason));
        reopened_revoked(d.path(), &s, floor);
    }
}
#[test]
fn UnattendedCapacity_ClosingPublicationFailureIsUnknown_006() {
    let (_d, s, old) = near_capacity(true, 1);
    s.backend.test_journal_fault(5);
    assert_eq!(s.observe(&old.expected(), 1100), Err(StoreError::Unknown));
    assert_eq!(s.observe(&old.expected(), 1100), Err(StoreError::Poisoned));
}

#[test]
fn UnattendedCapacity_TwoPairsAdvanceThenEqualWallCloses_007() {
    let (d, s, old) = near_capacity(true, 2);
    let advanced = s.observe(&old.expected(), 1100).unwrap();
    assert_eq!(advanced.0.status, Status::Enabled);
    assert_eq!(
        std::fs::metadata(path(&s, "commit.journal")).unwrap().len(),
        (journal::MAX_JOURNAL - 2 * journal::RECORD_BYTES) as u64
    );
    assert_eq!(
        s.observe(&advanced.expected(), 1100),
        Err(StoreError::CapacityRevoked)
    );
    reopened_revoked(d.path(), &s, 1100);
}
#[test]
fn UnattendedCapacity_TwoPairsEnableThenExplicitRevoke_008() {
    let (d, s, old) = near_capacity(false, 2);
    let enabled = s
        .enable(permit(&s, old.expected(), 1000, 2000), 1000)
        .unwrap();
    assert_eq!(enabled.0.status, Status::Enabled);
    s.revoke(&enabled.expected(), 1100).unwrap();
    reopened_revoked(d.path(), &s, 1100);
}
#[test]
fn UnattendedCapacity_RemainingRequiresOriginalVerifiedRead_009() {
    let (_d, s, _) = near_capacity(true, 2);
    let mut tx = s.backend.begin().unwrap();
    assert_eq!(tx.remaining_commits(), Err(StoreError::Invalid));
    tx.read().unwrap();
    assert_eq!(tx.remaining_commits(), Ok(2));
}

#[test]
fn UnattendedCapacity_PendingAndTornReadCannotYieldCapacity_010() {
    for cut in [journal::RECORD_BYTES, 1] {
        let d = directory();
        let s = create(d.path());
        initialize(&s);
        let bytes = std::fs::read(path(&s, "commit.journal")).unwrap();
        std::fs::write(path(&s, "commit.journal"), &bytes[..bytes.len() - cut]).unwrap();
        let mut tx = s.backend.begin().unwrap();
        assert!(tx.read().is_err());
        assert_eq!(tx.remaining_commits(), Err(StoreError::Poisoned));
    }
}
