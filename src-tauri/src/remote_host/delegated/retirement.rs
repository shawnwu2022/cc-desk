//! 同一原 generation 的真实回收汇合；唯一 receipt 仍只保存在 Session 内。
use super::*;
use crate::remote_unattended::capability::network::NetworkAttempt;
use cc_desk_remote_host::native_host::{ChannelRetirement, RetiredInstalledChannel};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetirementObservation {
    Pending,
    Ready,
    Unknown,
}
pub(crate) struct CompleteRetirement {
    identity: DelegatedAttemptIdentity,
    attempt: Arc<NetworkAttempt>,
    channel: Arc<NativeChannel>,
    installed: RetiredInstalledChannel,
}
impl CompleteRetirement {
    pub(crate) fn original_attempt(&self) -> &Arc<NetworkAttempt> {
        &self.attempt
    }
}
pub(crate) struct UninstalledRetirement {
    start: DelegatedSocketStart,
    attempt: Arc<NetworkAttempt>,
}
impl UninstalledRetirement {
    pub(crate) fn original_attempt(&self) -> &Arc<NetworkAttempt> {
        &self.attempt
    }
}

enum Slot {
    Empty,
    Ready(CompleteRetirement),
    Waiting(UninstalledRetirement),
    NextStarting,
    Unknown,
}
pub(super) struct RetirementCustody {
    observer: tokio::sync::Mutex<()>,
    slot: Mutex<Slot>,
    waiting_retries: std::sync::atomic::AtomicU8,
}
impl RetirementCustody {
    pub(super) fn new() -> Self {
        Self {
            observer: tokio::sync::Mutex::new(()),
            slot: Mutex::new(Slot::Empty),
            waiting_retries: std::sync::atomic::AtomicU8::new(0),
        }
    }
    pub(super) fn mark_unknown(&self) {
        // 只关闭观察资格，不移除原 socket/pump/Host 任务 custody。
        *self.slot.lock() = Slot::Unknown;
    }
    pub(super) fn handoff_generation(&self, active: &Arc<()>, handed_off: &Arc<()>) {
        if !Arc::ptr_eq(active, handed_off) {
            return;
        }
        let mut slot = self.slot.lock();
        if matches!(*slot, Slot::NextStarting) {
            *slot = Slot::Empty;
        }
    }
}
impl DelegatedSession {
    fn retirement_unknown(&self) -> RetirementObservation {
        *self.retirement.slot.lock() = Slot::Unknown;
        self.owner.request_revoke();
        self.clock.close();
        RetirementObservation::Unknown
    }
    fn local_retirement(&self) -> ChannelRetirement {
        let Some(service) = self.service.upgrade() else {
            return ChannelRetirement::Unknown;
        };
        let mut active = self.active.lock();
        let Some(active) = active.as_mut() else {
            return ChannelRetirement::Unknown;
        };
        service
            .with_host(|host| {
                let Some(host) = host else {
                    return ChannelRetirement::Unknown;
                };
                if active.local.is_none() {
                    match host.begin_channel_retirement(&active.channel) {
                        Ok(token) => active.local = Some(token),
                        Err(_) => return ChannelRetirement::Unknown,
                    }
                }
                host.observe_channel_retirement(active.local.as_ref().unwrap())
            })
            .unwrap_or(ChannelRetirement::Pending)
    }
    pub(crate) fn waiting_retry_available(&self) -> bool {
        self.retirement
            .waiting_retries
            .load(std::sync::atomic::Ordering::Acquire)
            < 3
    }
    pub(crate) async fn observe_waiting_retirement(self: &Arc<Self>) -> RetirementObservation {
        let _observer = self.retirement.observer.lock().await;
        if matches!(*self.retirement.slot.lock(), Slot::Waiting(_)) {
            return RetirementObservation::Ready;
        }
        let socket = self.socket.lock().clone();
        let Some(socket) = socket else {
            return self.retirement_unknown();
        };
        let start = match socket.take_waiting_start().await {
            Ok(Some(start)) => start,
            _ => return self.retirement_unknown(),
        };
        let _transition = self.transition.lock();
        if self.clock.is_closed()
            || self.reservation.operation().cancelled()
            || !self
                .socket
                .lock()
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &socket))
            || !socket.matches_identity(&start.identity)
            || !Arc::ptr_eq(&start.identity.session, &self.brand)
            || !Arc::ptr_eq(&start.identity.operation, &self.reservation.operation())
            || !start
                .session
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, self))
        {
            return self.retirement_unknown();
        }
        let Some(admission) = start.admission.as_ref() else {
            return self.retirement_unknown();
        };
        if !Arc::ptr_eq(&admission.brand(), &start.identity.attempt) {
            return self.retirement_unknown();
        }
        let attempt = admission.original_network_attempt();
        if start.retired.is_some() != self.active.lock().is_some() {
            return self.retirement_unknown();
        }
        let mut slot = self.retirement.slot.lock();
        if !matches!(*slot, Slot::Empty | Slot::NextStarting) {
            drop(slot);
            return self.retirement_unknown();
        }
        *slot = Slot::Waiting(UninstalledRetirement { start, attempt });
        RetirementObservation::Ready
    }
    pub(crate) async fn observe_retirement(self: &Arc<Self>) -> RetirementObservation {
        let _observer = self.retirement.observer.lock().await;
        match &*self.retirement.slot.lock() {
            Slot::Ready(_) | Slot::Waiting(_) => return RetirementObservation::Ready,
            Slot::Unknown => return RetirementObservation::Unknown,
            Slot::NextStarting => return RetirementObservation::Pending,
            Slot::Empty => {}
        }
        self.disconnect();
        let (identity, attempt, channel, pump) = {
            let active = self.active.lock();
            let Some(active) = active.as_ref() else {
                return self.retirement_unknown();
            };
            (
                DelegatedAttemptIdentity {
                    session: self.brand.clone(),
                    operation: self.reservation.operation(),
                    attempt: active.admission.brand(),
                },
                active.admission.original_network_attempt(),
                active.channel.clone(),
                active.pump.clone(),
            )
        };
        let socket = self.socket.lock().clone();
        let Some(socket) = socket else {
            return self.retirement_unknown();
        };
        if !socket.matches_original(&identity, &channel) {
            return self.retirement_unknown();
        }
        // 所有等待只借用原句柄；取消本观察不会取走或丢弃它们。
        let (network, pump, local) = tokio::join!(
            async {
                let result = socket.wait_closed().await;
                if result.is_err() {
                    self.retirement_unknown();
                }
                result
            },
            async {
                let result = pump.join().await;
                if !result {
                    self.retirement_unknown();
                }
                result
            },
            async {
                loop {
                    match self.local_retirement() {
                        ChannelRetirement::Retired => return true,
                        ChannelRetirement::Unknown => {
                            self.retirement_unknown();
                            return false;
                        }
                        ChannelRetirement::Pending => {
                            tokio::time::sleep(std::time::Duration::from_millis(2)).await
                        }
                    }
                }
            }
        );
        if network.is_err() || !pump || !local || self.clock.is_closed() {
            return self.retirement_unknown();
        }
        if !socket.matches_original(&identity, &channel) {
            return self.retirement_unknown();
        }
        let Some(service) = self.service.upgrade() else {
            return self.retirement_unknown();
        };
        let ready = {
            let active = self.active.lock();
            let Some(active) = active.as_ref() else {
                return self.retirement_unknown();
            };
            if !Arc::ptr_eq(&active.admission.brand(), &identity.attempt) {
                return self.retirement_unknown();
            }
            service.with_host(|host| {
                host.ok_or(())?
                    .retired_installed_channel(active.local.as_ref().ok_or(())?)
                    .map_err(|_| ())
            })
        };
        let Some(ready) = ready else {
            return RetirementObservation::Pending;
        };
        let Ok(Some(installed)) = ready else {
            return self.retirement_unknown();
        };
        if self.clock.is_closed() {
            return self.retirement_unknown();
        }
        // 独占 observer 下，mint 后到唯一 slot 写入之间没有 await。
        *self.retirement.slot.lock() = Slot::Ready(CompleteRetirement {
            identity,
            attempt,
            channel,
            installed,
        });
        RetirementObservation::Ready
    }
}

