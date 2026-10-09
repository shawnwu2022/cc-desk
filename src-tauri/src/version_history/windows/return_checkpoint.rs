//! 只有原协调器持有的真实终态、空Job及完整保存对象能签发返回检查点。
use super::{
    context::{bundle_restore::BundleRestoration, ContextJournal, ContextRestoration},
    coordinator_evidence::ReturnBoundary,
    lease::ExclusiveLease,
    manager_bundle::ManagerRecordReference,
    process::{JobKind, TerminalProcessJob},
    registration_state::{RegistrationJournal, RetainedRegistrationState},
    return_boundary::ReturnBoundaryAttempt,
    scope::FencedInstallation,
    security::CurrentUser,
    shortcuts::RetainedProductShortcuts,
    source_lifecycle::SourceHandoffTerminal,
    startup::{InstallationControl, TransactionDataReference, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::journal::{JournalBinding, JournalStore, ManifestRole},
};
use serde::{Deserialize, Serialize};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RETURN_CHECKPOINT_BLOCKED")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReturnCheckpointMaterials {
    pub(crate) schema: u32,
    pub(crate) binding: JournalBinding,
    pub(crate) data: TransactionDataReference,
    pub(crate) boundary: (String, ManagerRecordReference),
    pub(crate) bundle_plan: ManagerRecordReference,
    pub(crate) installer_terminal: (String, String),
    pub(crate) historical_terminal: (String, String),
}

/// 借用实际owner，不可Deserialize；材料记录不能反向构造此类型。
pub(crate) struct LiveReturnCheckpoint<'a> {
    pub(super) binding: &'a JournalBinding,
    pub(super) installation: &'a InstallationControl,
    pub(super) data: &'a TransactionDataRoot,
    pub(super) exclusive: &'a ExclusiveLease,
    pub(super) source: &'a SourceHandoffTerminal,
    pub(super) scope: &'a FencedInstallation,
    pub(super) installer: &'a TerminalProcessJob,
    pub(super) historical: &'a TerminalProcessJob,
    pub(super) boundary: &'a ReturnBoundary,
    pub(super) attempt: &'a ReturnBoundaryAttempt,
    pub(super) context: &'a ContextRestoration,
    pub(super) bundle: &'a BundleRestoration,
    pub(super) registration: &'a RetainedRegistrationState,
    pub(super) shortcuts: &'a RetainedProductShortcuts,
}
impl LiveReturnCheckpoint<'_> {
    pub(crate) fn binding(&self) -> &JournalBinding {
        self.binding
    }
    pub(crate) fn verify(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<Vec<u8>, SafeError> {
        let user = CurrentUser::capture().map_err(blocked)?;
        self.data.verify_installation(self.installation)?;
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        store.verify_windows_binding(self.installation.root(), self.binding, generation)?;
        self.source.verify(self.binding)?;
        self.scope.verify().map_err(blocked)?;
        self.installer.verify().map_err(blocked)?;
        self.historical.verify().map_err(blocked)?;
        self.boundary.verify_current_image()?;
        if self.scope.source_process_identity() != self.source.exit().host_identity()
            || self.boundary.binding() != self.binding
            || self.installer.job_kind() != JobKind::Installer
            || self.historical.job_kind() != JobKind::HistoricalApplication
            || self.installer.process_identity() == self.historical.process_identity()
            || self.installer.root_identity() != self.data.root().directory().identity()
            || self.historical.root_identity() != self.data.root().directory().identity()
        {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let (original, later) = self
            .context
            .verify_return_checkpoint(&user)
            .map_err(blocked)?;
        let inspection = store.inspect(self.binding)?;
        let state = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        if inspection.blocked || state.generation() != generation {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        for (role, actual) in [
            (ManifestRole::SourceContext, original),
            (ManifestRole::RetainedTargetContext, later),
        ] {
            let digest = state
                .manifest(role)
                .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
            if store.read_manifest(digest)? != actual {
                return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
            }
        }
        {
            let mut journal = ContextJournal::new(
                store,
                self.installation.root().clone(),
                self.exclusive,
                self.binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            self.bundle
                .verify_return_checkpoint(&user, &mut journal)
                .map_err(blocked)?;
        }
        {
            let mut journal = RegistrationJournal::new(
                store,
                self.installation.root().clone(),
                self.exclusive,
                self.binding.clone(),
                generation,
            )
            .map_err(blocked)?;
            self.registration
                .verify_retained(&mut journal)
                .map_err(blocked)?;
        }
        self.shortcuts.verify_retained().map_err(blocked)?;
        let (name, record) = self
            .attempt
            .record_reference()
            .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        serde_json::to_vec(&ReturnCheckpointMaterials {
            schema: 1,
            binding: self.binding.clone(),
            data: self.data.reference().clone(),
            boundary: (name.into(), record.clone()),
            bundle_plan: self.bundle.plan_reference().clone(),
            installer_terminal: self.installer.terminal_reference(),
            historical_terminal: self.historical.terminal_reference(),
        })
        .map_err(blocked)
    }
}
