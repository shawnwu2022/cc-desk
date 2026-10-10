//! Original-document switch requests. UUID allocation and transfer ownership
//! remain in PrepareService; paths, URLs and PIDs never enter this wire format.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SwitchDataMode {
    FreshSettings,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BeginSwitchRequest {
    pub(crate) preparation_id: String,
    pub(crate) data_mode: SwitchDataMode,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SwitchTicket {
    pub(crate) transaction_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectSwitchRequest {
    pub(crate) preparation_id: String,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SwitchReviewPhase {
    Preparing,
    Verified,
    HandoffIssued,
    Aborted,
    Cancelled,
    Unavailable,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SwitchReviewAction {
    Refresh,
    Review,
    BeginSwitch,
    CancelPreparation,
    PrepareAgain,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum SwitchReviewBlock {
    PreparationPending,
    PreparationBusy,
    PreparationExpired,
    PreparationFailed,
    PayloadUnverified,
    CoordinatorUnavailable,
    HandoffIssued,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SwitchContextPolicy {
    FreshSettingsPreserveCurrentSharedCli,
}
/// A read-only projection, never an execution permit. Even an offered action
/// must freshly authenticate the original document and all native guards.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SwitchReview {
    pub(crate) preparation_id: String,
    pub(crate) version: String,
    pub(crate) phase: SwitchReviewPhase,
    pub(crate) context_policy: SwitchContextPolicy,
    pub(crate) transaction_id: Option<String>,
    pub(crate) allowed_actions: Vec<SwitchReviewAction>,
    pub(crate) block_reason: Option<SwitchReviewBlock>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OrdinaryInstallAction {
    Refresh,
    Install,
    CancelPreparation,
    PrepareAgain,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OrdinaryInstallOutcome {
    NotStarted,
    HandoffUnknown,
    InstallerStarted,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrdinaryInstallReview {
    pub(crate) preparation_id: String,
    pub(crate) version: String,
    pub(crate) phase: SwitchReviewPhase,
    pub(crate) context_policy: &'static str,
    pub(crate) transaction_id: Option<String>,
    pub(crate) allowed_actions: Vec<OrdinaryInstallAction>,
    pub(crate) block_reason: Option<SwitchReviewBlock>,
    pub(crate) backup_location: Option<String>,
    pub(crate) installation_outcome: OrdinaryInstallOutcome,
}
