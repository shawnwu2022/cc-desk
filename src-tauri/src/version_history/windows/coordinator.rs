//! Dedicated-thread transaction execution. Only acquired native owners reach
//! these stages; progress projections and UI generations never grant authority.
use super::{
    context::bundle_restore::{
        BundlePreparationAttempt, BundleRestoration, RestoredInstallationBundle,
        RetainedInstallationBundle,
    },
    context::{
        ContextJournal, ContextRestoration, FreshContextRoots, HeldBundle, HeldContext, HeldRoot,
        LaterContextRoots, PrivateTreeCopy, ReadmittedRoot, RestoredContextRoots,
        RetainedContextRoots, ReturnedRoot,
    },
    coordinator_evidence::ReturnBoundary,
    durability::MarkerStore,
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess, PrivateDirectory},
    lease::{ControlLease, ExclusiveLease},
    manager_handoff::InitialManager,
    no_historical_launch::HistoricalLaunchPermit,
    package::RetainedPackage,
    preinstall_return::PreinstallReturnAttempt,
    process::{
        CancelledBeforeResume, CommandLine, DurableProcessIdentity, JobKind, PreparedProcess,
        TerminalProcessJob,
    },
    recovery_space::{AbortReserve, PartialAbortReserve},
    registration_state::{
        RegistrationJournal, RestoredRegistrationReceipt, RetainedRegistrationState,
    },
    return_boundary::{
        FailedInstallerReturnInputs, ReturnBoundaryAttempt, ReturnBoundaryInputs,
        ReturnBoundaryPreparation,
    },
    return_checkpoint::LiveReturnCheckpoint,
    scope::{ConfiguredExclusions, FencedInstallation},
    security::CurrentUser,
    shortcuts::{RetainedProductShortcuts, ShortcutJournal, ShortcutRestoreReceipt},
    source_boundary::{AdmittedSourceSnapshot, SourceSnapshotInputs},
    source_failure::{
        publish_source_abort, retain_source_partial, reverse_source_fence, SourceAbortInputs,
        SourceNoLaunch, SourceReturnInputs,
    },
    source_lifecycle::SourceHandoffTerminal,
    source_session::AcquiredSourceParts,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{
            EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore,
            ManifestRole, Observation, ObservedResult, RootKind, ShortcutSlot,
        },
        maintenance::{ActiveContextMarker, SnapshotBoundary},
        manager_types::{ManagerAction, ManagerBlockReason, ManagerStatus},
        manager_worker::{AcceptedManagerReturn, ProgressPublisher},
        payload_policy::PreservedCompanions,
        snapshot::{SnapshotLimits, SnapshotManifest},
    },
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};

#[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
use crate::version_history::acceptance::{self, AcceptanceScenario, AcceptanceStage};

#[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
fn acceptance_manifest(
    store: &JournalStore,
    binding: &JournalBinding,
    role: ManifestRole,
) -> Result<serde_json::Value, SafeError> {
    let inspection = store.inspect(binding)?;
    let state = inspection
        .last_valid
        .as_ref()
        .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
    let digest = state
        .manifest(role)
        .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
    serde_json::from_slice(&store.read_manifest(digest)?).map_err(blocked)
}

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RECOVERY_REQUIRED")
}
fn name(value: &str) -> Result<ComponentName, SafeError> {
    ComponentName::new(OsStr::new(value)).map_err(blocked)
}
fn root_name(root: RootKind) -> &'static str {
    match root {
        RootKind::Desk => "desk",
        RootKind::WebView => "webview",
    }
}

/// Read-only re-admission after a journaled same-object fence move. Location
/// changes never excuse a changed name, object identity, byte or permission.
fn readmit_source_bundle(
    prior: &HeldBundle,
    directory: Arc<Directory>,
    image_name: ComponentName,
    fence: Arc<Mutex<ImageFence>>,
    source_digest: &str,
) -> Result<HeldBundle, SafeError> {
    let observed = HeldBundle::capture(directory, image_name, fence, SnapshotLimits::default())
        .map_err(blocked)?;
    if observed.manifest().tree != prior.manifest().tree
        || observed.manifest().logical_digest().map_err(blocked)? != source_digest
    {
        return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
    }
    Ok(observed)
}

// Expands to disjoint field borrows so the source's HKEY/native owners stay on
// this thread while its separate journal/generation can be mutated explicitly.
macro_rules! source_return_inputs {
    ($source:expr) => {
        SourceReturnInputs {
            no_launch: $source
                .no_source_launch
                .as_ref()
                .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?,
            binding: &$source.parts.binding,
            installation: &$source.parts.installation,
            data: &$source.parts.data,
            terminal: &$source.parts.terminal,
            scope: &$source.parts.scope,
            fence: &$source.parts.fence,
            original_bundle: &$source.parts.original_bundle,
            registration: &$source.parts.registration,
            shortcuts: &$source.parts.shortcuts,
            exclusive: &$source.parts.exclusive,
            control: &$source.parts.control,
        }
    };
}

enum InstallerStage {
    Installed,
    ReturnRequested,
}

struct Pending {
    id: String,
    generation: u64,
}
struct TransactionOwners {
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    binding: JournalBinding,
    package: Arc<RetainedPackage>,
    payload: super::install_admission::InstallAdmission,
    companions: Option<PreservedCompanions>,
    global_custody: Option<super::startup::GlobalLeaseCustody>,
    original_bundle: Arc<RetainedInstallationBundle>,
    terminal: Arc<SourceHandoffTerminal>,
    scope: Arc<FencedInstallation>,
    fence: Arc<Mutex<ImageFence>>,
    registration: RetainedRegistrationState,
    shortcuts: RetainedProductShortcuts,
    exclusive: ExclusiveLease,
    control: ControlLease,
    store: JournalStore,
    generation: u64,
}
struct SourceExecution {
    parts: TransactionOwners,
    context: Option<HeldContext>,
    current_bundle: Option<HeldBundle>,
    exclusions: Arc<ConfiguredExclusions>,
    reserve: Option<AbortReserve>,
    partial_reserve: Option<Box<PartialAbortReserve>>,
    image_quarantine: Option<Arc<PrivateDirectory>>,
    source_quarantine: Option<Arc<PrivateDirectory>>,
    boundary: Option<SnapshotBoundary>,
    manifest: Option<SnapshotManifest>,
    copies: BTreeMap<RootKind, PrivateTreeCopy>,
    readmitted: BTreeMap<RootKind, ReadmittedRoot>,
    originals: Option<RetainedContextRoots>,
    fresh: Option<FreshContextRoots>,
    installer: Option<Arc<TerminalProcessJob>>,
    historical: Option<Arc<TerminalProcessJob>>,
    historical_launch: Option<HistoricalLaunchPermit>,
    installed: Option<HeldBundle>,
    requested_return: Option<AcceptedManagerReturn>,
    failure_recovery_started: bool,
    no_source_launch: Option<SourceNoLaunch>,
    preinstall_return: Option<PreinstallReturnAttempt>,
    reversed_roots: BTreeMap<RootKind, ReturnedRoot>,
    return_preparation: Option<ReturnBoundaryPreparation>,
    return_attempt: Option<ReturnBoundaryAttempt>,
    return_boundary: Option<Arc<ReturnBoundary>>,
    return_roots: Option<(HeldRoot, HeldRoot)>,
    later_quarantine: Option<Arc<PrivateDirectory>>,
    later: Option<LaterContextRoots>,
    bundle_preparation: Option<BundlePreparationAttempt>,
    bundle_restoration: Option<BundleRestoration>,
    restored_bundle: Option<RestoredInstallationBundle>,
    context_restoration: Option<ContextRestoration>,
    restored_context: Option<RestoredContextRoots>,
    restored_registration: Option<RestoredRegistrationReceipt>,
    restored_shortcuts: BTreeMap<ShortcutSlot, ShortcutRestoreReceipt>,
}
pub(crate) struct CoordinatorFailure {
    error: SafeError,
    source: Box<SourceExecution>,
}
impl CoordinatorFailure {
    pub(crate) fn error(&self) -> &SafeError {
        &self.error
    }
}

