//! Bounded read-only recovery entry after the initial manager process is gone.
//! Retained records locate actual protected material; they never recreate live
//! child/job/terminal authority. No installer, restore or marker write is exposed.
use super::{
    durability::MarkerStore,
    manager_handoff::HandoffManifest,
    manager_process::RetainedManagerInspection,
    process::ExactProcess,
    security::CurrentUser,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{JournalBinding, JournalStore, ManifestRole, SwitchJournal},
        maintenance::ActiveContextMarker,
        manager_entry::ManagerRequest,
        manager_types::ManagerStatus,
    },
};
use std::sync::Arc;

fn unavailable(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RECOVERY_REQUIRED")
}

pub(crate) struct ReenteredManager {
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    binding: JournalBinding,
    material: RetainedManagerInspection,
    current: ExactProcess,
}
impl ReenteredManager {
    /// With no CLI selector, the protected active marker identifies the only
    /// candidate transaction. The actual current image must then be the exact
    /// retained manager object belonging to that transaction's complete bundle.
    pub(crate) fn open(request: Option<&ManagerRequest>) -> Result<Self, SafeError> {
        let installation = InstallationControl::open(false)?;
        Self::open_under(installation, request)
    }
    fn open_under(
        installation: Arc<InstallationControl>,
        request: Option<&ManagerRequest>,
    ) -> Result<Self, SafeError> {
        let user = CurrentUser::capture().map_err(unavailable)?;
        user.require_unelevated().map_err(unavailable)?;
        let control = installation.acquire_control()?;
        let marker = {
            let stored = MarkerStore::open_existing(installation.root().clone(), &control)
                .map_err(unavailable)?
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            ActiveContextMarker::decode(stored.current().map_err(unavailable)?)?
        };
        if request
            .is_some_and(|request| request.transaction_id() != marker.binding().transaction_id)
        {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let binding = marker.binding().clone();
        let store = JournalStore::open_windows_transaction(
            installation.root().clone(),
            &binding.transaction_id,
        )?;
        let inspection = store.inspect(&binding)?;
        // A valid prefix remains diagnostic if the tail or marker checkpoint
        // is interrupted. No write binding, reconciliation or replay follows.
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        require_same_binding(journal, &binding)?;
        let handoff_digest = journal
            .manifest(ManifestRole::ManagerHandoff)
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
        let handoff: HandoffManifest =
            serde_json::from_slice(&store.read_manifest(handoff_digest)?).map_err(unavailable)?;
        if handoff.schema != 1 || handoff.binding != binding {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let data = Arc::new(TransactionDataRoot::reopen(
            installation.clone(),
            &control,
            &binding.transaction_id,
            handoff.data,
        )?);
        let material = RetainedManagerInspection::open(
            data.root().clone(),
            &binding.transaction_id,
            &handoff.bundle,
            &handoff.resume,
        )
        .map_err(unavailable)?;
        let current = ExactProcess::capture_observed(std::process::id()).map_err(unavailable)?;
        current.verify_current_user(&user).map_err(unavailable)?;
        current
            .verify_held_image(material.bundle().image())
            .map_err(unavailable)?;
        if material.bundle().source_bundle() != binding.source_bundle
            || material.diagnostic().installer_digest() != binding.target_package
            || current.terminal(0).map_err(unavailable)?.is_some()
        {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        control
            .verify_root(installation.root())
            .map_err(unavailable)?;
        drop(store);
        drop(control);
        Ok(Self {
            installation,
            data,
            binding,
            material,
            current,
        })
    }
    pub(crate) fn data(&self) -> &Arc<TransactionDataRoot> {
        &self.data
    }
    pub(crate) fn transaction_id(&self) -> &str {
        &self.binding.transaction_id
    }
    /// Fresh diagnostics only. A control/journal owner is held for this read and
    /// released before returning. Missing old process/job proof is never inferred
    /// from PIDs, absent handles, durable booleans, digests or elapsed time.
    pub(crate) fn inspect(&self) -> Result<ManagerStatus, SafeError> {
        let control = self.installation.acquire_control()?;
        self.data.verify_installation(&self.installation)?;
        self.material.verify().map_err(unavailable)?;
        let user = CurrentUser::capture().map_err(unavailable)?;
        self.current
            .verify_current_user(&user)
            .map_err(unavailable)?;
        self.current
            .verify_held_image(self.material.bundle().image())
            .map_err(unavailable)?;
        if self.current.terminal(0).map_err(unavailable)?.is_some() {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let marker = {
            let stored = MarkerStore::open_existing(self.installation.root().clone(), &control)
                .map_err(unavailable)?
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
            ActiveContextMarker::decode(stored.current().map_err(unavailable)?)?
        };
        if marker.binding() != &self.binding {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        let store = JournalStore::open_windows_transaction(
            self.installation.root().clone(),
            self.transaction_id(),
        )?;
        let inspection = store.inspect(&self.binding)?;
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        require_same_binding(journal, &self.binding)?;
        control
            .verify_root(self.installation.root())
            .map_err(unavailable)?;
        // Even a clean terminal checkpoint is a prior observation, not a live
        // proof reacquired by this process. This route offers refresh only.
        ManagerStatus::project_reentry(journal, self.material.diagnostic())
    }
}
fn require_same_binding(
    journal: &SwitchJournal,
    binding: &JournalBinding,
) -> Result<(), SafeError> {
    if journal.binding() != binding {
        return Err(error("HISTORY_HANDOFF_CHANGED"));
    }
    Ok(())
}
