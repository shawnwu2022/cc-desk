//! Native Return admission. Post-launch Return requires both terminal jobs;
//! failed-installer Return instead requires the live one-way no-launch custody.
//! A source boundary, serialized receipt or absent PID cannot mint either.
use super::{
    context::{
        bundle_restore::RetainedInstallationBundle, ContextJournal, HeldContext, HeldRoot,
        RetainedContextRoots, ReturnContextCustody,
    },
    coordinator_evidence::{CurrentImageEvidence, ReturnBoundary, VerifiedImageAbsence},
    fence::ImageFence,
    files::{ComponentName, Directory, FileAccess, PrivateDirectory},
    lease::{ExclusiveLease, ExclusiveLeaseWitness},
    manager_bundle::{ManagerRecord, ManagerRecordReference},
    no_historical_launch::{ClaimedNoHistoricalLaunch, NoHistoricalLaunch},
    package::RetainedPackage,
    process::{JobKind, TerminalProcessJob},
    registration_state::{RegistrationJournal, RetainedRegistrationState},
    scope::{ConfiguredExclusions, ConfiguredInventory, FencedInstallation},
    security::CurrentUser,
    shortcuts::RetainedProductShortcuts,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{
            EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore,
            ManifestRole, Observation, ObservedResult, RetainedRoleGuard, RootKind,
        },
        policy::PRODUCT_IDENTIFIER,
        verified_package::sha256,
    },
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RETURN_BOUNDARY_BLOCKED")
}
fn name(value: &str) -> Result<ComponentName, SafeError> {
    ComponentName::new(OsStr::new(value)).map_err(blocked)
}
fn verify_package_child(
    package: &PrivateDirectory,
    data: &PrivateDirectory,
    user: &CurrentUser,
) -> Result<(), SafeError> {
    package.verify(user).map_err(blocked)?;
    data.verify(user).map_err(blocked)?;
    let (parent, leaf) = package.directory().held_location().map_err(blocked)?;
    if parent.identity() != data.directory().identity() || leaf != name("package")? {
        return Err(blocked("package is outside its retained transaction child"));
    }
    Ok(())
}

pub(crate) struct ReturnBoundaryInputs<'a> {
    pub(crate) binding: JournalBinding,
    pub(crate) installation: Arc<InstallationControl>,
    pub(crate) data: Arc<TransactionDataRoot>,
    pub(crate) exclusive: &'a ExclusiveLease,
    pub(crate) installer: Arc<TerminalProcessJob>,
    pub(crate) historical: Arc<TerminalProcessJob>,
    pub(crate) package: Arc<RetainedPackage>,
    pub(crate) scope: Arc<FencedInstallation>,
    pub(crate) original_bundle: Arc<RetainedInstallationBundle>,
    pub(crate) original_context: &'a RetainedContextRoots,
    pub(crate) registration: &'a RetainedRegistrationState,
    pub(crate) shortcuts: &'a RetainedProductShortcuts,
}

/// This distinct input cannot replace a historical terminal with None. The
/// installer guard positively retains its terminal process and empty private
/// job; the separate live permit proves historical creation never began.
pub(crate) struct FailedInstallerReturnInputs<'a> {
    pub(crate) binding: JournalBinding,
    pub(crate) installation: Arc<InstallationControl>,
    pub(crate) data: Arc<TransactionDataRoot>,
    pub(crate) exclusive: &'a ExclusiveLease,
    pub(crate) installer: Arc<TerminalProcessJob>,
    pub(crate) no_historical: NoHistoricalLaunch,
    pub(crate) package: Arc<RetainedPackage>,
    pub(crate) scope: Arc<FencedInstallation>,
    pub(crate) original_bundle: Arc<RetainedInstallationBundle>,
    pub(crate) original_context: &'a RetainedContextRoots,
    pub(crate) registration: &'a RetainedRegistrationState,
    pub(crate) shortcuts: &'a RetainedProductShortcuts,
}

