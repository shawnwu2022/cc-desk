//! 单代网络时钟与关闭栅栏；不签发回收证明，不从持久记录恢复权限。
use super::*;
use cc_desk_remote_security::{GrantClock, GrantError};

pub(crate) struct NetworkAttempt {
    policy: Arc<Delegation>,
    ticket: Attempt,
    current: VerifiedBinding,
    closed: AtomicBool,
    resource: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl Delegation {
    pub(crate) fn begin_network(
        self: &Arc<Self>,
        current: &VerifiedBinding,
        resource: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<Arc<NetworkAttempt>, Denied> {
        if current.0.role != Role::Control || !resource() {
            return Err(Denied::Locked);
        }
        let ticket = self.begin(current)?.0;
        let attempt = Arc::new(NetworkAttempt {
            policy: self.clone(),
            ticket,
            current: current.clone(),
            closed: AtomicBool::new(false),
            resource: Arc::new(resource),
        });
        if attempt.now_ms().is_err() {
            attempt.close();
            return Err(Denied::Locked);
        }
        Ok(attempt)
    }
}
impl NetworkAttempt {
    // 普通断线只关闭本代；没有完整回收事实便始终占据原 live 槽。
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut state) = self.policy.owner.state.try_lock() {
            if self.policy.owner.same_attempt(&state, &self.ticket) {
                if let Some(live) = &mut state.live {
                    if live.phase != Phase::Unknown {
                        live.phase = Phase::Retiring;
                    }
                }
            }
        }
    }
    pub(crate) fn same_attempt(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.policy, &other.policy)
            && self.ticket.sequence == other.ticket.sequence
            && self.ticket.epoch == other.ticket.epoch
            && self.ticket.revision == other.ticket.revision
    }
    pub(crate) fn matches_channel(
        &self,
        authority: &cc_desk_remote_security::BoundDataAuthority,
    ) -> bool {
        let binding = &self.current.0;
        let context = authority.context();
        let device: String = binding
            .device
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        authority.local_role() == cc_desk_remote_security::Role::Host
            && authority.data_role() == cc_desk_remote_record::grant::DataRole::Controller
            && context.client_endpoint_id == device
            && context.host_key_epoch == binding.host_epoch.to_string()
            && context.client_key_epoch == binding.client_epoch.to_string()
            && context.policy_epoch == binding.policy_epoch.to_string()
            && context.issuer_epoch == binding.issuer_epoch.to_string()
            && context.origin.as_str() == binding.origin
            && authority.principal_id() == binding.principal
            && self.now_ms().is_ok()
            && authority.check().is_ok()
    }
    fn sample_checked(&self) -> Result<u64, GrantError> {
        if self.closed.load(Ordering::Acquire) || self.policy.closed.load(Ordering::Acquire) {
            return Err(GrantError::Closed);
        }
        // 原资源检查在锁外且必须非阻塞；随后重查关闭、期限和原票据。
        if !(self.resource)() {
            #[cfg(test)]
            eprintln!("TEST_ONLY_DELEGATED_CLOCK resource_rejected");
            self.policy.request_revoke();
            return Err(GrantError::Revoked);
        }
        // 网络持私有 Attempt，不能转换为本地 effect 的 Ticket。唯一 live 阻止有效
        // 本地 effect 并存；旧票据在 with_effect 的 IO 之前即被拒绝。因此这里只
        // 串行短状态检查，锁内无 resource 回调、IO 或 await。未来换代必须保留此约束。
        let mut state = self.policy.owner.state.lock().map_err(|_| {
            self.policy.request_revoke();
            GrantError::Closed
        })?;
        if self.closed.load(Ordering::Acquire) || self.policy.closed.load(Ordering::Acquire) {
            return Err(GrantError::Closed);
        }
        let sample = self.policy.sample().map_err(|_| GrantError::Clock)?;
        self.policy
            .owner
            .check_attempt_locked(&mut state, &self.ticket, &self.current.0, sample, false)
            .map_err(|_| {
                self.closed.store(true, Ordering::Release);
                GrantError::Expired
            })?;
        if self.closed.load(Ordering::Acquire) || self.policy.closed.load(Ordering::Acquire) {
            return Err(GrantError::Closed);
        }
        Ok(sample.wall_ms)
    }
}
impl GrantClock for NetworkAttempt {
    fn now_ms(&self) -> Result<u64, GrantError> {
        self.sample_checked()
    }
}
impl Drop for NetworkAttempt {
    fn drop(&mut self) {
        // Drop 不是 join，也不是永久 revoke；没有 receipt 不能释放 live。
        self.close();
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    #[test]
    fn DelegatedNetwork_CloseAndDropNeverReleaseLiveTicket_001() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        assert_eq!(attempt.now_ms(), Ok(1000));
        attempt.close();
        assert!(attempt.now_ms().is_err());
        assert!(!policy.closed.load(Ordering::Acquire));
        drop(attempt);
        assert!(matches!(
            policy.begin_network(&current, || true),
            Err(Denied::Busy)
        ));
        assert!(policy.owner.state.lock().unwrap().live.is_some());
    }
    #[test]
    fn DelegatedNetwork_ResourceClockAndRevokeFailClosed_002() {
        for mode in 0..4 {
            let (policy, current) = Delegation::test_new([7; 32], false);
            let live = Arc::new(AtomicBool::new(true));
            let check = live.clone();
            let attempt = policy
                .begin_network(&current, move || check.load(Ordering::Acquire))
                .unwrap();
            match mode {
                0 => live.store(false, Ordering::Release),
                1 => policy.test_clock(true),
                2 => policy.test_clock(false),
                _ => policy.request_revoke(),
            }
            assert!(attempt.now_ms().is_err());
            live.store(true, Ordering::Release);
            policy.test_restore_clock();
            assert!(attempt.now_ms().is_err());
            assert!(policy.begin_network(&current, || true).is_err());
        }
    }
    #[test]
    fn DelegatedNetwork_ConcurrentClockSamplesSerializeOnlyShortState_004() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        let clock = Arc::new(NetworkClock::new());
        clock.bind(&attempt).unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let clock = clock.clone();
                std::thread::spawn(move || {
                    for _ in 0..64 {
                        assert_eq!(clock.now_ms(), Ok(1000));
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(matches!(policy.begin(&current), Err(Denied::Busy)));
        let (foreign, binding) = Delegation::test_new([8; 32], false);
        let local = foreign.begin(&binding).unwrap();
        let entered = AtomicBool::new(false);
        assert!(policy
            .with_effect(&local, &current, |_| entered.store(true, Ordering::Release))
            .is_err());
        assert!(!entered.load(Ordering::Acquire));
    }
    #[test]
    fn DelegatedNetwork_CloseWhileSampleWaitsRejectsAfterLock_005() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let armed = Arc::new(AtomicBool::new(false));
        let check = armed.clone();
        let (arrived, seen) = std::sync::mpsc::sync_channel(1);
        let attempt = policy
            .begin_network(&current, move || {
                if check.load(Ordering::Acquire) {
                    let _ = arrived.try_send(());
                }
                true
            })
            .unwrap();
        let state = policy.owner.state.lock().unwrap();
        armed.store(true, Ordering::Release);
        let sampling = attempt.clone();
        let worker = std::thread::spawn(move || sampling.now_ms());
        seen.recv_timeout(Duration::from_secs(2)).unwrap();
        attempt.close();
        drop(state);
        assert!(worker.join().unwrap().is_err());
        assert!(policy.owner.state.lock().unwrap().live.is_some());
    }
    #[test]
    fn DelegatedNetwork_ViewAndWrongBindingCannotEnter_003() {
        let (policy, view) = Delegation::test_new([7; 32], true);
        assert!(policy.begin_network(&view, || true).is_err());
        let (policy, current) = Delegation::test_new([7; 32], false);
        assert!(policy
            .begin_network(&current.test_wrong_device(), || true)
            .is_err());
    }
}

#[cfg(test)]
impl Delegation {
    pub(crate) fn test_network_origin(&self, current: &mut VerifiedBinding, origin: String) {
        let mut state = self.owner.state.lock().unwrap();
        assert!(state.live.is_none());
        current.0.origin = origin;
        state.binding = Some(current.0.clone());
    }
    pub(crate) fn test_network_identity(&self, current: &mut VerifiedBinding, device: [u8; 32]) {
        let mut state = self.owner.state.lock().unwrap();
        assert!(state.live.is_none());
        current.0.device = device;
        current.0.host_epoch = 5;
        current.0.client_epoch = 7;
        current.0.policy_epoch = 13;
        current.0.issuer_epoch = 17;
        current.0.principal = [4; 32];
        current.0.origin = "wss://relay.invalid/".into();
        state.binding = Some(current.0.clone());
    }
}

// Session 持有唯一 Arc 域。本阶段只允许首次绑定，后续换代须完整 receipt。
pub(crate) struct NetworkClock {
    // 首次未绑定时退休也必须留痕；此位永不复位，续代不能调用 bind。
    initial_retired: AtomicBool,
    #[cfg(test)]
    before_renew: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    after_renew: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    after_sample: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    selected: Mutex<Option<std::sync::Weak<NetworkAttempt>>>,
    terminated: AtomicBool,
}
impl NetworkClock {
    pub(crate) fn new() -> Self {
        Self {
            selected: Mutex::new(None),
            initial_retired: AtomicBool::new(false),
            #[cfg(test)]
            before_renew: Mutex::new(None),
            #[cfg(test)]
            after_renew: Mutex::new(None),
            #[cfg(test)]
            after_sample: Mutex::new(None),
            terminated: AtomicBool::new(false),
        }
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.terminated.load(Ordering::Acquire)
    }
    // 永久 Session 终止位只置 true，任何续代都不能复位。
    pub(crate) fn close(&self) {
        self.terminated.store(true, Ordering::Release);
        self.retire_current();
    }
    // 普通断线仅封原 generation，保留 selected 身份及原 live，不代表可续代。
    pub(crate) fn retire_current(&self) {
        let attempt = match self.selected.lock() {
            Ok(slot) => {
                let attempt = slot.as_ref().and_then(std::sync::Weak::upgrade);
                if let Some(attempt) = &attempt {
                    attempt.closed.store(true, Ordering::Release);
                } else {
                    self.initial_retired.store(true, Ordering::Release);
                }
                attempt
            }
            Err(_) => {
                self.terminated.store(true, Ordering::Release);
                None
            }
        };
        if let Some(attempt) = attempt {
            attempt.close();
        }
    }
    pub(crate) fn bind(&self, attempt: &Arc<NetworkAttempt>) -> Result<(), Denied> {
        let mut slot = self.selected.try_lock().map_err(|_| Denied::Busy)?;
        if self.is_closed() || self.initial_retired.load(Ordering::Acquire) || slot.is_some() {
            return Err(Denied::Busy);
        }
        *slot = Some(Arc::downgrade(attempt));
        if self.is_closed() {
            attempt.close();
            return Err(Denied::Locked);
        }
        Ok(())
    }
}
impl GrantClock for NetworkClock {
    fn now_ms(&self) -> Result<u64, GrantError> {
        if self.is_closed() {
            return Err(GrantError::Closed);
        }
        let attempt = self
            .selected
            .lock()
            .map_err(|_| {
                self.terminated.store(true, Ordering::Release);
                GrantError::Closed
            })?
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .ok_or(GrantError::Closed)?;
        let sample = attempt.now_ms().inspect_err(|error| {
            if *error != GrantError::Closed || attempt.policy.closed.load(Ordering::Acquire) {
                self.terminated.store(true, Ordering::Release);
            }
        })?;
        // sample 的 Owner guard 已释放，再复核 selected；绝不反向持 Owner→selected。
        #[cfg(test)]
        if let Some(hook) = self.after_sample.lock().unwrap().take() {
            hook();
        }
        let same = self
            .selected
            .lock()
            .map_err(|_| {
                self.terminated.store(true, Ordering::Release);
                GrantError::Closed
            })?
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|current| {
                Arc::ptr_eq(&current, &attempt) && !attempt.closed.load(Ordering::Acquire)
            });
        if self.is_closed() || !same {
            return Err(GrantError::Closed);
        }
        Ok(sample)
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod clock_tests {
    use super::*;
    #[test]
    fn DelegatedClock_RetirementDoesNotResetOrTerminateDomain_001() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        let clock = NetworkClock::new();
        clock.bind(&attempt).unwrap();
        clock.retire_current();
        assert!(!clock.is_closed());
        assert!(clock.now_ms().is_err());
        assert!(!clock.is_closed());
        assert!(clock.bind(&attempt).is_err());
        assert!(policy.owner.state.lock().unwrap().live.is_some());
        clock.close();
        assert!(clock.is_closed());
        clock.retire_current();
        assert!(clock.is_closed());
    }
    #[test]
    fn DelegatedClock_LateSampleRejectsChangedSelection_002() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let armed = Arc::new(AtomicBool::new(false));
        let armed_worker = armed.clone();
        let (entered, seen) = std::sync::mpsc::sync_channel(0);
        let (release, resume) = std::sync::mpsc::sync_channel(0);
        let resume = Mutex::new(resume);
        let attempt = policy
            .begin_network(&current, move || {
                if armed_worker.load(Ordering::Acquire) {
                    entered.send(()).unwrap();
                    resume.lock().unwrap().recv().unwrap();
                }
                true
            })
            .unwrap();
        let clock = Arc::new(NetworkClock::new());
        clock.bind(&attempt).unwrap();
        armed.store(true, Ordering::Release);
        let original = clock.clone();
        let worker = std::thread::spawn(move || original.now_ms());
        seen.recv().unwrap();
        *clock.selected.lock().unwrap() = None;
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(policy.owner.state.lock().unwrap().live.is_some());
    }
    #[test]
    fn DelegatedClock_RetirementBeforeFirstBindNeverRevives_003() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        let clock = NetworkClock::new();
        clock.retire_current();
        assert!(!clock.is_closed());
        assert!(clock.bind(&attempt).is_err());
        assert!(clock.now_ms().is_err());
    }
    #[test]
    fn DelegatedClock_RetireAfterSuccessfulSampleRejectsLateReturn_004() {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        let clock = Arc::new(NetworkClock::new());
        clock.bind(&attempt).unwrap();
        let (entered, seen) = std::sync::mpsc::sync_channel(0);
        let (release, resume) = std::sync::mpsc::sync_channel(0);
        *clock.after_sample.lock().unwrap() = Some(Box::new(move || {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        }));
        let sampling = clock.clone();
        let worker = std::thread::spawn(move || sampling.now_ms());
        seen.recv().unwrap();
        clock.retire_current();
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(!clock.is_closed());
        assert!(policy.owner.state.lock().unwrap().live.is_some());
    }
}

