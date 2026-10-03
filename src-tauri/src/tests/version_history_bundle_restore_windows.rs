//! Disposable local NTFS probes. Process/registration authorization is fixture
//! evidence; every copy, namespace and security effect uses the real executor.
use super::*;
use crate::version_history::{journal::CapacityPlan, windows::lease::LeaseFiles};

// 检查完整原安装副本保留未知文件、原逻辑身份与独立私有副本身份，重新打开拒绝被改写的副本。
#[test]
fn BundleReturn_SourceCopy_001() {
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
        PrivateDirectory::create_new(parent.clone(), component("records").unwrap(), &user).unwrap(),
    );
    for (name, bytes) in [
        ("cc-desk.exe", "original image"),
        ("ConPTY.dll", "original DLL"),
        ("unknown.bin", "original unknown companion"),
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
    let mut journal = ContextJournal::new(&mut store, records, &lease, binding, 0).unwrap();
    let retained = RetainedInstallationBundle::preserve_observed(
        install.directory().clone(),
        &temp.path().join("install"),
        &source,
        data.clone(),
        &user,
        &mut journal,
    )
    .unwrap();
    retained.verify(&user).unwrap();
    for (original, copied) in retained
        .saved
        .copy
        .source
        .entries
        .iter()
        .zip(&retained.saved.copy.copy.entries)
    {
        assert_ne!(
            original.metadata.object_identity, copied.metadata.object_identity,
            "private copy must retain its own actual object ID"
        );
        assert_eq!(
            original.sha256, copied.sha256,
            "unknown companion bytes must be preserved"
        );
    }
    let expected = retained.reference().clone();
    let reopened =
        RetainedInstallationBundle::reopen(data.clone(), &expected, &user, &mut journal).unwrap();
    reopened.verify(&user).unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("install/unknown.bin")).unwrap(),
        std::fs::read(
            temp.path()
                .join("data/source-installation-copy/unknown.bin")
        )
        .unwrap()
    );
    drop(reopened);
    drop(retained);
    std::fs::write(
        temp.path()
            .join("data/source-installation-copy/unknown.bin"),
        b"changed private copy",
    )
    .unwrap();
    assert!(
        RetainedInstallationBundle::reopen(data, &expected, &user, &mut journal).is_err(),
        "record equality alone must not authorize modified backup bytes"
    );
    source.tree.verify().unwrap();
}