impl DelegatedSession {
    pub(crate) fn prepare_next(
        self: &Arc<Self>,
        current: &crate::remote_unattended::capability::VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            DelegatedSocketStart,
        ),
        SafeError,
    > {
        if matches!(*self.retirement.slot.lock(), Slot::Waiting(_)) {
            return self.prepare_after_wait(current, supervisor);
        }
        let lifetime = self.owner.network_lifetime(supervisor)?;
        let service = self
            .service
            .upgrade()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let _transition = self.transition.lock();
        if self.clock.is_closed() || self.reservation.operation().cancelled() {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let mut slot = self.retirement.slot.lock();
        let Slot::Ready(receipt) = &*slot else {
            return Err(crate::cli::profiles::error("REMOTE_HOST_BUSY"));
        };
        let exact = self.active.lock().as_ref().is_some_and(|active| {
            Arc::ptr_eq(&active.admission.brand(), &receipt.identity.attempt)
                && Arc::ptr_eq(&active.channel, &receipt.channel)
        });
        if !exact
            || !Arc::ptr_eq(&receipt.identity.session, &self.brand)
            || !Arc::ptr_eq(&receipt.identity.operation, &self.reservation.operation())
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let Slot::Ready(receipt) = std::mem::replace(&mut *slot, Slot::NextStarting) else {
            unreachable!()
        };
        let admission = match self
            .owner
            .resume_network(&receipt, current, &self.clock, &lifetime)
        {
            Ok(a) => a,
            Err(e) => {
                *slot = Slot::Unknown;
                self.owner.request_revoke();
                self.clock.close();
                return Err(e);
            }
        };
        let identity = DelegatedAttemptIdentity {
            session: self.brand.clone(),
            operation: self.reservation.operation(),
            attempt: admission.brand(),
        };
        let start = DelegatedSocketStart {
            request_gate: None,
            identity,
            session: Arc::downgrade(self),
            admission: Some(admission),
            clock: self.clock.clone(),
            retired: Some(receipt.installed),
        };
        let custody =
            crate::remote_host_network::delegated::SocketCustody::reserve(&service, &start);
        *self.socket.lock() = Some(custody.clone());
        if self.clock.is_closed() || self.reservation.operation().cancelled() {
            *slot = Slot::Unknown;
            self.owner.request_revoke();
            self.clock.close();
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        Ok((custody, start))
    }
}

impl DelegatedSession {
    fn prepare_after_wait(
        self: &Arc<Self>,
        current: &crate::remote_unattended::capability::VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            DelegatedSocketStart,
        ),
        SafeError,
    > {
        let lifetime = self.owner.network_lifetime(supervisor)?;
        let service = self
            .service
            .upgrade()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let _transition = self.transition.lock();
        let mut slot = self.retirement.slot.lock();
        let Slot::Waiting(receipt) = &*slot else {
            return Err(crate::cli::profiles::error("REMOTE_HOST_BUSY"));
        };
        if self.clock.is_closed()
            || self.reservation.operation().cancelled()
            || !self
                .socket
                .lock()
                .as_ref()
                .is_some_and(|socket| socket.matches_identity(&receipt.start.identity))
            || !Arc::ptr_eq(&receipt.start.identity.session, &self.brand)
            || self
                .retirement
                .waiting_retries
                .load(std::sync::atomic::Ordering::Acquire)
                >= 3
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        self.retirement
            .waiting_retries
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let Slot::Waiting(mut receipt) = std::mem::replace(&mut *slot, Slot::NextStarting) else {
            unreachable!()
        };
        let admission =
            match self
                .owner
                .resume_waiting_network(&receipt, current, &self.clock, &lifetime)
            {
                Ok(admission) => admission,
                Err(error) => {
                    *slot = Slot::Unknown;
                    self.owner.request_revoke();
                    self.clock.close();
                    return Err(error);
                }
            };
        let identity = DelegatedAttemptIdentity {
            session: self.brand.clone(),
            operation: self.reservation.operation(),
            attempt: admission.brand(),
        };
        let start = DelegatedSocketStart {
            identity,
            session: Arc::downgrade(self),
            admission: Some(admission),
            clock: self.clock.clone(),
            request_gate: None,
            retired: receipt.start.retired.take(),
        };
        let socket =
            crate::remote_host_network::delegated::SocketCustody::reserve(&service, &start);
        *self.socket.lock() = Some(socket.clone());
        if self.clock.is_closed() || self.reservation.operation().cancelled() {
            *slot = Slot::Unknown;
            self.owner.request_revoke();
            self.clock.close();
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        Ok((socket, start))
    }
}

impl DelegatedSession {
    // 单次短会话终止，不签发可重连的 Installed 退休收据。
    pub(crate) async fn close_terminal(self: &Arc<Self>) -> bool {
        self.revoke();
        let _observer = self.retirement.observer.lock().await;
        let socket = self.socket.lock().clone();
        let Some(socket) = socket else {
            return false;
        };
        if socket.wait_closed().await.is_err() {
            return false;
        }
        let pump = self
            .active
            .lock()
            .as_ref()
            .map(|active| active.pump.clone());
        let Some(pump) = pump else {
            // handoff 之前失败只有网络/材料任务；若已产生部分 Host，不能假定其退休。
            return self
                .service
                .upgrade()
                .and_then(|service| service.with_host(|host| host.is_none()))
                == Some(true);
        };
        if !pump.join().await {
            return false;
        }
        loop {
            match self.local_retirement() {
                ChannelRetirement::Retired => return true,
                ChannelRetirement::Unknown => return false,
                ChannelRetirement::Pending => {
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await
                }
            }
        }
    }
}

#[cfg(test)]
mod waiting_handoff_tests {
    use super::*;
    #[test]
    fn waiting_initial_empty_handoff_is_noop_and_foreign_cannot_clear_starting() {
        let custody = RetirementCustody::new();
        let active = Arc::new(());
        let foreign = Arc::new(());
        custody.handoff_generation(&active, &active);
        assert!(matches!(*custody.slot.lock(), Slot::Empty));
        *custody.slot.lock() = Slot::NextStarting;
        custody.handoff_generation(&active, &foreign);
        assert!(matches!(*custody.slot.lock(), Slot::NextStarting));
        custody.handoff_generation(&active, &active);
        assert!(matches!(*custody.slot.lock(), Slot::Empty));
        *custody.slot.lock() = Slot::Unknown;
        custody.handoff_generation(&active, &active);
        assert!(matches!(*custody.slot.lock(), Slot::Unknown));
    }
}
