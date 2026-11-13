//! 原生逐次确认组件；生产配置与材料供应者仍未接通。
#![allow(
    dead_code,
    reason = "Host description and key suppliers are separate admission gates"
)]
use parking_lot::Mutex;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};

static DIALOG_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const PENDING: Duration = Duration::from_secs(30);
const APPROVAL: Duration = Duration::from_secs(300);

#[cfg(test)]
#[path = "remote_host_approval_binding_tests.rs"]
mod binding_tests;
#[path = "remote_host_approval_frozen.rs"]
mod frozen;
pub(crate) use frozen::FrozenHostApproval;

/// 状态机只持不可变后端描述，不接收密钥、运行所有权或 IPC 数据。
/// 泛型仅限私有状态机；对外桥接使用具体描述。
struct ApprovalMachine<T> {
    state: Mutex<State<T>>,
    lease: Arc<ApprovalLease>,
}
struct State<T> {
    description: Option<T>,
    pending_end: Instant,
    cap: Instant,
    approved_end: Option<Instant>,
    closed: bool,
    callback_outstanding: bool,
}
/// 仅可移动；无公开构造、序列化或复制能力。
struct MachinePermit<T> {
    description: T,
    end: Instant,
    owner: std::sync::Weak<ApprovalMachine<T>>,
}
impl<T: Send + 'static> ApprovalMachine<T> {
    fn pending(description: T, now: Instant, cap: Instant) -> Arc<Self> {
        Arc::new(Self {
            lease: ApprovalLease::new(),
            state: Mutex::new(State {
                description: Some(description),
                pending_end: now + PENDING,
                cap,
                approved_end: None,
                closed: false,
                callback_outstanding: true,
            }),
        })
    }
    /// 展示文字由同一份冻结描述生成。
    /// 本步骤不打开密钥，也不启动网络。
    fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        description: T,
        message: String,
        cap: Instant,
    ) -> Arc<Self> {
        let owner = Self::pending(description, Instant::now(), cap);
        let weak = Arc::downgrade(&owner);
        app.dialog()
            .message(message)
            .title("Approve this remote Host session")
            .parent(window)
            .buttons(MessageDialogButtons::YesNo)
            .show_with_result(move |result| {
                if let Some(owner) = weak.upgrade() {
                    owner.complete(matches!(result, MessageDialogResult::Yes), Instant::now());
                }
                DIALOG_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
            });
        owner
    }
    fn complete(&self, yes: bool, now: Instant) {
        let mut state = self.state.lock();
        if !state.callback_outstanding {
            return;
        }
        state.callback_outstanding = false;
        if state.closed || !yes || now >= state.pending_end || now >= state.cap {
            self.lease.revoke_only();
            state.closed = true;
            let discarded = state.description.take();
            drop(state);
            drop(discarded);
            return;
        }
        state.approved_end = Some((now + APPROVAL).min(state.cap));
    }
    pub(crate) fn cancel(&self) {
        self.lease.revoke_only();
        let mut state = self.state.lock();
        state.closed = true;
        let discarded = state.description.take();
        drop(state);
        drop(discarded);
    }
    /// 到期只失效批准，不关闭原生框或其独立线程。
    pub(crate) fn take(self: &Arc<Self>) -> Option<MachinePermit<T>> {
        self.take_at(Instant::now())
    }
    fn take_at(self: &Arc<Self>, now: Instant) -> Option<MachinePermit<T>> {
        let mut state = self.state.lock();
        let end = state
            .approved_end
            .unwrap_or(state.pending_end.min(state.cap));
        if state.closed || now >= end {
            self.lease.revoke_only();
            state.closed = true;
            let discarded = state.description.take();
            drop(state);
            drop(discarded);
            return None;
        }
        state.approved_end?;
        Some(MachinePermit {
            description: state.description.take()?,
            end,
            owner: Arc::downgrade(self),
        })
    }
    pub(crate) fn callback_outstanding(&self) -> bool {
        self.state.lock().callback_outstanding
    }
}
impl<T> MachinePermit<T> {
    /// 首次尝试即消费许可，包括准备失败。
    /// 调用方必须重新验证原运行，并保管全部产出材料。
    pub(crate) fn consume<U>(self, prepare: impl FnOnce(T, Instant) -> U) -> Option<U> {
        let owner = self.owner.upgrade()?;
        if owner.lease.is_revoked() || owner.state.lock().closed || Instant::now() >= self.end {
            owner.lease.revoke_only();
            return None;
        }
        Some(prepare(self.description, self.end))
    }
}
impl<T> Drop for ApprovalMachine<T> {
    fn drop(&mut self) {
        self.lease.revoke_only();
    }
}
#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    #[test]
    fn HostApproval_SingleConsumption_001() {
        let now = Instant::now();
        let owner = ApprovalMachine::pending(42, now, now + APPROVAL);
        assert!(owner.take_at(now).is_none());
        owner.complete(true, now);
        let permit = owner.take_at(now).unwrap();
        assert!(owner.take_at(now).is_none());
        assert_eq!(permit.consume(|description, _| description), Some(42));
    }
    #[test]
    fn HostApproval_LateAndDenied_002() {
        let now = Instant::now();
        for (yes, delay) in [(false, Duration::ZERO), (true, PENDING)] {
            let owner = ApprovalMachine::pending(42, now, now + APPROVAL);
            owner.complete(yes, now + delay);
            owner.complete(true, now);
            assert!(owner.take_at(now).is_none());
        }
    }
    #[test]
    fn HostApproval_CancelPermit_003() {
        let now = Instant::now();
        let owner = ApprovalMachine::pending(42, now, now + APPROVAL);
        owner.complete(true, now);
        let permit = owner.take_at(now).unwrap();
        owner.cancel();
        assert!(permit.consume(|_, _| panic!("cancelled")).is_none());
    }
    #[test]
    fn HostApproval_DeadlineAndOutstanding_004() {
        let now = Instant::now();
        let owner = ApprovalMachine::pending(42, now, now + Duration::from_secs(1));
        owner.complete(true, now);
        assert!(owner.take_at(now + Duration::from_secs(1)).is_none());
        let pending = ApprovalMachine::pending(42, now, now + APPROVAL);
        assert!(pending.take_at(now + PENDING).is_none());
        assert!(pending.callback_outstanding());
        pending.complete(true, now + PENDING);
        assert!(!pending.callback_outstanding());
        assert!(pending.take_at(now).is_none());
    }
}