impl SourceExecution {
    fn new(parts: AcquiredSourceParts) -> Self {
        Self {
            context: Some(parts.context),
            current_bundle: Some(parts.current_bundle),
            exclusions: parts.exclusions,
            parts: TransactionOwners {
                installation: parts.installation,
                data: parts.data,
                binding: parts.binding,
                package: parts.package,
                payload: parts.payload,
                companions: parts.companions,
                global_custody: parts.global_custody,
                original_bundle: parts.original_bundle,
                terminal: parts.terminal,
                scope: parts.scope,
                fence: parts.fence,
                registration: parts.registration,
                shortcuts: parts.shortcuts,
                exclusive: parts.exclusive,
                control: parts.control,
                store: parts.store,
                generation: parts.generation,
            },
            reserve: None,
            partial_reserve: None,
            image_quarantine: None,
            source_quarantine: None,
            boundary: None,
            manifest: None,
            copies: BTreeMap::new(),
            readmitted: BTreeMap::new(),
            originals: None,
            fresh: None,
            installer: None,
            historical: None,
            historical_launch: None,
            installed: None,
            requested_return: None,
            failure_recovery_started: false,
            no_source_launch: None,
            preinstall_return: None,
            reversed_roots: BTreeMap::new(),
            return_preparation: None,
            return_attempt: None,
            return_boundary: None,
            return_roots: None,
            later_quarantine: None,
            later: None,
            bundle_preparation: None,
            bundle_restoration: None,
            restored_bundle: None,
            context_restoration: None,
            restored_context: None,
            restored_registration: None,
            restored_shortcuts: BTreeMap::new(),
        }
    }
    fn verify(&mut self) -> Result<(), SafeError> {
        if let Some(custody) = &self.parts.global_custody {
            custody.verify()?;
        }
        self.parts
            .data
            .verify_installation(&self.parts.installation)?;
        self.parts
            .control
            .verify_root(self.parts.installation.root())
            .map_err(blocked)?;
        self.parts
            .exclusive
            .verify_root(self.parts.installation.root())
            .map_err(blocked)?;
        self.parts.store.verify_windows_binding(
            self.parts.installation.root(),
            &self.parts.binding,
            self.parts.generation,
        )?;
        self.parts.terminal.verify(&self.parts.binding)?;
        self.parts.scope.verify().map_err(blocked)?;
        self.parts.package.verify_retained()?;
        self.parts
            .payload
            .verify_selection(self.parts.package.selection())?;
        self.parts
            .original_bundle
            .verify(&CurrentUser::capture().map_err(blocked)?)
            .map_err(blocked)?;
        self.exclusions.verify_external().map_err(blocked)?;
        Ok(())
    }
    fn checkpoint(&mut self) -> Result<(), SafeError> {
        self.verify()?;
        let checkpoint =
            ActiveContextMarker::transition_from(&self.parts.store.inspect(&self.parts.binding)?)?;
        let mut marker =
            MarkerStore::open_existing(self.parts.installation.root().clone(), &self.parts.control)
                .map_err(blocked)?
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
        if prior.binding() != &self.parts.binding || prior.is_terminal() {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        if prior.encode()? != checkpoint.encode()? {
            marker
                .append(&checkpoint, &mut self.parts.store)
                .map_err(blocked)?;
        }
        Ok(())
    }
    fn begin(
        &mut self,
        kind: EffectKind,
        before: &impl Serialize,
        expected: &impl Serialize,
    ) -> Result<Pending, SafeError> {
        self.verify()?;
        let before = self
            .parts
            .store
            .retain_manifest(&serde_json::to_vec(before).map_err(blocked)?)?;
        let expected_postconditions = self
            .parts
            .store
            .retain_manifest(&serde_json::to_vec(expected).map_err(blocked)?)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: id.clone(),
                    kind,
                    before,
                    expected_postconditions,
                },
            },
        )?;
        Ok(Pending {
            id,
            generation: self.parts.generation,
        })
    }
    fn applied(&mut self, pending: Pending, observed: &impl Serialize) -> Result<(), SafeError> {
        self.verify()?;
        let observed = self
            .parts
            .store
            .retain_manifest(&serde_json::to_vec(observed).map_err(blocked)?)?;
        let receipt =
            self.parts
                .store
                .retain_effect_receipt(&pending.id, Observation::Applied, &observed)?;
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Observed {
                effect_id: pending.id,
                intent_generation: pending.generation,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )?;
        Ok(())
    }
    fn unknown(&mut self, pending: Pending) {
        if let Ok(generation) = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Observed {
                effect_id: pending.id,
                intent_generation: pending.generation,
                result: ObservedResult {
                    observation: Observation::Unknown,
                    receipt: None,
                },
            },
        ) {
            self.parts.generation = generation;
        }
    }
    fn phase(&mut self, phase: JournalPhase) -> Result<(), SafeError> {
        self.verify()?;
        self.parts.generation = self
            .parts
            .store
            .append(self.parts.generation, JournalEvent::Phase { phase })?;
        self.checkpoint()
    }
    fn seal_and_create_fresh(&mut self, progress: &ProgressPublisher) -> Result<(), SafeError> {
        let user = CurrentUser::capture().map_err(blocked)?;
        self.verify()?;
        self.originals = Some(
            RetainedContextRoots::admit_retaining(
                &mut self.context,
                &mut self.copies,
                &mut self.readmitted,
                self.manifest
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                self.boundary
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                &user,
            )
            .map_err(blocked)?,
        );
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self
                .originals
                .as_ref()
                .expect("originals admitted")
                .record_preserved(
                    self.boundary.as_ref().expect("source boundary"),
                    &self.parts.fence.lock(),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            drop(journal);
            result.map_err(blocked)?;
        }
        self.phase(JournalPhase::SourceSealed)?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::SourceSealed,
            &self.parts.binding,
            self.parts.generation,
            || {
                Ok(
                    serde_json::json!({"binding":self.parts.binding,"context":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::SourceContext)?}),
                )
            },
        );
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        self.fresh = Some(
            FreshContextRoots::new(
                self.originals.as_ref().expect("originals admitted"),
                &self.parts.binding,
            )
            .map_err(blocked)?,
        );
        {
            self.reserve
                .as_ref()
                .expect("physical reserve")
                .verify_for(
                    &self.parts.data,
                    &self.parts.installation,
                    &self.parts.binding,
                )?;
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self.fresh.as_mut().expect("fresh attempt retained").create(
                self.originals.as_ref().expect("originals admitted"),
                self.boundary.as_ref().expect("source boundary"),
                &self.parts.fence.lock(),
                &user,
                &mut journal,
            );
            self.parts.generation = journal.generation();
            drop(journal);
            result.map_err(blocked)?;
        }
        let bytes = self
            .fresh
            .as_ref()
            .expect("fresh roots observed")
            .manifest_bytes(self.originals.as_ref().expect("originals admitted"), &user)
            .map_err(blocked)?;
        let digest = self.parts.store.retain_manifest(&bytes)?;
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Manifest {
                role: ManifestRole::FreshTargetContext,
                digest,
            },
        )?;
        self.phase(JournalPhase::FreshReady)?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::FreshReady,
            &self.parts.binding,
            self.parts.generation,
            || {
                Ok(
                    serde_json::json!({"binding":self.parts.binding,"context":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::FreshTargetContext)?}),
                )
            },
        );
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        Ok(())
    }
    fn prepare_source(&mut self, progress: &ProgressPublisher) -> Result<(), SafeError> {
        self.verify()?;
        self.exclusions
            .verify_context(self.context.as_ref().expect("durable source context"))
            .map_err(blocked)?;
        for root in [RootKind::Desk, RootKind::WebView] {
            let held = self
                .context
                .as_ref()
                .expect("durable source context")
                .tree(root)
                .root();
            let directory = match held {
                HeldRoot::Present(root) => root,
                HeldRoot::Absent { parent, .. } => parent,
            };
            directory
                .require_same_volume(self.parts.data.root().directory())
                .map_err(blocked)?;
        }
        self.checkpoint()?;
        self.reserve = match AbortReserve::create(
            self.parts.data.clone(),
            self.parts.installation.clone(),
            &self.parts.control,
            &mut self.parts.store,
            &self.parts.binding,
            self.parts.generation,
        ) {
            Ok(reserve) => Some(reserve),
            Err(failure) => {
                let error = failure.error().clone();
                self.partial_reserve = failure.into_partial();
                return Err(error);
            }
        };
        let user = CurrentUser::capture().map_err(blocked)?;
        self.image_quarantine = Some(Arc::new(
            PrivateDirectory::create_new(
                self.parts.data.root().directory().clone(),
                name("source-image")?,
                &user,
            )
            .map_err(blocked)?,
        ));
        let quarantine = self
            .image_quarantine
            .as_ref()
            .expect("owned quarantine")
            .clone();
        self.parts
            .scope
            .directory()
            .require_same_volume(quarantine.directory())
            .map_err(blocked)?;
        let before = {
            let fence = self.parts.fence.lock();
            self.parts.scope.verify_fence(&fence).map_err(blocked)?;
            let (parent, leaf) = fence.held_location().map_err(blocked)?;
            (
                fence.identity().clone(),
                parent,
                leaf.os_string().encode_wide().collect::<Vec<_>>(),
            )
        };
        let expected = (
            before.0.clone(),
            quarantine.directory().identity().clone(),
            "source-image.exe",
        );
        let pending = self.begin(EffectKind::FenceSourceImage, &before, &expected)?;
        self.reserve
            .as_ref()
            .expect("physical reserve")
            .verify_for(
                &self.parts.data,
                &self.parts.installation,
                &self.parts.binding,
            )?;
        self.exclusions
            .verify_context(self.context.as_ref().expect("durable source context"))
            .map_err(blocked)?;
        let renamed = {
            self.parts
                .fence
                .lock()
                .rename_to(quarantine.directory().clone(), name("source-image.exe")?)
        };
        if let Err(failure) = renamed {
            self.unknown(pending);
            return Err(blocked(failure));
        }
        let observed = {
            let fence = self.parts.fence.lock();
            self.parts.scope.verify_fence(&fence).map_err(blocked)?;
            let (parent, leaf) = fence.held_location().map_err(blocked)?;
            (
                fence.identity().clone(),
                parent,
                leaf.os_string().encode_wide().collect::<Vec<_>>(),
            )
        };
        self.applied(pending, &observed)?;
        // A complete observation records the image's held location as well as
        // its bytes. Re-admit the same moved fence after the journaled rename;
        // the earlier owner stays retained if any readback differs or fails.
        let readmitted_bundle = readmit_source_bundle(
            self.current_bundle.as_ref().expect("source bundle"),
            self.parts.scope.directory().clone(),
            self.parts.scope.image_name().clone(),
            self.parts.fence.clone(),
            &self.parts.binding.source_bundle,
        )?;
        self.current_bundle = Some(readmitted_bundle);
        if let Some(companions) = &self.parts.companions {
            companions
                .verify_source(self.current_bundle.as_ref().expect("source bundle"))
                .map_err(blocked)?;
        }
        let admitted = AdmittedSourceSnapshot::capture(
            SourceSnapshotInputs {
                binding: self.parts.binding.clone(),
                installation: self.parts.installation.clone(),
                data: self.parts.data.clone(),
                exclusive: &self.parts.exclusive,
                terminal: self.parts.terminal.clone(),
                scope: self.parts.scope.clone(),
                fence: self.parts.fence.clone(),
                image_quarantine: quarantine,
                original_bundle: self.parts.original_bundle.clone(),
                registration: &self.parts.registration,
                shortcuts: &self.parts.shortcuts,
            },
            self.current_bundle.as_ref().expect("source bundle"),
            self.context.as_mut().expect("durable source context"),
            &mut self.parts.store,
        )?;
        let (boundary, manifest, generation) = admitted.into_parts();
        self.boundary = Some(boundary);
        self.manifest = Some(manifest);
        self.parts.generation = generation;
        self.checkpoint()?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::check_evidence_scope(Some(&self.exclusions));
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::M0,
            &self.parts.binding,
            self.parts.generation,
            || {
                let bundle = self
                    .current_bundle
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?
                    .manifest();
                Ok(
                    serde_json::json!({"binding":self.parts.binding,"bundle":bundle,
                "bundleLogicalDigest":bundle.logical_digest().map_err(blocked)?,
                "context":self.manifest,
                "registration":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::Registration)?,
                "shortcuts":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::Shortcuts)?,
                "dataRoot":self.parts.data.root().directory().path().map_err(blocked)?.to_string_lossy(),
                "controlDirectory":self.parts.installation.root().directory().path().map_err(blocked)?.to_string_lossy()}),
                )
            },
        );
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        self.source_quarantine = Some(Arc::new(
            PrivateDirectory::create_new(
                self.parts.data.root().directory().clone(),
                name("source-context")?,
                &user,
            )
            .map_err(blocked)?,
        ));
        for kind in [RootKind::Desk, RootKind::WebView] {
            self.reserve
                .as_ref()
                .expect("physical reserve")
                .verify_for(
                    &self.parts.data,
                    &self.parts.installation,
                    &self.parts.binding,
                )?;
            self.boundary
                .as_ref()
                .expect("source boundary")
                .verify_live()?;
            self.copies.insert(
                kind,
                PrivateTreeCopy::new(
                    self.parts.data.root().clone(),
                    name(&format!("source-{}-copy", root_name(kind)))?,
                ),
            );
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let copied = self
                .copies
                .get_mut(&kind)
                .expect("copy retained")
                .copy_from(
                    self.context
                        .as_ref()
                        .expect("durable source context")
                        .tree(kind),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            drop(journal);
            copied.map_err(blocked)?;
        }
        for kind in [RootKind::Desk, RootKind::WebView] {
            let source = self
                .context
                .as_ref()
                .expect("durable source context")
                .tree(kind)
                .root();
            let HeldRoot::Present(root) = source else {
                continue;
            };
            let (parent, leaf) = root.held_location().map_err(blocked)?;
            self.reserve
                .as_ref()
                .expect("physical reserve")
                .verify_for(
                    &self.parts.data,
                    &self.parts.installation,
                    &self.parts.binding,
                )?;
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let moved = self
                .copies
                .get_mut(&kind)
                .expect("complete copy")
                .rotate_context_root(
                    self.context.as_mut().expect("durable source context"),
                    kind,
                    parent,
                    leaf,
                    self.source_quarantine
                        .as_ref()
                        .expect("source quarantine")
                        .clone(),
                    name(root_name(kind))?,
                    self.boundary.as_ref().expect("source boundary"),
                    &self.parts.fence.lock(),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            drop(journal);
            self.readmitted.insert(kind, moved.map_err(blocked)?);
        }
        self.checkpoint()?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        Ok(())
    }
}

