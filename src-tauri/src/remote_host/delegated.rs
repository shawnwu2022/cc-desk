//! 封闭委托及原任务回收续代；生产 consent、供给与 UI 入口仍未开放。
#![allow(dead_code, reason = "委托生产 consent 与材料供应尚未接入")]
use super::*;
use crate::cli::types::SafeError;
use crate::remote_unattended::capability::network::NetworkClock;
use crate::remote_unattended::capability::VerifiedBinding;
use crate::run_supervisor::delegated::{DelegatedNetworkAdmission, DelegatedRunOwner};
use cc_desk_remote_host::native_host::{ChannelRetirementToken, NativeChannel};
use std::sync::Weak;
#[path = "delegated/retirement.rs"]
pub(crate) mod retirement;

pub(crate) struct DelegatedSession {
    service: Weak<RemoteHostService>,
    brand: Arc<()>,
    clock: Arc<NetworkClock>,
    owner: DelegatedRunOwner,
    request_gate: std::sync::OnceLock<
        Arc<crate::remote_unattended_store::service::request_gate::ServiceRequestGate>,
    >,
    reservation: ActivationReservation,
    active: Mutex<Option<ActiveAttempt>>,
    transition: Mutex<()>,
    #[cfg(test)]
    bind_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pump_retirement: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    retirement: retirement::RetirementCustody,
    socket: Mutex<Option<Arc<crate::remote_host_network::delegated::SocketCustody>>>,
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
    retired: Option<cc_desk_remote_host::native_host::RetiredInstalledChannel>,
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
        self.try_reserve_delegation(owner)
            .map_err(|(error, _owner)| error)
    }
    pub(crate) fn try_reserve_delegation(
        self: &Arc<Self>,
        owner: DelegatedRunOwner,
    ) -> Result<DelegatedSession, (SafeError, DelegatedRunOwner)> {
        let mut state = self.state.lock();
        if state.closing
            || state.retired
            || state.activation.is_some()
            || state.one_shot_claimed
            || state.delegation.is_some()
            || !matches!(state.host, HostSlot::Empty)
        {
            return Err((crate::cli::profiles::error("REMOTE_HOST_LOCKED"), owner));
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
            request_gate: std::sync::OnceLock::new(),
            reservation,
            active: Mutex::new(None),
            transition: Mutex::new(()),
            #[cfg(test)]
            bind_hook: Mutex::new(None),
            #[cfg(test)]
            pump_retirement: Mutex::new(None),
            retirement: retirement::RetirementCustody::new(),
            socket: Mutex::new(None),
        })
    }
}
impl DelegatedSession {
    pub(crate) fn install_request_gate(
        &self,
        gate: crate::remote_unattended_store::service::request_gate::ServiceRequestGate,
    ) -> Result<(), SafeError> {
        self.request_gate
            .set(Arc::new(gate))
            .map_err(|_| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))
    }
    pub(crate) fn captured_owner(&self) -> &DelegatedRunOwner {
        &self.owner
    }
    pub(crate) fn adopt(&self, prepared: DelegatedPreparedChannel) -> Result<(), SafeError> {
        let rebind = prepared.retired.is_some();
        let result = self.adopt_inner(prepared);
        if rebind && result.is_err() {
            self.owner.request_revoke();
            self.retirement.mark_unknown();
            self.clock.close();
        }
        result
    }
    fn adopt_inner(&self, mut prepared: DelegatedPreparedChannel) -> Result<(), SafeError> {
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
        let rebind = prepared.retired.is_some();
        if active.is_some() != rebind {
            return Err(crate::cli::profiles::error("REMOTE_HOST_BUSY"));
        }
        let admission = prepared
            .admission
            .take()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        #[cfg(test)]
        let mut admission = admission;
        #[cfg(test)]
        if let Some(hook) = self.bind_hook.lock().take() {
            admission.test_before_actor(hook);
        }
        let mut checkout = service
            .checkout_host_for(true, Some(&self.brand))
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_BUSY"))?;
        let result = (|| {
            // 即使下游拒绝也保留同一个永久 Host，不制造替代 producer。
            if rebind && checkout.host.is_none() {
                return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
            }
            if checkout.host.is_none() {
                checkout.host = Some(Box::new(admission.create_host(&prepared).inspect_err(
                    |_e| {
                        #[cfg(test)]
                        eprintln!("TEST_ONLY_DELEGATED_ADOPT create_host_failed {}", _e.code);
                    },
                )?));
            }
            if !rebind && checkout.origin.is_some() {
                return Err(crate::cli::profiles::error("REMOTE_PRODUCER_OWNED"));
            }
            if let Some(retired) = prepared.retired.take() {
                admission.rebind_into(
                    checkout.host.as_deref_mut().expect("original Host"),
                    &prepared,
                    retired,
                )?;
            } else {
                admission.adopt_into(
                    checkout.host.as_deref_mut().expect("original Host"),
                    &prepared,
                )?;
            }
            let origin = admission.origin();
            let pin = admission.pin();
            let original = service.clone();
            let channel = prepared.channel.clone();
            #[cfg(test)]
            let retirement_hook = self.pump_retirement.lock().take();
            let request_gate = self.request_gate.get().cloned().map(|gate| {
                (
                    gate,
                    DelegatedAttemptIdentity {
                        session: self.brand.clone(),
                        operation: self.reservation.operation(),
                        attempt: admission.brand(),
                    },
                )
            });
            let worker = tauri::async_runtime::spawn(async move {
                let result = original.pump_peer_gated(channel, request_gate).await;
                #[cfg(test)]
                if let Some((entered, release)) = retirement_hook {
                    let _ = entered.send(());
                    let _ = release.await;
                }
                result
            });
            let pump = Arc::new(PeerPump {
                join: tokio::sync::Mutex::new(Some(worker)),
                joined: AtomicU8::new(0),
            });
            {
                let mut state = service.state.lock();
                if !rebind {
                    state.origin = Some(origin);
                    state.pins.push(pin);
                }
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
        drop(active);
        if rebind && result.is_ok() {
            let _transition = self.transition.lock();
            if !self.clock.is_closed()
                && self
                    .active
                    .lock()
                    .as_ref()
                    .is_some_and(|a| Arc::ptr_eq(&a.admission.brand(), &prepared.attempt_brand))
            {
                self.retirement.handoff_generation();
            }
        }
        result
    }
    pub(crate) fn disconnect(&self) {
        self.disconnect_generation(None);
    }
    fn disconnect_generation(&self, identity: Option<&DelegatedAttemptIdentity>) {
        let transition = self.transition.lock();
        let socket = self.socket.lock().clone();
        if identity.is_some_and(|id| !socket.as_ref().is_some_and(|s| s.matches_identity(id))) {
            return;
        }
        self.clock.retire_current();
        let original = self
            .active
            .lock()
            .as_ref()
            .map(|a| (a.admission.original_network_attempt(), a.channel.clone()));
        drop(transition);
        // 关闭/Host 回调在转换锁外；迟到的旧 start 不能关闭新 generation。
        if let Some(socket) = socket {
            socket.request_close();
        }
        if let Some((attempt, channel)) = original {
            attempt.close();
            channel.authority().request_close();
        }
    }
    pub(crate) fn revoke(&self) {
        self.owner.request_revoke();
        self.clock.close();
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
    pub(crate) fn test_hold_renew(&self, after: bool, hook: impl FnOnce() + Send + 'static) {
        self.clock.test_hold_renew(after, hook);
    }
    pub(crate) fn test_hold_bind(&self, hook: impl FnOnce() + Send + 'static) {
        *self.bind_hook.lock() = Some(Box::new(hook));
    }
    pub(crate) fn test_hold_pump(
        &self,
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    ) {
        *self.pump_retirement.lock() = Some((entered, release));
    }
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
                retired: None,
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

// 只有本 Session 可产生，网络机制不能自行构造 admission 或替换当前时钟。
#[derive(Clone)]
pub(crate) struct DelegatedAttemptIdentity {
    session: Arc<()>,
    operation: Arc<activation::ActivationOperation>,
    attempt: Arc<()>,
}
impl DelegatedAttemptIdentity {
    #[cfg(test)]
    pub(crate) fn test_foreign_attempt(&self) -> Self {
        Self {
            session: self.session.clone(),
            operation: self.operation.clone(),
            attempt: Arc::new(()),
        }
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session, &other.session)
            && Arc::ptr_eq(&self.operation, &other.operation)
            && Arc::ptr_eq(&self.attempt, &other.attempt)
    }
}
pub(crate) struct DelegatedSocketStart {
    identity: DelegatedAttemptIdentity,
    retired: Option<cc_desk_remote_host::native_host::RetiredInstalledChannel>,
    session: Weak<DelegatedSession>,
    admission: Option<DelegatedNetworkAdmission>,
    clock: Arc<NetworkClock>,
}
impl DelegatedSocketStart {
    pub(crate) fn has_request_gate(&self) -> bool {
        self.session
            .upgrade()
            .and_then(|session| session.request_gate.get().cloned())
            .is_some_and(|gate| gate.matches_attempt(&self.identity))
    }
    pub(crate) fn matches_final_control(
        &self,
        control: &Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    ) -> bool {
        self.admission
            .as_ref()
            .is_some_and(|admission| admission.matches_final_control(control))
    }
    pub(crate) fn identity(&self) -> DelegatedAttemptIdentity {
        self.identity.clone()
    }
    pub(crate) fn terminal_clock(&self) -> Arc<NetworkClock> {
        self.clock.clone()
    }
    pub(crate) fn keepalive(&self) -> Option<Arc<DelegatedSession>> {
        self.session.upgrade()
    }
    pub(crate) fn permits_start(
        &self,
        session: &DelegatedSession,
        service: &RemoteHostService,
        state: &ServiceState,
    ) -> bool {
        let operation = session.reservation.operation();
        session
            .service
            .upgrade()
            .is_some_and(|original| std::ptr::eq(Arc::as_ptr(&original), service))
            && !state.closing
            && !state.retired
            && !session.clock.is_closed()
            && !operation.cancelled()
            && state
                .delegation
                .as_ref()
                .is_some_and(|brand| Arc::ptr_eq(brand, &session.brand))
            && state
                .activation
                .as_ref()
                .is_some_and(|original| Arc::ptr_eq(original, &operation))
    }
    pub(crate) fn budget(
        &self,
    ) -> Result<crate::remote_unattended::capability::network::NetworkBudget, SafeError> {
        let admission = self
            .admission
            .as_ref()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        self.clock
            .budget(admission.original_network_attempt())
            .map_err(|_| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))
    }
    pub(crate) fn clock(&self) -> Arc<dyn cc_desk_remote_security::GrantClock> {
        self.clock.clone()
    }
    pub(crate) fn scope(&self) -> Result<cc_desk_remote_record::grant::SessionScope, SafeError> {
        self.admission
            .as_ref()
            .map(|a| a.scope())
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))
    }
    pub(crate) fn starting_open(&self) -> bool {
        let Some(session) = self.session.upgrade() else {
            return false;
        };
        let Some(service) = session.service.upgrade() else {
            return false;
        };
        let state = service.state.lock();
        self.permits_start(&session, &service, &state)
    }
    pub(crate) fn close(&self) {
        if let Some(session) = self.session.upgrade() {
            session.disconnect_generation(Some(&self.identity));
        }
        if let Some(admission) = self.admission.as_ref() {
            admission.close();
        }
    }
    pub(crate) fn handoff(&mut self, channel: Arc<NativeChannel>) -> Result<(), SafeError> {
        let session = self
            .session
            .upgrade()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let admission = self
            .admission
            .take()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        session.adopt(DelegatedPreparedChannel {
            brand: session.brand.clone(),
            attempt_brand: admission.brand(),
            admission: Some(admission),
            channel,
            adopted: false,
            retired: self.retired.take(),
        })
    }
}
#[cfg(test)]
impl DelegatedSession {
    pub(crate) fn test_socket(
        &self,
    ) -> Option<Arc<crate::remote_host_network::delegated::SocketCustody>> {
        self.socket.lock().clone()
    }
    pub(crate) fn test_begin_socket(
        self: &Arc<Self>,
        current: &VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
        material: crate::remote_host_network::delegated::TestMaterialBuilder,
    ) -> Result<Arc<crate::remote_host_network::delegated::SocketCustody>, SafeError> {
        self.test_begin_socket_with_hook(current, supervisor, material, |_| {})
    }
    pub(crate) fn test_begin_socket_with_hook(
        self: &Arc<Self>,
        current: &VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
        material: crate::remote_host_network::delegated::TestMaterialBuilder,
        hook: impl FnOnce(&Arc<crate::remote_host_network::delegated::SocketCustody>),
    ) -> Result<Arc<crate::remote_host_network::delegated::SocketCustody>, SafeError> {
        let (custody, start) = self.test_reserve_socket_start(current, supervisor)?;
        hook(&custody);
        custody
            .start(start, material)
            .map_err(|_| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        Ok(custody)
    }
    pub(crate) fn test_reserve_socket_start(
        self: &Arc<Self>,
        current: &VerifiedBinding,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            DelegatedSocketStart,
        ),
        SafeError,
    > {
        self.reserve_socket_inner(current, Some(supervisor))
    }
}
impl DelegatedSession {
    pub(crate) fn reserve_socket_start(
        self: &Arc<Self>,
        current: &VerifiedBinding,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            DelegatedSocketStart,
        ),
        SafeError,
    > {
        self.reserve_socket_inner(current, None)
    }
    fn reserve_socket_inner(
        self: &Arc<Self>,
        current: &VerifiedBinding,
        supervisor: Option<&crate::run_supervisor::NativeRunSupervisor>,
    ) -> Result<
        (
            Arc<crate::remote_host_network::delegated::SocketCustody>,
            DelegatedSocketStart,
        ),
        SafeError,
    > {
        let mut slot = self.socket.lock();
        if slot.is_some() || self.clock.is_closed() || self.reservation.operation().cancelled() {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        let service = self
            .service
            .upgrade()
            .ok_or_else(|| crate::cli::profiles::error("REMOTE_HOST_LOCKED"))?;
        let admission = match supervisor {
            Some(supervisor) => self.owner.admit_network(current, supervisor)?,
            None => self.owner.admit_owned_network(current)?,
        };
        admission.clock(&self.clock)?;
        let identity = DelegatedAttemptIdentity {
            session: self.brand.clone(),
            operation: self.reservation.operation(),
            attempt: admission.brand(),
        };
        let start = DelegatedSocketStart {
            identity,
            retired: None,
            session: Arc::downgrade(self),
            admission: Some(admission),
            clock: self.clock.clone(),
        };
        let custody =
            crate::remote_host_network::delegated::SocketCustody::reserve(&service, &start);
        // 原材料和网络线程启动前，Session 已持有完整 custody。
        *slot = Some(custody.clone());
        Ok((custody, start))
    }
}
