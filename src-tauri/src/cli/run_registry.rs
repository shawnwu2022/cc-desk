//! Backend-owned reservations and retained launch outcomes. Never a wire capability.
#![allow(dead_code)] // Live WebView lifetime binding is a separate D11 integration step.

use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::request_fingerprint::{validate_routing_id, Fingerprint, RequestFingerprinter};
use super::snapshot::CallerIdentity;
use super::types::{LaunchRequest, SafeError, WireU64};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunKey {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchPhase {
    Reserved,
    Starting,
    Running,
    Failed,
    Cancelled,
    Indeterminate,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchFailure {
    RouteUnavailable,
    ProcessStartFailed,
    Aborted,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchStatus {
    pub(crate) instance_id: String,
    pub(crate) request_id: String,
    pub(crate) run: RunKey,
    pub(crate) revision: WireU64,
    pub(crate) phase: LaunchPhase,
    pub(crate) failure: Option<LaunchFailure>,
}

#[cfg(all(windows, feature = "remote-observer"))]
#[derive(PartialEq, Eq)]
pub(crate) enum CatalogueNativeId {
    Codex(uuid::Uuid),
    Opencode(String),
    Claude(uuid::Uuid),
}

#[cfg(all(windows, feature = "remote-observer"))]
struct VerifiedCatalogueRun {
    native: CatalogueNativeId,
    project: Option<String>,
    status: LaunchStatus,
}

#[cfg(all(windows, feature = "remote-observer"))]
fn unique_management_run(
    runs: impl IntoIterator<Item = VerifiedCatalogueRun>,
    native: &CatalogueNativeId,
    project: &str,
) -> Option<LaunchStatus> {
    let mut exact = runs.into_iter().filter(|run| &run.native == native);
    let run = exact.next()?;
    if run.project.as_deref() != Some(project) || exact.next().is_some() {
        return None;
    }
    Some(run.status)
}

#[cfg(all(test, windows, feature = "remote-observer"))]
mod management_target_tests {
    use super::*;

    fn candidate(id: &str, project: Option<&str>, generation: u32) -> VerifiedCatalogueRun {
        VerifiedCatalogueRun {
            native: CatalogueNativeId::Opencode(id.into()),
            project: project.map(str::to_owned),
            status: LaunchStatus {
                instance_id: "instance".into(),
                request_id: format!("request-{generation}"),
                run: RunKey {
                    run_id: format!("run-{generation}"),
                    generation,
                },
                revision: WireU64::parse("2").unwrap(),
                phase: LaunchPhase::Running,
                failure: None,
            },
        }
    }

    #[test]
    fn original_management_target_keeps_exact_request_and_generation() {
        let id = CatalogueNativeId::Opencode("exact".into());
        let selected = unique_management_run(
            [
                candidate("other", Some("project"), 1),
                candidate("exact", Some("project"), 3),
            ],
            &id,
            "project",
        )
        .unwrap();
        assert_eq!(selected.run.generation, 3);
        assert_eq!(selected.run.run_id, "run-3");
        assert_eq!(selected.request_id, "request-3");
        assert_eq!(selected.phase, LaunchPhase::Running);
    }

    #[test]
    fn original_management_target_never_guesses_project_or_chooses_a_duplicate() {
        let id = CatalogueNativeId::Opencode("same".into());
        assert!(unique_management_run([candidate("same", None, 1)], &id, "project").is_none());
        assert!(
            unique_management_run([candidate("same", Some("other"), 1)], &id, "project").is_none()
        );
        assert!(unique_management_run(
            [
                candidate("same", Some("project"), 1),
                candidate("same", Some("project"), 2)
            ],
            &id,
            "project",
        )
        .is_none());
        assert!(unique_management_run(
            [
                candidate("same", Some("project"), 1),
                candidate("same", Some("other"), 2)
            ],
            &id,
            "project",
        )
        .is_none());
        let codex = uuid::Uuid::new_v4();
        let mut row = candidate("unused", Some("project"), 1);
        row.native = CatalogueNativeId::Codex(codex);
        assert!(
            unique_management_run([row], &CatalogueNativeId::Claude(codex), "project").is_none()
        );
    }
}

type RequestKey = (u64, String);

struct Record<R> {
    owner: CallerIdentity,
    fingerprint: Fingerprint,
    status: LaunchStatus,
    resource: Option<Arc<R>>,
    source: Option<Arc<super::source_provenance::SourceProvenance>>,
    #[cfg(all(windows, feature = "remote-observer"))]
    catalogue_identity:
        Option<Arc<crate::version_history::codex_archive_admission::CodexOwnerIdentity>>,
    #[cfg(all(windows, feature = "remote-observer"))]
    catalogue_opencode: Option<Arc<super::launch_service::OpencodeCatalogueIdentity>>,
    #[cfg(all(windows, feature = "remote-observer"))]
    catalogue_claude: Option<Arc<super::launch_service::ClaudeCatalogueIdentity>>,
    in_flight: bool,
    retired: bool,
    eviction_ready: bool,
}

impl<R> Record<R> {
    #[cfg(all(windows, feature = "remote-observer"))]
    fn catalogue_eligible(&self) -> bool {
        !self.in_flight
            && matches!(
                (self.status.phase, self.retired),
                (LaunchPhase::Running, false) | (LaunchPhase::Exited, true)
            )
    }

    fn transition(&mut self, phase: LaunchPhase, failure: Option<LaunchFailure>) {
        if self.status.phase == phase && self.status.failure == failure {
            return;
        }
        // Records start at zero and the private, acyclic state graph has at
        // most three changes. Repeated exit/retire never consume a revision.
        let revision = self
            .status
            .revision
            .get()
            .checked_add(1)
            .expect("finite launch state graph");
        self.status.revision =
            WireU64::parse(&revision.to_string()).expect("canonical internal revision");
        self.status.phase = phase;
        self.status.failure = failure;
    }
}

struct RegistryState<R> {
    epoch: u64,
    active: bool,
    records: HashMap<RequestKey, Record<R>>,
    runs: HashMap<String, RequestKey>,
    tabs: HashMap<(u64, String), RequestKey>,
    // 每个 tab 已插入记录的最大代次；墓碑被容量淘汰后仍保持 generation 单调。
    tab_generations: HashMap<(u64, String), u32>,
    // 记录插入序，供容量压力下最老优先淘汰。
    order: VecDeque<RequestKey>,
}

pub(crate) struct RunRegistry<R> {
    instance_id: String,
    capacity: usize,
    fingerprints: RequestFingerprinter,
    state: Mutex<RegistryState<R>>,
}

impl<R> std::fmt::Debug for RunRegistry<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunRegistry(<redacted>)")
    }
}

pub(super) enum Reservation<'a, R> {
    Existing(LaunchStatus),
    New(Ticket<'a, R>),
}

/// Only the insertion winner owns this non-cloneable completion permit.
/// Dropping it leaves a tombstone instead of authorizing another execution.
pub(super) struct Ticket<'a, R> {
    registry: &'a RunRegistry<R>,
    key: RequestKey,
    finished: Option<LaunchStatus>,
}