#[derive(Clone)]
enum HistoricalReturnCustody {
    Terminal(Arc<TerminalProcessJob>),
    NeverCreated(Arc<ClaimedNoHistoricalLaunch>),
}
impl HistoricalReturnCustody {
    fn verify(
        &self,
        binding: &JournalBinding,
        installation: &Arc<InstallationControl>,
        data: &Arc<TransactionDataRoot>,
        scope: &Arc<FencedInstallation>,
        installer: &TerminalProcessJob,
    ) -> Result<(), SafeError> {
        match self {
            Self::Terminal(historical) => {
                historical.verify().map_err(blocked)?;
                if historical.job_kind() != JobKind::HistoricalApplication
                    || historical.root_identity() != data.root().directory().identity()
                    || installer.process_identity() == historical.process_identity()
                {
                    return Err(blocked("foreign historical terminal custody"));
                }
                Ok(())
            }
            Self::NeverCreated(proof) => proof.verify_custody(binding, installation, data, scope),
        }
    }
    fn verify_image(&self, fence: &ImageFence) -> Result<(), SafeError> {
        match self {
            Self::Terminal(historical) => historical
                .verify_image(fence.identity(), fence.digest().map_err(blocked)?)
                .map_err(blocked),
            // A failed installer may leave any partial payload. Its observed
            // image is protected by this actual exclusive fence, without a
            // claimed successful payload or a nonexistent historical process.
            Self::NeverCreated(proof) => proof.verify_live(),
        }
    }
    fn verify_journal(&self, store: &mut JournalStore, generation: u64) -> Result<(), SafeError> {
        match self {
            Self::Terminal(_) => Ok(()),
            Self::NeverCreated(proof) => proof.verify_journal(store, generation),
        }
    }
    fn observation(&self) -> Result<serde_json::Value, SafeError> {
        match self {
            // Preserve the existing post-launch record shape.
            Self::Terminal(historical) => {
                serde_json::to_value(historical.terminal_reference()).map_err(blocked)
            }
            Self::NeverCreated(proof) => proof.observation(),
        }
    }
}

struct PreparationInputs<'a> {
    binding: JournalBinding,
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    exclusive: &'a ExclusiveLease,
    installer: Arc<TerminalProcessJob>,
    historical: HistoricalReturnCustody,
    package: Arc<RetainedPackage>,
    scope: Arc<FencedInstallation>,
    original_bundle: Arc<RetainedInstallationBundle>,
    original_context: &'a RetainedContextRoots,
    registration: &'a RetainedRegistrationState,
    shortcuts: &'a RetainedProductShortcuts,
}

struct ReturnDependencies {
    binding: JournalBinding,
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    exclusive: ExclusiveLeaseWitness,
    installer: Arc<TerminalProcessJob>,
    historical: HistoricalReturnCustody,
    package: Arc<RetainedPackage>,
    package_root: Arc<PrivateDirectory>,
    scope: Arc<FencedInstallation>,
    original_bundle: Arc<RetainedInstallationBundle>,
    original_context: ReturnContextCustody,
    exclusions: ConfiguredExclusions,
    roles: Vec<(ManifestRole, RetainedRoleGuard)>,
}
impl ReturnDependencies {
    fn verify(&self) -> Result<(), SafeError> {
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.data.verify_installation(&self.installation)?;
        self.scope.verify().map_err(blocked)?;
        self.installer.verify().map_err(blocked)?;
        self.historical.verify(
            &self.binding,
            &self.installation,
            &self.data,
            &self.scope,
            &self.installer,
        )?;
        if self.installer.job_kind() != JobKind::Installer
            || self.installer.root_identity() != self.data.root().directory().identity()
            || self.data.transaction_id() != self.binding.transaction_id
            || self.package.transaction_id() != self.binding.transaction_id
            || self.package.root_identity() != self.package_root.directory().identity()
            || self.original_bundle.directory().identity() != self.scope.directory().identity()
            || self
                .original_bundle
                .source_manifest()
                .logical_digest()
                .map_err(blocked)?
                != self.binding.source_bundle
        {
            return Err(blocked("foreign Return owners"));
        }
        self.package.verify_retained()?;
        let installer_image = self.package.installer_image()?;
        let installer_digest = installer_image.digest().map_err(blocked)?;
        if installer_digest != self.binding.target_package {
            return Err(blocked("installer differs from reviewed package"));
        }
        self.installer
            .verify_image(installer_image.identity(), &installer_digest)
            .map_err(blocked)?;
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        verify_package_child(&self.package_root, self.data.root(), &user)?;
        self.original_bundle.verify(&user).map_err(blocked)?;
        self.original_context.verify(&user).map_err(blocked)?;
        self.original_context
            .verify_data_root(self.data.root())
            .map_err(blocked)?;
        self.exclusions.verify_external().map_err(blocked)?;
        for (role, guard) in &self.roles {
            guard.verify_role(&self.binding, *role, self.installation.root())?;
        }
        Ok(())
    }
}

