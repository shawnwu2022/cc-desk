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

type RequestKey = (u64, String);

struct Record<R> {
    owner: CallerIdentity,
    fingerprint: Fingerprint,
    status: LaunchStatus,
    resource: Option<Arc<R>>,
    in_flight: bool,
    retired: bool,
}

impl<R> Record<R> {
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
    finished: bool,
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
        Ok(CallerIdentity {
            instance_id: self.instance_id.clone(),
            window_label: label.into(),
            webview_epoch: WireU64::parse(&epoch.to_string())?,
        })
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

    pub(crate) fn revoke_window(&self, caller: &CallerIdentity) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        self.authorize(&state, caller)?;
        state.active = false;
        // Existing resources stay owned. The lifecycle layer decides drain/stop;
        // revocation alone never drops a master PTY or kills an agent.
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
        let key = (caller.webview_epoch.get(), request.request_id.clone());
        let tab = (caller.webview_epoch.get(), request.tab_id.clone());
        let mut state = self.state.lock();
        self.authorize(&state, caller)?;
        if let Some(status) = self.existing_locked(&state, &key, fingerprint)? {
            return Ok(Reservation::Existing(status));
        }
        if state.records.len() >= self.capacity {
            self.evict_retired_locked(&mut state);
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
                in_flight: true,
                retired: false,
            },
        );
        state.runs.insert(request.run_id.clone(), key.clone());
        state.tabs.insert(tab, key.clone());
        Ok(Reservation::New(Ticket {
            registry: self,
            key,
            finished: false,
        }))
    }

    /// 容量压力下按插入序淘汰最老的已退休记录，绝不淘汰活跃（in_flight 或
    /// 未退休）记录；runs/tabs 索引同步移除，代次下限保留在 tab_generations。
    fn evict_retired_locked(&self, state: &mut RegistryState<R>) {
        let mut index = 0;
        while state.records.len() >= self.capacity && index < state.order.len() {
            let key = state.order[index].clone();
            let evictable = state
                .records
                .get(&key)
                .is_some_and(|record| !record.in_flight && record.retired);
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
        // reserve checks the full fingerprint, owner, run and generation. Keep a
        // new ticket alive until after publishing cancellation under the same
        // lock used by begin. Neither path calls external code under this lock.
        let ticket = match self.reserve(caller, request)? {
            Reservation::New(ticket) => Some(ticket),
            Reservation::Existing(_) => None,
        };
        let result = {
            let mut state = self.state.lock();
            self.authorize(&state, caller)?;
            let key = (caller.webview_epoch.get(), request.request_id.clone());
            let record = state
                .records
                .get_mut(&key)
                .expect("reservation is retained");
            if record.status.phase == LaunchPhase::Reserved {
                record.transition(LaunchPhase::Cancelled, None);
                record.in_flight = false;
                record.retired = true;
            }
            record.status.clone()
        };
        drop(ticket);
        Ok(result)
    }

    /// Validate that the current authenticated document owns this exact run.
    /// Unlike resource(), this remains valid after process handles are retired so
    /// a still-active document can explicitly abort an output drain.
    pub(crate) fn check_run(&self, caller: &CallerIdentity, run: &RunKey) -> Result<(), SafeError> {
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
        Ok(())
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
}

impl<R> Ticket<'_, R> {
    pub(super) fn status(&self) -> LaunchStatus {
        self.registry.state.lock().records[&self.key].status.clone()
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
            self.finished = true;
            return false;
        }
        if !authorized {
            record.transition(LaunchPhase::Cancelled, None);
            record.in_flight = false;
            record.retired = true;
            self.finished = true;
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
        }
        record.in_flight = false;
        self.finished = true;
        record.status.clone()
    }

    pub(super) fn complete(mut self, resource: R) -> LaunchStatus {
        let resource = Arc::new(resource);
        let mut state = self.registry.state.lock();
        let record = state
            .records
            .get_mut(&self.key)
            .expect("ticket owns retained record");
        record.resource = Some(resource);
        record.in_flight = false;
        if record.status.phase == LaunchPhase::Starting {
            record.transition(LaunchPhase::Running, None);
        }
        self.finished = true;
        record.status.clone()
    }
}

