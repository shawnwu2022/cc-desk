#![allow(non_snake_case)]
use super::*;
use crate::version_history::maintenance::RuntimeKind;
fn observed(id: uuid::Uuid) -> Arc<CodexOwnerIdentity> {
    let identity = CodexOwnerIdentity::new(Some(id));
    identity.observe(id);
    identity
}
#[test]
fn NativeArchiveOwners_RequestedResumeIdIsNotObservedIdentity_027() {
    let id = uuid::Uuid::new_v4();
    let identity = CodexOwnerIdentity::new(Some(id));
    assert!(identity.sessions().is_err());
    let actual = uuid::Uuid::new_v4();
    identity.observe(actual);
    assert_eq!(identity.sessions().unwrap(), BTreeSet::from([actual]));
}

#[test]
fn NativeArchiveOwners_UnrelatedCodexUsesScopedAdmission_016() {
    let gate = AdmissionGate::new();
    let active = uuid::Uuid::new_v4();
    let target = uuid::Uuid::new_v4();
    let identity = observed(active);
    let mut start = gate.begin_start(RuntimeKind::Native).unwrap().preparing();
    start.identify_codex(identity);
    let owned = start.begin_creation().child_created();
    assert!(gate.check_codex_archive_for(active).is_err());
    assert!(gate.check_metadata_idle().is_err());
    assert!(gate.begin_metadata().is_err());
    let result = gate.begin_codex_archive_for(target);
    assert!(
        result.is_ok(),
        "identified unrelated Codex must not globally block archive"
    );
    let (frozen, helper, owners) = result.unwrap();
    assert_eq!(owners.sessions(), &BTreeSet::from([active]));
    assert!(owners.current().is_ok());
    assert!(gate.begin_start(RuntimeKind::Native).is_err());
    assert!(gate.begin_mutation().is_err());
    drop(helper);
    frozen.release_review().unwrap();
    owned.reaped();
}

#[test]
fn NativeArchiveOwners_RawAndUnknownProcessKeepBarrier_017() {
    let target = uuid::Uuid::new_v4();
    for kind in [
        RuntimeKind::Native,
        RuntimeKind::NativeCli(crate::cli::types::CliKind::Shell),
    ] {
        let gate = AdmissionGate::new();
        let owned = gate.begin_start(kind).unwrap().child_created();
        assert!(gate.begin_codex_archive_for(target).is_err());
        drop(owned);
        assert!(gate.begin_codex_archive_for(target).is_err());
    }
}

#[test]
fn NativeArchiveOwners_IdentityChangesRevokeSnapshot_018() {
    let gate = AdmissionGate::new();
    let identity = observed(uuid::Uuid::new_v4());
    let owned = gate
        .begin_start(RuntimeKind::Codex(identity.clone()))
        .unwrap()
        .child_created();
    let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
    identity.observe(uuid::Uuid::new_v4());
    assert!(owners.current().is_err());
    drop(helper);
    frozen.release_review().unwrap();
    owned.reaped();
}

#[test]
fn NativeArchiveOwners_ExitRevokeSnapshotAndLateIdentityCannotRecreateOwner_019() {
    let gate = AdmissionGate::new();
    let identity = observed(uuid::Uuid::new_v4());
    let owned = gate
        .begin_start(RuntimeKind::Codex(identity.clone()))
        .unwrap()
        .child_created();
    let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
    owned.reaped();
    assert!(owners.current().is_err());
    identity.observe(uuid::Uuid::new_v4());
    drop(helper);
    frozen.release_review().unwrap();
    assert!(gate.check_metadata_idle().is_ok());
}

#[test]
fn NativeArchiveOwners_OverflowRemainsUnknown_020() {
    let identity = CodexOwnerIdentity::new(None);
    for _ in 0..65 {
        identity.observe(uuid::Uuid::new_v4());
    }
    assert!(identity.sessions().is_err());
    identity.observe(uuid::Uuid::new_v4());
    assert!(identity.sessions().is_err());
    let gate = AdmissionGate::new();
    let owned = gate
        .begin_start(RuntimeKind::Codex(identity))
        .unwrap()
        .child_created();
    assert!(gate.check_codex_archive_for(uuid::Uuid::new_v4()).is_err());
    assert!(gate.begin_codex_archive_for(uuid::Uuid::new_v4()).is_err());
    owned.reaped();
}

