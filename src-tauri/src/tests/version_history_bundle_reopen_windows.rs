//! Real NTFS object readmission. The fixture does not mint a live terminal
//! checkpoint: public seal admission is tested separately from these objects.
use super::*;
use crate::version_history::{journal::CapacityPlan, windows::lease::LeaseFiles};

fn prepared_bundle_probe(
    run: impl FnOnce(
        &tempfile::TempDir,
        &CurrentUser,
        Arc<RetainedInstallationBundle>,
        Arc<ReturnBoundary>,
        &ManagerRecordReference,
        &mut ContextJournal<'_>,
    ),
) {
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
    for (name, bytes) in [
        ("cc-desk.exe", "original image"),
        ("unknown.bin", "original companion"),
    ] {
        drop(ManagerRecord::create(install.clone(), name, &bytes, &user).unwrap());
    }
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
            CapacityPlan::for_effects(150, 200, 30, 4096).unwrap(),
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
            data.clone(),
            &user,
            &mut journal,
        )
        .unwrap(),
    );
    let source_digest = journal.retain(&source.manifest).unwrap();
    let context_digest = journal.retain(&"fixture source context").unwrap();
    for (role, digest) in [
        (ManifestRole::SourceBundle, source_digest),
        (ManifestRole::SourceContext, context_digest),
    ] {
        journal.generation = journal
            .store
            .append(journal.generation, JournalEvent::Manifest { role, digest })
            .unwrap();
    }
    drop(source);
    std::fs::write(temp.path().join("install/cc-desk.exe"), b"later executable").unwrap();
    std::fs::write(temp.path().join("install/unknown.bin"), b"later companion").unwrap();
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    let image = install
        .directory()
        .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
        .unwrap();
    let image_identity = image.identity().clone();
    let image_digest = image.digest().unwrap();
    drop(image);
    let mut fence = ImageFence::acquire(
        install.directory().clone(),
        component("cc-desk.exe").unwrap(),
        &image_identity,
        &image_digest,
    )
    .unwrap();
    let image_quarantine = Arc::new(
        PrivateDirectory::create_new(
            data.directory().clone(),
            component("historical-image").unwrap(),
            &user,
        )
        .unwrap(),
    );
    fence
        .rename_to(
            image_quarantine.directory().clone(),
            component("current-image.exe").unwrap(),
        )
        .unwrap();
    let boundary = Arc::new(
        ReturnBoundary::fixture(
            SnapshotBoundary::fixture(binding.clone()),
            install.directory().clone(),
            component("cc-desk.exe").unwrap(),
            Some(Arc::new(Mutex::new(fence))),
        )
        .unwrap(),
    );
    let prepared =
        BundleRestoration::prepare(original.clone(), boundary.clone(), &user, &mut journal)
            .unwrap();
    let reference = prepared.plan_reference().clone();
    assert_eq!(prepared.plan.detached_image.as_deref(), Some("cc-desk.exe"));
    let digest = journal.retain(&"fixture later context").unwrap();
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest,
            },
        )
        .unwrap();
    for kind in [
        EffectKind::FenceHistoricalImage,
        EffectKind::PreserveRoot {
            context: binding.target_context.clone(),
            root: RootKind::Desk,
        },
        EffectKind::PreserveRoot {
            context: binding.target_context.clone(),
            root: RootKind::WebView,
        },
    ] {
        let pending = journal
            .begin(kind, &"fixture before", &"fixture after")
            .unwrap();
        journal.applied(pending, &"fixture after").unwrap();
    }
    journal.generation = journal
        .store
        .append(
            journal.generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap();
    drop(prepared);
    run(&temp, &user, original, boundary, &reference, &mut journal);
}

#[test]
fn BundleReturn_PreparedReopenObjects_007() {
    prepared_bundle_probe(|temp, user, original, boundary, expected, journal| {
        let before = std::fs::read(temp.path().join("records/journal.log")).unwrap();
        let generation = journal.generation;
        let reopened = BundleRestoration::reopen_prepared_objects(
            original.clone(),
            boundary.clone(),
            expected,
            user,
            journal,
        )
        .unwrap();
        reopened.verify_return_checkpoint(user, journal).unwrap();
        assert_eq!(reopened.plan_reference(), expected);
        assert_eq!(reopened.writes.len(), 1);
        assert_eq!(reopened.directories.len(), 1);
        assert_eq!(reopened.plan.detached_image.as_deref(), Some("cc-desk.exe"));
        drop(reopened);
        assert!(
            BundleRestoration::reopen_prepared(original, boundary, expected, user, journal)
                .is_err(),
            "valid native objects alone must not mint a sealed return checkpoint"
        );
        assert_eq!(journal.generation, generation);
        assert_eq!(
            std::fs::read(temp.path().join("records/journal.log")).unwrap(),
            before
        );
        assert!(!temp.path().join("data/later-installation-objects").exists());
        assert!(!temp
            .path()
            .join("data/installation-return-result.json")
            .exists());
    });
}

