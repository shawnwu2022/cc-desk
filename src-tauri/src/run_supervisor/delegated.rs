//! 单 run 的原生委托能力；没有 UI、网络或恢复授权入口。
#![allow(dead_code, reason = "原生 consent 生产入口尚未开放")]
use super::*;
use crate::remote_unattended::capability::{CloseObservation, Delegation, Ticket, VerifiedBinding};
use cc_desk_remote_host::native_host::{NativeFrameResult, NativeLocalInput, NativeSessionScope};

// 只能由原生可信边界构造；当前仅测试能创建，不接受 JS DTO 或存储快照。
pub(crate) struct NativeDelegationConsent {
    policy: Arc<Delegation>,
    origin: NativeRunNetworkOrigin,
    scope: NativeSessionScope,
}
struct Captured {
    pin: NativeRunPin,
    original: Arc<NativeRun>,
    authority: NativeRunAuthority,
    scope: NativeSessionScope,
    policy: Arc<Delegation>,
    clock_input: NativeLocalInput,
}
pub(crate) struct DelegatedRunOwner(Arc<Captured>);
pub(crate) struct DelegatedRunAdmission {
    captured: Arc<Captured>,
    ticket: Ticket,
}
impl Captured {
    // 已 capture 的原 resource/scope 不会生产重填；所有生产 take 先退休 writer，
    // native_writer_status 先 stop 原 input。时钟只看不可回退的生命期原子位，
    // 不与 canonical checkout 争锁。真正 handoff/actor 仍完整 validate。
    // 若未来允许 resource 重填，必须先增加不可回退的失效门，不能沿用此证明。
    fn clock_live(&self) -> bool {
        let Some(supervisor) = self.pin.origin.upgrade() else {
            return false;
        };
        let Some(state) = self.authority.state.upgrade() else {
            return false;
        };
        let Some(resource) = self.authority.resource.upgrade() else {
            return false;
        };
        Arc::ptr_eq(&resource, &self.original)
            && !supervisor.shutting_down.load(Ordering::SeqCst)
            && !state.stop_requested.load(Ordering::SeqCst)
            && !state.reaped.load(Ordering::SeqCst)
            && self.clock_input.canonical_status()
                == cc_desk_remote_host::native_host::NativeCanonicalStatus::Complete
    }
    fn validate(&self) -> Result<(), SafeError> {
        let supervisor = self
            .pin
            .origin
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        if supervisor.shutting_down.load(Ordering::SeqCst) {
            return Err(error("RUN_NOT_READY"));
        }
        let state = self
            .authority
            .state
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let resource = self
            .authority
            .resource
            .upgrade()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        if !Arc::ptr_eq(&resource, &self.original)
            || state.stop_requested.load(Ordering::SeqCst)
            || state.reaped.load(Ordering::SeqCst)
            || !state
                .resource
                .try_lock()
                .ok_or_else(|| {
                    #[cfg(test)]
                    eprintln!("TEST_ONLY_CAPTURE resource_slot_busy");
                    error("RUN_NOT_READY")
                })?
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.original))
        {
            return Err(error("RUN_NOT_READY"));
        }
        let canonical = state.native_canonical.try_lock().ok_or_else(|| {
            #[cfg(test)]
            eprintln!("TEST_ONLY_CAPTURE canonical_slot_busy");
            error("RUN_NOT_READY")
        })?;
        if canonical
            .as_ref()
            .ok_or_else(|| error("RUN_NOT_READY"))?
            .remote_scope()?
            != self.scope
        {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
}
impl DelegatedRunOwner {
    pub(crate) fn capture(
        admission: NativeRunAdmission,
        consent: NativeDelegationConsent,
    ) -> Result<Self, SafeError> {
        admission.validate()?;
        let scope = admission.scope()?;
        if !consent.origin.matches(&admission) || consent.scope != scope {
            return Err(error("FORBIDDEN"));
        }
        admission.validate()?;
        let clock_input = admission
            .original
            .remote_input()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let NativeRunAdmission {
            pin,
            original,
            authority,
            access: _,
        } = admission;
        let captured = Captured {
            pin,
            original,
            authority,
            scope,
            policy: consent.policy,
            clock_input,
        };
        captured.validate()?;
        Ok(Self(Arc::new(captured)))
    }
    pub(crate) fn admit(
        &self,
        current: &VerifiedBinding,
        supervisor: &NativeRunSupervisor,
    ) -> Result<DelegatedRunAdmission, SafeError> {
        if !supervisor.owns_live_pin(&self.0.pin) {
            return Err(error("FORBIDDEN"));
        }
        self.0.validate()?;
        let ticket = self
            .0
            .policy
            .begin(current)
            .map_err(|_| error("FORBIDDEN"))?;
        if let Err(e) = self.0.validate() {
            self.request_revoke();
            return Err(e);
        }
        Ok(DelegatedRunAdmission {
            captured: self.0.clone(),
            ticket,
        })
    }
    pub(crate) fn request_revoke(&self) {
        self.0.policy.request_revoke();
    }
    pub(crate) fn observe_close(&self) -> CloseObservation {
        self.0.policy.observe_close()
    }
}
impl DelegatedRunAdmission {
    pub(crate) fn write_frame(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
    ) -> Result<NativeFrameResult, SafeError> {
        self.write_inner(current, bytes, || {})
    }
    fn write_inner(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
        before: impl FnOnce() + Send + 'static,
    ) -> Result<NativeFrameResult, SafeError> {
        self.captured.validate()?;
        let input = self
            .captured
            .original
            .remote_input()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let captured = self.captured.clone();
        self.captured
            .policy
            .commit(&self.ticket, current, input, bytes, move || {
                before();
                captured.validate().is_ok()
            })
            .map_err(|_| error("FORBIDDEN"))?
            .map_err(|_| error("PTY_WRITE_FAILED"))
    }
    pub(crate) fn resize(
        &self,
        current: &VerifiedBinding,
        rows: u16,
        columns: u16,
    ) -> Result<(), SafeError> {
        self.captured.validate()?;
        let input = self
            .captured
            .original
            .remote_input()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let captured = self.captured.clone();
        self.captured
            .policy
            .resize(&self.ticket, current, input, (rows, columns), move || {
                captured.validate().is_ok()
            })
            .map_err(|_| error("FORBIDDEN"))?
            .map_err(|_| error("PTY_RESIZE_FAILED"))
    }
    #[cfg(test)]
    pub(crate) fn test_write_paused(
        &self,
        current: &VerifiedBinding,
        bytes: &[u8],
        before: impl FnOnce() + Send + 'static,
    ) -> Result<NativeFrameResult, SafeError> {
        self.write_inner(current, bytes, before)
    }
}
impl Drop for DelegatedRunAdmission {
    fn drop(&mut self) {
        // 关闭请求不等待 actor，且不冒充 producer/socket 的回收证明。
        self.captured.policy.request_revoke();
    }
}
#[cfg(test)]
impl NativeDelegationConsent {
    pub(crate) fn test_new(
        admission: &NativeRunAdmission,
        view: bool,
    ) -> (Self, VerifiedBinding, Arc<Delegation>) {
        use sha2::{Digest, Sha256};
        let scope = admission.scope().unwrap();
        let run = Sha256::digest(
            format!("{}:{}:{}", scope.session, scope.run, scope.generation).as_bytes(),
        )
        .into();
        let (policy, current) = Delegation::test_new(run, view);
        (
            Self {
                policy: policy.clone(),
                origin: admission.network_origin().unwrap(),
                scope,
            },
            current,
            policy,
        )
    }
}

