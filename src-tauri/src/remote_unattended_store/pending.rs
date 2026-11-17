//! App 登记前只持有原批准和资源；不执行初始化、Enable 或 capture。
use super::*;
use crate::cli::native_runtime::NativeRuntime;

#[cfg(test)]
type PermitHook = Box<dyn FnOnce(&crate::remote_host::unattended::StartPermit) + Send>;

#[cfg(test)]
type TestNextSocket =
    Arc<parking_lot::Mutex<Option<Arc<crate::remote_host_network::delegated::SocketCustody>>>>;

pub(crate) struct StartKeys {
    pub(crate) custody: HostKeyCustody,
    pub(crate) ready: HostKeysReady,
    pub(crate) signers: HostSignerSet,
}
struct Inputs {
    consent: FinalConsent,
    admission: NativeRunAdmission,
    keys: Option<StartKeys>,
}
pub(crate) struct PendingStart<S: Sealer, B: PairingBackend, P: PairingSealer> {
    service: Arc<parking_lot::Mutex<SingleRunService<S>>>,
    inputs: Option<Inputs>,
    pairing: Arc<PairingStore<B, P>>,
    control: Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    end: std::time::Instant,
    network: Option<std::sync::Weak<crate::remote_host::RemoteHostService>>,
    #[cfg(test)]
    permit_hook: Option<PermitHook>,
    #[cfg(test)]
    network_accepted: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(test)]
    next_network_accepted: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(test)]
    cancel_after_receipt: bool,
    #[cfg(test)]
    late_wake_observed: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(test)]
    fail_next_material: Option<TestNextSocket>,
    #[cfg(test)]
    activation_race: Option<(
        Arc<std::sync::atomic::AtomicBool>,
        Arc<std::sync::atomic::AtomicBool>,
    )>,
}
impl<S: Sealer, B: PairingBackend, P: PairingSealer> PendingStart<S, B, P> {
    pub(crate) fn new(
        backend: WindowsUnattendedBackend,
        sealer: S,
        consent: FinalConsent,
        admission: NativeRunAdmission,
        keys: StartKeys,
        pairing: Arc<PairingStore<B, P>>,
    ) -> Result<Self> {
        let control = consent.control();
        let end = consent.end();
        let service = match SingleRunService::new(backend, sealer) {
            Ok(service) => service,
            Err(error) => {
                control.close();
                keys.custody.revoke();
                drop(keys.signers);
                drop(keys.ready);
                drop(keys.custody);
                return Err(error);
            }
        };
        Ok(Self {
            service: Arc::new(parking_lot::Mutex::new(service)),
            inputs: Some(Inputs {
                consent,
                admission,
                keys: Some(keys),
            }),
            pairing,
            control,
            end,
            network: None,
            #[cfg(test)]
            permit_hook: None,
            #[cfg(test)]
            network_accepted: None,
            #[cfg(test)]
            next_network_accepted: None,
            #[cfg(test)]
            cancel_after_receipt: false,
            #[cfg(test)]
            late_wake_observed: None,
            #[cfg(test)]
            fail_next_material: None,
            #[cfg(test)]
            activation_race: None,
        })
    }
    pub(crate) fn new_epochs(
        backend: WindowsUnattendedBackend,
        sealer: S,
        consent: FinalConsent,
        admission: NativeRunAdmission,
        pairing: Arc<PairingStore<B, P>>,
    ) -> Result<Self> {
        let control = consent.control();
        let end = consent.end();
        let service = SingleRunService::new(backend, sealer)?;
        Ok(Self {
            service: Arc::new(parking_lot::Mutex::new(service)),
            inputs: Some(Inputs {
                consent,
                admission,
                keys: None,
            }),
            pairing,
            control,
            end,
            network: None,
            #[cfg(test)]
            permit_hook: None,
            #[cfg(test)]
            network_accepted: None,
            #[cfg(test)]
            next_network_accepted: None,
            #[cfg(test)]
            cancel_after_receipt: false,
            #[cfg(test)]
            late_wake_observed: None,
            #[cfg(test)]
            fail_next_material: None,
            #[cfg(test)]
            activation_race: None,
        })
    }
    pub(crate) fn new_network(
        backend: WindowsUnattendedBackend,
        sealer: S,
        consent: FinalConsent,
        admission: NativeRunAdmission,
        pairing: Arc<PairingStore<B, P>>,
        host: &Arc<crate::remote_host::RemoteHostService>,
    ) -> Result<Self> {
        if !consent.permits_network() {
            return Err(StoreError::Unauthorized);
        }
        let mut pending = Self::new_epochs(backend, sealer, consent, admission, pairing)?;
        pending.network = Some(Arc::downgrade(host));
        Ok(pending)
    }
    pub(crate) fn epoch_end(&self) -> Option<std::time::Instant> {
        let service = self.service.lock();
        if service.signing_retired {
            None
        } else {
            service.epoch_end()
        }
    }
    pub(crate) fn advance_epoch(
        &mut self,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()>
    where
        S: Send + 'static,
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
    {
        if self.network.is_some() {
            let reconnect = {
                let service = self.service.lock();
                service
                    .consent
                    .as_ref()
                    .is_some_and(|c| c.permits_reconnect())
                    && service
                        .data_socket
                        .as_ref()
                        .is_some_and(|socket| socket.terminal_observed())
            };
            if reconnect {
                return self.reconnect(permit);
            }
            if self
                .epoch_end()
                .is_none_or(|end| std::time::Instant::now() < end)
            {
                #[cfg(test)]
                if let Some(observed) = &self.late_wake_observed {
                    observed.store(true, std::sync::atomic::Ordering::Release);
                }
                return Ok(());
            }
            let keys = {
                let service = self.service.lock();
                if !permit.matches(&self.control)
                    || !service
                        .consent
                        .as_ref()
                        .is_some_and(|c| c.permits_authenticated_data())
                    || service
                        .data_socket
                        .as_ref()
                        .is_none_or(|s| s.check_installed_data().is_err())
                {
                    self.control.close();
                    return Err(StoreError::Unauthorized);
                }
                service.keys.as_ref().ok_or(StoreError::Unknown)?.clone()
            };
            keys.revoke();
            loop {
                let observation = keys.advance();
                if observation.unknown {
                    self.control.close();
                }
                if !observation.outstanding {
                    if observation.unknown || !observation.disposal_complete {
                        return Err(StoreError::Unknown);
                    }
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let mut service = self.service.lock();
            if !self.control.active()
                || !service
                    .keys
                    .as_ref()
                    .is_some_and(|original| Arc::ptr_eq(original, &keys))
                || service
                    .data_socket
                    .as_ref()
                    .is_none_or(|s| s.check_installed_data().is_err())
            {
                self.control.close();
                return Err(StoreError::Unauthorized);
            }
            service.signing_retired = true;
            return service.refresh(&self.pairing);
        }
        self.service.lock().advance_epoch(&self.pairing, permit)
    }
    pub(crate) fn matches_runtime(&self, runtime: &NativeRuntime) -> bool {
        self.inputs.as_ref().is_some_and(|i| {
            i.admission.is_owned_by(runtime) && i.consent.matches_original(&i.admission)
        })
    }
    pub(crate) fn control(
        &self,
    ) -> Arc<crate::remote_host_approval::unattended::final_approval::FinalControl> {
        self.control.clone()
    }
    pub(crate) fn end(&self) -> std::time::Instant {
        self.end
    }
    pub(crate) fn start(
        &mut self,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()>
    where
        S: Send + 'static,
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
    {
        if !permit.matches(&self.control) {
            return Err(StoreError::Unauthorized);
        }
        let network = match &self.network {
            Some(host) => {
                let host = host.upgrade().ok_or(StoreError::Unauthorized)?;
                if !permit.matches_host(&host) {
                    return Err(StoreError::Unauthorized);
                }
                Some(host)
            }
            None => None,
        };
        #[cfg(test)]
        if let Some(hook) = self.permit_hook.take() {
            hook(permit);
        }
        let i = self.inputs.take().ok_or(StoreError::Conflict)?;
        match i.keys {
            Some(keys) => self.service.lock().start(
                i.consent,
                keys.custody,
                keys.ready,
                keys.signers,
                i.admission,
                &self.pairing,
            ),
            None => self
                .service
                .lock()
                .start_epochs(i.consent, i.admission, &self.pairing, permit),
        }?;
        if let Some(host) = network {
            let prepared = self
                .service
                .lock()
                .prepare_network(&host, &self.pairing, permit)?;
            self.launch_network(prepared, permit)?;
        }
        Ok(())
    }
    fn launch_network(
        &mut self,
        mut prepared: PreparedNetwork,
        _permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()>
    where
        S: Send + 'static,
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
    {
        let gate = request_gate::ServiceRequestGate::new(
            &self.service,
            self.pairing.clone(),
            self.control.clone(),
            prepared.start.identity(),
        );
        #[cfg(test)]
        let gate = match &self.activation_race {
            Some((observed, _)) => gate.test_observe_authorized(observed.clone()),
            None => gate,
        };
        prepared
            .start
            .install_request_gate(gate)
            .map_err(|_| StoreError::Unauthorized)?;
        #[cfg(test)]
        let material = match self
            .network_accepted
            .take()
            .or_else(|| self.next_network_accepted.take())
        {
            Some(accepted) => prepared.material.test_loopback_observed(accepted),
            None => prepared.material,
        };
        #[cfg(test)]
        let material = if self.service.lock().epoch_anonymous.is_some() {
            material.test_loopback()
        } else {
            material
        };
        #[cfg(not(test))]
        let material = prepared.material;
        #[cfg(test)]
        let material = match self.activation_race.take() {
            Some((observed, witnessed)) => material.test_activation_hook(true, move || {
                let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
                while !observed.load(std::sync::atomic::Ordering::Acquire)
                    && std::time::Instant::now() < end
                {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                witnessed.store(
                    observed.load(std::sync::atomic::Ordering::Acquire),
                    std::sync::atomic::Ordering::Release,
                );
            }),
            None => material,
        };
        if self
            .service
            .lock()
            .consent
            .as_ref()
            .is_some_and(|c| c.permits_reconnect())
        {
            prepared
                .socket
                .install_terminal_wake(&prepared.start, self.control.clone())
                .map_err(|_| StoreError::Unauthorized)?;
        }
        prepared
            .socket
            .start(prepared.start, material)
            .map_err(|_| StoreError::Unauthorized)?;
        Ok(())
    }
    fn reconnect(&mut self, permit: &crate::remote_host::unattended::StartPermit) -> Result<()>
    where
        S: Send + 'static,
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
    {
        let (session, keys) = {
            let service = self.service.lock();
            if !permit.matches(&self.control)
                || !self.control.active()
                || !service
                    .consent
                    .as_ref()
                    .is_some_and(|c| c.permits_reconnect())
            {
                return Err(StoreError::Unauthorized);
            }
            let Some(CapturedOwner::Network { session, .. }) = &service.captured else {
                return Err(StoreError::Unauthorized);
            };
            (
                session.clone(),
                service.keys.as_ref().ok_or(StoreError::Unknown)?.clone(),
            )
        };
        // 原 App worker 锁外汇合；任何 Unknown 均不签发下一代。
        let retired = tauri::async_runtime::block_on(session.observe_retirement());
        if !matches!(
            retired,
            crate::remote_host::delegated::retirement::RetirementObservation::Ready
        ) {
            self.control.close();
            return Err(StoreError::Unknown);
        }
        keys.revoke();
        loop {
            let observed = keys.advance();
            if observed.unknown {
                self.control.close();
            }
            if !observed.outstanding {
                if observed.unknown || !observed.disposal_complete {
                    return Err(StoreError::Unknown);
                }
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        #[cfg(test)]
        if self.cancel_after_receipt {
            self.control.close();
            self.next_network_accepted.take();
        }
        let prepared = {
            let mut service = self.service.lock();
            if !self.control.active()
                || !service
                    .keys
                    .as_ref()
                    .is_some_and(|original| Arc::ptr_eq(original, &keys))
            {
                return Err(StoreError::Unauthorized);
            }
            service.ready.take();
            service.signers.take();
            service.check_epoch_parent(&self.pairing)?;
            let expected = service
                .snapshot
                .as_ref()
                .ok_or(StoreError::Invalid)?
                .expected();
            match service.store.observe(&expected, now_ms()?) {
                Ok(snapshot) => service.snapshot = Some(snapshot),
                Err(error) => {
                    if matches!(
                        error,
                        StoreError::CapacityRevoked | StoreError::Clock | StoreError::Expired
                    ) {
                        service.durable_closed = true;
                    }
                    if error == StoreError::Unknown {
                        service.uncertain = true;
                    }
                    self.control.close();
                    return Err(error);
                }
            }
            service.check_epoch_parent(&self.pairing)?;
            let opened = service.open_epoch(&self.pairing, permit);
            opened?;
            service.signing_retired = false;
            service.prepare_next_network(session, &self.pairing, permit)?
        };
        drop(keys);
        #[cfg(test)]
        let prepared = {
            let mut prepared = prepared;
            if let Some(observed) = &self.fail_next_material {
                *observed.lock() = Some(prepared.socket.clone());
                prepared.material = prepared.material.test_fail_spawn();
            }
            prepared
        };
        let result = self.launch_network(prepared, permit);
        #[cfg(test)]
        if result.is_ok() && self.late_wake_observed.is_some() {
            // 原旧代通知只唤醒；当前槽已选新代，不能再次消费旧 receipt。
            self.control.notify_work();
        }
        result
    }
    pub(crate) fn finish(&mut self) -> controller::ControllerEnd {
        self.control.close();
        if let Some(i) = self.inputs.take() {
            // 未进入 start：只接管并撤销原密钥，绝不借清理执行 Enable。
            let attached = match i.keys {
                Some(keys) => self.service.lock().attach_start_resources(
                    i.consent,
                    keys.custody,
                    keys.ready,
                    keys.signers,
                ),
                None => self.service.lock().attach_epoch_consent(i.consent),
            };
            if attached.is_err() {
                return controller::ControllerEnd {
                    durable: Err(StoreError::Unknown),
                    retired: false,
                };
            }
        }
        self.service.lock().finish_owned()
    }
}

impl<S: Sealer, B: PairingBackend, P: PairingSealer> Drop for PendingStart<S, B, P> {
    fn drop(&mut self) {
        self.control.close();
        if let Some(i) = self.inputs.take() {
            // 即使尚未启动或发布线程失败，原 key owner 也必须在 OS custody 之前析构。
            let _ = match i.keys {
                Some(keys) => self.service.lock().attach_start_resources(
                    i.consent,
                    keys.custody,
                    keys.ready,
                    keys.signers,
                ),
                None => self.service.lock().attach_epoch_consent(i.consent),
            };
        }
        self.service.lock().request_close();
    }
}
#[cfg(test)]
impl<S: Sealer, B: PairingBackend, P: PairingSealer> PendingStart<S, B, P> {
    // 仅探测原网络退休收据；不消费下一代、不改变旧模式授权。
    pub(crate) fn test_normal_disconnect_receipt(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
    where
        S: Send + 'static,
    {
        let service = Arc::downgrade(&self.service);
        let control = self.control.clone();
        Box::pin(async move {
            let Some(service) = service.upgrade() else {
                return false;
            };
            let session = {
                let held = service.lock();
                match &held.captured {
                    Some(CapturedOwner::Network { session, .. }) => session.clone(),
                    _ => return false,
                }
            };
            let observed = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                session.observe_retirement(),
            )
            .await;
            eprintln!(
                "TEST_ONLY_NORMAL_DISCONNECT_RECEIPT {observed:?} parent_active={}",
                control.active()
            );
            matches!(
                observed,
                Ok(crate::remote_host::delegated::retirement::RetirementObservation::Ready)
            ) && control.active()
        })
    }
    pub(crate) fn test_live_tls_key_retirement(&self) -> Box<dyn FnOnce() -> bool + Send>
    where
        S: Send + 'static,
    {
        let service = Arc::downgrade(&self.service);
        let control = self.control.clone();
        Box::new(move || {
            let Some(service) = service.upgrade() else {
                return false;
            };
            let service = service.lock();
            if !control.active() || !matches!(service.captured, Some(CapturedOwner::Network { .. }))
            {
                return false;
            }
            let Some(keys) = service.keys.as_ref() else {
                return false;
            };
            keys.revoke();
            let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let fact = keys.advance();
                if !fact.outstanding {
                    let passed = fact.disposal_complete
                        && !fact.unknown
                        && keys.observer().test_notify_joins() > 0
                        && control.active();
                    eprintln!(
                        "TEST_ONLY_LIVE_TLS_KEYS retired={} parent_active={} notify_joined={}",
                        passed,
                        control.active(),
                        keys.observer().test_notify_joins()
                    );
                    return passed;
                }
                if std::time::Instant::now() >= end {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
    }
    pub(crate) fn test_late_reconnect_wake(&mut self) -> Arc<std::sync::atomic::AtomicBool> {
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.late_wake_observed = Some(observed.clone());
        observed
    }
    pub(crate) fn test_fail_next_material(&mut self) -> TestNextSocket {
        let observed = Arc::new(parking_lot::Mutex::new(None));
        self.fail_next_material = Some(observed.clone());
        observed
    }
    pub(crate) fn test_cancel_after_receipt(&mut self) {
        self.cancel_after_receipt = true;
    }
    pub(crate) fn test_next_network_accepted(
        &mut self,
        accepted: tokio::sync::oneshot::Sender<()>,
    ) {
        self.next_network_accepted = Some(accepted);
    }
    pub(crate) fn test_anonymous_network(
        &mut self,
        scalars: [[u8; 32]; 2],
        opened: Arc<[std::sync::atomic::AtomicUsize; 2]>,
        accepted: tokio::sync::oneshot::Sender<()>,
    ) {
        self.service.lock().epoch_anonymous = Some(AnonymousKeyFixture { scalars, opened });
        self.network_accepted = Some(accepted);
    }
    pub(crate) fn test_permit_hook(
        &mut self,
        hook: impl FnOnce(&crate::remote_host::unattended::StartPermit) + Send + 'static,
    ) {
        self.permit_hook = Some(Box::new(hook));
    }
    pub(crate) fn test_epoch_trace(&mut self, trace: crate::native_crypto::EpochTestTrace) {
        self.service.lock().epoch_trace = Some(trace);
    }
    pub(crate) fn test_after_enable(&mut self, action: impl FnOnce() + Send + 'static) {
        self.service.lock().test_after_enable(action);
    }
}

#[cfg(test)]
impl<S: Sealer, B: PairingBackend, P: PairingSealer> PendingStart<S, B, P> {
    pub(crate) fn test_activation_race(&mut self) -> Arc<std::sync::atomic::AtomicBool> {
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let witnessed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.activation_race = Some((observed, witnessed.clone()));
        witnessed
    }
    pub(crate) fn test_close_after_signing(&self) -> Box<dyn FnOnce() -> bool + Send>
    where
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
        S: Send + 'static,
    {
        let observed = self.test_observe_signing_retired();
        let control = self.control.clone();
        Box::new(move || {
            let retired = observed();
            control.close();
            retired && control.closed()
        })
    }
    pub(crate) fn test_observe_signing_retired(&self) -> Box<dyn FnOnce() -> bool + Send>
    where
        B: Send + Sync + 'static,
        P: Send + Sync + 'static,
        S: Send + 'static,
    {
        let service = Arc::downgrade(&self.service);
        Box::new(move || {
            service.upgrade().is_some_and(|service| {
                let service = service.lock();
                service.signing_retired
                    && service.keys.as_ref().is_some_and(|keys| {
                        let o = keys.snapshot();
                        !o.outstanding
                            && o.disposal_complete
                            && !o.unknown
                            && keys.observer().test_notify_joins() > 0
                    })
            })
        })
    }
}