/// Journal publication is kept separate from a mutable process lease borrow.
/// Callers retain the actual native owner for the whole operation.
fn record_intent(
    store: &mut JournalStore,
    generation: &mut u64,
    kind: EffectKind,
    before: &impl Serialize,
    expected: &impl Serialize,
) -> Result<Pending, SafeError> {
    let before = store.retain_manifest(&serde_json::to_vec(before).map_err(blocked)?)?;
    let expected_postconditions =
        store.retain_manifest(&serde_json::to_vec(expected).map_err(blocked)?)?;
    let id = uuid::Uuid::new_v4().to_string();
    *generation = store.append(
        *generation,
        JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before,
                expected_postconditions,
            },
        },
    )?;
    Ok(Pending {
        id,
        generation: *generation,
    })
}
fn record_applied(
    store: &mut JournalStore,
    generation: &mut u64,
    pending: Pending,
    observed: &[u8],
) -> Result<(), SafeError> {
    let manifest = store.retain_manifest(observed)?;
    let receipt = store.retain_effect_receipt(&pending.id, Observation::Applied, &manifest)?;
    *generation = store.append(
        *generation,
        JournalEvent::Observed {
            effect_id: pending.id,
            intent_generation: pending.generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        },
    )?;
    Ok(())
}
fn record_unknown(store: &mut JournalStore, generation: &mut u64, pending: Pending) {
    if let Ok(next) = store.append(
        *generation,
        JournalEvent::Observed {
            effect_id: pending.id,
            intent_generation: pending.generation,
            result: ObservedResult {
                observation: Observation::Unknown,
                receipt: None,
            },
        },
    ) {
        *generation = next;
    }
}
fn publish_checkpoint(
    installation: &InstallationControl,
    control: &ControlLease,
    binding: &JournalBinding,
    store: &mut JournalStore,
) -> Result<(), SafeError> {
    let checkpoint = ActiveContextMarker::transition_from(&store.inspect(binding)?)?;
    let mut marker = MarkerStore::open_existing(installation.root().clone(), control)
        .map_err(blocked)?
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
    if prior.binding() != binding || prior.is_terminal() {
        return Err(error("HISTORY_RECOVERY_REQUIRED"));
    }
    if prior.encode()? != checkpoint.encode()? {
        marker.append(&checkpoint, store).map_err(blocked)?;
    }
    Ok(())
}
fn reject_pending_commands(progress: &ProgressPublisher) -> Result<(), SafeError> {
    while let Some(command) = progress.try_command()? {
        command.finish(Err(error("HISTORY_OPERATION_PENDING")));
    }
    Ok(())
}
/// A publication, command-channel, or native observation failure is not an
/// authority to drop a resumed process. Keep every original owner on this
/// dedicated stack, including the mutable lease borrow and durable receipt.
/// Closing the application is a process-lifetime event, not recovery success.
fn park_process_failure<T>(owners: T, progress: &ProgressPublisher, failure: SafeError) -> ! {
    let _owners = owners;
    progress.fail(failure);
    loop {
        match progress.recv_command_timeout(std::time::Duration::from_secs(1)) {
            Ok(Some(command)) => command.finish(Err(error("HISTORY_RECOVERY_REQUIRED"))),
            Ok(None) => (),
            // A lost frontend cannot release or replay native custody either.
            Err(_) => std::thread::park_timeout(std::time::Duration::from_secs(1)),
        }
    }
}
struct UnstartedReturnContext<'a> {
    kind: JobKind,
    installation: &'a InstallationControl,
    control: &'a ControlLease,
    binding: &'a JournalBinding,
    store: &'a mut JournalStore,
    generation: &'a mut u64,
    user: &'a CurrentUser,
    progress: &'a ProgressPublisher,
}

