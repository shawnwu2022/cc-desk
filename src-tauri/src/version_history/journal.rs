//! Append-only, individually flushed records and immutable manifests. A record
//! proves an observed transcript, never authority to replay an OS operation.
//! No multi-file/registry atomicity or Windows directory durability is implied.
#[cfg(test)]
use super::maintenance::DirectoryDurability;
use super::verified_package::sha256;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
#[cfg(test)]
use cap_std::fs::{Dir, OpenOptions};
#[cfg(any(test, windows))]
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(any(test, windows))]
use std::fs::File;
#[cfg(test)]
use std::io::Write;
#[cfg(any(test, windows))]
use std::io::{Read, Seek, SeekFrom};
#[cfg(test)]
use std::sync::atomic::AtomicU64;
#[cfg(any(test, windows))]
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_RECORD_BYTES: usize = 128 * 1024;
const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_DEPENDENCY_HANDLES: usize = 4096;

/// Logical capacity admission, NOT free-disk-space or filesystem preallocation.
/// The reviewed operation plan must count EVERY forward and return copy/ACL/
/// registry/shortcut effect. Three records per effect cover intent, unknown and
/// later reconciliation. Additional attempts require their own planned capacity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapacityPlan {
    forward_records: u64,
    recovery_records: u64,
    record_byte_ceiling: u64,
}
impl CapacityPlan {
    pub(crate) fn for_effects(
        forward_effects: u64,
        recovery_effects: u64,
        control_records_per_lane: u64,
        record_byte_ceiling: u64,
    ) -> Result<Self, SafeError> {
        let records = |effects: u64| {
            effects
                .checked_mul(3)
                .and_then(|count| count.checked_add(control_records_per_lane))
                .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))
        };
        if forward_effects == 0 || recovery_effects == 0 || control_records_per_lane == 0 {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        let plan = Self {
            forward_records: records(forward_effects)?,
            recovery_records: records(recovery_effects)?,
            record_byte_ceiling,
        };
        plan.validate(Limits::default(), 0)?;
        Ok(plan)
    }
    fn validate(&self, limits: Limits, genesis_bytes: u64) -> Result<(), SafeError> {
        let records = self
            .forward_records
            .checked_add(self.recovery_records)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let bytes = (records - 1)
            .checked_mul(self.record_byte_ceiling)
            .and_then(|bytes| bytes.checked_add(genesis_bytes))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        if self.forward_records == 0
            || self.recovery_records == 0
            || self.record_byte_ceiling < 2048
            || self.record_byte_ceiling > MAX_RECORD_BYTES as u64
            || records > limits.records
            || bytes > limits.bytes
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Limits {
    records: u64,
    bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            records: MAX_RECORDS as u64,
            bytes: MAX_JOURNAL_BYTES as u64,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WriteLane {
    Forward,
    Recovery,
}
#[derive(Default)]
struct Usage {
    records: u64,
    bytes: u64,
    forward_records: u64,
    recovery_records: u64,
}
impl Usage {
    fn check(
        &self,
        plan: &CapacityPlan,
        lane: WriteLane,
        bytes: u64,
        limits: Limits,
    ) -> Result<(), SafeError> {
        let (used, reserved) = match lane {
            WriteLane::Forward => (self.forward_records, plan.forward_records),
            WriteLane::Recovery => (self.recovery_records, plan.recovery_records),
        };
        if bytes > plan.record_byte_ceiling
            || used >= reserved
            || self.records >= limits.records
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
    fn commit(&mut self, lane: WriteLane, bytes: u64) {
        self.records += 1;
        self.bytes += bytes;
        match lane {
            WriteLane::Forward => self.forward_records += 1,
            WriteLane::Recovery => self.recovery_records += 1,
        }
    }
}

pub(crate) fn validate_id(value: &str) -> Result<(), SafeError> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| error("HISTORY_IDENTITY_INVALID"))?;
    if parsed.is_nil() || parsed.hyphenated().to_string() != value {
        return Err(error("HISTORY_IDENTITY_INVALID"));
    }
    Ok(())
}
#[cfg(windows)]
type ContextInverse = (Option<String>, Option<(EffectSpec, u64)>);
#[cfg(windows)]
type ContextRootOutcome = Option<(EffectSpec, u64, bool)>;

pub(super) fn validate_digest(value: &str) -> Result<(), SafeError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(error("HISTORY_IDENTITY_INVALID"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RootKind {
    Desk,
    WebView,
}

/// Persisted backend observations, never an IPC request or proof constructor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JournalBinding {
    pub(crate) transaction_id: String,
    pub(crate) source_context: String,
    pub(crate) target_context: String,
    pub(crate) user_installation: String,
    pub(crate) source_bundle: String,
    pub(crate) target_package: String,
    pub(crate) target_payload: String,
    pub(crate) roots: String,
}
impl JournalBinding {
    pub(crate) fn validate(&self) -> Result<(), SafeError> {
        for id in [
            &self.transaction_id,
            &self.source_context,
            &self.target_context,
        ] {
            validate_id(id)?;
        }
        if self.source_context == self.target_context {
            return Err(error("HISTORY_IDENTITY_INVALID"));
        }
        for digest in [
            &self.user_installation,
            &self.source_bundle,
            &self.target_package,
            &self.target_payload,
            &self.roots,
        ] {
            validate_digest(digest)?;
        }
        Ok(())
    }
    pub(crate) fn has_context(&self, id: &str) -> bool {
        id == self.source_context || id == self.target_context
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RegistrationSlot {
    Uninstall,
    Publisher,
    DeskDirectory,
    DeskDirectoryBackground,
    LegacyDirectory,
    LegacyDirectoryBackground,
    /// Only the product-owned CC Desk value, never the shared Run tree.
    OwnedRun,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ShortcutSlot {
    Desktop,
    StartMenu,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum FilesystemOperation {
    CreateDirectory,
    CopyFile,
    Rename,
    SetPermissions,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum PrivateBackupOperation {
    CreateDirectory,
    CopyFile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RegistrationOperation {
    CreateKey,
    SetValue,
    RemoveOwnedValue,
    RemoveOwnedKey,
    SetPermissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ShortcutOperation {
    CreateFile,
    WriteBytes,
    SetAttributes,
    SetPermissions,
    RemoveFile,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum EffectKind {
    FenceSourceImage,
    PrivateBackupEntry {
        plan_generation: u64,
        operation: PrivateBackupOperation,
        manifest: String,
        entry_index: u32,
    },
    RotateSourceRoot {
        root: RootKind,
        manifest: String,
    },
    ReverseSourceRoot {
        original_effect_id: String,
        original_intent_generation: u64,
        current_manifest: String,
    },
    ReverseSourceFence {
        original_effect_id: String,
        original_intent_generation: u64,
    },
    FenceHistoricalImage,
    PreserveRoot {
        context: String,
        root: RootKind,
    },
    CreateFreshRoot {
        root: RootKind,
    },
    RestoreSourceRoot {
        root: RootKind,
    },
    /// Each individual copy/rename/permission write references a backend-held
    /// immutable manifest entry. No deserialized path becomes OS authority.
    FilesystemEntry {
        operation: FilesystemOperation,
        manifest: String,
        entry_index: u32,
    },
    RegistrationEntry {
        slot: RegistrationSlot,
        operation: RegistrationOperation,
        manifest: String,
        entry_index: u32,
    },
    RecoveryFilesystemEntry {
        context: String,
        operation: FilesystemOperation,
        manifest: String,
        entry_index: u32,
    },
    RecoveryRegistrationEntry {
        slot: RegistrationSlot,
        operation: RegistrationOperation,
        manifest: String,
        entry_index: u32,
    },
    RecoveryShortcutEntry {
        slot: ShortcutSlot,
        operation: ShortcutOperation,
        manifest: String,
        entry_index: u32,
    },
    /// These aggregate observations never replace individual effect records.
    VerifySourceBundleCopy,
    VerifySourceBundleRestore,
    InstallerCreateSuspended,
    InstallerResume,
    InstallerTerminalOutcome,
    HistoricalCreateSuspended,
    HistoricalResume,
    HistoricalTerminalOutcome,
    VerifyTargetBundle,
    ConfirmFirstLaunch,
    VerifyRegistrationRestore {
        slot: RegistrationSlot,
    },
    RestoreShortcut {
        slot: ShortcutSlot,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EffectSpec {
    pub(crate) effect_id: String,
    pub(crate) kind: EffectKind,
    /// Actual pre-operation state and expected postconditions are distinct.
    /// Expected postconditions cannot invent a future PID/file object identity.
    pub(crate) before: String,
    pub(crate) expected_postconditions: String,
}
impl EffectSpec {
    fn validate(&self, binding: &JournalBinding) -> Result<(), SafeError> {
        validate_id(&self.effect_id)?;
        validate_digest(&self.before)?;
        validate_digest(&self.expected_postconditions)?;
        if let EffectKind::PreserveRoot { context, .. }
        | EffectKind::RecoveryFilesystemEntry { context, .. } = &self.kind
        {
            if !binding.has_context(context) {
                return Err(error("HISTORY_CONTEXT_CHANGED"));
            }
        }
        if let EffectKind::ReverseSourceFence {
            original_effect_id,
            original_intent_generation,
        }
        | EffectKind::ReverseSourceRoot {
            original_effect_id,
            original_intent_generation,
            ..
        } = &self.kind
        {
            validate_id(original_effect_id)?;
            if *original_intent_generation == 0 {
                return Err(error("HISTORY_EFFECT_CHANGED"));
            }
        }
        if let Some(manifest) = self.entry_manifest() {
            validate_digest(manifest)?;
        }
        Ok(())
    }
    fn entry_manifest(&self) -> Option<&str> {
        match &self.kind {
            EffectKind::PrivateBackupEntry { manifest, .. }
            | EffectKind::RotateSourceRoot { manifest, .. }
            | EffectKind::ReverseSourceRoot {
                current_manifest: manifest,
                ..
            }
            | EffectKind::FilesystemEntry { manifest, .. }
            | EffectKind::RegistrationEntry { manifest, .. }
            | EffectKind::RecoveryFilesystemEntry { manifest, .. }
            | EffectKind::RecoveryRegistrationEntry { manifest, .. }
            | EffectKind::RecoveryShortcutEntry { manifest, .. } => Some(manifest),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Observation {
    Applied,
    NotApplied,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedResult {
    pub(crate) observation: Observation,
    pub(crate) receipt: Option<String>,
}
/// Immutable receipt generated AFTER an observation. The observed manifest may
/// contain identities that could not exist at intent time. This binds evidence;
/// the later typed OS executor must still validate what that evidence proves.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectReceipt {
    schema: u32,
    transaction_id: String,
    effect_id: String,
    intent_generation: u64,
    expected_postconditions: String,
    observation: Observation,
    observed_manifest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ManifestRole {
    /// Protected initial-manager selector. Opening its data root still requires
    /// same-user ACL/object checks and exact child/material re-admission.
    ManagerHandoff,
    /// Original owner observed zero admitted children/writers and matching
    /// BrowserProcessExited receipts before requesting source-host exit.
    SourceHandoffExit,
    SourceContext,
    FreshTargetContext,
    RetainedTargetContext,
    SourceBundle,
    Registration,
    Shortcuts,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum JournalPhase {
    Reviewed,
    SourceSealed,
    FreshReady,
    Installing,
    InstalledUnconfirmed,
    HistoricalActive,
    Restoring,
    Restored,
    PreContextAborted,
    RecoveryRequired,
}
/// A durable C1 attempt selector. It records observation and capacity only;
/// its live admission factory remains in the held Windows context executor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootBackupPlan {
    pub(crate) original_effect_id: String,
    pub(crate) original_intent_generation: u64,
    pub(crate) current_manifest: String,
    pub(crate) destination: String,
    pub(crate) prior_plan_generation: Option<u64>,
    pub(crate) preserved_manifest: Option<String>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) reservation_source: String,
    pub(crate) effects: u32,
    pub(crate) recovery_dependencies: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaterBackupPlan {
    pub(crate) root: RootKind,
    pub(crate) source_manifest: String,
    pub(crate) destination: String,
    pub(crate) source_reservation: String,
    pub(crate) previous_generation: Option<u64>,
    pub(crate) previous_observation: Option<String>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) effects: u32,
    pub(crate) recovery_dependencies: u32,
}
#[cfg(windows)]
pub(crate) struct LaterBackupRecord {
    pub(crate) generation: u64,
    pub(crate) plan: LaterBackupPlan,
    pub(crate) complete: Option<String>,
    pub(crate) applied: Vec<(u32, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundleBackupPlan {
    pub(crate) current_manifest: String,
    pub(crate) destination: String,
    pub(crate) previous_observation: String,
    pub(crate) previous_generation: Option<u64>,
    pub(crate) abandoned_effect: Option<(String, u64)>,
    pub(crate) effects: u32,
    pub(crate) recovery_dependencies: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum JournalEvent {
    AdmitBundleStart {
        seed: String,
        current_manifest: String,
        effects: u32,
        recovery_dependencies: u32,
        receipt: String,
    },
    PrepareBundleBackup {
        plan: BundleBackupPlan,
        receipt: String,
    },
    Begin {
        capacity: CapacityPlan,
    },
    PrivateBackupPlan {
        manifest: String,
        effects: u32,
        recovery_dependencies: u32,
    },
    PrepareLaterBackup {
        plan: LaterBackupPlan,
        receipt: String,
    },
    CompleteLaterBackup {
        root: RootKind,
        plan_generation: u64,
        copy_manifest: String,
        receipt: String,
    },
    AdmitPreinstallReturn {
        roots: BTreeMap<RootKind, String>,
        pending: Option<(String, u64)>,
        receipt: String,
    },
    ConfirmContextRoot {
        effect_id: String,
        intent_generation: u64,
        current_manifest: String,
        completed: bool,
        receipt: String,
    },
    PrepareRootBackup {
        plan: RootBackupPlan,
        receipt: String,
    },
    ConfirmRootReturned {
        effect_id: String,
        intent_generation: u64,
        current_manifest: String,
        receipt: String,
    },
    AdmitRootReverse {
        effect_id: String,
        intent_generation: u64,
        current_manifest: String,
        receipt: String,
    },
    Manifest {
        role: ManifestRole,
        digest: String,
    },
    Intent {
        effect: EffectSpec,
    },
    Observed {
        effect_id: String,
        intent_generation: u64,
        result: ObservedResult,
    },
    Phase {
        phase: JournalPhase,
    },
    AbortPreContext {
        receipt: String,
    },
    /// Missing acknowledgement is explicitly recorded as Unknown here when no
    /// Observed frame survived. Existing Unknown is never overwritten.
    CompensateUnknown {
        effect_id: String,
        intent_generation: u64,
        current_roots: String,
        receipt: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryObservations {
    processes: String,
    jobs: String,
    roots: String,
    registration: String,
}
impl RecoveryObservations {
    fn digests(&self) -> [&str; 4] {
        [&self.processes, &self.jobs, &self.roots, &self.registration]
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnchangedSourceObservations {
    bundle: String,
    roots: String,
    registration: String,
    quiescence: String,
}
impl UnchangedSourceObservations {
    fn digests(&self) -> [&str; 4] {
        [
            &self.bundle,
            &self.roots,
            &self.registration,
            &self.quiescence,
        ]
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionAnchor {
    binding: JournalBinding,
    generation: u64,
    head: String,
    journal_identity: String,
}
impl AdmissionAnchor {
    fn matches(&self, journal: &SwitchJournal, head: &str, identity: &str) -> bool {
        self.binding == journal.binding
            && self.generation == journal.generation
            && self.head == head
            && self.journal_identity == identity
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbortReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    unchanged: UnchangedSourceObservations,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompensationReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    effect_id: String,
    intent_generation: u64,
    current: RecoveryObservations,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootReverseReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    effect_id: String,
    intent_generation: u64,
    current_manifest: String,
    returned: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootBackupReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    plan: RootBackupPlan,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextReturnReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    roots: BTreeMap<RootKind, String>,
    pending: Option<(String, u64)>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRootReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    effect_id: String,
    intent_generation: u64,
    current_manifest: String,
    completed: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaterBackupReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    plan: LaterBackupPlan,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaterCompleteReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    root: RootKind,
    plan_generation: u64,
    copy_manifest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleStartReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    seed: String,
    current_manifest: String,
    effects: u32,
    recovery_dependencies: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleBackupReceipt {
    schema: u32,
    anchor: AdmissionAnchor,
    plan: BundleBackupPlan,
}
/// The private-only production factory retains exact live unchanged-source
/// ownership. A stronger reversed-context factory must additionally establish
/// real quiescence; hashes alone cannot construct either proof.
pub(crate) struct PreContextAbortProof<'a> {
    anchor: AdmissionAnchor,
    unchanged: UnchangedSourceObservations,
    guards: Box<dyn LiveAbortGuards + 'a>,
}
trait LiveAbortGuards {
    fn verify(&self) -> Result<(), SafeError>;
}
#[cfg(test)]
struct FixtureAbortGuards;
#[cfg(test)]
impl LiveAbortGuards for FixtureAbortGuards {
    fn verify(&self) -> Result<(), SafeError> {
        Ok(())
    }
}
#[cfg(windows)]
impl LiveAbortGuards for super::windows::pre_context_abort::PrivateAbortEvidence<'_> {
    fn verify(&self) -> Result<(), SafeError> {
        super::windows::pre_context_abort::PrivateAbortEvidence::verify(self)
    }
}
/// No production constructor or Deserialize. Missing process/job identity must
/// prevent a future factory from minting this admission, never become quiescence.
pub(crate) struct UnknownCompensationProof {
    anchor: AdmissionAnchor,
    effect_id: String,
    intent_generation: u64,
    current: RecoveryObservations,
    _guards: Box<dyn Send + Sync>,
}
struct EffectRecord {
    spec: EffectSpec,
    intent_generation: u64,
    result: Option<ObservedResult>,
}

pub(crate) struct SwitchJournal {
    binding: JournalBinding,
    generation: u64,
    phase: JournalPhase,
    effects: BTreeMap<String, EffectRecord>,
    applied_kinds: BTreeSet<EffectKind>,
    pending: Option<String>,
    manifests: BTreeMap<ManifestRole, String>,
    capacity: CapacityPlan,
    recovering: bool,
    compensated: BTreeSet<String>,
    return_roots: Option<String>,
    context_return_only: bool,
    admitted_root_reversals: BTreeMap<String, String>,
    returned_reversals: BTreeSet<String>,
    recovery_dependency_reserve: usize,
    planned_backup_effects: u64,
    backup_plans: BTreeMap<u64, (String, u32)>,
    root_backup_plans: BTreeMap<String, (u64, RootBackupPlan)>,
    recovery_reservations: BTreeMap<String, usize>,
    recovery_backup_owners: BTreeMap<u64, String>,
    source_was_sealed: bool,
    preinstall_return_only: bool,
    return_root_manifests: BTreeMap<RootKind, String>,
    confirmed_context_roots: BTreeSet<String>,
    later_backup_plans: BTreeMap<RootKind, (u64, LaterBackupPlan)>,
    later_backup_owners: BTreeMap<u64, RootKind>,
    later_backup_history: BTreeMap<u64, LaterBackupPlan>,
    later_complete: BTreeMap<u64, String>,
    later_return_only: bool,
    bundle_seed: Option<(u64, String)>,
    bundle_backup_plan: Option<(u64, BundleBackupPlan)>,
    bundle_backup_generations: BTreeSet<u64>,
    bundle_return_only: bool,
}
impl SwitchJournal {
    pub(crate) fn new(binding: JournalBinding, capacity: CapacityPlan) -> Result<Self, SafeError> {
        binding.validate()?;
        capacity.validate(Limits::default(), 0)?;
        Ok(Self {
            binding,
            generation: 0,
            phase: JournalPhase::Reviewed,
            effects: BTreeMap::new(),
            applied_kinds: BTreeSet::new(),
            pending: None,
            manifests: BTreeMap::new(),
            capacity,
            recovering: false,
            compensated: BTreeSet::new(),
            return_roots: None,
            context_return_only: false,
            admitted_root_reversals: BTreeMap::new(),
            returned_reversals: BTreeSet::new(),
            recovery_dependency_reserve: 0,
            planned_backup_effects: 0,
            backup_plans: BTreeMap::new(),
            root_backup_plans: BTreeMap::new(),
            recovery_reservations: BTreeMap::new(),
            recovery_backup_owners: BTreeMap::new(),
            source_was_sealed: false,
            preinstall_return_only: false,
            return_root_manifests: BTreeMap::new(),
            confirmed_context_roots: BTreeSet::new(),
            later_backup_plans: BTreeMap::new(),
            later_backup_owners: BTreeMap::new(),
            later_backup_history: BTreeMap::new(),
            later_complete: BTreeMap::new(),
            later_return_only: false,
            bundle_seed: None,
            bundle_backup_plan: None,
            bundle_backup_generations: BTreeSet::new(),
            bundle_return_only: false,
        })
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.binding
    }
    pub(crate) fn phase(&self) -> JournalPhase {
        self.phase
    }
    /// Diagnostic lookup; publication still requires the live healthy store,
    /// exact binding and current head under its exclusive mutable borrow.
    pub(crate) fn manifest(&self, role: ManifestRole) -> Option<&str> {
        self.manifests.get(&role).map(String::as_str)
    }
    pub(crate) fn pending_effect(&self) -> Option<&EffectSpec> {
        self.pending
            .as_ref()
            .and_then(|id| self.effects.get(id))
            .map(|effect| &effect.spec)
    }
    pub(crate) fn requires_reconciliation(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn effect_observation(&self, effect_id: &str) -> Option<Observation> {
        self.effects
            .get(effect_id)
            .and_then(|effect| effect.result.as_ref())
            .map(|result| result.observation)
    }
    pub(crate) fn has_historical_uncertainty(&self) -> bool {
        !self.compensated.is_empty()
    }
    /// Diagnostic prerequisite for the concrete source factory. This boolean
    /// supplies no quiescence or filesystem authority on its own.
    pub(crate) fn source_snapshot_candidate(&self) -> bool {
        self.phase == JournalPhase::Reviewed
            && !self.recovering
            && !self.requires_reconciliation()
            && !self.has_historical_uncertainty()
            && !self.context_return_only
            && !self.preinstall_return_only
            && !self.later_return_only
            && !self.bundle_return_only
            && self.applied(EffectKind::FenceSourceImage)
            && self.effects.values().all(|effect| {
                matches!(
                    effect.spec.kind,
                    EffectKind::PrivateBackupEntry { .. }
                        | EffectKind::VerifySourceBundleCopy
                        | EffectKind::FenceSourceImage
                )
            })
    }

    /// Validation is read-only. Commit changes only the indexed affected entry;
    /// failed validation never requires cloning/rolling back the entire history.
    pub(crate) fn apply(&mut self, event: JournalEvent) -> Result<(), SafeError> {
        self.validate_event(&event)?;
        self.commit_event(event);
        Ok(())
    }
    fn validate_event(&self, event: &JournalEvent) -> Result<(), SafeError> {
        self.generation
            .checked_add(1)
            .ok_or_else(|| error("HISTORY_JOURNAL_LIMIT"))?;
        if matches!(
            self.phase,
            JournalPhase::Restored | JournalPhase::PreContextAborted
        ) {
            return Err(error("HISTORY_TRANSACTION_TERMINAL"));
        }
        match event {
            JournalEvent::Begin { .. } => return Err(error("HISTORY_JOURNAL_INVALID")),
            JournalEvent::PrivateBackupPlan {
                manifest,
                effects,
                recovery_dependencies,
            } => {
                validate_digest(manifest)?;
                let expected_reserve = self.recovery_dependency_reserve.max(128).saturating_add(
                    if !self.recovering && !self.recovery_reservations.contains_key(manifest) {
                        (*effects as usize).saturating_mul(4).saturating_add(8)
                    } else {
                        0
                    },
                );
                if *recovery_dependencies as usize != expected_reserve
                    || self.requires_reconciliation()
                    || *effects == 0
                    || *effects > 100_000
                    || !(128..=4096).contains(recovery_dependencies)
                    || !matches!(
                        self.phase,
                        JournalPhase::Reviewed
                            | JournalPhase::RecoveryRequired
                            | JournalPhase::HistoricalActive
                            | JournalPhase::InstalledUnconfirmed
                    )
                {
                    return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED"));
                }
            }
            JournalEvent::AdmitBundleStart {
                seed,
                current_manifest,
                effects,
                recovery_dependencies,
                receipt,
            } => {
                for digest in [seed, current_manifest, receipt] {
                    validate_digest(digest)?;
                }
                if self.bundle_seed.is_some()
                    || self.requires_reconciliation()
                    || self.context_return_only
                    || !matches!(
                        self.phase,
                        JournalPhase::RecoveryRequired
                            | JournalPhase::HistoricalActive
                            | JournalPhase::InstalledUnconfirmed
                    )
                    || !self.manifests.contains_key(&ManifestRole::SourceBundle)
                    || *effects == 0
                    || *effects > 100_000
                    || *recovery_dependencies as usize != self.recovery_dependency_reserve.max(128)
                {
                    return Err(error("HISTORY_BUNDLE_RECOVERY_BLOCKED"));
                }
            }
            JournalEvent::PrepareBundleBackup { plan, receipt } => {
                validate_digest(receipt)?;
                for digest in [
                    &plan.current_manifest,
                    &plan.destination,
                    &plan.previous_observation,
                ] {
                    validate_digest(digest)?;
                }
                if !matches!(
                    self.phase,
                    JournalPhase::RecoveryRequired
                        | JournalPhase::HistoricalActive
                        | JournalPhase::InstalledUnconfirmed
                        | JournalPhase::Restoring
                ) || self.bundle_seed.is_none()
                    || self.context_return_only
                    || !self.manifests.contains_key(&ManifestRole::SourceBundle)
                    || self
                        .bundle_backup_plan
                        .as_ref()
                        .map(|(generation, _)| *generation)
                        != plan.previous_generation
                    || plan.effects == 0
                    || plan.effects > 100_000
                    || plan.recovery_dependencies as usize
                        != self.recovery_dependency_reserve.max(128)
                {
                    return Err(error("HISTORY_BUNDLE_RECOVERY_BLOCKED"));
                }
                match (&self.pending, &plan.abandoned_effect) {
                    (None, None) => {}
                    (Some(pending), Some((id, generation))) if pending == id => {
                        let old = &self.effects[id];
                        let allowed = match &old.spec.kind {
                            EffectKind::PrivateBackupEntry {
                                plan_generation,
                                manifest,
                                ..
                            } => {
                                if let Some((active, prior)) = &self.bundle_backup_plan {
                                    active == plan_generation && &prior.current_manifest == manifest
                                } else {
                                    self.bundle_seed
                                        .as_ref()
                                        .is_some_and(|(active, _)| active == plan_generation)
                                        && self
                                            .backup_plans
                                            .get(plan_generation)
                                            .is_some_and(|(source, _)| source == manifest)
                                }
                            }
                            EffectKind::RecoveryFilesystemEntry {
                                context, manifest, ..
                            } => {
                                context == &self.binding.source_context
                                    && self.manifests.get(&ManifestRole::SourceBundle)
                                        == Some(manifest)
                            }
                            EffectKind::VerifySourceBundleRestore => true,
                            _ => false,
                        };
                        if !allowed
                            || old.intent_generation != *generation
                            || old
                                .result
                                .as_ref()
                                .is_some_and(|result| result.observation != Observation::Unknown)
                        {
                            return Err(error("HISTORY_BUNDLE_RECOVERY_BLOCKED"));
                        }
                    }
                    _ => return Err(error("HISTORY_BUNDLE_RECOVERY_BLOCKED")),
                }
            }
            JournalEvent::PrepareLaterBackup { plan, receipt } => {
                validate_digest(receipt)?;
                for digest in [
                    &plan.source_manifest,
                    &plan.destination,
                    &plan.source_reservation,
                ] {
                    validate_digest(digest)?;
                }
                if let Some(digest) = &plan.previous_observation {
                    validate_digest(digest)?;
                }
                let previous = self.later_backup_plans.get(&plan.root);
                let reserve = self.recovery_dependency_reserve.max(128).saturating_sub(
                    self.recovery_reservations
                        .get(&plan.source_reservation)
                        .copied()
                        .unwrap_or(0),
                );
                if !self.source_was_sealed
                    || !self.manifests.contains_key(&ManifestRole::SourceContext)
                    || self
                        .manifests
                        .contains_key(&ManifestRole::RetainedTargetContext)
                    || self.context_return_only
                    || !matches!(
                        self.phase,
                        JournalPhase::RecoveryRequired
                            | JournalPhase::HistoricalActive
                            | JournalPhase::InstalledUnconfirmed
                    )
                    || plan.effects == 0
                    || plan.effects > 100_000
                    || plan.recovery_dependencies as usize != reserve
                    || previous.map(|(generation, _)| *generation) != plan.previous_generation
                    || previous.is_some() != plan.previous_observation.is_some()
                    || previous
                        .is_some_and(|(_, old)| old.source_reservation != plan.source_reservation)
                {
                    return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED"));
                }
                match (&self.pending, &plan.abandoned_effect) {
                    (None, None) => {}
                    (Some(pending), Some((id, generation))) if pending == id => {
                        let old = &self.effects[id];
                        if old.intent_generation != *generation
                            || old
                                .result
                                .as_ref()
                                .is_some_and(|result| result.observation != Observation::Unknown)
                            || !matches!(&old.spec.kind, EffectKind::PrivateBackupEntry { plan_generation, manifest, .. }
                                if Some(*plan_generation) == plan.previous_generation && previous.is_some_and(|(_, previous)| &previous.source_manifest == manifest))
                        {
                            return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED"));
                        }
                    }
                    _ => return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED")),
                }
            }
            JournalEvent::CompleteLaterBackup {
                root,
                plan_generation,
                copy_manifest,
                receipt,
            } => {
                validate_digest(copy_manifest)?;
                validate_digest(receipt)?;
                if self.requires_reconciliation()
                    || self.phase != JournalPhase::RecoveryRequired
                    || self
                        .later_backup_plans
                        .get(root)
                        .is_none_or(|(current, _)| current != plan_generation)
                    || self.later_complete.contains_key(plan_generation)
                {
                    return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED"));
                }
            }
            JournalEvent::AdmitPreinstallReturn {
                roots,
                pending,
                receipt,
            } => {
                validate_digest(receipt)?;
                for digest in roots.values() {
                    validate_digest(digest)?;
                }
                let exact_pending = self
                    .pending
                    .as_ref()
                    .map(|id| (id.clone(), self.effects[id].intent_generation));
                if roots.len() != 2
                    || !roots.contains_key(&RootKind::Desk)
                    || !roots.contains_key(&RootKind::WebView)
                    || !self.source_was_sealed
                    || !self.preserved(&self.binding.source_context)
                    || ![
                        ManifestRole::SourceContext,
                        ManifestRole::SourceBundle,
                        ManifestRole::Registration,
                        ManifestRole::Shortcuts,
                    ]
                    .into_iter()
                    .all(|role| self.manifests.contains_key(&role))
                    || !self.applied(EffectKind::FenceSourceImage)
                    || !matches!(
                        self.phase,
                        JournalPhase::SourceSealed
                            | JournalPhase::FreshReady
                            | JournalPhase::RecoveryRequired
                    )
                    || &exact_pending != pending
                    || self.context_return_only
                    || self.effects.values().any(|effect| {
                        matches!(
                            effect.spec.kind,
                            EffectKind::InstallerCreateSuspended
                                | EffectKind::InstallerResume
                                | EffectKind::InstallerTerminalOutcome
                                | EffectKind::HistoricalCreateSuspended
                                | EffectKind::HistoricalResume
                                | EffectKind::HistoricalTerminalOutcome
                                | EffectKind::VerifyTargetBundle
                                | EffectKind::ConfirmFirstLaunch
                        )
                    })
                {
                    return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
                }
                if let Some((id, _)) = pending {
                    if !matches!(
                        self.effects[id].spec.kind,
                        EffectKind::CreateFreshRoot { .. }
                    ) {
                        return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
                    }
                }
            }
            JournalEvent::ConfirmContextRoot {
                effect_id,
                intent_generation,
                current_manifest,
                receipt,
                ..
            } => {
                validate_digest(receipt)?;
                validate_digest(current_manifest)?;
                let effect = self
                    .effects
                    .get(effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if self.pending.as_deref() != Some(effect_id)
                    || effect.intent_generation != *intent_generation
                    || effect
                        .result
                        .as_ref()
                        .is_some_and(|result| result.observation != Observation::Unknown)
                    || !match &effect.spec.kind {
                        EffectKind::PreserveRoot { context, .. } => {
                            context == &self.binding.target_context
                                && matches!(
                                    self.phase,
                                    JournalPhase::RecoveryRequired
                                        | JournalPhase::HistoricalActive
                                        | JournalPhase::InstalledUnconfirmed
                                )
                        }
                        EffectKind::RestoreSourceRoot { .. } => {
                            self.phase == JournalPhase::Restoring
                        }
                        _ => false,
                    }
                {
                    return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
                }
            }
            JournalEvent::PrepareRootBackup { plan, receipt } => {
                validate_digest(receipt)?;
                for digest in [
                    &plan.current_manifest,
                    &plan.destination,
                    &plan.reservation_source,
                ] {
                    validate_digest(digest)?;
                }
                if let Some(digest) = &plan.preserved_manifest {
                    validate_digest(digest)?;
                }
                let original = self
                    .effects
                    .get(&plan.original_effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                let prior = self.root_backup_plans.get(&plan.original_effect_id);
                let remaining = self.recovery_dependency_reserve.max(128).saturating_sub(
                    self.recovery_reservations
                        .get(&plan.reservation_source)
                        .copied()
                        .unwrap_or(0),
                );
                if !matches!(original.spec.kind, EffectKind::RotateSourceRoot { .. })
                    || original.intent_generation != plan.original_intent_generation
                    || !self.admitted_root_reversals.contains_key(&plan.original_effect_id)
                    || !self.context_return_only || !self.pre_context_phase()
                    || plan.effects == 0 || plan.effects > 100_000
                    || plan.recovery_dependencies as usize != remaining
                    || prior.map(|(generation, _)| *generation) != plan.prior_plan_generation
                    || prior.is_some() != plan.preserved_manifest.is_some()
                    || self.effects.values().any(|effect| matches!(&effect.spec.kind,
                        EffectKind::ReverseSourceRoot { original_effect_id, .. } if original_effect_id == &plan.original_effect_id))
                { return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED")); }
                match (&self.pending, &plan.abandoned_effect) {
                    (None, None) => {}
                    (Some(pending), Some((id, generation))) if pending == id => {
                        let old = &self.effects[id];
                        if old.intent_generation != *generation
                            || !matches!(&old.spec.kind, EffectKind::PrivateBackupEntry { plan_generation, manifest, .. }
                                if Some(*plan_generation) == plan.prior_plan_generation
                                    && prior.is_some_and(|(_, prior)| &prior.current_manifest == manifest))
                            || old
                                .result
                                .as_ref()
                                .is_some_and(|result| result.observation != Observation::Unknown)
                        {
                            return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED"));
                        }
                    }
                    _ => return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED")),
                }
            }
            JournalEvent::ConfirmRootReturned {
                effect_id,
                intent_generation,
                current_manifest,
                receipt,
            } => {
                validate_digest(current_manifest)?;
                validate_digest(receipt)?;
                let effect = self
                    .effects
                    .get(effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if !matches!(effect.spec.kind, EffectKind::ReverseSourceRoot { .. })
                    || effect.intent_generation != *intent_generation
                    || self.pending.as_deref() != Some(effect_id)
                    || effect
                        .result
                        .as_ref()
                        .is_some_and(|result| result.observation != Observation::Unknown)
                    || self.returned_reversals.contains(effect_id)
                    || !self.context_return_only
                    || !self.pre_context_phase()
                {
                    return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED"));
                }
            }
            JournalEvent::AdmitRootReverse {
                effect_id,
                intent_generation,
                current_manifest,
                receipt,
            } => {
                validate_digest(current_manifest)?;
                validate_digest(receipt)?;
                let effect = self
                    .effects
                    .get(effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if !matches!(effect.spec.kind, EffectKind::RotateSourceRoot { .. })
                    || effect.intent_generation != *intent_generation
                    || self.pending.as_deref().is_some_and(|pending| {
                        pending != effect_id
                            && !matches!(&self.effects[pending].spec.kind,
                                EffectKind::PrivateBackupEntry { plan_generation, .. }
                                if !self.recovery_backup_owners.contains_key(plan_generation))
                    })
                    || self.admitted_root_reversals.contains_key(effect_id)
                    || self.has_historical_uncertainty()
                    || !self.pre_context_phase()
                {
                    return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED"));
                }
            }
            JournalEvent::Manifest { role, digest } => {
                validate_digest(digest)?;
                if (self.bundle_return_only || self.later_return_only)
                    && (*role != ManifestRole::RetainedTargetContext
                        || self.phase != JournalPhase::RecoveryRequired)
                {
                    return Err(error("HISTORY_RETURN_ONLY"));
                }
                if *role == ManifestRole::SourceHandoffExit
                    && !self.manifests.contains_key(&ManifestRole::ManagerHandoff)
                {
                    return Err(error("HISTORY_HANDOFF_CHANGED"));
                }
                if matches!(
                    role,
                    ManifestRole::ManagerHandoff | ManifestRole::SourceHandoffExit
                ) && self.phase != JournalPhase::Reviewed
                {
                    return Err(error("HISTORY_HANDOFF_CHANGED"));
                }
                if self.context_return_only
                    || (self.preinstall_return_only && *role != ManifestRole::RetainedTargetContext)
                {
                    return Err(error("HISTORY_CONTEXT_RETURN_ONLY"));
                }
                if self.requires_reconciliation() || self.manifests.contains_key(role) {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
                if self.has_historical_uncertainty()
                    && (*role != ManifestRole::RetainedTargetContext
                        || self.phase != JournalPhase::RecoveryRequired)
                {
                    return Err(error("HISTORY_RETURN_ONLY"));
                }
            }
            JournalEvent::Intent { effect } => {
                effect.validate(&self.binding)?;
                if (self.bundle_return_only || self.later_return_only)
                    && !self.return_effect_allowed(effect)
                {
                    return Err(error("HISTORY_RETURN_ONLY"));
                }
                if self.preinstall_return_only && !self.preinstall_return_effect(&effect.kind) {
                    return Err(error("HISTORY_CONTEXT_RETURN_ONLY"));
                }
                if matches!(effect.kind, EffectKind::RecoveryShortcutEntry { .. })
                    && !self.return_effect_allowed(effect)
                {
                    return Err(error("HISTORY_SHORTCUT_RESTORE_BLOCKED"));
                }
                if self.context_return_only
                    && !matches!(
                        effect.kind,
                        EffectKind::PrivateBackupEntry { .. }
                            | EffectKind::ReverseSourceRoot { .. }
                            | EffectKind::ReverseSourceFence { .. }
                    )
                {
                    return Err(error("HISTORY_CONTEXT_RETURN_ONLY"));
                }
                if let EffectKind::PrivateBackupEntry {
                    plan_generation,
                    manifest,
                    entry_index,
                    ..
                } = &effect.kind
                {
                    if self.bundle_backup_generations.contains(plan_generation)
                        && (self
                            .bundle_backup_plan
                            .as_ref()
                            .map(|(generation, _)| *generation)
                            .or_else(|| {
                                self.bundle_seed.as_ref().map(|(generation, _)| *generation)
                            })
                            != Some(*plan_generation)
                            || !matches!(
                                self.phase,
                                JournalPhase::RecoveryRequired | JournalPhase::Restoring
                            ))
                    {
                        return Err(error("HISTORY_BUNDLE_RECOVERY_BLOCKED"));
                    }
                    if self
                        .later_backup_owners
                        .get(plan_generation)
                        .is_some_and(|root| {
                            self.later_backup_plans
                                .get(root)
                                .is_none_or(|(current, _)| current != plan_generation)
                                || !matches!(
                                    self.phase,
                                    JournalPhase::RecoveryRequired
                                        | JournalPhase::HistoricalActive
                                        | JournalPhase::InstalledUnconfirmed
                                )
                        })
                    {
                        return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED"));
                    }
                    if self.recovery_backup_owners.get(plan_generation).is_some_and(|owner|
                        self.root_backup_plans.get(owner).is_none_or(|(current, _)| current != plan_generation)
                            || self.effects.values().any(|existing| matches!(&existing.spec.kind,
                                EffectKind::ReverseSourceRoot { original_effect_id, .. } if original_effect_id == owner)))
                        || self.backup_plans.get(plan_generation)
                        .is_none_or(|(source, count)| source != manifest || entry_index >= count)
                        || self.effects.values().any(|old| matches!(&old.spec.kind,
                            EffectKind::PrivateBackupEntry { plan_generation: old_plan, entry_index: old_index, .. }
                            if old_plan == plan_generation && old_index == entry_index))
                    { return Err(error("HISTORY_CONTEXT_PLAN_BLOCKED")); }
                }
                if let EffectKind::ReverseSourceRoot {
                    original_effect_id,
                    original_intent_generation,
                    current_manifest,
                } = &effect.kind
                {
                    let original = self
                        .effects
                        .get(original_effect_id)
                        .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    if !matches!(original.spec.kind, EffectKind::RotateSourceRoot { .. })
                        || original.intent_generation != *original_intent_generation
                        || self.admitted_root_reversals.get(original_effect_id) != Some(current_manifest)
                        || self.effects.values().any(|existing| matches!(&existing.spec.kind,
                            EffectKind::ReverseSourceRoot { original_effect_id: old, .. } if old == original_effect_id)) {
                        return Err(error("HISTORY_CONTEXT_REVERSE_BLOCKED"));
                    }
                }
                if self.requires_reconciliation()
                    || self.effects.len() >= MAX_RECORDS
                    || self.effects.contains_key(&effect.effect_id)
                {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
                if self.has_historical_uncertainty() && !self.return_effect_allowed(effect) {
                    return Err(error("HISTORY_RETURN_ONLY"));
                }
                if let EffectKind::ReverseSourceFence {
                    original_effect_id,
                    original_intent_generation,
                } = &effect.kind
                {
                    let original = self
                        .effects
                        .get(original_effect_id)
                        .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    if original.spec.kind != EffectKind::FenceSourceImage
                        || original.intent_generation != *original_intent_generation
                        || self.effect_observation(original_effect_id) != Some(Observation::Applied)
                        || effect.expected_postconditions != original.spec.before
                        || self.applied(effect.kind.clone())
                    {
                        return Err(error("HISTORY_EFFECT_CHANGED"));
                    }
                }
            }
            JournalEvent::Observed {
                effect_id,
                intent_generation,
                result,
            } => {
                let effect = self
                    .effects
                    .get(effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if self.pending.as_deref() != Some(effect_id.as_str())
                    || effect.intent_generation != *intent_generation
                    || effect
                        .result
                        .as_ref()
                        .is_some_and(|old| old.observation != Observation::Unknown)
                {
                    return Err(error("HISTORY_EFFECT_CHANGED"));
                }
                match (&result.observation, &result.receipt) {
                    (Observation::Applied | Observation::NotApplied, Some(receipt)) => {
                        validate_digest(receipt)?
                    }
                    (Observation::Unknown, None) => (),
                    _ => return Err(error("HISTORY_EFFECT_CHANGED")),
                }
            }
            JournalEvent::Phase { phase } => {
                if (self.bundle_return_only || self.later_return_only)
                    && !matches!(phase, JournalPhase::Restoring | JournalPhase::Restored)
                {
                    return Err(error("HISTORY_RETURN_ONLY"));
                }
                if self.context_return_only && *phase != JournalPhase::RecoveryRequired {
                    return Err(error("HISTORY_CONTEXT_RETURN_ONLY"));
                }
                if *phase != JournalPhase::RecoveryRequired
                    && (self.requires_reconciliation() || !self.phase_allowed(*phase))
                {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
            }
            JournalEvent::AbortPreContext { receipt } => {
                validate_digest(receipt)?;
                if !self.can_abort_pre_context() {
                    return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
                }
            }
            JournalEvent::CompensateUnknown {
                effect_id,
                intent_generation,
                current_roots,
                receipt,
            } => {
                validate_digest(receipt)?;
                validate_digest(current_roots)?;
                let effect = self
                    .effects
                    .get(effect_id)
                    .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if self.pending.as_deref() != Some(effect_id.as_str())
                    || effect.intent_generation != *intent_generation
                    || effect
                        .result
                        .as_ref()
                        .is_some_and(|result| result.observation != Observation::Unknown)
                    || !matches!(
                        effect.spec.kind,
                        EffectKind::InstallerCreateSuspended
                            | EffectKind::InstallerResume
                            | EffectKind::InstallerTerminalOutcome
                            | EffectKind::HistoricalCreateSuspended
                            | EffectKind::HistoricalResume
                            | EffectKind::HistoricalTerminalOutcome
                    )
                    || !matches!(
                        self.phase,
                        JournalPhase::Installing
                            | JournalPhase::InstalledUnconfirmed
                            | JournalPhase::HistoricalActive
                            | JournalPhase::RecoveryRequired
                    )
                    || !self.source_ready_for_return()
                    || self.has_historical_uncertainty()
                {
                    return Err(error("HISTORY_COMPENSATION_BLOCKED"));
                }
            }
        }
        Ok(())
    }
    fn lane(&self, event: &JournalEvent) -> WriteLane {
        if self.recovering
            || matches!(
                event,
                JournalEvent::Phase {
                    phase: JournalPhase::RecoveryRequired | JournalPhase::Restoring
                } | JournalEvent::AbortPreContext { .. }
                    | JournalEvent::CompensateUnknown { .. }
                    | JournalEvent::AdmitRootReverse { .. }
                    | JournalEvent::ConfirmRootReturned { .. }
                    | JournalEvent::PrepareRootBackup { .. }
                    | JournalEvent::AdmitPreinstallReturn { .. }
                    | JournalEvent::ConfirmContextRoot { .. }
                    | JournalEvent::PrepareLaterBackup { .. }
                    | JournalEvent::CompleteLaterBackup { .. }
                    | JournalEvent::PrepareBundleBackup { .. }
                    | JournalEvent::AdmitBundleStart { .. }
            )
        {
            WriteLane::Recovery
        } else {
            WriteLane::Forward
        }
    }
    fn commit_event(&mut self, event: JournalEvent) {
        self.generation += 1;
        self.recovering |= self.lane(&event) == WriteLane::Recovery;
        match event {
            JournalEvent::Begin { .. } => unreachable!("genesis is not an appended event"),
            JournalEvent::PrivateBackupPlan {
                manifest,
                recovery_dependencies,
                effects,
            } => {
                self.recovery_dependency_reserve = recovery_dependencies as usize;
                self.backup_plans
                    .insert(self.generation, (manifest.clone(), effects));
                if !self.recovering {
                    self.planned_backup_effects += effects as u64;
                    self.recovery_reservations
                        .entry(manifest)
                        .or_insert(effects as usize * 4 + 8);
                }
            }
            JournalEvent::AdmitBundleStart {
                seed,
                current_manifest,
                effects,
                recovery_dependencies,
                ..
            } => {
                self.bundle_seed = Some((self.generation, seed));
                self.backup_plans
                    .insert(self.generation, (current_manifest, effects));
                self.bundle_backup_generations.insert(self.generation);
                self.recovery_dependency_reserve = recovery_dependencies as usize;
                self.bundle_return_only = true;
                self.phase = JournalPhase::RecoveryRequired;
            }
            JournalEvent::PrepareBundleBackup { plan, .. } => {
                if let Some((id, _)) = &plan.abandoned_effect {
                    let old = self.effects.get_mut(id).expect("validated bundle effect");
                    if old.result.is_none() {
                        old.result = Some(ObservedResult {
                            observation: Observation::Unknown,
                            receipt: None,
                        });
                    }
                }
                self.pending = None;
                self.bundle_return_only = true;
                self.applied_kinds
                    .remove(&EffectKind::VerifySourceBundleRestore);
                self.recovery_dependency_reserve = plan.recovery_dependencies as usize;
                self.backup_plans.insert(
                    self.generation,
                    (plan.current_manifest.clone(), plan.effects),
                );
                self.bundle_backup_generations.insert(self.generation);
                self.bundle_backup_plan = Some((self.generation, plan));
                if self.phase != JournalPhase::Restoring {
                    self.phase = JournalPhase::RecoveryRequired;
                }
            }
            JournalEvent::PrepareLaterBackup { plan, .. } => {
                self.later_return_only = true;
                self.phase = JournalPhase::RecoveryRequired;
                if let Some((id, _)) = &plan.abandoned_effect {
                    let old = self.effects.get_mut(id).expect("validated later copy");
                    if old.result.is_none() {
                        old.result = Some(ObservedResult {
                            observation: Observation::Unknown,
                            receipt: None,
                        });
                    }
                }
                self.pending = None;
                self.recovery_reservations.remove(&plan.source_reservation);
                self.recovery_dependency_reserve = plan.recovery_dependencies as usize;
                self.backup_plans.insert(
                    self.generation,
                    (plan.source_manifest.clone(), plan.effects),
                );
                self.later_backup_owners.insert(self.generation, plan.root);
                self.later_backup_history
                    .insert(self.generation, plan.clone());
                self.later_backup_plans
                    .insert(plan.root, (self.generation, plan));
            }
            JournalEvent::CompleteLaterBackup {
                plan_generation,
                copy_manifest,
                ..
            } => {
                self.later_complete.insert(plan_generation, copy_manifest);
            }
            JournalEvent::AdmitPreinstallReturn { roots, pending, .. } => {
                if let Some((id, _)) = pending {
                    let effect = self
                        .effects
                        .get_mut(&id)
                        .expect("validated uncertain fresh effect");
                    if effect.result.is_none() {
                        effect.result = Some(ObservedResult {
                            observation: Observation::Unknown,
                            receipt: None,
                        });
                    }
                }
                self.pending = None;
                self.preinstall_return_only = true;
                self.return_root_manifests = roots;
                self.phase = JournalPhase::RecoveryRequired;
            }
            JournalEvent::ConfirmContextRoot {
                effect_id,
                completed,
                ..
            } => {
                let effect = self
                    .effects
                    .get_mut(&effect_id)
                    .expect("validated root observation");
                if effect.result.is_none() {
                    effect.result = Some(ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    });
                }
                if completed {
                    self.confirmed_context_roots.insert(effect_id);
                }
                self.pending = None;
            }
            JournalEvent::PrepareRootBackup { plan, .. } => {
                if let Some((id, _)) = &plan.abandoned_effect {
                    let old = self.effects.get_mut(id).expect("validated old C1 effect");
                    if old.result.is_none() {
                        old.result = Some(ObservedResult {
                            observation: Observation::Unknown,
                            receipt: None,
                        });
                    }
                }
                self.pending = None;
                self.recovery_backup_owners
                    .insert(self.generation, plan.original_effect_id.clone());
                self.recovery_reservations.remove(&plan.reservation_source);
                self.recovery_dependency_reserve = plan.recovery_dependencies as usize;
                self.backup_plans.insert(
                    self.generation,
                    (plan.current_manifest.clone(), plan.effects),
                );
                self.admitted_root_reversals.insert(
                    plan.original_effect_id.clone(),
                    plan.current_manifest.clone(),
                );
                self.root_backup_plans
                    .insert(plan.original_effect_id.clone(), (self.generation, plan));
            }
            JournalEvent::ConfirmRootReturned { effect_id, .. } => {
                let effect = self
                    .effects
                    .get_mut(&effect_id)
                    .expect("validated returned root");
                if effect.result.is_none() {
                    effect.result = Some(ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    });
                }
                self.returned_reversals.insert(effect_id);
                self.pending = None;
            }
            JournalEvent::AdmitRootReverse {
                effect_id,
                current_manifest,
                ..
            } => {
                if let Some(pending) = &self.pending {
                    if pending != &effect_id {
                        let backup = self
                            .effects
                            .get_mut(pending)
                            .expect("validated private backup uncertainty");
                        if backup.result.is_none() {
                            backup.result = Some(ObservedResult {
                                observation: Observation::Unknown,
                                receipt: None,
                            });
                        }
                    }
                }
                let effect = self
                    .effects
                    .get_mut(&effect_id)
                    .expect("validated root reversal");
                if effect.result.is_none() {
                    effect.result = Some(ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    });
                }
                self.admitted_root_reversals
                    .insert(effect_id, current_manifest);
                self.pending = None;
                self.context_return_only = true;
                self.phase = JournalPhase::RecoveryRequired;
            }
            JournalEvent::Manifest { role, digest } => {
                self.manifests.insert(role, digest);
            }
            JournalEvent::Intent { effect } => {
                self.pending = Some(effect.effect_id.clone());
                self.effects.insert(
                    effect.effect_id.clone(),
                    EffectRecord {
                        spec: effect,
                        intent_generation: self.generation,
                        result: None,
                    },
                );
            }
            JournalEvent::Observed {
                effect_id, result, ..
            } => {
                let effect = self.effects.get_mut(&effect_id).expect("validated effect");
                if result.observation == Observation::Applied {
                    self.applied_kinds.insert(effect.spec.kind.clone());
                }
                if result.observation != Observation::Unknown {
                    self.pending = None;
                }
                effect.result = Some(result);
            }
            JournalEvent::Phase { phase } => {
                self.source_was_sealed |= phase == JournalPhase::SourceSealed;
                self.phase = phase;
            }
            JournalEvent::AbortPreContext { .. } => {
                if let Some(pending) = self.pending.take() {
                    let effect = self
                        .effects
                        .get_mut(&pending)
                        .expect("validated private backup uncertainty");
                    if effect.result.is_none() {
                        effect.result = Some(ObservedResult {
                            observation: Observation::Unknown,
                            receipt: None,
                        });
                    }
                }
                self.phase = JournalPhase::PreContextAborted;
            }
            JournalEvent::CompensateUnknown {
                effect_id,
                current_roots,
                ..
            } => {
                let effect = self
                    .effects
                    .get_mut(&effect_id)
                    .expect("validated pending effect");
                if effect.result.is_none() {
                    effect.result = Some(ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    });
                }
                self.compensated.insert(effect_id);
                self.pending = None;
                self.return_roots = Some(current_roots);
                self.phase = JournalPhase::RecoveryRequired;
            }
        }
    }
    fn source_ready_for_return(&self) -> bool {
        [
            ManifestRole::SourceContext,
            ManifestRole::SourceBundle,
            ManifestRole::Registration,
            ManifestRole::Shortcuts,
            ManifestRole::FreshTargetContext,
        ]
        .into_iter()
        .all(|role| self.manifests.contains_key(&role))
            && self.preserved(&self.binding.source_context)
            && self.applied(EffectKind::VerifySourceBundleCopy)
            && self.applied(EffectKind::FenceSourceImage)
            && [RootKind::Desk, RootKind::WebView]
                .into_iter()
                .all(|root| self.applied(EffectKind::CreateFreshRoot { root }))
    }
    fn pre_context_phase(&self) -> bool {
        matches!(
            self.phase,
            JournalPhase::Reviewed | JournalPhase::RecoveryRequired
        ) && ![
            ManifestRole::SourceContext,
            ManifestRole::FreshTargetContext,
            ManifestRole::RetainedTargetContext,
        ]
        .into_iter()
        .any(|role| self.manifests.contains_key(&role))
    }
    fn can_abort_pre_context(&self) -> bool {
        if self.has_historical_uncertainty()
            || !self.pre_context_phase()
            || self.pending.as_ref().is_some_and(|pending| {
                !matches!(
                    self.effects[pending].spec.kind,
                    EffectKind::PrivateBackupEntry { .. }
                )
            })
        {
            return false;
        }
        self.effects.values().all(|effect| {
            if matches!(effect.spec.kind, EffectKind::PrivateBackupEntry { .. }) { return true; }
            if matches!(effect.spec.kind, EffectKind::RotateSourceRoot { .. }) {
                return self.effects.values().any(|reverse| matches!(&reverse.spec.kind,
                    EffectKind::ReverseSourceRoot { original_effect_id, original_intent_generation, .. }
                    if original_effect_id == &effect.spec.effect_id && *original_intent_generation == effect.intent_generation)
                    && (self.returned_reversals.contains(&reverse.spec.effect_id) || reverse.result.as_ref().is_some_and(|result| result.observation == Observation::Applied)));
            }
            let Some(result) = &effect.result else { return false; };
            if result.observation == Observation::Unknown && !self.returned_reversals.contains(&effect.spec.effect_id) { return false; }
            match &effect.spec.kind {
                EffectKind::VerifySourceBundleCopy | EffectKind::ReverseSourceFence { .. } | EffectKind::ReverseSourceRoot { .. } => true,
                EffectKind::FenceSourceImage => result.observation == Observation::NotApplied
                    || self.applied(EffectKind::ReverseSourceFence { original_effect_id: effect.spec.effect_id.clone(), original_intent_generation: effect.intent_generation }),
                _ => false,
            }
        })
    }
    fn can_abort_private_only(&self) -> bool {
        self.pre_context_phase()
            && !self.has_historical_uncertainty()
            && !self.manifests.contains_key(&ManifestRole::ManagerHandoff)
            && self.effects.values().all(|effect| {
                matches!(
                    effect.spec.kind,
                    EffectKind::PrivateBackupEntry { .. } | EffectKind::VerifySourceBundleCopy
                )
            })
            && self.can_abort_pre_context()
    }
    #[cfg(test)]
    pub(crate) fn fixture_private_abort_eligible(&self) -> bool {
        self.can_abort_private_only()
    }
    fn return_effect_allowed(&self, effect: &EffectSpec) -> bool {
        match &effect.kind {
            EffectKind::PrivateBackupEntry { .. } => matches!(
                self.phase,
                JournalPhase::RecoveryRequired | JournalPhase::Restoring
            ),
            EffectKind::FenceHistoricalImage => self.phase == JournalPhase::RecoveryRequired,
            EffectKind::PreserveRoot { context, .. } => {
                self.phase == JournalPhase::RecoveryRequired
                    && context == &self.binding.target_context
            }
            EffectKind::RecoveryFilesystemEntry {
                context, manifest, ..
            } => {
                if context == &self.binding.target_context
                    && self.phase == JournalPhase::RecoveryRequired
                {
                    self.return_roots.as_ref() == Some(manifest)
                        || self.manifests.get(&ManifestRole::RetainedTargetContext)
                            == Some(manifest)
                } else {
                    context == &self.binding.source_context
                        && self.phase == JournalPhase::Restoring
                        && [ManifestRole::SourceContext, ManifestRole::SourceBundle]
                            .into_iter()
                            .any(|role| self.manifests.get(&role) == Some(manifest))
                }
            }
            EffectKind::RecoveryRegistrationEntry { manifest, .. } => {
                self.phase == JournalPhase::Restoring
                    && self.manifests.get(&ManifestRole::Registration) == Some(manifest)
            }
            EffectKind::RecoveryShortcutEntry { manifest, .. } => {
                self.phase == JournalPhase::Restoring
                    && self.manifests.get(&ManifestRole::Shortcuts) == Some(manifest)
            }
            EffectKind::VerifySourceBundleRestore
            | EffectKind::RestoreSourceRoot { .. }
            | EffectKind::VerifyRegistrationRestore { .. }
            | EffectKind::RestoreShortcut { .. } => self.phase == JournalPhase::Restoring,
            _ => false,
        }
    }
    fn preinstall_return_effect(&self, kind: &EffectKind) -> bool {
        match kind {
            EffectKind::PrivateBackupEntry { .. } => true,
            EffectKind::PreserveRoot { context, .. } => context == &self.binding.target_context,
            EffectKind::RecoveryFilesystemEntry { context, .. } => {
                context == &self.binding.source_context && self.phase == JournalPhase::Restoring
            }
            EffectKind::RestoreSourceRoot { .. }
            | EffectKind::VerifySourceBundleRestore
            | EffectKind::RecoveryRegistrationEntry { .. }
            | EffectKind::RecoveryShortcutEntry { .. }
            | EffectKind::VerifyRegistrationRestore { .. }
            | EffectKind::RestoreShortcut { .. } => self.phase == JournalPhase::Restoring,
            _ => false,
        }
    }
    fn applied(&self, kind: EffectKind) -> bool {
        self.applied_kinds.contains(&kind)
            || self.confirmed_context_roots.iter().any(|id| {
                self.effects
                    .get(id)
                    .is_some_and(|effect| effect.spec.kind == kind)
            })
    }
    fn preserved(&self, context: &str) -> bool {
        [RootKind::Desk, RootKind::WebView].into_iter().all(|root| {
            self.applied(EffectKind::PreserveRoot {
                context: context.into(),
                root,
            })
        })
    }
    fn phase_allowed(&self, next: JournalPhase) -> bool {
        if self.preinstall_return_only
            && !matches!(next, JournalPhase::Restoring | JournalPhase::Restored)
        {
            return false;
        }
        match (self.phase, next) {
            (JournalPhase::Reviewed, JournalPhase::SourceSealed) => {
                self.applied(EffectKind::VerifySourceBundleCopy)
                    && self.applied(EffectKind::FenceSourceImage)
                    && self.preserved(&self.binding.source_context)
                    && [
                        ManifestRole::SourceContext,
                        ManifestRole::SourceBundle,
                        ManifestRole::Registration,
                        ManifestRole::Shortcuts,
                    ]
                    .into_iter()
                    .all(|role| self.manifests.contains_key(&role))
            }
            (JournalPhase::SourceSealed, JournalPhase::FreshReady) => {
                [RootKind::Desk, RootKind::WebView]
                    .into_iter()
                    .all(|root| self.applied(EffectKind::CreateFreshRoot { root }))
                    && self
                        .manifests
                        .contains_key(&ManifestRole::FreshTargetContext)
            }
            (JournalPhase::FreshReady, JournalPhase::Installing) => true,
            (JournalPhase::Installing, JournalPhase::InstalledUnconfirmed) => {
                self.applied(EffectKind::InstallerCreateSuspended)
                    && self.applied(EffectKind::InstallerResume)
                    && self.applied(EffectKind::InstallerTerminalOutcome)
                    && self.applied(EffectKind::VerifyTargetBundle)
            }
            (JournalPhase::InstalledUnconfirmed, JournalPhase::HistoricalActive) => {
                self.applied(EffectKind::ConfirmFirstLaunch)
            }
            (
                JournalPhase::HistoricalActive
                | JournalPhase::RecoveryRequired
                | JournalPhase::InstalledUnconfirmed,
                JournalPhase::Restoring,
            ) => {
                (self.applied(EffectKind::FenceHistoricalImage) || self.preinstall_return_only)
                    && self.preserved(&self.binding.target_context)
                    && self
                        .manifests
                        .contains_key(&ManifestRole::RetainedTargetContext)
                    && self.manifests.contains_key(&ManifestRole::SourceContext)
            }
            (JournalPhase::Restoring, JournalPhase::Restored) => {
                self.applied(EffectKind::VerifySourceBundleRestore)
                    && [RootKind::Desk, RootKind::WebView]
                        .into_iter()
                        .all(|root| self.applied(EffectKind::RestoreSourceRoot { root }))
                    && [
                        RegistrationSlot::Uninstall,
                        RegistrationSlot::Publisher,
                        RegistrationSlot::DeskDirectory,
                        RegistrationSlot::DeskDirectoryBackground,
                        RegistrationSlot::LegacyDirectory,
                        RegistrationSlot::LegacyDirectoryBackground,
                        RegistrationSlot::OwnedRun,
                    ]
                    .into_iter()
                    .all(|slot| self.applied(EffectKind::VerifyRegistrationRestore { slot }))
                    && [ShortcutSlot::Desktop, ShortcutSlot::StartMenu]
                        .into_iter()
                        .all(|slot| self.applied(EffectKind::RestoreShortcut { slot }))
            }
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    binding: JournalBinding,
    generation: u64,
    previous: Option<String>,
    lane: Option<WriteLane>,
    event: JournalEvent,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    record: Record,
    digest: String,
}
impl Envelope {
    fn new(record: Record) -> Result<Self, SafeError> {
        let bytes = serde_json::to_vec(&record).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        Ok(Self {
            record,
            digest: sha256(&bytes),
        })
    }
    fn encode(&self) -> Result<Vec<u8>, SafeError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(error("HISTORY_JOURNAL_LIMIT"));
        }
        Ok(bytes)
    }
    fn decode(bytes: &[u8]) -> Result<Self, SafeError> {
        if bytes.len() > MAX_RECORD_BYTES || !bytes.ends_with(b"\n") {
            return Err(error("HISTORY_JOURNAL_INVALID"));
        }
        let envelope: Self =
            serde_json::from_slice(bytes).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        let record =
            serde_json::to_vec(&envelope.record).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        if envelope.record.schema != 2 || sha256(&record) != envelope.digest {
            return Err(error("HISTORY_JOURNAL_INVALID"));
        }
        Ok(envelope)
    }
}

pub(crate) struct JournalInspection {
    /// Prior valid state is diagnostic only, never fallback permission to write.
    pub(crate) last_valid: Option<SwitchJournal>,
    pub(crate) blocked: bool,
    head: Option<String>,
    usage: Usage,
    hash: Sha256,
    identity: String,
}
impl JournalInspection {
    pub(super) fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }
}
#[cfg(any(test, windows))]
struct WriterState {
    journal: SwitchJournal,
    head: String,
    usage: Usage,
    hash: Sha256,
    identity: String,
}
#[cfg(any(test, windows))]
struct ProtectedManifest {
    #[cfg(test)]
    fixture: Option<FixtureManifest>,
    #[cfg(windows)]
    windows: Option<std::sync::Arc<Mutex<super::windows::durability::DurableArtifact>>>,
}
/// Shares the actual protected artifact handle with the writer. Its mutex also
/// serializes the shared Windows file cursor; no duplicate ambient file open or
/// caller-supplied artifact digest can create this evidence.
#[cfg(windows)]
pub(crate) struct RetainedRoleGuard {
    binding: JournalBinding,
    role: ManifestRole,
    root: std::sync::Arc<super::windows::files::PrivateDirectory>,
    artifact: std::sync::Arc<Mutex<super::windows::durability::DurableArtifact>>,
}
#[cfg(windows)]
impl RetainedRoleGuard {
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.artifact.lock().verify().map_err(storage_error)
    }
    pub(crate) fn read(&self) -> Result<Vec<u8>, SafeError> {
        self.artifact.lock().read().map_err(storage_error)
    }
    pub(crate) fn verify_role(
        &self,
        binding: &JournalBinding,
        role: ManifestRole,
        root: &super::windows::files::PrivateDirectory,
    ) -> Result<(), SafeError> {
        if &self.binding != binding
            || self.role != role
            || self.root.directory().identity() != root.directory().identity()
        {
            return Err(error("HISTORY_MANIFEST_CHANGED"));
        }
        self.verify()
    }
}
#[cfg(test)]
struct FixtureManifest {
    file: File,
    identity: String,
    length: u64,
}

#[derive(Clone, Copy)]
#[cfg(any(test, windows))]
enum WriterTrust {
    /// Real retained writable Windows handle with FILE_SHARE_READ only. Other
    /// write/delete opens and pathname replacement are denied for its lifetime.
    #[cfg(windows)]
    HeldWindowsHandle,
    /// Test fixtures on unsupported platforms rehash EVERY byte before append.
    /// This is intentionally not a production writer-exclusion proof or fast path.
    FixtureFullHash,
}

#[cfg(any(test, windows))]
enum JournalStorage {
    #[cfg(test)]
    Fixture {
        directory: Dir,
        durability: Box<dyn DirectoryDurability>,
    },
    #[cfg(windows)]
    Windows(super::windows::durability::WindowsJournalStorage),
}

#[cfg(any(test, windows))]
pub(crate) struct JournalStore {
    storage: JournalStorage,
    _writer_lock: Option<File>,
    log: Mutex<File>,
    /// A referenced artifact is part of the validated writer state. Keep the
    /// original no-external-write/no-delete handle, not just a remembered hash.
    /// Deduplication bounds OS handles; never evict a recovery prerequisite.
    dependencies: Mutex<BTreeMap<String, ProtectedManifest>>,
    dependency_limit: usize,
    writer: Option<WriterState>,
    trust: WriterTrust,
    limits: Limits,
    poisoned: AtomicBool,
    #[cfg(test)]
    replay_count: AtomicU64,
}

#[cfg(any(test, windows))]
impl JournalStore {
    /// Budget the complete private-copy slice before any destination mutation.
    /// This is logical record/dependency admission, not a free-space promise.
    #[cfg(windows)]
    pub(crate) fn plan_private_backup(
        &mut self,
        expected_generation: u64,
        effects: usize,
        source: &[u8],
    ) -> Result<(u64, String), SafeError> {
        self.check_writer_current()?;
        let effects = effects.max(1);
        let needed = effects
            .checked_mul(4)
            .and_then(|n| n.checked_add(8))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let recovery_dependencies = if state.journal.recovering {
            state.journal.recovery_dependency_reserve.max(128)
        } else {
            state
                .journal
                .recovery_dependency_reserve
                .max(128)
                .checked_add(
                    if state
                        .journal
                        .recovery_reservations
                        .contains_key(&sha256(source))
                    {
                        0
                    } else {
                        needed
                    },
                )
                .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?
        };
        let unavailable = recovery_dependencies;
        if state.journal.generation != expected_generation
            || effects > 100_000
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(needed + unavailable)
                .is_none_or(|n| n > self.dependency_limit)
        {
            return Err(error("HISTORY_DEPENDENCY_LIMIT"));
        }
        let records = (effects as u64)
            .checked_mul(3)
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let (used, reserved) = if state.journal.recovering {
            (
                state.usage.recovery_records,
                state.journal.capacity.recovery_records,
            )
        } else {
            (
                state.usage.forward_records,
                state.journal.capacity.forward_records,
            )
        };
        if used.checked_add(records).is_none_or(|n| n > reserved)
            || (!state.journal.recovering
                && state
                    .journal
                    .planned_backup_effects
                    .saturating_add(effects as u64)
                    .saturating_mul(3)
                    .saturating_add(32)
                    > state.journal.capacity.recovery_records)
            || state
                .usage
                .bytes
                .checked_add(records.saturating_mul(state.journal.capacity.record_byte_ceiling))
                .is_none_or(|n| n > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        let manifest = self.retain_manifest(source)?;
        let generation = self.append(
            expected_generation,
            JournalEvent::PrivateBackupPlan {
                manifest: manifest.clone(),
                effects: effects as u32,
                recovery_dependencies: recovery_dependencies as u32,
            },
        )?;
        Ok((generation, manifest))
    }
    #[cfg(windows)]
    pub(crate) fn verify_preinstall_return(&self) -> Result<(), SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        if !journal.preinstall_return_only
            || !matches!(
                journal.phase,
                JournalPhase::RecoveryRequired | JournalPhase::Restoring
            )
            || journal.effects.values().any(|effect| {
                matches!(
                    effect.spec.kind,
                    EffectKind::InstallerCreateSuspended
                        | EffectKind::InstallerResume
                        | EffectKind::InstallerTerminalOutcome
                        | EffectKind::HistoricalCreateSuspended
                        | EffectKind::HistoricalResume
                        | EffectKind::HistoricalTerminalOutcome
                        | EffectKind::VerifyTargetBundle
                        | EffectKind::ConfirmFirstLaunch
                )
            })
        {
            return Err(error("HISTORY_CONTEXT_RETURN_BLOCKED"));
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn admit_context_capacity(
        &self,
        generation: u64,
        effects: u64,
        artifacts: usize,
    ) -> Result<(), SafeError> {
        self.admit_context_capacity_lane(generation, effects, artifacts, false)
    }
    #[cfg(windows)]
    fn admit_context_capacity_lane(
        &self,
        generation: u64,
        effects: u64,
        artifacts: usize,
        force_recovery: bool,
    ) -> Result<(), SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let reserve = state.journal.recovery_dependency_reserve.max(128);
        let records = effects.saturating_mul(3).saturating_add(8);
        let (used, limit) = if force_recovery || state.journal.recovering {
            (
                state.usage.recovery_records,
                state.journal.capacity.recovery_records,
            )
        } else {
            (
                state.usage.forward_records,
                state.journal.capacity.forward_records,
            )
        };
        if generation != state.journal.generation
            || effects > 16
            || artifacts > 128
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(artifacts + reserve)
                .is_none_or(|n| n > self.dependency_limit)
            || used.saturating_add(records) > limit
            || state
                .usage
                .bytes
                .checked_add(records.saturating_mul(state.journal.capacity.record_byte_ceiling))
                .is_none_or(|n| n > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn admit_preinstall_return(
        &mut self,
        evidence: &super::windows::context::PreinstallReturnEvidence<'_>,
    ) -> Result<u64, SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        self.admit_context_capacity_lane(request.generation, 2, 16, true)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let roots = request
            .roots
            .iter()
            .map(|(root, bytes)| {
                self.retain_manifest_in_lane(bytes, true)
                    .map(|digest| (*root, digest))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let receipt = ContextReturnReceipt {
            schema: 1,
            anchor,
            roots: roots.clone(),
            pending: request.pending.clone(),
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        self.append_admitted(
            request.generation,
            JournalEvent::AdmitPreinstallReturn {
                roots,
                pending: request.pending,
                receipt,
            },
        )
    }
    #[cfg(windows)]
    pub(crate) fn confirm_context_root(
        &mut self,
        evidence: &super::windows::context::ContextRootEvidence<'_>,
    ) -> Result<u64, SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let current_manifest = self.retain_manifest_in_lane(&request.current, true)?;
        let saved = ContextRootReceipt {
            schema: 1,
            anchor,
            effect_id: request.effect_id.clone(),
            intent_generation: request.intent_generation,
            current_manifest: current_manifest.clone(),
            completed: request.completed,
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&saved).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        self.append_admitted(
            request.generation,
            JournalEvent::ConfirmContextRoot {
                effect_id: request.effect_id,
                intent_generation: request.intent_generation,
                current_manifest,
                completed: request.completed,
                receipt,
            },
        )
    }
    #[cfg(windows)]
    pub(crate) fn context_later_backup(
        &self,
        root: RootKind,
    ) -> Result<Option<(u64, LaterBackupPlan)>, SafeError> {
        self.check_writer_current()?;
        Ok(self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .later_backup_plans
            .get(&root)
            .cloned())
    }
    /// Validated immutable plan/receipt history. These selectors confer no
    /// filesystem authority; consumers must re-open actual backend-held slots.
    #[cfg(windows)]
    pub(crate) fn context_later_history(
        &self,
        root: RootKind,
    ) -> Result<Vec<LaterBackupRecord>, SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        let mut records = Vec::new();
        for (generation, plan) in journal
            .later_backup_history
            .iter()
            .filter(|(_, plan)| plan.root == root)
        {
            let mut applied = Vec::new();
            for effect in journal.effects.values() {
                let EffectKind::PrivateBackupEntry {
                    plan_generation,
                    manifest,
                    entry_index,
                    ..
                } = &effect.spec.kind
                else {
                    continue;
                };
                if plan_generation != generation || manifest != &plan.source_manifest {
                    continue;
                }
                let Some(result) = &effect.result else {
                    continue;
                };
                if result.observation != Observation::Applied {
                    continue;
                }
                let receipt = result
                    .receipt
                    .as_ref()
                    .ok_or_else(|| error("HISTORY_RECEIPT_INVALID"))?;
                let saved: EffectReceipt = serde_json::from_slice(&self.read_manifest(receipt)?)
                    .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || saved.transaction_id != journal.binding.transaction_id
                    || saved.effect_id != effect.spec.effect_id
                    || saved.intent_generation != effect.intent_generation
                    || saved.expected_postconditions != effect.spec.expected_postconditions
                    || saved.observation != Observation::Applied
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.read_manifest(&saved.observed_manifest)?;
                applied.push((*entry_index, saved.observed_manifest));
            }
            records.push(LaterBackupRecord {
                generation: *generation,
                plan: plan.clone(),
                complete: journal.later_complete.get(generation).cloned(),
                applied,
            });
        }
        Ok(records)
    }
    #[cfg(windows)]
    pub(crate) fn complete_later_backup(
        &mut self,
        evidence: &super::windows::context::LaterCompleteEvidence<'_>,
    ) -> Result<u64, SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        self.admit_context_capacity_lane(request.generation, 1, 4, true)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let copy_manifest = self.retain_manifest_in_lane(&request.copy, true)?;
        let saved = LaterCompleteReceipt {
            schema: 1,
            anchor,
            root: request.root,
            plan_generation: request.plan_generation,
            copy_manifest: copy_manifest.clone(),
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&saved).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        self.append_admitted(
            request.generation,
            JournalEvent::CompleteLaterBackup {
                root: request.root,
                plan_generation: request.plan_generation,
                copy_manifest,
                receipt,
            },
        )
    }
    #[cfg(windows)]
    pub(crate) fn prepare_later_backup(
        &mut self,
        evidence: &super::windows::context::LaterBackupEvidence<'_>,
    ) -> Result<(u64, String), SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let effects = request.effects.max(1);
        let needed = effects
            .checked_mul(4)
            .and_then(|count| count.checked_add(16))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let remaining = state
            .journal
            .recovery_dependency_reserve
            .max(128)
            .saturating_sub(
                state
                    .journal
                    .recovery_reservations
                    .get(&request.source_reservation)
                    .copied()
                    .unwrap_or(0),
            );
        let records = (effects as u64).saturating_mul(3).saturating_add(4);
        let other_records = state
            .journal
            .recovery_reservations
            .iter()
            .filter(|(source, _)| *source != &request.source_reservation)
            .map(|(_, count)| {
                ((count.saturating_sub(8) / 4) as u64)
                    .saturating_mul(3)
                    .saturating_add(4)
            })
            .sum::<u64>()
            .saturating_add(32);
        if effects > 100_000
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(needed + remaining)
                .is_none_or(|count| count > self.dependency_limit)
            || state
                .usage
                .recovery_records
                .saturating_add(records)
                .saturating_add(other_records)
                > state.journal.capacity.recovery_records
            || state
                .usage
                .bytes
                .checked_add(
                    records
                        .saturating_add(other_records)
                        .saturating_mul(state.journal.capacity.record_byte_ceiling),
                )
                .is_none_or(|bytes| bytes > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let source_manifest = self.retain_manifest_in_lane(&request.source, true)?;
        let destination = self.retain_manifest_in_lane(&request.destination, true)?;
        let previous_observation = request
            .previous
            .as_ref()
            .map(|bytes| self.retain_manifest_in_lane(bytes, true))
            .transpose()?;
        let plan = LaterBackupPlan {
            root: request.root,
            source_manifest: source_manifest.clone(),
            destination,
            source_reservation: request.source_reservation,
            previous_generation: request.previous_generation,
            previous_observation,
            abandoned_effect: request.abandoned_effect,
            effects: effects as u32,
            recovery_dependencies: remaining as u32,
        };
        let receipt = LaterBackupReceipt {
            schema: 1,
            anchor,
            plan: plan.clone(),
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        let generation = self.append_admitted(
            request.generation,
            JournalEvent::PrepareLaterBackup { plan, receipt },
        )?;
        Ok((generation, source_manifest))
    }
    #[cfg(windows)]
    pub(crate) fn context_preservation(
        &self,
        context: &str,
        root: RootKind,
    ) -> Result<(EffectSpec, u64), SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        let effect = journal
            .effects
            .values()
            .filter(|effect| {
                effect.spec.kind
                    == (EffectKind::PreserveRoot {
                        context: context.into(),
                        root,
                    })
                    && (effect
                        .result
                        .as_ref()
                        .is_some_and(|result| result.observation == Observation::Applied)
                        || journal
                            .confirmed_context_roots
                            .contains(&effect.spec.effect_id))
            })
            .max_by_key(|effect| effect.intent_generation)
            .ok_or_else(|| error("HISTORY_CONTEXT_CHANGED"))?;
        Ok((effect.spec.clone(), effect.intent_generation))
    }
    #[cfg(windows)]
    pub(crate) fn latest_bundle_seed(&self) -> Result<Option<(u64, String)>, SafeError> {
        self.check_writer_current()?;
        Ok(self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .bundle_seed
            .clone())
    }
    #[cfg(windows)]
    fn admit_bundle_copy_capacity(&self, generation: u64, effects: usize) -> Result<(), SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let reserve = state.journal.recovery_dependency_reserve.max(128);
        let artifacts = effects
            .checked_mul(4)
            .and_then(|count| count.checked_add(24))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let records = (effects as u64)
            .checked_mul(3)
            .and_then(|count| count.checked_add(8))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let other_records = state
            .journal
            .recovery_reservations
            .values()
            .map(|count| {
                ((count.saturating_sub(8) / 4) as u64)
                    .saturating_mul(3)
                    .saturating_add(4)
            })
            .sum::<u64>()
            .saturating_add(32);
        if generation != state.journal.generation
            || effects == 0
            || effects > 100_000
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(artifacts + reserve)
                .is_none_or(|count| count > self.dependency_limit)
            || state
                .usage
                .recovery_records
                .checked_add(records.saturating_add(other_records))
                .is_none_or(|count| count > state.journal.capacity.recovery_records)
            || records
                .saturating_add(other_records)
                .checked_mul(state.journal.capacity.record_byte_ceiling)
                .and_then(|bytes| state.usage.bytes.checked_add(bytes))
                .is_none_or(|bytes| bytes > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn admit_bundle_start(
        &mut self,
        evidence: &super::windows::context::bundle_restore::BundleStartEvidence<'_>,
    ) -> Result<(u64, String), SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        self.admit_bundle_copy_capacity(request.generation, request.effects)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let reserve = state.journal.recovery_dependency_reserve.max(128) as u32;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let seed = self.retain_manifest_in_lane(&request.seed, true)?;
        let current_manifest = self.retain_manifest_in_lane(&request.current, true)?;
        let saved = BundleStartReceipt {
            schema: 1,
            anchor,
            seed: seed.clone(),
            current_manifest: current_manifest.clone(),
            effects: request.effects as u32,
            recovery_dependencies: reserve,
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&saved).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        let generation = self.append_admitted(
            request.generation,
            JournalEvent::AdmitBundleStart {
                seed,
                current_manifest: current_manifest.clone(),
                effects: request.effects as u32,
                recovery_dependencies: reserve,
                receipt,
            },
        )?;
        Ok((generation, current_manifest))
    }
    #[cfg(windows)]
    pub(crate) fn latest_bundle_backup(
        &self,
    ) -> Result<Option<(u64, BundleBackupPlan)>, SafeError> {
        self.check_writer_current()?;
        Ok(self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .bundle_backup_plan
            .clone())
    }
    /// Read only the current bundle attempt's authenticated successful entry
    /// receipts. A missing ready-plan record cannot erase these known objects.
    #[cfg(windows)]
    pub(crate) fn bundle_copy_receipts(
        &self,
        generation: u64,
        manifest: &str,
    ) -> Result<Vec<(u32, String)>, SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        let active = journal
            .bundle_backup_plan
            .as_ref()
            .map(|(generation, _)| *generation)
            .or_else(|| {
                journal
                    .bundle_seed
                    .as_ref()
                    .map(|(generation, _)| *generation)
            });
        if active != Some(generation)
            || journal
                .backup_plans
                .get(&generation)
                .is_none_or(|(source, _)| source != manifest)
        {
            return Err(error("HISTORY_EFFECT_CHANGED"));
        }
        let mut entries = Vec::new();
        for effect in journal.effects.values() {
            let EffectKind::PrivateBackupEntry {
                plan_generation,
                manifest: source,
                entry_index,
                ..
            } = &effect.spec.kind
            else {
                continue;
            };
            if *plan_generation != generation || source != manifest {
                continue;
            }
            let Some(result) = &effect.result else {
                continue;
            };
            if result.observation != Observation::Applied {
                continue;
            }
            let digest = result
                .receipt
                .as_ref()
                .ok_or_else(|| error("HISTORY_RECEIPT_INVALID"))?;
            let receipt: EffectReceipt = serde_json::from_slice(&self.read_manifest(digest)?)
                .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
            if receipt.schema != 1
                || receipt.transaction_id != journal.binding.transaction_id
                || receipt.effect_id != effect.spec.effect_id
                || receipt.intent_generation != effect.intent_generation
                || receipt.expected_postconditions != effect.spec.expected_postconditions
                || receipt.observation != Observation::Applied
            {
                return Err(error("HISTORY_RECEIPT_INVALID"));
            }
            // The payload remains a protected dependency, not caller input.
            self.read_manifest(&receipt.observed_manifest)?;
            entries.push((*entry_index, receipt.observed_manifest));
        }
        Ok(entries)
    }
    #[cfg(windows)]
    pub(crate) fn prepare_bundle_backup(
        &mut self,
        evidence: &super::windows::context::bundle_restore::BundleRecoveryEvidence<'_>,
    ) -> Result<(u64, String), SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let effects = request.effects.max(1);
        let reserve = state.journal.recovery_dependency_reserve.max(128);
        self.admit_bundle_copy_capacity(request.generation, effects)?;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let current_manifest = self.retain_manifest_in_lane(&request.current, true)?;
        let destination = self.retain_manifest_in_lane(&request.destination, true)?;
        let previous_observation = self.retain_manifest_in_lane(&request.previous, true)?;
        let plan = BundleBackupPlan {
            current_manifest: current_manifest.clone(),
            destination,
            previous_observation,
            previous_generation: request.previous_generation,
            abandoned_effect: request.abandoned_effect,
            effects: effects as u32,
            recovery_dependencies: reserve as u32,
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&BundleBackupReceipt {
                schema: 1,
                anchor,
                plan: plan.clone(),
            })
            .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        let generation = self.append_admitted(
            request.generation,
            JournalEvent::PrepareBundleBackup { plan, receipt },
        )?;
        Ok((generation, current_manifest))
    }
    /// Read-only complete bundle effect budget. This does not confer any path,
    /// permission, process, or restoration authority and reserves no disk space.
    #[cfg(windows)]
    pub(crate) fn admit_bundle_capacity(
        &self,
        generation: u64,
        effects: u64,
        artifacts: usize,
    ) -> Result<(), SafeError> {
        self.admit_restore_capacity(generation, effects, artifacts)
    }
    /// Shared read-only recovery budgeting for complete bundle/registration/
    /// shortcut plans. Effect counts alone are never operation authority.
    #[cfg(windows)]
    pub(crate) fn admit_restore_capacity(
        &self,
        generation: u64,
        effects: u64,
        artifacts: usize,
    ) -> Result<(), SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let records = effects
            .checked_mul(3)
            .and_then(|count| count.checked_add(8))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let reserve = state.journal.recovery_dependency_reserve.max(128);
        let other_records = state
            .journal
            .recovery_reservations
            .values()
            .map(|count| {
                ((count.saturating_sub(8) / 4) as u64)
                    .saturating_mul(3)
                    .saturating_add(4)
            })
            .sum::<u64>()
            .saturating_add(32);
        if generation != state.journal.generation
            || state.journal.phase != JournalPhase::Restoring
            || state.journal.requires_reconciliation()
            || effects == 0
            || effects > 100_000
            || artifacts > self.dependency_limit
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(artifacts)
                .and_then(|count| count.checked_add(reserve))
                .is_none_or(|count| count > self.dependency_limit)
            || state
                .usage
                .recovery_records
                .checked_add(records.saturating_add(other_records))
                .is_none_or(|count| count > state.journal.capacity.recovery_records)
            || records
                .saturating_add(other_records)
                .checked_mul(state.journal.capacity.record_byte_ceiling)
                .and_then(|bytes| state.usage.bytes.checked_add(bytes))
                .is_none_or(|bytes| bytes > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn context_root_backup(
        &self,
        original: &str,
    ) -> Result<Option<(u64, RootBackupPlan)>, SafeError> {
        self.check_writer_current()?;
        Ok(self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .root_backup_plans
            .get(original)
            .cloned())
    }
    #[cfg(windows)]
    pub(crate) fn latest_context_root_effect(
        &self,
        kind: &EffectKind,
    ) -> Result<ContextRootOutcome, SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        if !matches!(kind, EffectKind::PreserveRoot { context, .. }
            if context == &journal.binding.target_context)
            && !matches!(kind, EffectKind::RestoreSourceRoot { .. })
        {
            return Err(error("HISTORY_CONTEXT_CHANGED"));
        }
        Ok(journal
            .effects
            .values()
            .filter(|effect| &effect.spec.kind == kind)
            .max_by_key(|effect| effect.intent_generation)
            .map(|effect| {
                let complete = effect
                    .result
                    .as_ref()
                    .is_some_and(|result| result.observation == Observation::Applied)
                    || journal
                        .confirmed_context_roots
                        .contains(&effect.spec.effect_id);
                (effect.spec.clone(), effect.intent_generation, complete)
            }))
    }
    #[cfg(windows)]
    pub(crate) fn context_pending(&self) -> Result<Option<(EffectSpec, u64)>, SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        Ok(journal.pending.as_ref().map(|id| {
            let effect = &journal.effects[id];
            (effect.spec.clone(), effect.intent_generation)
        }))
    }
    #[cfg(windows)]
    pub(crate) fn prepare_root_backup(
        &mut self,
        evidence: &super::windows::context::RootBackupEvidence<'_>,
    ) -> Result<(u64, String), SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let effects = request.effects.max(1);
        let needed = effects
            .checked_mul(4)
            .and_then(|n| n.checked_add(16))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let remaining = state
            .journal
            .recovery_dependency_reserve
            .max(128)
            .saturating_sub(
                state
                    .journal
                    .recovery_reservations
                    .get(&request.reservation_source)
                    .copied()
                    .unwrap_or(0),
            );
        let records = (effects as u64).saturating_mul(3).saturating_add(4);
        let other_records = state
            .journal
            .recovery_reservations
            .iter()
            .filter(|(source, _)| *source != &request.reservation_source)
            .map(|(_, dependencies)| {
                ((dependencies.saturating_sub(8) / 4) as u64)
                    .saturating_mul(3)
                    .saturating_add(4)
            })
            .sum::<u64>()
            .saturating_add(32);
        if effects > 100_000
            || self
                .dependencies
                .lock()
                .len()
                .checked_add(needed + remaining)
                .is_none_or(|n| n > self.dependency_limit)
            || state
                .usage
                .recovery_records
                .saturating_add(records)
                .saturating_add(other_records)
                > state.journal.capacity.recovery_records
            || state
                .usage
                .bytes
                .checked_add(
                    records
                        .saturating_add(other_records)
                        .saturating_mul(state.journal.capacity.record_byte_ceiling),
                )
                .is_none_or(|n| n > self.limits.bytes)
        {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let current_manifest = self.retain_manifest_in_lane(&request.current, true)?;
        let destination = self.retain_manifest_in_lane(&request.destination, true)?;
        let preserved_manifest = request
            .preserved
            .as_ref()
            .map(|bytes| self.retain_manifest_in_lane(bytes, true))
            .transpose()?;
        let plan = RootBackupPlan {
            original_effect_id: request.effect_id,
            original_intent_generation: request.intent_generation,
            current_manifest: current_manifest.clone(),
            destination,
            prior_plan_generation: request.prior_plan_generation,
            preserved_manifest,
            abandoned_effect: request.abandoned_effect,
            reservation_source: request.reservation_source,
            effects: effects as u32,
            recovery_dependencies: remaining as u32,
        };
        let receipt = RootBackupReceipt {
            schema: 1,
            anchor,
            plan: plan.clone(),
        };
        let receipt = self.retain_manifest_in_lane(
            &serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        let generation = self.append_admitted(
            request.generation,
            JournalEvent::PrepareRootBackup { plan, receipt },
        )?;
        Ok((generation, current_manifest))
    }
    #[cfg(windows)]
    pub(crate) fn context_rotation(&self, effect_id: &str) -> Result<(EffectSpec, u64), SafeError> {
        self.check_writer_current()?;
        let effect = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .effects
            .get(effect_id)
            .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
        Ok((effect.spec.clone(), effect.intent_generation))
    }
    #[cfg(windows)]
    pub(crate) fn context_inverse(&self, original: &str) -> Result<ContextInverse, SafeError> {
        self.check_writer_current()?;
        let state = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        let inverse = state.effects.values().find(|effect| matches!(&effect.spec.kind,
            EffectKind::ReverseSourceRoot { original_effect_id, .. } if original_effect_id == original))
            .map(|effect| (effect.spec.clone(), effect.intent_generation));
        Ok((
            state.admitted_root_reversals.get(original).cloned(),
            inverse,
        ))
    }
    #[cfg(windows)]
    pub(crate) fn admit_root_reverse(
        &mut self,
        evidence: &super::windows::context::RootRecoveryEvidence<'_>,
    ) -> Result<u64, SafeError> {
        let request = evidence.verify(self).map_err(storage_error)?;
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        let current_manifest = self.retain_manifest_in_lane(&request.current, true)?;
        let receipt = RootReverseReceipt {
            schema: 1,
            anchor,
            effect_id: request.effect_id.clone(),
            intent_generation: request.intent_generation,
            current_manifest: current_manifest.clone(),
            returned: request.returned,
        };
        let digest = self.retain_manifest_in_lane(
            &serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
            true,
        )?;
        evidence.verify(self).map_err(storage_error)?;
        let event = if request.returned {
            JournalEvent::ConfirmRootReturned {
                effect_id: request.effect_id,
                intent_generation: request.intent_generation,
                current_manifest,
                receipt: digest,
            }
        } else {
            JournalEvent::AdmitRootReverse {
                effect_id: request.effect_id,
                intent_generation: request.intent_generation,
                current_manifest,
                receipt: digest,
            }
        };
        self.append_admitted(request.generation, event)
    }
    /// Operation adapters bind to this live secured writer, not an inspection
    /// snapshot or a caller-supplied root digest. Recheck at each effect boundary.
    #[cfg(windows)]
    pub(crate) fn verify_windows_binding(
        &mut self,
        root: &super::windows::files::PrivateDirectory,
        binding: &JournalBinding,
        generation: u64,
    ) -> Result<(), SafeError> {
        self.check_writer_current()?;
        #[cfg(not(test))]
        let JournalStorage::Windows(storage) = &self.storage;
        #[cfg(test)]
        let storage = match &self.storage {
            JournalStorage::Windows(storage) => storage,
            JournalStorage::Fixture { .. } => return Err(error("HISTORY_PLATFORM_UNSUPPORTED")),
        };
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if !storage.matches_root(root) || &state.journal.binding != binding {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        if state.journal.generation != generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        Ok(())
    }
    /// Marker publication holds an exclusive borrow of this original writer;
    /// detached inspection data never supplies current/root publication authority.
    #[cfg(windows)]
    pub(super) fn validate_marker_publication(
        &mut self,
        root: &super::windows::files::PrivateDirectory,
        marker: &super::maintenance::ActiveContextMarker,
    ) -> Result<(), SafeError> {
        self.check_writer_current()?;
        #[cfg(not(test))]
        let JournalStorage::Windows(storage) = &self.storage;
        #[cfg(test)]
        let storage = match &self.storage {
            JournalStorage::Windows(storage) => storage,
            JournalStorage::Fixture { .. } => return Err(error("HISTORY_PLATFORM_UNSUPPORTED")),
        };
        if !storage.matches_root(root) {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        marker.validate_checkpoint(&state.journal, &state.head)
    }
    #[cfg(windows)]
    pub(super) fn validate_marker_successor(
        &mut self,
        root: &super::windows::files::PrivateDirectory,
        marker: &super::maintenance::ActiveContextMarker,
    ) -> Result<(), SafeError> {
        self.validate_marker_publication(root, marker)?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if state.journal.phase != JournalPhase::Reviewed || state.journal.requires_reconciliation()
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        Ok(())
    }
    fn validate_storage_transaction(&self, binding: &JournalBinding) -> Result<(), SafeError> {
        #[cfg(not(windows))]
        let _ = binding;
        match &self.storage {
            #[cfg(windows)]
            JournalStorage::Windows(storage)
                if !storage.matches_transaction(&binding.transaction_id) =>
            {
                Err(error("HISTORY_TRANSACTION_CHANGED"))
            }
            _ => Ok(()),
        }
    }
    /// The held private root is storage authority only. Scope/exclusion,
    /// snapshot and installation admission remain separate coordinator proofs.
    #[cfg(windows)]
    pub(crate) fn open_windows(
        root: std::sync::Arc<super::windows::files::PrivateDirectory>,
    ) -> Result<Self, SafeError> {
        let storage =
            super::windows::durability::WindowsJournalStorage::open(root).map_err(storage_error)?;
        Self::from_windows_storage(storage)
    }
    #[cfg(windows)]
    pub(crate) fn create_windows_transaction(
        root: std::sync::Arc<super::windows::files::PrivateDirectory>,
        transaction_id: &str,
    ) -> Result<Self, SafeError> {
        Self::from_windows_storage(
            super::windows::durability::WindowsJournalStorage::transaction(
                root,
                transaction_id,
                true,
            )
            .map_err(storage_error)?,
        )
    }
    /// Missing recovery logs remain missing. Unlike the legacy storage-only
    /// primitive this path never creates a new empty journal while inspecting.
    #[cfg(windows)]
    pub(crate) fn open_windows_transaction(
        root: std::sync::Arc<super::windows::files::PrivateDirectory>,
        transaction_id: &str,
    ) -> Result<Self, SafeError> {
        Self::from_windows_storage(
            super::windows::durability::WindowsJournalStorage::transaction(
                root,
                transaction_id,
                false,
            )
            .map_err(storage_error)?,
        )
    }
    #[cfg(windows)]
    fn from_windows_storage(
        storage: super::windows::durability::WindowsJournalStorage,
    ) -> Result<Self, SafeError> {
        let log = storage.log_file().map_err(storage_error)?;
        regular_file(&log)?;
        Ok(Self {
            storage: JournalStorage::Windows(storage),
            _writer_lock: None, // The log's original no-share-write handle excludes another writer.
            log: Mutex::new(log),
            writer: None,
            dependencies: Mutex::new(BTreeMap::new()),
            dependency_limit: MAX_DEPENDENCY_HANDLES,
            trust: WriterTrust::HeldWindowsHandle,
            limits: Limits::default(),
            poisoned: AtomicBool::new(false),
            #[cfg(test)]
            replay_count: AtomicU64::new(0),
        })
    }
    #[cfg(test)]
    fn open_parts(
        directory: Dir,
        durability: Box<dyn DirectoryDurability>,
        limits: Limits,
        trust: WriterTrust,
    ) -> Result<Self, SafeError> {
        let mut options = file_options(true);
        options.read(true).write(true).create(true);
        let file = directory
            .open_with("journal.lock", &options)
            .map_err(storage_error)?
            .into_std();
        regular_file(&file)?;
        file.try_lock()
            .map_err(|_| error("HISTORY_TRANSACTION_BUSY"))?;
        let mut log_options = file_options(false);
        log_options.read(true).write(true).create(true);
        // Never truncate or replace this file. Previous immutable frames and a
        // possible torn tail survive all failures. Closing/reopening is recovery.
        let log = directory
            .open_with("journal.log", &log_options)
            .map_err(storage_error)?
            .into_std();
        regular_file(&log)?;
        Ok(Self {
            storage: JournalStorage::Fixture {
                directory,
                durability,
            },
            _writer_lock: Some(file),
            log: Mutex::new(log),
            writer: None,
            dependencies: Mutex::new(BTreeMap::new()),
            dependency_limit: MAX_DEPENDENCY_HANDLES,
            trust,
            limits,
            poisoned: AtomicBool::new(false),
            #[cfg(test)]
            replay_count: AtomicU64::new(0),
        })
    }

    /// The complete reviewed operation budget is durable in genesis before ANY
    /// intent. Reserve return/recovery separately; forward work cannot spend it.
    pub(crate) fn initialize(
        &mut self,
        binding: JournalBinding,
        capacity: CapacityPlan,
    ) -> Result<(), SafeError> {
        self.healthy()?;
        self.validate_storage_transaction(&binding)?;
        if self.writer.is_some()
            || !self.namespace_valid()?
            || self.log.lock().metadata().map_err(storage_error)?.len() != 0
        {
            return Err(error("HISTORY_JOURNAL_EXISTS"));
        }
        let journal = SwitchJournal::new(binding.clone(), capacity.clone())?;
        let record = Envelope::new(Record {
            schema: 2,
            binding,
            generation: 0,
            previous: None,
            lane: None,
            event: JournalEvent::Begin { capacity },
        })?;
        let bytes = record.encode()?;
        journal.capacity.validate(self.limits, bytes.len() as u64)?;
        let identity = regular_file(&self.log.lock())?;
        self.write_frame(0, &bytes)?;
        let mut hash = Sha256::new();
        hash.update(&bytes);
        self.writer = Some(WriterState {
            journal,
            head: record.digest,
            identity,
            hash,
            usage: Usage {
                records: 1,
                bytes: bytes.len() as u64,
                ..Usage::default()
            },
        });
        Ok(())
    }
    pub(crate) fn bind_existing(&mut self, binding: &JournalBinding) -> Result<(), SafeError> {
        self.healthy()?;
        self.validate_storage_transaction(binding)?;
        let read = self.inspect(binding)?;
        if read.blocked {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let journal = read
            .last_valid
            .ok_or_else(|| error("HISTORY_JOURNAL_INVALID"))?;
        let head = read.head.ok_or_else(|| error("HISTORY_JOURNAL_INVALID"))?;
        self.writer = Some(WriterState {
            journal,
            head,
            usage: read.usage,
            hash: read.hash,
            identity: read.identity,
        });
        Ok(())
    }
    pub(crate) fn append(
        &mut self,
        expected_generation: u64,
        event: JournalEvent,
    ) -> Result<u64, SafeError> {
        if matches!(
            event,
            JournalEvent::AbortPreContext { .. }
                | JournalEvent::CompensateUnknown { .. }
                | JournalEvent::AdmitRootReverse { .. }
                | JournalEvent::ConfirmRootReturned { .. }
                | JournalEvent::PrepareRootBackup { .. }
                | JournalEvent::AdmitPreinstallReturn { .. }
                | JournalEvent::ConfirmContextRoot { .. }
                | JournalEvent::PrepareLaterBackup { .. }
                | JournalEvent::CompleteLaterBackup { .. }
                | JournalEvent::PrepareBundleBackup { .. }
                | JournalEvent::AdmitBundleStart { .. }
        ) {
            return Err(error("HISTORY_LIVE_EVIDENCE_REQUIRED"));
        }
        self.append_admitted(expected_generation, event)
    }
    #[cfg(windows)]
    pub(crate) fn admit_private_abort<'a>(
        &mut self,
        evidence: super::windows::pre_context_abort::PrivateAbortEvidence<'a>,
    ) -> Result<PreContextAbortProof<'a>, SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if evidence.binding() != state.journal.binding() || !state.journal.can_abort_private_only()
        {
            return Err(error("HISTORY_EARLY_ABORT_BLOCKED"));
        }
        let anchor = AdmissionAnchor {
            binding: state.journal.binding.clone(),
            generation: state.journal.generation,
            head: state.head.clone(),
            journal_identity: state.identity.clone(),
        };
        evidence.verify_writer(self, anchor.generation)?;
        let [bundle, roots, registration, quiescence] = evidence.observations()?;
        let unchanged = UnchangedSourceObservations {
            bundle: self.retain_manifest_in_lane(&bundle, true)?,
            roots: self.retain_manifest_in_lane(&roots, true)?,
            registration: self.retain_manifest_in_lane(&registration, true)?,
            // This artifact explicitly records live original ownership and a
            // frozen empty ledger; it never claims browser shutdown or M0.
            quiescence: self.retain_manifest_in_lane(&quiescence, true)?,
        };
        evidence.verify_writer(self, anchor.generation)?;
        Ok(PreContextAbortProof {
            anchor,
            unchanged,
            guards: Box::new(evidence),
        })
    }
    pub(crate) fn abort_pre_context(
        &mut self,
        proof: &PreContextAbortProof<'_>,
    ) -> Result<u64, SafeError> {
        self.check_admission_anchor(&proof.anchor)?;
        proof.guards.verify()?;
        let receipt = AbortReceipt {
            schema: 1,
            anchor: proof.anchor.clone(),
            unchanged: proof.unchanged.clone(),
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
        let event = JournalEvent::AbortPreContext {
            receipt: sha256(&bytes),
        };
        self.writer
            .as_ref()
            .expect("bound writer")
            .journal
            .validate_event(&event)?;
        for digest in proof.unchanged.digests() {
            self.protect_manifest(digest)?;
        }
        self.retain_manifest_in_lane(&bytes, true)?;
        let generation = self.append_admitted(proof.anchor.generation, event)?;
        proof.guards.verify()?;
        Ok(generation)
    }
    pub(crate) fn compensate_unknown(
        &mut self,
        proof: &UnknownCompensationProof,
    ) -> Result<u64, SafeError> {
        self.check_admission_anchor(&proof.anchor)?;
        let receipt = CompensationReceipt {
            schema: 1,
            anchor: proof.anchor.clone(),
            effect_id: proof.effect_id.clone(),
            intent_generation: proof.intent_generation,
            current: proof.current.clone(),
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
        let event = JournalEvent::CompensateUnknown {
            effect_id: proof.effect_id.clone(),
            intent_generation: proof.intent_generation,
            current_roots: proof.current.roots.clone(),
            receipt: sha256(&bytes),
        };
        self.writer
            .as_ref()
            .expect("bound writer")
            .journal
            .validate_event(&event)?;
        for digest in proof.current.digests() {
            self.protect_manifest(digest)?;
        }
        self.retain_manifest_in_lane(&bytes, true)?;
        self.append_admitted(proof.anchor.generation, event)
    }
    fn check_admission_anchor(&self, anchor: &AdmissionAnchor) -> Result<(), SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if !anchor.matches(&state.journal, &state.head, &state.identity) {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        Ok(())
    }
    fn append_admitted(
        &mut self,
        expected_generation: u64,
        event: JournalEvent,
    ) -> Result<u64, SafeError> {
        self.check_writer_current()?;
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if state.journal.generation != expected_generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        state.journal.validate_event(&event)?;
        self.validate_artifacts(&state.journal, &event, &state.head, &state.identity)?;
        let lane = state.journal.lane(&event);
        let record = Envelope::new(Record {
            schema: 2,
            binding: state.journal.binding.clone(),
            generation: expected_generation + 1,
            previous: Some(state.head.clone()),
            lane: Some(lane),
            event: event.clone(),
        })?;
        let bytes = record.encode()?;
        // Prospective total AND reserved-lane checks precede every disk write.
        state.usage.check(
            &state.journal.capacity,
            lane,
            bytes.len() as u64,
            self.limits,
        )?;
        self.write_frame(state.usage.bytes, &bytes)?;
        let state = self
            .writer
            .as_mut()
            .expect("writer retained across exclusive append");
        state.journal.commit_event(event);
        state.head = record.digest;
        state.hash.update(&bytes);
        state.usage.commit(lane, bytes.len() as u64);
        Ok(state.journal.generation)
    }
    fn healthy(&self) -> Result<(), SafeError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        Ok(())
    }
    fn check_writer_current(&self) -> Result<(), SafeError> {
        self.healthy()?;
        let result = self
            .check_writer_object()
            .and_then(|_| self.check_protected_dependencies());
        if result.is_err() {
            self.poisoned.store(true, Ordering::SeqCst);
        }
        result
    }
    fn check_writer_object(&self) -> Result<(), SafeError> {
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        #[allow(unused_mut)] // Only non-production fixture verification reads bytes.
        let mut file = self.log.lock();
        if regular_file(&file)? != state.identity
            || file.metadata().map_err(storage_error)?.len() != state.usage.bytes
        {
            return Err(error("HISTORY_JOURNAL_CHANGED"));
        }
        // Reopen for identity observation only. Share-write admits OUR already
        // held writer, not another writer: the original handle denies that open.
        match &self.storage {
            #[cfg(test)]
            JournalStorage::Fixture { directory, .. } => {
                let mut options = file_options(true);
                options.read(true);
                let named = directory
                    .open_with("journal.log", &options)
                    .map_err(storage_error)?
                    .into_std();
                if regular_file(&named)? != state.identity {
                    return Err(error("HISTORY_JOURNAL_CHANGED"));
                }
            }
            #[cfg(windows)]
            JournalStorage::Windows(storage) => storage.verify().map_err(storage_error)?,
        }
        match self.trust {
            #[cfg(windows)]
            WriterTrust::HeldWindowsHandle => (),
            WriterTrust::FixtureFullHash => {
                file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
                let mut hash = Sha256::new();
                let mut total = 0u64;
                let mut buffer = [0u8; 65536];
                loop {
                    let count = file.read(&mut buffer).map_err(storage_error)?;
                    if count == 0 {
                        break;
                    }
                    total += count as u64;
                    if total > self.limits.bytes {
                        return Err(error("HISTORY_JOURNAL_CHANGED"));
                    }
                    hash.update(&buffer[..count]);
                }
                if total != state.usage.bytes || hash.finalize() != state.hash.clone().finalize() {
                    return Err(error("HISTORY_JOURNAL_CHANGED"));
                }
            }
        }
        Ok(())
    }
    fn check_protected_dependencies(&self) -> Result<(), SafeError> {
        match self.trust {
            // Exact held handles deny external writes/deletes for every
            // cached dependency, including receipt wrappers and observed data.
            #[cfg(windows)]
            WriterTrust::HeldWindowsHandle => {
                for held in self.dependencies.lock().values() {
                    if let Some(artifact) = &held.windows {
                        artifact.lock().verify().map_err(storage_error)?;
                    }
                }
                Ok(())
            }
            WriterTrust::FixtureFullHash => {
                let mut dependencies = self.dependencies.lock();
                for (digest, held) in dependencies.iter_mut() {
                    self.read_protected_manifest(digest, held)?;
                }
                Ok(())
            }
        }
    }
    fn write_frame(&self, expected_length: u64, bytes: &[u8]) -> Result<(), SafeError> {
        let result = (|| {
            #[allow(unused_mut)]
            let mut file = self.log.lock();
            if file.metadata().map_err(storage_error)?.len() != expected_length {
                return Err(error("HISTORY_JOURNAL_CHANGED"));
            }
            match &self.storage {
                #[cfg(test)]
                JournalStorage::Fixture {
                    directory,
                    durability,
                } => {
                    file.seek(SeekFrom::Start(expected_length))
                        .map_err(storage_error)?;
                    file.write_all(bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(storage_error)?;
                    durability.sync_directory(directory).map_err(storage_error)
                }
                #[cfg(windows)]
                JournalStorage::Windows(storage) => storage
                    .append(expected_length, bytes)
                    .map_err(storage_error),
            }
        })();
        if result.is_err() {
            self.poisoned.store(true, Ordering::SeqCst);
        }
        result
    }

    /// Called after an actual observation. Persist the typed observed manifest
    /// first, then this bound receipt, then the Observed record. A crash at any
    /// boundary leaves the original intent unresolved; it never authorizes replay.
    pub(crate) fn retain_effect_receipt(
        &self,
        effect_id: &str,
        observation: Observation,
        observed_manifest: &str,
    ) -> Result<String, SafeError> {
        self.check_writer_current()?;
        let journal = &self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal;
        let effect = journal
            .effects
            .get(effect_id)
            .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
        if journal.pending.as_deref() != Some(effect_id)
            || observation == Observation::Unknown
            || (observation == Observation::NotApplied && observed_manifest != effect.spec.before)
        {
            return Err(error("HISTORY_EFFECT_CHANGED"));
        }
        self.read_manifest(observed_manifest)?;
        let receipt = EffectReceipt {
            schema: 1,
            transaction_id: journal.binding.transaction_id.clone(),
            effect_id: effect_id.into(),
            intent_generation: effect.intent_generation,
            expected_postconditions: effect.spec.expected_postconditions.clone(),
            observation,
            observed_manifest: observed_manifest.into(),
        };
        self.retain_manifest(
            &serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?,
        )
    }
    fn validate_artifacts(
        &self,
        journal: &SwitchJournal,
        event: &JournalEvent,
        head: &str,
        identity: &str,
    ) -> Result<(), SafeError> {
        match event {
            JournalEvent::PrivateBackupPlan {
                manifest: digest, ..
            }
            | JournalEvent::Manifest { digest, .. } => {
                self.protect_manifest(digest)?;
            }
            JournalEvent::Intent { effect } => {
                self.protect_manifest(&effect.before)?;
                self.protect_manifest(&effect.expected_postconditions)?;
                if let Some(manifest) = effect.entry_manifest() {
                    self.protect_manifest(manifest)?;
                }
                if let EffectKind::ReverseSourceFence {
                    original_effect_id, ..
                } = &effect.kind
                {
                    let original = journal
                        .effects
                        .get(original_effect_id)
                        .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    let digest = original
                        .result
                        .as_ref()
                        .and_then(|result| result.receipt.as_ref())
                        .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    let receipt: EffectReceipt =
                        serde_json::from_slice(&self.protect_manifest(digest)?)
                            .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                    if effect.before != receipt.observed_manifest {
                        return Err(error("HISTORY_EFFECT_CHANGED"));
                    }
                }
            }
            JournalEvent::AdmitBundleStart {
                seed,
                current_manifest,
                effects,
                recovery_dependencies,
                receipt,
            } => {
                let saved: BundleStartReceipt =
                    serde_json::from_slice(&self.protect_manifest(receipt)?)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.seed != seed
                    || &saved.current_manifest != current_manifest
                    || &saved.effects != effects
                    || &saved.recovery_dependencies != recovery_dependencies
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.protect_manifest(seed)?;
                self.protect_manifest(current_manifest)?;
            }
            JournalEvent::PrepareBundleBackup { plan, receipt } => {
                let saved: BundleBackupReceipt =
                    serde_json::from_slice(&self.protect_manifest(receipt)?)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.plan != plan
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                for digest in [
                    &plan.current_manifest,
                    &plan.destination,
                    &plan.previous_observation,
                ] {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::PrepareLaterBackup { plan, receipt } => {
                let bytes = self.protect_manifest(receipt)?;
                if bytes.len() > 16384 {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                let saved: LaterBackupReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.plan != plan
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                for digest in [
                    &plan.source_manifest,
                    &plan.destination,
                    &plan.source_reservation,
                ] {
                    self.protect_manifest(digest)?;
                }
                if let Some(digest) = &plan.previous_observation {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::CompleteLaterBackup {
                root,
                plan_generation,
                copy_manifest,
                receipt,
            } => {
                let saved: LaterCompleteReceipt =
                    serde_json::from_slice(&self.protect_manifest(receipt)?)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || saved.root != *root
                    || saved.plan_generation != *plan_generation
                    || &saved.copy_manifest != copy_manifest
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.protect_manifest(copy_manifest)?;
            }
            JournalEvent::AdmitPreinstallReturn {
                roots,
                pending,
                receipt,
            } => {
                let saved: ContextReturnReceipt =
                    serde_json::from_slice(&self.protect_manifest(receipt)?)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.roots != roots
                    || &saved.pending != pending
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                for digest in roots.values() {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::ConfirmContextRoot {
                effect_id,
                intent_generation,
                current_manifest,
                completed,
                receipt,
            } => {
                let saved: ContextRootReceipt =
                    serde_json::from_slice(&self.protect_manifest(receipt)?)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.effect_id != effect_id
                    || saved.intent_generation != *intent_generation
                    || &saved.current_manifest != current_manifest
                    || saved.completed != *completed
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.protect_manifest(current_manifest)?;
            }
            JournalEvent::PrepareRootBackup { plan, receipt } => {
                let bytes = self.protect_manifest(receipt)?;
                if bytes.len() > 16384 {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                let saved: RootBackupReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if saved.schema != 1
                    || !saved.anchor.matches(journal, head, identity)
                    || &saved.plan != plan
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.protect_manifest(&plan.current_manifest)?;
                self.protect_manifest(&plan.destination)?;
                self.protect_manifest(&plan.reservation_source)?;
                if let Some(digest) = &plan.preserved_manifest {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::ConfirmRootReturned {
                effect_id,
                intent_generation,
                current_manifest,
                receipt,
            }
            | JournalEvent::AdmitRootReverse {
                effect_id,
                intent_generation,
                current_manifest,
                receipt,
            } => {
                let bytes = self.protect_manifest(receipt)?;
                if bytes.len() > 16384 {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                let receipt: RootReverseReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if receipt.schema != 1
                    || receipt.returned != matches!(event, JournalEvent::ConfirmRootReturned { .. })
                    || !receipt.anchor.matches(journal, head, identity)
                    || &receipt.effect_id != effect_id
                    || receipt.intent_generation != *intent_generation
                    || &receipt.current_manifest != current_manifest
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                self.protect_manifest(current_manifest)?;
            }
            JournalEvent::AbortPreContext { receipt } => {
                let bytes = self.protect_manifest(receipt)?;
                if bytes.len() > 16384 {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                let receipt: AbortReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if receipt.schema != 1 || !receipt.anchor.matches(journal, head, identity) {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                for digest in receipt.unchanged.digests() {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::CompensateUnknown {
                effect_id,
                intent_generation,
                current_roots,
                receipt,
            } => {
                let bytes = self.protect_manifest(receipt)?;
                if bytes.len() > 16384 {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                let receipt: CompensationReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                if receipt.schema != 1
                    || !receipt.anchor.matches(journal, head, identity)
                    || &receipt.effect_id != effect_id
                    || receipt.intent_generation != *intent_generation
                    || &receipt.current.roots != current_roots
                {
                    return Err(error("HISTORY_RECEIPT_INVALID"));
                }
                for digest in receipt.current.digests() {
                    self.protect_manifest(digest)?;
                }
            }
            JournalEvent::Observed {
                effect_id,
                intent_generation,
                result,
            } => {
                if let Some(digest) = &result.receipt {
                    let bytes = self.protect_manifest(digest)?;
                    if bytes.len() > 16384 {
                        return Err(error("HISTORY_RECEIPT_INVALID"));
                    }
                    let receipt: EffectReceipt = serde_json::from_slice(&bytes)
                        .map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                    let effect = journal
                        .effects
                        .get(effect_id)
                        .ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    if receipt.schema != 1
                        || receipt.transaction_id != journal.binding.transaction_id
                        || receipt.effect_id != *effect_id
                        || receipt.intent_generation != *intent_generation
                        || receipt.intent_generation != effect.intent_generation
                        || receipt.expected_postconditions != effect.spec.expected_postconditions
                        || receipt.observation != result.observation
                        || receipt.observation == Observation::Unknown
                        || (receipt.observation == Observation::NotApplied
                            && receipt.observed_manifest != effect.spec.before)
                    {
                        return Err(error("HISTORY_RECEIPT_INVALID"));
                    }
                    self.protect_manifest(&receipt.observed_manifest)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    #[cfg(test)]
    fn create_immutable(
        &self,
        directory: &Dir,
        durability: &dyn DirectoryDurability,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), SafeError> {
        let result = (|| {
            let mut options = file_options(false);
            options.write(true).create_new(true);
            let mut file = directory
                .open_with(name, &options)
                .map_err(storage_error)?
                .into_std();
            regular_file(&file)?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(storage_error)?;
            durability.sync_directory(directory).map_err(storage_error)
        })();
        if result.is_err() {
            self.poisoned.store(true, Ordering::SeqCst);
        }
        result
    }
    pub(crate) fn retain_manifest(&self, bytes: &[u8]) -> Result<String, SafeError> {
        self.retain_manifest_in_lane(bytes, false)
    }
    fn retain_manifest_in_lane(&self, bytes: &[u8], recovery: bool) -> Result<String, SafeError> {
        #[cfg(not(windows))]
        let _ = recovery;
        self.healthy()?;
        if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
            return Err(error("HISTORY_MANIFEST_INVALID"));
        }
        let digest = sha256(bytes);
        match &self.storage {
            #[cfg(test)]
            JournalStorage::Fixture {
                directory,
                durability,
            } => {
                let name = format!("manifest-{digest}.json");
                if directory.symlink_metadata(&name).is_ok() {
                    if self.read_manifest(&digest)? == bytes {
                        return Ok(digest);
                    }
                    return Err(error("HISTORY_MANIFEST_CHANGED"));
                }
                self.create_immutable(directory, durability.as_ref(), &name, bytes)?;
            }
            #[cfg(windows)]
            JournalStorage::Windows(storage) => {
                let mut dependencies = self.dependencies.lock();
                if let Some(held) = dependencies.get_mut(&digest) {
                    if self.read_protected_manifest(&digest, held)? == bytes {
                        return Ok(digest);
                    }
                    return Err(error("HISTORY_MANIFEST_CHANGED"));
                }
                let reserve = self
                    .writer
                    .as_ref()
                    .filter(|state| !recovery && !state.journal.recovering)
                    .map_or(0, |state| state.journal.recovery_dependency_reserve);
                if dependencies.len() >= self.dependency_limit.saturating_sub(reserve) {
                    return Err(error("HISTORY_DEPENDENCY_LIMIT"));
                }
                let artifact = match storage.open_artifact(&digest) {
                    Ok(artifact) => artifact,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        match storage.create_artifact(&digest, bytes) {
                            Ok(artifact) => artifact,
                            Err(failure) => {
                                self.poisoned.store(true, Ordering::SeqCst);
                                return Err(storage_error(failure));
                            }
                        }
                    }
                    Err(failure) => return Err(storage_error(failure)),
                };
                if artifact.read().map_err(storage_error)? != bytes {
                    self.poisoned.store(true, Ordering::SeqCst);
                    return Err(error("HISTORY_MANIFEST_CHANGED"));
                }
                dependencies.insert(
                    digest.clone(),
                    ProtectedManifest {
                        #[cfg(test)]
                        fixture: None,
                        windows: Some(std::sync::Arc::new(Mutex::new(artifact))),
                    },
                );
            }
        }
        Ok(digest)
    }
    pub(crate) fn read_manifest(&self, digest: &str) -> Result<Vec<u8>, SafeError> {
        validate_digest(digest)?;
        let mut dependencies = self.dependencies.lock();
        if let Some(held) = dependencies.get_mut(digest) {
            return self.read_protected_manifest(digest, held);
        }
        drop(dependencies);
        let mut held = self.open_manifest(digest)?;
        self.read_protected_manifest(digest, &mut held)
    }
    #[cfg(windows)]
    pub(crate) fn retain_role_guard(
        &mut self,
        root: std::sync::Arc<super::windows::files::PrivateDirectory>,
        binding: &JournalBinding,
        generation: u64,
        role: ManifestRole,
    ) -> Result<RetainedRoleGuard, SafeError> {
        self.verify_windows_binding(&root, binding, generation)?;
        let digest = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?
            .journal
            .manifest(role)
            .ok_or_else(|| error("HISTORY_MANIFEST_INVALID"))?
            .to_owned();
        self.protect_manifest(&digest)?;
        let artifact = self
            .dependencies
            .lock()
            .get(&digest)
            .and_then(|held| held.windows.as_ref())
            .cloned()
            .ok_or_else(|| error("HISTORY_MANIFEST_CHANGED"))?;
        let retained = RetainedRoleGuard {
            binding: binding.clone(),
            role,
            root: root.clone(),
            artifact,
        };
        retained.verify_role(binding, role, &root)?;
        self.verify_windows_binding(&root, binding, generation)?;
        Ok(retained)
    }
    /// Acquire and hash-validate before the first frame that references this
    /// artifact. Existing dependencies share one retained handle per digest.
    fn protect_manifest(&self, digest: &str) -> Result<Vec<u8>, SafeError> {
        validate_digest(digest)?;
        let mut dependencies = self.dependencies.lock();
        if let Some(held) = dependencies.get_mut(digest) {
            return self.read_protected_manifest(digest, held);
        }
        if dependencies.len() >= self.dependency_limit {
            return Err(error("HISTORY_DEPENDENCY_LIMIT"));
        }
        let mut held = self.open_manifest(digest)?;
        let bytes = self.read_protected_manifest(digest, &mut held)?;
        dependencies.insert(digest.into(), held);
        Ok(bytes)
    }
    fn open_manifest(&self, digest: &str) -> Result<ProtectedManifest, SafeError> {
        match &self.storage {
            #[cfg(windows)]
            JournalStorage::Windows(storage) => Ok(ProtectedManifest {
                #[cfg(test)]
                fixture: None,
                windows: Some(std::sync::Arc::new(Mutex::new(
                    storage.open_artifact(digest).map_err(storage_error)?,
                ))),
            }),
            #[cfg(test)]
            JournalStorage::Fixture { directory, .. } => {
                let mut options = file_options(false);
                options.read(true);
                let file = directory
                    .open_with(format!("manifest-{digest}.json"), &options)
                    .map_err(storage_error)?
                    .into_std();
                let identity = regular_file(&file)?;
                let length = file.metadata().map_err(storage_error)?.len();
                if length > MAX_MANIFEST_BYTES as u64 {
                    return Err(error("HISTORY_MANIFEST_INVALID"));
                }
                Ok(ProtectedManifest {
                    fixture: Some(FixtureManifest {
                        file,
                        identity,
                        length,
                    }),
                    #[cfg(windows)]
                    windows: None,
                })
            }
        }
    }
    fn read_protected_manifest(
        &self,
        digest: &str,
        held: &mut ProtectedManifest,
    ) -> Result<Vec<u8>, SafeError> {
        #[cfg(windows)]
        if let Some(artifact) = &held.windows {
            return artifact.lock().read().map_err(storage_error);
        }
        #[cfg(test)]
        {
            let directory = match &self.storage {
                JournalStorage::Fixture { directory, .. } => directory,
                #[cfg(windows)]
                JournalStorage::Windows(_) => return Err(error("HISTORY_MANIFEST_CHANGED")),
            };
            let held = held
                .fixture
                .as_mut()
                .ok_or_else(|| error("HISTORY_MANIFEST_CHANGED"))?;
            if regular_file(&held.file)? != held.identity
                || held.file.metadata().map_err(storage_error)?.len() != held.length
            {
                return Err(error("HISTORY_MANIFEST_CHANGED"));
            }
            let mut options = file_options(false);
            options.read(true);
            let named = directory
                .open_with(format!("manifest-{digest}.json"), &options)
                .map_err(storage_error)?
                .into_std();
            if regular_file(&named)? != held.identity {
                return Err(error("HISTORY_MANIFEST_CHANGED"));
            }
            held.file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
            let mut bytes = Vec::new();
            (&mut held.file)
                .take(MAX_MANIFEST_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(storage_error)?;
            if bytes.len() as u64 != held.length
                || sha256(&bytes) != digest
                || regular_file(&held.file)? != held.identity
            {
                return Err(error("HISTORY_MANIFEST_CHANGED"));
            }
            Ok(bytes)
        }
        #[cfg(not(test))]
        {
            let _ = (digest, held);
            Err(error("HISTORY_PLATFORM_UNSUPPORTED"))
        }
    }
    fn namespace_valid(&self) -> Result<bool, SafeError> {
        match &self.storage {
            #[cfg(windows)]
            JournalStorage::Windows(storage) => storage.namespace_valid().map_err(storage_error),
            #[cfg(test)]
            JournalStorage::Fixture { directory, .. } => {
                for (index, entry) in directory.entries().map_err(storage_error)?.enumerate() {
                    if index > MAX_RECORDS * 4 {
                        return Ok(false);
                    }
                    let entry = entry.map_err(storage_error)?;
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                        return Ok(false);
                    };
                    if name == "journal.lock" || name == "journal.log" {
                        continue;
                    }
                    let kind = entry.file_type().map_err(storage_error)?;
                    if !kind.is_file() || kind.is_symlink() {
                        return Ok(false);
                    }
                    let Some(digest) = name
                        .strip_prefix("manifest-")
                        .and_then(|name| name.strip_suffix(".json"))
                    else {
                        return Ok(false);
                    };
                    if validate_digest(digest).is_err() {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// Full recovery validation is explicit and linear in records/artifact reads.
    /// No successful prefix is reused as authority when any suffix is uncertain.
    pub(crate) fn inspect(&self, binding: &JournalBinding) -> Result<JournalInspection, SafeError> {
        binding.validate()?;
        self.validate_storage_transaction(binding)?;
        let mut file = self.log.lock();
        let identity = regular_file(&file)?;
        file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
        let mut bytes = Vec::new();
        (&mut *file)
            .take(self.limits.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(storage_error)?;
        let mut status = JournalInspection {
            last_valid: None,
            blocked: self.poisoned.load(Ordering::SeqCst) || !self.namespace_valid()?,
            head: None,
            usage: Usage::default(),
            hash: Sha256::new(),
            identity,
        };
        if bytes.len() as u64 > self.limits.bytes
            || file.metadata().map_err(storage_error)?.len() != bytes.len() as u64
        {
            status.blocked = true;
        }
        for frame in bytes.split_inclusive(|byte| *byte == b'\n') {
            if status.usage.records >= self.limits.records
                || status.usage.bytes + frame.len() as u64 > self.limits.bytes
            {
                status.blocked = true;
                break;
            }
            #[cfg(test)]
            self.replay_count.fetch_add(1, Ordering::SeqCst);
            let envelope = match Envelope::decode(frame) {
                Ok(envelope) => envelope,
                Err(_) => {
                    status.blocked = true;
                    break;
                }
            };
            let record = envelope.record;
            if &record.binding != binding
                || record.generation != status.usage.records
                || record.previous != status.head
            {
                status.blocked = true;
                break;
            }
            if status.usage.records == 0 {
                let JournalEvent::Begin { capacity } = record.event else {
                    status.blocked = true;
                    break;
                };
                if record.lane.is_some()
                    || capacity.validate(self.limits, frame.len() as u64).is_err()
                {
                    status.blocked = true;
                    break;
                }
                status.last_valid = Some(SwitchJournal::new(binding.clone(), capacity)?);
                status.usage.records = 1;
                status.usage.bytes = frame.len() as u64;
            } else {
                let Some(journal) = status.last_valid.as_mut() else {
                    status.blocked = true;
                    break;
                };
                let lane = journal.lane(&record.event);
                if record.lane != Some(lane)
                    || journal.validate_event(&record.event).is_err()
                    || self
                        .validate_artifacts(
                            journal,
                            &record.event,
                            status.head.as_deref().unwrap_or_default(),
                            &status.identity,
                        )
                        .is_err()
                    || status
                        .usage
                        .check(&journal.capacity, lane, frame.len() as u64, self.limits)
                        .is_err()
                {
                    status.blocked = true;
                    break;
                }
                journal.commit_event(record.event);
                status.usage.commit(lane, frame.len() as u64);
            }
            status.hash.update(frame);
            status.head = Some(envelope.digest);
        }
        if let Some(cached) = &self.writer {
            if status.head.as_deref() != Some(cached.head.as_str())
                || status.usage.bytes != cached.usage.bytes
                || status.identity != cached.identity
            {
                status.blocked = true;
            }
        }
        if status.blocked {
            self.poisoned.store(true, Ordering::SeqCst);
        }
        Ok(status)
    }

    #[cfg(test)]
    pub(crate) fn fixture_replay_count(&self) -> u64 {
        self.replay_count.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn fixture_dependency_count(&self) -> usize {
        self.dependencies.lock().len()
    }
    #[cfg(all(test, windows))]
    pub(crate) fn fixture_windows_dependency_limit(
        root: std::sync::Arc<super::windows::files::PrivateDirectory>,
        limit: usize,
    ) -> Result<Self, SafeError> {
        if limit == 0 || limit > MAX_DEPENDENCY_HANDLES {
            return Err(error("HISTORY_DEPENDENCY_LIMIT"));
        }
        let mut store = Self::open_windows(root)?;
        store.dependency_limit = limit;
        Ok(store)
    }
    #[cfg(test)]
    pub(crate) fn fixture_with_dependency_limit(
        directory: Dir,
        limit: usize,
    ) -> Result<Self, SafeError> {
        if limit == 0 || limit > MAX_DEPENDENCY_HANDLES {
            return Err(error("HISTORY_DEPENDENCY_LIMIT"));
        }
        let mut store = Self::fixture(directory)?;
        store.dependency_limit = limit;
        Ok(store)
    }
    #[cfg(test)]
    pub(crate) fn fixture_with_durability(
        directory: Dir,
        durability: Box<dyn DirectoryDurability>,
    ) -> Result<Self, SafeError> {
        Self::open_parts(
            directory,
            durability,
            Limits::default(),
            Self::fixture_trust(),
        )
    }
    #[cfg(test)]
    fn fixture_trust() -> WriterTrust {
        #[cfg(windows)]
        {
            WriterTrust::HeldWindowsHandle
        }
        #[cfg(not(windows))]
        {
            WriterTrust::FixtureFullHash
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture_with_limits(
        directory: Dir,
        records: u64,
        bytes: u64,
    ) -> Result<Self, SafeError> {
        Self::open_parts(
            directory,
            Box::new(TestDurability),
            Limits { records, bytes },
            Self::fixture_trust(),
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture(directory: Dir) -> Result<Self, SafeError> {
        Self::fixture_with_durability(directory, Box::new(TestDurability))
    }
}

#[cfg(test)]
struct TestDurability;
#[cfg(test)]
impl AdmissionAnchor {
    fn fixture(inspection: &JournalInspection) -> Self {
        assert!(!inspection.blocked);
        let journal = inspection.last_valid.as_ref().unwrap();
        Self {
            binding: journal.binding.clone(),
            generation: journal.generation,
            head: inspection.head.clone().unwrap(),
            journal_identity: inspection.identity.clone(),
        }
    }
}
#[cfg(test)]
impl PreContextAbortProof<'static> {
    pub(crate) fn fixture(inspection: &JournalInspection, digests: [String; 4]) -> Self {
        let [bundle, roots, registration, quiescence] = digests;
        Self {
            anchor: AdmissionAnchor::fixture(inspection),
            unchanged: UnchangedSourceObservations {
                bundle,
                roots,
                registration,
                quiescence,
            },
            guards: Box::new(FixtureAbortGuards),
        }
    }
}
#[cfg(test)]
impl UnknownCompensationProof {
    pub(crate) fn fixture(inspection: &JournalInspection, digests: [String; 4]) -> Self {
        let journal = inspection.last_valid.as_ref().unwrap();
        let pending = journal.pending.as_ref().unwrap();
        let effect = journal.effects.get(pending).unwrap();
        let [processes, jobs, roots, registration] = digests;
        Self {
            anchor: AdmissionAnchor::fixture(inspection),
            effect_id: pending.clone(),
            intent_generation: effect.intent_generation,
            current: RecoveryObservations {
                processes,
                jobs,
                roots,
                registration,
            },
            _guards: Box::new(()),
        }
    }
}
#[cfg(test)]
impl DirectoryDurability for TestDurability {
    fn sync_directory(&self, directory: &Dir) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            directory.try_clone()?.into_std_file().sync_all()
        }
        #[cfg(not(unix))]
        {
            let _ = directory;
            Ok(())
        }
    }
}
#[cfg(any(test, windows))]
fn storage_error(_: std::io::Error) -> SafeError {
    error("HISTORY_STORAGE_UNAVAILABLE")
}
#[cfg(test)]
fn file_options(shared_writer: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let _ = shared_writer;
    }
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options
            .share_mode(if shared_writer { 3 } else { 1 })
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = shared_writer;
    }
    options
}
#[cfg(any(test, windows))]
fn regular_file(file: &File) -> Result<String, SafeError> {
    let metadata = file.metadata().map_err(storage_error)?;
    if !metadata.is_file() {
        return Err(error("HISTORY_UNSAFE_TREE"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(error("HISTORY_UNSAFE_TREE"));
        }
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        if info.nNumberOfLinks != 1
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || (info.nFileIndexHigh == 0 && info.nFileIndexLow == 0)
        {
            return Err(error("HISTORY_UNSAFE_TREE"));
        }
        Ok(format!(
            "windows:{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(error("HISTORY_PLATFORM_UNSUPPORTED"))
    }
}
