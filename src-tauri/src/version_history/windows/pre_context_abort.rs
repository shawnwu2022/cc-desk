//! Pure-private pre-context abort. Browser shutdown/M0 content are deliberately
//! not invented: the healthy transcript must contain no source mutation at all.
use super::{
    durability::MarkerStore,
    lease::ControlLease,
    registration_state::HeldRegistrationState,
    scope::ConfiguredInventory,
    shortcuts::HeldProductShortcuts,
    source_lifecycle::SourceHandoff,
    startup::{InstallationControl, OrdinaryStartup, RegisteredStartupSource, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{JournalBinding, JournalStore},
        maintenance::ActiveContextMarker,
        verified_package::sha256,
    },
};
use std::{os::windows::ffi::OsStrExt, sync::Arc};

#[derive(Clone)]
pub(crate) struct PrivateAbortEvidence<'a> {
    binding: JournalBinding,
    source: &'a RegisteredStartupSource,
    inventory: &'a ConfiguredInventory,
    registration: &'a HeldRegistrationState,
    shortcuts: &'a HeldProductShortcuts,
    owner: &'a Arc<SourceHandoff>,
    startup: &'a OrdinaryStartup,
    installation: &'a Arc<InstallationControl>,
    data: &'a TransactionDataRoot,
}

/// Holds the actual terminal marker/control lease borrow, unchanged source
/// guards and validated journal throughout admission/pin release. No serialized
/// state can reconstruct this outcome.
pub(crate) struct VerifiedPrivateAbort<'a> {
    evidence: PrivateAbortEvidence<'a>,
    store: &'a JournalStore,
    marker: MarkerStore<'a>,
    checkpoint: ActiveContextMarker,
}
impl VerifiedPrivateAbort<'_> {
    pub(crate) fn transaction(&self) -> &str {
        &self.evidence.binding.transaction_id
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.evidence.verify()?;
        let observed = ActiveContextMarker::decode(self.marker.current().map_err(blocked)?)?;
        if observed.encode()? != self.checkpoint.encode()? {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        let inspection = self.store.inspect(&self.evidence.binding)?;
        let terminal = ActiveContextMarker::pre_context_aborted(&inspection)?;
        if terminal.encode()? != self.checkpoint.encode()? {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        Ok(())
    }
    pub(crate) fn verify_document(
        &self,
        caller: &crate::cli::snapshot::CallerIdentity,
        transaction: &str,
    ) -> Result<(), SafeError> {
        if transaction != self.transaction() {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        self.evidence
            .owner
            .verify_original_document(caller, transaction)
    }
}

pub(crate) fn publish_private_abort<'a>(
    evidence: PrivateAbortEvidence<'a>,
    store: &'a mut JournalStore,
    control: &'a ControlLease,
) -> Result<VerifiedPrivateAbort<'a>, SafeError> {
    let root = evidence.installation.root().clone();
    control.verify_root(&root).map_err(blocked)?;
    let proof = store.admit_private_abort(evidence.clone())?;
    store.abort_pre_context(&proof)?;
    let checkpoint = ActiveContextMarker::pre_context_aborted(&store.inspect(evidence.binding())?)?;
    let mut marker = MarkerStore::open_existing(root, control)
        .map_err(blocked)?
        .ok_or_else(|| error("HISTORY_EARLY_ABORT_BLOCKED"))?;
    if ActiveContextMarker::decode(marker.current().map_err(blocked)?)?.binding()
        != evidence.binding()
    {
        return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
    }
    evidence.verify()?;
    marker.append(&checkpoint, store).map_err(blocked)?;
    drop(proof);
    let outcome = VerifiedPrivateAbort {
        evidence,
        store,
        marker,
        checkpoint,
    };
    outcome.verify()?;
    Ok(outcome)
}

