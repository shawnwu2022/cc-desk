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
        RetainedContextRoots,
    },
    coordinator_evidence::ReturnBoundary,
    durability::MarkerStore,
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess, PrivateDirectory},
    lease::{ControlLease, ExclusiveLease},
    manager_handoff::InitialManager,
    package::RetainedPackage,
    process::{CommandLine, DurableProcessIdentity, JobKind, PreparedProcess, TerminalProcessJob},
    recovery_space::{AbortReserve, PartialAbortReserve},
    registration_state::{
        RegistrationJournal, RestoredRegistrationReceipt, RetainedRegistrationState,
    },
    return_boundary::{ReturnBoundaryAttempt, ReturnBoundaryInputs},
    scope::{ConfiguredExclusions, FencedInstallation},
    security::CurrentUser,
    shortcuts::{RetainedProductShortcuts, ShortcutJournal, ShortcutRestoreReceipt},
    source_boundary::{AdmittedSourceSnapshot, SourceSnapshotInputs},
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
        manager_worker::{AuthenticatedManagerCommand, ProgressPublisher},
        payload_policy::{PayloadAdmission, PreservedCompanions},
        snapshot::{SnapshotLimits, SnapshotManifest},
    },
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};

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

struct Pending {
    id: String,
    generation: u64,
}
struct TransactionOwners {
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    binding: JournalBinding,
    package: Arc<RetainedPackage>,
    payload: PayloadAdmission,
    companions: PreservedCompanions,
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
    installed: Option<HeldBundle>,
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
            installed: None,
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
        self.parts
            .companions
            .verify_source(self.current_bundle.as_ref().expect("source bundle"))
            .map_err(blocked)?;
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
    fn install(
        &mut self,
        owner: &InitialManager,
        progress: &ProgressPublisher,
    ) -> Result<(), SafeError> {
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
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
        let pending = self.begin(
            EffectKind::InstallerCreateSuspended,
            &(
                self.parts.package.record_digest(),
                self.parts.scope.directory().identity(),
            ),
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
        let exit_code = match outcome {
            Ok(exit_code) => exit_code,
            Err(failure) => park_process_failure(
                (&process, &receipt, &terminal_owner, owner),
                progress,
                failure,
            ),
        };
        // Every fallible process operation above keeps the exact process/job,
        // receipt, and exclusive lease borrow alive on failure.
        drop(process);
        self.installer = terminal_owner;
        self.checkpoint()?;
        if exit_code != 0 {
            return Err(error("HISTORY_INSTALLER_FAILED"));
        }
        self.verify_target(progress)
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
        let pending = self.begin(EffectKind::VerifyTargetBundle,
            &(self.parts.payload.inventory_digest(), self.parts.package.record_digest()),
            &"complete actual installed inventory equals reviewed payload plus preserved companions")?;
        self.parts
            .payload
            .verify_installed(target.tree(), &self.parts.companions)?
            .verify()?;
        self.applied(pending, target.manifest())?;
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
    ) -> Result<AuthenticatedManagerCommand, SafeError> {
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
        if let Err(failure) = launched {
            park_process_failure(
                (&process, &receipt, &terminal_owner, owner),
                progress,
                failure,
            );
        }
        drop(process);
        // Releasing the process image plus complete target readers is necessary
        // before the later factory obtains its own exclusive image fence.
        drop(self.installed.take());
        self.historical = terminal_owner;
        self.checkpoint()?;
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
                return Ok(command);
            }
            command.finish(Err(error("HISTORY_OPERATION_PENDING")));
        }
    }
}

impl SourceExecution {
    fn return_previous(
        &mut self,
        command: &AuthenticatedManagerCommand,
        progress: &ProgressPublisher,
    ) -> Result<ManagerStatus, SafeError> {
        command.check(&self.parts.binding, &self.parts.store)?;
        self.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        self.return_attempt = Some(ReturnBoundaryAttempt::prepare(
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
        )?);
        // Review generation and original native document are checked again at
        // the first return mutation. A stale queued click cannot start fencing.
        command.check(&self.parts.binding, &self.parts.store)?;
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
    let mut source = Box::new(SourceExecution::new(parts));
    let result = (|| {
        source.prepare_source(&progress)?;
        source.seal_and_create_fresh(&progress)?;
        let initial = owner.lock();
        source.install(&initial, &progress)?;
        let command = source.launch_and_wait_for_return(&initial, &progress)?;
        let result = source.return_previous(&command, &progress);
        let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
        command.finish(result);
        outcome
    })();
    match result {
        Ok(()) => Ok(()),
        Err(failure) => {
            progress.fail(failure.clone());
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
