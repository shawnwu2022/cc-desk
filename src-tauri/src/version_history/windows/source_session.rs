//! Thread-local initial manager source capture. A successful preparation keeps
//! the complete original installation pinned before native Ready is published.
//! Post-exit acquisition stops at an UNRENAMED image fence: the caller still
//! needs a physical abort reserve and a journaled executor before any cutover.
use super::{
    context::{
        bundle_restore::RetainedInstallationBundle, ContextJournal, HeldBundle, HeldContext,
        HeldRoot,
    },
    durability::MarkerStore,
    fence::ImageFence,
    files::{ComponentName, Directory, PrivateDirectory},
    lease::{ControlLease, ExclusiveLease, SharedLease},
    package::RetainedPackage,
    process::{ExactProcess, ProcessIdentity},
    registration_state::{HeldRegistrationState, RegistrationJournal, RetainedRegistrationState},
    scope::{
        ConfiguredExclusions, ConfiguredInventory, ExitedInstallation, FencedInstallation,
        RegisteredInstallation,
    },
    security::CurrentUser,
    shortcuts::{HeldProductShortcuts, RetainedProductShortcuts, ShortcutJournal},
    source_lifecycle::SourceHandoffTerminal,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{JournalBinding, JournalPhase, JournalStore, ManifestRole, RootKind},
        maintenance::ActiveContextMarker,
        payload_policy::{PayloadAdmission, PreservedCompanions},
        policy::PRODUCT_IDENTIFIER,
        snapshot::SnapshotLimits,
        verified_package::sha256,
    },
};
use parking_lot::Mutex;
use std::{ffi::OsStr, io, marker::PhantomData, os::windows::ffi::OsStrExt, rc::Rc, sync::Arc};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_SOURCE_SNAPSHOT_BLOCKED")
}
fn component(value: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(value))
}

/// Failure stages describe the attempted operation, never an inferred outcome.
/// In particular CopyOriginalBundle can contain a partially completed copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceCaptureStage {
    ObserveSource,
    BindWriter,
    CopyOriginalBundle,
    PublishCopyCheckpoint,
    Prepared,
    ValidateSourceExit,
    AcquireExclusive,
    AdmitBrowserExit,
    CaptureContext,
    CaptureExclusions,
    RetainRegistration,
    RetainShortcuts,
    ReleaseSourceReaders,
    AcquireImageFence,
    CaptureFencedBundle,
    Acquired,
}

/// This object deliberately cannot be sent to Tauri managed state. The worker
/// must construct, retain, and consume it on its original OS thread; HKEYs stay
/// here even if a future wrapper gains an unsafe Send implementation.
pub(crate) struct SourceCaptureSession {
    evidence: SourceEvidence,
}

/// A failed attempt retains every surviving native owner and private artifact.
/// There is no retry conversion: unknown/partial effects need their recorded
/// reconciliation path, and dropping this value never deletes copied objects.
pub(crate) struct SourceCaptureFailure {
    error: SafeError,
    evidence: Box<SourceEvidence>,
}
impl SourceCaptureFailure {
    pub(crate) fn error(&self) -> &SafeError {
        &self.error
    }
    pub(crate) fn stage(&self) -> SourceCaptureStage {
        self.evidence.stage
    }
}

struct SourceEvidence {
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    binding: JournalBinding,
    package: Arc<RetainedPackage>,
    source_identity: ProcessIdentity,
    source_input: Option<ExactProcess>,
    registered: Option<RegisteredInstallation>,
    bundle: Option<HeldBundle>,
    held_registration: Option<HeldRegistrationState>,
    held_shortcuts: Option<HeldProductShortcuts>,
    payload: Option<PayloadAdmission>,
    companions: Option<PreservedCompanions>,
    original_bundle: Option<Arc<RetainedInstallationBundle>>,
    shared: Option<SharedLease>,
    control: Option<ControlLease>,
    store: Option<JournalStore>,
    exclusive: Option<ExclusiveLease>,
    terminal_input: Option<ExactProcess>,
    terminal: Option<Arc<SourceHandoffTerminal>>,
    context: Option<HeldContext>,
    exclusions: Option<Arc<ConfiguredExclusions>>,
    registration: Option<RetainedRegistrationState>,
    shortcuts: Option<RetainedProductShortcuts>,
    exited: Option<ExitedInstallation>,
    scope: Option<Arc<FencedInstallation>>,
    fence: Option<Arc<Mutex<ImageFence>>>,
    generation: u64,
    stage: SourceCaptureStage,
    _thread_bound: PhantomData<Rc<()>>,
}