// 检查返回完整恢复文件名、未知伴随文件、空目录、字节与权限，并保留稍后安装及其新增文件。
#[test]
fn BundleReturn_CompleteRestore_002() {
    for current_image_exists in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let install = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("install").unwrap(), &user)
                .unwrap(),
        );
        let data = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("data").unwrap(), &user)
                .unwrap(),
        );
        let records = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("records").unwrap(), &user)
                .unwrap(),
        );
        let nested = Arc::new(
            PrivateDirectory::create_new(
                install.directory().clone(),
                component("resources").unwrap(),
                &user,
            )
            .unwrap(),
        );
        drop(
            PrivateDirectory::create_new(
                nested.directory().clone(),
                component("empty").unwrap(),
                &user,
            )
            .unwrap(),
        );
        drop(
            ManagerRecord::create(
                nested.clone(),
                "unknown.txt",
                &"original nested companion",
                &user,
            )
            .unwrap(),
        );
        drop(nested);
        for (name, bytes) in [
            ("cc-desk.exe", "original image"),
            ("ConPTY.dll", "original DLL"),
            ("OpenConsole.exe", "original host"),
        ] {
            drop(ManagerRecord::create(install.clone(), name, &bytes, &user).unwrap());
        }
        // Windows creates this child with an unprotected inherited DACL.
        // Change only its owner to the supported unelevated source owner;
        // inherited DACL and actual primary group are left untouched.
        std::fs::write(
            temp.path().join("install/resources/inherited.txt"),
            b"inherited permission contract",
        )
        .unwrap();
        {
            use windows::Win32::Foundation::{LocalFree, HLOCAL};
            use windows::Win32::Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SetFileSecurityW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            };
            let sddl: Vec<u16> = format!("O:{}", user.sid_text())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let path: Vec<u16> = temp
                .path()
                .join("install/resources/inherited.txt")
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            unsafe {
                let mut descriptor = PSECURITY_DESCRIPTOR::default();
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    1,
                    &mut descriptor,
                    None,
                )
                .unwrap();
                assert!(SetFileSecurityW(
                    PCWSTR(path.as_ptr()),
                    OWNER_SECURITY_INFORMATION,
                    descriptor
                )
                .as_bool());
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            }
        }
        // Keep a real, nondefault source DACL distinct from the private copy.
        {
            use windows::Win32::Foundation::{LocalFree, HLOCAL};
            use windows::Win32::Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR,
            };
            let sddl: Vec<u16> = format!("D:P(A;;FA;;;{})(A;;FR;;;SY)", user.sid_text())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let path: Vec<u16> = temp
                .path()
                .join("install/ConPTY.dll")
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            unsafe {
                let mut security = PSECURITY_DESCRIPTOR::default();
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    1,
                    &mut security,
                    None,
                )
                .unwrap();
                assert!(SetFileSecurityW(
                    PCWSTR(path.as_ptr()),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    security
                )
                .as_bool());
                let _ = LocalFree(Some(HLOCAL(security.0)));
            }
        }
        // Exercise exact restoration of a supported nondefault DOS attribute.
        let attribute_path: Vec<_> = temp
            .path()
            .join("install/ConPTY.dll")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            windows::Win32::Storage::FileSystem::SetFileAttributesW(
                PCWSTR(attribute_path.as_ptr()),
                FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_READONLY,
            )
        }
        .unwrap();
        let tree = HeldTree::admit(
            HeldRoot::Present(install.directory().clone()),
            &mut Budget::new(SnapshotLimits::default()).unwrap(),
            None,
        )
        .unwrap();
        let manifest = InstalledBundleManifest {
            schema: 1,
            original_image_name: "cc-desk.exe".into(),
            fenced_image_location: "1".repeat(64),
            tree: tree.manifest.clone(),
        };
        let source = HeldBundle { tree, manifest };
        let original_contract = source.manifest.tree.clone();
        let inherited = original_contract
            .entries
            .iter()
            .find(|entry| entry.metadata.path == "resources/inherited.txt")
            .unwrap();
        let PermissionRecord::Windows { descriptor, .. } = &inherited.metadata.permissions else {
            panic!("Windows inherited descriptor required");
        };
        assert_eq!(
            u16::from_le_bytes([descriptor[2], descriptor[3]])
                & windows::Win32::Security::SE_DACL_PROTECTED.0,
            0,
            "fixture must exercise real unprotected inherited DACL restoration"
        );

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
        let mut journal =
            ContextJournal::new(&mut store, records.clone(), &lease, binding.clone(), 0).unwrap();
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
        let context_manifest = journal
            .retain(&"fixture original context already preserved")
            .unwrap();
        journal.generation = journal
            .store
            .append(
                journal.generation,
                JournalEvent::Manifest {
                    role: ManifestRole::SourceContext,
                    digest: context_manifest,
                },
            )
            .unwrap();
        drop(source);
        unsafe {
            windows::Win32::Storage::FileSystem::SetFileAttributesW(
                PCWSTR(attribute_path.as_ptr()),
                FILE_ATTRIBUTE_NORMAL,
            )
        }
        .unwrap();
        // Fixture installer replaces bytes and removes the old resource subtree.
        // The later executable is absent, as after an interrupted NSIS install.
        std::fs::remove_file(temp.path().join("install/cc-desk.exe")).unwrap();
        std::fs::remove_dir_all(temp.path().join("install/resources")).unwrap();
        std::fs::write(temp.path().join("install/ConPTY.dll"), b"later DLL bytes").unwrap();
        std::fs::write(
            temp.path().join("install/newer-companion.txt"),
            b"newer user companion",
        )
        .unwrap();
        journal.generation = journal
            .store
            .append(
                journal.generation,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired,
                },
            )
            .unwrap();
        let current_fence = if current_image_exists {
            std::fs::write(
                temp.path().join("install/cc-desk.exe"),
                b"later installed executable",
            )
            .unwrap();
            let image = install
                .directory()
                .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
                .unwrap();
            let id = image.identity().clone();
            let digest = image.digest().unwrap();
            drop(image);
            Some(Arc::new(Mutex::new(
                ImageFence::acquire(
                    install.directory().clone(),
                    component("cc-desk.exe").unwrap(),
                    &id,
                    &digest,
                )
                .unwrap(),
            )))
        } else {
            None
        };
        let boundary = Arc::new(
            ReturnBoundary::fixture(
                SnapshotBoundary::fixture(binding.clone()),
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                current_fence,
            )
            .unwrap(),
        );
        let mut restore =
            BundleRestoration::prepare(original.clone(), boundary.clone(), &user, &mut journal)
                .unwrap();
        // 准备阶段持有原根写权限和目录，但尚未产生恢复效果，必须能封存检查点。
        restore
            .verify_return_checkpoint(&user, &mut journal)
            .unwrap();
        {
            let role = ManifestRole::RetainedTargetContext;
            let digest = journal
                .retain(&("fixture context subsystem", role))
                .unwrap();
            journal.generation = journal
                .store
                .append(journal.generation, JournalEvent::Manifest { role, digest })
                .unwrap();
        }
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
                .begin(
                    kind,
                    &"fixture independent process/context evidence",
                    &"fixture independent process/context evidence",
                )
                .unwrap();
            journal
                .applied(pending, &"fixture independent process/context evidence")
                .unwrap();
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
        let generation = journal.generation;
        assert!(journal
            .store
            .admit_bundle_capacity(generation, 1, 4)
            .is_ok());
        assert!(journal
            .store
            .admit_bundle_capacity(generation, 100_001, 4)
            .is_err());
        assert!(journal
            .store
            .admit_bundle_capacity(generation, 1, usize::MAX)
            .is_err());
        assert_eq!(
            journal.generation, generation,
            "capacity checks must not create effects"
        );
        let receipt = restore.restore(&user, &mut journal).unwrap();
        receipt.verify(&user).unwrap();
        verify_logical_restore(&original_contract, receipt.manifest()).unwrap();
        assert!(
            !temp.path().join("install/newer-companion.txt").exists(),
            "later-only names must remain absent in exact original namespace"
        );
        assert!(
            temp.path().join("install/resources/empty").is_dir(),
            "original empty directory must return"
        );
        assert_eq!(
            std::fs::read(temp.path().join("data/later-installation-copy/ConPTY.dll")).unwrap(),
            b"later DLL bytes"
        );
        // Exclusive evacuated files are intentionally not reopened through paths.
        assert_eq!(
            std::fs::read(
                temp.path()
                    .join("data/later-installation-copy/newer-companion.txt")
            )
            .unwrap(),
            b"newer user companion"
        );
        assert_eq!(
            receipt.later_manifest().source.entries.len(),
            receipt.later_manifest().copy.entries.len()
        );
        let expected = receipt.reference().clone();
        let generation = journal.generation;
        assert!(
            restore.restore(&user, &mut journal).is_err(),
            "successful effects are never replayed"
        );
        assert_eq!(journal.generation, generation);
        drop(receipt);
        drop(restore);
        drop(boundary);
        // Restart readmission proves complete current objects and applied receipt.
        let image = install
            .directory()
            .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
            .unwrap();
        let image_id = image.identity().clone();
        let image_digest = image.digest().unwrap();
        drop(image);
        let fence = Arc::new(Mutex::new(
            ImageFence::acquire(
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                &image_id,
                &image_digest,
            )
            .unwrap(),
        ));
        let restarted_boundary = Arc::new(
            ReturnBoundary::fixture(
                SnapshotBoundary::fixture(binding),
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                Some(fence),
            )
            .unwrap(),
        );
        let reopened = RestoredInstallationBundle::reopen(
            original,
            restarted_boundary,
            &expected,
            &user,
            &mut journal,
        )
        .unwrap();
        reopened.verify(&user).unwrap();
        assert_eq!(
            journal.generation, generation,
            "completion readmission must not repeat effects"
        );
    }
}

