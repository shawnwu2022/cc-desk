//! Disposable NTFS custody regression; fixture evidence is not native acceptance.
#![allow(non_snake_case)]

use super::*;
use crate::version_history::{journal::CapacityPlan, windows::lease::LeaseFiles};

// 检查稍后副本写入中断保留原树、部分副本及根权限句柄，拒绝重放且不发布终态证明。
#[test]
fn BundlePrepare_RetainCopyFailure_001() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let install = Arc::new(
        PrivateDirectory::create_new(parent.clone(), component("install").unwrap(), &user).unwrap(),
    );
    let data = Arc::new(
        PrivateDirectory::create_new(parent.clone(), component("data").unwrap(), &user).unwrap(),
    );
    let records = Arc::new(
        PrivateDirectory::create_new(parent, component("records").unwrap(), &user).unwrap(),
    );
    drop(ManagerRecord::create(install.clone(), "cc-desk.exe", &"original image", &user).unwrap());
    let tree = HeldTree::capture_private(
        install.directory().clone(),
        SnapshotLimits::default(),
        &user,
    )
    .unwrap();
    let manifest = InstalledBundleManifest {
        schema: 1,
        original_image_name: "cc-desk.exe".into(),
        fenced_image_location: "1".repeat(64),
        tree: tree.manifest.clone(),
    };
    let source = HeldBundle { tree, manifest };
    let binding = JournalBinding {
        transaction_id: "11111111-1111-4111-8111-111111111111".into(),
        source_context: "22222222-2222-4222-8222-222222222222".into(),
        target_context: "33333333-3333-4333-8333-333333333333".into(),
        user_installation: "1".repeat(64),
        source_bundle: source.manifest.logical_digest().unwrap(),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let mut store = JournalStore::open_windows(records.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(150, 250, 30, 4096).unwrap(),
        )
        .unwrap();
    let leases = LeaseFiles::open(records.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let lease = leases.acquire_exclusive(&control).unwrap();
    let mut journal = ContextJournal::new(&mut store, records, &lease, binding.clone(), 0).unwrap();
    let original = Arc::new(
        RetainedInstallationBundle::preserve_observed(
            install.directory().clone(),
            &temp.path().join("install"),
            &source,
            data,
            &user,
            &mut journal,
        )
        .unwrap(),
    );
    let digest = journal.retain(&source.manifest).unwrap();
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Manifest {
                role: ManifestRole::SourceBundle,
                digest,
            },
        )
        .unwrap();
    let digest = journal
        .retain(&"fixture original context already preserved")
        .unwrap();
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Manifest {
                role: ManifestRole::SourceContext,
                digest,
            },
        )
        .unwrap();
    drop(source);
    std::fs::remove_file(temp.path().join("install/cc-desk.exe")).unwrap();
    let later_bytes = serde_json::to_vec(&"later installation data").unwrap();
    drop(
        ManagerRecord::create(
            install.clone(),
            "later.bin",
            &"later installation data",
            &user,
        )
        .unwrap(),
    );
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let boundary = Arc::new(
        ReturnBoundary::fixture(
            SnapshotBoundary::fixture(binding.clone()),
            install.directory().clone(),
            component("cc-desk.exe").unwrap(),
            None,
        )
        .unwrap(),
    );
    let mut attempt = BundlePreparationAttempt::new(original, boundary);
    let probe = probe_copy_failure(CopyFault::AfterWrite);
    let failure = attempt
        .prepare(&user, &mut journal)
        .err()
        .expect("injected later-copy write interruption must reject preparation");
    drop(probe);
    assert_eq!(failure.to_string(), "injected copy boundary failure");
    assert!(attempt.attempted, "failed preparation consumes its attempt");
    let current = attempt.current.as_ref().expect("current tree stays held");
    current.verify().unwrap();
    let later = attempt.later.as_ref().expect("partial copy stays held");
    assert!(
        later.manifest().is_err(),
        "partial copy is not a complete backup"
    );
    let partial = later.tree.as_ref().expect("partial copy tree stays held");
    let HeldRoot::Present(copy_root) = &partial.root else {
        panic!("copy root was created before the injected file-write failure");
    };
    copy_root.recheck().unwrap();
    let copy_identity = copy_root.identity().clone();
    let copied = partial
        .entries
        .get("later.bin")
        .expect("created copy file stays held");
    let HeldEntry::File(copied) = copied else {
        panic!("later.bin must remain an owned partial file");
    };
    assert_eq!(copied.digest().unwrap(), sha256(&later_bytes));
    let permission_owner = attempt
        .root_write
        .as_ref()
        .expect("root permissions stay held");
    let permission_identity =
        crate::version_history::windows::files::metadata(handle(permission_owner))
            .unwrap()
            .identity;
    assert_eq!(&permission_identity, install.directory().identity());
    let current_path = temp.path().join("install/later.bin");
    let copy_path = temp.path().join("data").join(LATER_COPY).join("later.bin");
    assert_eq!(std::fs::read(&current_path).unwrap(), later_bytes);
    assert_eq!(std::fs::read(&copy_path).unwrap(), later_bytes);
    assert!(std::fs::write(&current_path, b"replace current").is_err());
    assert!(std::fs::write(&copy_path, b"replace partial copy").is_err());
    let generation = journal.generation;
    let pending = journal
        .store
        .context_pending()
        .unwrap()
        .unwrap()
        .0
        .effect_id;
    let seed = journal.store.latest_bundle_seed().unwrap();
    assert!(seed.is_some(), "failed copy retains its admitted seed");
    let rejected_retry = attempt
        .prepare(&user, &mut journal)
        .err()
        .expect("same preparation attempt must never replay a failed copy");
    assert_eq!(
        rejected_retry.to_string(),
        "bundle preparation requires reconciliation"
    );
    assert_eq!(
        journal.generation, generation,
        "rejected retry cannot append an effect"
    );
    assert_eq!(journal.store.latest_bundle_seed().unwrap(), seed);
    assert_eq!(
        journal
            .store
            .context_pending()
            .unwrap()
            .unwrap()
            .0
            .effect_id,
        pending
    );
    attempt.current.as_ref().unwrap().verify().unwrap();
    let HeldRoot::Present(retained_root) =
        &attempt.later.as_ref().unwrap().tree.as_ref().unwrap().root
    else {
        panic!("rejected retry must retain the exact partial-copy root");
    };
    assert_eq!(retained_root.identity(), &copy_identity);
    assert_eq!(
        crate::version_history::windows::files::metadata(handle(
            attempt.root_write.as_ref().unwrap()
        ))
        .unwrap()
        .identity,
        permission_identity,
        "rejected retry must retain the same root-permission owner"
    );
    assert_eq!(std::fs::read(&current_path).unwrap(), later_bytes);
    assert_eq!(std::fs::read(&copy_path).unwrap(), later_bytes);
    attempt.original.verify(&user).unwrap();
    assert!(
        attempt.prepared.is_none(),
        "failed copy cannot create a restoration proof"
    );
    for unpublished in [RETURN_PLAN, RETURN_RESULT, LATER_OBJECTS] {
        assert!(
            !temp.path().join("data").join(unpublished).exists(),
            "failed preparation must not publish {unpublished}"
        );
    }
    let state = journal.store.inspect(&binding).unwrap().last_valid.unwrap();
    assert_eq!(state.phase(), JournalPhase::RecoveryRequired);
    assert_eq!(
        state.effect_observation(&pending),
        Some(Observation::Unknown)
    );
    assert!(
        state.requires_reconciliation(),
        "partial copy never proves terminal restoration"
    );
}