impl NetworkClock {
    // 唯一调用方持 Session transition；这里只按 selected→Owner，绝不执行资源回调。
    pub(crate) fn renew(
        &self,
        receipt: &crate::remote_host::delegated::retirement::CompleteRetirement,
        current: &VerifiedBinding,
        lifetime: &crate::run_supervisor::delegated::NativeRunLifetime,
    ) -> Result<Arc<NetworkAttempt>, Denied> {
        let old = receipt.original_attempt();
        let policy = &old.policy;
        let mut selected = self.selected.lock().map_err(|_| Denied::Unknown)?;
        let mut state = policy.owner.state.lock().map_err(|_| Denied::Unknown)?;
        #[cfg(test)]
        if let Some(hook) = self.before_renew.lock().unwrap().take() {
            hook();
        }
        let result = (|| {
            if self.is_closed()
                || policy.closed.load(Ordering::Acquire)
                || !old.closed.load(Ordering::Acquire)
                || !selected
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade)
                    .is_some_and(|a| Arc::ptr_eq(&a, old))
                || !lifetime.matches_policy(policy)
                || !lifetime.live()
                || current.0.role != Role::Control
                || old.ticket.epoch != state.epoch
                || old.ticket.revision != state.revision
            {
                return Err(Denied::Locked);
            }
            let now = policy.sample()?;
            Owner::check(&mut state, &current.0, now)?;
            policy.owner.finish_retirement_locked(
                &mut state,
                RetirementProof {
                    owner: old.ticket.owner.clone(),
                    sequence: old.ticket.sequence,
                },
            )?;
            let ticket = policy.owner.begin_locked(&mut state, &current.0, now)?;
            let next = Arc::new(NetworkAttempt {
                policy: policy.clone(),
                ticket,
                current: current.clone(),
                closed: AtomicBool::new(false),
                resource: old.resource.clone(),
            });
            if self.is_closed() || policy.closed.load(Ordering::Acquire) || !lifetime.live() {
                next.closed.store(true, Ordering::Release);
                return Err(Denied::Locked);
            }
            let final_now = policy.sample()?;
            policy.owner.check_attempt_locked(
                &mut state,
                &next.ticket,
                &current.0,
                final_now,
                false,
            )?;
            *selected = Some(Arc::downgrade(&next));
            #[cfg(test)]
            if let Some(hook) = self.after_renew.lock().unwrap().take() {
                hook();
            }
            if self.is_closed() || policy.closed.load(Ordering::Acquire) || !lifetime.live() {
                next.closed.store(true, Ordering::Release);
                return Err(Denied::Locked);
            }
            Ok(next)
        })();
        // 消费之后任何失败，仍持 Owner 锁先关闭；不得留下 Enabled+空 live 窗口。
        if result.is_err() {
            Owner::close(&mut state, Gate::Unknown);
            policy.closed.store(true, Ordering::Release);
            self.terminated.store(true, Ordering::Release);
        }
        result
    }
}

#[cfg(test)]
impl NetworkClock {
    pub(crate) fn test_hold_renew(&self, after: bool, hook: impl FnOnce() + Send + 'static) {
        // 仅确定性测试刻意停在提交前后；生产临界区没有此回调或等待。
        *(if after {
            &self.after_renew
        } else {
            &self.before_renew
        })
        .lock()
        .unwrap() = Some(Box::new(hook));
    }
}

#[cfg(test)]
impl Delegation {
    pub(crate) fn test_exhaust_sequence(&self) {
        self.owner.state.lock().unwrap().sequence = u64::MAX;
    }
    pub(crate) fn test_owner_gate_closed(&self) -> bool {
        self.owner.state.lock().unwrap().gate != Gate::Enabled
    }
}