impl Drop for DelegatedRunOwner {
    fn drop(&mut self) {
        self.request_revoke();
    }
}

// 独立网络票据不复用本地 effect admission 的永久 revoke Drop 语义。
pub(crate) struct DelegatedNetworkAdmission {
    brand: Arc<()>,
    captured: Arc<Captured>,
    attempt: Arc<crate::remote_unattended::capability::network::NetworkAttempt>,
    #[cfg(test)]
    before_actor: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl DelegatedRunOwner {
    pub(crate) fn admit_network(
        &self,
        current: &VerifiedBinding,
        supervisor: &NativeRunSupervisor,
    ) -> Result<DelegatedNetworkAdmission, SafeError> {
        if !supervisor.owns_live_pin(&self.0.pin) {
            return Err(error("FORBIDDEN"));
        }
        self.0.validate()?;
        let captured = self.0.clone();
        let attempt = self
            .0
            .policy
            .begin_network(current, move || captured.clock_live())
            .map_err(|_| error("FORBIDDEN"))?;
        Ok(DelegatedNetworkAdmission {
            brand: Arc::new(()),
            captured: self.0.clone(),
            attempt,
            #[cfg(test)]
            before_actor: std::sync::Mutex::new(None),
        })
    }
}
impl DelegatedNetworkAdmission {
    pub(crate) fn original_network_attempt(
        &self,
    ) -> Arc<crate::remote_unattended::capability::network::NetworkAttempt> {
        self.attempt.clone()
    }
    pub(crate) fn clock(
        &self,
        domain: &Arc<crate::remote_unattended::capability::network::NetworkClock>,
    ) -> Result<Arc<dyn cc_desk_remote_security::GrantClock>, SafeError> {
        domain
            .bind(&self.attempt)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        Ok(domain.clone())
    }
    pub(crate) fn scope(&self) -> NativeSessionScope {
        self.captured.scope.clone()
    }
    pub(crate) fn brand(&self) -> Arc<()> {
        self.brand.clone()
    }
    pub(crate) fn close(&self) {
        self.attempt.close();
    }
    fn validate_channel(
        &self,
        channel: &cc_desk_remote_host::native_host::NativeChannel,
    ) -> Result<(), SafeError> {
        self.captured.validate()?;
        if self.scope() != *channel.authority().scope()
            || !self.attempt.matches_channel(&channel.authority())
        {
            #[cfg(test)]
            eprintln!(
                "TEST_ONLY_DELEGATED_VALIDATE scope={} attempt={}",
                self.scope() == *channel.authority().scope(),
                self.attempt.matches_channel(&channel.authority())
            );
            return Err(error("REMOTE_HOST_AUTHORITY"));
        }
        Ok(())
    }
    pub(crate) fn create_host(
        &self,
        prepared: &crate::remote_host::delegated::DelegatedPreparedChannel,
    ) -> Result<cc_desk_remote_host::native_host::NativeHost, SafeError> {
        let channel = prepared.channel_for(&self.brand)?;
        self.validate_channel(&channel)?;
        let state = self
            .captured
            .pin
            .state
            .as_ref()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let checkout = NativeProducerCheckout::take(state)?;
        self.validate_channel(&channel)?;
        #[cfg(test)]
        eprintln!(
            "TEST_ONLY_DELEGATED_FACTORY peer={} complete={} scope={} authority={:?}",
            channel.peer_install_enabled(),
            checkout.producer.as_ref().unwrap().complete_from_zero(),
            checkout.producer.as_ref().unwrap().native_scope() == *channel.authority().scope(),
            channel.authority().check()
        );
        cc_desk_remote_host::native_host::NativeHost::for_native_resource(
            channel,
            checkout.producer.as_ref().expect("original producer"),
        )
        .map_err(|_e| {
            #[cfg(test)]
            eprintln!("TEST_ONLY_DELEGATED_FACTORY error={_e:?}");
            error("REMOTE_HOST_AUTHORITY")
        })
    }
    pub(crate) fn adopt_into(
        &self,
        host: &mut cc_desk_remote_host::native_host::NativeHost,
        prepared: &crate::remote_host::delegated::DelegatedPreparedChannel,
    ) -> Result<(), SafeError> {
        let channel = prepared.channel_for(&self.brand)?;
        self.validate_channel(&channel)?;
        let state = self
            .captured
            .pin
            .state
            .as_ref()
            .ok_or_else(|| error("RUN_NOT_READY"))?;
        let mut checkout = NativeProducerCheckout::take(state)?;
        self.validate_channel(&channel)?;
        let captured = self.captured.clone();
        let attempt = self.attempt.clone();
        let authority = channel.authority();
        #[cfg(test)]
        let before = self.before_actor.lock().unwrap().take();
        host.adopt_native_producer_slot(
            &mut checkout.producer,
            channel,
            Box::new(move || {
                #[cfg(test)]
                if let Some(before) = before {
                    before();
                }
                attempt.matches_channel(&authority)
                    && captured.validate().is_ok()
                    && attempt.matches_channel(&authority)
            }),
        )
        .map(|_| ())
        .map_err(|_| error("REMOTE_HOST_AUTHORITY"))
    }
    #[cfg(test)]
    pub(crate) fn test_clock_with_slots_locked(
        &self,
        clock: &dyn cc_desk_remote_security::GrantClock,
    ) -> Result<u64, cc_desk_remote_security::GrantError> {
        let state = self.captured.authority.state.upgrade().unwrap();
        let _resource = state.resource.lock();
        let _canonical = state.native_canonical.lock();
        clock.now_ms()
    }
    #[cfg(test)]
    pub(crate) fn test_stop_original_input(&self) {
        self.captured.clock_input.stop();
    }
    #[cfg(test)]
    pub(crate) fn test_before_actor(&mut self, hook: impl FnOnce() + Send + 'static) {
        *self.before_actor.lock().unwrap() = Some(Box::new(hook));
    }
    pub(crate) fn pin(&self) -> NativeRunPin {
        NativeRunPin {
            run: self.captured.pin.run.clone(),
            origin: self.captured.pin.origin.clone(),
            state: self.captured.pin.state.clone(),
        }
    }
    pub(crate) fn origin(&self) -> NativeRunNetworkOrigin {
        NativeRunNetworkOrigin {
            supervisor: self.captured.pin.origin.clone(),
            state: self.captured.authority.state.clone(),
            resource: self.captured.authority.resource.clone(),
            run: self.captured.pin.run.clone(),
        }
    }
}
impl Drop for DelegatedNetworkAdmission {
    fn drop(&mut self) {
        self.attempt.close();
    }
}
#[cfg(test)]
impl NativeDelegationConsent {
    pub(crate) fn test_network(
        admission: &NativeRunAdmission,
        device: [u8; 32],
    ) -> (Self, VerifiedBinding, Arc<Delegation>) {
        let (consent, mut current, policy) = Self::test_new(admission, false);
        policy.test_network_identity(&mut current, device);
        (consent, current, policy)
    }
}

// 仅从原 Captured 完整核验后取得；提交点借用已持有对象，不升级 Weak 或执行回调。
pub(crate) struct NativeRunLifetime {
    captured: Arc<Captured>,
    supervisor: Arc<SupervisorCore>,
    state: Arc<SupervisedRun>,
}
impl NativeRunLifetime {
    pub(crate) fn matches_policy(&self, policy: &Arc<Delegation>) -> bool {
        Arc::ptr_eq(policy, &self.captured.policy)
    }
    pub(crate) fn live(&self) -> bool {
        !self.supervisor.shutting_down.load(Ordering::SeqCst)
            && !self.state.stop_requested.load(Ordering::SeqCst)
            && !self.state.reaped.load(Ordering::SeqCst)
            && self.captured.clock_input.canonical_status()
                == cc_desk_remote_host::native_host::NativeCanonicalStatus::Complete
    }
}
impl DelegatedRunOwner {
    pub(crate) fn network_lifetime(
        &self,
        supervisor: &NativeRunSupervisor,
    ) -> Result<NativeRunLifetime, SafeError> {
        if !supervisor.owns_live_pin(&self.0.pin) {
            return Err(error("FORBIDDEN"));
        }
        self.0.validate()?;
        let lifetime = NativeRunLifetime {
            captured: self.0.clone(),
            supervisor: self
                .0
                .pin
                .origin
                .upgrade()
                .ok_or_else(|| error("RUN_NOT_READY"))?,
            state: self
                .0
                .authority
                .state
                .upgrade()
                .ok_or_else(|| error("RUN_NOT_READY"))?,
        };
        if !lifetime.live() {
            return Err(error("RUN_NOT_READY"));
        }
        Ok(lifetime)
    }
    pub(crate) fn resume_network(
        &self,
        receipt: &crate::remote_host::delegated::retirement::CompleteRetirement,
        current: &VerifiedBinding,
        clock: &crate::remote_unattended::capability::network::NetworkClock,
        lifetime: &NativeRunLifetime,
    ) -> Result<DelegatedNetworkAdmission, SafeError> {
        if !Arc::ptr_eq(&self.0, &lifetime.captured) {
            return Err(error("FORBIDDEN"));
        }
        let attempt = clock
            .renew(receipt, current, lifetime)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        Ok(DelegatedNetworkAdmission {
            brand: Arc::new(()),
            captured: self.0.clone(),
            attempt,
            #[cfg(test)]
            before_actor: std::sync::Mutex::new(None),
        })
    }
}
impl DelegatedNetworkAdmission {
    pub(crate) fn rebind_into(
        &self,
        host: &mut cc_desk_remote_host::native_host::NativeHost,
        prepared: &crate::remote_host::delegated::DelegatedPreparedChannel,
        retired: cc_desk_remote_host::native_host::RetiredInstalledChannel,
    ) -> Result<(), SafeError> {
        let channel = prepared.channel_for(&self.brand)?;
        self.validate_channel(&channel)?;
        let captured = self.captured.clone();
        let attempt = self.attempt.clone();
        let authority = channel.authority();
        host.begin_native_resource_rebind_retired(
            retired,
            channel,
            Box::new(move || {
                attempt.matches_channel(&authority)
                    && captured.validate().is_ok()
                    && attempt.matches_channel(&authority)
            }),
        )
        .map_err(|_| error("REMOTE_HOST_AUTHORITY"))
    }
}

// TEST_ONLY：持原资源锁建立屏障，不改资源状态或生产权限。
#[cfg(test)]
pub(crate) fn test_hold_admission_resource(admission: &NativeRunAdmission, work: impl FnOnce()) {
    let state = admission.authority.state.upgrade().unwrap();
    let _resource = state.resource.lock();
    work();
}