/// Returns false only when cleanup is unavailable. No process is terminated
/// until a current original-document Return has been accepted. Every acquired
/// cancellation owner and command is stored on the caller's retaining stack.
fn request_unstarted_return(
    process: &mut PreparedProcess<'_>,
    receipt: Option<&DurableProcessIdentity>,
    cancelled: &mut Option<CancelledBeforeResume>,
    accepted: &mut Option<AcceptedManagerReturn>,
    context: UnstartedReturnContext<'_>,
) -> Result<bool, SafeError> {
    if !process.can_cancel_before_resume().map_err(blocked)? {
        return Ok(false);
    }
    let UnstartedReturnContext {
        kind,
        installation,
        control,
        binding,
        store,
        generation,
        user,
        progress,
    } = context;
    store.verify_windows_binding(installation.root(), binding, *generation)?;
    let (create_kind, resume_kind, terminal_kind) = match kind {
        JobKind::Installer => (
            EffectKind::InstallerCreateSuspended,
            EffectKind::InstallerResume,
            EffectKind::InstallerTerminalOutcome,
        ),
        JobKind::OrdinaryInstaller => return Err(error("HISTORY_ORDINARY_INSTALL_UNAVAILABLE")),
        JobKind::HistoricalApplication => (
            EffectKind::HistoricalCreateSuspended,
            EffectKind::HistoricalResume,
            EffectKind::HistoricalTerminalOutcome,
        ),
    };
    if let Some((effect, _)) = store.context_pending()? {
        if effect.kind != create_kind && effect.kind != resume_kind {
            return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
        }
    }
    *generation = store.append(
        *generation,
        JournalEvent::Phase {
            phase: JournalPhase::RecoveryRequired,
        },
    )?;
    publish_checkpoint(installation, control, binding, store)?;
    progress.publish(
        store,
        None,
        &[ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
    )?;
    loop {
        let command = progress.recv_command()?;
        if command.action() != ManagerAction::ReturnToPrevious {
            command.finish(Err(error("HISTORY_OPERATION_PENDING")));
            continue;
        }
        match command.accept_return(binding, store) {
            Ok(command) => {
                *accepted = Some(command);
                break;
            }
            Err(_) => continue,
        }
    }
    let command = accepted.as_ref().expect("accepted unstarted Return");
    command.verify(binding, store, *generation)?;
    if !process.can_cancel_before_resume().map_err(blocked)? {
        return Err(error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"));
    }
    // Existing typed cleanup rejects every attempted/unknown/successful resume
    // and spends its own one-attempt guard before native termination.
    process.cancel_before_resume().map_err(blocked)?;
    loop {
        if let Some(observed) = process
            .observe_cancelled_before_resume(receipt, user)
            .map_err(blocked)?
        {
            *cancelled = Some(observed);
            break;
        }
        // A completed primary alone cannot prove that its actual job is empty.
        if let Some(extra) = progress.recv_command_timeout(std::time::Duration::from_millis(200))? {
            extra.finish(Err(error("HISTORY_OPERATION_PENDING")));
        }
    }
    let proof = cancelled.as_ref().expect("retained unstarted terminal");
    proof.verify().map_err(blocked)?;
    if proof.terminal().job_kind() != kind {
        return Err(error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"));
    }
    command.verify(binding, store, *generation)?;
    if let Some((effect, intent_generation)) = store.context_pending()? {
        let (observation, bytes) = if effect.kind == create_kind {
            // The actual child existed. Never relabel creation NotApplied.
            (
                Observation::Applied,
                proof.creation_bytes().map_err(blocked)?,
            )
        } else if effect.kind == resume_kind {
            // Only this unforgeable pre-resume cancellation outcome proves the
            // original resume was never attempted; generic terminal is not enough.
            (Observation::NotApplied, proof.terminal_bytes())
        } else {
            return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
        };
        let observed = store.retain_manifest(bytes)?;
        let receipt = store.retain_effect_receipt(&effect.effect_id, observation, &observed)?;
        proof.verify().map_err(blocked)?;
        *generation = store.append(
            *generation,
            JournalEvent::Observed {
                effect_id: effect.effect_id,
                intent_generation,
                result: ObservedResult {
                    observation,
                    receipt: Some(receipt),
                },
            },
        )?;
    }
    proof.verify().map_err(blocked)?;
    let pending = record_intent(store, generation, terminal_kind, &process.launch_record(),
        &"explicit Return cleaned exact never-resumed child; actual terminal and empty authenticated job")?;
    record_applied(store, generation, pending, proof.terminal_bytes())?;
    publish_checkpoint(installation, control, binding, store)?;
    command.verify(binding, store, *generation)?;
    #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
    if kind == JobKind::Installer {
        acceptance::observe(
            AcceptanceStage::CancelledBeforeResume,
            binding,
            *generation,
            || {
                Ok(
                    serde_json::json!({"binding":binding,"emptyOwnedJob":true,"resumeAttempted":false,
                "terminal":serde_json::from_slice::<serde_json::Value>(proof.terminal_bytes()).map_err(blocked)?}),
                )
            },
        );
    }
    progress.publish(store, None, &[ManagerAction::Refresh])?;
    Ok(true)
}

/// A failed publication/resume never releases a live native owner early. Waits
/// observe the original process handle and exact owned job; time grants no proof.
fn await_terminal(
    process: &PreparedProcess<'_>,
    receipt: &DurableProcessIdentity,
    user: &CurrentUser,
    progress: &ProgressPublisher,
) -> Result<(Arc<TerminalProcessJob>, u32), SafeError> {
    loop {
        reject_pending_commands(progress)?;
        if let Some(terminal) = process.wait_terminal(200).map_err(blocked)? {
            if process.active_processes().map_err(blocked)? == 0 {
                let guard = process
                    .observe_terminal_guard(receipt, user)
                    .map_err(blocked)?
                    .ok_or_else(|| error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"))?;
                return Ok((Arc::new(guard), terminal.exit_code()));
            }
            // An exited primary handle is immediately signaled. Yield while
            // descendants remain; this timeout grants no terminal evidence.
            if let Some(command) =
                progress.recv_command_timeout(std::time::Duration::from_millis(200))?
            {
                command.finish(Err(error("HISTORY_OPERATION_PENDING")));
            }
        }
    }
}

impl SourceExecution {
    /// A distinct one-shot handoff. Complete source backups and fresh roots are
    /// admitted by the same executors, but no installed-output/Return claim is made.
    fn handoff_ordinary(
        &mut self,
        owner: &InitialManager,
        progress: &ProgressPublisher,
    ) -> Result<(), SafeError> {
        if !self.parts.payload.is_ordinary() || !self.parts.installation.is_ordinary_backup() {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        self.originals
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?
            .verify(&user)
            .map_err(blocked)?;
        self.parts.original_bundle.verify(&user).map_err(blocked)?;
        self.reserve
            .as_ref()
            .ok_or_else(|| error("HISTORY_CAPACITY"))?
            .verify_for(
                &self.parts.data,
                &self.parts.installation,
                &self.parts.binding,
            )?;
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self
                .fresh
                .as_ref()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?
                .verify_for_launch(
                    self.originals.as_ref().expect("sealed originals"),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            result.map_err(blocked)?;
        }
        let backup_location = self.parts.installation.ordinary_backup_location()?;
        // Readers are released only after durable full backup admission. The
        // protected source copies remain present for manual restoration.
        drop(self.fresh.take());
        drop(self.current_bundle.take());
        self.phase(JournalPhase::Installing)?;
        progress.publish_ordinary(&mut self.parts.store, Some(&backup_location), false, None)?;
        let image = self.parts.package.installer_image()?;
        let command = CommandLine::ordinary_nsis(
            &super::manager_process::launch_path(image.raw()).map_err(blocked)?,
            &image,
            self.parts.scope.original_path().as_os_str(),
        )
        .map_err(blocked)?;
        self.no_source_launch
            .as_mut()
            .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?
            .invalidate_before_process_intent();
        let pending = self.begin(
            EffectKind::InstallerCreateSuspended,
            &(
                self.parts.package.record_digest(),
                self.parts.scope.directory().identity().clone(),
            ),
            &(JobKind::OrdinaryInstaller, command.text()),
        )?;
        let mut process = match PreparedProcess::create_suspended_from_manager(
            image,
            command,
            JobKind::OrdinaryInstaller,
            self.parts.data.root().clone(),
            &user,
            &mut self.parts.exclusive,
            owner.child.manager_job(),
        ) {
            Ok(process) => process,
            Err(failure) => {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, pending);
                return Err(blocked(failure));
            }
        };
        // Persisted creation precedes the resume intent. Any uncertainty retains
        // every owner on this worker; it never turns into replay or target success.
        let mut receipt = None;
        let outcome = (|| {
            receipt = Some(process.persist_identity(&user).map_err(blocked)?);
            let receipt = receipt.as_ref().expect("ordinary process identity");
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                pending,
                receipt.record_bytes().map_err(blocked)?,
            )?;
            let resume = record_intent(
                &mut self.parts.store,
                &mut self.parts.generation,
                EffectKind::InstallerResume,
                &process.launch_record(),
                &"one-shot ordinary signed installer handoff",
            )?;
            self.parts.package.verify_retained()?;
            self.parts.original_bundle.verify(&user).map_err(blocked)?;
            self.originals
                .as_ref()
                .expect("sealed originals")
                .verify(&user)
                .map_err(blocked)?;
            if let Err(failure) = process.ordinary_prepare_resume(receipt) {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, resume);
                return Err(blocked(failure));
            }
            // The independently admitted global installation lease protects the
            // source through backup and final prepare, then releases immediately
            // before the exact normal installer resume so its target can start.
            self.parts
                .global_custody
                .take()
                .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?
                .release_at_installer_handoff()?;
            if let Err(failure) = process.ordinary_resume_prepared(receipt) {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, resume);
                return Err(blocked(failure));
            }
            process
                .verify_ordinary_launch(receipt, &user)
                .map_err(blocked)?;
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                resume,
                &serde_json::to_vec(&process.launch_record()).map_err(blocked)?,
            )?;
            publish_checkpoint(
                &self.parts.installation,
                &self.parts.control,
                &self.parts.binding,
                &mut self.parts.store,
            )?;
            progress.publish_ordinary(&mut self.parts.store, Some(&backup_location), true, None)?;
            Ok::<_, SafeError>(())
        })();
        if let Err(failure) = outcome {
            progress.fail_ordinary(failure, ManagerBlockReason::InstallerOutcomeUnknown);
            let _owners = (process, receipt);
            loop {
                match progress.recv_command_timeout(std::time::Duration::from_secs(1)) {
                    Ok(Some(command)) => command.finish(Err(error("HISTORY_RECOVERY_REQUIRED"))),
                    Ok(None) => (),
                    Err(_) => std::thread::park_timeout(std::time::Duration::from_secs(1)),
                }
            }
        }
        // Zero-kill lifetime was read back and exact resume receipt admitted.
        // Normal installer interaction owns completion; backups are not deleted.
        drop(process);
        Ok(())
    }
    fn install(
        &mut self,
        owner: &InitialManager,
        progress: &ProgressPublisher,
    ) -> Result<InstallerStage, SafeError> {
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        if self.historical_launch.is_some() {
            return Err(error("HISTORY_OPERATION_PENDING"));
        }
        self.historical_launch = Some(HistoricalLaunchPermit::acquire(
            self.parts.binding.clone(),
            self.parts.installation.clone(),
            self.parts.data.clone(),
            &self.parts.exclusive,
            self.parts.terminal.clone(),
            self.parts.scope.clone(),
            &mut self.parts.store,
            self.parts.generation,
        )?);
        // Fresh roots are durably recorded before dropping their create handles.
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self
                .fresh
                .as_ref()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?
                .verify_for_launch(
                    self.originals.as_ref().expect("sealed originals"),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            result.map_err(blocked)?;
        }
        drop(self.fresh.take());
        // The retained complete return copy survives. Ordinary source bundle
        // readers must be gone before NSIS can replace its companion files.
        drop(self.current_bundle.take());
        self.reserve
            .as_ref()
            .expect("physical reserve")
            .verify_for(
                &self.parts.data,
                &self.parts.installation,
                &self.parts.binding,
            )?;
        self.phase(JournalPhase::Installing)?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        let image = self.parts.package.installer_image()?;
        let command = CommandLine::nsis(
            &image.path().map_err(blocked)?,
            self.parts.scope.original_path().as_os_str(),
        )
        .map_err(blocked)?;
        self.no_source_launch
            .as_mut()
            .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?
            .invalidate_before_process_intent();
        let before = (
            self.parts.package.record_digest(),
            self.parts.scope.directory().identity().clone(),
        );
        let pending = self.begin(
            EffectKind::InstallerCreateSuspended,
            &before,
            &(JobKind::Installer, command.text()),
        )?;
        let manager_job = owner.child.manager_job();
        let mut process = match PreparedProcess::create_suspended_from_manager(
            image,
            command,
            JobKind::Installer,
            self.parts.data.root().clone(),
            &user,
            &mut self.parts.exclusive,
            manager_job,
        ) {
            Ok(process) => process,
            Err(failure) => {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, pending);
                return Err(blocked(failure));
            }
        };
        let mut receipt = None;
        let mut terminal_owner = None;
        let outcome = (|| {
            receipt = Some(process.persist_identity(&user).map_err(blocked)?);
            let receipt = receipt.as_ref().expect("retained process identity");
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                pending,
                receipt.record_bytes().map_err(blocked)?,
            )?;

            #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
            {
                acceptance::observe(
                    AcceptanceStage::InstallerSuspended,
                    &self.parts.binding,
                    self.parts.generation,
                    || {
                        Ok(serde_json::json!({"binding":self.parts.binding,
                        "creation":serde_json::from_slice::<serde_json::Value>(receipt.record_bytes().map_err(blocked)?).map_err(blocked)?,
                        "resumeAttempted":false}))
                    },
                );
                if acceptance::scenario() == AcceptanceScenario::BeforeInstallerResume {
                    acceptance::observe(
                        AcceptanceStage::InjectedPreResumeFailure,
                        &self.parts.binding,
                        self.parts.generation,
                        || {
                            Ok(
                                serde_json::json!({"binding":self.parts.binding,"resumeAttempted":false,"injection":"before-installer-resume"}),
                            )
                        },
                    );
                }
                // The sole intentional failure is inside this already-owned
                // outcome closure, after durable create and before resume intent.
                acceptance::before_installer_resume()?;
            }
            let resume = record_intent(
                &mut self.parts.store,
                &mut self.parts.generation,
                EffectKind::InstallerResume,
                &process.launch_record(),
                &"resume exact owned installer once",
            )?;
            self.parts.package.verify_retained()?;
            if let Err(failure) = process.resume(receipt) {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, resume);
                return Err(blocked(failure));
            }
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                resume,
                receipt.record_bytes().map_err(blocked)?,
            )?;
            let terminal_intent = record_intent(
                &mut self.parts.store,
                &mut self.parts.generation,
                EffectKind::InstallerTerminalOutcome,
                &process.launch_record(),
                &"exact terminal process and empty owned job",
            )?;
            let (terminal, exit_code) = await_terminal(&process, receipt, &user, progress)?;
            terminal_owner = Some(terminal);
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                terminal_intent,
                terminal_owner
                    .as_ref()
                    .expect("retained terminal")
                    .terminal_bytes(),
            )?;
            Ok::<_, SafeError>(exit_code)
        })();
        let mut cancelled = None;
        let mut accepted = None;
        let exit_code = match outcome {
            Ok(exit_code) => Some(exit_code),
            Err(failure) => {
                let cleanup = request_unstarted_return(
                    &mut process,
                    receipt.as_ref(),
                    &mut cancelled,
                    &mut accepted,
                    UnstartedReturnContext {
                        kind: JobKind::Installer,
                        installation: &self.parts.installation,
                        control: &self.parts.control,
                        binding: &self.parts.binding,
                        store: &mut self.parts.store,
                        generation: &mut self.parts.generation,
                        user: &user,
                        progress,
                    },
                );
                match cleanup {
                    Ok(true) => {
                        terminal_owner = Some(Arc::new(
                            cancelled
                                .take()
                                .expect("verified unstarted installer")
                                .into_terminal(),
                        ));
                        None
                    }
                    outcome => {
                        let failure = outcome.err().unwrap_or(failure);
                        if let Some(command) = accepted.take() {
                            command.finish(Err(failure.clone()));
                        }
                        park_process_failure(
                            (&process, &receipt, &terminal_owner, &cancelled, owner),
                            progress,
                            failure,
                        )
                    }
                }
            }
        };
        // Every fallible process operation above keeps the exact process/job,
        // receipt, and exclusive lease borrow alive on failure.
        drop(process);
        self.installer = terminal_owner;
        self.requested_return = accepted;
        self.checkpoint()?;
        let Some(exit_code) = exit_code else {
            return Ok(InstallerStage::ReturnRequested);
        };
        if exit_code != 0 {
            return Err(error("HISTORY_INSTALLER_FAILED"));
        }
        self.verify_target(progress)?;
        Ok(InstallerStage::Installed)
    }
    fn verify_target(&mut self, progress: &ProgressPublisher) -> Result<(), SafeError> {
        self.verify()?;
        self.installer
            .as_ref()
            .ok_or_else(|| error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"))?
            .verify()
            .map_err(blocked)?;
        let image = self
            .parts
            .scope
            .directory()
            .open_file(self.parts.scope.image_name().clone(), FileAccess::Read)
            .map_err(blocked)?;
        let identity = image.identity().clone();
        let digest = image.digest().map_err(blocked)?;
        drop(image);
        // A real image fence rejects any early/unmanaged target instance.
        let fence = Arc::new(Mutex::new(
            ImageFence::acquire(
                self.parts.scope.directory().clone(),
                self.parts.scope.image_name().clone(),
                &identity,
                &digest,
            )
            .map_err(blocked)?,
        ));
        let target = HeldBundle::capture(
            self.parts.scope.directory().clone(),
            self.parts.scope.image_name().clone(),
            fence,
            SnapshotLimits::default(),
        )
        .map_err(blocked)?;
        self.parts
            .payload
            .verify_installed(target.tree(), &self.parts.companions)?
            .verify()?;
        let before = (
            self.parts.payload.inventory_digest().to_owned(),
            self.parts.package.record_digest(),
        );
        let pending = self.begin(EffectKind::VerifyTargetBundle,
            &before,
            &"complete actual installed inventory equals reviewed payload plus preserved companions")?;
        self.parts
            .payload
            .verify_installed(target.tree(), &self.parts.companions)?
            .verify()?;
        self.applied(pending, target.manifest())?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::TargetVerified,
            &self.parts.binding,
            self.parts.generation,
            || {
                Ok(
                    serde_json::json!({"binding":self.parts.binding,"bundle":target.manifest(),
                "bundleLogicalDigest":target.manifest().logical_digest().map_err(blocked)?}),
                )
            },
        );
        // No target bytes are edited after release. Historical launch reopens
        // the same actual executable identity and rechecks the measured bundle.
        let measured = target.manifest().tree.clone();
        drop(target);
        let installed = HeldBundle::capture_installed(
            self.parts.scope.directory().clone(),
            self.parts.scope.image_name().clone(),
            SnapshotLimits::default(),
        )
        .map_err(blocked)?;
        if installed.manifest().tree != measured {
            return Err(error("HISTORY_PAYLOAD_CHANGED"));
        }
        self.parts
            .payload
            .verify_installed(installed.tree(), &self.parts.companions)?
            .verify()?;
        self.installed = Some(installed);
        self.phase(JournalPhase::InstalledUnconfirmed)?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        Ok(())
    }
}