/// Actual acquired capabilities only. No serialized digest/boolean supplies
/// admission, and no source namespace effect has occurred in this module.
pub(crate) struct AcquiredSourceSession {
    evidence: SourceEvidence,
}

/// Consumed by the physical-reserve/journaled-fence executor on this SAME
/// worker. Keep context readers until the reviewed context executor consumes
/// them. The pre-fence exclusions may be dropped only after the source factory
/// has recaptured its own exclusions from this exact durable context.
pub(crate) struct AcquiredSourceParts {
    pub(crate) installation: Arc<InstallationControl>,
    pub(crate) data: Arc<TransactionDataRoot>,
    pub(crate) binding: JournalBinding,
    pub(crate) package: Arc<RetainedPackage>,
    pub(crate) payload: PayloadAdmission,
    pub(crate) companions: PreservedCompanions,
    pub(crate) original_bundle: Arc<RetainedInstallationBundle>,
    pub(crate) terminal: Arc<SourceHandoffTerminal>,
    pub(crate) scope: Arc<FencedInstallation>,
    pub(crate) fence: Arc<Mutex<ImageFence>>,
    pub(crate) current_bundle: HeldBundle,
    pub(crate) context: HeldContext,
    pub(crate) exclusions: Arc<ConfiguredExclusions>,
    pub(crate) registration: RetainedRegistrationState,
    pub(crate) shortcuts: RetainedProductShortcuts,
    pub(crate) exclusive: ExclusiveLease,
    pub(crate) control: ControlLease,
    pub(crate) store: JournalStore,
    pub(crate) generation: u64,
    _thread_bound: PhantomData<Rc<()>>,
}

impl SourceCaptureSession {
    pub(crate) fn prepare(
        installation: Arc<InstallationControl>,
        data: Arc<TransactionDataRoot>,
        binding: JournalBinding,
        source: ExactProcess,
        package: Arc<RetainedPackage>,
    ) -> Result<Self, SourceCaptureFailure> {
        let mut evidence = SourceEvidence {
            installation,
            data,
            binding,
            package,
            source_identity: source.identity().clone(),
            source_input: Some(source),
            registered: None,
            bundle: None,
            held_registration: None,
            held_shortcuts: None,
            payload: None,
            companions: None,
            original_bundle: None,
            shared: None,
            control: None,
            store: None,
            exclusive: None,
            terminal_input: None,
            terminal: None,
            context: None,
            exclusions: None,
            registration: None,
            shortcuts: None,
            exited: None,
            scope: None,
            fence: None,
            generation: 0,
            stage: SourceCaptureStage::ObserveSource,
            _thread_bound: PhantomData,
        };
        if let Err(error) = evidence.prepare() {
            return Err(SourceCaptureFailure {
                error,
                evidence: Box::new(evidence),
            });
        }
        Ok(Self { evidence })
    }

    /// The caller transfers the exact source handle held by ManagerChildAdmission,
    /// after its UI/controller lease has been released on actual source exit.
    /// A timeout, absent PID, or missing job is never an accepted input.
    pub(crate) fn acquire_after_exit(
        self,
        source: ExactProcess,
    ) -> Result<AcquiredSourceSession, SourceCaptureFailure> {
        let mut evidence = self.evidence;
        evidence.stage = SourceCaptureStage::ValidateSourceExit;
        evidence.terminal_input = Some(source);
        if let Err(error) = evidence.acquire_after_exit() {
            return Err(SourceCaptureFailure {
                error,
                evidence: Box::new(evidence),
            });
        }
        Ok(AcquiredSourceSession { evidence })
    }
}

