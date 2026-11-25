//! 只接收后端已批准的 Rust 能力；实际批准来源仍由独立可信 owner 提供。
#![allow(
    dead_code,
    reason = "The real approved Host capability supplier remains an independent product gate"
)]
use crate::{
    cli::native_runtime::NativeRuntime,
    remote_host::RemoteHostService,
    run_supervisor::{NativeRunAdmission, NativeRunNetworkOrigin},
};
use cc_desk_remote_record::grant::DataRole;
use cc_desk_remote_security::{
    self as security, BrowserProfile, GrantClock, GrantError, GrantGate, HostSnapshot,
    LocalIdentity, NativeGrantPolicy, P256Spki, PeerPin, RunningChannel,
};
use cc_desk_remote_transport::{self as transport, wss::RelayAdmissionIdentity};
use parking_lot::Mutex;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Weak,
    },
    time::Duration,
};
use tokio::{sync::watch, time::Instant};
#[path = "remote_host_approval_stop.rs"]
mod approval_stop;
#[path = "remote_host_network/delegated.rs"]
pub(crate) mod delegated;
#[path = "remote_host_network_wire.rs"]
mod wire;
pub(crate) use approval_stop::NativeApprovalStop;

thread_local! { static SOURCE_SAMPLE_DEPTH: Cell<usize> = const { Cell::new(0) }; }
thread_local! { static ACTIVE_SOURCE_CLOCKS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) }; }
struct SourceClockEntry(usize);
impl SourceClockEntry {
    fn enter(identity: usize) -> Option<Self> {
        ACTIVE_SOURCE_CLOCKS.with(|active| {
            let mut active = active.borrow_mut();
            if active.contains(&identity) {
                return None;
            }
            active.push(identity);
            Some(Self(identity))
        })
    }
}
impl Drop for SourceClockEntry {
    fn drop(&mut self) {
        ACTIVE_SOURCE_CLOCKS.with(|active| {
            let removed = active.borrow_mut().pop();
            debug_assert_eq!(removed, Some(self.0));
        });
    }
}
struct SourceSampleGuard;
impl SourceSampleGuard {
    fn enter() -> Self {
        SOURCE_SAMPLE_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
    fn active() -> bool {
        SOURCE_SAMPLE_DEPTH.with(|depth| depth.get() != 0)
    }
}
impl Drop for SourceSampleGuard {
    fn drop(&mut self) {
        SOURCE_SAMPLE_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// 必须由独立可信的后端批准 owner 提供，解析或构造公开值不产生批准。
pub(crate) struct ApprovedHostSource {
    pub(crate) clock: Arc<dyn GrantClock>,
    pub(crate) revoked: watch::Receiver<bool>,
    pub(crate) expires_host_ms: u64,
}
/// 仅原网络任务在原注册来源和原 setup end 下构造，不持 service/run。
#[cfg(windows)]
pub(crate) struct NetworkSigningScope {
    authority: SigningAuthority,
    end: std::time::Instant,
}
#[cfg(windows)]
enum SigningAuthority {
    Original(NativeApprovalStop),
    Delegated(
        Box<crate::remote_host_approval::unattended::final_approval::DelegatedSigningAuthorization>,
    ),
}
#[cfg(windows)]
impl NetworkSigningScope {
    fn original(approval: NativeApprovalStop, end: std::time::Instant) -> Self {
        Self {
            authority: SigningAuthority::Original(approval),
            end,
        }
    }
    pub(super) fn delegated(
        authority: crate::remote_host_approval::unattended::final_approval::DelegatedSigningAuthorization,
        setup_end: std::time::Instant,
    ) -> Self {
        let end = authority.end().min(setup_end);
        Self {
            authority: SigningAuthority::Delegated(Box::new(authority)),
            end,
        }
    }
    pub(crate) fn matches(
        &self,
        token: &Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
    ) -> bool {
        match &self.authority {
            SigningAuthority::Original(approval) => {
                approval.matches_native(token, cap) && !approval.is_invalid()
            }
            SigningAuthority::Delegated(authority) => authority.matches(token, cap),
        }
    }
    pub(crate) fn end(&self) -> std::time::Instant {
        self.end
    }
    #[cfg(test)]
    pub(crate) fn test_new(
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
        end: std::time::Instant,
    ) -> Self {
        Self::original(NativeApprovalStop::new(token, cap), end)
    }
}
#[cfg(test)]
type TestAcceptedGate = (
    tokio::sync::oneshot::Sender<()>,
    Option<tokio::sync::oneshot::Receiver<()>>,
);
pub(crate) struct ApprovedHostInputs {
    #[cfg(windows)]
    pub(crate) stages: Option<crate::native_crypto::host::HostSigningStages>,
    pub(crate) address: SocketAddr,
    pub(crate) relay: RelayAdmissionIdentity,
    pub(crate) tunnel: [u8; 16],
    pub(crate) relay_mode: transport::wss::RelayMode,
    pub(crate) local: LocalIdentity,
    pub(crate) phone_pin: PeerPin,
    pub(crate) binding: HostSnapshot,
    pub(crate) browser: BrowserProfile,
    pub(crate) issuer: P256Spki,
    pub(crate) principal: [u8; 32],
    pub(crate) policy: NativeGrantPolicy,
    pub(crate) source: Arc<ApprovedHostSource>,
    pub(crate) setup_budget: Duration,
    pub(crate) io_budget: Duration,
    #[cfg(test)]
    pub(crate) outer_accepted: Option<Box<TestAcceptedGate>>,
    #[cfg(test)]
    pub(crate) termination: Option<tokio::sync::oneshot::Sender<TestAttemptEnd>>,
}
/// 每次只转移新的签名能力；此类型没有可覆盖批准元数据的字段。
pub(crate) struct HostAttemptMaterials {
    #[cfg(windows)]
    pub(crate) stages: Option<crate::native_crypto::host::HostSigningStages>,
    pub(crate) local: LocalIdentity,
    pub(crate) relay: RelayAdmissionIdentity,
    #[cfg(test)]
    pub(crate) outer_accepted: Option<Box<TestAcceptedGate>>,
    #[cfg(test)]
    pub(crate) termination: Option<tokio::sync::oneshot::Sender<TestAttemptEnd>>,
}
struct HostSourceDescription {
    address: SocketAddr,
    tunnel: [u8; 16],
    relay_mode: transport::wss::RelayMode,
    phone_pin: PeerPin,
    binding: HostSnapshot,
    browser: BrowserProfile,
    issuer: P256Spki,
    principal: [u8; 32],
    policy: NativeGrantPolicy,
    source: Arc<ApprovedHostSource>,
    setup_budget: Duration,
    io_budget: Duration,
}
impl HostSourceDescription {
    fn inputs(&self, material: HostAttemptMaterials) -> ApprovedHostInputs {
        ApprovedHostInputs {
            #[cfg(windows)]
            stages: material.stages,
            address: self.address,
            relay: material.relay,
            tunnel: self.tunnel,
            relay_mode: self.relay_mode,
            local: material.local,
            phone_pin: self.phone_pin.clone(),
            binding: self.binding.clone(),
            browser: self.browser.clone(),
            issuer: self.issuer.clone(),
            principal: self.principal,
            policy: NativeGrantPolicy {
                scope: self.policy.scope.clone(),
                role: self.policy.role,
                duration: self.policy.duration,
            },
            source: self.source.clone(),
            setup_budget: self.setup_budget,
            io_budget: self.io_budget,
            #[cfg(test)]
            outer_accepted: material.outer_accepted,
            #[cfg(test)]
            termination: material.termination,
        }
    }
}
struct RegisteredHostSource {
    description: HostSourceDescription,
    #[cfg(windows)]
    formal_observer_endpoint:
        Option<crate::remote_host_controller::startup::configuration::FormalObserverEndpoint>,
    origin: NativeRunNetworkOrigin,
    live: AtomicBool,
    approval: Option<NativeApprovalStop>,
}
impl RegisteredHostSource {
    fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && self.approval.as_ref().is_none_or(|stop| !stop.is_invalid())
    }
}
/// The original registry entry and the wire-owned grant must move together.
/// This type is never constructed from frontend input or delegated wire.
pub(crate) struct OriginalObserverHandoff {
    registered: Arc<RegisteredHostSource>,
    grant: wire::OriginalObserverGrant,
}
impl OriginalObserverHandoff {
    pub(crate) fn into_parts(
        self,
        admission: &NativeRunAdmission,
    ) -> Result<(Arc<RunningChannel>, NativeApprovalStop, Instant), HostNetworkError> {
        if !self.registered.is_live() || !self.registered.origin.matches(admission) {
            return Err(HostNetworkError::Authority);
        }
        Ok(self.grant.into_parts())
    }
}
/// 只能由本服务接受原 native 批准能力后取得；不是可序列化授权或 Ready。
#[derive(Clone)]
pub(crate) struct HostSourceToken(Weak<RegisteredHostSource>);
#[derive(Default)]
pub(crate) struct HostSourceSlot {
    registration: Option<Arc<RegisteredHostSource>>,
    fresh: Option<HostAttemptMaterials>,
    closed: bool,
    preflight: bool,
    releasing: bool,
    material_unknown: bool,
}
impl HostSourceSlot {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed
            || self
                .registration
                .as_ref()
                .is_some_and(|source| !source.is_live())
    }
    pub(crate) fn material_returned(&self) -> Option<Result<(), HostNetworkError>> {
        if self.material_unknown {
            Some(Err(HostNetworkError::Unknown))
        } else if self.preflight || self.releasing || self.fresh.is_some() {
            None
        } else {
            Some(Ok(()))
        }
    }
    pub(crate) fn close(&mut self) -> Option<HostAttemptMaterials> {
        self.closed = true;
        if let Some(registration) = &self.registration {
            registration.live.store(false, Ordering::Release);
        }
        self.drain_closed()
    }
    fn drain_closed(&mut self) -> Option<HostAttemptMaterials> {
        // 时钟调用栈内只撤销资格；原 begin 返回或 owned task 终态再释放材料。
        if self.closed && !SourceSampleGuard::active() && !self.releasing {
            let material = self.fresh.take();
            if material.is_some() {
                self.releasing = true;
            }
            material
        } else {
            None
        }
    }
}
enum SourceMaterialPhase {
    Preflight,
    Release,
}
struct SourceMaterialHold<'a> {
    owner: &'a RemoteHostService,
    phase: SourceMaterialPhase,
    returned: bool,
}
impl SourceMaterialHold<'_> {
    fn complete(mut self) {
        let mut state = self.owner.state.lock();
        match self.phase {
            SourceMaterialPhase::Preflight => state.source_slot.preflight = false,
            SourceMaterialPhase::Release => state.source_slot.releasing = false,
        }
        self.returned = true;
    }
}
impl Drop for SourceMaterialHold<'_> {
    fn drop(&mut self) {
        if !self.returned {
            let mut state = self.owner.state.lock();
            state.source_slot.material_unknown = true;
            match self.phase {
                SourceMaterialPhase::Preflight => state.source_slot.preflight = false,
                SourceMaterialPhase::Release => state.source_slot.releasing = false,
            }
            drop(state);
            // 析构没有正常返回；只记录 Unknown，不在 unwind 栈通知任何 owner。
            self.owner.network.latch_unknown();
        }
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TestAttemptCause {
    Setup,
    AttemptStop,
    SourceWatch,
    Approval,
    Lease,
    Channel,
}
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct TestAttemptEnd {
    pub(crate) cause: TestAttemptCause,
    pub(crate) selected_at: Instant,
    pub(crate) lease_end: Option<Instant>,
    pub(crate) mux: Option<transport::TestOnlyMuxEnd>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostNetworkError {
    Bounds,
    Locked,
    Closed,
    Authority,
    Deadline,
    Relay,
    Security,
    Grant,
    Handoff,
    Unknown,
}
struct ContinuityChecks(Box<dyn Fn() -> bool + Send + Sync>);
struct SourceClock {
    approval: Option<NativeApprovalStop>,
    source: Arc<ApprovedHostSource>,
    checks: Mutex<Weak<ContinuityChecks>>,
    sampled: Mutex<Option<u64>>,
    failed: AtomicBool,
    unknown: Arc<AtomicBool>,
}
impl GrantClock for SourceClock {
    fn now_ms(&self) -> Result<u64, GrantError> {
        let _sampling = SourceSampleGuard::enter();
        // 同线程同一 clock 在任何锁之前拒绝；不同 clock 和其他线程仍可正常采样。
        let Some(_entry) = SourceClockEntry::enter(self as *const Self as usize) else {
            self.revoke_approval();
            self.failed.store(true, Ordering::Release);
            return Err(GrantError::Clock);
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sample())) {
            Ok(result) => {
                if result.is_err() {
                    self.revoke_approval();
                }
                result
            }
            Err(_) => {
                self.revoke_approval();
                self.failed.store(true, Ordering::Release);
                self.unknown.store(true, Ordering::Release);
                Err(GrantError::Clock)
            }
        }
    }
}
impl SourceClock {
    fn revoke_approval(&self) {
        if let Some(approval) = &self.approval {
            approval.revoke();
        }
    }
    fn sample(&self) -> Result<u64, GrantError> {
        if self.failed.load(Ordering::Acquire) || self.unknown.load(Ordering::Acquire) {
            return Err(GrantError::Revoked);
        }
        let fail = |error| {
            self.failed.store(true, Ordering::Release);
            Err(error)
        };
        if *self.source.revoked.borrow() || !matches!(self.source.revoked.has_changed(), Ok(false))
        {
            return fail(GrantError::Revoked);
        }
        let checks = self.checks.lock().upgrade().ok_or(GrantError::Revoked)?;
        if !(checks.0)() {
            return fail(GrantError::Revoked);
        }
        // 采样和发布共用短锁；不能把并发旧样本的晚提交误当源时钟回退。
        let mut sampled = self.sampled.lock();
        let now = match self.source.clock.now_ms() {
            Ok(now) => now,
            Err(error) => return fail(error),
        };
        if self.failed.load(Ordering::Acquire) {
            return Err(GrantError::Revoked);
        }
        if *self.source.revoked.borrow() || !matches!(self.source.revoked.has_changed(), Ok(false))
        {
            return fail(GrantError::Revoked);
        }
        // provider 可以同步撤销原批准；发布本次样本前重验同一纯活性检查。
        if !(checks.0)() {
            return fail(GrantError::Revoked);
        }
        if sampled.is_some_and(|last| now < last) {
            return fail(GrantError::Clock);
        }
        if now >= self.source.expires_host_ms {
            return fail(GrantError::Expired);
        }
        *sampled = Some(now);
        Ok(now)
    }
}
struct ClockOrigin {
    origin: NativeRunNetworkOrigin,
    clock: Arc<SourceClock>,
}
#[derive(Default)]
struct NetworkState {
    closing: bool,
    responsibility_started: bool,
    unknown: bool,
    clock: Option<ClockOrigin>,
    attempts: Vec<Arc<HostNetworkAttempt>>,
}
#[derive(Default)]
pub(crate) struct HostNetworkCustody {
    state: Mutex<NetworkState>,
    unknown_latched: Arc<AtomicBool>,
}
pub(crate) struct HostNetworkAttempt {
    custody: Weak<HostNetworkCustody>,
    stop: watch::Sender<bool>,
    admitted: watch::Receiver<Option<Result<(), HostNetworkError>>>,
    join:
        tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<Result<(), HostNetworkError>>>>,
    terminal: Arc<AtomicU8>,
}
impl HostNetworkAttempt {
    fn known_joined(&self) -> bool {
        self.terminal.load(Ordering::Acquire) == 1
            && self.join.try_lock().is_ok_and(|slot| slot.is_none())
    }
    pub(crate) fn request_close(&self) {
        self.stop.send_if_modified(|stopped| {
            if *stopped {
                false
            } else {
                *stopped = true;
                true
            }
        });
    }
    pub(crate) async fn wait_admitted(&self) -> Result<(), HostNetworkError> {
        let mut state = self.admitted.clone();
        loop {
            if let Some(result) = *state.borrow_and_update() {
                return result;
            }
            tokio::select! {
                changed = state.changed() => if changed.is_err() { return Err(HostNetworkError::Unknown); },
                known = self.join() => return if known { (*state.borrow()).unwrap_or(Err(HostNetworkError::Unknown)) } else { Err(HostNetworkError::Unknown) },
            }
        }
    }
    async fn join(&self) -> bool {
        let mut slot = self.join.lock().await;
        #[cfg(test)]
        let mut consumed_original = false;
        if let Some(task) = slot.as_mut() {
            let known = !matches!(task.await, Err(_) | Ok(Err(HostNetworkError::Unknown)));
            self.terminal
                .store(if known { 1 } else { 2 }, Ordering::Release);
            *slot = None;
            #[cfg(test)]
            {
                consumed_original = true;
            }
        }
        let known = self.terminal.load(Ordering::Acquire) == 1;
        drop(slot);
        #[cfg(test)]
        eprintln!("TEST_ONLY_NETWORK_JOIN consumed_original={consumed_original} known={known}");
        if !known {
            if let Some(custody) = self.custody.upgrade() {
                custody.mark_unknown();
            }
        }
        known
    }
    pub(crate) async fn wait_closed(&self) -> Result<(), HostNetworkError> {
        if self.join().await {
            Ok(())
        } else {
            Err(HostNetworkError::Unknown)
        }
    }
}
#[derive(Clone, Copy)]
enum Connector {
    WebPki,
    #[cfg(test)]
    PublicLoopbackFixture,
}
struct NetworkStart {
    end: Instant,
    registration: Option<Arc<RegisteredHostSource>>,
}
impl HostNetworkCustody {
    pub(crate) fn sealed_never_remote(&self) -> bool {
        let state = self.state.lock();
        state.closing
            && !state.responsibility_started
            && !state.unknown
            && !self.unknown_latched.load(Ordering::Acquire)
            && state.attempts.is_empty()
    }
    pub(crate) fn latch_unknown(&self) {
        self.unknown_latched.store(true, Ordering::Release);
    }
    fn open_state(&self) -> Result<parking_lot::MutexGuard<'_, NetworkState>, HostNetworkError> {
        let mut state = self.state.lock();
        let mut busy = false;
        let unknown = self.unknown_latched.load(Ordering::Acquire)
            || state.attempts.iter().any(|attempt| {
                if attempt.terminal.load(Ordering::Acquire) == 2 {
                    return true;
                }
                match attempt.join.try_lock() {
                    Ok(slot) => slot.as_ref().is_some_and(|task| {
                        task.inner().is_finished() && attempt.terminal.load(Ordering::Acquire) == 0
                    }),
                    Err(_) => {
                        busy = true;
                        false
                    }
                }
            });
        if unknown {
            state.unknown = true;
            drop(state);
            self.begin_shutdown();
            return Err(HostNetworkError::Unknown);
        }
        if state.unknown {
            return Err(HostNetworkError::Unknown);
        }
        if state.closing || busy {
            return Err(HostNetworkError::Closed);
        }
        state.responsibility_started = true;
        state.attempts.retain(|attempt| !attempt.known_joined());
        Ok(state)
    }
    fn begin(
        self: &Arc<Self>,
        service: &Arc<RemoteHostService>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        inputs: ApprovedHostInputs,
        connector: Connector,
        start: NetworkStart,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        let end = start.end;
        remaining(end)?;
        if inputs.setup_budget.is_zero()
            || inputs.setup_budget > Duration::from_secs(10)
            || inputs.io_budget.is_zero()
            || inputs.io_budget > transport::MAX_IO_BUDGET
            || inputs.tunnel == [0; 16]
        {
            return Err(HostNetworkError::Bounds);
        }
        if !admission.is_owned_by(&runtime)
            || admission.scope().map_err(|_| HostNetworkError::Authority)? != inputs.policy.scope
        {
            return Err(HostNetworkError::Authority);
        }
        let (origin, checks) = admission
            .network_checks()
            .map_err(|_| HostNetworkError::Authority)?;
        let registration = start.registration;
        if registration.as_ref().is_some_and(|registered| {
            !registered.origin.matches(&admission)
                || !Arc::ptr_eq(&registered.description.source, &inputs.source)
        }) {
            return Err(HostNetworkError::Authority);
        }
        let approval_stop = registration
            .as_ref()
            .and_then(|source| source.approval.clone());
        let live_registration = registration.clone();
        let fresh = Arc::new(ContinuityChecks(Box::new(move || {
            live_registration
                .as_ref()
                .is_none_or(|registered| registered.is_live())
                && checks()
        })));
        let (clock, continuity) = {
            let mut state = self.open_state()?;
            if state.attempts.len() >= 64 {
                return Err(HostNetworkError::Closed);
            }
            if state.attempts.iter().any(|a| a.admitted.borrow().is_none()) {
                return Err(HostNetworkError::Closed);
            }
            if let Some(existing) = &state.clock {
                if !existing.origin.matches(&admission)
                    || !Arc::ptr_eq(&existing.clock.source, &inputs.source)
                {
                    return Err(HostNetworkError::Authority);
                }
                let mut original = existing.clock.checks.lock();
                let held = original.upgrade().unwrap_or_else(|| {
                    *original = Arc::downgrade(&fresh);
                    fresh.clone()
                });
                (existing.clock.clone(), held)
            } else {
                let clock = Arc::new(SourceClock {
                    approval: approval_stop.clone(),
                    source: inputs.source.clone(),
                    checks: Mutex::new(Arc::downgrade(&fresh)),
                    sampled: Mutex::new(None),
                    failed: AtomicBool::new(false),
                    unknown: self.unknown_latched.clone(),
                });
                state.clock = Some(ClockOrigin {
                    origin,
                    clock: clock.clone(),
                });
                (clock, fresh.clone())
            }
        };
        // provider 可能唤醒其他任务，检查不能持有 service 或 slot 锁。
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clock.now_ms())) {
            Ok(result) => {
                result.map_err(|_| {
                    if clock.unknown.load(Ordering::Acquire) {
                        HostNetworkError::Unknown
                    } else {
                        HostNetworkError::Authority
                    }
                })?;
            }
            Err(_) => {
                self.mark_unknown();
                return Err(HostNetworkError::Unknown);
            }
        }
        remaining(end)?;
        if !service.network_admission_open() {
            return Err(HostNetworkError::Closed);
        }
        let (stop, stopped) = watch::channel(false);
        let (published, admitted) = watch::channel(None);
        let terminal = Arc::new(AtomicU8::new(0));
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(self),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(None),
            terminal: terminal.clone(),
        });
        let mut state = self.open_state()?;
        if state.attempts.len() >= 64
            || state.attempts.iter().any(|a| a.admitted.borrow().is_none())
        {
            return Err(HostNetworkError::Closed);
        }
        let (ready, start) = tokio::sync::oneshot::channel();
        let service = Arc::downgrade(service);
        let release_service = service.clone();
        let custody = Arc::downgrade(self);
        let task = tauri::async_runtime::spawn(async move {
            let result = if start.await.is_err() {
                Err(HostNetworkError::Closed)
            } else {
                run_attempt(
                    service,
                    runtime,
                    admission,
                    inputs,
                    connector,
                    stopped,
                    end,
                    clock,
                    continuity,
                    approval_stop,
                    registration.as_ref(),
                    &published,
                )
                .await
            };
            if result == Err(HostNetworkError::Unknown) {
                if let Some(owner) = custody.upgrade() {
                    owner.mark_unknown();
                }
            }
            if registration
                .as_ref()
                .is_some_and(|registered| !registered.is_live())
            {
                if let Some(owner) = custody.upgrade() {
                    owner.begin_shutdown();
                }
            }
            if let Some(owner) = release_service.upgrade() {
                owner.drain_closed_source_materials();
            }
            if published.borrow().is_none() {
                published.send_replace(Some(result));
            }
            terminal.store(
                if result == Err(HostNetworkError::Unknown) {
                    2
                } else {
                    1
                },
                Ordering::Release,
            );
            result
        });
        *attempt
            .join
            .try_lock()
            .expect("unpublished original attempt") = Some(task);
        state.attempts.push(attempt.clone());
        drop(state);
        let _ = ready.send(());
        Ok(attempt)
    }
    fn mark_unknown(&self) {
        self.unknown_latched.store(true, Ordering::Release);
        self.state.lock().unknown = true;
        self.begin_shutdown();
    }
    pub(crate) fn begin_shutdown(&self) {
        let attempts = {
            let mut state = self.state.lock();
            if state.closing {
                return;
            }
            state.closing = true;
            state.attempts.clone()
        };
        for attempt in attempts {
            attempt.request_close();
        }
    }
    pub(crate) async fn join_all(&self) -> bool {
        let attempts = self.state.lock().attempts.clone();
        let known = futures_util::future::join_all(attempts.iter().map(|a| a.join()))
            .await
            .into_iter()
            .all(|v| v);
        if !known {
            self.mark_unknown();
        }
        if self.unknown_latched.load(Ordering::Acquire) {
            self.mark_unknown();
        }
        known && !self.unknown_latched.load(Ordering::Acquire) && !self.state.lock().unknown
    }
}
pub(crate) struct ObserverPublicationGuard<'a> {
    registration: &'a Arc<RegisteredHostSource>,
    receipt: &'a crate::run_supervisor::NativeObserverInstallReceipt,
}
impl ObserverPublicationGuard<'_> {
    // Runs within the application publication locks. Never sample a GrantClock
    // or call any provider from this guard.
    pub(crate) fn is_current(&self, slot: &HostSourceSlot) -> bool {
        slot.registration
            .as_ref()
            .is_some_and(|registered| Arc::ptr_eq(registered, self.registration))
            && self.registration.is_live()
            && !*self.registration.description.source.revoked.borrow()
            && matches!(
                self.registration.description.source.revoked.has_changed(),
                Ok(false)
            )
            && self.receipt.is_current_for_publication()
    }
}