impl SourceExecution {
    fn launch_and_wait_for_return(
        &mut self,
        owner: &InitialManager,
        progress: &ProgressPublisher,
    ) -> Result<AcceptedManagerReturn, SafeError> {
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        self.installer
            .as_ref()
            .expect("observed installer")
            .verify()
            .map_err(blocked)?;
        let installed = self
            .installed
            .as_ref()
            .ok_or_else(|| error("HISTORY_PAYLOAD_UNVERIFIED"))?;
        self.parts
            .payload
            .verify_installed(installed.tree(), &self.parts.companions)?
            .verify()?;
        let image = self
            .parts
            .scope
            .directory()
            .open_file(self.parts.scope.image_name().clone(), FileAccess::Read)
            .map_err(blocked)?;
        let command = CommandLine::historical(&image.path().map_err(blocked)?).map_err(blocked)?;
        self.historical_launch
            .as_ref()
            .ok_or_else(|| error("HISTORY_NO_HISTORICAL_LAUNCH_CHANGED"))?
            .begin_historical_creation()?;
        let pending = self.begin(
            EffectKind::HistoricalCreateSuspended,
            &(image.identity(), image.digest().map_err(blocked)?),
            &(JobKind::HistoricalApplication, command.text()),
        )?;
        let mut process = match PreparedProcess::create_suspended_from_manager(
            image,
            command,
            JobKind::HistoricalApplication,
            self.parts.data.root().clone(),
            &user,
            &mut self.parts.exclusive,
            owner.child.manager_job(),
        ) {
            Ok(process) => process,
            Err(failure) => {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, pending);
                return Err(blocked(failure));
            }
        };
        let mut receipt = None;
        let mut terminal_owner = None;
        let launched = (|| {
            receipt = Some(process.persist_identity(&user).map_err(blocked)?);
            let receipt = receipt.as_ref().expect("retained process identity");
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                pending,
                receipt.record_bytes().map_err(blocked)?,
            )?;
            let resume = record_intent(
                &mut self.parts.store,
                &mut self.parts.generation,
                EffectKind::HistoricalResume,
                &process.launch_record(),
                &"resume exact historical child once",
            )?;
            self.parts
                .payload
                .verify_installed(
                    self.installed
                        .as_ref()
                        .expect("held installed bundle")
                        .tree(),
                    &self.parts.companions,
                )?
                .verify()?;
            if let Err(failure) = process.resume(receipt) {
                record_unknown(&mut self.parts.store, &mut self.parts.generation, resume);
                return Err(blocked(failure));
            }
            record_applied(
                &mut self.parts.store,
                &mut self.parts.generation,
                resume,
                receipt.record_bytes().map_err(blocked)?,
            )?;
            publish_checkpoint(
                &self.parts.installation,
                &self.parts.control,
                &self.parts.binding,
                &mut self.parts.store,
            )?;
            #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
            acceptance::observe(
                AcceptanceStage::HistoricalLaunched,
                &self.parts.binding,
                self.parts.generation,
                || {
                    Ok(
                        serde_json::json!({"binding":self.parts.binding,"creation":serde_json::from_slice::<serde_json::Value>(receipt.record_bytes().map_err(blocked)?).map_err(blocked)?,"resumeApplied":true}),
                    )
                },
            );
            let mut confirmed = false;
            progress.publish(
                &mut self.parts.store,
                Some(ManagerBlockReason::SessionsNotQuiescent),
                &[
                    ManagerAction::Refresh,
                    ManagerAction::ConfirmHistoricalVersion,
                ],
            )?;
            loop {
                // Neither timeout nor a terminated primary process proves all
                // App/WebView/CLI descendants have left the actual private job.
                let primary_ended = process.wait_terminal(200).map_err(blocked)?.is_some();
                if primary_ended && process.active_processes().map_err(blocked)? == 0 {
                    let pending = record_intent(
                        &mut self.parts.store,
                        &mut self.parts.generation,
                        EffectKind::HistoricalTerminalOutcome,
                        &process.launch_record(),
                        &"same historical process terminal and authenticated job empty",
                    )?;
                    terminal_owner = Some(Arc::new(
                        process
                            .observe_terminal_guard(receipt, &user)
                            .map_err(blocked)?
                            .ok_or_else(|| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?,
                    ));
                    record_applied(
                        &mut self.parts.store,
                        &mut self.parts.generation,
                        pending,
                        terminal_owner
                            .as_ref()
                            .expect("retained terminal")
                            .terminal_bytes(),
                    )?;
                    return Ok::<_, SafeError>(());
                }
                let command = if primary_ended {
                    progress.recv_command_timeout(std::time::Duration::from_millis(200))?
                } else {
                    progress.try_command()?
                };
                let Some(command) = command else {
                    continue;
                };
                let admission = (|| {
                    command.check(&self.parts.binding, &self.parts.store)?;
                    if command.action() != ManagerAction::ConfirmHistoricalVersion || confirmed {
                        return Err(error("HISTORY_SESSIONS_NOT_QUIESCENT"));
                    }
                    if process.try_terminal().map_err(blocked)?.is_some()
                        || process.active_processes().map_err(blocked)? == 0
                    {
                        return Err(error("HISTORY_SOURCE_EXIT_UNCONFIRMED"));
                    }
                    self.parts
                        .payload
                        .verify_installed(
                            self.installed.as_ref().expect("held payload").tree(),
                            &self.parts.companions,
                        )?
                        .verify()?;
                    command.check(&self.parts.binding, &self.parts.store)
                })();
                if let Err(failure) = admission {
                    command.finish(Err(failure));
                    continue;
                }
                // Once journaling starts, any failure parks the original
                // native owners; this command can never retry a partial ACK.
                let acknowledged = (|| {
                    let pending = record_intent(&mut self.parts.store, &mut self.parts.generation,
                        EffectKind::ConfirmFirstLaunch, &process.launch_record(),
                        &"explicit authenticated manager acknowledgment of launched historical version")?;
                    record_applied(
                        &mut self.parts.store,
                        &mut self.parts.generation,
                        pending,
                        receipt.record_bytes().map_err(blocked)?,
                    )?;
                    self.parts.generation = self.parts.store.append(
                        self.parts.generation,
                        JournalEvent::Phase {
                            phase: JournalPhase::HistoricalActive,
                        },
                    )?;
                    publish_checkpoint(
                        &self.parts.installation,
                        &self.parts.control,
                        &self.parts.binding,
                        &mut self.parts.store,
                    )?;
                    progress.publish(
                        &mut self.parts.store,
                        Some(ManagerBlockReason::SessionsNotQuiescent),
                        &[ManagerAction::Refresh],
                    )
                })();
                let failure = acknowledged.as_ref().err().cloned();
                command.finish(acknowledged);
                if let Some(failure) = failure {
                    return Err(failure);
                }
                confirmed = true;
            }
        })();
        let mut cancelled = None;
        let mut accepted = None;
        if let Err(failure) = launched {
            let cleanup = request_unstarted_return(
                &mut process,
                receipt.as_ref(),
                &mut cancelled,
                &mut accepted,
                UnstartedReturnContext {
                    kind: JobKind::HistoricalApplication,
                    installation: &self.parts.installation,
                    control: &self.parts.control,
                    binding: &self.parts.binding,
                    store: &mut self.parts.store,
                    generation: &mut self.parts.generation,
                    user: &user,
                    progress,
                },
            );
            match cleanup {
                Ok(true) => {
                    terminal_owner = Some(Arc::new(
                        cancelled
                            .take()
                            .expect("verified unstarted historical child")
                            .into_terminal(),
                    ));
                }
                outcome => {
                    let failure = outcome.err().unwrap_or(failure);
                    if let Some(command) = accepted.take() {
                        command.finish(Err(failure.clone()));
                    }
                    park_process_failure(
                        (&process, &receipt, &terminal_owner, &cancelled, owner),
                        progress,
                        failure,
                    )
                }
            }
        }
        drop(process);
        // Releasing the process image plus complete target readers is necessary
        // before the later factory obtains its own exclusive image fence.
        drop(self.installed.take());
        self.historical = terminal_owner;
        self.requested_return = accepted;
        self.checkpoint()?;
        if let Some(command) = self.requested_return.as_ref() {
            command.verify(
                &self.parts.binding,
                &self.parts.store,
                self.parts.generation,
            )?;
            return Ok(self
                .requested_return
                .take()
                .expect("retained accepted Return"));
        }
        progress.publish(
            &mut self.parts.store,
            None,
            &[ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
        )?;
        loop {
            let command = progress.recv_command()?;
            if let Err(failure) = command.check(&self.parts.binding, &self.parts.store) {
                command.finish(Err(failure));
                continue;
            }
            if command.action() == ManagerAction::ReturnToPrevious {
                return command.accept_return(&self.parts.binding, &self.parts.store);
            }
            command.finish(Err(error("HISTORY_OPERATION_PENDING")));
        }
    }
}

