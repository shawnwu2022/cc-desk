//! 单次封闭委托接线；生产材料构造与完整回收证明仍未开放。
#![allow(dead_code, reason = "委托生产 consent 与材料供应尚未接入")]
use super::*;
use crate::cli::types::SafeError;
use crate::remote_unattended::capability::network::NetworkClock;
#[cfg(test)]
use crate::remote_unattended::capability::VerifiedBinding;
use crate::run_supervisor::delegated::{DelegatedNetworkAdmission, DelegatedRunOwner};
use cc_desk_remote_host::native_host::{ChannelRetirementToken, NativeChannel};
use std::sync::Weak;

pub(crate) struct DelegatedSession {
    service: Weak<RemoteHostService>,
    brand: Arc<()>,
    clock: Arc<NetworkClock>,
    owner: DelegatedRunOwner,
    reservation: ActivationReservation,
    active: Mutex<Option<ActiveAttempt>>,
}
struct ActiveAttempt {
    admission: DelegatedNetworkAdmission,
    channel: Arc<NativeChannel>,
    pump: Arc<PeerPump>,
    local: Option<ChannelRetirementToken>,
}
pub(crate) struct DelegatedPreparedChannel {
    brand: Arc<()>,
    attempt_brand: Arc<()>,
    admission: Option<DelegatedNetworkAdmission>,
    channel: Arc<NativeChannel>,
    adopted: bool,
}
impl DelegatedPreparedChannel {
    pub(crate) fn channel_for(&self, brand: &Arc<()>) -> Result<Arc<NativeChannel>, SafeError> {
        if !Arc::ptr_eq(brand, &self.attempt_brand) {
            return Err(crate::cli::profiles::error("REMOTE_HOST_AUTHORITY"));
        }
        Ok(self.channel.clone())
    }
}
impl Drop for DelegatedPreparedChannel {
    fn drop(&mut self) {
        if !self.adopted {
            self.channel.authority().request_close();
            if let Some(admission) = &self.admission {
                admission.close();
            }
        }
    }
}
impl RemoteHostService {
    pub(crate) fn reserve_delegation(
        self: &Arc<Self>,
        owner: DelegatedRunOwner,
    ) -> Result<DelegatedSession, SafeError> {
        let mut state = self.state.lock();
        if state.closing
            || state.retired
            || state.activation.is_some()
            || state.one_shot_claimed
            || state.delegation.is_some()
            || !matches!(state.host, HostSlot::Empty)
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let brand = Arc::new(());
        let reservation = ActivationReservation::new();
        state.activation = Some(reservation.operation());
        state.delegation = Some(brand.clone());
        Ok(DelegatedSession {
            service: Arc::downgrade(self),
            brand,
            clock: Arc::new(NetworkClock::new()),
            owner,
            reservation,
            active: Mutex::new(None),
        })
    }
}
impl DelegatedSession {
    pub(crate) fn adopt(&self, mut prepared: DelegatedPreparedChannel) -> Result<(), SafeError> {
        if !Arc::ptr_eq(&self.brand, &prepared.brand)
            || self.reservation.operation().cancelled()
            || self.clock.is_closed()
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let service = self
            .service
            .upgrade()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let mut active = self
            .active
            .try_lock()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_BUSY"))?;
        if active.is_some() {
            return Err(crate::cli::profiles::error("REMOTE_HOST_BUSY"));
        }
        let admission = prepared
            .admission
            .take()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let mut checkout = service
            .checkout_host_for(true, Some(&self.brand))
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_BUSY"))?;
        let result = (|| {
            // 即使下游拒绝也保留同一个永久 Host，不制造替代 producer。
            if checkout.host.is_none() {
                checkout.host = Some(Box::new(admission.create_host(&prepared).inspect_err(
                    |_e| {
                        #[cfg(test)]
                        eprintln!("TEST_ONLY_DELEGATED_ADOPT create_host_failed {}", _e.code);
                    },
                )?));
            }
            if checkout.origin.is_some() {
                return Err(crate::cli::profiles::error("REMOTE_PRODUCER_OWNED"));
            }
            admission
                .adopt_into(
                    checkout.host.as_deref_mut().expect("original Host"),
                    &prepared,
                )
                .inspect_err(|_e| {
                    #[cfg(test)]
                    eprintln!("TEST_ONLY_DELEGATED_ADOPT handoff_failed {}", _e.code);
                })?;
            let origin = admission.origin();
            let pin = admission.pin();
            let original = service.clone();
            let channel = prepared.channel.clone();
            let worker =
                tauri::async_runtime::spawn(async move { original.pump_peer(channel).await });
            let pump = Arc::new(PeerPump {
                join: tokio::sync::Mutex::new(Some(worker)),
                joined: AtomicU8::new(0),
            });
            {
                let mut state = service.state.lock();
                state.origin = Some(origin);
                state.pins.push(pin);
                state.peer_pumps.push(pump.clone());
            }
            // 这里只登记原任务 Arc，绝不再次启动 dispatcher。
            *active = Some(ActiveAttempt {
                admission,
                channel: prepared.channel.clone(),
                pump,
                local: None,
            });
            prepared.adopted = true;
            Ok(())
        })();
        checkout.finish();
        result
    }
    pub(crate) fn disconnect(&self) {
        // Starting 尚未进入 active，也必须同步关闭这代安装及其固定时钟。
        self.clock.close();
        let mut active = self.active.lock();
        if let Some(attempt) = active.as_mut() {
            attempt.admission.close();
            if attempt.local.is_none() {
                if let Some(service) = self.service.upgrade() {
                    attempt.local = service
                        .with_host(|host| {
                            host.and_then(|h| h.begin_channel_retirement(&attempt.channel).ok())
                        })
                        .flatten();
                }
            }
            attempt.channel.authority().request_close();
        }
        // 本阶段没有完整 receipt：即使本地已关，也不清空 active 或 live。
    }
    pub(crate) fn revoke(&self) {
        self.owner.request_revoke();
        self.disconnect();
    }
}
impl Drop for DelegatedSession {
    fn drop(&mut self) {
        self.revoke();
    }
}