// 检查写入、刷新、权限、命名及回执中断保留原副本和稍后副本，重复调用与重启不会重放变更。
#[test]
fn BundleReturn_InterruptedNoReplay_003() {
    for (scenario, fault) in [
        Some(BundleFault::BeforeCreate),
        Some(BundleFault::AfterWrite),
        Some(BundleFault::AfterFlush),
        Some(BundleFault::AfterPermissions),
        Some(BundleFault::AfterGuardRelease),
        Some(BundleFault::AfterMove),
        Some(BundleFault::BeforeReadmission),
        Some(BundleFault::BeforeFinalReceipt),
        None,
        None,
        None,
        // Known copied data cannot be reclassified as an unknown partial tree.
        Some(BundleFault::BeforeCreate), // 11: first complete copy changed
        Some(BundleFault::BeforeCreate), // 12: first complete copy missing
        Some(BundleFault::BeforeCreate), // 13: ready plan missing and copy changed
        Some(BundleFault::BeforeCreate), // 14: fresh complete copy changed
        Some(BundleFault::BeforeCreate), // 15: fresh complete copy missing
        Some(BundleFault::BeforeCreate), // 16: older protected copy changed
        Some(BundleFault::BeforeCreate), // 17: older protected copy missing
        Some(BundleFault::BeforeCreate), // 18: older protected quarantine changed
        Some(BundleFault::BeforeCreate), // 19: older protected quarantine missing
    ]
    .into_iter()
    .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let install = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("install").unwrap(), &user)
                .unwrap(),
        );
        let data = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("data").unwrap(), &user)
                .unwrap(),
        );
        let records = Arc::new(
            PrivateDirectory::create_new(parent, component("records").unwrap(), &user).unwrap(),
        );
        drop(
            ManagerRecord::create(install.clone(), "cc-desk.exe", &"original image", &user)
                .unwrap(),
        );
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
        let mut journal =
            ContextJournal::new(&mut store, records.clone(), &lease, binding.clone(), 0).unwrap();
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
        let context_manifest = journal
            .retain(&"fixture original context already preserved")
            .unwrap();
        journal.generation = journal
            .store
            .append(
                journal.generation,
                JournalEvent::Manifest {
                    role: ManifestRole::SourceContext,
                    digest: context_manifest,
                },
            )
            .unwrap();
        drop(source);
        std::fs::remove_file(temp.path().join("install/cc-desk.exe")).unwrap();
        std::fs::write(
            temp.path().join("install/later.bin"),
            b"must survive interrupted return",
        )
        .unwrap();
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
        let mut restore =
            BundleRestoration::prepare(original.clone(), boundary, &user, &mut journal).unwrap();
        let plan_reference = restore.plan_reference().clone();
        {
            let role = ManifestRole::RetainedTargetContext;
            let digest = journal
                .retain(&("fixture context subsystem", role))
                .unwrap();
            journal.generation = journal
                .store
                .append(journal.generation, JournalEvent::Manifest { role, digest })
                .unwrap();
        }
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
                .begin(kind, &"fixture subsystem", &"fixture subsystem")
                .unwrap();
            journal.applied(pending, &"fixture subsystem").unwrap();
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
        let probe = fault.map(probe_bundle_failure);
        if scenario == 8 {
            drop(
                PrivateDirectory::create_new(
                    data.directory().clone(),
                    component(LATER_OBJECTS).unwrap(),
                    &user,
                )
                .unwrap(),
            );
            std::fs::write(
                temp.path()
                    .join("data/later-installation-objects/foreign.bin"),
                b"do not replace collision",
            )
            .unwrap();
        }
        if scenario == 9 {
            std::fs::write(
                temp.path().join("install/arrived-after-copy.bin"),
                b"newer untouched current bytes",
            )
            .unwrap();
        }
        let race_path = temp.path().join("install/cc-desk.exe");
        let rename_probe = if scenario == 10 {
            Some(super::super::super::files::probe_before_rename(move || {
                std::fs::write(race_path, b"newer canonical image").unwrap();
            }))
        } else {
            None
        };
        assert!(restore.restore(&user, &mut journal).is_err());
        drop(rename_probe);
        drop(probe);
        if scenario == 8 {
            assert_eq!(
                std::fs::read(
                    temp.path()
                        .join("data/later-installation-objects/foreign.bin")
                )
                .unwrap(),
                b"do not replace collision"
            );
        }
        if scenario == 9 {
            assert_eq!(
                std::fs::read(temp.path().join("install/arrived-after-copy.bin")).unwrap(),
                b"newer untouched current bytes"
            );
        }
        if scenario == 10 {
            assert_eq!(
                std::fs::read(temp.path().join("install/cc-desk.exe")).unwrap(),
                b"newer canonical image"
            );
        }
        let generation = journal.generation;
        assert!(restore.restore(&user, &mut journal).is_err());
        assert_eq!(
            journal.generation, generation,
            "uncertain return must not add retry effects"
        );
        assert_eq!(
            std::fs::read(temp.path().join("data/later-installation-copy/later.bin")).unwrap(),
            b"must survive interrupted return"
        );
        assert_eq!(
            std::fs::read(
                temp.path()
                    .join("data/source-installation-copy/cc-desk.exe")
            )
            .unwrap(),
            b"\"original image\""
        );
        drop(restore);
        std::fs::write(
            temp.path().join("install/after-interruption.bin"),
            b"new current data after first interruption",
        )
        .unwrap();
        drop(journal);
        drop(store);
        store = JournalStore::open_windows(records.clone()).unwrap();
        store.bind_existing(&binding).unwrap();
        journal = ContextJournal::new(
            &mut store,
            records.clone(),
            &lease,
            binding.clone(),
            generation,
        )
        .unwrap();
        let fence = if temp.path().join("install/cc-desk.exe").exists() {
            let image = install
                .directory()
                .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
                .unwrap();
            let id = image.identity().clone();
            let digest = image.digest().unwrap();
            drop(image);
            Some(Arc::new(Mutex::new(
                ImageFence::acquire(
                    install.directory().clone(),
                    component("cc-desk.exe").unwrap(),
                    &id,
                    &digest,
                )
                .unwrap(),
            )))
        } else {
            None
        };
        let boundary = Arc::new(
            ReturnBoundary::fixture(
                SnapshotBoundary::fixture(binding.clone()),
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                fence,
            )
            .unwrap(),
        );
        if (11..=13).contains(&scenario) {
            let copy = temp.path().join("data").join(LATER_COPY);
            if scenario == 12 {
                std::fs::rename(&copy, temp.path().join("displaced-known-copy")).unwrap();
            } else {
                std::fs::write(copy.join("later.bin"), b"changed known retained bytes").unwrap();
            }
            if scenario == 13 {
                std::fs::rename(
                    temp.path().join("data").join(RETURN_PLAN),
                    temp.path().join("displaced-ready-plan"),
                )
                .unwrap();
            }
            assert!(
                InterruptedInstallationReturn::reopen_pending(
                    original.clone(),
                    boundary,
                    &user,
                    &mut journal
                )
                .is_err(),
                "known copied bytes must remain exact even without a ready-plan file"
            );
            assert_eq!(
                journal.generation, generation,
                "integrity failure is read-only"
            );
            assert_eq!(
                std::fs::read(temp.path().join("install/after-interruption.bin")).unwrap(),
                b"new current data after first interruption"
            );
            continue;
        }
        let reopened = InterruptedInstallationReturn::reopen(
            original.clone(),
            boundary,
            &plan_reference,
            &user,
            &mut journal,
        )
        .unwrap();
        reopened.verify(&user).unwrap();
        assert_eq!(
            reopened.generation(),
            generation,
            "restart must observe the exact pending generation"
        );
        assert_eq!(
            journal.generation, generation,
            "read-only restart must preserve every pending effect"
        );
        if fault == Some(BundleFault::BeforeFinalReceipt) {
            let uncommitted = ManagerRecord::observe(data.clone(), RETURN_RESULT, &user).unwrap();
            assert!(
                RestoredInstallationBundle::reopen(
                    original.clone(),
                    reopened.boundary.clone(),
                    uncommitted.reference(),
                    &user,
                    &mut journal
                )
                .is_err(),
                "an existing result record without its journal receipt must not mint completion"
            );
        }
        let old_pending = journal
            .store
            .context_pending()
            .unwrap()
            .map(|(effect, generation)| (effect.effect_id, generation));
        let fresh = reopened.prepare_fresh_attempt(&user, &mut journal);
        let mut fresh = Some(fresh.unwrap());
        if let Some((id, _)) = &old_pending {
            assert_eq!(
                journal
                    .store
                    .inspect(&binding)
                    .unwrap()
                    .last_valid
                    .unwrap()
                    .effect_observation(id),
                Some(Observation::Unknown),
                "old unknown mutation must remain unknown after new admission"
            );
        }
        let active = journal.store.latest_bundle_backup().unwrap().unwrap();
        let first_attempt = fresh.as_ref().unwrap().plan.attempt.clone();
        if (14..=19).contains(&scenario) {
            let boundary = fresh.as_ref().unwrap().boundary.clone();
            drop(fresh.take());
            std::fs::write(
                temp.path().join("install/after-fresh-plan.bin"),
                b"new current data after fresh plan",
            )
            .unwrap();
            let name = match scenario {
                14 | 15 => first_attempt.name(LATER_COPY).unwrap(),
                16 | 17 => LATER_COPY.into(),
                _ => LATER_OBJECTS.into(),
            };
            let retained = temp.path().join("data").join(name);
            if scenario % 2 == 1 {
                std::fs::rename(&retained, temp.path().join("displaced-known-history")).unwrap();
            } else {
                std::fs::write(
                    retained.join(if scenario == 14 {
                        "after-interruption.bin"
                    } else {
                        "later.bin"
                    }),
                    b"changed known retained history",
                )
                .unwrap();
            }
            let generation = journal.generation;
            drop(journal);
            drop(store);
            store = JournalStore::open_windows(records.clone()).unwrap();
            store.bind_existing(&binding).unwrap();
            journal = ContextJournal::new(
                &mut store,
                records.clone(),
                &lease,
                binding.clone(),
                generation,
            )
            .unwrap();
            assert!(
                InterruptedInstallationReturn::reopen_pending(
                    original.clone(),
                    boundary,
                    &user,
                    &mut journal
                )
                .is_err(),
                "missing or changed known history must block a fresh admission"
            );
            assert_eq!(
                journal.generation, generation,
                "integrity failure must not add effects"
            );
            assert_eq!(
                std::fs::read(temp.path().join("install/after-fresh-plan.bin")).unwrap(),
                b"new current data after fresh plan"
            );
            if let Some((id, _)) = &old_pending {
                assert_eq!(
                    journal
                        .store
                        .inspect(&binding)
                        .unwrap()
                        .last_valid
                        .unwrap()
                        .effect_observation(id),
                    Some(Observation::Unknown)
                );
            }
            continue;
        }
        // One case exercises two more interrupted attempts, including a copy
        // interruption before the next complete return-plan file is created.
        if scenario == 1 {
            let probe = probe_bundle_failure(BundleFault::AfterMove);
            assert!(fresh
                .as_mut()
                .unwrap()
                .restore(&user, &mut journal)
                .is_err());
            drop(probe);
            drop(fresh.take());
            std::fs::write(
                temp.path().join("install/after-second-failure.bin"),
                b"later data after second failure",
            )
            .unwrap();
            for copy_failure in [true, false] {
                let generation = journal.generation;
                drop(journal);
                drop(store);
                store = JournalStore::open_windows(records.clone()).unwrap();
                store.bind_existing(&binding).unwrap();
                journal = ContextJournal::new(
                    &mut store,
                    records.clone(),
                    &lease,
                    binding.clone(),
                    generation,
                )
                .unwrap();
                let fence = if temp.path().join("install/cc-desk.exe").exists() {
                    let image = install
                        .directory()
                        .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
                        .unwrap();
                    let id = image.identity().clone();
                    let digest = image.digest().unwrap();
                    drop(image);
                    Some(Arc::new(Mutex::new(
                        ImageFence::acquire(
                            install.directory().clone(),
                            component("cc-desk.exe").unwrap(),
                            &id,
                            &digest,
                        )
                        .unwrap(),
                    )))
                } else {
                    None
                };
                let boundary = Arc::new(
                    ReturnBoundary::fixture(
                        SnapshotBoundary::fixture(binding.clone()),
                        install.directory().clone(),
                        component("cc-desk.exe").unwrap(),
                        fence,
                    )
                    .unwrap(),
                );
                let observed = InterruptedInstallationReturn::reopen(
                    original.clone(),
                    boundary,
                    &plan_reference,
                    &user,
                    &mut journal,
                )
                .unwrap();
                if copy_failure {
                    let probe = probe_copy_failure(CopyFault::AfterWrite);
                    assert!(observed.prepare_fresh_attempt(&user, &mut journal).is_err());
                    drop(probe);
                    std::fs::write(
                        temp.path().join("install/after-copy-failure.bin"),
                        b"new data after interrupted copy",
                    )
                    .unwrap();
                } else {
                    fresh = Some(observed.prepare_fresh_attempt(&user, &mut journal).unwrap());
                    assert_ne!(
                        fresh.as_ref().unwrap().plan.attempt,
                        first_attempt,
                        "new attempt must use different exact slots"
                    );
                    // A stale observation/generation never admits another plan.
                    let bad = JournalEvent::PrepareBundleBackup {
                        plan: crate::version_history::journal::BundleBackupPlan {
                            current_manifest: active.1.current_manifest.clone(),
                            destination: active.1.destination.clone(),
                            previous_observation: active.1.previous_observation.clone(),
                            previous_generation: None,
                            abandoned_effect: None,
                            effects: 1,
                            recovery_dependencies: active.1.recovery_dependencies,
                        },
                        receipt: active.1.previous_observation.clone(),
                    };
                    let before = journal.generation;
                    assert!(
                        journal.store.append(before, bad.clone()).is_err(),
                        "serialized bundle plans must not mint live recovery admission"
                    );
                    assert!(
                        journal
                            .store
                            .inspect(&binding)
                            .unwrap()
                            .last_valid
                            .unwrap()
                            .apply(bad)
                            .is_err(),
                        "stale previous generation must be rejected by replay validation"
                    );
                    assert_eq!(journal.generation, before);
                    break;
                }
            }
        }
        let mut fresh = fresh.expect("fresh attempt prepared after bounded test interruptions");
        if scenario == 10 {
            let observer = InterruptedInstallationReturn::reopen_pending(
                original.clone(),
                fresh.boundary.clone(),
                &user,
                &mut journal,
            )
            .unwrap();
            let replacement = observer.prepare_fresh_attempt(&user, &mut journal).unwrap();
            let generation = journal.generation;
            assert!(
                fresh.restore(&user, &mut journal).is_err(),
                "superseded live attempt must fail before any namespace mutation"
            );
            assert_eq!(
                journal.generation, generation,
                "stale attempt must not append an intent"
            );
            drop(fresh);
            fresh = replacement;
        }
        let retained_current = fresh.plan.later_copy.source.clone();
        let later_path = temp
            .path()
            .join("data")
            .join(fresh.plan.attempt.name(LATER_COPY).unwrap());
        let receipt = fresh.restore(&user, &mut journal).unwrap();
        receipt.verify(&user).unwrap();
        assert_eq!(
            std::fs::read(
                temp.path()
                    .join("data")
                    .join(first_attempt.name(LATER_COPY).unwrap())
                    .join("after-interruption.bin")
            )
            .unwrap(),
            b"new current data after first interruption"
        );
        if scenario == 1 {
            assert_eq!(
                std::fs::read(later_path.join("after-second-failure.bin")).unwrap(),
                b"later data after second failure"
            );
            assert_eq!(
                std::fs::read(later_path.join("after-copy-failure.bin")).unwrap(),
                b"new data after interrupted copy"
            );
            assert!(
                receipt.result.history.len() >= 3,
                "all earlier complete and partial copies must remain held"
            );
        }
        assert_eq!(receipt.later_manifest().source, retained_current);
        assert_eq!(
            std::fs::read(temp.path().join("install/cc-desk.exe")).unwrap(),
            b"\"original image\""
        );
        let before = journal.generation;
        let dummy = journal.retain(&"blocked forward effect").unwrap();
        assert!(
            journal
                .store
                .append(
                    before,
                    JournalEvent::Intent {
                        effect: EffectSpec {
                            effect_id: uuid::Uuid::new_v4().to_string(),
                            kind: EffectKind::FilesystemEntry {
                                operation: FilesystemOperation::CopyFile,
                                manifest: dummy.clone(),
                                entry_index: 0
                            },
                            before: dummy.clone(),
                            expected_postconditions: dummy
                        }
                    }
                )
                .is_err(),
            "bundle reconciliation must keep the transaction permanently return-only"
        );
        assert_eq!(journal.generation, before);
        let reference = receipt.return_reference();
        drop(receipt);
        drop(fresh);
        let image = install
            .directory()
            .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
            .unwrap();
        let id = image.identity().clone();
        let digest = image.digest().unwrap();
        drop(image);
        let fence = Arc::new(Mutex::new(
            ImageFence::acquire(
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                &id,
                &digest,
            )
            .unwrap(),
        ));
        let boundary = Arc::new(
            ReturnBoundary::fixture(
                SnapshotBoundary::fixture(binding.clone()),
                install.directory().clone(),
                component("cc-desk.exe").unwrap(),
                Some(fence),
            )
            .unwrap(),
        );
        RestoredInstallationBundle::reopen_return(
            original,
            boundary,
            &reference,
            &user,
            &mut journal,
        )
        .unwrap()
        .verify(&user)
        .unwrap();
        assert_eq!(
            journal.generation, before,
            "dynamic receipt reopen must not write or replay"
        );
    }
}

