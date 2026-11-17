//! App 登记前只持有原批准和资源；不执行初始化、Enable 或 capture。
use super::*;
use crate::cli::native_runtime::NativeRuntime;

#[cfg(test)]
type PermitHook = Box<dyn FnOnce(&crate::remote_host::unattended::StartPermit) + Send>;

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
        self.service.lock().epoch_end()
    }
    pub(crate) fn advance_epoch(
        &mut self,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()> {
        if self.network.is_some() {
            self.control.close();
            return Err(StoreError::Unauthorized);
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
            let gate = request_gate::ServiceRequestGate::new(
                &self.service,
                self.pairing.clone(),
                self.control.clone(),
                prepared.start.identity(),
            );
            prepared
                .session
                .install_request_gate(gate)
                .map_err(|_| StoreError::Unauthorized)?;
            #[cfg(test)]
            let material = match self.network_accepted.take() {
                Some(accepted) => prepared.material.test_loopback_observed(accepted),
                None => prepared.material,
            };
            #[cfg(not(test))]
            let material = prepared.material;
            prepared
                .socket
                .start(prepared.start, material)
                .map_err(|_| StoreError::Unauthorized)?;
        }
        Ok(())
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