#[test]
fn NativeArchiveOwners_PendingResumeOnlyVetoesExactTarget_030() {
    let gate = AdmissionGate::new();
    let hint = uuid::Uuid::new_v4();
    let target = uuid::Uuid::new_v4();
    let identity = CodexOwnerIdentity::new(Some(hint));
    let owned = gate
        .begin_start(RuntimeKind::Codex(identity))
        .unwrap()
        .child_created();
    assert!(gate.check_codex_archive_for(hint).is_err());
    assert!(gate.check_codex_archive_for(target).is_ok());
    let (frozen, helper, owners) = gate.begin_codex_archive_for(target).unwrap();
    assert!(
        owners.sessions().is_empty(),
        "resume intent must not become a relation-query identity"
    );
    assert!(owners.current().is_ok());
    drop(helper);
    frozen.release_review().unwrap();
    assert!(gate.check_codex_archive_idle().is_err());
    assert!(gate.begin_codex_archive().is_err());
    assert!(gate.begin_metadata().is_err());
    owned.reaped();
}

#[test]
fn NativeArchiveOwners_PendingNewUsesNativeWriterBoundary_031() {
    let gate = AdmissionGate::new();
    let target = uuid::Uuid::new_v4();
    let owned = gate
        .begin_start(RuntimeKind::Codex(CodexOwnerIdentity::new(None)))
        .unwrap()
        .child_created();
    assert!(gate.check_codex_archive_for(target).is_ok());
    let (frozen, helper, owners) = gate.begin_codex_archive_for(target).unwrap();
    assert!(owners.sessions().is_empty());
    assert!(owners.current().is_ok());
    drop(helper);
    frozen.release_review().unwrap();
    owned.wait_failed();
    assert!(
        gate.check_codex_archive_for(target).is_err(),
        "unknown process lifetime still blocks"
    );
}

#[test]
fn NativeArchiveOwners_PendingObservationRevokesEvenWhenUnionUnchanged_032() {
    let gate = AdmissionGate::new();
    let active = uuid::Uuid::new_v4();
    let known = gate
        .begin_start(RuntimeKind::Codex(observed(active)))
        .unwrap()
        .child_created();
    let pending_identity = CodexOwnerIdentity::new(Some(active));
    let pending = gate
        .begin_start(RuntimeKind::Codex(pending_identity.clone()))
        .unwrap()
        .child_created();
    let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
    assert_eq!(owners.sessions(), &BTreeSet::from([active]));
    assert!(owners.current().is_ok());
    pending_identity.observe(active);
    assert!(
        owners.current().is_err(),
        "per-owner authentication changes cannot hide in an unchanged UUID union"
    );
    drop(helper);
    frozen.release_review().unwrap();
    pending.reaped();
    known.reaped();
}

#[test]
fn NativeArchiveOwners_ResumeHintNeverBecomesObservedRelation_033() {
    let gate = AdmissionGate::new();
    let hint = uuid::Uuid::new_v4();
    let actual = uuid::Uuid::new_v4();
    let identity = CodexOwnerIdentity::new(Some(hint));
    identity.observe(actual);
    assert_eq!(identity.sessions().unwrap(), BTreeSet::from([actual]));
    let owned = gate
        .begin_start(RuntimeKind::Codex(identity))
        .unwrap()
        .child_created();
    assert!(gate.check_codex_archive_for(hint).is_err());
    assert!(gate.check_codex_archive_for(actual).is_err());
    let (frozen, helper, owners) = gate.begin_codex_archive_for(uuid::Uuid::new_v4()).unwrap();
    assert_eq!(owners.sessions(), &BTreeSet::from([actual]));
    drop(helper);
    frozen.release_review().unwrap();
    owned.reaped();
}