impl<R> RunRegistry<R> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            capacity,
            fingerprints: RequestFingerprinter::new(),
            state: Mutex::new(RegistryState {
                epoch: 0,
                active: false,
                records: HashMap::new(),
                runs: HashMap::new(),
                tabs: HashMap::new(),
                tab_generations: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Backend lifecycle entry only. Do not expose this as caller-chosen epoch IPC.
    pub(crate) fn activate_window(&self, label: &str) -> Result<CallerIdentity, SafeError> {
        authorize_profile_window(label)?;
        let mut state = self.state.lock();
        let epoch = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| error("WINDOW_EPOCH_EXHAUSTED"))?;
        state.epoch = epoch;
        state.active = true;
        // 旧 document 已永久失去准入资格；仅其代次下限可释放，运行资源仍由后台保有。
        state.tab_generations.clear();
        let sources: Vec<_> = state
            .records
            .values_mut()
            .filter_map(|record| record.source.take())
            .collect();
        let caller = CallerIdentity {
            instance_id: self.instance_id.clone(),
            window_label: label.into(),
            webview_epoch: WireU64::parse(&epoch.to_string())?,
        };
        drop(state);
        drop(sources);
        Ok(caller)
    }

    fn authorize(
        &self,
        state: &RegistryState<R>,
        caller: &CallerIdentity,
    ) -> Result<(), SafeError> {
        if caller.instance_id != self.instance_id
            || caller.window_label != "main"
            || !state.active
            || caller.webview_epoch.get() != state.epoch
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }

    /// Check current authority without reserving or reading a launch record.
    /// Effectful callers still revalidate at their existing commit points.
    pub(crate) fn check_caller(&self, caller: &CallerIdentity) -> Result<(), SafeError> {
        self.authorize(&self.state.lock(), caller)
    }

    /// Linearize a short process-creation commit with document revocation.
    pub(crate) fn with_current_caller<T>(
        &self,
        caller: &CallerIdentity,
        operation: impl FnOnce() -> T,
    ) -> Result<T, SafeError> {
        let state = self.state.lock();
        self.authorize(&state, caller)?;
        Ok(operation())
    }
    pub(crate) fn revoke_window(&self, caller: &CallerIdentity) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        self.authorize(&state, caller)?;
        state.active = false;
        let sources: Vec<_> = state
            .records
            .values_mut()
            .filter_map(|record| record.source.take())
            .collect();
        // Existing resources stay owned. The lifecycle layer decides drain/stop;
        // revocation alone never drops a master PTY or kills an agent.
        drop(state);
        drop(sources);
        Ok(())
    }

    pub(super) fn existing(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> Result<Option<LaunchStatus>, SafeError> {
        self.authorize(&self.state.lock(), caller)?;
        let fingerprint = self.fingerprints.fingerprint(request)?;
        let state = self.state.lock();
        self.authorize(&state, caller)?;
        let key = (caller.webview_epoch.get(), request.request_id.clone());
        self.existing_locked(&state, &key, fingerprint)
    }

    fn existing_locked(
        &self,
        state: &RegistryState<R>,
        key: &RequestKey,
        fingerprint: Fingerprint,
    ) -> Result<Option<LaunchStatus>, SafeError> {
        let Some(record) = state.records.get(key) else {
            return Ok(None);
        };
        if record.fingerprint != fingerprint {
            return Err(error("REQUEST_CONFLICT"));
        }
        Ok(Some(record.status.clone()))
    }

    pub(super) fn reserve(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> Result<Reservation<'_, R>, SafeError> {
        self.authorize(&self.state.lock(), caller)?;
        let fingerprint = self.fingerprints.fingerprint(request)?;
        let mut state = self.state.lock();
        self.authorize(&state, caller)?;
        self.reserve_locked(&mut state, caller, request, fingerprint)
    }

    fn reserve_locked(
        &self,
        state: &mut RegistryState<R>,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        fingerprint: Fingerprint,
    ) -> Result<Reservation<'_, R>, SafeError> {
        let key = (caller.webview_epoch.get(), request.request_id.clone());
        let tab = (caller.webview_epoch.get(), request.tab_id.clone());
        if let Some(status) = self.existing_locked(state, &key, fingerprint)? {
            return Ok(Reservation::Existing(status));
        }
        // 活 document 的下限不能淘汰，否则会允许旧 generation 重放。独立有限预算
        // 容纳记录淘汰后的历史 tab；满额只拒绝新 tab，不阻止已知 tab 的下一代。
        if !state.tab_generations.contains_key(&tab)
            && state.tab_generations.len() >= self.capacity.saturating_mul(2)
        {
            return Err(error("REGISTRY_CAPACITY"));
        }
        if state.records.len() >= self.capacity {
            self.evict_retired_locked(state);
            if state.records.len() >= self.capacity {
                return Err(error("REGISTRY_CAPACITY"));
            }
        }
        if state.runs.contains_key(&request.run_id) {
            return Err(error("RUN_ID_CONFLICT"));
        }
        if let Some(previous) = state.tabs.get(&tab).and_then(|key| state.records.get(key)) {
            if !previous.retired {
                return Err(error("TAB_BUSY"));
            }
        }
        // 容量压力可能已淘汰墓碑；代次下限保证同 tab 的 generation 单调不回退。
        if let Some(floor) = state.tab_generations.get(&tab) {
            if request.generation <= *floor {
                return Err(error("STALE_GENERATION"));
            }
        }
        let status = LaunchStatus {
            instance_id: self.instance_id.clone(),
            request_id: request.request_id.clone(),
            run: RunKey {
                run_id: request.run_id.clone(),
                generation: request.generation,
            },
            revision: WireU64::parse("0")?,
            phase: LaunchPhase::Reserved,
            failure: None,
        };
        state.order.push_back(key.clone());
        let floor = state.tab_generations.entry(tab.clone()).or_insert(0);
        *floor = (*floor).max(request.generation);
        state.records.insert(
            key.clone(),
            Record {
                owner: caller.clone(),
                fingerprint,
                status,
                resource: None,
                source: None,
                #[cfg(all(windows, feature = "remote-observer"))]
                catalogue_identity: None,
                #[cfg(all(windows, feature = "remote-observer"))]
                catalogue_opencode: None,
                #[cfg(all(windows, feature = "remote-observer"))]
                catalogue_claude: None,
                in_flight: true,
                retired: false,
                eviction_ready: false,
            },
        );
        state.runs.insert(request.run_id.clone(), key.clone());
        state.tabs.insert(tab, key.clone());
        Ok(Reservation::New(Ticket {
            registry: self,
            key,
            finished: None,
        }))
    }

    /// 只有完整 supervisor 收口或从未持有 child 的墓碑才可淘汰；已 reap 但仍
    /// draining 的 receipt 保留，避免同 RunKey 新 child 撞上旧 active owner。
    fn evict_retired_locked(&self, state: &mut RegistryState<R>) {
        let mut index = 0;
        while state.records.len() >= self.capacity && index < state.order.len() {
            let key = state.order[index].clone();
            let evictable = state
                .records
                .get(&key)
                .is_some_and(|record| !record.in_flight && record.retired && record.eviction_ready);
            if !evictable {
                index += 1;
                continue;
            }
            state.order.remove(index);
            let run_id = state
                .records
                .remove(&key)
                .map(|record| record.status.run.run_id);
            if let Some(run_id) = run_id {
                if state.runs.get(&run_id).is_some_and(|mapped| *mapped == key) {
                    state.runs.remove(&run_id);
                }
            }
            state.tabs.retain(|_, mapped| *mapped != key);
        }
    }

    pub(crate) fn status(
        &self,
        caller: &CallerIdentity,
        request_id: &str,
    ) -> Result<LaunchStatus, SafeError> {
        let state = self.state.lock();
        self.authorize(&state, caller)?;
        validate_routing_id("requestId", request_id)?;
        let record = state
            .records
            .get(&(caller.webview_epoch.get(), request_id.into()))
            .ok_or_else(|| error("LAUNCH_NOT_FOUND"))?;
        Ok(record.status.clone())
    }

    /// Cancel this exact authenticated request before its spawn commit point.
    /// An absent request receives a retained tombstone, so a preparation already
    /// in flight cannot reserve/spawn later. Once begin wins, only a real process
    /// exit can establish a terminal outcome; cancellation returns that receipt.
    pub(crate) fn cancel(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> Result<LaunchStatus, SafeError> {
        request.validate()?;
        self.authorize(&self.state.lock(), caller)?;
        let fingerprint = self.fingerprints.fingerprint(request)?;
        let mut state = self.state.lock();
        self.authorize(&state, caller)?;
        // 同锁准入与取消，既保留原 owner 的 Ticket，也防止终态回执在二次查找前被淘汰。
        let mut ticket = match self.reserve_locked(&mut state, caller, request, fingerprint)? {
            Reservation::New(ticket) => Some(ticket),
            Reservation::Existing(_) => None,
        };
        let result = {
            let key = (caller.webview_epoch.get(), request.request_id.clone());
            let record = state
                .records
                .get_mut(&key)
                .expect("reservation is retained");
            if record.status.phase == LaunchPhase::Reserved {
                record.transition(LaunchPhase::Cancelled, None);
                record.retired = true;
                record.eviction_ready = true;
                // 同锁结束 ticket，不能让容量回收先于随后 Drop 读取其记录。
                if let Some(ticket) = ticket.as_mut() {
                    record.in_flight = false;
                    ticket.finished = Some(record.status.clone());
                }
            }
            record.status.clone()
        };
        drop(state);
        drop(ticket);
        Ok(result)
    }

    /// Validate that the current authenticated document owns this exact run.
    /// Unlike resource(), this remains valid after process handles are retired so
    /// a still-active document can explicitly abort an output drain.
    pub(crate) fn check_run(&self, caller: &CallerIdentity, run: &RunKey) -> Result<(), SafeError> {
        let state = self.state.lock();
        self.checked_run_record(&state, caller, run)?;
        Ok(())
    }

    fn checked_run_record<'a>(
        &self,
        state: &'a RegistryState<R>,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<&'a Record<R>, SafeError> {
        self.authorize(state, caller)?;
        let key = state
            .runs
            .get(&run.run_id)
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        let record = state.records.get(key).expect("run index is retained");
        if record.owner != *caller {
            return Err(error("FORBIDDEN"));
        }
        if record.status.run != *run {
            return Err(error("STALE_GENERATION"));
        }
        Ok(record)
    }

    /// 停止准入在同一锁内冻结精确 request/run receipt，不把迟到旧请求改绑到新运行。
    pub(crate) fn admit_run(
        &self,
        caller: &CallerIdentity,
        run: &RunKey,
        request_id: &str,
    ) -> Result<LaunchStatus, SafeError> {
        let state = self.state.lock();
        let record = self.checked_run_record(&state, caller, run)?;
        validate_routing_id("requestId", request_id)?;
        if record.status.request_id != request_id {
            return Err(error("REQUEST_CONFLICT"));
        }
        Ok(record.status.clone())
    }

    /// Backend-only handle acquisition. A returned Arc is not a wire capability;
    /// each later external operation must be admitted against its own caller/run.
    /// Exited resources remain available for drain until explicit retirement.
    pub(crate) fn resource(
        &self,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<Arc<R>, SafeError> {
        let state = self.state.lock();
        self.authorize(&state, caller)?;
        let key = state
            .runs
            .get(&run.run_id)
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        let record = state.records.get(key).expect("run index is retained");
        if record.owner != *caller {
            return Err(error("FORBIDDEN"));
        }
        if record.status.run != *run {
            return Err(error("STALE_GENERATION"));
        }
        record
            .resource
            .clone()
            .ok_or_else(|| error("RUN_NOT_READY"))
    }

    /// External mutations need a live run, unlike resource() used for retained
    /// output drain. Call again after waiting for the PTY mutation owner.
    pub(crate) fn mutation_resource(
        &self,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<Arc<R>, SafeError> {
        let state = self.state.lock();
        let record = self.checked_run_record(&state, caller, run)?;
        if record.status.phase != LaunchPhase::Running {
            return Err(error("RUN_NOT_READY"));
        }
        record
            .resource
            .clone()
            .ok_or_else(|| error("RUN_NOT_READY"))
    }

    /// 只读来源证明随 exact receipt 保留；它不包含进程、环境或 argv 能力。
    pub(crate) fn source_provenance(
        &self,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<Arc<super::source_provenance::SourceProvenance>, SafeError> {
        let source = {
            let state = self.state.lock();
            let record = self.checked_run_record(&state, caller, run)?;
            if !matches!(
                record.status.phase,
                LaunchPhase::Running | LaunchPhase::Exited
            ) {
                return Err(error("RUN_NOT_READY"));
            }
            record
                .source
                .clone()
                .ok_or_else(|| error("RUN_NOT_READY"))?
        };
        source.selection()?;
        let state = self.state.lock();
        let record = self.checked_run_record(&state, caller, run)?;
        if !record
            .source
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &source))
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(source)
    }
    /// Backend lifecycle adoption must survive document revocation. Never expose
    /// this method to a wire command: use resource(caller, run) for caller access.
    pub(crate) fn retained_resource(&self, run: &RunKey) -> Result<Arc<R>, SafeError> {
        let state = self.state.lock();
        let key = state
            .runs
            .get(&run.run_id)
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        let record = state.records.get(key).expect("run index is retained");
        if record.status.run != *run {
            return Err(error("STALE_GENERATION"));
        }
        record
            .resource
            .clone()
            .ok_or_else(|| error("RUN_NOT_READY"))
    }

    /// Published only after adoption of the exact original resource.
    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn record_catalogue_adoption(
        &self,
        run: &RunKey,
        resource: &Arc<R>,
        identity: Arc<crate::version_history::codex_archive_admission::CodexOwnerIdentity>,
    ) {
        let mut state = self.state.lock();
        let Some(key) = state.runs.get(&run.run_id).cloned() else {
            return;
        };
        let Some(record) = state.records.get_mut(&key) else {
            return;
        };
        if record.status.run == *run
            && record.catalogue_identity.is_none()
            && record
                .resource
                .as_ref()
                .is_some_and(|held| Arc::ptr_eq(held, resource))
        {
            record.catalogue_identity = Some(identity);
        }
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    fn codex_catalogue_runs(
        &self,
        caller: &CallerIdentity,
        profile_id: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<VerifiedCatalogueRun>, SafeError> {
        let candidates = {
            let state = self.state.lock();
            self.authorize(&state, caller)?;
            state
                .records
                .values()
                .filter_map(|record| {
                    if record.owner != *caller || !record.catalogue_eligible() {
                        return None;
                    }
                    let source = record.source.as_ref()?;
                    if source.cli != super::types::CliKind::Codex
                        || source.profile_id != profile_id
                        || source.profile_revision != revision
                    {
                        return None;
                    }
                    Some((
                        record.status.clone(),
                        source.clone(),
                        record.catalogue_identity.as_ref()?.clone(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let mut verified = Vec::new();
        for (status, source, identity) in candidates {
            let Ok(selection) = source.selection() else {
                continue;
            };
            if selection.root.key() != root {
                continue;
            }
            let Some(id) = identity.catalogue_primary() else {
                continue;
            };
            {
                let state = self.state.lock();
                let Ok(current) = self.checked_run_record(&state, caller, &status.run) else {
                    continue;
                };
                if current.status != status
                    || !current.catalogue_eligible()
                    || !current
                        .source
                        .as_ref()
                        .is_some_and(|held| Arc::ptr_eq(held, &source))
                    || !current
                        .catalogue_identity
                        .as_ref()
                        .is_some_and(|v| Arc::ptr_eq(v, &identity))
                {
                    continue;
                }
            }
            if identity.catalogue_primary() != Some(id) {
                continue;
            }
            verified.push(VerifiedCatalogueRun {
                native: CatalogueNativeId::Codex(id),
                project: selection
                    .project
                    .as_ref()
                    .map(|project| project.key().to_owned()),
                status,
            });
        }
        self.check_caller(caller)?;
        Ok(verified)
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn record_opencode_catalogue_adoption(
        &self,
        run: &RunKey,
        resource: &Arc<R>,
        identity: Arc<super::launch_service::OpencodeCatalogueIdentity>,
    ) {
        let mut state = self.state.lock();
        let Some(key) = state.runs.get(&run.run_id).cloned() else {
            return;
        };
        let Some(record) = state.records.get_mut(&key) else {
            return;
        };
        if record.status.run == *run
            && record.catalogue_opencode.is_none()
            && record
                .resource
                .as_ref()
                .is_some_and(|held| Arc::ptr_eq(held, resource))
        {
            record.catalogue_opencode = Some(identity);
        }
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    fn opencode_catalogue_runs(
        &self,
        caller: &CallerIdentity,
        profile_id: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<VerifiedCatalogueRun>, SafeError> {
        let candidates = {
            let state = self.state.lock();
            self.authorize(&state, caller)?;
            state
                .records
                .values()
                .filter_map(|record| {
                    if record.owner != *caller || !record.catalogue_eligible() {
                        return None;
                    }
                    let source = record.source.as_ref()?;
                    if source.cli != super::types::CliKind::Opencode
                        || source.profile_id != profile_id
                        || source.profile_revision != revision
                    {
                        return None;
                    }
                    Some((
                        record.status.clone(),
                        source.clone(),
                        record.catalogue_opencode.as_ref()?.clone(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let mut verified = Vec::new();
        for (status, source, identity) in candidates {
            let Some((source_key, project_key)) = identity.current_keys() else {
                continue;
            };
            if root != source_key || !source.selection().is_ok_and(|s| s.root.key() == root) {
                continue;
            }
            let state = self.state.lock();
            let Ok(current) = self.checked_run_record(&state, caller, &status.run) else {
                continue;
            };
            if current.status != status
                || !current.catalogue_eligible()
                || !current
                    .source
                    .as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, &source))
                || !current
                    .catalogue_opencode
                    .as_ref()
                    .is_some_and(|v| Arc::ptr_eq(v, &identity))
            {
                continue;
            }
            verified.push(VerifiedCatalogueRun {
                native: CatalogueNativeId::Opencode(identity.native_session_id.clone()),
                project: Some(project_key.to_owned()),
                status,
            });
        }
        self.check_caller(caller)?;
        Ok(verified)
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn record_claude_catalogue_adoption(
        &self,
        run: &RunKey,
        resource: &Arc<R>,
        identity: Arc<super::launch_service::ClaudeCatalogueIdentity>,
    ) {
        let mut state = self.state.lock();
        let Some(key) = state.runs.get(&run.run_id).cloned() else {
            return;
        };
        let Some(record) = state.records.get_mut(&key) else {
            return;
        };
        if record.status.run == *run
            && record.catalogue_claude.is_none()
            && record
                .resource
                .as_ref()
                .is_some_and(|held| Arc::ptr_eq(held, resource))
        {
            record.catalogue_claude = Some(identity);
        }
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    fn claude_catalogue_runs(
        &self,
        caller: &CallerIdentity,
        profile_id: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<VerifiedCatalogueRun>, SafeError> {
        let candidates = {
            let state = self.state.lock();
            self.authorize(&state, caller)?;
            state
                .records
                .values()
                .filter_map(|record| {
                    if record.owner != *caller || !record.catalogue_eligible() {
                        return None;
                    }
                    let source = record.source.as_ref()?;
                    if source.cli != super::types::CliKind::Claude
                        || source.profile_id != profile_id
                        || source.profile_revision != revision
                    {
                        return None;
                    }
                    Some((
                        record.status.clone(),
                        source.clone(),
                        record.catalogue_claude.as_ref()?.clone(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let mut verified = Vec::new();
        for (status, source, identity) in candidates {
            let Some((id, project_key)) = identity.current() else {
                continue;
            };
            if !source.selection().is_ok_and(|s| s.root.key() == root) {
                continue;
            }
            let state = self.state.lock();
            let Ok(current) = self.checked_run_record(&state, caller, &status.run) else {
                continue;
            };
            if !identity.is_current()
                || current.status != status
                || !current.catalogue_eligible()
                || !current
                    .source
                    .as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, &source))
                || !current
                    .catalogue_claude
                    .as_ref()
                    .is_some_and(|v| Arc::ptr_eq(v, &identity))
            {
                continue;
            }
            verified.push(VerifiedCatalogueRun {
                native: CatalogueNativeId::Claude(id),
                project: Some(project_key.to_owned()),
                status,
            });
        }
        self.check_caller(caller)?;
        Ok(verified)
    }

    // Read-only callers still receive only display facts, never a native run receipt.
    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn codex_catalogue_lifecycle(
        &self,
        caller: &CallerIdentity,
        profile: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<(uuid::Uuid, LaunchPhase)>, SafeError> {
        Ok(self
            .codex_catalogue_runs(caller, profile, revision, root)?
            .into_iter()
            .filter_map(|row| match row.native {
                CatalogueNativeId::Codex(id) => Some((id, row.status.phase)),
                _ => None,
            })
            .collect())
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn opencode_catalogue_lifecycle(
        &self,
        caller: &CallerIdentity,
        profile: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<(String, String, LaunchPhase)>, SafeError> {
        Ok(self
            .opencode_catalogue_runs(caller, profile, revision, root)?
            .into_iter()
            .filter_map(|row| match row.native {
                CatalogueNativeId::Opencode(id) => Some((id, row.project?, row.status.phase)),
                _ => None,
            })
            .collect())
    }

    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn claude_catalogue_lifecycle(
        &self,
        caller: &CallerIdentity,
        profile: &str,
        revision: WireU64,
        root: &str,
    ) -> Result<Vec<(uuid::Uuid, String, LaunchPhase)>, SafeError> {
        Ok(self
            .claude_catalogue_runs(caller, profile, revision, root)?
            .into_iter()
            .filter_map(|row| match row.native {
                CatalogueNativeId::Claude(id) => Some((id, row.project?, row.status.phase)),
                _ => None,
            })
            .collect())
    }

    /// Only accepts a provider identity and a physical project resolved by
    /// the desktop from its fresh scoped history, never a phone argument.
    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn resolve_original_management_run(
        &self,
        caller: &CallerIdentity,
        profile: &str,
        revision: WireU64,
        root: &str,
        project: &str,
        native: &CatalogueNativeId,
    ) -> Result<LaunchStatus, SafeError> {
        let candidates = match native {
            CatalogueNativeId::Codex(_) => {
                self.codex_catalogue_runs(caller, profile, revision, root)
            }
            CatalogueNativeId::Opencode(_) => {
                self.opencode_catalogue_runs(caller, profile, revision, root)
            }
            CatalogueNativeId::Claude(_) => {
                self.claude_catalogue_runs(caller, profile, revision, root)
            }
        }?;
        let selected = unique_management_run(candidates, native, project)
            .ok_or_else(|| error("REMOTE_MANAGEMENT_TARGET_UNAVAILABLE"))?;
        let state = self.state.lock();
        let record = self.checked_run_record(&state, caller, &selected.run)?;
        if record.status != selected || !record.catalogue_eligible() {
            return Err(error("REMOTE_MANAGEMENT_TARGET_UNAVAILABLE"));
        }
        if matches!(native, CatalogueNativeId::Claude(_))
            && !record
                .catalogue_claude
                .as_ref()
                .is_some_and(|identity| identity.is_current())
        {
            return Err(error("REMOTE_MANAGEMENT_TARGET_UNAVAILABLE"));
        }
        Ok(selected)
    }

    /// Called by a backend waiter, never directly by a WebView.
    pub(crate) fn mark_exited(&self, run: &RunKey) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        let key = state
            .runs
            .get(&run.run_id)
            .cloned()
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        let record = state.records.get_mut(&key).expect("run index is retained");
        if record.status.run != *run {
            return Err(error("STALE_GENERATION"));
        }
        match record.status.phase {
            LaunchPhase::Starting | LaunchPhase::Running | LaunchPhase::Indeterminate => {
                record.transition(LaunchPhase::Exited, None);
            }
            LaunchPhase::Exited => {}
            _ => return Err(error("RUN_STATE_CONFLICT")),
        }
        Ok(())
    }

    /// The waiter/drain owner calls this after it is safe to release handles.
    /// It is deliberately distinct from process exit and leaves the replay record.
    pub(crate) fn retire(&self, run: &RunKey) -> Result<(), SafeError> {
        let resource = {
            let mut state = self.state.lock();
            let key = state
                .runs
                .get(&run.run_id)
                .cloned()
                .ok_or_else(|| error("RUN_NOT_FOUND"))?;
            let record = state.records.get_mut(&key).expect("run index is retained");
            if record.status.run != *run {
                return Err(error("STALE_GENERATION"));
            }
            if record.in_flight
                || !matches!(
                    record.status.phase,
                    LaunchPhase::Exited | LaunchPhase::Failed | LaunchPhase::Cancelled
                )
            {
                return Err(error("RUN_NOT_READY"));
            }
            record.retired = true;
            record.resource.take()
        };
        // Destructors may block or call back into the registry; never run under its lock.
        drop(resource);
        Ok(())
    }

    /// 持有 exact active supervisor 锁且生命周期完全收口后调用；只修改元数据。
    pub(crate) fn complete_retirement(
        &self,
        run: &RunKey,
        request_id: &str,
    ) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        let key = state
            .runs
            .get(&run.run_id)
            .cloned()
            .ok_or_else(|| error("RUN_NOT_FOUND"))?;
        let record = state.records.get_mut(&key).expect("run index is retained");
        if record.status.run != *run || record.status.request_id != request_id {
            return Err(error("REQUEST_CONFLICT"));
        }
        if record.in_flight
            || !record.retired
            || record.resource.is_some()
            || record.status.phase != LaunchPhase::Exited
        {
            return Err(error("RUN_NOT_READY"));
        }
        record.eviction_ready = true;
        Ok(())
    }
}

impl<R> Ticket<'_, R> {
    pub(super) fn status(&self) -> LaunchStatus {
        // 终态可立即容量回收，最后一次读取使用 ticket 自己冻结的安全回执。
        self.finished
            .clone()
            .unwrap_or_else(|| self.registry.state.lock().records[&self.key].status.clone())
    }

    /// Linearization point: a valid owner commits to a single spawn attempt.
    pub(super) fn begin(&mut self) -> bool {
        let mut state = self.registry.state.lock();
        let owner = state.records[&self.key].owner.clone();
        let authorized = self.registry.authorize(&state, &owner).is_ok();
        let record = state
            .records
            .get_mut(&self.key)
            .expect("ticket owns retained record");
        if record.status.phase == LaunchPhase::Cancelled {
            record.in_flight = false;
            self.finished = Some(record.status.clone());
            return false;
        }
        if !authorized {
            record.transition(LaunchPhase::Cancelled, None);
            record.in_flight = false;
            record.retired = true;
            record.eviction_ready = true;
            self.finished = Some(record.status.clone());
            return false;
        }
        record.transition(LaunchPhase::Starting, None);
        true
    }

    pub(super) fn fail(mut self, failure: LaunchFailure) -> LaunchStatus {
        let mut state = self.registry.state.lock();
        let record = state
            .records
            .get_mut(&self.key)
            .expect("ticket owns retained record");
        if !matches!(
            record.status.phase,
            LaunchPhase::Exited | LaunchPhase::Cancelled
        ) {
            record.transition(LaunchPhase::Failed, Some(failure));
            record.retired = true;
            record.eviction_ready = true;
        }
        record.in_flight = false;
        self.finished = Some(record.status.clone());
        record.status.clone()
    }

    pub(super) fn complete(mut self, resource: R) -> LaunchStatus {
        self.complete_inner(resource, None)
    }
    pub(super) fn complete_with_source(
        mut self,
        resource: R,
        source: Arc<super::source_provenance::SourceProvenance>,
    ) -> LaunchStatus {
        self.complete_inner(resource, Some(source))
    }
    fn complete_inner(
        &mut self,
        resource: R,
        source: Option<Arc<super::source_provenance::SourceProvenance>>,
    ) -> LaunchStatus {
        let resource = Arc::new(resource);
        let mut state = self.registry.state.lock();
        let record = state
            .records
            .get_mut(&self.key)
            .expect("ticket owns retained record");
        record.resource = Some(resource);
        record.source = source;
        record.in_flight = false;
        if record.status.phase == LaunchPhase::Starting {
            record.transition(LaunchPhase::Running, None);
        }
        self.finished = Some(record.status.clone());
        record.status.clone()
    }
}

impl<R> Drop for Ticket<'_, R> {
    fn drop(&mut self) {
        if self.finished.is_some() {
            return;
        }
        let mut state = self.registry.state.lock();
        let record = state
            .records
            .get_mut(&self.key)
            .expect("ticket owns retained record");
        record.in_flight = false;
        match record.status.phase {
            LaunchPhase::Reserved => {
                record.transition(LaunchPhase::Failed, Some(LaunchFailure::Aborted));
                record.retired = true;
                record.eviction_ready = true;
            }
            LaunchPhase::Starting => {
                record.transition(
                    LaunchPhase::Indeterminate,
                    Some(LaunchFailure::OutcomeUnknown),
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
#[path = "../tests/native_cli_registry_capacity.rs"]
mod capacity_tests;
