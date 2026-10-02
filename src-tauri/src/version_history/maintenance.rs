//! Backend-held admission contracts. No command, IPC constructor, automatic kill,
//! or production Windows proof constructor is supplied by this foundation.
use super::journal::{JournalBinding, RootKind};
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock};

#[derive(Clone, Copy)]
pub(crate) enum RuntimeKind {
    Native,
    Legacy,
}

enum ChildState {
    Starting(RuntimeKind),
    Running(RuntimeKind),
    Unknown,
}
#[derive(Default)]
struct AdmissionState {
    children: HashMap<uuid::Uuid, ChildState>,
    mutations: HashMap<uuid::Uuid, MutationState>,
    frozen: Option<String>,
}
enum MutationState {
    Active,
    Unknown,
}

/// One shared ledger must cover BOTH runtimes, including starts before spawn.
/// Existing registry/map emptiness is deliberately not an input to this API.
#[derive(Clone, Default)]
pub(crate) struct AdmissionGate(Arc<Mutex<AdmissionState>>);
static PROCESS_ADMISSIONS: LazyLock<AdmissionGate> = LazyLock::new(AdmissionGate::new);

/// Static config/check writers and freshly constructed repositories share this owner.
pub(crate) fn process_admissions() -> AdmissionGate {
    PROCESS_ADMISSIONS.clone()
}

impl std::fmt::Debug for AdmissionGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdmissionGate(<redacted>)")
    }
}

impl AdmissionGate {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_start(&self, runtime: RuntimeKind) -> Result<StartTicket, SafeError> {
        let mut state = self.0.lock();
        if state.frozen.is_some() {
            return Err(error("HISTORY_MAINTENANCE_ACTIVE"));
        }
        let id = uuid::Uuid::new_v4();
        state.children.insert(id, ChildState::Starting(runtime));
        Ok(StartTicket {
            gate: self.clone(),
            id,
            completed: false,
        })
    }

    /// Narrow central config/projects/workspace integration must acquire this
    /// before the authoritative mutation begins, preserving existing store locks.
    pub(crate) fn begin_mutation(&self) -> Result<MutationTicket, SafeError> {
        let mut state = self.0.lock();
        if state.frozen.is_some() {
            return Err(error("HISTORY_MAINTENANCE_ACTIVE"));
        }
        let id = uuid::Uuid::new_v4();
        state.mutations.insert(id, MutationState::Active);
        Ok(MutationTicket {
            gate: self.clone(),
            id,
            completed: false,
        })
    }

    /// Zero-owner observation and admission freeze share one mutex. Never stops
    /// a session. Unknown ownership remains a blocker even after map removal.
    pub(crate) fn freeze(&self, transaction_id: &str) -> Result<FrozenAdmissions, SafeError> {
        super::journal::validate_id(transaction_id)?;
        let mut state = self.0.lock();
        if state.frozen.is_some() || !state.children.is_empty() {
            return Err(error("HISTORY_SESSIONS_NOT_QUIESCENT"));
        }
        state.frozen = Some(transaction_id.into());
        Ok(FrozenAdmissions {
            gate: self.clone(),
            transaction_id: transaction_id.into(),
            committed: false,
        })
    }
}

/// Complete only after the authoritative writer has finished, not merely a
/// frontend queue or process::exit. Existing successful store writes do NOT
/// prove Windows metadata durability; post-exit platform flush/verification is
/// a separate SnapshotBoundary prerequisite. Failures require reconciliation.
pub(crate) struct MutationTicket {
    gate: AdmissionGate,
    id: uuid::Uuid,
    completed: bool,
}
impl MutationTicket {
    /// Only before an authoritative writer or effectful callback has been entered.
    pub(crate) fn no_write_performed(self) {
        self.completed_authoritative_write();
    }

    pub(crate) fn preparing(self) -> PreparingMutation {
        PreparingMutation(Some(self))
    }
    pub(crate) fn completed_authoritative_write(mut self) {
        self.gate.0.lock().mutations.remove(&self.id);
        self.completed = true;
    }
}
impl Drop for MutationTicket {
    fn drop(&mut self) {
        if !self.completed {
            self.gate
                .0
                .lock()
                .mutations
                .insert(self.id, MutationState::Unknown);
        }
    }
}

