//! 原管理器退出后的有界入口。普通receipt仍只读；新协议的完整返回检查点
//! 必须重新取得实际对象与独占权，再由新文档确认授权唯一返回。
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
mod return_execution;
pub(crate) use return_execution::{
    RecoveredSnapshotGuards, ReenteredReturnCheckpoint, VerifiedRecoveryImageOrigin,
};

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
        let installation = match request {
            Some(request) => InstallationControl::open_for_manager(request.transaction_id())?,
            None => InstallationControl::open(false)?,
        };
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
        if !self.installation.is_ordinary_backup() {
            if let Ok(status) = self.inspect_recovered_return() {
                return Ok(status);
            }
        }
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
        let mut store = JournalStore::open_windows_transaction(
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
        let status = ManagerStatus::project_reentry(journal, self.material.diagnostic())?;
        if self.installation.is_ordinary_backup() {
            match self.inspect_ordinary_backup(&control, &mut store) {
                Ok((location, handed_off)) => {
                    status.with_ordinary_install(Some(&location), handed_off)
                }
                Err(_) => status.with_ordinary_install(None, false),
            }
        } else {
            Ok(status)
        }
    }
    /// Re-admit held backup contents for display only. No expired process, Job,
    /// snapshot boundary, Return or installer replay owner is reconstructed.
    fn inspect_ordinary_backup(
        &self,
        control: &super::lease::ControlLease,
        store: &mut JournalStore,
    ) -> Result<(String, bool), SafeError> {
        use super::{
            context::bundle_restore::ObservedInstallationBackup,
            context::{ContextJournal, PrivateCopyManifest, RetainedContextRoots},
            files::{ComponentName, PrivateDirectory},
            manager_bundle::ManagerRecordReference,
            source_lifecycle::SourceHandoffExitManifest,
        };
        use crate::version_history::journal::{EffectKind, JournalPhase};
        if !self.installation.is_ordinary_backup() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        self.material.verify().map_err(unavailable)?;
        self.data.verify_installation(&self.installation)?;
        let user = CurrentUser::capture().map_err(unavailable)?;
        let exclusive = self
            .installation
            .leases()
            .acquire_exclusive(control)
            .map_err(unavailable)?;
        store.bind_existing(&self.binding)?;
        let state = store
            .inspect(&self.binding)?
            .last_valid
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if !matches!(
            state.phase(),
            JournalPhase::SourceSealed
                | JournalPhase::FreshReady
                | JournalPhase::Installing
                | JournalPhase::RecoveryRequired
        ) {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let exit: SourceHandoffExitManifest = serde_json::from_slice(
            &store.read_manifest(
                state
                    .manifest(ManifestRole::SourceHandoffExit)
                    .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?,
            )?,
        )
        .map_err(unavailable)?;
        let parents = exit.recovery_context_parents(&self.binding)?;
        let quarantine = Arc::new(
            PrivateDirectory::open_existing(
                self.data.root().directory().clone(),
                ComponentName::new(std::ffi::OsStr::new("source-context")).map_err(unavailable)?,
                &user,
            )
            .map_err(unavailable)?,
        );
        let (_, source_receipt) =
            store.ordinary_backup_observation(&EffectKind::VerifySourceBundleCopy)?;
        let (reference, _copy): (ManagerRecordReference, PrivateCopyManifest) =
            serde_json::from_slice(&source_receipt).map_err(unavailable)?;
        let mut journal = ContextJournal::new(
            store,
            self.installation.root().clone(),
            &exclusive,
            self.binding.clone(),
            state.generation(),
        )
        .map_err(unavailable)?;
        let bundle = ObservedInstallationBackup::reopen(
            self.data.root().clone(),
            &reference,
            &user,
            &self.binding,
        )
        .map_err(unavailable)?;
        let context = RetainedContextRoots::reopen_observation(
            parents,
            self.data.root().clone(),
            quarantine,
            &user,
            &mut journal,
        )
        .map_err(unavailable)?;
        bundle.verify(&user).map_err(unavailable)?;
        context.verify(&user).map_err(unavailable)?;
        drop(journal);
        let handed_off = match (
            store.ordinary_backup_observation(&EffectKind::InstallerCreateSuspended),
            store.ordinary_backup_observation(&EffectKind::InstallerResume),
        ) {
            (Ok((_, created)), Ok((_, resumed))) => {
                super::process::verify_ordinary_handoff_observation(
                    &created,
                    &resumed,
                    &self.binding.target_package,
                )
                .is_ok()
            }
            _ => false,
        };
        let location = self.installation.ordinary_backup_location()?;
        bundle.verify(&user).map_err(unavailable)?;
        context.verify(&user).map_err(unavailable)?;
        exclusive
            .verify_root(self.installation.root())
            .map_err(unavailable)?;
        control
            .verify_root(self.installation.root())
            .map_err(unavailable)?;
        Ok((location, handed_off))
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