/// The separate, non-default acceptance build never reaches beyond local loopback.
pub(crate) fn local_observer_address(address: SocketAddr) -> bool {
    cfg!(all(
        test,
        windows,
        feature = "remote-observer-local-acceptance"
    )) && address.port() != 0
        && address.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

fn observer_route_allowed(
    connector: Connector,
    address: SocketAddr,
    origin: &cc_desk_remote_record::appbind::CanonicalOrigin,
    #[cfg(windows)] endpoint: Option<
        &crate::remote_host_controller::startup::configuration::FormalObserverEndpoint,
    >,
) -> bool {
    match connector {
        Connector::WebPki => {
            #[cfg(windows)]
            {
                cfg!(feature = "remote-observer")
                    && endpoint.is_some_and(|endpoint| endpoint.matches(address, origin))
            }
            #[cfg(not(windows))]
            {
                let _ = (address, origin);
                false
            }
        }
        #[cfg(test)]
        Connector::PublicLoopbackFixture => local_observer_address(address),
    }
}

fn remaining(end: Instant) -> Result<Duration, HostNetworkError> {
    end.checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(HostNetworkError::Deadline)
}
fn publish_handoff(
    owner: &RemoteHostService,
    stopped: &watch::Receiver<bool>,
    published: &watch::Sender<Option<Result<(), HostNetworkError>>>,
    clock: &dyn GrantClock,
    end: Instant,
) -> Result<(), HostNetworkError> {
    remaining(end)?;
    if !owner.network_admission_open()
        || *stopped.borrow()
        || !matches!(stopped.has_changed(), Ok(false))
    {
        return Err(HostNetworkError::Closed);
    }
    clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
    owner.commit_network_handoff(published, stopped, end)
}
async fn guarded<F: Future<Output = Result<(), HostNetworkError>>>(
    work: F,
) -> Result<(), HostNetworkError> {
    tokio::pin!(work);
    std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work.as_mut().poll(cx))) {
            Ok(poll) => poll,
            Err(_) => std::task::Poll::Ready(Err(HostNetworkError::Unknown)),
        }
    })
    .await
}
#[allow(clippy::too_many_arguments)]
async fn run_attempt(
    service: Weak<RemoteHostService>,
    runtime: Arc<NativeRuntime>,
    admission: NativeRunAdmission,
    inputs: ApprovedHostInputs,
    connector: Connector,
    mut stopped: watch::Receiver<bool>,
    end: Instant,
    clock: Arc<SourceClock>,
    _continuity: Arc<ContinuityChecks>,
    approval_stop: Option<NativeApprovalStop>,
    registration: Option<&Arc<RegisteredHostSource>>,
    published: &watch::Sender<Option<Result<(), HostNetworkError>>>,
) -> Result<(), HostNetworkError> {
    #[cfg(test)]
    let mut inputs = inputs;
    #[cfg(test)]
    let termination = inputs.termination.take();
    let observer = inputs.policy.role == DataRole::Observer;
    if observer
        && !observer_route_allowed(
            connector,
            inputs.address,
            &inputs.binding.origin,
            #[cfg(windows)]
            registration.and_then(|source| source.formal_observer_endpoint.as_ref()),
        )
    {
        return Err(HostNetworkError::Locked);
    }
    #[cfg(all(
        windows,
        any(
            feature = "remote-observer",
            all(test, feature = "remote-observer-local-acceptance")
        )
    ))]
    let mut observer_owner = None;
    let source = inputs.source.clone();
    let mut wire = wire::NetworkWire::new();
    let mut source_stop = source.revoked.clone();
    let mut source_end = None;
    let setup_allowed = !*stopped.borrow() && stopped.has_changed().is_ok();
    let mut original = Some(admission);
    let publication_stop = stopped.clone();
    let work = async {
        if !setup_allowed {
            return Err(HostNetworkError::Closed);
        }
        let observer_source = if observer {
            let registered = registration.ok_or(HostNetworkError::Authority)?;
            let admission = original.as_ref().ok_or(HostNetworkError::Authority)?;
            if !registered.is_live()
                || !Arc::ptr_eq(&registered.description.source, &source)
                || !registered.origin.matches(admission)
                || registered.description.policy.role != DataRole::Observer
            {
                return Err(HostNetworkError::Authority);
            }
            #[cfg(windows)]
            let matched = inputs.stages.as_ref().is_some_and(|stages| {
                approval_stop
                    .as_ref()
                    .is_some_and(|approval| stages.matches_approval(approval))
            });
            #[cfg(not(windows))]
            let matched = false;
            if !matched {
                return Err(HostNetworkError::Authority);
            }
            Some(registered)
        } else {
            None
        };
        let source_anchor = Instant::now();
        let now = clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
        source_end = Some(
            source_anchor
                .checked_add(Duration::from_millis(source.expires_host_ms - now))
                .ok_or(HostNetworkError::Bounds)?,
        );
        let channel = wire
            .setup(
                wire::WireInputs::from_approved(inputs),
                connector,
                clock.clone(),
                end,
                approval_stop.as_ref(),
                source.clone(),
            )
            .await?;
        remaining(end)?;
        clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
        if let Some(registered) = observer_source {
            if !registered.is_live() {
                return Err(HostNetworkError::Authority);
            }
            let approval = registered
                .approval
                .as_ref()
                .ok_or(HostNetworkError::Authority)?;
            let grant = wire.mint_original_observer_handoff(
                &registered.description.source,
                approval,
                end.min(source_end.ok_or(HostNetworkError::Bounds)?),
            )?;
            let handoff = OriginalObserverHandoff {
                registered: registered.clone(),
                grant,
            };
            #[cfg(not(all(
                windows,
                any(
                    feature = "remote-observer",
                    all(test, feature = "remote-observer-local-acceptance")
                )
            )))]
            {
                drop(handoff);
                return Err(HostNetworkError::Locked);
            }
            #[cfg(all(
                windows,
                any(
                    feature = "remote-observer",
                    all(test, feature = "remote-observer-local-acceptance")
                )
            ))]
            {
                let owner = service.upgrade().ok_or(HostNetworkError::Closed)?;
                let admission = original.take().ok_or(HostNetworkError::Authority)?;
                observer_owner = Some(
                    admission
                        .start_observer_once(handoff)
                        .map_err(|_| HostNetworkError::Handoff)?,
                );
                let receipt = observer_owner
                    .as_mut()
                    .ok_or(HostNetworkError::Handoff)?
                    .wait_install_receipt()
                    .await
                    .ok_or(HostNetworkError::Handoff)?;
                if !registered.is_live() || !receipt.is_live() {
                    return Err(HostNetworkError::Authority);
                }
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                return owner.commit_network_handoff_checked(
                    published,
                    &publication_stop,
                    end,
                    Some(ObserverPublicationGuard {
                        registration: &registered,
                        receipt: &receipt,
                    }),
                );
            }
        }
        let owned = original.take().expect("original synchronous admission");
        let owner = service.upgrade().ok_or(HostNetworkError::Closed)?;
        if let Err(rejected) = owner.adopt_admitted(owned, channel, &runtime) {
            let (_, retained) = *rejected;
            original = Some(retained);
            return Err(HostNetworkError::Handoff);
        }
        // 已交接的原 pin/Host 继续保管；关闭或失权不能再公布成功。
        publish_handoff(&owner, &publication_stop, published, clock.as_ref(), end)
    };
    let result = {
        let work = guarded(work);
        tokio::pin!(work);
        tokio::select! {biased;
            _ = stopped.changed() => Err(HostNetworkError::Closed),
            _ = source_stop.changed() => Err(HostNetworkError::Authority),
            _ = NativeApprovalStop::wait(approval_stop.as_ref()) => Err(HostNetworkError::Authority),
            _ = tokio::time::sleep_until(end) => Err(HostNetworkError::Deadline),
            result = &mut work => result,
        }
    };
    #[cfg(test)]
    let (mut cause, mut selected_at) = (TestAttemptCause::Setup, Instant::now());
    let result = if result.is_ok() {
        let expiry = source_end.expect("original source lease deadline");
        let channel = wire.running.as_ref().expect("original handed-off channel");
        tokio::select! {biased;
            _ = stopped.changed() => { #[cfg(test)] { cause = TestAttemptCause::AttemptStop; selected_at = Instant::now(); } Ok(()) },
            _ = source_stop.changed() => { #[cfg(test)] { cause = TestAttemptCause::SourceWatch; selected_at = Instant::now(); } Ok(()) },
            _ = NativeApprovalStop::wait(approval_stop.as_ref()) => { #[cfg(test)] { cause = TestAttemptCause::Approval; selected_at = Instant::now(); } Ok(()) },
            _ = tokio::time::sleep_until(expiry) => { #[cfg(test)] { cause = TestAttemptCause::Lease; selected_at = Instant::now(); } Ok(()) },
            result = channel.wait_closed_checked() => { #[cfg(test)] { cause = TestAttemptCause::Channel; selected_at = Instant::now(); } result.map(|_|()).map_err(|_|HostNetworkError::Unknown) },
        }
    } else {
        result
    };
    #[cfg(not(all(
        windows,
        any(
            feature = "remote-observer",
            all(test, feature = "remote-observer-local-acceptance")
        )
    )))]
    let wire_known = wire.close_and_join().await;
    #[cfg(all(
        windows,
        any(
            feature = "remote-observer",
            all(test, feature = "remote-observer-local-acceptance")
        )
    ))]
    let wire_known = {
        let retire_observer = async {
            match observer_owner.as_mut() {
                Some(owner) => owner.retire().await.is_ok(),
                None => true,
            }
        };
        let (wire_closed, observer_closed) = tokio::join!(wire.close_and_join(), retire_observer);
        wire_closed && observer_closed
    };
    drop(original);
    #[cfg(test)]
    if let Some(termination) = termination {
        let mux = wire
            .mux_termination
            .as_mut()
            .and_then(|receiver| receiver.try_recv().ok());
        let _ = termination.send(TestAttemptEnd {
            cause,
            selected_at,
            lease_end: source_end,
            mux,
        });
    }
    if clock.unknown.load(Ordering::Acquire) || !wire_known {
        Err(HostNetworkError::Unknown)
    } else {
        result
    }
}
impl RemoteHostService {
    pub(crate) fn register_native_source(
        &self,
        runtime: &NativeRuntime,
        admission: &NativeRunAdmission,
        inputs: ApprovedHostInputs,
    ) -> Result<HostSourceToken, Box<(HostNetworkError, ApprovedHostInputs)>> {
        self.register_approved_native_source(
            runtime,
            admission,
            inputs,
            None,
            #[cfg(windows)]
            None,
        )
    }
    pub(crate) fn register_approved_native_source(
        &self,
        runtime: &NativeRuntime,
        admission: &NativeRunAdmission,
        inputs: ApprovedHostInputs,
        approval: Option<NativeApprovalStop>,
        #[cfg(windows)] formal_observer_endpoint: Option<
            crate::remote_host_controller::startup::configuration::FormalObserverEndpoint,
        >,
    ) -> Result<HostSourceToken, Box<(HostNetworkError, ApprovedHostInputs)>> {
        #[cfg(windows)]
        if inputs.stages.as_ref().is_some_and(|stages| {
            approval
                .as_ref()
                .is_none_or(|approval| !stages.matches_approval(approval))
        }) {
            return Err(Box::new((HostNetworkError::Authority, inputs)));
        }
        if self.network.unknown_latched.load(Ordering::Acquire) {
            return Err(Box::new((HostNetworkError::Unknown, inputs)));
        }
        #[cfg(windows)]
        if formal_observer_endpoint.as_ref().is_some_and(|endpoint| {
            inputs.policy.role != DataRole::Observer
                || !endpoint.matches(inputs.address, &inputs.binding.origin)
                || approval.as_ref().is_none_or(NativeApprovalStop::is_invalid)
        }) {
            return Err(Box::new((HostNetworkError::Authority, inputs)));
        }
        let checked = (|| {
            if !admission.is_owned_by(runtime)
                || admission.scope().map_err(|_| HostNetworkError::Authority)?
                    != inputs.policy.scope
            {
                return Err(HostNetworkError::Authority);
            }
            admission
                .network_origin()
                .map_err(|_| HostNetworkError::Authority)
        })();
        let origin = match checked {
            Ok(origin) => origin,
            Err(error) => return Err(Box::new((error, inputs))),
        };
        let mut state = self.state.lock();
        if state.closing
            || state.delegation.is_some()
            || state.source_slot.closed
            || state.source_slot.registration.is_some()
            || approval
                .as_ref()
                .is_some_and(NativeApprovalStop::is_invalid)
        {
            return Err(Box::new((HostNetworkError::Locked, inputs)));
        }
        state.one_shot_claimed = true;
        let ApprovedHostInputs {
            #[cfg(windows)]
            stages,
            address,
            relay,
            tunnel,
            relay_mode,
            local,
            phone_pin,
            binding,
            browser,
            issuer,
            principal,
            policy,
            source,
            setup_budget,
            io_budget,
            #[cfg(test)]
            outer_accepted,
            #[cfg(test)]
            termination,
        } = inputs;
        let registration = Arc::new(RegisteredHostSource {
            #[cfg(windows)]
            formal_observer_endpoint,
            description: HostSourceDescription {
                address,
                tunnel,
                relay_mode,
                phone_pin,
                binding,
                browser,
                issuer,
                principal,
                policy,
                source,
                setup_budget,
                io_budget,
            },
            origin,
            live: AtomicBool::new(true),
            approval,
        });
        state.source_slot.fresh = Some(HostAttemptMaterials {
            #[cfg(windows)]
            stages,
            local,
            relay,
            #[cfg(test)]
            outer_accepted,
            #[cfg(test)]
            termination,
        });
        state.source_slot.registration = Some(registration.clone());
        Ok(HostSourceToken(Arc::downgrade(&registration)))
    }
    pub(crate) fn replenish_native_source(
        &self,
        runtime: &NativeRuntime,
        admission: &NativeRunAdmission,
        token: &HostSourceToken,
        material: HostAttemptMaterials,
    ) -> Result<(), Box<(HostNetworkError, HostAttemptMaterials)>> {
        if self.network.unknown_latched.load(Ordering::Acquire) {
            return Err(Box::new((HostNetworkError::Unknown, material)));
        }
        let Some(registration) = token.0.upgrade() else {
            return Err(Box::new((HostNetworkError::Authority, material)));
        };
        if !admission.is_owned_by(runtime)
            || !registration.origin.matches(admission)
            || admission.scope().ok().as_ref() != Some(&registration.description.policy.scope)
        {
            return Err(Box::new((HostNetworkError::Authority, material)));
        }
        #[cfg(windows)]
        if material.stages.as_ref().is_some_and(|stages| {
            registration
                .approval
                .as_ref()
                .is_none_or(|approval| !stages.matches_approval(approval))
        }) {
            return Err(Box::new((HostNetworkError::Authority, material)));
        }
        let mut state = self.state.lock();
        if state.closing
            || state.source_slot.closed
            || state.source_slot.preflight
            || state.source_slot.material_unknown
            || !registration.is_live()
            || !state
                .source_slot
                .registration
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &registration))
            || state.source_slot.fresh.is_some()
        {
            return Err(Box::new((HostNetworkError::Locked, material)));
        }
        state.source_slot.fresh = Some(material);
        Ok(())
    }
    pub(crate) fn revoke_native_source(
        &self,
        token: &HostSourceToken,
    ) -> Result<(), HostNetworkError> {
        let registration = token.0.upgrade().ok_or(HostNetworkError::Authority)?;
        let mut state = self.state.lock();
        if !state
            .source_slot
            .registration
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &registration))
        {
            return Err(HostNetworkError::Authority);
        }
        let material = state.source_slot.close();
        drop(state);
        self.release_source_material(material);
        Ok(())
    }
    pub(crate) fn release_source_material(&self, material: Option<HostAttemptMaterials>) {
        let held = material.as_ref().map(|_| SourceMaterialHold {
            owner: self,
            phase: SourceMaterialPhase::Release,
            returned: false,
        });
        // guard 必须先于任意通知存在；LivePolicy 采样栈内只失效，不通知或析构。
        let closed = self.state.lock().source_slot.closed;
        if closed && !SourceSampleGuard::active() {
            self.network.begin_shutdown();
        }
        drop(material);
        if let Some(held) = held {
            held.complete();
        }
    }
    pub(crate) fn source_material_returned(&self) -> Option<Result<(), HostNetworkError>> {
        self.state.lock().source_slot.material_returned()
    }
    pub(crate) fn drain_closed_source_materials(&self) {
        let material = self.state.lock().source_slot.drain_closed();
        self.release_source_material(material);
    }
    fn begin_registered_admitted_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        connector: Connector,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        let started = Instant::now();
        if self.network.unknown_latched.load(Ordering::Acquire) {
            return Err(HostNetworkError::Unknown);
        }
        let registration = {
            let state = self.state.lock();
            if state.closing || state.source_slot.closed {
                return Err(HostNetworkError::Closed);
            }
            state
                .source_slot
                .registration
                .clone()
                .ok_or(HostNetworkError::Locked)?
        };
        let end = started
            .checked_add(registration.description.setup_budget)
            .ok_or(HostNetworkError::Bounds)?;
        if !admission.is_owned_by(&runtime)
            || !registration.origin.matches(&admission)
            || admission.scope().map_err(|_| HostNetworkError::Authority)?
                != registration.description.policy.scope
        {
            return Err(HostNetworkError::Authority);
        }
        let material = {
            let mut state = self.state.lock();
            if state.closing
                || state.source_slot.closed
                || state.source_slot.preflight
                || state.source_slot.material_unknown
                || !registration.is_live()
                || !state
                    .source_slot
                    .registration
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &registration))
            {
                return Err(HostNetworkError::Closed);
            }
            if state.source_slot.fresh.is_none() {
                return Err(HostNetworkError::Locked);
            }
            state.source_slot.preflight = true;
            state
                .source_slot
                .fresh
                .take()
                .expect("original single material reserved under the same lock")
        };
        let held = SourceMaterialHold {
            owner: self,
            phase: SourceMaterialPhase::Preflight,
            returned: false,
        };
        let inputs = registration.description.inputs(material);
        let result = self.network.begin(
            self,
            runtime,
            admission,
            inputs,
            connector,
            NetworkStart {
                end,
                registration: Some(registration),
            },
        );
        held.complete();
        self.drain_closed_source_materials();
        result
    }
    /// 仅原生已鉴权的单次启动驱动可调用，不接收前端配置或批准布尔值。
    pub(crate) fn begin_owned_native_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        self.begin_registered_admitted_network(runtime, admission, Connector::WebPki)
    }
    /// 实际网络接线；批准来源注册独立，签名能力仍保管自己的请求和密钥任务。
    pub(crate) fn begin_native_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        webview: &tauri::Webview,
        headers: &tauri::http::HeaderMap,
        body: &tauri::ipc::InvokeBody,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        let admission = runtime
            .admit_remote_run(webview, headers, body)
            .map_err(|_| HostNetworkError::Authority)?;
        self.begin_registered_admitted_network(runtime, admission, Connector::WebPki)
    }
    #[cfg(test)]
    fn begin_admitted_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        inputs: ApprovedHostInputs,
        connector: Connector,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        if !self.network_admission_open() {
            return Err(HostNetworkError::Closed);
        }
        let end = Instant::now()
            .checked_add(inputs.setup_budget)
            .ok_or(HostNetworkError::Bounds)?;
        self.network.begin(
            self,
            runtime,
            admission,
            inputs,
            connector,
            NetworkStart {
                end,
                registration: None,
            },
        )
    }
    #[cfg(test)]
    pub(crate) fn test_begin_admitted_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        inputs: ApprovedHostInputs,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        self.begin_admitted_network(runtime, admission, inputs, Connector::PublicLoopbackFixture)
    }
    #[cfg(test)]
    pub(crate) fn test_begin_registered_network(
        self: &Arc<Self>,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
    ) -> Result<Arc<HostNetworkAttempt>, HostNetworkError> {
        self.begin_registered_admitted_network(runtime, admission, Connector::PublicLoopbackFixture)
    }
    #[cfg(test)]
    pub(crate) fn test_original_attempt(&self) -> Option<Arc<HostNetworkAttempt>> {
        self.network.state.lock().attempts.last().cloned()
    }
    #[cfg(test)]
    pub(crate) fn test_network_attempts(&self) -> usize {
        self.network.state.lock().attempts.len()
    }
    #[cfg(test)]
    pub(crate) fn test_registered_source_token(&self) -> HostSourceToken {
        HostSourceToken(Arc::downgrade(
            self.state.lock().source_slot.registration.as_ref().unwrap(),
        ))
    }
    #[cfg(test)]
    pub(crate) fn test_gate_fresh_after_accepted(
        &self,
        proceed: tokio::sync::oneshot::Receiver<()>,
    ) {
        let mut state = self.state.lock();
        let (_, gate) = state
            .source_slot
            .fresh
            .as_mut()
            .unwrap()
            .outer_accepted
            .as_mut()
            .unwrap()
            .as_mut();
        assert!(gate.is_none());
        *gate = Some(proceed);
    }
    #[cfg(test)]
    pub(crate) fn test_observe_fresh_material(
        &self,
        accepted: tokio::sync::oneshot::Sender<()>,
        termination: tokio::sync::oneshot::Sender<TestAttemptEnd>,
    ) {
        let mut state = self.state.lock();
        let material = state
            .source_slot
            .fresh
            .as_mut()
            .expect("original unconsumed material");
        assert!(material.outer_accepted.is_none() && material.termination.is_none());
        material.outer_accepted = Some(Box::new((accepted, None)));
        material.termination = Some(termination);
    }
    #[cfg(test)]
    pub(crate) fn test_source_material_pending(&self) -> bool {
        self.state.lock().source_slot.fresh.is_some()
    }
    #[cfg(test)]
    pub(crate) fn test_network_stop_receiver(&self) -> Option<watch::Receiver<bool>> {
        self.network
            .state
            .lock()
            .attempts
            .last()
            .map(|attempt| attempt.stop.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[allow(non_snake_case)]
    fn HostApproval_SourceValueFailureRevokesKeys_010() {
        let mut results = Vec::new();
        for next in [999u64, 11000] {
            let token = crate::native_crypto::owner::NativeRevocation::new();
            let stop = NativeApprovalStop::new(
                token.clone(),
                std::time::Instant::now() + Duration::from_secs(30),
            );
            let live = stop.clone();
            let checks = Arc::new(ContinuityChecks(Box::new(move || !live.is_invalid())));
            let value = Arc::new(std::sync::atomic::AtomicU64::new(1000));
            let provider = value.clone();
            let (watch, revoked) = watch::channel(false);
            let clock = SourceClock {
                approval: Some(stop),
                source: Arc::new(ApprovedHostSource {
                    clock: Arc::new(move || Ok(provider.load(Ordering::Acquire))),
                    revoked,
                    expires_host_ms: 11000,
                }),
                checks: Mutex::new(Arc::downgrade(&checks)),
                sampled: Mutex::new(None),
                failed: AtomicBool::new(false),
                unknown: Arc::new(AtomicBool::new(false)),
            };
            assert_eq!(clock.now_ms(), Ok(1000));
            value.store(next, Ordering::Release);
            assert_eq!(
                clock.now_ms(),
                Err(if next == 999 {
                    GrantError::Clock
                } else {
                    GrantError::Expired
                })
            );
            results.push((
                next,
                token.is_revoked(),
                *watch.borrow(),
                clock.unknown.load(Ordering::Acquire),
            ));
        }
        eprintln!("TEST_ONLY_SOURCE_VALUE_FAILURE {results:?}");
        assert!(
            results
                .iter()
                .all(|(_, revoked, watch, unknown)| *revoked && !watch && !unknown),
            "source value failure did not revoke original key token"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn source_material_drop_must_return_before_real_exit_publication() {
        use crate::cli::{launch_service::LaunchService, storage::WorkspaceRepository};
        struct HeldRelayDrop {
            entered: std::sync::mpsc::SyncSender<()>,
            released: Mutex<std::sync::mpsc::Receiver<()>>,
            panics: bool,
        }
        impl transport::wss::RelaySigner for HeldRelayDrop {
            fn sign<'a>(
                &'a self,
                _: &'a transport::wss::SigningRequest,
            ) -> std::pin::Pin<
                Box<dyn Future<Output = Result<[u8; 64], transport::wss::SignError>> + Send + 'a>,
            > {
                Box::pin(async { Err(transport::wss::SignError) })
            }
        }
        impl Drop for HeldRelayDrop {
            fn drop(&mut self) {
                self.entered.send(()).unwrap();
                self.released
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                assert!(!self.panics, "TEST_ONLY_SOURCE_MATERIAL_DROP_PANIC");
            }
        }
        for panics in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let launch = Arc::new(LaunchService::new(
                WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap(),
                Some(Default::default()),
                None,
            ));
            let native = Arc::new(NativeRuntime::new(launch));
            let client = Arc::new(crate::remote_client_ipc::RemoteClientService::default());
            let owner = Arc::new(RemoteHostService::default());
            let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
            let (release, released) = std::sync::mpsc::sync_channel(1);
            let local = RemoteHostService::test_public_source_material_identity();
            owner.state.lock().source_slot.fresh = Some(HostAttemptMaterials {
                #[cfg(windows)]
                stages: None,
                local,
                relay: RelayAdmissionIdentity::new(
                    [1; 16],
                    transport::wss::RelayRole::Host,
                    Arc::new(HeldRelayDrop {
                        entered: entered_tx,
                        released: Mutex::new(released),
                        panics,
                    }),
                ),
                outer_accepted: None,
                termination: None,
            });
            let closer = owner.clone();
            let closing = std::thread::spawn(move || closer.request_exit(101, false));
            entered.recv_timeout(Duration::from_secs(2)).unwrap();
            let (published, mut publication) = tokio::sync::oneshot::channel();
            owner.start_exit(101, native, client, move |code| {
                let _ = published.send(code);
            });
            let pending_observed = tokio::time::timeout(Duration::from_secs(2), async {
                while !owner.state.lock().source_wait_pending {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok();
            let early = publication.try_recv();
            let published_while_drop_held = early.is_ok();
            release.send(()).unwrap();
            let closing = closing.join();
            if panics {
                assert!(closing.is_err());
            } else {
                assert_eq!(
                    closing.unwrap(),
                    crate::remote_host::ExitDecision::Wait(101)
                );
            }
            let code = match early {
                Ok(code) => Some(code),
                Err(_) => tokio::time::timeout(Duration::from_secs(2), publication)
                    .await
                    .unwrap()
                    .ok(),
            };
            assert!(owner.test_join_exit_worker().await);
            eprintln!("TEST_ONLY_SOURCE_DROP_EXIT panics={panics} waiter_observed_source_pending={pending_observed} published_while_drop_held={published_while_drop_held} final_code={code:?} original_waiter_joined=true");
            if panics {
                assert_eq!(code, None);
                assert_eq!(
                    owner.source_material_returned(),
                    Some(Err(HostNetworkError::Unknown))
                );
            } else {
                assert_eq!(code, Some(101));
                assert_eq!(owner.source_material_returned(), Some(Ok(())));
            }
            assert!(
                pending_observed,
                "original exit waiter never observed the held source capability"
            );
            assert!(
                !published_while_drop_held,
                "actual source capability Drop was still blocked when exit was published"
            );
        }
    }
    #[tokio::test]
    async fn live_provider_request_exit_does_not_notify_inside_sampling() {
        use crate::cli::{launch_service::LaunchService, storage::WorkspaceRepository};
        use futures_util::task::{waker, ArcWake};
        use std::task::{Context, Poll};
        struct Probe(AtomicBool);
        impl ArcWake for Probe {
            fn wake_by_ref(this: &Arc<Self>) {
                if SourceSampleGuard::active() {
                    this.0.store(true, Ordering::Release);
                }
            }
        }
        let owner = Arc::new(RemoteHostService::default());
        let dir = tempfile::tempdir().unwrap();
        let native = Arc::new(NativeRuntime::new(Arc::new(LaunchService::new(
            WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap(),
            Some(Default::default()),
            None,
        ))));
        let client = Arc::new(crate::remote_client_ipc::RemoteClientService::default());
        let (exit, mut exiting) = tokio::sync::oneshot::channel();
        let exit = Mutex::new(Some(exit));
        let waiter_held = Arc::new(AtomicBool::new(false));
        let observed_waiter = waiter_held.clone();
        let (stop, mut stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Ok(())));
        let (release, held) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            held.await.unwrap();
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&owner.network),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        owner.network.state.lock().attempts.push(attempt.clone());
        let source_owner = owner.clone();
        let checked_owner = Arc::downgrade(&owner);
        let checks = Arc::new(ContinuityChecks(Box::new(move || {
            checked_owner
                .upgrade()
                .is_some_and(|owner| owner.network_admission_open())
        })));
        let (_lease, revoked) = watch::channel(false);
        let clock = SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: Arc::new(move || {
                    assert_eq!(
                        source_owner.request_exit(99, false),
                        crate::remote_host::ExitDecision::Wait(99)
                    );
                    let exit = exit.lock().take().unwrap();
                    source_owner.start_exit(99, native.clone(), client.clone(), move |code| {
                        let _ = exit.send(code);
                    });
                    source_owner.start_exit(99, native.clone(), client.clone(), |_| {
                        panic!("duplicate exit waiter")
                    });
                    assert_eq!(
                        source_owner.request_exit(0, false),
                        crate::remote_host::ExitDecision::Prevent
                    );
                    observed_waiter.store(source_owner.test_exit_worker_held(), Ordering::Release);
                    Ok(1000)
                }),
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: owner.network.unknown_latched.clone(),
        };
        let probe = Arc::new(Probe(AtomicBool::new(false)));
        let notification = stopped.changed();
        tokio::pin!(notification);
        let wake = waker(probe.clone());
        assert!(matches!(
            notification.as_mut().poll(&mut Context::from_waker(&wake)),
            Poll::Pending
        ));
        let sample = clock.now_ms();
        let inline = probe.0.load(Ordering::Acquire);
        let unpublished_while_original_held = matches!(
            exiting.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        owner.drain_closed_source_materials();
        release.send(()).unwrap();
        assert!(attempt.join().await);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), exiting)
                .await
                .unwrap()
                .unwrap(),
            99
        );
        assert!(owner.test_join_exit_worker().await);
        eprintln!("TEST_ONLY_LIVE_PROVIDER_EXIT inline_notification={inline} sample={sample:?} original_waiter_held={} unpublished_before_original_join={unpublished_while_original_held} first_code=99", waiter_held.load(Ordering::Acquire));
        assert_eq!(sample, Err(GrantError::Revoked));
        assert!(
            !inline,
            "request_exit notified a live original attempt inside SourceClock/LivePolicy sampling"
        );
        assert!(waiter_held.load(Ordering::Acquire) && unpublished_while_original_held);
    }
    #[tokio::test]
    async fn closed_source_safe_boundary_fans_out_after_sampling_returns() {
        let owner = Arc::new(RemoteHostService::default());
        let (stop, stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Ok(())));
        let (release, held) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            held.await.unwrap();
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&owner.network),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        owner.network.state.lock().attempts.push(attempt.clone());
        {
            let _sampling = SourceSampleGuard::enter();
            let material = owner.state.lock().source_slot.close();
            assert!(material.is_none());
            owner.drain_closed_source_materials();
            assert!(
                !*stopped.borrow(),
                "sampling stack notified an original attempt"
            );
        }
        assert!(
            !*stopped.borrow(),
            "sampling guard Drop notified an original attempt"
        );
        owner.drain_closed_source_materials();
        let stopped_at_safe_boundary = *stopped.borrow();
        release.send(()).unwrap();
        assert!(attempt.join().await);
        assert!(
            stopped_at_safe_boundary,
            "closed source reached a safe boundary without stopping the original idle attempt"
        );
    }
    #[test]
    fn provider_revoking_continuity_cannot_return_a_live_sample() {
        let live = Arc::new(AtomicBool::new(true));
        let checked = live.clone();
        let revoked_in_callback = live.clone();
        let checks = Arc::new(ContinuityChecks(Box::new(move || {
            checked.load(Ordering::Acquire)
        })));
        let (_lease, revoked) = watch::channel(false);
        let clock = SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: Arc::new(move || {
                    revoked_in_callback.store(false, Ordering::Release);
                    Ok(1000)
                }),
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: Arc::new(AtomicBool::new(false)),
        };
        let result = clock.now_ms();
        live.store(true, Ordering::Release);
        let repeated = clock.now_ms();
        eprintln!(
            "TEST_ONLY_CONTINUITY_REVOKE_DURING_PROVIDER first={result:?} repeated={repeated:?}"
        );
        assert_eq!(result, Err(GrantError::Revoked));
        assert_eq!(repeated, Err(GrantError::Revoked));
    }
    #[test]
    fn publication_callback_close_cannot_commit_success() {
        let mut observations = Vec::new();
        for close_kind in [0, 1, 2] {
            let owner = Arc::new(RemoteHostService::default());
            let custody = Arc::new(HostNetworkCustody::default());
            let (stop, stopped) = watch::channel(false);
            let (published, admitted) = watch::channel(None);
            let attempt = Arc::new(HostNetworkAttempt {
                custody: Arc::downgrade(&custody),
                stop,
                admitted,
                join: tokio::sync::Mutex::new(None),
                terminal: Arc::new(AtomicU8::new(0)),
            });
            let callback_owner = owner.clone();
            let clock = move || {
                match close_kind {
                    0 => {
                        callback_owner.request_exit(97, false);
                    }
                    1 => attempt.request_close(),
                    _ => {
                        let material = callback_owner.state.lock().source_slot.close();
                        drop(material);
                    }
                }
                Ok(1000)
            };
            let result = publish_handoff(
                &owner,
                &stopped,
                &published,
                &clock,
                Instant::now() + Duration::from_secs(1),
            );
            observations.push((close_kind, result, *published.borrow()));
        }
        eprintln!("TEST_ONLY_PUBLICATION_CALLBACK_CLOSE {observations:?}");
        assert!(
            observations.iter().all(
                |(_, result, stored)| *result == Err(HostNetworkError::Closed)
                    && *stored != Some(Ok(()))
            ),
            "callback closed original owner/attempt before publication but success was stored"
        );
    }
    use std::sync::atomic::{AtomicBool, AtomicU64};
    #[tokio::test]
    async fn repeated_shutdown_does_not_renotify_the_registered_attempt() {
        let custody = Arc::new(HostNetworkCustody::default());
        let (stop, mut stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Err(HostNetworkError::Relay)));
        let (release, held) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            held.await.unwrap();
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&custody),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        custody.state.lock().attempts.push(attempt.clone());
        custody.begin_shutdown();
        assert!(*stopped.borrow_and_update());
        custody.begin_shutdown();
        let repeated = stopped.has_changed().unwrap();
        stopped.borrow_and_update();
        attempt.request_close();
        let direct = stopped.has_changed().unwrap();
        release.send(()).unwrap();
        assert!(attempt.join().await);
        assert!(
            !repeated && !direct,
            "a repeated close renewed the stop version"
        );
    }
    async fn assert_late_callback_panic_is_sticky(continuity_panics: bool, expect_deferred: bool) {
        use futures_util::task::{waker, ArcWake};
        use std::task::{Context, Poll};
        struct WakeProbe {
            clock: Weak<SourceClock>,
            custody: Weak<HostNetworkCustody>,
            unlocked: AtomicBool,
            notified: AtomicBool,
        }
        impl ArcWake for WakeProbe {
            fn wake_by_ref(this: &Arc<Self>) {
                this.notified.store(true, Ordering::Release);
                let clock_free = this
                    .clock
                    .upgrade()
                    .is_some_and(|clock| clock.sampled.try_lock().is_some());
                let state_free = this
                    .custody
                    .upgrade()
                    .is_some_and(|owner| owner.state.try_lock().is_some());
                this.unlocked
                    .store(clock_free && state_free, Ordering::Release);
            }
        }
        let custody = Arc::new(HostNetworkCustody::default());
        let fault = Arc::new(AtomicBool::new(false));
        let (lease, revoked) = watch::channel(false);
        let provider_fault = fault.clone();
        let checks_fault = fault.clone();
        let checks = Arc::new(ContinuityChecks(Box::new(move || {
            assert!(
                !continuity_panics || !checks_fault.load(Ordering::Acquire),
                "TEST_ONLY_CONTINUITY_PANIC"
            );
            true
        })));
        let clock = Arc::new(SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: Arc::new(move || {
                    assert!(
                        continuity_panics || !provider_fault.load(Ordering::Acquire),
                        "TEST_ONLY_PROVIDER_PANIC"
                    );
                    Ok(1000)
                }),
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: custody.unknown_latched.clone(),
        });
        let public: Arc<dyn GrantClock> = clock.clone();
        assert_eq!(public.now_ms(), Ok(1000));
        let (stop, mut stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Ok(())));
        let (release, held) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            held.await.unwrap();
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&custody),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        custody.state.lock().attempts.push(attempt.clone());
        let probe = Arc::new(WakeProbe {
            clock: Arc::downgrade(&clock),
            custody: Arc::downgrade(&custody),
            unlocked: AtomicBool::new(false),
            notified: AtomicBool::new(false),
        });
        let notification = stopped.changed();
        tokio::pin!(notification);
        let wake = waker(probe.clone());
        assert!(matches!(
            notification.as_mut().poll(&mut Context::from_waker(&wake)),
            Poll::Pending
        ));
        fault.store(true, Ordering::Release);
        let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| public.now_ms()));
        fault.store(false, Ordering::Release);
        let sticky = public.now_ms().is_err();
        let inline_notification = probe.notified.load(Ordering::Acquire);
        let synchronous_unknown = custody.unknown_latched.load(Ordering::Acquire);
        let unknown = matches!(custody.open_state(), Err(HostNetworkError::Unknown));
        release.send(()).unwrap();
        assert!(attempt.join().await);
        drop(lease);
        assert!(
            matches!(sampled, Ok(Err(_))),
            "public clock boundary let a late callback panic escape"
        );
        if expect_deferred {
            assert!(!inline_notification && synchronous_unknown, "clock callback must latch Unknown without notifying inside the caller's lock stack");
        }
        assert!(sticky && unknown, "late callback panic revived admission");
        assert!(
            probe.unlocked.load(Ordering::Acquire),
            "stop notification held clock/state locks"
        );
    }
    #[tokio::test]
    async fn late_provider_panic_is_sticky_and_seals_custody() {
        assert_late_callback_panic_is_sticky(false, false).await;
    }
    #[tokio::test]
    async fn late_continuity_panic_is_sticky_and_seals_custody() {
        assert_late_callback_panic_is_sticky(true, false).await;
    }
    #[tokio::test]
    async fn late_provider_panic_latches_without_inline_notification() {
        assert_late_callback_panic_is_sticky(false, true).await;
    }
    #[tokio::test]
    async fn late_continuity_panic_latches_without_inline_notification() {
        assert_late_callback_panic_is_sticky(true, true).await;
    }
    #[tokio::test]
    async fn known_joined_failures_release_attempt_capacity() {
        let custody = Arc::new(HostNetworkCustody::default());
        let mut old = Vec::new();
        for _ in 0..130 {
            let (stop, _stopped) = watch::channel(false);
            let (_published, admitted) = watch::channel(Some(Err(HostNetworkError::Relay)));
            let task = tauri::async_runtime::spawn(async { Err(HostNetworkError::Relay) });
            let attempt = Arc::new(HostNetworkAttempt {
                custody: Arc::downgrade(&custody),
                stop,
                admitted,
                join: tokio::sync::Mutex::new(Some(task)),
                terminal: Arc::new(AtomicU8::new(0)),
            });
            custody.state.lock().attempts.push(attempt.clone());
            assert!(attempt.join().await);
            old.push(attempt);
            assert!(custody.open_state().unwrap().attempts.is_empty());
        }
        for attempt in old {
            attempt.request_close();
            assert!(attempt.wait_closed().await.is_ok());
            assert_eq!(attempt.wait_admitted().await, Err(HostNetworkError::Relay));
        }
        let available = custody.open_state().unwrap().attempts.len() < 64;
        assert!(
            available,
            "known joined failures permanently exhausted admission"
        );
    }
    #[tokio::test]
    async fn unjoined_terminal_markers_do_not_release_attempt_capacity() {
        let custody = Arc::new(HostNetworkCustody::default());
        for _ in 0..64 {
            let (stop, _stopped) = watch::channel(false);
            let (_published, admitted) = watch::channel(Some(Err(HostNetworkError::Relay)));
            let task = tauri::async_runtime::spawn(async { Err(HostNetworkError::Relay) });
            custody
                .state
                .lock()
                .attempts
                .push(Arc::new(HostNetworkAttempt {
                    custody: Arc::downgrade(&custody),
                    stop,
                    admitted,
                    join: tokio::sync::Mutex::new(Some(task)),
                    terminal: Arc::new(AtomicU8::new(1)),
                }));
        }
        assert_eq!(custody.open_state().unwrap().attempts.len(), 64);
        assert!(custody.join_all().await);
        assert!(
            custody.open_state().unwrap().attempts.len() < 64,
            "actual consumed handles did not release slots"
        );
    }
    #[tokio::test]
    async fn cancelled_join_observer_keeps_stopped_original_attempt_until_real_completion() {
        struct Witness(Arc<AtomicBool>);
        impl Drop for Witness {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let custody = Arc::new(HostNetworkCustody::default());
        let (stop, mut stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(None);
        let (release, held) = tokio::sync::oneshot::channel();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = Witness(dropped.clone());
        let task = tauri::async_runtime::spawn(async move {
            let _original = witness;
            entered.send(()).unwrap();
            if !*stopped.borrow() {
                stopped.changed().await.unwrap();
            }
            held.await.unwrap();
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&custody),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        custody.state.lock().attempts.push(attempt.clone());
        ready.await.unwrap();
        custody.begin_shutdown();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), custody.join_all())
                .await
                .is_err()
        );
        assert!(attempt.join.lock().await.is_some());
        assert!(!dropped.load(Ordering::Acquire));
        release.send(()).unwrap();
        assert!(custody.join_all().await);
        assert!(dropped.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn joining_panicked_original_task_seals_the_registered_custody() {
        let custody = Arc::new(HostNetworkCustody::default());
        let (stop, _stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Ok(())));
        let task = tauri::async_runtime::spawn(async {
            panic!("TEST_ONLY_NETWORK_TASK_PANIC");
            #[allow(unreachable_code)]
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&custody),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        custody.state.lock().attempts.push(attempt.clone());
        assert!(!attempt.join().await);
        let state = custody.state.lock();
        assert!(
            state.unknown && state.closing,
            "observed original JoinError must block a fresh network attempt"
        );
    }
    #[tokio::test]
    async fn published_success_then_unjoined_finished_panic_blocks_fresh_reservation() {
        let custody = Arc::new(HostNetworkCustody::default());
        let (stop, _stopped) = watch::channel(false);
        let (_published, admitted) = watch::channel(Some(Ok(())));
        let task = tauri::async_runtime::spawn(async {
            panic!("TEST_ONLY_NETWORK_LATE_PANIC");
            #[allow(unreachable_code)]
            Ok(())
        });
        let attempt = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&custody),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(Some(task)),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        custody.state.lock().attempts.push(attempt.clone());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let finished = {
                    let slot = attempt.join.lock().await;
                    slot.as_ref().unwrap().inner().is_finished()
                };
                if finished {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let rejected = matches!(custody.open_state(), Err(HostNetworkError::Unknown));
        assert!(!attempt.join().await);
        assert!(
            rejected,
            "published success does not attest a later panicked original task"
        );
    }
    fn clock_fixture() -> (
        SourceClock,
        Arc<ContinuityChecks>,
        watch::Sender<bool>,
        Arc<AtomicU64>,
    ) {
        let (stop, revoked) = watch::channel(false);
        let now = Arc::new(AtomicU64::new(1000));
        let time = now.clone();
        let checks = Arc::new(ContinuityChecks(Box::new(|| true)));
        let clock = SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: Arc::new(move || Ok(time.load(Ordering::Acquire))),
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: Arc::new(AtomicBool::new(false)),
        };
        (clock, checks, stop, now)
    }
    #[test]
    fn source_revoke_version_cannot_be_hidden_by_restoring_false() {
        let (clock, _checks, stop, _time) = clock_fixture();
        assert_eq!(clock.now_ms(), Ok(1000));
        stop.send_replace(true);
        stop.send_replace(false);
        assert_eq!(clock.now_ms(), Err(GrantError::Revoked));
    }
    #[test]
    fn source_expiry_is_sticky_across_a_later_clock_rollback() {
        let (clock, _checks, _stop, time) = clock_fixture();
        assert_eq!(clock.now_ms(), Ok(1000));
        time.store(11000, Ordering::Release);
        assert_eq!(clock.now_ms(), Err(GrantError::Expired));
        time.store(1000, Ordering::Release);
        assert!(clock.now_ms().is_err());
    }
    #[test]
    fn source_clock_rollback_cannot_start_a_fresh_validity_interval() {
        let (clock, _checks, _stop, time) = clock_fixture();
        assert_eq!(clock.now_ms(), Ok(1000));
        time.store(999, Ordering::Release);
        assert_eq!(clock.now_ms(), Err(GrantError::Clock));
        time.store(1001, Ordering::Release);
        assert!(clock.now_ms().is_err());
    }
    #[test]
    fn historical_clock_does_not_keep_original_continuity_capture_alive() {
        struct Witness(Arc<AtomicBool>);
        impl Drop for Witness {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let (clock, checks, _stop, _time) = clock_fixture();
        drop(checks);
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = Witness(dropped.clone());
        let held = Arc::new(ContinuityChecks(Box::new(move || {
            let _ = &witness;
            true
        })));
        *clock.checks.lock() = Arc::downgrade(&held);
        assert_eq!(clock.now_ms(), Ok(1000));
        assert!(!dropped.load(Ordering::Acquire));
        drop(held);
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(clock.now_ms(), Err(GrantError::Revoked));
    }
    #[test]
    fn concurrent_source_sampling_does_not_invent_clock_rollback() {
        use std::sync::{atomic::AtomicUsize, mpsc, OnceLock};
        let slot = Arc::new(OnceLock::<Weak<SourceClock>>::new());
        let source_slot = slot.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let source_calls = calls.clone();
        let (first_tx, first_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        let source: Arc<dyn GrantClock> = Arc::new(move || {
            if source_calls.fetch_add(1, Ordering::AcqRel) == 0 {
                let clock = source_slot.get().unwrap().upgrade().unwrap();
                first_tx.send(clock.sampled.try_lock().is_none()).unwrap();
                release.lock().recv().unwrap();
                Ok(1000)
            } else {
                Ok(1001)
            }
        });
        let (stop, revoked) = watch::channel(false);
        let checks = Arc::new(ContinuityChecks(Box::new(|| true)));
        let clock = Arc::new(SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: source,
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: Arc::new(AtomicBool::new(false)),
        });
        assert!(slot.set(Arc::downgrade(&clock)).is_ok());
        let first = clock.clone();
        let a = std::thread::spawn(move || first.now_ms());
        let serialized = first_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = clock.clone();
        let mut b = Some(std::thread::spawn(move || second.now_ms()));
        let early = if serialized {
            None
        } else {
            Some(b.take().unwrap().join().unwrap())
        };
        release_tx.send(()).unwrap();
        let a = a.join().unwrap();
        let b = early.unwrap_or_else(|| b.take().unwrap().join().unwrap());
        assert_eq!(a, Ok(1000));
        assert_eq!(b, Ok(1001));
        assert!(serialized);
        drop(stop);
    }
}

#[cfg(test)]
mod reentry_regression {
    use super::*;
    use std::sync::OnceLock;
    fn clock(
        provider: Arc<dyn GrantClock>,
    ) -> (Arc<SourceClock>, watch::Sender<bool>, Arc<ContinuityChecks>) {
        let (sender, revoked) = watch::channel(false);
        let checks = Arc::new(ContinuityChecks(Box::new(|| true)));
        let clock = Arc::new(SourceClock {
            approval: None,
            source: Arc::new(ApprovedHostSource {
                clock: provider,
                revoked,
                expires_host_ms: 11000,
            }),
            checks: Mutex::new(Arc::downgrade(&checks)),
            sampled: Mutex::new(None),
            failed: AtomicBool::new(false),
            unknown: Arc::new(AtomicBool::new(false)),
        });
        (clock, sender, checks)
    }
    // 父测试始终使用独立子进程及有界 watchdog，递归同步自锁不会阻塞测试进程。
    #[test]
    #[allow(non_snake_case)]
    fn SourceClock_SameThreadReentry_001() {
        if std::env::var_os("CC_DESK_CLOCK_REENTRY_CHILD").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "remote_host_network::reentry_regression::SourceClock_SameThreadReentry_001",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("CC_DESK_CLOCK_REENTRY_CHILD", "1")
                .spawn()
                .unwrap();
            // 仅终止本测试创建的子进程；终止后也只进行有界回收观察。
            let cleanup = |child: &mut std::process::Child| {
                let _ = child.kill();
                let end = std::time::Instant::now() + Duration::from_secs(2);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => return true,
                        Err(_) => return false,
                        Ok(None) if std::time::Instant::now() >= end => return false,
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            };
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        assert!(status.success(), "clock child failed: {status}");
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let cleaned = cleanup(&mut child);
                        panic!(
                            "owned clock child observation failed: {error}; cleanupUnknown={}",
                            !cleaned
                        );
                    }
                }
                if std::time::Instant::now() >= deadline {
                    let cleaned = cleanup(&mut child);
                    panic!(
                        "owned clock child exceeded watchdog; cleanupUnknown={}",
                        !cleaned
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            return;
        }

        let slot = Arc::new(OnceLock::<Weak<SourceClock>>::new());
        let source_slot = slot.clone();
        let (original, _sender, _checks) = clock(Arc::new(move || {
            let nested = source_slot.get().unwrap().upgrade().unwrap().now_ms();
            assert_eq!(nested, Err(GrantError::Clock));
            // 外部 provider 吞掉递归错误也不能恢复原 clock。
            Ok(1000)
        }));
        slot.set(Arc::downgrade(&original)).ok().unwrap();
        eprintln!("TEST_ONLY_SOURCE_CLOCK_REENTRY entering_original");
        assert_eq!(original.now_ms(), Err(GrantError::Revoked));
        assert_eq!(original.now_ms(), Err(GrantError::Revoked));
        assert!(original.failed.load(Ordering::Acquire));
        assert!(!SourceSampleGuard::active());
        assert!(ACTIVE_SOURCE_CLOCKS.with(|active| active.borrow().is_empty()));
        let (independent, _independent_sender, _independent_checks) = clock(Arc::new(|| Ok(1001)));
        assert_eq!(independent.now_ms(), Ok(1001));
        eprintln!("TEST_ONLY_SOURCE_CLOCK_REENTRY recursive_error=true original_sticky=true depth_restored=true");
    }
    #[test]
    #[allow(non_snake_case)]
    fn SourceClock_DifferentClockNesting_002() {
        let (inner, _sender, _checks) = clock(Arc::new(|| Ok(1000)));
        let provider = inner.clone();
        let (outer, _outer_sender, _outer_checks) = clock(Arc::new(move || provider.now_ms()));
        assert_eq!(outer.now_ms(), Ok(1000));
        assert_eq!(inner.now_ms(), Ok(1000));
        assert!(!SourceSampleGuard::active());
    }
    #[test]
    #[allow(non_snake_case)]
    fn SourceClock_PanicRestoresActiveGuard_003() {
        let (panicking, _sender, _checks) =
            clock(Arc::new(|| panic!("TEST_ONLY_CLOCK_PROVIDER_PANIC")));
        assert_eq!(panicking.now_ms(), Err(GrantError::Clock));
        assert!(ACTIVE_SOURCE_CLOCKS.with(|active| active.borrow().is_empty()));
        assert!(!SourceSampleGuard::active());
        let (independent, _other_sender, _other_checks) = clock(Arc::new(|| Ok(1000)));
        assert_eq!(independent.now_ms(), Ok(1000));
    }
}