/// Move the ticket into the exact spawn owner; dropped/unresolved tickets poison
/// admission. Only a positively known pre-spawn failure calls no_child_created.
pub(crate) struct StartTicket {
    gate: AdmissionGate,
    id: uuid::Uuid,
    completed: bool,
}
impl StartTicket {
    pub(crate) fn preparing(self) -> PreparingStart {
        PreparingStart(Some(self))
    }
    pub(crate) fn no_child_created(mut self) {
        self.gate.0.lock().children.remove(&self.id);
        self.completed = true;
    }

    pub(crate) fn child_created(mut self) -> OwnedChildTicket {
        let mut state = self.gate.0.lock();
        if let Some(ChildState::Starting(runtime)) = state.children.get(&self.id) {
            let runtime = *runtime;
            state.children.insert(self.id, ChildState::Running(runtime));
        }
        self.completed = true;
        OwnedChildTicket {
            gate: self.gate.clone(),
            id: self.id,
            completed: false,
        }
    }
}
impl Drop for StartTicket {
    fn drop(&mut self) {
        if !self.completed {
            self.gate
                .0
                .lock()
                .children
                .insert(self.id, ChildState::Unknown);
        }
    }
}

/// A backend preparation scope knows that no process-creation call has begun.
/// Move its ticket out immediately before entering spawn; errors/panics thereafter
/// remain unknown unless the audited platform adapter proves no child was created.
pub(crate) struct PreparingStart(Option<StartTicket>);
impl PreparingStart {
    pub(crate) fn begin_creation(&mut self) -> StartTicket {
        self.0.take().expect("start ticket already transferred")
    }
}
impl Drop for PreparingStart {
    fn drop(&mut self) {
        if let Some(ticket) = self.0.take() {
            ticket.no_child_created();
        }
    }
}

/// Only use around preparation known not to mutate authoritative data. An
/// effectful callback (including deletion) must take the ticket before entry.
pub(crate) struct PreparingMutation(Option<MutationTicket>);
impl PreparingMutation {
    pub(crate) fn begin_write(&mut self) -> MutationTicket {
        self.0.take().expect("mutation ticket already transferred")
    }
}
impl Drop for PreparingMutation {
    fn drop(&mut self) {
        if let Some(ticket) = self.0.take() {
            ticket.no_write_performed();
        }
    }
}

/// Move into the direct child's waiter, independently of UI/PTY registration.
/// A kill request, synthetic exit, reader EOF, or drain timeout must never call
/// reaped(). Unix wait/try_wait must reap; Windows requires WAIT_OBJECT_0 on
/// the exact retained process handle. Output may still be draining afterwards.
pub(crate) struct OwnedChildTicket {
    gate: AdmissionGate,
    id: uuid::Uuid,
    completed: bool,
}
impl OwnedChildTicket {
    pub(crate) fn reaped(mut self) {
        self.gate.0.lock().children.remove(&self.id);
        self.completed = true;
    }
    pub(crate) fn wait_failed(self) {
        drop(self);
    }
}
impl Drop for OwnedChildTicket {
    fn drop(&mut self) {
        if !self.completed {
            self.gate
                .0
                .lock()
                .children
                .insert(self.id, ChildState::Unknown);
        }
    }
}