/// The initial barrier precedes every private-copy effect and all manager
/// process creation. Failed/torn publication remains recovery-owned.
pub(crate) fn publish_source_transition(
    installation: &InstallationControl,
    control: &ControlLease,
    store: &mut JournalStore,
    binding: &JournalBinding,
) -> Result<(), SafeError> {
    control.verify_root(installation.root()).map_err(blocked)?;
    store.verify_windows_binding(installation.root(), binding, 0)?;
    let checkpoint = ActiveContextMarker::transition_from(&store.inspect(binding)?)?;
    match MarkerStore::open_existing(installation.root().clone(), control).map_err(blocked)? {
        None => {
            MarkerStore::create(installation.root().clone(), control, &checkpoint, store)
                .map_err(blocked)?;
        }
        Some(mut marker) => {
            let prior = ActiveContextMarker::decode(marker.current().map_err(blocked)?)?;
            if prior.binding() == binding || !prior.is_terminal() {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
            let mut previous = JournalStore::open_windows_transaction(
                installation.root().clone(),
                &prior.binding().transaction_id,
            )?;
            previous.bind_existing(prior.binding())?;
            marker
                .append_successor(&checkpoint, &mut previous, store)
                .map_err(blocked)?;
        }
    }
    control.verify_root(installation.root()).map_err(blocked)
}
impl<'a> PrivateAbortEvidence<'a> {
    pub(crate) fn capture(
        binding: JournalBinding,
        original: (
            &'a RegisteredStartupSource,
            &'a ConfiguredInventory,
            &'a HeldRegistrationState,
            &'a HeldProductShortcuts,
        ),
        ownership: (
            &'a Arc<SourceHandoff>,
            &'a OrdinaryStartup,
            &'a Arc<InstallationControl>,
            &'a TransactionDataRoot,
        ),
    ) -> Result<Self, SafeError> {
        let (source, inventory, registration, shortcuts) = original;
        let (owner, startup, installation, data) = ownership;
        let evidence = Self {
            binding,
            source,
            inventory,
            registration,
            shortcuts,
            owner,
            startup,
            installation,
            data,
        };
        evidence.verify()?;
        Ok(evidence)
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.binding
    }
    pub(crate) fn verify_writer(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.verify()?;
        store.verify_windows_binding(self.installation.root(), &self.binding, generation)
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.owner
            .verify_private_abort_owner(&self.binding.transaction_id)?;
        self.startup
            .shared()
            .verify_root(self.startup.control().root())
            .map_err(blocked)?;
        self.data.verify_installation(self.installation)?;
        self.source.installation().recheck().map_err(blocked)?;
        self.source.bundle().tree().verify().map_err(blocked)?;
        if self
            .source
            .bundle()
            .manifest()
            .logical_digest()
            .map_err(blocked)?
            != self.binding.source_bundle
            || self.data.transaction_id() != self.binding.transaction_id
            || sha256(&root_bytes(
                self.inventory,
                self.owner,
                self.source,
                self.data,
            )?) != self.binding.roots
        {
            return Err(error("HISTORY_SOURCE_CHANGED"));
        }
        // Shared CLI files may legitimately change while the original app is
        // still alive. This private-only route proves root ownership, not a
        // quiescent M0 snapshot or ownership of external CLI data.
        self.inventory.desk_root().recheck().map_err(blocked)?;
        self.registration.recheck().map_err(blocked)?;
        self.shortcuts.verify().map_err(blocked)?;
        Ok(())
    }
    pub(crate) fn observations(&self) -> Result<[Vec<u8>; 4], SafeError> {
        self.verify()?;
        Ok([
            self.source.bundle().manifest().encode().map_err(blocked)?,
            root_bytes(self.inventory, self.owner, self.source, self.data)?,
            serde_json::to_vec(&(
                self.registration.encode().map_err(blocked)?,
                self.shortcuts.encode().map_err(blocked)?,
            ))
            .map_err(blocked)?,
            self.owner
                .private_abort_observation(&self.binding.transaction_id)?,
        ])
    }
}
fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_EARLY_ABORT_BLOCKED")
}
pub(super) fn root_bytes(
    inventory: &ConfiguredInventory,
    owner: &SourceHandoff,
    source: &RegisteredStartupSource,
    data: &TransactionDataRoot,
) -> Result<Vec<u8>, SafeError> {
    let root = inventory.desk_root();
    root.recheck().map_err(blocked)?;
    let desk = if let Some(directory) = root.directory() {
        serde_json::json!({"present":directory.identity()})
    } else {
        let (parent, suffix) = root
            .absent_location()
            .ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?;
        serde_json::json!({"absentParent":parent.identity(),"suffix":suffix.iter().map(|name|name.os_string().encode_wide().collect::<Vec<_>>()).collect::<Vec<_>>()})
    };
    serde_json::to_vec(&(
        desk,
        owner.udf().identity(),
        source.installation().directory().identity(),
        data.root().directory().identity(),
    ))
    .map_err(blocked)
}