use crate::{
    cli::native_runtime::NativeRuntime,
    remote_host::RemoteHostService,
    remote_host_network::{
        ApprovedHostInputs, ApprovedHostSource, HostAttemptMaterials, HostNetworkError,
        HostSourceToken,
    },
    run_supervisor::{NativeRunAdmission, NativeRunNetworkOrigin},
};
use cc_desk_remote_security::{
    BrowserProfile, GrantClock, GrantError, HostSnapshot, NativeGrantPolicy, P256Spki, PeerPin,
};
use tokio::sync::watch;

/// 原生后端字段在展示前移入批准 owner 并冻结。
pub(crate) struct HostApprovalDescription {
    pub endpoint_id: [u8; 32],
    pub endpoint_role: cc_desk_remote_security::Role,
    pub relay_binding: ([u8; 16], cc_desk_remote_transport::wss::RelayRole),
    pub address: std::net::SocketAddr,
    pub tunnel: [u8; 16],
    pub phone_pin: PeerPin,
    pub binding: HostSnapshot,
    pub browser: BrowserProfile,
    pub issuer: P256Spki,
    pub principal: [u8; 32],
    pub policy: NativeGrantPolicy,
    pub clock: Arc<dyn GrantClock>,
    pub expires_host_ms: u64,
    pub setup_budget: Duration,
    pub io_budget: Duration,
}
type BoundDescription = FrozenHostApproval;
struct ApprovalLease {
    #[cfg(windows)]
    native: Arc<crate::native_crypto::owner::NativeRevocation>,
    #[cfg(not(windows))]
    invalid: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    before_publish: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    revoked: watch::Sender<bool>,
    registration: Mutex<Option<(std::sync::Weak<RemoteHostService>, HostSourceToken)>>,
}
impl ApprovalLease {
    fn new() -> Arc<Self> {
        let (revoked, _) = watch::channel(false);
        Arc::new(Self {
            #[cfg(windows)]
            native: crate::native_crypto::owner::NativeRevocation::new(),
            #[cfg(not(windows))]
            invalid: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            before_publish: Mutex::new(None),
            revoked,
            registration: Mutex::new(None),
        })
    }
    fn is_revoked(&self) -> bool {
        #[cfg(windows)]
        {
            self.native.is_revoked()
        }
        #[cfg(not(windows))]
        {
            self.invalid.load(std::sync::atomic::Ordering::Acquire)
        }
    }
    /// 资格事实先失效；这里没有 wake、service 升级、材料析构或外部 clock。
    fn revoke_only(&self) {
        #[cfg(windows)]
        self.native.revoke();
        #[cfg(not(windows))]
        self.invalid
            .store(true, std::sync::atomic::Ordering::Release);
    }
    fn cancel(&self) {
        self.revoke_only();
        self.revoked.send_if_modified(|value| {
            if *value {
                false
            } else {
                *value = true;
                true
            }
        });
        let registered = self.registration.lock().clone();
        if let Some((service, token)) = registered {
            if let Some(service) = service.upgrade() {
                let _ = service.revoke_native_source(&token);
            }
        }
    }
}
/// 原票据/采样失败只能纯撤销，不在 prepare 或 release worker 上升级 service。
struct LeaseFailure {
    lease: Arc<ApprovalLease>,
    armed: bool,
}
impl LeaseFailure {
    fn new(lease: Arc<ApprovalLease>) -> Self {
        Self { lease, armed: true }
    }
    fn complete(&mut self) {
        self.armed = false;
    }
}
impl Drop for LeaseFailure {
    fn drop(&mut self) {
        if self.armed {
            self.lease.revoke_only();
        }
    }
}
struct ApprovalClock {
    original: Arc<dyn GrantClock>,
    lease: Arc<ApprovalLease>,
    end: Instant,
}
impl GrantClock for ApprovalClock {
    fn now_ms(&self) -> Result<u64, GrantError> {
        if self.lease.is_revoked() {
            return Err(GrantError::Revoked);
        }
        let mut failure = LeaseFailure::new(self.lease.clone());
        if Instant::now() >= self.end {
            return Err(GrantError::Expired);
        }
        let now = self.original.now_ms()?;
        if self.lease.is_revoked() {
            return Err(GrantError::Revoked);
        }
        if Instant::now() >= self.end {
            return Err(GrantError::Expired);
        }
        failure.complete();
        Ok(now)
    }
}
pub(crate) struct NativeHostApproval {
    machine: Arc<ApprovalMachine<BoundDescription>>,
    lease: Arc<ApprovalLease>,
}
pub(crate) enum ApprovalRegistrationError {
    /// 拒绝时由调用方继续保管原材料。
    Rejected(HostAttemptMaterials),
    /// 原网络登记错误保留已组装输入。
    Registration(Box<(HostNetworkError, ApprovedHostInputs)>),
    Revoked,
}
fn public_text(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}
impl HostApprovalDescription {
    fn render(&self) -> String {
        let browser = self.browser.display_metadata();
        format!("Allow one remote connection for this running session, for at most five minutes (earlier policy limits apply). No automatic reconnect.\nRoute: {}\nTunnel: {:02x?}\nOrigin: {}\nSession: {}\nRun: {}\nGeneration: {}\nRole: {:?}; grant duration: {:?}\nPhone endpoint: {:02x?}\nBrowser origin: {}; RP: {}\nCredential fingerprint: {:02x?}\nBrowser key fingerprint: {:02x?}\nBackup eligible: {}; allow backed-up: {}\nIssuer identity: {:02x?}\nPrincipal: {:02x?}\nActivation: {:02x?}\nEpochs host/client/policy/issuer: {}/{}/{}/{}; incarnation: {}\nSetup/IO budgets: {:?}/{:?}",
            self.address, self.tunnel, self.binding.origin.as_str(), public_text(&self.policy.scope.session), public_text(&self.policy.scope.run), public_text(&self.policy.scope.generation), self.policy.role, self.policy.duration, self.phone_pin.endpoint_id(), browser.origin, browser.rp_id, browser.credential_fingerprint, browser.key_fingerprint, browser.backup_eligible, browser.allow_backed_up, self.issuer.endpoint_id(cc_desk_remote_security::Role::Host), self.principal, self.binding.activation, self.binding.host_key_epoch, self.binding.client_key_epoch, self.binding.policy_epoch, self.binding.issuer_epoch, self.binding.incarnation, self.setup_budget, self.io_budget)
    }
}
impl NativeHostApproval {
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        mut frozen: FrozenHostApproval,
        admission: &NativeRunAdmission,
    ) -> Result<Self, HostNetworkError> {
        if !frozen.matches(admission) {
            return Err(HostNetworkError::Authority);
        }
        frozen.constrain_from_clock()?;
        let cap = frozen.cap();
        let description = &frozen.fields;
        if DIALOG_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(HostNetworkError::Locked);
        }
        let full = format!(
            "Local endpoint: {:02x?} ({:?})\nRelay identity: {:?}\n{}{}",
            description.endpoint_id,
            description.endpoint_role,
            (
                description.relay_binding.0,
                if description.relay_binding.1 == cc_desk_remote_transport::wss::RelayRole::Host {
                    "Host"
                } else {
                    "Phone"
                }
            ),
            frozen.signing_text(),
            description.render()
        );
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(b"CC-DESK-NATIVE-HOST-APPROVAL-v1\0");
        digest.update(full.as_bytes());
        digest.update(
            format!(
                "{:?}|{}|{:?}",
                description.policy.scope, description.expires_host_ms, cap
            )
            .as_bytes(),
        );
        let fingerprint: [u8; 32] = digest.finalize().into();
        let text = format!("One connection only; no automatic reconnect.\nRun: {}\nRole: {:?}\nOrigin: {}\nPhone: {:02x?}\nExpires within five minutes or earlier profile/policy limit.\nFrozen description SHA256: {:02x?}\n\n{}", public_text(&description.policy.scope.run), description.policy.role, description.binding.origin.as_str(), description.phone_pin.endpoint_id(), fingerprint, full);

        let machine = ApprovalMachine::show(app, window, frozen, text, cap);
        Ok(Self {
            lease: machine.lease.clone(),
            machine,
        })
    }
    pub(crate) fn cancel(&self) {
        self.lease.cancel();
        self.machine.cancel();
    }
    pub(crate) fn callback_outstanding(&self) -> bool {
        self.machine.callback_outstanding()
    }
    #[cfg(test)]
    pub(crate) fn has_supplier(&self) -> bool {
        #[cfg(windows)]
        {
            self.machine
                .state
                .lock()
                .description
                .as_ref()
                .is_some_and(|d| d.supplier.is_some())
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    /// 调用方轮询可清理到期描述；这不表示原生框线程已回收。
    pub(crate) fn poll_expiry(&self) {
        let now = Instant::now();
        let expired = {
            let state = self.machine.state.lock();
            now >= state
                .approved_end
                .unwrap_or(state.pending_end.min(state.cap))
        };
        if expired {
            self.cancel();
        }
    }
    #[cfg(test)]
    pub(crate) fn test_clock(&self, clock: Arc<dyn GrantClock>) {
        self.machine
            .state
            .lock()
            .description
            .as_mut()
            .unwrap()
            .fields
            .clock = clock;
    }
    #[cfg(test)]
    pub(crate) fn test_before_publish(&self, hook: Box<dyn FnOnce() + Send>) {
        *self.lease.before_publish.lock() = Some(hook);
    }
    #[cfg(test)]
    pub(crate) fn test_fail_clock(&self) {
        self.machine
            .state
            .lock()
            .description
            .as_mut()
            .unwrap()
            .fields
            .clock = Arc::new(|| Err(GrantError::Clock));
    }
    #[cfg(test)]
    pub(crate) fn test_expire(&self) {
        self.machine.state.lock().approved_end = Some(Instant::now());
    }
    #[cfg(test)]
    pub(crate) fn test_revoke_only(&self) {
        self.lease.revoke_only();
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_native_token(&self) -> Arc<crate::native_crypto::owner::NativeRevocation> {
        self.lease.native.clone()
    }
    #[cfg(test)]
    pub(crate) fn test_watch_revoked(&self) -> bool {
        *self.lease.revoked.borrow()
    }
    #[cfg(test)]
    pub(crate) fn test_record_description(
        fields: HostApprovalDescription,
        admission: &NativeRunAdmission,
    ) -> Self {
        let origin = admission.network_origin().unwrap();
        let now = Instant::now();
        let machine = ApprovalMachine::pending(
            BoundDescription::fixture(fields, origin, now + APPROVAL),
            now,
            now + APPROVAL,
        );
        machine.complete(true, now);
        Self {
            lease: machine.lease.clone(),
            machine,
        }
    }
    #[cfg(test)]
    pub(crate) fn test_record_frozen(
        mut frozen: FrozenHostApproval,
        yes: bool,
    ) -> Result<Self, HostNetworkError> {
        frozen.constrain_from_clock()?;
        let cap = frozen.cap();
        let now = Instant::now();
        let machine = ApprovalMachine::pending(frozen, now, cap);
        machine.complete(yes, now);
        Ok(Self {
            lease: machine.lease.clone(),
            machine,
        })
    }
    #[cfg(test)]
    pub(crate) fn test_take_description(&self) -> HostApprovalDescription {
        self.machine.state.lock().description.take().unwrap().fields
    }
    #[cfg(test)]
    pub(crate) fn test_from_inputs(
        inputs: ApprovedHostInputs,
        admission: &NativeRunAdmission,
        yes: bool,
    ) -> (Self, HostAttemptMaterials) {
        let ApprovedHostInputs {
            address,
            relay,
            tunnel,
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
            outer_accepted,
            termination,
        } = inputs;
        let fields = HostApprovalDescription {
            endpoint_id: local.endpoint_id(),
            endpoint_role: local.role(),
            relay_binding: relay.binding(),
            address,
            tunnel,
            phone_pin,
            binding,
            browser,
            issuer,
            principal,
            policy,
            clock: source.clock.clone(),
            expires_host_ms: source.expires_host_ms,
            setup_budget,
            io_budget,
        };
        let origin = admission.network_origin().unwrap();
        let now = Instant::now();
        let machine = ApprovalMachine::pending(
            BoundDescription::fixture(fields, origin, now + APPROVAL),
            now,
            now + APPROVAL,
        );
        machine.complete(yes, now);
        (
            Self {
                lease: machine.lease.clone(),
                machine,
            },
            HostAttemptMaterials {
                local,
                relay,
                outer_accepted,
                termination,
            },
        )
    }
    pub(crate) fn control_reply(&self) -> crate::remote_host_controller::HostControlReply {
        use crate::remote_host_controller::HostControlReply as Reply;
        self.poll_expiry();
        let state = self.machine.state.lock();
        if state.closed {
            return if Instant::now()
                >= state
                    .approved_end
                    .unwrap_or(state.pending_end.min(state.cap))
            {
                Reply::Expired
            } else {
                Reply::Cancelled
            };
        }
        if self.lease.is_revoked() {
            return Reply::Cancelled;
        }
        if self.lease.registration.lock().is_some() {
            return Reply::Registered;
        }
        if state.description.is_none() {
            return Reply::Consumed;
        }
        if state.approved_end.is_some() {
            Reply::ApprovalRecorded
        } else {
            Reply::Pending
        }
    }
    pub(crate) fn take_activation(
        &self,
        admission: &NativeRunAdmission,
    ) -> Result<ActivationTicket, HostNetworkError> {
        self.claim_activation(admission)?.validate(admission)
    }
    /// 只认原 run；不取 clock。未取得 claim 的外国/重复请求无撤销副作用。
    pub(crate) fn claim_activation(
        &self,
        admission: &NativeRunAdmission,
    ) -> Result<ActivationClaim, HostNetworkError> {
        admission
            .network_origin()
            .map_err(|_| HostNetworkError::Authority)?;
        {
            let state = self.machine.state.lock();
            let bound = state.description.as_ref().ok_or(HostNetworkError::Locked)?;
            if !bound.origin.matches(admission) {
                return Err(HostNetworkError::Authority);
            }
        }
        let permit = self.machine.take().ok_or(HostNetworkError::Locked)?;
        Ok(ActivationClaim {
            permit: Some(permit),
            lease: self.lease.clone(),
            failure: Some(LeaseFailure::new(self.lease.clone())),
        })
    }
    #[cfg(test)]
    pub(crate) fn register_once(
        &self,
        runtime: &NativeRuntime,
        admission: &NativeRunAdmission,
        service: &Arc<RemoteHostService>,
        material: HostAttemptMaterials,
    ) -> Result<(), ApprovalRegistrationError> {
        let ticket = match self.take_activation(admission) {
            Ok(ticket) => ticket,
            Err(_) => return Err(ApprovalRegistrationError::Rejected(material)),
        };
        ticket.register_material(runtime, admission, service, material)
    }
}
pub(crate) struct ActivationClaim {
    failure: Option<LeaseFailure>,
    permit: Option<MachinePermit<BoundDescription>>,
    lease: Arc<ApprovalLease>,
}
impl ActivationClaim {
    #[cfg(windows)]
    pub(crate) fn has_supplier(&self) -> bool {
        self.permit
            .as_ref()
            .expect("original claim")
            .description
            .supplier
            .is_some()
    }
    pub(crate) fn end(&self) -> Instant {
        self.permit.as_ref().expect("original claim").end
    }
    #[cfg(windows)]
    pub(crate) fn native_token(&self) -> Arc<crate::native_crypto::owner::NativeRevocation> {
        self.lease.native.clone()
    }
    pub(crate) fn validate(
        mut self,
        admission: &NativeRunAdmission,
    ) -> Result<ActivationTicket, HostNetworkError> {
        let permit = self.permit.as_ref().expect("original claim");
        let bound = &permit.description;
        if self.lease.is_revoked()
            || Instant::now() >= permit.end
            || !bound.origin.matches(admission)
            || admission.network_origin().is_err()
        {
            return Err(HostNetworkError::Authority);
        }
        let sample =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bound.fields.clock.now_ms()));
        if !matches!(sample, Ok(Ok(now)) if now < bound.fields.expires_host_ms)
            || self.lease.is_revoked()
            || Instant::now() >= permit.end
        {
            return Err(HostNetworkError::Deadline);
        }
        Ok(ActivationTicket {
            permit: self.permit.take().expect("original claim"),
            lease: self.lease.clone(),
            failure: self.failure.take().expect("original failure guard"),
        })
    }
}
/// 私有构造且不可复制；消费批准之后才允许准备材料。
pub(crate) struct ActivationTicket {
    failure: LeaseFailure,
    permit: MachinePermit<BoundDescription>,
    lease: Arc<ApprovalLease>,
}
impl ActivationTicket {
    #[cfg(windows)]
    pub(crate) fn prepare_frozen(
        mut self,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        service: std::sync::Weak<RemoteHostService>,
        signal: Arc<crate::remote_host::activation::ActivationSignal>,
        publisher: crate::native_crypto::host::HostKeyPublisher,
    ) -> crate::remote_host::activation::PreparationResult {
        use crate::remote_host::activation::PreparationResult;
        if signal.cancelled()
            || self.lease.is_revoked()
            || Instant::now() >= self.permit.end
            || !self.permit.description.origin.matches(&admission)
            || admission.network_origin().is_err()
        {
            return PreparationResult::Complete(false);
        }
        let Some(supplier) = self.permit.description.supplier.take() else {
            return PreparationResult::Complete(false);
        };
        let public = match P256Spki::new(&supplier.binding().endpoint_spki) {
            Ok(public) => public,
            Err(_) => return PreparationResult::Complete(false),
        };
        let set = match publisher.prepare(supplier) {
            Ok(set) => set,
            Err(_) => return PreparationResult::Complete(false),
        };
        let local = match cc_desk_remote_security::LocalIdentity::new(
            cc_desk_remote_security::Role::Host,
            public,
            set.endpoint,
        ) {
            Ok(local) => local,
            Err(_) => return PreparationResult::Complete(false),
        };
        let material = HostAttemptMaterials {
            local,
            relay: set.relay,
            #[cfg(test)]
            outer_accepted: None,
            #[cfg(test)]
            termination: None,
        };
        self.completion(runtime, admission, service, signal, material)
    }
    #[cfg(test)]
    pub(crate) fn prepare_fixture(
        self,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        service: std::sync::Weak<RemoteHostService>,
        signal: Arc<crate::remote_host::activation::ActivationSignal>,
        prepare: impl FnOnce() -> HostAttemptMaterials,
    ) -> crate::remote_host::activation::PreparationResult {
        use crate::remote_host::activation::PreparationResult;
        if signal.cancelled()
            || self.lease.is_revoked()
            || Instant::now() >= self.permit.end
            || !self.permit.description.origin.matches(&admission)
            || admission.network_origin().is_err()
        {
            return PreparationResult::Complete(false);
        }
        let material = prepare();
        self.completion(runtime, admission, service, signal, material)
    }
    fn completion(
        self,
        runtime: Arc<NativeRuntime>,
        admission: NativeRunAdmission,
        service: std::sync::Weak<RemoteHostService>,
        signal: Arc<crate::remote_host::activation::ActivationSignal>,
        material: HostAttemptMaterials,
    ) -> crate::remote_host::activation::PreparationResult {
        use crate::remote_host::activation::PreparationResult;
        // worker 只归还材料与登记能力。升级 service 必须等原线程实际 join 之后。
        PreparationResult::Prepared(Box::new(move || {
            if signal.cancelled() {
                return false;
            }
            let Some(service) = service.upgrade() else {
                return false;
            };
            self.register_material(&runtime, &admission, &service, material)
                .is_ok()
        }))
    }
    fn register_material(
        self,
        runtime: &NativeRuntime,
        admission: &NativeRunAdmission,
        service: &Arc<RemoteHostService>,
        material: HostAttemptMaterials,
    ) -> Result<(), ApprovalRegistrationError> {
        let Self {
            permit,
            lease,
            failure,
        } = self;
        let end = permit.end;
        let bound = permit.description;
        let validation_failure = failure;
        if lease.is_revoked()
            || Instant::now() >= end
            || !bound.origin.matches(admission)
            || admission.network_origin().is_err()
        {
            return Err(ApprovalRegistrationError::Rejected(material));
        }
        let d = bound.fields;
        let mut failure = validation_failure;
        if material.local.endpoint_id() != d.endpoint_id
            || material.local.role() != d.endpoint_role
            || material.relay.binding() != d.relay_binding
        {
            return Err(ApprovalRegistrationError::Rejected(material));
        }
        // 外部时钟不占用 owner 锁；返回后再次检查撤销和本机期限。
        let sample = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.clock.now_ms()));
        if !matches!(sample, Ok(Ok(now)) if now < d.expires_host_ms)
            || lease.is_revoked()
            || Instant::now() >= end
        {
            return Err(ApprovalRegistrationError::Rejected(material));
        }
        let source = Arc::new(ApprovedHostSource {
            clock: Arc::new(ApprovalClock {
                original: d.clock,
                lease: lease.clone(),
                end,
            }),
            revoked: lease.revoked.subscribe(),
            expires_host_ms: d.expires_host_ms,
        });
        let HostAttemptMaterials {
            local,
            relay,
            #[cfg(test)]
            outer_accepted,
            #[cfg(test)]
            termination,
        } = material;
        let inputs = ApprovedHostInputs {
            address: d.address,
            relay,
            tunnel: d.tunnel,
            local,
            phone_pin: d.phone_pin,
            binding: d.binding,
            browser: d.browser,
            issuer: d.issuer,
            principal: d.principal,
            policy: d.policy,
            source,
            setup_budget: d.setup_budget,
            io_budget: d.io_budget,
            #[cfg(test)]
            outer_accepted,
            #[cfg(test)]
            termination,
        };
        #[cfg(windows)]
        let stop = Some(crate::remote_host_network::NativeApprovalStop::new(
            lease.native.clone(),
            end,
        ));
        #[cfg(not(windows))]
        let stop = None;
        let token = service
            .register_approved_native_source(runtime, admission, inputs, stop)
            .map_err(ApprovalRegistrationError::Registration)?;
        #[cfg(test)]
        {
            let hook = lease.before_publish.lock().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        *lease.registration.lock() = Some((Arc::downgrade(service), token.clone()));
        if lease.is_revoked() || Instant::now() >= end {
            lease.cancel();
            return Err(ApprovalRegistrationError::Revoked);
        }
        failure.complete();
        Ok(())
    }
}

