//! Live one-way historical launch custody. An empty job, PID, journal record or
//! failed installer alone cannot prove that historical creation never began.
use super::{
    lease::{ExclusiveLease, ExclusiveLeaseWitness},
    scope::FencedInstallation,
    security::CurrentUser,
    source_lifecycle::SourceHandoffTerminal,
    startup::{InstallationControl, TransactionDataRoot},
};
use crate::{
    cli::{profiles::error, types::SafeError},
    version_history::{
        journal::{JournalBinding, JournalPhase, JournalStore, ManifestRole, RetainedRoleGuard},
        policy::PRODUCT_IDENTIFIER,
        verified_package::sha256,
    },
};
use std::{
    os::windows::ffi::OsStrExt,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

fn blocked(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_NO_HISTORICAL_LAUNCH_CHANGED")
}

const AVAILABLE: u8 = 0;
const HISTORICAL_CREATION_BEGAN: u8 = 1;
const RETURN_CLAIMED: u8 = 2;

/// Shared by every witness. Neither branch can release this custody back to
/// AVAILABLE, including a failed journal append before CreateProcessW.
struct LaunchState(AtomicU8);
impl LaunchState {
    fn new() -> Self {
        Self(AtomicU8::new(AVAILABLE))
    }
    fn begin_historical_creation(&self) -> Result<(), SafeError> {
        self.0
            .compare_exchange(
                AVAILABLE,
                HISTORICAL_CREATION_BEGAN,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map(|_| ())
            .map_err(blocked)
    }
    fn claim_return(&self) -> Result<(), SafeError> {
        self.0
            .compare_exchange(
                AVAILABLE,
                RETURN_CLAIMED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map(|_| ())
            .map_err(blocked)
    }
    fn verify(&self, expected: u8) -> Result<(), SafeError> {
        if self.0.load(Ordering::SeqCst) != expected {
            return Err(blocked("historical launch custody changed"));
        }
        Ok(())
    }
}

struct LaunchCustody {
    binding: JournalBinding,
    installation: Arc<InstallationControl>,
    data: Arc<TransactionDataRoot>,
    exclusive: ExclusiveLeaseWitness,
    terminal: Arc<SourceHandoffTerminal>,
    scope: Arc<FencedInstallation>,
    source_exit: RetainedRoleGuard,
    admitted_generation: u64,
    state: LaunchState,
}
impl LaunchCustody {
    fn verify_native(&self) -> Result<(), SafeError> {
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(blocked)?;
        self.data.verify_installation(&self.installation)?;
        self.terminal.verify(&self.binding)?;
        self.scope.verify().map_err(blocked)?;
        if self.data.transaction_id() != self.binding.transaction_id
            || self.scope.source_process_identity() != self.terminal.exit().host_identity()
        {
            return Err(blocked("foreign original source custody"));
        }
        self.source_exit.verify_role(
            &self.binding,
            ManifestRole::SourceHandoffExit,
            self.installation.root(),
        )?;
        Ok(())
    }
}

/// The live coordinator owns this once, from FreshReady until one irreversible
/// branch wins. It must revoke immediately BEFORE the durable historical create
/// intent, even when no process is eventually created. Never recreate on error.
pub(crate) struct HistoricalLaunchPermit(Arc<LaunchCustody>);

/// Read-only candidate evidence; clones all observe the same launch revocation.
/// No serialized/boolean/PID constructor is provided.
#[derive(Clone)]
pub(crate) struct NoHistoricalLaunch(Arc<LaunchCustody>);

/// Only the Return branch can own this state, after a successful one-way claim.
pub(super) struct ClaimedNoHistoricalLaunch(Arc<LaunchCustody>);

impl HistoricalLaunchPermit {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn acquire(
        binding: JournalBinding,
        installation: Arc<InstallationControl>,
        data: Arc<TransactionDataRoot>,
        exclusive: &ExclusiveLease,
        terminal: Arc<SourceHandoffTerminal>,
        scope: Arc<FencedInstallation>,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<Self, SafeError> {
        exclusive
            .verify_root(installation.root())
            .map_err(blocked)?;
        data.verify_installation(&installation)?;
        terminal.verify(&binding)?;
        scope.verify().map_err(blocked)?;
        let user = CurrentUser::capture().map_err(blocked)?;
        user.require_unelevated().map_err(blocked)?;
        let expected_installation = sha256(
            &serde_json::to_vec(&(
                PRODUCT_IDENTIFIER,
                user.sid_text(),
                scope
                    .original_path()
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>(),
            ))
            .map_err(blocked)?,
        );
        if expected_installation != binding.user_installation
            || data.transaction_id() != binding.transaction_id
            || scope.source_process_identity() != terminal.exit().host_identity()
        {
            return Err(blocked("foreign original source custody"));
        }
        let inspection = store.inspect(&binding)?;
        let state = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| blocked("missing journal"))?;
        // FreshReady precedes any installer attempt. A later failed creation or
        // payload check cannot mint a replacement for a revoked live permit.
        if inspection.blocked
            || state.requires_reconciliation()
            || state.phase() != JournalPhase::FreshReady
            || state.generation() != generation
        {
            return Err(blocked("launch permit requires exact healthy FreshReady"));
        }
        store.verify_windows_binding(installation.root(), &binding, generation)?;
        store.verify_no_historical_launch(generation)?;
        let source_exit = store.retain_role_guard(
            installation.root().clone(),
            &binding,
            generation,
            ManifestRole::SourceHandoffExit,
        )?;
        let result = Self(Arc::new(LaunchCustody {
            binding,
            installation,
            data,
            exclusive: exclusive.witness().map_err(blocked)?,
            terminal,
            scope,
            source_exit,
            admitted_generation: generation,
            state: LaunchState::new(),
        }));
        result.0.verify_native()?;
        store.verify_no_historical_launch(generation)?;
        Ok(result)
    }
    pub(crate) fn witness(&self) -> Result<NoHistoricalLaunch, SafeError> {
        self.0.verify_native()?;
        self.0.state.verify(AVAILABLE)?;
        Ok(NoHistoricalLaunch(self.0.clone()))
    }
    pub(crate) fn begin_historical_creation(&self) -> Result<(), SafeError> {
        self.0.verify_native()?;
        self.0.state.begin_historical_creation()
    }
}

impl NoHistoricalLaunch {
    pub(super) fn claim_return(&self) -> Result<ClaimedNoHistoricalLaunch, SafeError> {
        self.0.verify_native()?;
        self.0.state.claim_return()?;
        Ok(ClaimedNoHistoricalLaunch(self.0.clone()))
    }
}
impl ClaimedNoHistoricalLaunch {
    pub(super) fn verify_live(&self) -> Result<(), SafeError> {
        self.0.state.verify(RETURN_CLAIMED)?;
        self.0.verify_native()?;
        self.0.state.verify(RETURN_CLAIMED)
    }
    pub(super) fn verify_custody(
        &self,
        binding: &JournalBinding,
        installation: &Arc<InstallationControl>,
        data: &Arc<TransactionDataRoot>,
        scope: &Arc<FencedInstallation>,
    ) -> Result<(), SafeError> {
        self.verify_live()?;
        if binding != &self.0.binding
            || !Arc::ptr_eq(installation, &self.0.installation)
            || !Arc::ptr_eq(data, &self.0.data)
            || !Arc::ptr_eq(scope, &self.0.scope)
        {
            return Err(blocked("Return differs from live original source custody"));
        }
        Ok(())
    }
    pub(super) fn verify_journal(
        &self,
        store: &mut JournalStore,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.verify_live()?;
        store.verify_windows_binding(self.0.installation.root(), &self.0.binding, generation)?;
        store.verify_no_historical_launch(generation)?;
        self.verify_live()
    }
    /// Audit description only. Restoring these bytes cannot restore capability.
    pub(super) fn observation(&self) -> Result<serde_json::Value, SafeError> {
        self.verify_live()?;
        Ok(serde_json::json!({
            "kind": "historicalCreationNeverBegan",
            "admittedGeneration": self.0.admitted_generation,
            "sourceExit": sha256(&self.0.source_exit.read()?),
        }))
    }
}

#[cfg(test)]
#[path = "../../tests/version_history_no_historical_launch_windows.rs"]
#[allow(non_snake_case)]
mod tests;