/// Does not own later Desk/UDF descendants. Those are released once actual
/// roots are transferred to LaterContextRoots for its guarded rotation.
pub(crate) struct ReturnSnapshotGuards {
    dependencies: Arc<ReturnDependencies>,
    roots: BTreeMap<RootKind, String>,
    image_quarantine: Arc<PrivateDirectory>,
    record: Arc<ManagerRecord>,
}
impl ReturnSnapshotGuards {
    pub(crate) fn verify_live(&self) -> Result<(), SafeError> {
        self.dependencies.verify()?;
        let user = CurrentUser::capture().map_err(blocked)?;
        self.image_quarantine.verify(&user).map_err(blocked)?;
        let (parent, _) = self
            .image_quarantine
            .directory()
            .held_location()
            .map_err(blocked)?;
        if parent.identity() != self.dependencies.data.root().directory().identity() {
            return Err(blocked("foreign current image quarantine"));
        }
        self.record.verify(&user).map_err(blocked)?;
        Ok(())
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.dependencies.binding
    }
    pub(crate) fn roots(&self) -> &BTreeMap<RootKind, String> {
        &self.roots
    }
    pub(super) fn installation(&self) -> &Arc<Directory> {
        self.dependencies.scope.directory()
    }
    pub(super) fn image_name(&self) -> &ComponentName {
        self.dependencies.scope.image_name()
    }
}