#[test]
fn BundleReturn_PreparedReopenRejectsChangedObjects_008() {
    for (relative, replace_identity) in [
        ("install/unknown.bin", false),
        ("data/later-installation-copy/unknown.bin", false),
        ("install/unknown.bin", true),
        ("data/later-installation-copy/unknown.bin", true),
    ] {
        prepared_bundle_probe(|temp, user, original, boundary, expected, journal| {
            let path = temp.path().join(relative);
            if replace_identity {
                let bytes = std::fs::read(&path).unwrap();
                std::fs::remove_file(&path).unwrap();
                std::fs::write(&path, bytes).unwrap();
            } else {
                std::fs::write(&path, b"altered content").unwrap();
            }
            let before = std::fs::read(temp.path().join("records/journal.log")).unwrap();
            assert!(BundleRestoration::reopen_prepared_objects(
                original, boundary, expected, user, journal,
            )
            .is_err());
            assert_eq!(
                std::fs::read(temp.path().join("records/journal.log")).unwrap(),
                before
            );
            assert!(!temp.path().join("data/later-installation-objects").exists());
        });
    }
}

#[test]
fn BundleReturn_PreparedReopenRejectsUnexpectedObjects_009() {
    for relative in [
        "install/unplanned.bin",
        "data/later-installation-copy/unplanned.bin",
        "data/later-installation-objects",
        "data/installation-return-result.json",
    ] {
        prepared_bundle_probe(|temp, user, original, boundary, expected, journal| {
            std::fs::write(temp.path().join(relative), b"unexpected object").unwrap();
            let before = std::fs::read(temp.path().join("records/journal.log")).unwrap();
            assert!(BundleRestoration::reopen_prepared_objects(
                original, boundary, expected, user, journal,
            )
            .is_err());
            assert_eq!(
                std::fs::read(temp.path().join("records/journal.log")).unwrap(),
                before
            );
        });
    }
}

#[test]
fn BundleReturn_PreparedReopenRestores_010() {
    prepared_bundle_probe(|temp, user, original, boundary, expected, journal| {
        let mut reopened =
            BundleRestoration::reopen_prepared_objects(original, boundary, expected, user, journal)
                .unwrap();
        let restored = reopened.restore(user, journal).unwrap();
        restored.verify(user).unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("install/unknown.bin")).unwrap(),
            b"\"original companion\""
        );
        assert_eq!(
            std::fs::read(temp.path().join("data/later-installation-copy/unknown.bin")).unwrap(),
            b"later companion"
        );
        let generation = journal.generation;
        assert!(reopened.restore(user, journal).is_err());
        assert_eq!(journal.generation, generation);
    });
}

#[test]
fn BundleReturn_PreparedReopenRejectsPlanMismatch_011() {
    for changed in [
        "attempt",
        "source",
        "slot",
        "detached_image",
        "image_identity",
    ] {
        prepared_bundle_probe(|temp, user, original, boundary, expected, journal| {
            let path = temp.path().join("data/installation-return-plan.json");
            let mut plan: ReturnPlan =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            match changed {
                "attempt" => plan.attempt = AttemptNames::fresh(),
                "source" => plan.source = expected.clone(),
                "slot" => plan.slot.name = "another-installation".into(),
                "detached_image" => plan.detached_image = None,
                "image_identity" => plan.image_identity = None,
                _ => unreachable!(),
            }
            std::fs::write(path, serde_json::to_vec(&plan).unwrap()).unwrap();
            // Exercise semantic validation with this changed record's actual
            // identity/digest. The public gate must never accept this new ref.
            let changed_record =
                ManagerRecord::observe(original.data.clone(), RETURN_PLAN, user).unwrap();
            let changed_reference = changed_record.reference().clone();
            drop(changed_record);
            let before = std::fs::read(temp.path().join("records/journal.log")).unwrap();
            assert!(BundleRestoration::reopen_prepared_objects(
                original,
                boundary,
                &changed_reference,
                user,
                journal,
            )
            .is_err());
            assert_eq!(
                std::fs::read(temp.path().join("records/journal.log")).unwrap(),
                before
            );
        });
    }
}
