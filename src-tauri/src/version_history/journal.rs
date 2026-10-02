//! Append-only, individually flushed records and immutable manifests. A record
//! proves an observed transcript, never authority to replay an OS operation.
//! No multi-file/registry atomicity or Windows directory durability is implied.
use super::maintenance::{DirectoryDurability, PrivateRecoveryRoot};
use super::verified_package::sha256;
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::atomic::AtomicU64;

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
    pub(crate) fn for_effects(forward_effects: u64, recovery_effects: u64, control_records_per_lane: u64, record_byte_ceiling: u64) -> Result<Self, SafeError> {
        let records = |effects: u64| effects.checked_mul(3).and_then(|count| count.checked_add(control_records_per_lane))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"));
        if forward_effects == 0 || recovery_effects == 0 || control_records_per_lane == 0 {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        let plan = Self { forward_records: records(forward_effects)?, recovery_records: records(recovery_effects)?, record_byte_ceiling };
        plan.validate(Limits::default(), 0)?;
        Ok(plan)
    }
    fn validate(&self, limits: Limits, genesis_bytes: u64) -> Result<(), SafeError> {
        let records = self.forward_records.checked_add(self.recovery_records).and_then(|count| count.checked_add(1))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        let bytes = (records - 1).checked_mul(self.record_byte_ceiling).and_then(|bytes| bytes.checked_add(genesis_bytes))
            .ok_or_else(|| error("HISTORY_JOURNAL_CAPACITY"))?;
        if self.forward_records == 0 || self.recovery_records == 0 || self.record_byte_ceiling < 2048
            || self.record_byte_ceiling > MAX_RECORD_BYTES as u64 || records > limits.records || bytes > limits.bytes {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Limits { records: u64, bytes: u64 }
impl Default for Limits {
    fn default() -> Self { Self { records: MAX_RECORDS as u64, bytes: MAX_JOURNAL_BYTES as u64 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum WriteLane { Forward, Recovery }
#[derive(Default)]
struct Usage { records: u64, bytes: u64, forward_records: u64, recovery_records: u64 }
impl Usage {
    fn check(&self, plan: &CapacityPlan, lane: WriteLane, bytes: u64, limits: Limits) -> Result<(), SafeError> {
        let (used, reserved) = match lane {
            WriteLane::Forward => (self.forward_records, plan.forward_records),
            WriteLane::Recovery => (self.recovery_records, plan.recovery_records),
        };
        if bytes > plan.record_byte_ceiling || used >= reserved || self.records >= limits.records
            || self.bytes.checked_add(bytes).is_none_or(|total| total > limits.bytes) {
            return Err(error("HISTORY_JOURNAL_CAPACITY"));
        }
        Ok(())
    }
    fn commit(&mut self, lane: WriteLane, bytes: u64) {
        self.records += 1;
        self.bytes += bytes;
        match lane { WriteLane::Forward => self.forward_records += 1, WriteLane::Recovery => self.recovery_records += 1 }
    }
}

pub(super) fn validate_id(value: &str) -> Result<(), SafeError> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| error("HISTORY_IDENTITY_INVALID"))?;
    if parsed.is_nil() || parsed.hyphenated().to_string() != value { return Err(error("HISTORY_IDENTITY_INVALID")); }
    Ok(())
}
pub(super) fn validate_digest(value: &str) -> Result<(), SafeError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err(error("HISTORY_IDENTITY_INVALID"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RootKind { Desk, WebView }

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
        for id in [&self.transaction_id, &self.source_context, &self.target_context] { validate_id(id)?; }
        if self.source_context == self.target_context { return Err(error("HISTORY_IDENTITY_INVALID")); }
        for digest in [&self.user_installation, &self.source_bundle, &self.target_package, &self.target_payload, &self.roots] {
            validate_digest(digest)?;
        }
        Ok(())
    }
    pub(crate) fn has_context(&self, id: &str) -> bool { id == self.source_context || id == self.target_context }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RegistrationSlot {
    Uninstall, Publisher, DeskDirectory, DeskDirectoryBackground, LegacyDirectory, LegacyDirectoryBackground,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ShortcutSlot { Desktop, StartMenu }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum FilesystemOperation { CreateDirectory, CopyFile, Rename, SetPermissions }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum RegistrationOperation { CreateKey, SetValue, RemoveOwnedValue, RemoveOwnedKey, SetPermissions }

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum EffectKind {
    FenceSourceImage,
    FenceHistoricalImage,
    PreserveRoot { context: String, root: RootKind },
    CreateFreshRoot { root: RootKind },
    RestoreSourceRoot { root: RootKind },
    /// Each individual copy/rename/permission write references a backend-held
    /// immutable manifest entry. No deserialized path becomes OS authority.
    FilesystemEntry { operation: FilesystemOperation, manifest: String, entry_index: u32 },
    RegistrationEntry { slot: RegistrationSlot, operation: RegistrationOperation, manifest: String, entry_index: u32 },
    /// These aggregate observations never replace individual effect records.
    VerifySourceBundleCopy,
    VerifySourceBundleRestore,
    InstallerCreateSuspended,
    InstallerResume,
    InstallerTerminalOutcome,
    VerifyTargetBundle,
    ConfirmFirstLaunch,
    VerifyRegistrationRestore { slot: RegistrationSlot },
    RestoreShortcut { slot: ShortcutSlot },
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
        if let EffectKind::PreserveRoot { context, .. } = &self.kind {
            if !binding.has_context(context) { return Err(error("HISTORY_CONTEXT_CHANGED")); }
        }
        if let Some(manifest) = self.entry_manifest() { validate_digest(manifest)?; }
        Ok(())
    }
    fn entry_manifest(&self) -> Option<&str> {
        match &self.kind {
            EffectKind::FilesystemEntry { manifest, .. } | EffectKind::RegistrationEntry { manifest, .. } => Some(manifest),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Observation { Applied, NotApplied, Unknown }
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
pub(crate) enum ManifestRole { SourceContext, FreshTargetContext, RetainedTargetContext, SourceBundle, Registration, Shortcuts }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum JournalPhase {
    Reviewed, SourceSealed, FreshReady, Installing, InstalledUnconfirmed,
    HistoricalActive, Restoring, Restored, RecoveryRequired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum JournalEvent {
    Begin { capacity: CapacityPlan },
    Manifest { role: ManifestRole, digest: String },
    Intent { effect: EffectSpec },
    Observed { effect_id: String, intent_generation: u64, result: ObservedResult },
    Phase { phase: JournalPhase },
}
struct EffectRecord { spec: EffectSpec, intent_generation: u64, result: Option<ObservedResult> }

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
}
impl SwitchJournal {
    pub(crate) fn new(binding: JournalBinding, capacity: CapacityPlan) -> Result<Self, SafeError> {
        binding.validate()?;
        capacity.validate(Limits::default(), 0)?;
        Ok(Self { binding, generation: 0, phase: JournalPhase::Reviewed, effects: BTreeMap::new(),
            applied_kinds: BTreeSet::new(), pending: None, manifests: BTreeMap::new(), capacity, recovering: false })
    }
    pub(crate) fn generation(&self) -> u64 { self.generation }
    pub(crate) fn binding(&self) -> &JournalBinding { &self.binding }
    pub(crate) fn phase(&self) -> JournalPhase { self.phase }
    pub(crate) fn pending_effect(&self) -> Option<&EffectSpec> {
        self.pending.as_ref().and_then(|id| self.effects.get(id)).map(|effect| &effect.spec)
    }
    pub(crate) fn requires_reconciliation(&self) -> bool { self.pending.is_some() }

    /// Validation is read-only. Commit changes only the indexed affected entry;
    /// failed validation never requires cloning/rolling back the entire history.
    pub(crate) fn apply(&mut self, event: JournalEvent) -> Result<(), SafeError> {
        self.validate_event(&event)?;
        self.commit_event(event);
        Ok(())
    }
    fn validate_event(&self, event: &JournalEvent) -> Result<(), SafeError> {
        self.generation.checked_add(1).ok_or_else(|| error("HISTORY_JOURNAL_LIMIT"))?;
        if self.phase == JournalPhase::Restored { return Err(error("HISTORY_TRANSACTION_TERMINAL")); }
        match event {
            JournalEvent::Begin { .. } => return Err(error("HISTORY_JOURNAL_INVALID")),
            JournalEvent::Manifest { role, digest } => {
                validate_digest(digest)?;
                if self.requires_reconciliation() || self.manifests.contains_key(role) {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
            }
            JournalEvent::Intent { effect } => {
                effect.validate(&self.binding)?;
                if self.requires_reconciliation() || self.effects.len() >= MAX_RECORDS || self.effects.contains_key(&effect.effect_id) {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
            }
            JournalEvent::Observed { effect_id, intent_generation, result } => {
                let effect = self.effects.get(effect_id).ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                if self.pending.as_deref() != Some(effect_id.as_str()) || effect.intent_generation != *intent_generation
                    || effect.result.as_ref().is_some_and(|old| old.observation != Observation::Unknown) {
                    return Err(error("HISTORY_EFFECT_CHANGED"));
                }
                match (&result.observation, &result.receipt) {
                    (Observation::Applied | Observation::NotApplied, Some(receipt)) => validate_digest(receipt)?,
                    (Observation::Unknown, None) => (),
                    _ => return Err(error("HISTORY_EFFECT_CHANGED")),
                }
            }
            JournalEvent::Phase { phase } => {
                if *phase != JournalPhase::RecoveryRequired && (self.requires_reconciliation() || !self.phase_allowed(*phase)) {
                    return Err(error("HISTORY_RECONCILIATION_REQUIRED"));
                }
            }
        }
        Ok(())
    }
    fn lane(&self, event: &JournalEvent) -> WriteLane {
        if self.recovering || matches!(event, JournalEvent::Phase { phase: JournalPhase::RecoveryRequired | JournalPhase::Restoring }) {
            WriteLane::Recovery
        } else { WriteLane::Forward }
    }
    fn commit_event(&mut self, event: JournalEvent) {
        self.generation += 1;
        self.recovering |= self.lane(&event) == WriteLane::Recovery;
        match event {
            JournalEvent::Begin { .. } => unreachable!("genesis is not an appended event"),
            JournalEvent::Manifest { role, digest } => { self.manifests.insert(role, digest); }
            JournalEvent::Intent { effect } => {
                self.pending = Some(effect.effect_id.clone());
                self.effects.insert(effect.effect_id.clone(), EffectRecord { spec: effect, intent_generation: self.generation, result: None });
            }
            JournalEvent::Observed { effect_id, result, .. } => {
                let effect = self.effects.get_mut(&effect_id).expect("validated effect");
                if result.observation == Observation::Applied { self.applied_kinds.insert(effect.spec.kind.clone()); }
                if result.observation != Observation::Unknown { self.pending = None; }
                effect.result = Some(result);
            }
            JournalEvent::Phase { phase } => { self.phase = phase; }
        }
    }
    fn applied(&self, kind: EffectKind) -> bool { self.applied_kinds.contains(&kind) }
    fn preserved(&self, context: &str) -> bool {
        [RootKind::Desk, RootKind::WebView].into_iter().all(|root| self.applied(EffectKind::PreserveRoot { context: context.into(), root }))
    }
    fn phase_allowed(&self, next: JournalPhase) -> bool {
        match (self.phase, next) {
            (JournalPhase::Reviewed, JournalPhase::SourceSealed) => {
                self.applied(EffectKind::VerifySourceBundleCopy) && self.applied(EffectKind::FenceSourceImage) && self.preserved(&self.binding.source_context)
                    && [ManifestRole::SourceContext, ManifestRole::SourceBundle, ManifestRole::Registration, ManifestRole::Shortcuts]
                        .into_iter().all(|role| self.manifests.contains_key(&role))
            }
            (JournalPhase::SourceSealed, JournalPhase::FreshReady) => {
                [RootKind::Desk, RootKind::WebView].into_iter().all(|root| self.applied(EffectKind::CreateFreshRoot { root }))
                    && self.manifests.contains_key(&ManifestRole::FreshTargetContext)
            }
            (JournalPhase::FreshReady, JournalPhase::Installing) => true,
            (JournalPhase::Installing, JournalPhase::InstalledUnconfirmed) => {
                self.applied(EffectKind::InstallerCreateSuspended) && self.applied(EffectKind::InstallerResume)
                    && self.applied(EffectKind::InstallerTerminalOutcome) && self.applied(EffectKind::VerifyTargetBundle)
            }
            (JournalPhase::InstalledUnconfirmed, JournalPhase::HistoricalActive) => self.applied(EffectKind::ConfirmFirstLaunch),
            (JournalPhase::HistoricalActive | JournalPhase::RecoveryRequired | JournalPhase::InstalledUnconfirmed, JournalPhase::Restoring) => {
                self.applied(EffectKind::FenceHistoricalImage) && self.preserved(&self.binding.target_context)
                    && self.manifests.contains_key(&ManifestRole::RetainedTargetContext)
                    && self.manifests.contains_key(&ManifestRole::SourceContext)
            }
            (JournalPhase::Restoring, JournalPhase::Restored) => {
                self.applied(EffectKind::VerifySourceBundleRestore)
                    && [RootKind::Desk, RootKind::WebView].into_iter().all(|root| self.applied(EffectKind::RestoreSourceRoot { root }))
                    && [RegistrationSlot::Uninstall, RegistrationSlot::Publisher, RegistrationSlot::DeskDirectory,
                        RegistrationSlot::DeskDirectoryBackground, RegistrationSlot::LegacyDirectory, RegistrationSlot::LegacyDirectoryBackground]
                        .into_iter().all(|slot| self.applied(EffectKind::VerifyRegistrationRestore { slot }))
                    && [ShortcutSlot::Desktop, ShortcutSlot::StartMenu].into_iter().all(|slot| self.applied(EffectKind::RestoreShortcut { slot }))
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
struct Envelope { record: Record, digest: String }
impl Envelope {
    fn new(record: Record) -> Result<Self, SafeError> {
        let bytes = serde_json::to_vec(&record).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        Ok(Self { record, digest: sha256(&bytes) })
    }
    fn encode(&self) -> Result<Vec<u8>, SafeError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES { return Err(error("HISTORY_JOURNAL_LIMIT")); }
        Ok(bytes)
    }
    fn decode(bytes: &[u8]) -> Result<Self, SafeError> {
        if bytes.len() > MAX_RECORD_BYTES || !bytes.ends_with(b"\n") { return Err(error("HISTORY_JOURNAL_INVALID")); }
        let envelope: Self = serde_json::from_slice(bytes).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        let record = serde_json::to_vec(&envelope.record).map_err(|_| error("HISTORY_JOURNAL_INVALID"))?;
        if envelope.record.schema != 2 || sha256(&record) != envelope.digest { return Err(error("HISTORY_JOURNAL_INVALID")); }
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
    pub(super) fn head(&self) -> Option<&str> { self.head.as_deref() }
}
struct WriterState {
    journal: SwitchJournal,
    head: String,
    usage: Usage,
    hash: Sha256,
    identity: String,
}
struct ProtectedManifest {
    file: File,
    identity: String,
    length: u64,
}

#[derive(Clone, Copy)]
enum WriterTrust {
    /// Real retained writable Windows handle with FILE_SHARE_READ only. Other
    /// write/delete opens and pathname replacement are denied for its lifetime.
    #[cfg(windows)]
    HeldWindowsHandle,
    /// Test fixtures on unsupported platforms rehash EVERY byte before append.
    /// This is intentionally not a production writer-exclusion proof or fast path.
    FixtureFullHash,
}

pub(crate) struct JournalStore {
    directory: Dir,
    durability: Box<dyn DirectoryDurability>,
    _writer_lock: File,
    log: Mutex<File>,
    /// A referenced artifact is part of the validated writer state. Keep the
    /// original read-only no-write/no-delete handle, not just a remembered hash.
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
impl JournalStore {
    pub(crate) fn open(root: PrivateRecoveryRoot) -> Result<Self, SafeError> {
        #[cfg(windows)]
        {
            let (directory, durability) = root.into_parts();
            Self::open_parts(directory, durability, Limits::default(), WriterTrust::HeldWindowsHandle)
        }
        #[cfg(not(windows))]
        { let _ = root; Err(error("HISTORY_PLATFORM_UNSUPPORTED")) }
    }
    fn open_parts(directory: Dir, durability: Box<dyn DirectoryDurability>, limits: Limits, trust: WriterTrust) -> Result<Self, SafeError> {
        let mut options = file_options(true);
        options.read(true).write(true).create(true);
        let file = directory.open_with("journal.lock", &options).map_err(storage_error)?.into_std();
        regular_file(&file)?;
        file.try_lock().map_err(|_| error("HISTORY_TRANSACTION_BUSY"))?;
        let mut log_options = file_options(false);
        log_options.read(true).write(true).create(true);
        // Never truncate or replace this file. Previous immutable frames and a
        // possible torn tail survive all failures. Closing/reopening is recovery.
        let log = directory.open_with("journal.log", &log_options).map_err(storage_error)?.into_std();
        regular_file(&log)?;
        Ok(Self { directory, durability, _writer_lock: file, log: Mutex::new(log), writer: None,
            dependencies: Mutex::new(BTreeMap::new()), dependency_limit: MAX_DEPENDENCY_HANDLES,
            trust, limits, poisoned: AtomicBool::new(false), #[cfg(test)] replay_count: AtomicU64::new(0) })
    }

    /// The complete reviewed operation budget is durable in genesis before ANY
    /// intent. Reserve return/recovery separately; forward work cannot spend it.
    pub(crate) fn initialize(&mut self, binding: JournalBinding, capacity: CapacityPlan) -> Result<(), SafeError> {
        self.healthy()?;
        if self.writer.is_some() || !self.namespace_valid()? || self.log.lock().metadata().map_err(storage_error)?.len() != 0 {
            return Err(error("HISTORY_JOURNAL_EXISTS"));
        }
        let journal = SwitchJournal::new(binding.clone(), capacity.clone())?;
        let record = Envelope::new(Record { schema: 2, binding, generation: 0, previous: None, lane: None, event: JournalEvent::Begin { capacity } })?;
        let bytes = record.encode()?;
        journal.capacity.validate(self.limits, bytes.len() as u64)?;
        let identity = regular_file(&self.log.lock())?;
        self.write_frame(0, &bytes)?;
        let mut hash = Sha256::new();
        hash.update(&bytes);
        self.writer = Some(WriterState { journal, head: record.digest, identity, hash,
            usage: Usage { records: 1, bytes: bytes.len() as u64, ..Usage::default() } });
        Ok(())
    }
    pub(crate) fn bind_existing(&mut self, binding: &JournalBinding) -> Result<(), SafeError> {
        self.healthy()?;
        let read = self.inspect(binding)?;
        if read.blocked { return Err(error("HISTORY_RECOVERY_REQUIRED")); }
        let journal = read.last_valid.ok_or_else(|| error("HISTORY_JOURNAL_INVALID"))?;
        let head = read.head.ok_or_else(|| error("HISTORY_JOURNAL_INVALID"))?;
        self.writer = Some(WriterState { journal, head, usage: read.usage, hash: read.hash, identity: read.identity });
        Ok(())
    }
    pub(crate) fn append(&mut self, expected_generation: u64, event: JournalEvent) -> Result<u64, SafeError> {
        self.check_writer_current()?;
        let state = self.writer.as_ref().ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if state.journal.generation != expected_generation { return Err(error("HISTORY_GENERATION_CHANGED")); }
        state.journal.validate_event(&event)?;
        self.validate_artifacts(&state.journal, &event)?;
        let lane = state.journal.lane(&event);
        let record = Envelope::new(Record { schema: 2, binding: state.journal.binding.clone(), generation: expected_generation + 1,
            previous: Some(state.head.clone()), lane: Some(lane), event: event.clone() })?;
        let bytes = record.encode()?;
        // Prospective total AND reserved-lane checks precede every disk write.
        state.usage.check(&state.journal.capacity, lane, bytes.len() as u64, self.limits)?;
        self.write_frame(state.usage.bytes, &bytes)?;
        let state = self.writer.as_mut().expect("writer retained across exclusive append");
        state.journal.commit_event(event);
        state.head = record.digest;
        state.hash.update(&bytes);
        state.usage.commit(lane, bytes.len() as u64);
        Ok(state.journal.generation)
    }
    fn healthy(&self) -> Result<(), SafeError> {
        if self.poisoned.load(Ordering::SeqCst) { return Err(error("HISTORY_RECOVERY_REQUIRED")); }
        Ok(())
    }
    fn check_writer_current(&self) -> Result<(), SafeError> {
        self.healthy()?;
        let result = self.check_writer_object().and_then(|_| self.check_protected_dependencies());
        if result.is_err() { self.poisoned.store(true, Ordering::SeqCst); }
        result
    }
    fn check_writer_object(&self) -> Result<(), SafeError> {
        let state = self.writer.as_ref().ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        #[allow(unused_mut)] // Only non-production fixture verification reads bytes.
        let mut file = self.log.lock();
        if regular_file(&file)? != state.identity || file.metadata().map_err(storage_error)?.len() != state.usage.bytes {
            return Err(error("HISTORY_JOURNAL_CHANGED"));
        }
        // Reopen for identity observation only. Share-write admits OUR already
        // held writer, not another writer: the original handle denies that open.
        let mut options = file_options(true);
        options.read(true);
        let named = self.directory.open_with("journal.log", &options).map_err(storage_error)?.into_std();
        if regular_file(&named)? != state.identity { return Err(error("HISTORY_JOURNAL_CHANGED")); }
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
                    if count == 0 { break; }
                    total += count as u64;
                    if total > self.limits.bytes { return Err(error("HISTORY_JOURNAL_CHANGED")); }
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
            // Exact read-only handles deny ordinary writes/deletes for every
            // cached dependency, including receipt wrappers and observed data.
            #[cfg(windows)]
            WriterTrust::HeldWindowsHandle => Ok(()),
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
            let mut file = self.log.lock();
            if file.metadata().map_err(storage_error)?.len() != expected_length { return Err(error("HISTORY_JOURNAL_CHANGED")); }
            file.seek(SeekFrom::Start(expected_length)).map_err(storage_error)?;
            file.write_all(bytes).and_then(|_| file.sync_all()).map_err(storage_error)?;
            self.durability.sync_directory(&self.directory).map_err(storage_error)
        })();
        if result.is_err() { self.poisoned.store(true, Ordering::SeqCst); }
        result
    }

    /// Called after an actual observation. Persist the typed observed manifest
    /// first, then this bound receipt, then the Observed record. A crash at any
    /// boundary leaves the original intent unresolved; it never authorizes replay.
    pub(crate) fn retain_effect_receipt(&self, effect_id: &str, observation: Observation, observed_manifest: &str) -> Result<String, SafeError> {
        self.check_writer_current()?;
        let journal = &self.writer.as_ref().ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?.journal;
        let effect = journal.effects.get(effect_id).ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
        if journal.pending.as_deref() != Some(effect_id) || observation == Observation::Unknown
            || (observation == Observation::NotApplied && observed_manifest != effect.spec.before) {
            return Err(error("HISTORY_EFFECT_CHANGED"));
        }
        self.read_manifest(observed_manifest)?;
        let receipt = EffectReceipt { schema: 1, transaction_id: journal.binding.transaction_id.clone(), effect_id: effect_id.into(),
            intent_generation: effect.intent_generation, expected_postconditions: effect.spec.expected_postconditions.clone(), observation,
            observed_manifest: observed_manifest.into() };
        self.retain_manifest(&serde_json::to_vec(&receipt).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?)
    }
    fn validate_artifacts(&self, journal: &SwitchJournal, event: &JournalEvent) -> Result<(), SafeError> {
        match event {
            JournalEvent::Manifest { digest, .. } => { self.protect_manifest(digest)?; }
            JournalEvent::Intent { effect } => {
                self.protect_manifest(&effect.before)?;
                self.protect_manifest(&effect.expected_postconditions)?;
                if let Some(manifest) = effect.entry_manifest() { self.protect_manifest(manifest)?; }
            }
            JournalEvent::Observed { effect_id, intent_generation, result } => {
                if let Some(digest) = &result.receipt {
                    let bytes = self.protect_manifest(digest)?;
                    if bytes.len() > 16384 { return Err(error("HISTORY_RECEIPT_INVALID")); }
                    let receipt: EffectReceipt = serde_json::from_slice(&bytes).map_err(|_| error("HISTORY_RECEIPT_INVALID"))?;
                    let effect = journal.effects.get(effect_id).ok_or_else(|| error("HISTORY_EFFECT_CHANGED"))?;
                    if receipt.schema != 1 || receipt.transaction_id != journal.binding.transaction_id || receipt.effect_id != *effect_id
                        || receipt.intent_generation != *intent_generation || receipt.intent_generation != effect.intent_generation
                        || receipt.expected_postconditions != effect.spec.expected_postconditions || receipt.observation != result.observation
                        || receipt.observation == Observation::Unknown
                        || (receipt.observation == Observation::NotApplied && receipt.observed_manifest != effect.spec.before) {
                        return Err(error("HISTORY_RECEIPT_INVALID"));
                    }
                    self.protect_manifest(&receipt.observed_manifest)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn create_immutable(&self, name: &str, bytes: &[u8]) -> Result<(), SafeError> {
        let result = (|| {
            let mut options = file_options(false);
            options.write(true).create_new(true);
            let mut file = self.directory.open_with(name, &options).map_err(storage_error)?.into_std();
            regular_file(&file)?;
            file.write_all(bytes).and_then(|_| file.sync_all()).map_err(storage_error)?;
            self.durability.sync_directory(&self.directory).map_err(storage_error)
        })();
        if result.is_err() { self.poisoned.store(true, Ordering::SeqCst); }
        result
    }
    pub(crate) fn retain_manifest(&self, bytes: &[u8]) -> Result<String, SafeError> {
        self.healthy()?;
        if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES { return Err(error("HISTORY_MANIFEST_INVALID")); }
        let digest = sha256(bytes);
        let name = format!("manifest-{digest}.json");
        if self.directory.symlink_metadata(&name).is_ok() {
            if self.read_manifest(&digest)? == bytes { return Ok(digest); }
            return Err(error("HISTORY_MANIFEST_CHANGED"));
        }
        self.create_immutable(&name, bytes)?;
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
    /// Acquire and hash-validate before the first frame that references this
    /// artifact. Existing dependencies share one retained handle per digest.
    fn protect_manifest(&self, digest: &str) -> Result<Vec<u8>, SafeError> {
        validate_digest(digest)?;
        let mut dependencies = self.dependencies.lock();
        if let Some(held) = dependencies.get_mut(digest) {
            return self.read_protected_manifest(digest, held);
        }
        if dependencies.len() >= self.dependency_limit { return Err(error("HISTORY_DEPENDENCY_LIMIT")); }
        let mut held = self.open_manifest(digest)?;
        let bytes = self.read_protected_manifest(digest, &mut held)?;
        dependencies.insert(digest.into(), held);
        Ok(bytes)
    }
    fn open_manifest(&self, digest: &str) -> Result<ProtectedManifest, SafeError> {
        let mut options = file_options(false);
        options.read(true);
        let file = self.directory.open_with(format!("manifest-{digest}.json"), &options).map_err(storage_error)?.into_std();
        let identity = regular_file(&file)?;
        let length = file.metadata().map_err(storage_error)?.len();
        if length > MAX_MANIFEST_BYTES as u64 { return Err(error("HISTORY_MANIFEST_INVALID")); }
        Ok(ProtectedManifest { file, identity, length })
    }
    fn read_protected_manifest(&self, digest: &str, held: &mut ProtectedManifest) -> Result<Vec<u8>, SafeError> {
        if regular_file(&held.file)? != held.identity || held.file.metadata().map_err(storage_error)?.len() != held.length {
            return Err(error("HISTORY_MANIFEST_CHANGED"));
        }
        let mut options = file_options(false);
        options.read(true);
        let named = self.directory.open_with(format!("manifest-{digest}.json"), &options).map_err(storage_error)?.into_std();
        if regular_file(&named)? != held.identity { return Err(error("HISTORY_MANIFEST_CHANGED")); }
        held.file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
        let mut bytes = Vec::new();
        (&mut held.file).take(MAX_MANIFEST_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(storage_error)?;
        if bytes.len() as u64 != held.length || sha256(&bytes) != digest || regular_file(&held.file)? != held.identity {
            return Err(error("HISTORY_MANIFEST_CHANGED"));
        }
        Ok(bytes)
    }
    fn namespace_valid(&self) -> Result<bool, SafeError> {
        for (index, entry) in self.directory.entries().map_err(storage_error)?.enumerate() {
            if index > MAX_RECORDS * 4 { return Ok(false); }
            let entry = entry.map_err(storage_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { return Ok(false); };
            if name == "journal.lock" || name == "journal.log" { continue; }
            let kind = entry.file_type().map_err(storage_error)?;
            if !kind.is_file() || kind.is_symlink() { return Ok(false); }
            let Some(digest) = name.strip_prefix("manifest-").and_then(|name| name.strip_suffix(".json")) else { return Ok(false); };
            if validate_digest(digest).is_err() { return Ok(false); }
        }
        Ok(true)
    }

    /// Full recovery validation is explicit and linear in records/artifact reads.
    /// No successful prefix is reused as authority when any suffix is uncertain.
    pub(crate) fn inspect(&self, binding: &JournalBinding) -> Result<JournalInspection, SafeError> {
        binding.validate()?;
        let mut file = self.log.lock();
        let identity = regular_file(&file)?;
        file.seek(SeekFrom::Start(0)).map_err(storage_error)?;
        let mut bytes = Vec::new();
        (&mut *file).take(self.limits.bytes + 1).read_to_end(&mut bytes).map_err(storage_error)?;
        let mut status = JournalInspection { last_valid: None, blocked: self.poisoned.load(Ordering::SeqCst) || !self.namespace_valid()?,
            head: None, usage: Usage::default(), hash: Sha256::new(), identity };
        if bytes.len() as u64 > self.limits.bytes || file.metadata().map_err(storage_error)?.len() != bytes.len() as u64 { status.blocked = true; }
        for frame in bytes.split_inclusive(|byte| *byte == b'\n') {
            if status.usage.records >= self.limits.records || status.usage.bytes + frame.len() as u64 > self.limits.bytes {
                status.blocked = true; break;
            }
            #[cfg(test)]
            self.replay_count.fetch_add(1, Ordering::SeqCst);
            let envelope = match Envelope::decode(frame) { Ok(envelope) => envelope, Err(_) => { status.blocked = true; break; } };
            let record = envelope.record;
            if &record.binding != binding || record.generation != status.usage.records || record.previous != status.head {
                status.blocked = true; break;
            }
            if status.usage.records == 0 {
                let JournalEvent::Begin { capacity } = record.event else { status.blocked = true; break; };
                if record.lane.is_some() || capacity.validate(self.limits, frame.len() as u64).is_err() { status.blocked = true; break; }
                status.last_valid = Some(SwitchJournal::new(binding.clone(), capacity)?);
                status.usage.records = 1;
                status.usage.bytes = frame.len() as u64;
            } else {
                let Some(journal) = status.last_valid.as_mut() else { status.blocked = true; break; };
                let lane = journal.lane(&record.event);
                if record.lane != Some(lane) || journal.validate_event(&record.event).is_err()
                    || self.validate_artifacts(journal, &record.event).is_err()
                    || status.usage.check(&journal.capacity, lane, frame.len() as u64, self.limits).is_err() {
                    status.blocked = true; break;
                }
                journal.commit_event(record.event);
                status.usage.commit(lane, frame.len() as u64);
            }
            status.hash.update(frame);
            status.head = Some(envelope.digest);
        }
        if let Some(cached) = &self.writer {
            if status.head.as_deref() != Some(cached.head.as_str()) || status.usage.bytes != cached.usage.bytes || status.identity != cached.identity {
                status.blocked = true;
            }
        }
        if status.blocked { self.poisoned.store(true, Ordering::SeqCst); }
        Ok(status)
    }

    #[cfg(test)]
    pub(crate) fn fixture_replay_count(&self) -> u64 { self.replay_count.load(Ordering::SeqCst) }
    #[cfg(test)]
    pub(crate) fn fixture_dependency_count(&self) -> usize { self.dependencies.lock().len() }
    #[cfg(test)]
    pub(crate) fn fixture_with_dependency_limit(directory: Dir, limit: usize) -> Result<Self, SafeError> {
        if limit == 0 || limit > MAX_DEPENDENCY_HANDLES { return Err(error("HISTORY_DEPENDENCY_LIMIT")); }
        let mut store = Self::fixture(directory)?;
        store.dependency_limit = limit;
        Ok(store)
    }
    #[cfg(test)]
    pub(crate) fn fixture_with_durability(directory: Dir, durability: Box<dyn DirectoryDurability>) -> Result<Self, SafeError> {
        Self::open_parts(directory, durability, Limits::default(), Self::fixture_trust())
    }
    #[cfg(test)]
    fn fixture_trust() -> WriterTrust {
        #[cfg(windows)]
        { WriterTrust::HeldWindowsHandle }
        #[cfg(not(windows))]
        { WriterTrust::FixtureFullHash }
    }
    #[cfg(test)]
    pub(crate) fn fixture_with_limits(directory: Dir, records: u64, bytes: u64) -> Result<Self, SafeError> {
        Self::open_parts(directory, Box::new(TestDurability), Limits { records, bytes }, Self::fixture_trust())
    }
    #[cfg(test)]
    pub(crate) fn fixture(directory: Dir) -> Result<Self, SafeError> {
        Self::fixture_with_durability(directory, Box::new(TestDurability))
    }
}

#[cfg(test)]
struct TestDurability;
#[cfg(test)]
impl DirectoryDurability for TestDurability {
    fn sync_directory(&self, directory: &Dir) -> std::io::Result<()> {
        #[cfg(unix)]
        { directory.try_clone()?.into_std_file().sync_all() }
        #[cfg(not(unix))]
        { let _ = directory; Ok(()) }
    }
}
fn storage_error(_: std::io::Error) -> SafeError { error("HISTORY_STORAGE_UNAVAILABLE") }
fn file_options(shared_writer: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let _ = shared_writer;
    }
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.share_mode(if shared_writer { 3 } else { 1 })
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = shared_writer; }
    options
}
fn regular_file(file: &File) -> Result<String, SafeError> {
    let metadata = file.metadata().map_err(storage_error)?;
    if !metadata.is_file() { return Err(error("HISTORY_UNSAFE_TREE")); }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 { return Err(error("HISTORY_UNSAFE_TREE")); }
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT};
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        if info.nNumberOfLinks != 1 || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || (info.nFileIndexHigh == 0 && info.nFileIndexLow == 0) {
            return Err(error("HISTORY_UNSAFE_TREE"));
        }
        Ok(format!("windows:{}:{}:{}", info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    }
    #[cfg(not(any(unix, windows)))]
    { Err(error("HISTORY_PLATFORM_UNSUPPORTED")) }
}