/// Keep this owner on every failed admit. An uncertain rename/receipt never
/// drops the actual exclusive image, resumes a process or retries an effect.
pub(crate) struct ReturnBoundaryAttempt {
    dependencies: Arc<ReturnDependencies>,
    context: Option<HeldContext>,
    image: CurrentImageEvidence,
    quarantine: Arc<PrivateDirectory>,
    record_name: String,
    record: Option<Arc<ManagerRecord>>,
    generation: u64,
    attempted: bool,
}
pub(crate) struct AdmittedReturnBoundary {
    boundary: ReturnBoundary,
    desk: HeldRoot,
    webview: HeldRoot,
    generation: u64,
}
impl AdmittedReturnBoundary {
    pub(crate) fn into_parts(self) -> (ReturnBoundary, HeldRoot, HeldRoot, u64) {
        (self.boundary, self.desk, self.webview, self.generation)
    }
}
/// Preparation itself retains irreversible branch claims and newly acquired
/// native observations before any later fallible read/capture. The coordinator
/// stores this owner before calling either one-use preparation method.
pub(crate) struct ReturnBoundaryPreparation {
    attempted: bool,
    candidate: Option<NoHistoricalLaunch>,
    historical: Option<HistoricalReturnCustody>,
    dependencies: Option<Arc<ReturnDependencies>>,
    context: Option<HeldContext>,
    image: Option<CurrentImageEvidence>,
    quarantine: Option<Arc<PrivateDirectory>>,
    prepared: Option<ReturnBoundaryAttempt>,
}
impl ReturnBoundaryPreparation {
    pub(crate) fn new() -> Self {
        Self {
            attempted: false,
            candidate: None,
            historical: None,
            dependencies: None,
            context: None,
            image: None,
            quarantine: None,
            prepared: None,
        }
    }
    pub(crate) fn prepare_normal(
        &mut self,
        inputs: ReturnBoundaryInputs<'_>,
        store: &mut JournalStore,
    ) -> Result<ReturnBoundaryAttempt, SafeError> {
        if self.attempted {
            return Err(blocked("Return preparation requires reconciliation"));
        }
        self.attempted = true;
        self.historical = Some(HistoricalReturnCustody::Terminal(inputs.historical));
        self.prepare_custody(
            PreparationInputs {
                binding: inputs.binding,
                installation: inputs.installation,
                data: inputs.data,
                exclusive: inputs.exclusive,
                installer: inputs.installer,
                historical: self
                    .historical
                    .as_ref()
                    .expect("retained historical terminal")
                    .clone(),
                package: inputs.package,
                scope: inputs.scope,
                original_bundle: inputs.original_bundle,
                original_context: inputs.original_context,
                registration: inputs.registration,
                shortcuts: inputs.shortcuts,
            },
            store,
        )
    }
    pub(crate) fn prepare_failed_installer(
        &mut self,
        inputs: FailedInstallerReturnInputs<'_>,
        store: &mut JournalStore,
    ) -> Result<ReturnBoundaryAttempt, SafeError> {
        if self.attempted {
            return Err(blocked("Return preparation requires reconciliation"));
        }
        self.attempted = true;
        self.candidate = Some(inputs.no_historical);
        inputs.installer.verify().map_err(blocked)?;
        if inputs.installer.job_kind() != JobKind::Installer
            || inputs.installer.root_identity() != inputs.data.root().directory().identity()
        {
            return Err(blocked("foreign installer terminal custody"));
        }
        self.historical = Some(HistoricalReturnCustody::NeverCreated(Arc::new(
            self.candidate
                .as_ref()
                .expect("retained no-launch candidate")
                .claim_return()?,
        )));
        self.prepare_custody(
            PreparationInputs {
                binding: inputs.binding,
                installation: inputs.installation,
                data: inputs.data,
                exclusive: inputs.exclusive,
                installer: inputs.installer,
                historical: self
                    .historical
                    .as_ref()
                    .expect("retained return claim")
                    .clone(),
                package: inputs.package,
                scope: inputs.scope,
                original_bundle: inputs.original_bundle,
                original_context: inputs.original_context,
                registration: inputs.registration,
                shortcuts: inputs.shortcuts,
            },
            store,
        )
    }
    fn prepare_custody(
        &mut self,
        inputs: PreparationInputs<'_>,
        store: &mut JournalStore,
    ) -> Result<ReturnBoundaryAttempt, SafeError> {
        inputs
            .exclusive
            .verify_root(inputs.installation.root())
            .map_err(blocked)?;
        inputs.data.verify_installation(&inputs.installation)?;
        inputs.scope.verify().map_err(blocked)?;
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        let expected_installation = sha256(
            &serde_json::to_vec(&(
                PRODUCT_IDENTIFIER,
                user.sid_text(),
                inputs
                    .scope
                    .original_path()
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>(),
            ))
            .map_err(blocked)?,
        );
        if expected_installation != inputs.binding.user_installation {
            return Err(blocked("registered installation binding differs"));
        }
        let inspection = store.inspect(&inputs.binding)?;
        let state = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| blocked("missing journal"))?;
        if inspection.blocked
            || state.requires_reconciliation()
            || !matches!(
                state.phase(),
                JournalPhase::RecoveryRequired
                    | JournalPhase::HistoricalActive
                    | JournalPhase::InstalledUnconfirmed
            )
        {
            return Err(blocked("Return requires reconciled post-launch state"));
        }
        let generation = state.generation();
        store.verify_windows_binding(inputs.installation.root(), &inputs.binding, generation)?;
        inputs.historical.verify_journal(store, generation)?;
        let original_bundle = {
            let mut journal = ContextJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                inputs.binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            Arc::new(
                RetainedInstallationBundle::reopen(
                    inputs.data.root().clone(),
                    inputs.original_bundle.reference(),
                    &user,
                    &mut journal,
                )
                .map_err(blocked)?,
            )
        };
        {
            let mut journal = RegistrationJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                inputs.binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            inputs
                .registration
                .verify_retained(&mut journal)
                .map_err(blocked)?;
        }
        inputs.shortcuts.verify_retained().map_err(blocked)?;
        let package_root = Arc::new(
            PrivateDirectory::open_existing(
                inputs.data.root().directory().clone(),
                name("package")?,
                &user,
            )
            .map_err(blocked)?,
        );
        if package_root.directory().identity() != inputs.package.root_identity() {
            return Err(blocked("retained package child differs"));
        }
        let original_context = inputs
            .original_context
            .retain_return_custody(&user)
            .map_err(blocked)?;
        original_context
            .verify_data_root(inputs.data.root())
            .map_err(blocked)?;
        let source_context_bytes = original_context.snapshot().encode()?;
        if original_context.snapshot().context_id != inputs.binding.source_context {
            return Err(blocked("foreign original context"));
        }
        let mut roles = Vec::new();
        for role in [
            ManifestRole::SourceBundle,
            ManifestRole::SourceContext,
            ManifestRole::Registration,
            ManifestRole::Shortcuts,
            ManifestRole::FreshTargetContext,
            ManifestRole::SourceHandoffExit,
        ] {
            let guard = store.retain_role_guard(
                inputs.installation.root().clone(),
                &inputs.binding,
                generation,
                role,
            )?;
            let expected = match role {
                ManifestRole::SourceBundle => Some(sha256(
                    &original_bundle
                        .source_manifest()
                        .encode()
                        .map_err(blocked)?,
                )),
                ManifestRole::SourceContext => Some(sha256(&source_context_bytes)),
                ManifestRole::Registration => Some(inputs.registration.digest().to_owned()),
                ManifestRole::Shortcuts => Some(inputs.shortcuts.digest().to_owned()),
                _ => None,
            };
            if expected
                .is_some_and(|expected| sha256(&guard.read().unwrap_or_default()) != expected)
            {
                return Err(blocked("retained original role differs"));
            }
            roles.push((role, guard));
        }
        // Terminal custody is checked before capturing any current data. Each
        // launched process retains its actual empty authenticated private job.
        inputs.installer.verify().map_err(blocked)?;
        inputs.historical.verify(
            &inputs.binding,
            &inputs.installation,
            &inputs.data,
            &inputs.scope,
            &inputs.installer,
        )?;
        if inputs.installer.job_kind() != JobKind::Installer {
            return Err(blocked("wrong terminal job kind"));
        }
        original_context
            .capture_current_retaining(&mut self.context, &user)
            .map_err(blocked)?;
        let context = self.context.as_mut().expect("retained current context");
        let exclusions = ConfiguredInventory::capture_for_context(context)
            .map_err(blocked)?
            .into_exclusions(inputs.scope.directory().clone(), inputs.data.root().clone())
            .map_err(blocked)?;
        exclusions.verify_context(context).map_err(blocked)?;
        self.dependencies = Some(Arc::new(ReturnDependencies {
            binding: inputs.binding,
            installation: inputs.installation,
            data: inputs.data,
            exclusive: inputs.exclusive.witness().map_err(blocked)?,
            installer: inputs.installer,
            historical: inputs.historical,
            package: inputs.package,
            package_root,
            scope: inputs.scope,
            original_bundle,
            original_context,
            exclusions,
            roles,
        }));
        let dependencies = self
            .dependencies
            .as_ref()
            .expect("retained return dependencies");
        dependencies.verify()?;
        capture_image(dependencies, &mut self.image)?;
        let attempt = uuid::Uuid::new_v4().simple().to_string();
        self.quarantine = Some(Arc::new(
            PrivateDirectory::create_new(
                dependencies.data.root().directory().clone(),
                name(&format!("return-image-{attempt}"))?,
                &user,
            )
            .map_err(blocked)?,
        ));
        let quarantine = self.quarantine.as_ref().expect("retained image quarantine");
        dependencies
            .scope
            .directory()
            .require_same_volume(quarantine.directory())
            .map_err(blocked)?;
        self.prepared = Some(ReturnBoundaryAttempt {
            dependencies: dependencies.clone(),
            context: self.context.take(),
            image: self.image.take().expect("retained current image"),
            quarantine: quarantine.clone(),
            record_name: format!("return-boundary-{attempt}.json"),
            record: None,
            generation,
            attempted: false,
        });
        self.prepared
            .as_ref()
            .expect("retained prepared Return")
            .verify_before_effect(store)?;
        Ok(self.prepared.take().expect("verified prepared Return"))
    }
}