#[cfg(all(test, windows))]
#[test]
#[allow(non_snake_case)]
fn ObserverRouting_FormalProofCannotSubstituteFixtureOrEndpoint() {
    use crate::remote_host_controller::startup::configuration::{
        test_json, FrozenRegistrationStart,
    };
    use cc_desk_remote_record::appbind::CanonicalOrigin;

    let mut request: serde_json::Value = serde_json::from_str(&test_json()).unwrap();
    request["origin"] = "wss://relay.invalid:8443/one".into();
    request["address"] = "192.0.2.1:8443".into();
    let controller = FrozenRegistrationStart::parse(&request.to_string()).unwrap();
    assert!(controller.formal_observer_endpoint().is_none());
    request["accessMode"] = "observerOnce".into();
    let observer = FrozenRegistrationStart::parse(&request.to_string()).unwrap();
    let check = |connector, address, origin, endpoint| {
        observer_route_allowed(connector, address, origin, endpoint)
    };
    let endpoint = observer.formal_observer_endpoint();
    let different = CanonicalOrigin::trusted("wss://relay.invalid:8443/other").unwrap();
    assert!(!check(
        Connector::WebPki,
        observer.address,
        &observer.origin,
        None
    ));
    assert_eq!(
        check(
            Connector::WebPki,
            observer.address,
            &observer.origin,
            endpoint.as_ref()
        ),
        cfg!(feature = "remote-observer")
    );
    assert!(!check(
        Connector::WebPki,
        observer.address,
        &different,
        endpoint.as_ref()
    ));
    assert!(!check(
        Connector::WebPki,
        "192.0.2.2:8443".parse().unwrap(),
        &observer.origin,
        endpoint.as_ref()
    ));
    assert!(!check(
        Connector::PublicLoopbackFixture,
        observer.address,
        &observer.origin,
        endpoint.as_ref()
    ));
    assert!(!check(
        Connector::WebPki,
        "127.0.0.1:8443".parse().unwrap(),
        &observer.origin,
        None
    ));
}