#[cfg(test)]
impl DelegatedPreparedChannel {
    pub(crate) fn test_before_actor(&mut self, hook: impl FnOnce() + Send + 'static) {
        self.admission.as_mut().unwrap().test_before_actor(hook);
    }
}
#[cfg(test)]
impl DelegatedSession {
    pub(crate) async fn test_prepare(
        &self,
        current: &VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
        wrong_scope: bool,
    ) -> Result<
        (
            DelegatedPreparedChannel,
            NativeChannel,
            cc_desk_remote_security::GrantGate,
            cc_desk_remote_security::CancelHandle,
        ),
        SafeError,
    > {
        if self.clock.is_closed() || self.reservation.operation().cancelled() {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let admission = self.owner.admit_network(current, supervisor)?;
        let mut scope = admission.scope();
        if wrong_scope {
            scope.run.push_str("-wrong");
        }
        // 唯一构造点亲自运行握手，时钟只能来自原 sealed admission。
        let (client, channel, gate, cancel) = crate::tests::remote_channels::channels_with_clock(
            scope,
            admission.clock(&self.clock)?,
        )
        .await;
        Ok((
            DelegatedPreparedChannel {
                brand: self.brand.clone(),
                attempt_brand: admission.brand(),
                admission: Some(admission),
                channel: Arc::new(channel),
                adopted: false,
            },
            client,
            gate,
            cancel,
        ))
    }
    pub(crate) async fn test_join_pump(&self) -> bool {
        let pump = self
            .active
            .lock()
            .as_ref()
            .map(|active| active.pump.clone());
        match pump {
            Some(pump) => pump.join().await,
            None => false,
        }
    }
}