impl SourceEvidence {
    fn prepare(&mut self) -> Result<(), SafeError> {
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        self.data.verify_installation(&self.installation)?;
        self.verify_package(&user)?;
        let source = self.source_input.as_ref().expect("initial source owner");
        let image = source.observed_image_path().map_err(blocked)?;
        self.registered = Some(
            RegisteredInstallation::capture_for_source(
                self.source_input.take().expect("initial source owner"),
                &image,
            )
            .map_err(blocked)?,
        );
        let registered = self.registered.as_ref().expect("registered source");
        self.verify_installation_identity(registered, &user)?;
        registered
            .directory()
            .require_disjoint(&[
                self.installation.root().directory().clone(),
                self.data.root().directory().clone(),
            ])
            .map_err(blocked)?;
        self.bundle = Some(
            HeldBundle::capture_registered_source(registered, SnapshotLimits::default())
                .map_err(blocked)?,
        );
        self.verify_original_inventory()?;
        self.held_registration = Some(HeldRegistrationState::capture(registered).map_err(blocked)?);
        self.held_shortcuts = Some(HeldProductShortcuts::capture_current_user().map_err(blocked)?);
        let payload = PayloadAdmission::admit_retained(&self.package)?;
        if payload.inventory_digest() != self.binding.target_payload {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        self.companions =
            Some(payload.retain_source(self.bundle.as_ref().expect("source bundle"))?);
        self.payload = Some(payload);

        self.stage = SourceCaptureStage::BindWriter;
        self.control = Some(self.installation.acquire_control()?);
        let control = self.control.as_ref().expect("source control");
        self.shared = Some(
            self.installation
                .leases()
                .acquire_shared(control)
                .map_err(blocked)?,
        );
        let (store, generation) =
            open_bound_writer(&self.installation, control, &self.binding, None)?;
        self.store = Some(store);
        self.generation = generation;
        self.stage = SourceCaptureStage::CopyOriginalBundle;
        {
            let mut journal = ContextJournal::new_precommit(
                self.store.as_mut().expect("source writer"),
                self.installation.root().clone(),
                control,
                self.shared.as_ref().expect("source shared lease"),
                self.binding.clone(),
                self.generation,
            )
            .map_err(blocked)?;
            let original = RetainedInstallationBundle::preserve(
                registered,
                self.bundle.as_ref().expect("source bundle"),
                self.data.root().clone(),
                &user,
                &mut journal,
            )
            .map_err(blocked)?;
            self.original_bundle = Some(Arc::new(original));
            self.generation = journal.generation();
        }
        self.verify_original_inventory()?;
        self.held_registration
            .as_ref()
            .expect("source registration")
            .recheck()
            .map_err(blocked)?;
        self.held_shortcuts
            .as_ref()
            .expect("source shortcuts")
            .verify()
            .map_err(blocked)?;
        self.verify_package(&user)?;
        self.stage = SourceCaptureStage::PublishCopyCheckpoint;
        publish_copy_checkpoint(
            &self.installation,
            self.control.as_ref().expect("source control"),
            self.store.as_mut().expect("source writer"),
            &self.binding,
        )?;
        // Source close/BrowserProcessExited uses this same journal and control.
        // Neither mutable owner may survive native Ready publication.
        drop(self.store.take());
        drop(self.control.take());
        self.stage = SourceCaptureStage::Prepared;
        Ok(())
    }

    fn acquire_after_exit(&mut self) -> Result<(), SafeError> {
        require_source_terminal(
            self.terminal_input.as_ref().expect("exact child source"),
            &self.source_identity,
        )?;
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        self.verify_package(&user)?;
        self.verify_original_inventory()?;
        self.original_bundle
            .as_ref()
            .expect("independent original copy")
            .verify(&user)
            .map_err(blocked)?;
        self.data.verify_installation(&self.installation)?;
        self.stage = SourceCaptureStage::AcquireExclusive;
        self.control = Some(self.installation.acquire_control()?);
        let control = self.control.as_ref().expect("source control");
        self.exclusive = Some(acquire_new_exclusive(
            &self.installation,
            control,
            &mut self.shared,
        )?);
        let (store, generation) = open_bound_writer(
            &self.installation,
            control,
            &self.binding,
            Some(
                self.generation
                    .checked_add(1)
                    .ok_or_else(|| error("HISTORY_GENERATION_CHANGED"))?,
            ),
        )?;
        self.store = Some(store);
        self.generation = generation;
        self.stage = SourceCaptureStage::AdmitBrowserExit;
        let terminal = SourceHandoffTerminal::admit(
            self.store.as_mut().expect("source writer"),
            &self.installation,
            self.exclusive.as_ref().expect("exclusive source lease"),
            &self.binding,
            &self.data,
            self.terminal_input.take().expect("exact child source"),
        )?;
        if terminal.exit().host_identity() != &self.source_identity {
            return Err(error("HISTORY_SOURCE_EXIT_UNCONFIRMED"));
        }
        self.terminal = Some(Arc::new(terminal));
        self.stage = SourceCaptureStage::CaptureContext;
        let home = dirs::home_dir().ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?;
        let parent = Directory::open_absolute(&home).map_err(blocked)?;
        let desk = capture_desk_root(parent).map_err(blocked)?;
        let udf = self.terminal.as_ref().expect("source exit").udf().clone();
        self.context = Some(
            HeldContext::capture_durable(desk, HeldRoot::Present(udf), SnapshotLimits::default())
                .map_err(blocked)?,
        );
        self.verify_original_roots()?;
        self.stage = SourceCaptureStage::CaptureExclusions;
        self.exclusions = Some(Arc::new(
            ConfiguredInventory::capture_for_context(self.context.as_mut().expect("durable M0"))
                .map_err(blocked)?
                .into_exclusions(
                    self.registered
                        .as_ref()
                        .expect("registered source")
                        .directory()
                        .clone(),
                    self.data.root().clone(),
                )
                .map_err(blocked)?,
        ));
        self.exclusions
            .as_ref()
            .expect("final exclusions")
            .verify_context(self.context.as_ref().expect("durable M0"))
            .map_err(blocked)?;

        self.stage = SourceCaptureStage::RetainRegistration;
        {
            let mut journal = RegistrationJournal::new(
                self.store.as_mut().expect("source writer"),
                self.installation.root().clone(),
                self.exclusive.as_ref().expect("exclusive source lease"),
                self.binding.clone(),
                self.generation,
            )
            .map_err(blocked)?;
            self.registration = Some(
                self.held_registration
                    .take()
                    .expect("source registration")
                    .retain(&mut journal)
                    .map_err(blocked)?,
            );
            self.generation = journal.generation();
        }
        self.stage = SourceCaptureStage::RetainShortcuts;
        {
            let mut journal = ShortcutJournal::new(
                self.store.as_mut().expect("source writer"),
                self.installation.root().clone(),
                self.exclusive.as_ref().expect("exclusive source lease"),
                self.binding.clone(),
                self.generation,
            )
            .map_err(blocked)?;
            self.shortcuts = Some(
                self.held_shortcuts
                    .take()
                    .expect("source shortcuts")
                    .retain(&mut journal)
                    .map_err(blocked)?,
            );
            self.generation = journal.generation();
        }
        self.verify_original_inventory()?;
        self.stage = SourceCaptureStage::ReleaseSourceReaders;
        self.exited = Some(
            self.registered
                .take()
                .expect("registered source")
                .release_after_exit()
                .map_err(blocked)?,
        );
        // The complete original manifest and measured companion inventory have
        // already been retained. These duplicate image readers must disappear
        // before share-zero acquisition; the exact terminal objects stay held.
        drop(self.bundle.take());
        self.stage = SourceCaptureStage::AcquireImageFence;
        self.fence = Some(Arc::new(Mutex::new(
            self.exited
                .as_ref()
                .expect("exited installation")
                .acquire_source_fence()
                .map_err(blocked)?,
        )));
        self.scope = Some(Arc::new(
            self.exited
                .take()
                .expect("exited installation")
                .into_fenced(self.fence.as_ref().expect("unrenamed image fence").clone())
                .map_err(blocked)?,
        ));
        self.stage = SourceCaptureStage::CaptureFencedBundle;
        let scope = self.scope.as_ref().expect("fenced scope");
        if scope.source_process_identity()
            != self
                .terminal
                .as_ref()
                .expect("source exit")
                .exit()
                .host_identity()
        {
            return Err(error("HISTORY_SOURCE_EXIT_UNCONFIRMED"));
        }
        self.bundle = Some(
            HeldBundle::capture(
                scope.directory().clone(),
                scope.image_name().clone(),
                self.fence.as_ref().expect("unrenamed image fence").clone(),
                SnapshotLimits::default(),
            )
            .map_err(blocked)?,
        );
        self.verify_original_inventory()?;
        self.exclusions
            .as_ref()
            .expect("final exclusions")
            .verify_context(self.context.as_ref().expect("durable M0"))
            .map_err(blocked)?;
        self.terminal
            .as_ref()
            .expect("source exit")
            .verify(&self.binding)?;
        self.original_bundle
            .as_ref()
            .expect("independent original copy")
            .verify(&user)
            .map_err(blocked)?;
        self.verify_package(&user)?;
        self.stage = SourceCaptureStage::Acquired;
        Ok(())
    }

    fn verify_package(&self, user: &CurrentUser) -> Result<(), SafeError> {
        self.data.verify_installation(&self.installation)?;
        self.package.verify_retained()?;
        if self.data.transaction_id() != self.binding.transaction_id
            || self.package.transaction_id() != self.binding.transaction_id
            || self.package.selection().installer().sha256() != self.binding.target_package
        {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        let actual = PrivateDirectory::open_existing(
            self.data.root().directory().clone(),
            component("package").map_err(blocked)?,
            user,
        )
        .map_err(blocked)?;
        if actual.directory().identity() != self.package.root_identity() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        actual.verify(user).map_err(blocked)
    }

    fn verify_installation_identity(
        &self,
        registered: &RegisteredInstallation,
        user: &CurrentUser,
    ) -> Result<(), SafeError> {
        registered.recheck().map_err(blocked)?;
        let actual = sha256(
            &serde_json::to_vec(&(
                PRODUCT_IDENTIFIER,
                user.sid_text(),
                registered
                    .original_path()
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>(),
            ))
            .map_err(blocked)?,
        );
        if actual != self.binding.user_installation {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        Ok(())
    }

    fn verify_original_inventory(&self) -> Result<(), SafeError> {
        let bundle = self.bundle.as_ref().expect("complete source bundle");
        bundle.tree().verify().map_err(blocked)?;
        if bundle.manifest().logical_digest().map_err(blocked)? != self.binding.source_bundle {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        if let Some(original) = &self.original_bundle {
            if original.source_manifest().tree != bundle.manifest().tree {
                return Err(error("HISTORY_SOURCE_CHANGED"));
            }
        }
        if let Some(companions) = &self.companions {
            companions.verify_source(bundle)?;
        }
        Ok(())
    }

    fn verify_original_roots(&self) -> Result<(), SafeError> {
        let context = self.context.as_ref().expect("durable M0");
        context.verify_durable().map_err(blocked)?;
        let desk = match context.tree(RootKind::Desk).root() {
            HeldRoot::Present(root) => serde_json::json!({"present":root.identity()}),
            HeldRoot::Absent { parent, name } => serde_json::json!({
                "absentParent":parent.identity(), "suffix":[name.os_string().encode_wide().collect::<Vec<_>>()]
            }),
        };
        let terminal = self.terminal.as_ref().expect("source exit");
        let roots = sha256(
            &serde_json::to_vec(&(
                desk,
                terminal.udf().identity(),
                self.registered
                    .as_ref()
                    .expect("registered source")
                    .directory()
                    .identity(),
                self.data.root().directory().identity(),
            ))
            .map_err(blocked)?,
        );
        if roots != self.binding.roots {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        Ok(())
    }
}

impl AcquiredSourceSession {
    pub(crate) fn into_parts(self) -> AcquiredSourceParts {
        let evidence = self.evidence;
        // Only the successful complete acquisition above constructs this type.
        AcquiredSourceParts {
            installation: evidence.installation,
            data: evidence.data,
            binding: evidence.binding,
            package: evidence.package,
            payload: evidence.payload.expect("measured payload"),
            companions: evidence.companions.expect("complete companion inventory"),
            original_bundle: evidence.original_bundle.expect("independent original copy"),
            terminal: evidence.terminal.expect("source exit"),
            scope: evidence.scope.expect("fenced scope"),
            fence: evidence.fence.expect("unrenamed image fence"),
            current_bundle: evidence.bundle.expect("complete source bundle"),
            context: evidence.context.expect("durable M0"),
            exclusions: evidence.exclusions.expect("final exclusions"),
            registration: evidence.registration.expect("retained registration"),
            shortcuts: evidence.shortcuts.expect("retained shortcuts"),
            exclusive: evidence.exclusive.expect("exclusive source lease"),
            control: evidence.control.expect("source control"),
            store: evidence.store.expect("source writer"),
            generation: evidence.generation,
            _thread_bound: PhantomData,
        }
    }
}

/// Read once, retain its exact identity, then release the ordinary reader before
/// requesting DELETE access. Neither wrong type nor sharing denial is absence.
fn capture_desk_root(parent: Arc<Directory>) -> io::Result<HeldRoot> {
    let name = component(".cc-box")?;
    match HeldRoot::observe(parent.clone(), name.clone())? {
        HeldRoot::Present(observed) => {
            let identity = observed.identity().clone();
            drop(observed);
            Ok(HeldRoot::Present(parent.open_for_rename(name, &identity)?))
        }
        absent @ HeldRoot::Absent { .. } => Ok(absent),
    }
}

fn require_source_terminal(
    source: &ExactProcess,
    expected: &ProcessIdentity,
) -> Result<(), SafeError> {
    if source.identity() != expected || source.terminal(0).map_err(blocked)?.is_none() {
        return Err(error("HISTORY_SOURCE_EXIT_UNCONFIRMED"));
    }
    Ok(())
}

fn acquire_new_exclusive(
    installation: &InstallationControl,
    control: &ControlLease,
    shared: &mut Option<SharedLease>,
) -> Result<ExclusiveLease, SafeError> {
    control.verify_root(installation.root()).map_err(blocked)?;
    shared
        .as_ref()
        .ok_or_else(|| error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"))?
        .verify_root(installation.root())
        .map_err(blocked)?;
    drop(shared.take());
    installation
        .leases()
        .acquire_exclusive(control)
        .map_err(blocked)
}

fn open_bound_writer(
    installation: &InstallationControl,
    control: &ControlLease,
    binding: &JournalBinding,
    expected_exit_generation: Option<u64>,
) -> Result<(JournalStore, u64), SafeError> {
    control.verify_root(installation.root()).map_err(blocked)?;
    let mut store = JournalStore::open_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )?;
    store.bind_existing(binding)?;
    let inspection = store.inspect(binding)?;
    let state = inspection
        .last_valid
        .as_ref()
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    if inspection.blocked
        || state.phase() != JournalPhase::Reviewed
        || state.requires_reconciliation()
        || state.has_historical_uncertainty()
        || state.manifest(ManifestRole::ManagerHandoff).is_none()
    {
        return Err(error("HISTORY_RECOVERY_REQUIRED"));
    }
    let marker = MarkerStore::open_existing(installation.root().clone(), control)
        .map_err(blocked)?
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    let current = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
    if current.binding() != binding || current.is_terminal() {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    if let Some(expected) = expected_exit_generation {
        // SourceHandoff persists exactly one SourceHandoffExit role before
        // exiting. A missing receipt or unrelated advancement
        // cannot borrow this live session's original source admission.
        if expected != state.generation()
            || state.manifest(ManifestRole::SourceHandoffExit).is_none()
        {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
    } else if state.manifest(ManifestRole::SourceHandoffExit).is_some() {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    current.validate_checkpoint(
        state,
        inspection
            .head()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?,
    )?;
    let generation = state.generation();
    store.verify_windows_binding(installation.root(), binding, generation)?;
    Ok((store, generation))
}

fn publish_copy_checkpoint(
    installation: &InstallationControl,
    control: &ControlLease,
    store: &mut JournalStore,
    binding: &JournalBinding,
) -> Result<(), SafeError> {
    control.verify_root(installation.root()).map_err(blocked)?;
    let inspected = store.inspect(binding)?;
    let state = inspected
        .last_valid
        .as_ref()
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    if inspected.blocked
        || state.phase() != JournalPhase::Reviewed
        || state.requires_reconciliation()
    {
        return Err(error("HISTORY_RECOVERY_REQUIRED"));
    }
    store.verify_windows_binding(installation.root(), binding, state.generation())?;
    let checkpoint = ActiveContextMarker::transition_from(&inspected)?;
    let mut marker = MarkerStore::open_existing(installation.root().clone(), control)
        .map_err(blocked)?
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    let current = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
    if current.binding() != binding || current.is_terminal() {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    marker.append(&checkpoint, store).map_err(blocked)?;
    ActiveContextMarker::decode(marker.current().map_err(blocked)?)?.validate_checkpoint(
        state,
        inspected
            .head()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?,
    )
}

#[cfg(test)]
#[allow(non_snake_case)]
#[path = "../../tests/version_history_source_session_windows.rs"]
mod tests;
