//! Disposable NTFS custody probes. Only unrelated subsystem journal envelopes
//! are fixtures; fresh creation, failed observations, preservation and return
//! execute the production held-object operations. No process is launched.
#![allow(non_snake_case)]

use super::*;
use crate::version_history::{
    journal::CapacityPlan, snapshot::capture_context, windows::lease::LeaseFiles,
};

struct Fixture {
    temporary: tempfile::TempDir,
    user: CurrentUser,
    records: Arc<PrivateDirectory>,
    quarantine: Arc<PrivateDirectory>,
    lease: ExclusiveLease,
    binding: JournalBinding,
    store: JournalStore,
    generation: u64,
    originals: Option<RetainedContextRoots>,
    boundary: SnapshotBoundary,
    fence: ImageFence,
}

impl Fixture {
    fn new(present_udf: bool) -> Self {
        Self::setup(present_udf, true)
    }
    fn setup(present_udf: bool, sealed: bool) -> Self {
        Self::setup_named(present_udf, sealed, "desk")
    }
    fn setup_named(present_udf: bool, sealed: bool, desk_name: &str) -> Self {
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
        let desk = PrivateDirectory::create_renameable_new(parent.clone(), name(desk_name), &user)
            .unwrap();
        std::fs::write(
            temporary.path().join(desk_name).join("state"),
            b"original desk",
        )
        .unwrap();
        let udf = present_udf.then(|| {
            PrivateDirectory::create_renameable_new(parent.clone(), name("udf"), &user).unwrap()
        });
        if present_udf {
            std::fs::write(temporary.path().join("udf/state"), b"original udf").unwrap();
        }
        let mut context = HeldContext::capture_durable(
            HeldRoot::Present(desk.directory().clone()),
            udf.as_ref().map_or_else(
                || HeldRoot::observe(parent.clone(), name("udf")).unwrap(),
                |root| HeldRoot::Present(root.directory().clone()),
            ),
            SnapshotLimits::default(),
        )
        .unwrap();
        drop(desk);
        drop(udf);
        let boundary =
            SnapshotBoundary::fixture_with_roots(binding.clone(), context.root_identities());
        let snapshot = capture_context(
            &boundary,
            &binding.source_context,
            &mut context,
            SnapshotLimits::default(),
        )
        .unwrap();
        let records =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("records"), &user).unwrap());
        let copies_root =
            Arc::new(PrivateDirectory::create_new(parent.clone(), name("copies"), &user).unwrap());
        let quarantine = Arc::new(
            PrivateDirectory::create_new(parent.clone(), name("quarantine"), &user).unwrap(),
        );
        let mut store = JournalStore::open_windows(records.clone()).unwrap();
        store
            .initialize(
                binding.clone(),
                CapacityPlan::for_effects(100, 100, 20, 4096).unwrap(),
            )
            .unwrap();
        let leases = LeaseFiles::open(records.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let lease = leases.acquire_exclusive(&control).unwrap();
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
            ContextJournal::new(&mut store, records.clone(), &lease, binding.clone(), 0).unwrap();
        let mut copies = BTreeMap::new();
        let mut readmitted = BTreeMap::new();
        for (kind, origin, retained, backup) in [
            (RootKind::Desk, desk_name, "desk-old", "desk-copy"),
            (RootKind::WebView, "udf", "udf-old", "udf-copy"),
        ] {
            let mut copy = PrivateTreeCopy::new(copies_root.clone(), name(backup));
            copy.copy_from(context.tree(kind), &user, &mut journal)
                .unwrap();
            if !context.tree(kind).manifest.entries.is_empty() {
                let proof = copy
                    .rotate_context_root(
                        &mut context,
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
        let mut generation = journal.generation();
        drop(journal);
        let originals =
            RetainedContextRoots::admit(context, copies, readmitted, snapshot, &boundary, &user)
                .unwrap();
        let digest = store
            .retain_manifest(&originals.snapshot().encode().unwrap())
            .unwrap();
        generation = store
            .append(
                generation,
                JournalEvent::Manifest {
                    role: ManifestRole::SourceContext,
                    digest,
                },
            )
            .unwrap();
        let mut journal = ContextJournal::new(
            &mut store,
            records.clone(),
            &lease,
            binding.clone(),
            generation,
        )
        .unwrap();
        if sealed {
            originals
                .record_preserved(&boundary, &fence, &user, &mut journal)
                .unwrap();
        }
        generation = journal.generation();
        drop(journal);
        // These other subsystem records supply only this context test's phase
        // preconditions. They are never source/no-launch or terminal proofs.
        for role in [
            ManifestRole::SourceBundle,
            ManifestRole::Registration,
            ManifestRole::Shortcuts,
        ] {
            let digest = store
                .retain_manifest(b"unrelated subsystem fixture")
                .unwrap();
            generation = store
                .append(generation, JournalEvent::Manifest { role, digest })
                .unwrap();
        }
        let mut journal = ContextJournal::new(
            &mut store,
            records.clone(),
            &lease,
            binding.clone(),
            generation,
        )
        .unwrap();
        for kind in [
            EffectKind::VerifySourceBundleCopy,
            EffectKind::FenceSourceImage,
        ] {
            let pending = journal.begin(kind, &"fixture", &"fixture").unwrap();
            journal.applied(pending, &"fixture").unwrap();
        }
        generation = journal.generation();
        drop(journal);
        if sealed {
            generation = store
                .append(
                    generation,
                    JournalEvent::Phase {
                        phase: JournalPhase::SourceSealed,
                    },
                )
                .unwrap();
        }
        Self {
            temporary,
            user,
            records,
            quarantine,
            lease,
            binding,
            store,
            generation,
            originals: Some(originals),
            boundary,
            fence,
        }
    }

    fn fresh(&mut self, fault: Option<CopyFault>) -> Option<FreshContextRoots> {
        let originals = self.originals.as_ref().unwrap();
        let mut fresh = FreshContextRoots::new(originals, &self.binding).unwrap();
        let mut journal = ContextJournal::new(
            &mut self.store,
            self.records.clone(),
            &self.lease,
            self.binding.clone(),
            self.generation,
        )
        .unwrap();
        let injected = fault.map(probe_copy_failure);
        assert_eq!(
            fresh
                .create(
                    originals,
                    &self.boundary,
                    &self.fence,
                    &self.user,
                    &mut journal
                )
                .is_err(),
            fault.is_some()
        );
        drop(injected);
        self.generation = journal.generation();
        Some(fresh)
    }
}

// 重开从实际已保留的后来树解配置；恢复后原槽已存在，历史absence仍须由完整副本核对。
#[test]
fn HistoryReentryExclusions_RetainedAndCompleted_001() {
    use crate::cli::environment::EnvMap;
    use crate::version_history::windows::scope::{ConfiguredExclusions, ConfiguredInventory};
    use std::ffi::OsString;
    for later_present in [false, true] {
        let mut fixture = Fixture::setup_named(true, true, ".cc-box");
        std::fs::create_dir(fixture.temporary.path().join("installation")).unwrap();
        let installation =
            Directory::open_absolute(&fixture.temporary.path().join("installation")).unwrap();
        let environment = EnvMap::from([(
            OsString::from("USERPROFILE"),
            fixture.temporary.path().as_os_str().to_owned(),
        )]);
        let mut fresh = if later_present {
            fixture.fresh(None)
        } else {
            Some(
                FreshContextRoots::new(fixture.originals.as_ref().unwrap(), &fixture.binding)
                    .unwrap(),
            )
        };
        if later_present {
            std::fs::write(fixture.temporary.path().join(".cc-box/config.json"), b"{}").unwrap();
        }
        let mut later = None;
        FreshContextRoots::observe_for_return_retaining(
            &mut fresh,
            &mut later,
            fixture.originals.as_ref().unwrap(),
            fixture.quarantine.clone(),
            &fixture.boundary,
            &fixture.user,
        )
        .unwrap();
        let expected = ConfiguredInventory::fixture_for_context(
            &mut later.as_mut().unwrap().context,
            fixture.temporary.path(),
            environment.clone(),
        )
        .unwrap()
        .into_exclusions(installation.clone(), fixture.quarantine.clone())
        .unwrap()
        .configuration_identity()
        .to_owned();
        let originals = fixture.originals.as_ref().unwrap();
        let mut journal = ContextJournal::new(
            &mut fixture.store,
            fixture.records.clone(),
            &fixture.lease,
            fixture.binding.clone(),
            fixture.generation,
        )
        .unwrap();
        later
            .as_ref()
            .unwrap()
            .admit_preinstall_return(
                originals,
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        later
            .as_mut()
            .unwrap()
            .preserve(
                originals,
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        fixture.generation = journal.generation();
        drop(journal);
        let bytes = later
            .as_ref()
            .unwrap()
            .manifest_bytes(originals, &fixture.user)
            .unwrap();
        let digest = fixture.store.retain_manifest(&bytes).unwrap();
        fixture.generation = fixture
            .store
            .append(
                fixture.generation,
                JournalEvent::Manifest {
                    role: ManifestRole::RetainedTargetContext,
                    digest,
                },
            )
            .unwrap();
        fixture.generation = fixture
            .store
            .append(
                fixture.generation,
                JournalEvent::Phase {
                    phase: JournalPhase::Restoring,
                },
            )
            .unwrap();
        let mut returning =
            ContextRestoration::new_retaining(&mut fixture.originals, &mut later).unwrap();
        assert!(ConfiguredExclusions::fixture_reopen_for_return(
            &mut returning,
            installation.clone(),
            fixture.quarantine.clone(),
            &"f".repeat(64),
            &fixture.user,
            fixture.temporary.path(),
            environment.clone(),
        )
        .is_err());
        let exclusions = ConfiguredExclusions::fixture_reopen_for_return(
            &mut returning,
            installation.clone(),
            fixture.quarantine.clone(),
            &expected,
            &fixture.user,
            fixture.temporary.path(),
            environment.clone(),
        )
        .unwrap();
        exclusions.verify_external().unwrap();
        let mut overlaps = environment.clone();
        overlaps.insert(
            OsString::from("CODEX_HOME"),
            fixture.temporary.path().join(".cc-box").into_os_string(),
        );
        assert!(ConfiguredExclusions::fixture_reopen_for_return(
            &mut returning,
            installation.clone(),
            fixture.quarantine.clone(),
            &expected,
            &fixture.user,
            fixture.temporary.path(),
            overlaps,
        )
        .is_err());
        let mut journal = ContextJournal::new(
            &mut fixture.store,
            fixture.records.clone(),
            &fixture.lease,
            fixture.binding.clone(),
            fixture.generation,
        )
        .unwrap();
        returning
            .restore(
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        drop(journal);
        assert!(fixture.temporary.path().join(".cc-box/state").exists());
        assert!(fixture.temporary.path().join("udf/state").exists());
        exclusions.verify_external().unwrap();
        let completed = ConfiguredExclusions::fixture_reopen_for_return(
            &mut returning,
            installation,
            fixture.quarantine.clone(),
            &expected,
            &fixture.user,
            fixture.temporary.path(),
            environment,
        )
        .unwrap();
        completed.verify_external().unwrap();
        std::fs::create_dir(fixture.temporary.path().join(".claude")).unwrap();
        assert!(completed.verify_external().is_err());
    }
}

// 失败的第二根读取不释放第一根及原始副本；修正观察障碍后保留同一 fresh 对象。
#[test]
fn HistoryPreinstallCustody_FailedObservationRetainsOwners_001() {
    for fault in [CopyFault::AfterFreshCreate, CopyFault::BeforeFreshReceipt] {
        let mut fixture = Fixture::new(true);
        let mut fresh = fixture.fresh(Some(fault));
        let created = match &fresh.as_ref().unwrap().attempts[&RootKind::Desk]
            .tree
            .as_ref()
            .unwrap()
            .root
        {
            HeldRoot::Present(root) => root.identity().clone(),
            _ => panic!("fresh Desk must exist"),
        };
        std::fs::write(
            fixture.temporary.path().join("desk/later"),
            b"actual partial data",
        )
        .unwrap();
        std::fs::write(fixture.temporary.path().join("udf"), b"wrong-kind blocker").unwrap();
        let mut later = None;
        assert!(FreshContextRoots::observe_for_return_retaining(
            &mut fresh,
            &mut later,
            fixture.originals.as_ref().unwrap(),
            fixture.quarantine.clone(),
            &fixture.boundary,
            &fixture.user,
        )
        .is_err());
        let retained = fresh
            .as_ref()
            .expect("partial fresh owner survives failed read");
        assert!(later.is_none());
        assert_eq!(retained.attempts.len(), 1);
        assert!(
            matches!(&retained.return_roots[&RootKind::Desk], HeldRoot::Present(root) if root.identity() == &created)
        );
        fixture
            .originals
            .as_ref()
            .unwrap()
            .verify(&fixture.user)
            .unwrap();
        assert!(std::fs::write(
            fixture.temporary.path().join("quarantine/desk-old/state"),
            b"lost custody"
        )
        .is_err());
        std::fs::remove_file(fixture.temporary.path().join("udf")).unwrap();
        FreshContextRoots::observe_for_return_retaining(
            &mut fresh,
            &mut later,
            fixture.originals.as_ref().unwrap(),
            fixture.quarantine.clone(),
            &fixture.boundary,
            &fixture.user,
        )
        .unwrap();
        assert!(fresh.is_none());
        let later = later.unwrap();
        assert!(
            matches!(&later.context.tree(RootKind::Desk).root, HeldRoot::Present(root) if root.identity() == &created)
        );
        assert!(later
            .context
            .tree(RootKind::Desk)
            .manifest
            .entries
            .iter()
            .any(|entry| entry.metadata.path == "later"));
    }
}

// source 已封存但 fresh 未开始或部分创建失败，后续实际状态独立保留，原始对象准确归位。
fn check_retained_return(present_udf: bool, create: bool, fault: Option<CopyFault>) {
    let mut fixture = Fixture::new(present_udf);
    let expected = fixture.originals.as_ref().unwrap().snapshot().clone();
    let mut fresh = if create {
        fixture.fresh(fault)
    } else {
        Some(FreshContextRoots::new(fixture.originals.as_ref().unwrap(), &fixture.binding).unwrap())
    };
    if fixture.temporary.path().join("desk").exists() {
        std::fs::write(
            fixture.temporary.path().join("desk/later"),
            b"retained later state",
        )
        .unwrap();
    }
    let mut later = None;
    FreshContextRoots::observe_for_return_retaining(
        &mut fresh,
        &mut later,
        fixture.originals.as_ref().unwrap(),
        fixture.quarantine.clone(),
        &fixture.boundary,
        &fixture.user,
    )
    .unwrap();
    let originals = fixture.originals.as_ref().unwrap();
    let mut journal = ContextJournal::new(
        &mut fixture.store,
        fixture.records.clone(),
        &fixture.lease,
        fixture.binding.clone(),
        fixture.generation,
    )
    .unwrap();
    later
        .as_ref()
        .unwrap()
        .admit_preinstall_return(
            originals,
            &fixture.boundary,
            &fixture.fence,
            &fixture.user,
            &mut journal,
        )
        .unwrap();
    later
        .as_mut()
        .unwrap()
        .preserve(
            originals,
            &fixture.boundary,
            &fixture.fence,
            &fixture.user,
            &mut journal,
        )
        .unwrap();
    fixture.generation = journal.generation();
    drop(journal);
    let bytes = later
        .as_ref()
        .unwrap()
        .manifest_bytes(originals, &fixture.user)
        .unwrap();
    let digest = fixture.store.retain_manifest(&bytes).unwrap();
    fixture.generation = fixture
        .store
        .append(
            fixture.generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest,
            },
        )
        .unwrap();
    fixture.generation = fixture
        .store
        .append(
            fixture.generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap();
    let mut restoration =
        Some(ContextRestoration::new_retaining(&mut fixture.originals, &mut later).unwrap());
    let mut journal = ContextJournal::new(
        &mut fixture.store,
        fixture.records.clone(),
        &fixture.lease,
        fixture.binding.clone(),
        fixture.generation,
    )
    .unwrap();
    restoration
        .as_mut()
        .unwrap()
        .restore(
            &fixture.boundary,
            &fixture.fence,
            &fixture.user,
            &mut journal,
        )
        .unwrap();
    let restored = ContextRestoration::finish_retaining(&mut restoration, &fixture.user).unwrap();
    assert!(restoration.is_none());
    restored.verify(&fixture.user).unwrap();
    assert_eq!(restored.original_snapshot(), &expected);
    assert_eq!(
        std::fs::read(fixture.temporary.path().join("desk/state")).unwrap(),
        b"original desk"
    );
    assert_eq!(fixture.temporary.path().join("udf").exists(), present_udf);
    assert!(!fixture.temporary.path().join("desk/later").exists());
    if create && fault != Some(CopyFault::BeforeFreshCreate) {
        assert!(restored
            .later
            .context
            .tree(RootKind::Desk)
            .manifest
            .entries
            .iter()
            .any(|entry| entry.metadata.path == "later"));
    }
}

#[test]
fn HistoryCustody_ReturnAbsentNoFresh_002() {
    check_retained_return(false, false, None);
}

#[test]
fn HistoryCustody_ReturnBeforeCreate_002() {
    check_retained_return(true, true, Some(CopyFault::BeforeFreshCreate));
}

#[test]
fn HistoryCustody_ReturnAfterCreate_002() {
    check_retained_return(true, true, Some(CopyFault::AfterFreshCreate));
}

#[test]
fn HistoryCustody_ReturnAbsentFresh_002() {
    check_retained_return(false, true, None);
}

// 元数据回执缺失或 Unknown 只重读同一保留对象；不重放 rename，也不补造 SourceSealed。
#[test]
fn HistoryPreinstallCustody_UnsealedPreservationReadback_003() {
    for unknown in [false, true] {
        let mut fixture = Fixture::setup(true, false);
        let originals = fixture.originals.as_ref().unwrap();
        let before = originals.source.root_identities();
        let mut journal = ContextJournal::new(
            &mut fixture.store,
            fixture.records.clone(),
            &fixture.lease,
            fixture.binding.clone(),
            fixture.generation,
        )
        .unwrap();
        let injected = probe_copy_failure(CopyFault::BeforeReceipt);
        assert!(originals
            .complete_preservation_observations(
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .is_err());
        drop(injected);
        let (effect, intent_generation) = journal.store.context_pending().unwrap().unwrap();
        assert!(
            matches!(&effect.kind, EffectKind::PreserveRoot { context, root: RootKind::Desk }
            if context == &fixture.binding.source_context)
        );
        if unknown {
            journal
                .unknown(PendingEffect {
                    id: effect.effect_id.clone(),
                    generation: intent_generation,
                })
                .unwrap();
        }
        originals
            .complete_preservation_observations(
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        assert_eq!(originals.source.root_identities(), before);
        let (observed, observed_generation, outcome) = journal
            .store
            .source_preservation(RootKind::Desk)
            .unwrap()
            .unwrap();
        assert_eq!(observed.effect_id, effect.effect_id);
        assert_eq!(observed_generation, intent_generation);
        assert_eq!(outcome, Some(Observation::Applied));
        assert_eq!(
            journal
                .store
                .inspect(&fixture.binding)
                .unwrap()
                .last_valid
                .unwrap()
                .phase(),
            JournalPhase::Reviewed
        );
        let mut fresh = Some(FreshContextRoots::new(originals, &fixture.binding).unwrap());
        let mut later = None;
        FreshContextRoots::observe_for_return_retaining(
            &mut fresh,
            &mut later,
            originals,
            fixture.quarantine.clone(),
            &fixture.boundary,
            &fixture.user,
        )
        .unwrap();
        later
            .as_ref()
            .unwrap()
            .admit_preinstall_return(
                originals,
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        later
            .as_mut()
            .unwrap()
            .preserve(
                originals,
                &fixture.boundary,
                &fixture.fence,
                &fixture.user,
                &mut journal,
            )
            .unwrap();
        let observed = journal
            .store
            .inspect(&fixture.binding)
            .unwrap()
            .last_valid
            .unwrap();
        assert_eq!(observed.phase(), JournalPhase::RecoveryRequired);
        assert_eq!(originals.source.root_identities(), before);
        assert!(!fixture.temporary.path().join("desk").exists());
        assert!(!fixture.temporary.path().join("udf").exists());
    }
}

// 非本次真实保留计划不能凭 source PreserveRoot 标签取得 Applied 或放行返回。
#[test]
fn HistoryPreinstallCustody_ForeignPreservationPlanBlocked_004() {
    let mut fixture = Fixture::setup(true, false);
    let originals = fixture.originals.as_ref().unwrap();
    let before = originals.source.root_identities();
    let mut journal = ContextJournal::new(
        &mut fixture.store,
        fixture.records.clone(),
        &fixture.lease,
        fixture.binding.clone(),
        fixture.generation,
    )
    .unwrap();
    let effect = journal
        .begin(
            EffectKind::PreserveRoot {
                context: fixture.binding.source_context.clone(),
                root: RootKind::Desk,
            },
            &"foreign serialized plan",
            &"foreign expectation",
        )
        .unwrap();
    let generation = journal.generation();
    assert!(originals
        .complete_preservation_observations(
            &fixture.boundary,
            &fixture.fence,
            &fixture.user,
            &mut journal,
        )
        .is_err());
    assert_eq!(journal.generation(), generation);
    assert_eq!(originals.source.root_identities(), before);
    let observed = journal
        .store
        .inspect(&fixture.binding)
        .unwrap()
        .last_valid
        .unwrap();
    assert_eq!(observed.effect_observation(&effect.id), None);
    assert_eq!(observed.pending_effect().unwrap().effect_id, effect.id);
    originals.verify(&fixture.user).unwrap();
}

// 当前上下文完整捕获后的保管检查失败仍保留当前文件句柄，不能再次捕获替换 owner。
#[test]
fn HistoryPreinstallCustody_PostCaptureFailureRetainsContext_005() {
    let mut fixture = Fixture::new(true);
    let custody = fixture
        .originals
        .as_ref()
        .unwrap()
        .retain_return_custody(&fixture.user)
        .unwrap();
    let fresh = fixture.fresh(None).unwrap();
    let originals = fixture.originals.as_ref().unwrap();
    let bytes = fresh.manifest_bytes(originals, &fixture.user).unwrap();
    let digest = fixture.store.retain_manifest(&bytes).unwrap();
    fixture.generation = fixture
        .store
        .append(
            fixture.generation,
            JournalEvent::Manifest {
                role: ManifestRole::FreshTargetContext,
                digest,
            },
        )
        .unwrap();
    fixture.generation = fixture
        .store
        .append(
            fixture.generation,
            JournalEvent::Phase {
                phase: JournalPhase::FreshReady,
            },
        )
        .unwrap();
    let mut journal = ContextJournal::new(
        &mut fixture.store,
        fixture.records.clone(),
        &fixture.lease,
        fixture.binding.clone(),
        fixture.generation,
    )
    .unwrap();
    fresh
        .verify_for_launch(originals, &fixture.user, &mut journal)
        .unwrap();
    drop(journal);
    // Production releases these create handles after the same verified launch
    // check. Keeping their DELETE access here blocks even the first root read,
    // so the post-capture failure would never be reached. No process is launched.
    drop(fresh);
    let path = fixture.temporary.path().join("desk/current-data");
    std::fs::write(&path, b"current target context").unwrap();
    let mut captured = None;
    CONTEXT_CAPTURE_FAILURE.set(true);
    let error = custody
        .capture_current_retaining(&mut captured, &fixture.user)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "injected post-capture custody verification"
    );
    assert!(!CONTEXT_CAPTURE_FAILURE.get());
    let context = captured
        .as_ref()
        .expect("completed context capture must remain owned");
    context.verify_durable().unwrap();
    let identities = context.root_identities();
    assert!(std::fs::write(&path, b"lost custody").is_err());
    assert_eq!(
        custody
            .capture_current_retaining(&mut captured, &fixture.user)
            .unwrap_err()
            .to_string(),
        "current context custody is already retained"
    );
    assert_eq!(captured.as_ref().unwrap().root_identities(), identities);
    drop(captured);
    std::fs::write(&path, b"reader released explicitly").unwrap();
    custody.verify(&fixture.user).unwrap();
}