impl SourceExecution {
    fn recover_source_failure(&mut self, progress: &ProgressPublisher) -> Result<(), SafeError> {
        if self.failure_recovery_started {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        self.failure_recovery_started = true;
        if self.return_attempt.is_some()
            || self.return_boundary.is_some()
            || self.preinstall_return.is_some()
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        source_return_inputs!(self).verify(&mut self.parts.store, self.parts.generation)?;
        self.enter_failure_phase()?;
        progress.publish(
            &mut self.parts.store,
            None,
            &[ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
        )?;
        loop {
            let command = progress.recv_command()?;
            if let Err(failure) = command.check(&self.parts.binding, &self.parts.store) {
                command.finish(Err(failure));
                continue;
            }
            if command.action() != ManagerAction::ReturnToPrevious {
                command.finish(Err(error("HISTORY_OPERATION_PENDING")));
                continue;
            }
            let command = match command.accept_return(&self.parts.binding, &self.parts.store) {
                Ok(command) => command,
                Err(_) => continue,
            };
            let result = if self.originals.is_some() {
                self.return_before_installer(&command, progress)
            } else {
                self.abort_source_before_seal(&command, progress)
            };
            let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
            command.finish(result);
            return outcome;
        }
    }
    fn release_abort_space(&mut self) -> Result<(), SafeError> {
        // A failed reserve construction is never upgraded to capacity proof.
        // Before source mutation, missing reserve is compatible with a verified
        // unchanged-source abort. Actual journal writes can still fail closed.
        if let Some(reserve) = &mut self.reserve {
            reserve.verify_for(
                &self.parts.data,
                &self.parts.installation,
                &self.parts.binding,
            )?;
            reserve.release_once(&self.parts.control)?;
        }
        Ok(())
    }
    fn return_before_installer(
        &mut self,
        command: &AcceptedManagerReturn,
        progress: &ProgressPublisher,
    ) -> Result<ManagerStatus, SafeError> {
        command.verify(
            &self.parts.binding,
            &self.parts.store,
            self.parts.generation,
        )?;
        source_return_inputs!(self).verify(&mut self.parts.store, self.parts.generation)?;
        self.release_abort_space()?;
        if self.preinstall_return.is_some() {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        // Infallible adoption precedes every fallible context recovery step.
        self.preinstall_return = Some(PreinstallReturnAttempt::new(
            self.originals.take(),
            self.fresh.take(),
        ));
        let inputs = source_return_inputs!(self);
        let boundary = self
            .boundary
            .as_ref()
            .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?;
        let attempt = self
            .preinstall_return
            .as_mut()
            .expect("retained preinstall return");
        attempt.restore(
            &inputs,
            boundary,
            &mut self.parts.store,
            &mut self.parts.generation,
        )?;
        let proof = attempt.verify_restored(
            &inputs,
            boundary,
            &mut self.parts.store,
            self.parts.generation,
        )?;
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restored,
            },
        )?;
        proof.verify(
            &inputs,
            boundary,
            &mut self.parts.store,
            self.parts.generation,
        )?;
        let terminal =
            ActiveContextMarker::restored(&self.parts.store.inspect(&self.parts.binding)?)?;
        {
            let mut marker = MarkerStore::open_existing(
                self.parts.installation.root().clone(),
                &self.parts.control,
            )
            .map_err(blocked)?
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
            if prior.binding() != &self.parts.binding || prior.is_terminal() {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
            proof.verify(
                &inputs,
                boundary,
                &mut self.parts.store,
                self.parts.generation,
            )?;
            marker
                .append(&terminal, &mut self.parts.store)
                .map_err(blocked)?;
            if marker.current().map_err(blocked)? != terminal.encode()? {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
        }
        proof.verify(
            &inputs,
            boundary,
            &mut self.parts.store,
            self.parts.generation,
        )?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])
    }
    fn abort_source_before_seal(
        &mut self,
        command: &AcceptedManagerReturn,
        progress: &ProgressPublisher,
    ) -> Result<ManagerStatus, SafeError> {
        command.verify(
            &self.parts.binding,
            &self.parts.store,
            self.parts.generation,
        )?;
        source_return_inputs!(self).verify(&mut self.parts.store, self.parts.generation)?;
        self.release_abort_space()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        if let Some((effect, _)) = self.parts.store.context_pending()? {
            if let EffectKind::PrivateBackupEntry {
                plan_generation, ..
            } = effect.kind
            {
                let copy = self.copies.values_mut().find(|copy| {
                    copy.source_plan_generation() == Some(plan_generation)
                        && !copy.has_source_rotation()
                });
                if let Some(copy) = copy {
                    let inputs = SourceAbortInputs {
                        owners: source_return_inputs!(self),
                        current_bundle: self
                            .current_bundle
                            .as_ref()
                            .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?,
                        context: self
                            .context
                            .as_ref()
                            .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                        exclusions: &self.exclusions,
                    };
                    retain_source_partial(
                        &inputs,
                        copy,
                        &mut self.parts.store,
                        &mut self.parts.generation,
                    )?;
                }
                // A nested C1 copy belongs to its original root-rotation owner
                // and is reconciled only by reverse_context_root below.
            }
        }
        for root in [RootKind::Desk, RootKind::WebView] {
            let Some(copy) = self
                .copies
                .get_mut(&root)
                .filter(|copy| copy.has_source_rotation())
            else {
                continue;
            };
            let context = self
                .context
                .as_mut()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?;
            let boundary = self
                .boundary
                .as_ref()
                .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?;
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let returned = copy.reverse_context_root(
                context,
                boundary,
                &self.parts.fence.lock(),
                &user,
                &mut journal,
            );
            self.parts.generation = journal.generation();
            drop(journal);
            self.reversed_roots.insert(root, returned.map_err(blocked)?);
        }
        {
            let inputs = SourceAbortInputs {
                owners: source_return_inputs!(self),
                current_bundle: self
                    .current_bundle
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?,
                context: self
                    .context
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                exclusions: &self.exclusions,
            };
            reverse_source_fence(&inputs, &mut self.parts.store, &mut self.parts.generation)?;
        }
        let returned_bundle = readmit_source_bundle(
            self.current_bundle
                .as_ref()
                .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?,
            self.parts.scope.directory().clone(),
            self.parts.scope.image_name().clone(),
            self.parts.fence.clone(),
            &self.parts.binding.source_bundle,
        )?;
        self.current_bundle = Some(returned_bundle);
        let inputs = SourceAbortInputs {
            owners: source_return_inputs!(self),
            current_bundle: self
                .current_bundle
                .as_ref()
                .expect("readmitted original bundle"),
            context: self
                .context
                .as_ref()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
            exclusions: &self.exclusions,
        };
        publish_source_abort(inputs, &mut self.parts.store, &mut self.parts.generation)?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])
    }
    /// A known installer terminal is distinct from historical launch. This
    /// branch requires its actual empty private job and the live unspent permit.
    fn recover_failed_installer(&mut self, progress: &ProgressPublisher) -> Result<(), SafeError> {
        if self.failure_recovery_started {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        self.failure_recovery_started = true;
        if self.return_attempt.is_some()
            || self.return_boundary.is_some()
            || self.historical.is_some()
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        self.installer
            .as_ref()
            .ok_or_else(|| error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"))?
            .verify()
            .map_err(blocked)?;
        let no_historical = self
            .historical_launch
            .as_ref()
            .ok_or_else(|| error("HISTORY_NO_HISTORICAL_LAUNCH_CHANGED"))?
            .witness()?;
        self.enter_failure_phase()?;
        // The typed never-created branch will acquire its own exclusive current
        // image fence; ordinary measured readers must be released for that cut.
        drop(self.installed.take());
        self.return_preparation = Some(ReturnBoundaryPreparation::new());
        self.return_attempt = Some(
            self.return_preparation
                .as_mut()
                .expect("retained failed Return preparation")
                .prepare_failed_installer(
                    FailedInstallerReturnInputs {
                        binding: self.parts.binding.clone(),
                        installation: self.parts.installation.clone(),
                        data: self.parts.data.clone(),
                        exclusive: &self.parts.exclusive,
                        installer: self
                            .installer
                            .as_ref()
                            .expect("observed installer terminal")
                            .clone(),
                        no_historical,
                        package: self.parts.package.clone(),
                        scope: self.parts.scope.clone(),
                        original_bundle: self.parts.original_bundle.clone(),
                        original_context: self
                            .originals
                            .as_ref()
                            .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                        registration: &self.parts.registration,
                        shortcuts: &self.parts.shortcuts,
                    },
                    &mut self.parts.store,
                )?,
        );
        progress.publish(
            &mut self.parts.store,
            None,
            &[ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
        )?;
        if let Some(command) = self.requested_return.take() {
            let result = self.return_previous(&command, progress);
            let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
            command.finish(result);
            return outcome;
        }
        loop {
            let command = progress.recv_command()?;
            if let Err(failure) = command.check(&self.parts.binding, &self.parts.store) {
                command.finish(Err(failure));
                continue;
            }
            if command.action() != ManagerAction::ReturnToPrevious {
                command.finish(Err(error("HISTORY_OPERATION_PENDING")));
                continue;
            }
            let command = match command.accept_return(&self.parts.binding, &self.parts.store) {
                Ok(command) => command,
                Err(_) => continue,
            };
            let result = self.return_previous(&command, progress);
            let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
            command.finish(result);
            return outcome;
        }
    }
    fn enter_failure_phase(&mut self) -> Result<(), SafeError> {
        self.parts.store.verify_windows_binding(
            self.parts.installation.root(),
            &self.parts.binding,
            self.parts.generation,
        )?;
        let inspected = self.parts.store.inspect(&self.parts.binding)?;
        let journal = inspected
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if inspected.blocked
            || matches!(
                journal.phase(),
                JournalPhase::Restored | JournalPhase::PreContextAborted
            )
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        if journal.phase() != JournalPhase::RecoveryRequired {
            self.parts.generation = self.parts.store.append(
                self.parts.generation,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired,
                },
            )?;
        }
        publish_checkpoint(
            &self.parts.installation,
            &self.parts.control,
            &self.parts.binding,
            &mut self.parts.store,
        )
    }
    fn with_return_checkpoint(
        &mut self,
        apply: impl FnOnce(&mut JournalStore, &LiveReturnCheckpoint<'_>, u64) -> Result<u64, SafeError>,
    ) -> Result<u64, SafeError> {
        self.verify()?;
        if self.historical_launch.is_some() {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let evidence = LiveReturnCheckpoint {
            binding: &self.parts.binding,
            installation: &self.parts.installation,
            data: &self.parts.data,
            exclusive: &self.parts.exclusive,
            source: &self.parts.terminal,
            scope: &self.parts.scope,
            installer: self
                .installer
                .as_ref()
                .ok_or_else(|| error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"))?,
            historical: self
                .historical
                .as_ref()
                .ok_or_else(|| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?,
            boundary: self
                .return_boundary
                .as_ref()
                .ok_or_else(|| error("HISTORY_RETURN_BOUNDARY_BLOCKED"))?,
            attempt: self
                .return_attempt
                .as_ref()
                .ok_or_else(|| error("HISTORY_RETURN_BOUNDARY_BLOCKED"))?,
            context: self
                .context_restoration
                .as_ref()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
            bundle: self
                .bundle_restoration
                .as_ref()
                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
            registration: &self.parts.registration,
            shortcuts: &self.parts.shortcuts,
        };
        apply(&mut self.parts.store, &evidence, self.parts.generation)
    }
    fn return_previous(
        &mut self,
        command: &AcceptedManagerReturn,
        progress: &ProgressPublisher,
    ) -> Result<ManagerStatus, SafeError> {
        command.verify(
            &self.parts.binding,
            &self.parts.store,
            self.parts.generation,
        )?;
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        if self.return_attempt.is_none() {
            if self.return_preparation.is_some() {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
            self.return_preparation = Some(ReturnBoundaryPreparation::new());
            self.return_attempt = Some(
                self.return_preparation
                    .as_mut()
                    .expect("retained normal Return preparation")
                    .prepare_normal(
                        ReturnBoundaryInputs {
                            binding: self.parts.binding.clone(),
                            installation: self.parts.installation.clone(),
                            data: self.parts.data.clone(),
                            exclusive: &self.parts.exclusive,
                            installer: self
                                .installer
                                .as_ref()
                                .ok_or_else(|| error("HISTORY_INSTALLER_OUTCOME_UNKNOWN"))?
                                .clone(),
                            historical: self
                                .historical
                                .as_ref()
                                .ok_or_else(|| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?
                                .clone(),
                            package: self.parts.package.clone(),
                            scope: self.parts.scope.clone(),
                            original_bundle: self.parts.original_bundle.clone(),
                            original_context: self
                                .originals
                                .as_ref()
                                .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?,
                            registration: &self.parts.registration,
                            shortcuts: &self.parts.shortcuts,
                        },
                        &mut self.parts.store,
                    )?,
            );
        }
        // Review generation and original native document are checked again at
        // the first return mutation. A stale queued click cannot start fencing.
        command.verify(
            &self.parts.binding,
            &self.parts.store,
            self.parts.generation,
        )?;
        self.reserve
            .as_ref()
            .ok_or_else(|| error("HISTORY_ABORT_RESERVE_UNAVAILABLE"))?
            .verify_for(
                &self.parts.data,
                &self.parts.installation,
                &self.parts.binding,
            )?;
        self.reserve
            .as_mut()
            .expect("verified reserve")
            .release_once(&self.parts.control)?;
        let attempt = self
            .return_attempt
            .as_mut()
            .expect("retained return attempt");
        let admitted = attempt.admit(&mut self.parts.store);
        self.parts.generation = attempt.generation();
        let (boundary, desk, webview, generation) = admitted?.into_parts();
        self.parts.generation = generation;
        self.return_boundary = Some(Arc::new(boundary));
        self.return_roots = Some((desk, webview));
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::check_evidence_scope(
            self.return_attempt
                .as_ref()
                .map(|attempt| attempt.acceptance_exclusions()),
        );
        self.checkpoint()?;
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        self.later_quarantine = Some(Arc::new(
            PrivateDirectory::create_new(
                self.parts.data.root().directory().clone(),
                name("later-context")?,
                &user,
            )
            .map_err(blocked)?,
        ));
        let (desk, webview) = self.return_roots.as_ref().expect("admitted current roots");
        self.later = Some(
            LaterContextRoots::capture_after_exit(
                self.originals.as_ref().expect("sealed source context"),
                desk.clone(),
                webview.clone(),
                self.later_quarantine
                    .as_ref()
                    .expect("later quarantine")
                    .clone(),
                self.return_boundary
                    .as_ref()
                    .expect("native return boundary"),
                &user,
            )
            .map_err(blocked)?,
        );
        drop(self.return_roots.take());
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let preserved = self
                .later
                .as_mut()
                .expect("retained later context")
                .preserve_after_exit(
                    self.originals.as_ref().expect("sealed source context"),
                    self.return_boundary
                        .as_ref()
                        .expect("native return boundary"),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            drop(journal);
            preserved.map_err(blocked)?;
        }
        let later_manifest = self
            .later
            .as_ref()
            .expect("preserved later context")
            .manifest_bytes(
                self.originals.as_ref().expect("sealed source context"),
                &user,
            )
            .map_err(blocked)?;
        let digest = self.parts.store.retain_manifest(&later_manifest)?;
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Manifest {
                role: ManifestRole::RetainedTargetContext,
                digest,
            },
        )?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::LaterCaptured,
            &self.parts.binding,
            self.parts.generation,
            || {
                Ok(serde_json::json!({"binding":self.parts.binding,
                "retainedContext":serde_json::from_slice::<serde_json::Value>(&later_manifest).map_err(blocked)?,
                "retainedContextLocations":self.later.as_ref().ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?.acceptance_retained_locations().map_err(blocked)?,
                "laterContextDirectory":self.later_quarantine.as_ref().ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?.directory().path().map_err(blocked)?.to_string_lossy()}))
            },
        );
        self.bundle_preparation = Some(BundlePreparationAttempt::new(
            self.parts.original_bundle.clone(),
            self.return_boundary
                .as_ref()
                .expect("native return boundary")
                .clone(),
        ));
        {
            // Preserve the complete target installation before changing any of
            // its bytes, alongside its independently preserved later context.
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let prepared = self
                .bundle_preparation
                .as_mut()
                .expect("retained bundle preparation")
                .prepare(&user, &mut journal);
            self.parts.generation = journal.generation();
            drop(journal);
            self.bundle_restoration = Some(prepared.map_err(blocked)?);
        }
        self.context_restoration = Some(
            ContextRestoration::new_retaining(&mut self.originals, &mut self.later)
                .map_err(blocked)?,
        );
        self.phase(JournalPhase::Restoring)?;
        // 从未创建历史程序或在首次resume前取消的旧live返回保持原证明路径。
        if self
            .historical
            .as_ref()
            .is_some_and(|terminal| !terminal.was_cancelled_before_resume())
        {
            // 原受管进程已终止。废弃启动permit，封存完整返回点，再持久化唯一执行claim。
            drop(self.historical_launch.take());
            self.parts.generation =
                self.with_return_checkpoint(|store, evidence, generation| {
                    store.seal_live_return_checkpoint(evidence, generation)
                })?;
            self.checkpoint()?;
            let marker = {
                let retained = MarkerStore::open_existing(
                    self.parts.installation.root().clone(),
                    &self.parts.control,
                )
                .map_err(blocked)?
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
                ActiveContextMarker::decode(retained.current().map_err(blocked)?)?
            };
            self.parts.generation =
                self.with_return_checkpoint(|store, evidence, generation| {
                    store.claim_live_return_checkpoint(evidence, &marker, generation)
                })?;
        }
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])?;
        // Restore matching Desk/WebView context while the startup marker and
        // lifetime lease still block the feature-bearing original application.
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self
                .context_restoration
                .as_mut()
                .expect("owned context return")
                .restore_after_exit(
                    self.return_boundary
                        .as_ref()
                        .expect("native return boundary"),
                    &user,
                    &mut journal,
                );
            self.parts.generation = journal.generation();
            drop(journal);
            result.map_err(blocked)?;
        }
        self.restored_context = Some(
            ContextRestoration::finish_retaining(&mut self.context_restoration, &user)
                .map_err(blocked)?,
        );
        {
            let mut journal = ContextJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self
                .bundle_restoration
                .as_mut()
                .expect("owned bundle return")
                .restore(&user, &mut journal);
            self.parts.generation = journal.generation();
            drop(journal);
            self.restored_bundle = Some(result.map_err(blocked)?);
        }
        {
            let mut journal = RegistrationJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self.parts.registration.restore(&mut journal);
            self.parts.generation = journal.generation();
            drop(journal);
            self.restored_registration = Some(result.map_err(blocked)?);
        }
        for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
            let mut journal = ShortcutJournal::new(
                &mut self.parts.store,
                self.parts.installation.root().clone(),
                &self.parts.exclusive,
                self.parts.binding.clone(),
                self.parts.generation,
            )
            .map_err(blocked)?;
            let result = self.parts.shortcuts.restore(slot, &mut journal);
            self.parts.generation = journal.generation();
            drop(journal);
            self.restored_shortcuts
                .insert(slot, result.map_err(blocked)?);
        }
        self.verify_restored()?;
        self.parts.generation = self.parts.store.append(
            self.parts.generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restored,
            },
        )?;
        let terminal =
            ActiveContextMarker::restored(&self.parts.store.inspect(&self.parts.binding)?)?;
        self.verify_restored()?;
        {
            let mut marker = MarkerStore::open_existing(
                self.parts.installation.root().clone(),
                &self.parts.control,
            )
            .map_err(blocked)?
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
            if prior.binding() != &self.parts.binding || prior.is_terminal() {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
            marker
                .append(&terminal, &mut self.parts.store)
                .map_err(blocked)?;
            if marker.current().map_err(blocked)? != terminal.encode()? {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
        }
        // Release startup only after the exact final marker and all actual
        // restored state were checked together under the original lease.
        self.verify_restored()?;
        #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
        acceptance::observe(
            AcceptanceStage::FinalRestored,
            &self.parts.binding,
            self.parts.generation,
            || {
                let bundle = self
                    .restored_bundle
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
                let context = self
                    .restored_context
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
                let inspection = self.parts.store.inspect(&self.parts.binding)?;
                let state = inspection
                    .last_valid
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
                let marker = MarkerStore::open_existing(
                    self.parts.installation.root().clone(),
                    &self.parts.control,
                )
                .map_err(blocked)?
                .ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?;
                let marker_bytes = marker.current().map_err(blocked)?;
                if inspection.blocked
                    || state.phase() != JournalPhase::Restored
                    || state.requires_reconciliation()
                    || state.generation() != self.parts.generation
                    || marker_bytes != terminal.encode()?
                {
                    return Err(error("HISTORY_ACCEPTANCE_REPORT_MISSING"));
                }
                Ok(
                    serde_json::json!({"binding":self.parts.binding,"phase":state.phase(),"pending":false,
                "marker":serde_json::from_slice::<serde_json::Value>(marker_bytes).map_err(blocked)?,
                "markerLogPath":std::path::PathBuf::from(self.parts.installation.root().directory().path().map_err(blocked)?).join("active-context.log").to_string_lossy(),
                "journalLogPath":std::path::PathBuf::from(self.parts.installation.root().directory().path().map_err(blocked)?).join(format!("journal-{}.log",self.parts.binding.transaction_id)).to_string_lossy(),
                "journalHead":inspection.head().ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?,
                "bundle":bundle.manifest(),"sourceBundle":bundle.source_manifest(),
                "bundleLogicalDigest":bundle.source_manifest().logical_digest().map_err(blocked)?,
                "context":context.original_snapshot(),
                "registration":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::Registration)?,
                "shortcuts":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::Shortcuts)?,
                "retainedContext":acceptance_manifest(&self.parts.store,&self.parts.binding,ManifestRole::RetainedTargetContext)?,
                "retainedBundle":bundle.later_manifest(),
                    "retainedBundleDirectory":bundle.acceptance_retained_directory().map_err(blocked)?,
                    "retainedContextLocations":context.acceptance_retained_locations().map_err(blocked)?,
                "dataRoot":self.parts.data.root().directory().path().map_err(blocked)?.to_string_lossy(),
                "laterContextDirectory":self.later_quarantine.as_ref().ok_or_else(|| error("HISTORY_ACCEPTANCE_REPORT_MISSING"))?.directory().path().map_err(blocked)?.to_string_lossy()}),
                )
            },
        );
        progress.publish(&mut self.parts.store, None, &[ManagerAction::Refresh])
    }
    fn verify_restored(&mut self) -> Result<(), SafeError> {
        self.verify()?;
        self.return_boundary
            .as_ref()
            .ok_or_else(|| error("HISTORY_RETURN_BOUNDARY_BLOCKED"))?
            .verify_live()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        let context = self
            .restored_context
            .as_ref()
            .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?;
        context.verify(&user).map_err(blocked)?;
        if context.original_snapshot().context_id != self.parts.binding.source_context {
            return Err(error("HISTORY_CONTEXT_CHANGED"));
        }
        let bundle = self
            .restored_bundle
            .as_ref()
            .ok_or_else(|| error("HISTORY_INSTALLATION_CHANGED"))?;
        bundle.verify(&user).map_err(blocked)?;
        if bundle.source_manifest().logical_digest().map_err(blocked)?
            != self.parts.binding.source_bundle
        {
            return Err(error("HISTORY_INSTALLATION_CHANGED"));
        }
        self.restored_registration
            .as_ref()
            .ok_or_else(|| error("HISTORY_INSTALLATION_CHANGED"))?
            .verify()
            .map_err(blocked)?;
        if self.restored_shortcuts.len() != 2 {
            return Err(error("HISTORY_INSTALLATION_CHANGED"));
        }
        for slot in [ShortcutSlot::Desktop, ShortcutSlot::StartMenu] {
            self.restored_shortcuts
                .get(&slot)
                .ok_or_else(|| error("HISTORY_INSTALLATION_CHANGED"))?
                .verify()
                .map_err(blocked)?;
        }
        Ok(())
    }
}

