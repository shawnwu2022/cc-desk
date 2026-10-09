//! First source snapshot admission. Every input is a retained native capability;
//! encoded manifests are derived here from complete actual observations.
use super::{
    context::{
        bundle_restore::RetainedInstallationBundle, ContextJournal, HeldBundle, HeldContext,
        HeldRoot,
    },
    fence::ImageFence,
    files::{ComponentName, PrivateDirectory},
    lease::{ExclusiveLease, ExclusiveLeaseWitness},
    registration_state::{RegistrationJournal, RetainedRegistrationState},
    scope::{ConfiguredExclusions, ConfiguredInventory, FencedInstallation},
    security::CurrentUser,
    shortcuts::RetainedProductShortcuts,
    source_lifecycle::SourceHandoffTerminal,
    space::SpaceAdmission,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{
            JournalBinding, JournalEvent, JournalStore, ManifestRole, RetainedRoleGuard, RootKind,
        },
        maintenance::SnapshotBoundary,
        policy::PRODUCT_IDENTIFIER,
        snapshot::SnapshotManifest,
        verified_package::sha256,
    },
};
use parking_lot::Mutex;
use std::{collections::BTreeMap, ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_SOURCE_SNAPSHOT_BLOCKED")
}

pub(crate) struct SourceSnapshotInputs<'a> {
    pub(crate) binding: JournalBinding,
    pub(crate) installation: Arc<InstallationControl>,
    pub(crate) data: Arc<TransactionDataRoot>,
    pub(crate) exclusive: &'a ExclusiveLease,
    pub(crate) terminal: Arc<SourceHandoffTerminal>,
    pub(crate) scope: Arc<FencedInstallation>,
    pub(crate) fence: Arc<Mutex<ImageFence>>,
    pub(crate) image_quarantine: Arc<PrivateDirectory>,
    pub(crate) original_bundle: Arc<RetainedInstallationBundle>,
    pub(crate) registration: &'a RetainedRegistrationState,
    pub(crate) shortcuts: &'a RetainedProductShortcuts,
}

