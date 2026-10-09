//! Real held-root durability and lease lifetime dependencies only. These tests
//! never fabricate the native source/browser terminal admission.
use crate::version_history::{
    journal::{JournalBinding, RootKind},
    snapshot::{SnapshotLimits, SnapshotManifest},
    windows::{
        context::{HeldContext, HeldRoot},
        files::{ComponentName, Directory, PrivateDirectory},
        lease::LeaseFiles,
        security::CurrentUser,
    },
};
use std::{ffi::OsStr, sync::Arc};
fn component(value: &str) -> ComponentName {
    ComponentName::new(OsStr::new(value)).unwrap()
}
fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000311".into(),
        source_context: "00000000-0000-4000-8000-000000000312".into(),
        target_context: "00000000-0000-4000-8000-000000000313".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}

// 只读capture不能冒充flush完成；实际同对象flush的M0才可产生数据manifest。
#[test]
fn HistorySourceBoundary_DurableDataObservation_001() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("desk")).unwrap();
    std::fs::create_dir(temporary.path().join("udf")).unwrap();
    std::fs::write(temporary.path().join("desk/config.json"), b"{}").unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let desk_identity = {
        let observed = parent.open_directory(component("desk")).unwrap();
        observed.identity().clone()
    };
    let udf_identity = {
        let observed = parent.open_directory(component("udf")).unwrap();
        observed.identity().clone()
    };
    let desk = parent
        .open_for_rename(component("desk"), &desk_identity)
        .unwrap();
    let udf = parent
        .open_for_rename(component("udf"), &udf_identity)
        .unwrap();
    let readonly = HeldContext::capture(
        HeldRoot::Present(desk.clone()),
        HeldRoot::Present(udf.clone()),
        SnapshotLimits::default(),
    )
    .unwrap();
    assert!(SnapshotManifest::observe_durable_context(&binding(), &readonly).is_err());
    drop(readonly);
    let durable = HeldContext::capture_durable(
        HeldRoot::Present(desk),
        HeldRoot::Present(udf),
        SnapshotLimits::default(),
    )
    .unwrap();
    let observed = SnapshotManifest::observe_durable_context(&binding(), &durable).unwrap();
    assert_eq!(observed.context_id, binding().source_context);
    let desk = observed
        .roots
        .iter()
        .find(|root| root.root == RootKind::Desk)
        .unwrap();
    assert_eq!(
        desk.entries
            .iter()
            .find(|entry| entry.metadata.path == "config.json")
            .unwrap()
            .sha256
            .as_deref(),
        Some(crate::version_history::verified_package::sha256(b"{}").as_str())
    );
    durable.verify_durable().unwrap();
}

// witness保留同一个真实独占锁；controller owner释放不会提前开放普通shared准入。
#[test]
fn HistorySourceBoundary_ExclusiveWitnessLifetime_002() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(temporary.path()).unwrap(),
            component("private"),
            &user,
        )
        .unwrap(),
    );
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let owner = leases.acquire_exclusive(&control).unwrap();
    let witness = owner.witness().unwrap();
    let final_witness = owner.witness().unwrap();
    assert!(leases.acquire_shared(&control).is_err());
    drop(owner);
    witness.verify_root(&root).unwrap();
    assert!(leases.acquire_shared(&control).is_err());
    drop(witness);
    assert!(leases.acquire_shared(&control).is_err());
    drop(final_witness);
    leases.acquire_shared(&control).unwrap();
}