impl Drop for NativeHostApproval {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
#[test]
#[allow(non_snake_case)]
fn HostApproval_ClockRevocationIsSticky_005() {
    let lease = ApprovalLease::new();
    let clock = ApprovalClock {
        original: Arc::new(|| Ok(123)),
        lease: lease.clone(),
        end: Instant::now() + APPROVAL,
    };
    assert_eq!(clock.now_ms(), Ok(123));
    lease.cancel();
    lease.cancel();
    assert_eq!(clock.now_ms(), Err(GrantError::Revoked));
}

#[cfg(test)]
#[test]
#[allow(non_snake_case)]
fn HostApproval_RevokedLeaseNeverReportsAvailable_006() {
    let now = Instant::now();
    let lease = ApprovalLease::new();
    let owner = NativeHostApproval {
        machine: Arc::new(ApprovalMachine {
            lease: lease.clone(),
            state: Mutex::new(State {
                description: None,
                pending_end: now + PENDING,
                cap: now + APPROVAL,
                approved_end: Some(now + APPROVAL),
                closed: false,
                callback_outstanding: false,
            }),
        }),
        lease,
    };
    assert_eq!(
        owner.control_reply(),
        crate::remote_host_controller::HostControlReply::Consumed
    );
    owner.lease.cancel();
    assert_eq!(
        owner.control_reply(),
        crate::remote_host_controller::HostControlReply::Cancelled
    );
    owner.machine.state.lock().approved_end = Some(now - Duration::from_millis(1));
    assert_eq!(
        owner.control_reply(),
        crate::remote_host_controller::HostControlReply::Expired
    );
}