impl<R> Drop for Ticket<'_, R> {
    fn drop(&mut self) {
        if self.finished {
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
mod capacity_tests {
    use super::*;
    use crate::cli::types::{CliKind, LaunchAction};

    fn caller(registry: &RunRegistry<usize>) -> CallerIdentity {
        registry.activate_window("main").unwrap()
    }

    fn request(id: &str, tab: &str, run: &str, generation: u32) -> LaunchRequest {
        LaunchRequest {
            request_id: id.into(),
            tab_id: tab.into(),
            run_id: run.into(),
            generation,
            profile_id: "profile-test".into(),
            expected_profile_revision: WireU64::parse("1").unwrap(),
            cli: CliKind::Codex,
            launch_cwd: "cwd".into(),
            action: LaunchAction::Raw {
                argv: vec!["private-argv".into()],
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        }
    }

    fn running(
        registry: &RunRegistry<usize>,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> LaunchStatus {
        match registry.reserve(caller, request).unwrap() {
            Reservation::New(mut ticket) => {
                assert!(ticket.begin());
                ticket.complete(1usize)
            }
            Reservation::Existing(status) => status,
        }
    }

    fn retired(
        registry: &RunRegistry<usize>,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> LaunchStatus {
        let status = running(registry, caller, request);
        registry.mark_exited(&status.run).unwrap();
        registry.retire(&status.run).unwrap();
        status
    }

    // 容量满时按最老优先淘汰非活跃记录，新 launch 注册成功。
    #[test]
    fn RegistryCapacity_EvictsOldestRetiredForNewLaunch_001() {
        let registry = RunRegistry::new(2);
        let caller = caller(&registry);
        retired(
            &registry,
            &caller,
            &request("request-1", "tab-1", "run-1", 1),
        );
        running(
            &registry,
            &caller,
            &request("request-2", "tab-2", "run-2", 1),
        );

        let third = running(
            &registry,
            &caller,
            &request("request-3", "tab-1", "run-3", 2),
        );
        assert_eq!(third.run.run_id, "run-3");
        assert_eq!(
            registry.status(&caller, "request-1").unwrap_err().code,
            "LAUNCH_NOT_FOUND"
        );
        assert!(registry.status(&caller, "request-2").is_ok());
        assert!(registry.status(&caller, "request-3").is_ok());
    }

    // 活跃记录即使最老也绝不淘汰；满员且无可淘汰记录时仍然拒绝。
    #[test]
    fn RegistryCapacity_NeverEvictsActiveRun_002() {
        let registry = RunRegistry::new(2);
        let caller = caller(&registry);
        running(
            &registry,
            &caller,
            &request("request-1", "tab-1", "run-1", 1),
        );
        retired(
            &registry,
            &caller,
            &request("request-2", "tab-2", "run-2", 1),
        );

        running(
            &registry,
            &caller,
            &request("request-3", "tab-3", "run-3", 1),
        );
        assert!(registry.status(&caller, "request-1").is_ok());
        assert_eq!(
            registry.status(&caller, "request-2").unwrap_err().code,
            "LAUNCH_NOT_FOUND"
        );
        let full = match registry.reserve(&caller, &request("request-4", "tab-4", "run-4", 1)) {
            Err(failure) => failure,
            Ok(_) => panic!("full registry must reject without an evictable record"),
        };
        assert_eq!(full.code, "REGISTRY_CAPACITY");
    }

    // 失败/取消墓碑同样可淘汰；淘汰后同 tab 的代次下限保持单调。
    #[test]
    fn RegistryCapacity_EvictedTombstoneKeepsGenerationFloor_003() {
        let registry = RunRegistry::new(3);
        let caller = caller(&registry);
        retired(
            &registry,
            &caller,
            &request("request-1", "tab-1", "run-1", 5),
        );
        retired(
            &registry,
            &caller,
            &request("request-2", "tab-2", "run-2", 1),
        );
        running(
            &registry,
            &caller,
            &request("request-3", "tab-3", "run-3", 1),
        );

        let stale = match registry.reserve(&caller, &request("request-4", "tab-1", "run-4", 5)) {
            Err(failure) => failure,
            Ok(_) => panic!("evicted tombstone generation floor must reject stale reuse"),
        };
        assert_eq!(stale.code, "STALE_GENERATION");
        // 只淘汰了一个名额，request-2 不应被顺带清除。
        assert!(registry.status(&caller, "request-2").is_ok());
        let fresh = running(
            &registry,
            &caller,
            &request("request-4", "tab-1", "run-4", 6),
        );
        assert_eq!(fresh.run.generation, 6);
    }
}