// 检查首次稍后副本在创建、写入、刷新或回执前中断且尚无完整计划文件时，同一事务仍可安全返回。
#[test]
fn BundleReturn_FirstCopySeed_004() {
    for (fault, missing_known_root) in [
        (CopyFault::BeforeCreate, false),
        (CopyFault::AfterWrite, false),
        (CopyFault::AfterFlush, false),
        (CopyFault::BeforeReceipt, false),
        (CopyFault::AfterWrite, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let parent = Directory::open_absolute(temp.path()).unwrap();
        let install = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("install").unwrap(), &user)
                .unwrap(),
        );
        let data = Arc::new(
            PrivateDirectory::create_new(parent.clone(), component("data").unwrap(), &user)
                .unwrap(),
        );
        let records = Arc::new(
            PrivateDirectory::create_new(parent, component("records").unwrap(), &user).unwrap(),
        );
        drop(
            ManagerRecord::create(install.clone(), "cc-desk.exe", &"original image", &user)
                .unwrap(),
        );
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
        let mut journal =
            ContextJournal::new(&mut store, records.clone(), &lease, binding.clone(), 0).unwrap();
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
        std::fs::write(
            temp.path().join("install/later.bin"),
            b"later installation data",
        )
        .unwrap();
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
        let probe = probe_copy_failure(fault);
        assert!(
            BundleRestoration::prepare(original.clone(), boundary, &user, &mut journal).is_err()
        );
        drop(probe);
        assert!(
            !temp
                .path()
                .join("data/installation-return-plan.json")
                .exists(),
            "interrupted first copy must not claim a completed plan"
        );
        assert!(
            journal.store.latest_bundle_seed().unwrap().is_some(),
            "exact seed must precede every first-copy effect"
        );
        let pending = journal
            .store
            .context_pending()
            .unwrap()
            .unwrap()
            .0
            .effect_id;
        let generation = journal.generation;
        drop(journal);
        drop(store);
        std::fs::write(
            temp.path().join("install/arrived-after-first-copy.bin"),
            b"new user data after first copy failed",
        )
        .unwrap();
        if matches!(fault, CopyFault::AfterWrite | CopyFault::AfterFlush) {
            // The file mutation has no Applied receipt. Keep its actual bytes
            // as unknown history; an Applied root entry still stays exact.
            std::fs::write(
                temp.path().join("data").join(LATER_COPY).join("later.bin"),
                b"changed unresolved partial bytes",
            )
            .unwrap();
            std::fs::write(
                temp.path()
                    .join("data")
                    .join(LATER_COPY)
                    .join("unknown-companion.bin"),
                b"new unresolved companion",
            )
            .unwrap();
        }
        store = JournalStore::open_windows(records.clone()).unwrap();
        store.bind_existing(&binding).unwrap();
        journal = ContextJournal::new(
            &mut store,
            records.clone(),
            &lease,
            binding.clone(),
            generation,
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
        if missing_known_root {
            std::fs::rename(
                temp.path().join("data").join(LATER_COPY),
                temp.path().join("displaced-applied-root"),
            )
            .unwrap();
            assert!(
                InterruptedInstallationReturn::reopen_pending(
                    original.clone(),
                    boundary,
                    &user,
                    &mut journal
                )
                .is_err(),
                "a partial copy still requires its Applied root object"
            );
            assert_eq!(journal.generation, generation);
            assert_eq!(
                std::fs::read(temp.path().join("install/arrived-after-first-copy.bin")).unwrap(),
                b"new user data after first copy failed"
            );
            continue;
        }
        let observed = InterruptedInstallationReturn::reopen_pending(
            original.clone(),
            boundary,
            &user,
            &mut journal,
        )
        .unwrap();
        let mut fresh = observed.prepare_fresh_attempt(&user, &mut journal).unwrap();
        assert_eq!(
            journal
                .store
                .inspect(&binding)
                .unwrap()
                .last_valid
                .unwrap()
                .effect_observation(&pending),
            Some(Observation::Unknown),
            "first uncertain copy must stay historical Unknown"
        );
        let digest = journal.retain(&"fixture retained later context").unwrap();
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
                .begin(
                    kind,
                    &"fixture independent subsystem",
                    &"fixture independent subsystem",
                )
                .unwrap();
            journal
                .applied(pending, &"fixture independent subsystem")
                .unwrap();
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
        let later_path = temp
            .path()
            .join("data")
            .join(fresh.plan.attempt.name(LATER_COPY).unwrap());
        let receipt = fresh.restore(&user, &mut journal).unwrap();
        if matches!(fault, CopyFault::AfterWrite | CopyFault::AfterFlush) {
            assert_eq!(
                std::fs::read(temp.path().join("data").join(LATER_COPY).join("later.bin")).unwrap(),
                b"changed unresolved partial bytes"
            );
            assert_eq!(
                std::fs::read(
                    temp.path()
                        .join("data")
                        .join(LATER_COPY)
                        .join("unknown-companion.bin")
                )
                .unwrap(),
                b"new unresolved companion"
            );
        }
        receipt.verify(&user).unwrap();
        assert_eq!(
            std::fs::read(later_path.join("later.bin")).unwrap(),
            b"later installation data"
        );
        assert_eq!(
            std::fs::read(later_path.join("arrived-after-first-copy.bin")).unwrap(),
            b"new user data after first copy failed"
        );
        assert_eq!(
            std::fs::read(temp.path().join("install/cc-desk.exe")).unwrap(),
            b"\"original image\""
        );
        assert_eq!(
            receipt.result.history.len(),
            1,
            "partial original attempt must remain represented and held"
        );
        assert_eq!(
            journal
                .store
                .inspect(&binding)
                .unwrap()
                .last_valid
                .unwrap()
                .effect_observation(&pending),
            Some(Observation::Unknown)
        );
    }
}