impl ReturnBoundaryAttempt {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    fn verify_before_effect(&self, store: &mut JournalStore) -> Result<(), SafeError> {
        self.dependencies.verify()?;
        store.verify_windows_binding(
            self.dependencies.installation.root(),
            &self.dependencies.binding,
            self.generation,
        )?;
        self.dependencies
            .historical
            .verify_journal(store, self.generation)?;
        let context = self
            .context
            .as_ref()
            .ok_or_else(|| blocked("current context transferred"))?;
        self.dependencies
            .exclusions
            .verify_context(context)
            .map_err(blocked)?;
        match &self.image {
            CurrentImageEvidence::Fenced(fence) => {
                let fence = fence.lock();
                fence.verify().map_err(blocked)?;
                self.dependencies.historical.verify_image(&fence)?;
            }
            CurrentImageEvidence::Absent(absence) => absence.verify().map_err(blocked)?,
        }
        Ok(())
    }
    pub(crate) fn admit(
        &mut self,
        store: &mut JournalStore,
    ) -> Result<AdmittedReturnBoundary, SafeError> {
        if self.attempted {
            return Err(blocked("Return admission requires reconciliation"));
        }
        self.verify_before_effect(store)?;
        self.attempted = true;
        let before = self.retain(
            store,
            &serde_json::json!({
                "binding": self.dependencies.binding,
                "installation": self.dependencies.scope.directory().identity(),
                "image": self.image_observation()?,
                "installer": self.dependencies.installer.terminal_reference(),
                "historical": self.dependencies.historical.observation()?,
            }),
        )?;
        let expected = self.retain(store, &serde_json::json!({
            "quarantine": self.quarantine.directory().identity(), "imageName": "current-image.exe",
            "recordName": self.record_name,
        }))?;
        let effect_id = uuid::Uuid::new_v4().to_string();
        self.generation = store.append(
            self.generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: effect_id.clone(),
                    kind: EffectKind::FenceHistoricalImage,
                    before,
                    expected_postconditions: expected,
                },
            },
        )?;
        let intent_generation = self.generation;
        // Any subsequent failure leaves the journal pending and all native
        // owners in this attempt. No automatic replay or lease release.
        self.verify_before_effect(store)?;
        if let CurrentImageEvidence::Fenced(fence) = &self.image {
            fence
                .lock()
                .rename_to(
                    self.quarantine.directory().clone(),
                    name("current-image.exe")?,
                )
                .map_err(blocked)?;
        }
        self.verify_before_effect(store)?;
        let roots = self
            .context
            .as_ref()
            .expect("untransferred context")
            .root_identities();
        let observed_image = self.image_observation()?;
        if let CurrentImageEvidence::Fenced(fence) = &self.image {
            let (parent, leaf) = fence.lock().held_location().map_err(blocked)?;
            if &parent != self.quarantine.directory().identity()
                || leaf != name("current-image.exe")?
            {
                return Err(blocked("current fence quarantine differs"));
            }
        }
        let record = Arc::new(
            ManagerRecord::create(
                self.dependencies.data.root().clone(),
                &self.record_name,
                &serde_json::json!({
                    "schema": 1, "binding": self.dependencies.binding,
                    "installation": self.dependencies.scope.directory().identity(),
                    "data": self.dependencies.data.reference(), "roots": roots,
                    "originalBundle": self.dependencies.original_bundle.reference(),
                    "originalContext": self.dependencies.original_context.snapshot(),
                    "installer": self.dependencies.installer.terminal_reference(),
                    "historical": self.dependencies.historical.observation()?,
                    "image": observed_image, "quarantine": self.quarantine.directory().identity(),
                    "configuredExclusions": self.dependencies.exclusions.configuration_identity(),
                }),
                &CurrentUser::capture().map_err(blocked)?,
            )
            .map_err(blocked)?,
        );
        self.record = Some(record.clone());
        self.verify_before_effect(store)?;
        let observed = self.retain(
            store,
            &(&self.record_name, record.reference(), &observed_image),
        )?;
        let receipt = store.retain_effect_receipt(&effect_id, Observation::Applied, &observed)?;
        self.generation = store.append(
            self.generation,
            JournalEvent::Observed {
                effect_id,
                intent_generation,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )?;
        let boundary = ReturnBoundary::from_native(
            ReturnSnapshotGuards {
                dependencies: self.dependencies.clone(),
                roots,
                image_quarantine: self.quarantine.clone(),
                record,
            },
            self.image.clone(),
        )?;
        let context = self.context.take().expect("untransferred context");
        let desk = context.tree(RootKind::Desk).root().clone();
        let webview = context.tree(RootKind::WebView).root().clone();
        drop(context); // Do not retain descendants across later context moves.
        Ok(AdmittedReturnBoundary {
            boundary,
            desk,
            webview,
            generation: self.generation,
        })
    }
    fn retain(
        &self,
        store: &mut JournalStore,
        value: &impl Serialize,
    ) -> Result<String, SafeError> {
        store.retain_manifest(&serde_json::to_vec(value).map_err(blocked)?)
    }
    fn image_observation(&self) -> Result<serde_json::Value, SafeError> {
        match &self.image {
            CurrentImageEvidence::Fenced(fence) => {
                let fence = fence.lock();
                let (parent, name) = fence.held_location().map_err(blocked)?;
                Ok(
                    serde_json::json!({"fenced": fence.identity(), "digest": fence.digest().map_err(blocked)?,
                    "parent": parent, "name": name.os_string().encode_wide().collect::<Vec<_>>() }),
                )
            }
            CurrentImageEvidence::Absent(absence) => {
                absence.verify().map_err(blocked)?;
                Ok(
                    serde_json::json!({"absentParent": self.dependencies.scope.directory().identity(),
                    "name": self.dependencies.scope.image_name().os_string().encode_wide().collect::<Vec<_>>() }),
                )
            }
        }
    }
    pub(crate) fn record_reference(&self) -> Option<(&str, &ManagerRecordReference)> {
        self.record
            .as_ref()
            .map(|record| (self.record_name.as_str(), record.reference()))
    }
}