pub(crate) struct FrozenAdmissions {
    gate: AdmissionGate,
    transaction_id: String,
    committed: bool,
}
impl FrozenAdmissions {
    /// The coordinator calls this only after the persistent barrier is durable.
    /// Post-commit interruption belongs to journaled recovery, never UI cancel.
    pub(crate) fn mark_committed(&mut self) -> Result<(), SafeError> {
        let state = self.gate.0.lock();
        if state.frozen.as_deref() != Some(self.transaction_id.as_str())
            || !state.mutations.is_empty()
        {
            return Err(error("HISTORY_MUTATIONS_NOT_SETTLED"));
        }
        self.committed = true;
        Ok(())
    }
    pub(crate) fn release_review(self) -> Result<(), SafeError> {
        if self.committed {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let mut state = self.gate.0.lock();
        if state.frozen.as_deref() != Some(self.transaction_id.as_str()) {
            return Err(error("HISTORY_TRANSACTION_CHANGED"));
        }
        state.frozen = None;
        Ok(())
    }
}
// No Drop unfreezes admission: unwinding must not reopen a committed operation.

/// The future Windows coordinator must retain all the OS handles in this object:
/// exact source App/WebView exits and relevant UDF users observed; exclusive
/// stable installation lease; same installed-image no-sharing handle renamed
/// without replacement on the same volume; secured root/parent capabilities.
/// Central authoritative writes must already have completed before source exit.
/// Afterwards, retained-file Windows flush and verified metadata/bytes must be
/// positively established. Neither a store Ok nor source PID exit proves those.
/// There is intentionally NO production constructor until those checks exist.
pub(crate) struct SnapshotBoundary {
    binding: JournalBinding,
    root_identities: BTreeMap<RootKind, String>,
    _held_platform_guards: Box<dyn Send + Sync>,
}
impl SnapshotBoundary {
    pub(crate) fn binding(&self) -> &JournalBinding {
        &self.binding
    }
    pub(crate) fn root_identity(&self, root: RootKind) -> Option<&str> {
        self.root_identities.get(&root).map(String::as_str)
    }

