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
enum Slot {
    Empty,
    Ready(CompleteRetirement),
    NextStarting,
    Unknown,
}
pub(super) struct RetirementCustody {
    observer: tokio::sync::Mutex<()>,
    slot: Mutex<Slot>,
}
impl RetirementCustody {
    pub(super) fn new() -> Self {
        Self {
            observer: tokio::sync::Mutex::new(()),
            slot: Mutex::new(Slot::Empty),
        }
    }
    pub(super) fn mark_unknown(&self) {
        // 只关闭观察资格，不移除原 socket/pump/Host 任务 custody。
        *self.slot.lock() = Slot::Unknown;
    }
    pub(super) fn handoff_generation(&self) {
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
    pub(crate) async fn observe_retirement(self: &Arc<Self>) -> RetirementObservation {
        let _observer = self.retirement.observer.lock().await;
        match &*self.retirement.slot.lock() {
            Slot::Ready(_) => return RetirementObservation::Ready,
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