// 检查私有父目录不能掩盖安装文件对 Everyone 的读取权限，普通和排他映像句柄均拒绝此保留方式。
#[test]
fn BundleReturn_ConfidentialObjects_005() {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(parent, component("private-install").unwrap(), &user).unwrap(),
    );
    drop(
        ManagerRecord::create(root.clone(), "cc-desk.exe", &"private source image", &user).unwrap(),
    );
    let path: Vec<u16> = temp
        .path()
        .join("private-install/cc-desk.exe")
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let sddl: Vec<u16> = format!("D:P(A;;FA;;;{})(A;;FR;;;WD)", user.sid_text())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .unwrap();
        assert!(SetFileSecurityW(
            PCWSTR(path.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor
        )
        .as_bool());
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    root.verify(&user).unwrap();
    let tree = HeldTree::admit(
        HeldRoot::Present(root.directory().clone()),
        &mut Budget::new(SnapshotLimits::default()).unwrap(),
        None,
    )
    .unwrap();
    assert!(
        verify_bundle_confidential(&tree, &user).is_err(),
        "private parent alone must not authorize retention of broadly readable objects"
    );
    drop(tree);
    let file = root
        .directory()
        .open_file(component("cc-desk.exe").unwrap(), FileAccess::Read)
        .unwrap();
    let id = file.identity().clone();
    let digest = file.digest().unwrap();
    drop(file);
    let fence = Arc::new(Mutex::new(
        ImageFence::acquire(
            root.directory().clone(),
            component("cc-desk.exe").unwrap(),
            &id,
            &digest,
        )
        .unwrap(),
    ));
    let bundle = HeldBundle::capture(
        root.directory().clone(),
        component("cc-desk.exe").unwrap(),
        fence,
        SnapshotLimits::default(),
    )
    .unwrap();
    assert!(
        verify_bundle_confidential(bundle.tree(), &user).is_err(),
        "exclusive fence must check its actual image ACL too"
    );
}

