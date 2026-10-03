//! Safe manager presentation DTOs. Offered actions are not effect authority:
//! commands must re-admit the original document/generation and live OS proofs.
use super::{
    catalog::{RetainedSelectionDiagnostic, SelectionMetadata},
    journal::{JournalPhase, SwitchJournal},
};
use crate::cli::{
    profiles::error,
    types::{SafeError, WireU64},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectManagerRequest {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManagerActionRequest {
    pub(crate) expected_generation: WireU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManagerPhase {
    Preparing,
    Installing,
    InstalledUnconfirmed,
    HistoricalActive,
    Returning,
    Restored,
    PreContextAborted,
    RecoveryRequired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManagerAction {
    Refresh,
    ConfirmHistoricalVersion,
    ReturnToPrevious,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ManagerBlockReason {
    SourceStillRunning,
    SourceExitUnconfirmed,
    SessionsNotQuiescent,
    InstallerOutcomeUnknown,
    PayloadUnverified,
    PayloadChanged,
    ReturnConflict,
    RecoveryEvidenceUnavailable,
    StorageUnavailable,
    DocumentChanged,
    ManagerHandoffInterrupted,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagerStatus {
    pub(crate) transaction_id: String,
    pub(crate) generation: WireU64,
    pub(crate) source_version: String,
    pub(crate) target_version: String,
    pub(crate) phase: ManagerPhase,
    pub(crate) blocked_reason: Option<ManagerBlockReason>,
    pub(crate) allowed_actions: Vec<ManagerAction>,
}
impl ManagerStatus {
    /// Projection of the implemented durable state machine. The coordinator
    /// offers an action only after its fresh checks; projection cannot add one.
    pub(crate) fn project(
        journal: &SwitchJournal,
        selected: &SelectionMetadata,
        blocked_reason: Option<ManagerBlockReason>,
        offered: &[ManagerAction],
    ) -> Result<Self, SafeError> {
        if journal.binding().target_package != selected.installer().sha256() {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        let phase = match journal.phase() {
            JournalPhase::Reviewed | JournalPhase::SourceSealed | JournalPhase::FreshReady => {
                ManagerPhase::Preparing
            }
            JournalPhase::Installing => ManagerPhase::Installing,
            JournalPhase::InstalledUnconfirmed => ManagerPhase::InstalledUnconfirmed,
            JournalPhase::HistoricalActive => ManagerPhase::HistoricalActive,
            JournalPhase::Restoring => ManagerPhase::Returning,
            JournalPhase::Restored => ManagerPhase::Restored,
            JournalPhase::PreContextAborted => ManagerPhase::PreContextAborted,
            JournalPhase::RecoveryRequired => ManagerPhase::RecoveryRequired,
        };
        Self::validate_projection(phase, offered)?;
        if journal.requires_reconciliation()
            && offered.contains(&ManagerAction::ConfirmHistoricalVersion)
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        Ok(Self {
            transaction_id: journal.binding().transaction_id.clone(),
            generation: WireU64::parse(&journal.generation().to_string())?,
            source_version: env!("CARGO_PKG_VERSION").into(),
            target_version: selected.version().into(),
            phase,
            blocked_reason,
            allowed_actions: offered.to_vec(),
        })
    }
    /// A reopened protected transcript is diagnostic, never a replacement for
    /// the original coordinator's live installer/job and source-terminal owner.
    /// This projection cannot advertise confirm, restore or implicit success.
    pub(crate) fn project_reentry(
        journal: &SwitchJournal,
        diagnostic: &RetainedSelectionDiagnostic,
    ) -> Result<Self, SafeError> {
        if journal.binding().target_package != diagnostic.installer_digest() {
            return Err(error("HISTORY_TARGET_CHANGED"));
        }
        let phase = ManagerPhase::RecoveryRequired;
        let allowed_actions = vec![ManagerAction::Refresh];
        Self::validate_projection(phase, &allowed_actions)?;
        Ok(Self {
            transaction_id: journal.binding().transaction_id.clone(),
            generation: WireU64::parse(&journal.generation().to_string())?,
            source_version: env!("CARGO_PKG_VERSION").into(),
            target_version: diagnostic.version().into(),
            phase,
            blocked_reason: Some(ManagerBlockReason::RecoveryEvidenceUnavailable),
            allowed_actions,
        })
    }
    /// 原生重开路径完成对象重验后才能调用。这里只检查投影与日志阶段吻合，
    /// 不会从日志或按钮本身构造恢复权限；完成状态须另行核对真实恢复结果。
    pub(crate) fn project_recovered_return(
        journal: &SwitchJournal,
        diagnostic: &RetainedSelectionDiagnostic,
        phase: ManagerPhase,
        actions: &[ManagerAction],
    ) -> Result<Self, SafeError> {
        let allowed_phase = matches!(
            (journal.phase(), phase),
            (
                JournalPhase::Restoring,
                ManagerPhase::RecoveryRequired | ManagerPhase::Returning
            ) | (
                JournalPhase::RecoveryRequired,
                ManagerPhase::RecoveryRequired
            ) | (JournalPhase::Restored, ManagerPhase::Restored)
        );
        if !allowed_phase
            || actions.contains(&ManagerAction::ConfirmHistoricalVersion)
            || (actions.contains(&ManagerAction::ReturnToPrevious)
                && (journal.phase() != JournalPhase::Restoring
                    || phase != ManagerPhase::RecoveryRequired
                    || journal.requires_reconciliation()))
            || (phase == ManagerPhase::Restored && journal.requires_reconciliation())
        {
            return Err(error("HISTORY_INVALID_STATUS"));
        }
        Self::validate_projection(phase, actions)?;
        let mut status = Self::project_reentry(journal, diagnostic)?;
        status.phase = phase;
        status.allowed_actions = actions.to_vec();
        status.blocked_reason = if phase == ManagerPhase::RecoveryRequired
            && !actions.contains(&ManagerAction::ReturnToPrevious)
        {
            Some(ManagerBlockReason::RecoveryEvidenceUnavailable)
        } else {
            None
        };
        Ok(status)
    }
    fn validate_projection(
        phase: ManagerPhase,
        actions: &[ManagerAction],
    ) -> Result<(), SafeError> {
        if actions.is_empty()
            || actions.len() > 3
            || actions[0] != ManagerAction::Refresh
            || actions
                .iter()
                .enumerate()
                .any(|(index, action)| actions[..index].contains(action))
            || (actions.contains(&ManagerAction::ConfirmHistoricalVersion)
                && phase != ManagerPhase::InstalledUnconfirmed)
            || (actions.contains(&ManagerAction::ReturnToPrevious)
                && !matches!(
                    phase,
                    ManagerPhase::InstalledUnconfirmed
                        | ManagerPhase::HistoricalActive
                        | ManagerPhase::RecoveryRequired
                ))
        {
            return Err(error("HISTORY_INVALID_STATUS"));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn fixture(
        phase: ManagerPhase,
        blocked_reason: Option<ManagerBlockReason>,
        allowed_actions: Vec<ManagerAction>,
    ) -> Self {
        Self::validate_projection(phase, &allowed_actions).unwrap();
        Self {
            transaction_id: "11111111-1111-4111-8111-111111111111".into(),
            generation: WireU64::parse("17").unwrap(),
            source_version: "0.18.0".into(),
            target_version: "0.17.7".into(),
            phase,
            blocked_reason,
            allowed_actions,
        }
    }
}