    #[cfg(test)]
    pub(crate) fn fixture(binding: JournalBinding) -> Self {
        Self {
            binding,
            root_identities: BTreeMap::from([
                (RootKind::Desk, "desk-object".into()),
                (RootKind::WebView, "webview-parent/absent".into()),
            ]),
            _held_platform_guards: Box::new(()),
        }
    }
}

/// Secured, identity-pinned private directory OUTSIDE installation/data roots.
/// Future Windows construction verifies the owner ACL, ancestor/reparse and
/// overlap policy, and supplies validated durable-directory sync semantics.
/// No frontend path or boolean can construct this capability.
pub(crate) struct PrivateRecoveryRoot {
    directory: cap_std::fs::Dir,
    durability: Box<dyn DirectoryDurability>,
}
pub(crate) trait DirectoryDurability: Send + Sync {
    fn sync_directory(&self, directory: &cap_std::fs::Dir) -> std::io::Result<()>;
}
impl PrivateRecoveryRoot {
    pub(super) fn into_parts(self) -> (cap_std::fs::Dir, Box<dyn DirectoryDurability>) {
        (self.directory, self.durability)
    }
}

/// Holds a SHARED lock on the stable per-user/registered-install admission file.
/// The Windows adapter must acquire it, then read/recheck the persistent marker
/// under that same lock, BEFORE ConPTY/logger/Native storage/WebView bootstrap.
/// Keep it for the whole ordinary process lifetime. No production constructor.
/// Acquire the separate short control lock FIRST; acquire shared lifetime lease,
/// read marker, and decide_startup under control. Release control after decision.
pub(crate) struct SharedStartupLease {
    user_installation: String,
    actual_bundle: String,
    registered_entrypoint: bool,
    _held_shared_lease: Box<dyn Send + Sync>,
}
/// Separate cross-process control lock serializes ordinary admission with marker
/// publication. A source app publishes the durable transition marker under THIS
/// lock while retaining its ordinary shared lifetime lock. After exact source
/// exit, manager acquires a NEW exclusive lifetime lease, never a shared→exclusive
/// upgrade. Contention/another admitted instance aborts before image/data effects;
/// waiting for an unrelated window to disappear does not prove source quiescence.
/// The exclusive coordinator and durable marker writer are Task4, not this core.
pub(crate) struct StartupControlLease {
    user_installation: String,
    _held_control: Box<dyn Send + Sync>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupDecision {
    Ordinary,
    RecoveryOnly,
}
pub(crate) enum MarkerRead<'a> {
    /// Only authoritative NotFound in the pinned stable directory, never an IO
    /// error, missing environment variable or absent manager PID.
    Absent,
    Unreadable,
    Present {
        bytes: &'a [u8],
        journal: Option<&'a super::journal::JournalInspection>,
    },
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
enum BarrierState {
    Transition,
    Restored,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveContextMarker {
    schema: u32,
    binding: JournalBinding,
    generation: u64,
    journal_digest: String,
    state: BarrierState,
}
impl ActiveContextMarker {
    pub(crate) fn transition(
        binding: JournalBinding,
        generation: u64,
        journal_digest: String,
    ) -> Result<Self, SafeError> {
        binding.validate()?;
        super::journal::validate_digest(&journal_digest)?;
        Ok(Self {
            schema: 1,
            binding,
            generation,
            journal_digest,
            state: BarrierState::Transition,
        })
    }
    /// Prepare the final marker only from a fully validated terminal transcript.
    /// Task4 must verify the actual restored bundle/data/registration again and
    /// durably publish it under the exclusive lease before releasing startup.
    pub(crate) fn restored(
        inspection: &super::journal::JournalInspection,
    ) -> Result<Self, SafeError> {
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if inspection.blocked
            || journal.phase() != super::journal::JournalPhase::Restored
            || journal.requires_reconciliation()
        {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let mut marker = Self::transition(
            journal.binding().clone(),
            journal.generation(),
            inspection
                .head()
                .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?
                .into(),
        )?;
        marker.state = BarrierState::Restored;
        Ok(marker)
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, SafeError> {
        serde_json::to_vec(self).map_err(|_| error("HISTORY_MARKER_INVALID"))
    }
}

pub(crate) fn decide_startup(
    control: &StartupControlLease,
    lease: &SharedStartupLease,
    marker: MarkerRead<'_>,
) -> StartupDecision {
    if control.user_installation != lease.user_installation || !lease.registered_entrypoint {
        return StartupDecision::RecoveryOnly;
    }
    match marker {
        MarkerRead::Absent => StartupDecision::Ordinary,
        MarkerRead::Unreadable => StartupDecision::RecoveryOnly,
        MarkerRead::Present { bytes, journal } => {
            if bytes.len() > 16384 {
                return StartupDecision::RecoveryOnly;
            }
            let Ok(marker) = serde_json::from_slice::<ActiveContextMarker>(bytes) else {
                return StartupDecision::RecoveryOnly;
            };
            if marker.schema != 1
                || marker.binding.validate().is_err()
                || marker.state != BarrierState::Restored
                || marker.binding.user_installation != lease.user_installation
                || marker.binding.source_bundle != lease.actual_bundle
            {
                return StartupDecision::RecoveryOnly;
            }
            let Some(inspection) = journal else {
                return StartupDecision::RecoveryOnly;
            };
            let Some(journal) = inspection.last_valid.as_ref() else {
                return StartupDecision::RecoveryOnly;
            };
            if inspection.blocked
                || journal.phase() != super::journal::JournalPhase::Restored
                || journal.requires_reconciliation()
                || journal.binding() != &marker.binding
                || journal.generation() != marker.generation
                || inspection.head() != Some(marker.journal_digest.as_str())
            {
                return StartupDecision::RecoveryOnly;
            }
            StartupDecision::Ordinary
        }
    }
}

#[cfg(test)]
impl SharedStartupLease {
    pub(crate) fn fixture(binding: JournalBinding, backup: bool) -> Self {
        Self {
            user_installation: binding.user_installation,
            actual_bundle: binding.source_bundle,
            registered_entrypoint: !backup,
            _held_shared_lease: Box::new(()),
        }
    }
    pub(crate) fn fixture_with_guard(binding: JournalBinding, guard: Box<dyn Send + Sync>) -> Self {
        Self {
            user_installation: binding.user_installation,
            actual_bundle: binding.source_bundle,
            registered_entrypoint: true,
            _held_shared_lease: guard,
        }
    }
}

#[cfg(test)]
impl StartupControlLease {
    pub(crate) fn fixture(binding: JournalBinding) -> Self {
        Self {
            user_installation: binding.user_installation,
            _held_control: Box::new(()),
        }
    }
    pub(crate) fn fixture_with_guard(binding: JournalBinding, guard: Box<dyn Send + Sync>) -> Self {
        Self {
            user_installation: binding.user_installation,
            _held_control: guard,
        }
    }
}