// 检查完整安装恢复允许系统正向设置继承标记，但原始权限相等比较与其他权限字段保持严格。
#[test]
fn BundleReturn_SecurityContract_006() {
    use windows::Win32::Security::SE_DACL_AUTO_INHERITED;

    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temp.path()).unwrap();
    let root = Arc::new(
        PrivateDirectory::create_new(parent, component("contract").unwrap(), &user).unwrap(),
    );
    let tree =
        HeldTree::capture_private(root.directory().clone(), SnapshotLimits::default(), &user)
            .unwrap();
    let mut original = tree.manifest.clone();
    let PermissionRecord::Windows { descriptor, .. } =
        &mut original.entries[0].metadata.permissions
    else {
        unreachable!()
    };
    let control = u16::from_le_bytes([descriptor[2], descriptor[3]]);
    descriptor[2..4].copy_from_slice(&(control & !SE_DACL_AUTO_INHERITED.0).to_le_bytes());
    let mut restored = original.clone();
    let PermissionRecord::Windows { descriptor, .. } =
        &mut restored.entries[0].metadata.permissions
    else {
        unreachable!()
    };
    descriptor[2..4].copy_from_slice(&(control | SE_DACL_AUTO_INHERITED.0).to_le_bytes());
    assert_ne!(
        original.entries[0].metadata.permissions,
        restored.entries[0].metadata.permissions
    );
    assert_ne!(original.digest().unwrap(), restored.digest().unwrap());
    verify_logical_restore(&original, &restored).unwrap();
    assert!(verify_logical_restore(&restored, &original).is_err());
    let expected = &original.entries[0].metadata.permissions;
    let observed = &restored.entries[0].metadata.permissions;
    assert!(restored_permissions_match(expected, observed));
    let PermissionRecord::Windows { descriptor, .. } = observed else {
        unreachable!()
    };
    for offset in 0..descriptor.len() {
        for bit in 0..8 {
            if offset == 3 && bit == 2 {
                continue;
            }
            let mut changed = restored.clone();
            let PermissionRecord::Windows { descriptor, .. } =
                &mut changed.entries[0].metadata.permissions
            else {
                unreachable!()
            };
            descriptor[offset] ^= 1 << bit;
            assert!(
                verify_logical_restore(&original, &changed).is_err(),
                "installation restoration accepted byte {offset}, bit {bit}"
            );
        }
    }
    let PermissionRecord::Windows { attributes, .. } =
        &mut restored.entries[0].metadata.permissions
    else {
        unreachable!()
    };
    *attributes ^= windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY.0;
    assert!(verify_logical_restore(&original, &restored).is_err());
}