/// The worker calls this once after transferring actual source-terminal and
/// snapshot admission owners. No status DTO or CLI selector can enter here.
/// Failure returns all partially mutated owners; success drops native readers
/// and leases only after final marker/readback verification has completed.
pub(crate) fn run_acquired(
    parts: AcquiredSourceParts,
    owner: Arc<Mutex<InitialManager>>,
    progress: ProgressPublisher,
) -> Result<(), CoordinatorFailure> {
    let no_source_launch = SourceNoLaunch::capture(&parts);
    let mut source = Box::new(SourceExecution::new(parts));
    let no_source_launch = match no_source_launch {
        Ok(proof) => proof,
        Err(failure) => {
            progress.fail(failure.clone());
            return Err(CoordinatorFailure {
                error: failure,
                source,
            });
        }
    };
    source.no_source_launch = Some(no_source_launch);
    let result = (|| {
        source.prepare_source(&progress)?;
        source.seal_and_create_fresh(&progress)?;
        let initial = owner.lock();
        if source.parts.payload.is_ordinary() {
            return source.handoff_ordinary(&initial, &progress);
        }
        if matches!(
            source.install(&initial, &progress)?,
            InstallerStage::ReturnRequested
        ) {
            return source.recover_failed_installer(&progress);
        }
        let command = source.launch_and_wait_for_return(&initial, &progress)?;
        let result = source.return_previous(&command, &progress);
        let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
        command.finish(result);
        outcome
    })();
    let ordinary = source.parts.payload.is_ordinary();
    let result = match result {
        Err(_)
            if !ordinary
                && !source.failure_recovery_started
                && source.installer.is_some()
                && source.historical.is_none()
                && source.return_attempt.is_none()
                && source.return_boundary.is_none() =>
        {
            source.recover_failed_installer(&progress)
        }
        Err(_)
            if !ordinary
                && !source.failure_recovery_started
                && source.installer.is_none()
                && source.historical.is_none()
                && source.return_attempt.is_none()
                && source.return_boundary.is_none() =>
        {
            source.recover_source_failure(&progress)
        }
        result => result,
    };
    match result {
        Ok(()) => Ok(()),
        Err(failure) => {
            if let Some(command) = source.requested_return.take() {
                command.finish(Err(failure.clone()));
            }
            if ordinary {
                progress.fail_ordinary(
                    failure.clone(),
                    ManagerBlockReason::RecoveryEvidenceUnavailable,
                );
            } else {
                progress.fail(failure.clone());
            }
            Err(CoordinatorFailure {
                error: failure,
                source,
            })
        }
    }
}

#[cfg(test)]
#[path = "../../tests/version_history_coordinator_windows.rs"]
#[allow(non_snake_case)]
mod tests;