/// No current Desk/UDF descendant readers are retained here. The context
/// executor owns them and performs its recorded release/re-admission gap.
pub(crate) struct SourceSnapshotGuards {
    binding: JournalBinding,
    roots: BTreeMap<RootKind, String>,
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    exclusive: ExclusiveLeaseWitness,
    terminal: Arc<SourceHandoffTerminal>,
    scope: Arc<FencedInstallation>,
    image_quarantine: Arc<PrivateDirectory>,
    exclusions: Arc<ConfiguredExclusions>,
    original_bundle: Arc<RetainedInstallationBundle>,
    roles: Vec<(ManifestRole, RetainedRoleGuard)>,
}
impl SourceSnapshotGuards {
    pub(crate) fn verify_live(&self) -> Result<(), SafeError> {
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.data.verify_installation(&self.installation)?;
        self.terminal.verify(&self.binding)?;
        self.scope.verify().map_err(blocked)?;
        if self.scope.source_process_identity() != self.terminal.exit().host_identity() {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        let user = CurrentUser::capture().map_err(blocked)?;
        self.image_quarantine.verify(&user).map_err(blocked)?;
        self.exclusions.verify_external().map_err(blocked)?;
        self.original_bundle.verify(&user).map_err(blocked)?;
        for (role, guard) in &self.roles {
            guard.verify_role(&self.binding, *role, self.installation.root())?;
        }
        // FencedInstallation retains the exact same fence Arc. Do not lock it
        // recursively: context effects call this while holding that mutex and
        // separately verify the actual fence immediately around their effect.
        Ok(())
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.binding
    }
    pub(crate) fn roots(&self) -> &BTreeMap<RootKind, String> {
        &self.roots
    }
}

pub(crate) struct AdmittedSourceSnapshot {
    boundary: SnapshotBoundary,
    manifest: SnapshotManifest,
    generation: u64,
}
impl AdmittedSourceSnapshot {
    pub(crate) fn capture(
        inputs: SourceSnapshotInputs<'_>,
        current_bundle: &HeldBundle,
        context: &mut HeldContext,
        store: &mut JournalStore,
    ) -> Result<Self, SafeError> {
        let mut inputs = inputs;
        let binding = &inputs.binding;
        inputs.data.verify_installation(&inputs.installation)?;
        inputs
            .exclusive
            .verify_root(inputs.installation.root())
            .map_err(blocked)?;
        inputs.terminal.verify(binding)?;
        inputs.scope.verify().map_err(blocked)?;
        if inputs.scope.source_process_identity() != inputs.terminal.exit().host_identity() {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        let installation_identity = sha256(
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
        if installation_identity != binding.user_installation
            || inputs.data.transaction_id() != binding.transaction_id
        {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        inputs.image_quarantine.verify(&user).map_err(blocked)?;
        let (quarantine_parent, quarantine_name) = inputs
            .image_quarantine
            .directory()
            .held_location()
            .map_err(blocked)?;
        if quarantine_parent.identity() != inputs.data.root().directory().identity()
            || quarantine_name != ComponentName::new(OsStr::new("source-image")).map_err(blocked)?
        {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        {
            let fence = inputs.fence.lock();
            inputs.scope.verify_fence(&fence).map_err(blocked)?;
            let (parent, name) = fence.held_location().map_err(blocked)?;
            if &parent != inputs.image_quarantine.directory().identity()
                || name != ComponentName::new(OsStr::new("source-image.exe")).map_err(blocked)?
            {
                return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
            }
        }
        current_bundle.tree().verify().map_err(blocked)?;
        inputs.original_bundle.verify(&user).map_err(blocked)?;
        if inputs.original_bundle.directory().identity() != inputs.scope.directory().identity()
            || current_bundle.manifest().tree != inputs.original_bundle.source_manifest().tree
            || current_bundle
                .manifest()
                .logical_digest()
                .map_err(blocked)?
                != binding.source_bundle
            || inputs
                .original_bundle
                .source_manifest()
                .logical_digest()
                .map_err(blocked)?
                != binding.source_bundle
        {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        let exclusions = Arc::new(
            ConfiguredInventory::capture_for_context(context)
                .map_err(blocked)?
                .into_exclusions(inputs.scope.directory().clone(), inputs.data.root().clone())
                .map_err(blocked)?,
        );
        exclusions.verify_context(context).map_err(blocked)?;
        let space = SpaceAdmission::context_copy(inputs.data.root().clone(), context)?;
        verify_original_roots(&inputs, context)?;
        let inspection = store.inspect(binding)?;
        let state = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if inspection.blocked || !state.source_snapshot_candidate() {
            return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
        }
        let mut generation = state.generation();
        store.verify_windows_binding(inputs.installation.root(), binding, generation)?;
        // Re-open through this transaction's actual private data root and the
        // retained record identity. A same-content copy from another recovery
        // root/transaction cannot substitute for original restoration custody.
        inputs.original_bundle = {
            let mut journal = ContextJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                binding.clone(),
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
                binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            inputs
                .registration
                .verify_original(&mut journal)
                .map_err(blocked)?;
        }
        inputs.shortcuts.verify_original().map_err(blocked)?;
        let manifest = SnapshotManifest::observe_durable_context(binding, context)?;
        retain_exact_role(
            store,
            binding,
            &mut generation,
            ManifestRole::SourceBundle,
            &inputs
                .original_bundle
                .source_manifest()
                .encode()
                .map_err(blocked)?,
        )?;
        retain_exact_role(
            store,
            binding,
            &mut generation,
            ManifestRole::SourceContext,
            &manifest.encode()?,
        )?;
        let mut roles = Vec::new();
        for role in [
            ManifestRole::SourceBundle,
            ManifestRole::SourceContext,
            ManifestRole::Registration,
            ManifestRole::Shortcuts,
            ManifestRole::SourceHandoffExit,
        ] {
            let guard = store.retain_role_guard(
                inputs.installation.root().clone(),
                binding,
                generation,
                role,
            )?;
            let expected = match role {
                ManifestRole::Registration => Some(inputs.registration.digest()),
                ManifestRole::Shortcuts => Some(inputs.shortcuts.digest()),
                _ => None,
            };
            if let Some(expected) = expected {
                if sha256(&guard.read()?) != expected {
                    return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
                }
            }
            roles.push((role, guard));
        }
        exclusions.verify_context(context).map_err(blocked)?;
        space.verify()?;
        current_bundle.tree().verify().map_err(blocked)?;
        {
            let mut journal = RegistrationJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            inputs
                .registration
                .verify_original(&mut journal)
                .map_err(blocked)?;
        }
        inputs.shortcuts.verify_original().map_err(blocked)?;
        inputs
            .scope
            .verify_fence(&inputs.fence.lock())
            .map_err(blocked)?;
        let guards = SourceSnapshotGuards {
            binding: inputs.binding,
            roots: context.root_identities(),
            installation: inputs.installation.clone(),
            data: inputs.data,
            exclusive: inputs.exclusive.witness().map_err(blocked)?,
            terminal: inputs.terminal,
            scope: inputs.scope,
            image_quarantine: inputs.image_quarantine,
            exclusions,
            original_bundle: inputs.original_bundle.clone(),
            roles,
        };
        let boundary = SnapshotBoundary::from_source(guards)?;
        {
            let mut journal = ContextJournal::new(
                store,
                inputs.installation.root().clone(),
                inputs.exclusive,
                boundary.binding().clone(),
                generation,
            )
            .map_err(blocked)?;
            inputs
                .original_bundle
                .record_preserved(current_bundle, &boundary, &user, &mut journal)
                .map_err(blocked)?;
            generation = journal.generation();
        }
        Ok(Self {
            boundary,
            manifest,
            generation,
        })
    }
    pub(crate) fn into_parts(self) -> (SnapshotBoundary, SnapshotManifest, u64) {
        (self.boundary, self.manifest, self.generation)
    }
}

fn verify_original_roots(
    inputs: &SourceSnapshotInputs<'_>,
    context: &HeldContext,
) -> Result<(), SafeError> {
    context.verify_durable().map_err(blocked)?;
    for kind in [RootKind::Desk, RootKind::WebView] {
        if let HeldRoot::Present(root) = context.tree(kind).root() {
            root.require_renameable().map_err(blocked)?;
        }
    }
    let desk = match context.tree(RootKind::Desk).root() {
        HeldRoot::Present(root) => serde_json::json!({"present":root.identity()}),
        HeldRoot::Absent { parent, name } => {
            serde_json::json!({"absentParent":parent.identity(),"suffix":[name.os_string().encode_wide().collect::<Vec<_>>()]})
        }
    };
    let HeldRoot::Present(udf) = context.tree(RootKind::WebView).root() else {
        return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
    };
    if udf.identity() != inputs.terminal.udf().identity() {
        return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
    }
    let roots = sha256(
        &serde_json::to_vec(&(
            desk,
            udf.identity(),
            inputs.scope.directory().identity(),
            inputs.data.root().directory().identity(),
        ))
        .map_err(blocked)?,
    );
    if roots != inputs.binding.roots {
        return Err(error("HISTORY_SOURCE_SNAPSHOT_BLOCKED"));
    }
    Ok(())
}
fn retain_exact_role(
    store: &mut JournalStore,
    binding: &JournalBinding,
    generation: &mut u64,
    role: ManifestRole,
    bytes: &[u8],
) -> Result<(), SafeError> {
    let digest = store.retain_manifest(bytes)?;
    let inspection = store.inspect(binding)?;
    if inspection.blocked {
        return Err(error("HISTORY_RECOVERY_REQUIRED"));
    }
    let state = inspection
        .last_valid
        .as_ref()
        .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
    match state.manifest(role) {
        Some(expected) if expected == digest => {}
        Some(_) => return Err(error("HISTORY_SOURCE_CHANGED")),
        None => *generation = store.append(*generation, JournalEvent::Manifest { role, digest })?,
    }
    Ok(())
}