fn capture_image(
    dependencies: &ReturnDependencies,
    retained: &mut Option<CurrentImageEvidence>,
) -> Result<(), SafeError> {
    dependencies.verify()?;
    let directory = dependencies.scope.directory().clone();
    let name = dependencies.scope.image_name().clone();
    let historical = match &dependencies.historical {
        HistoricalReturnCustody::Terminal(historical) => Some(historical.as_ref()),
        HistoricalReturnCustody::NeverCreated(proof) => {
            proof.verify_live()?;
            None
        }
    };
    capture_current_image(retained, directory, name, historical)?;
    dependencies.verify()?;
    if let Some(CurrentImageEvidence::Fenced(fence)) = retained.as_ref() {
        dependencies.historical.verify_image(&fence.lock())?;
    }
    Ok(())
}

/// Actual image observation only; this helper cannot mint Return admission.
/// Callers supply terminal equality exclusively for the post-launch branch.
fn capture_current_image(
    retained: &mut Option<CurrentImageEvidence>,
    directory: Arc<Directory>,
    name: ComponentName,
    historical: Option<&TerminalProcessJob>,
) -> Result<(), SafeError> {
    if retained.is_some() {
        return Err(blocked("current image custody is already retained"));
    }
    match directory.open_file(name.clone(), FileAccess::Read) {
        Ok(image) => {
            let identity = image.identity().clone();
            let digest = image.digest().map_err(blocked)?;
            if let Some(historical) = historical {
                historical
                    .verify_image(&identity, &digest)
                    .map_err(blocked)?;
            }
            drop(image);
            let fence =
                ImageFence::acquire(directory, name, &identity, &digest).map_err(blocked)?;
            // The caller's preparation owns the exclusive guard before every
            // fallible post-acquisition identity/dependency check.
            *retained = Some(CurrentImageEvidence::Fenced(Arc::new(Mutex::new(fence))));
            #[cfg(test)]
            if IMAGE_CAPTURE_FAILURE.replace(false) {
                return Err(blocked("injected post-acquisition image verification"));
            }
            let Some(CurrentImageEvidence::Fenced(fence)) = retained.as_ref() else {
                unreachable!("exclusive image retained before verification")
            };
            let fence = fence.lock();
            if let Some(historical) = historical {
                historical
                    .verify_image(fence.identity(), fence.digest().map_err(blocked)?)
                    .map_err(blocked)?;
            }
            Ok(())
        }
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
            *retained = Some(CurrentImageEvidence::Absent(
                VerifiedImageAbsence::capture(directory, name).map_err(blocked)?,
            ));
            Ok(())
        }
        Err(failure) => Err(blocked(failure)),
    }
}

#[cfg(test)]
thread_local! {
    static IMAGE_CAPTURE_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
#[path = "../../tests/version_history_return_boundary_windows.rs"]
#[allow(non_snake_case)]
mod tests;
