//! Custody checks use disposable real Windows objects, never application roots.
#![allow(non_snake_case)]

use super::*;
use crate::version_history::{
    journal::CapacityPlan, snapshot::capture_context, windows::lease::LeaseFiles,
};

// 检查准入失败保留已旋转原始文件、完整副本和再准入证据，修正输入后仍可准入。
#[test]
fn HistoryCustody_AdmissionFailure_001() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let name = |value: &str| ComponentName::new(OsStr::new(value)).unwrap();
    let binding = JournalBinding {
        transaction_id: "11111111-1111-4111-8111-111111111111".into(),
        source_context: "22222222-2222-4222-8222-222222222222".into(),
        target_context: "33333333-3333-4333-8333-333333333333".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let desk =
        PrivateDirectory::create_renameable_new(parent.clone(), name("desk"), &user).unwrap();
    std::fs::write(temporary.path().join("desk/state"), b"original state").unwrap();
    let mut source = HeldContext::capture_durable(
        HeldRoot::Present(desk.directory().clone()),
        HeldRoot::observe(parent.clone(), name("udf")).unwrap(),
        SnapshotLimits::default(),
    )
    .unwrap();
    drop(desk);
    let boundary = SnapshotBoundary::fixture_with_roots(binding.clone(), source.root_identities());
    let expected = capture_context(
        &boundary,
        &binding.source_context,
        &mut source,
        SnapshotLimits::default(),
    )
    .unwrap();
    let records =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("records"), &user).unwrap());
    let copies_root =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("copies"), &user).unwrap());
    let quarantine =
        Arc::new(PrivateDirectory::create_new(parent.clone(), name("quarantine"), &user).unwrap());
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
        )
        .unwrap();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    std::fs::write(temporary.path().join("image.exe"), b"fixture image").unwrap();
    let image = parent
        .open_file(name("image.exe"), FileAccess::Read)
        .unwrap();
    let image_identity = image.identity().clone();
    let image_digest = image.digest().unwrap();
    drop(image);
    let fence = ImageFence::acquire(
        parent.clone(),
        name("image.exe"),
        &image_identity,
        &image_digest,
    )
    .unwrap();
    let mut journal =
        ContextJournal::new(&mut store, records, &exclusive, binding.clone(), 0).unwrap();
    let mut copies = BTreeMap::new();
    let mut readmitted = BTreeMap::new();
    for (kind, origin, retained, backup) in [
        (RootKind::Desk, "desk", "desk-old", "desk-copy"),
        (RootKind::WebView, "udf", "udf-old", "udf-copy"),
    ] {
        let mut copy = PrivateTreeCopy::new(copies_root.clone(), name(backup));
        copy.copy_from(source.tree(kind), &user, &mut journal)
            .unwrap();
        if !source.tree(kind).manifest.entries.is_empty() {
            let proof = copy
                .rotate_context_root(
                    &mut source,
                    kind,
                    parent.clone(),
                    name(origin),
                    quarantine.clone(),
                    name(retained),
                    &boundary,
                    &fence,
                    &user,
                    &mut journal,
                )
                .unwrap();
            readmitted.insert(kind, proof);
        }
        copies.insert(kind, copy);
    }
    let mut source = Some(source);
    let mut wrong = expected.clone();
    wrong.context_id = binding.target_context.clone();
    assert!(RetainedContextRoots::admit_retaining(
        &mut source,
        &mut copies,
        &mut readmitted,
        &wrong,
        &boundary,
        &user,
    )
    .is_err());
    assert!(
        source.is_some(),
        "failed admission must retain source custody"
    );
    assert_eq!(copies.len(), 2, "both original copies must remain held");
    assert_eq!(readmitted.len(), 1, "rotation proof must remain held");
    source.as_ref().unwrap().verify_durable().unwrap();
    for copy in copies.values() {
        copy.verify(&user).unwrap();
    }
    assert!(
        std::fs::write(
            temporary.path().join("quarantine/desk-old/state"),
            b"changed"
        )
        .is_err(),
        "failed admission must not release the rotated original file guard"
    );
    let originals = RetainedContextRoots::admit_retaining(
        &mut source,
        &mut copies,
        &mut readmitted,
        &expected,
        &boundary,
        &user,
    )
    .unwrap();
    originals.verify(&user).unwrap();
    assert!(
        source.is_none(),
        "successful admission transfers source custody"
    );
    assert!(
        copies.is_empty(),
        "successful admission transfers copy custody"
    );
    assert!(
        readmitted.is_empty(),
        "successful admission retires rotation proofs"
    );
}
