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
    service: SingleRunService<S>,
    inputs: Option<Inputs>,
    pairing: Arc<PairingStore<B, P>>,
    control: Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    end: std::time::Instant,
    #[cfg(test)]
    permit_hook: Option<PermitHook>,
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
            service,
            inputs: Some(Inputs {
                consent,
                admission,
                keys: Some(keys),
            }),
            pairing,
            control,
            end,
            #[cfg(test)]
            permit_hook: None,
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
            service,
            inputs: Some(Inputs {
                consent,
                admission,
                keys: None,
            }),
            pairing,
            control,
            end,
            #[cfg(test)]
            permit_hook: None,
        })
    }
    pub(crate) fn epoch_end(&self) -> Option<std::time::Instant> {
        self.service.epoch_end()
    }
    pub(crate) fn advance_epoch(
        &mut self,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()> {
        self.service.advance_epoch(&self.pairing, permit)
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
    ) -> Result<()> {
        if !permit.matches(&self.control) {
            return Err(StoreError::Unauthorized);
        }
        #[cfg(test)]
        if let Some(hook) = self.permit_hook.take() {
            hook(permit);
        }
        let i = self.inputs.take().ok_or(StoreError::Conflict)?;
        match i.keys {
            Some(keys) => self.service.start(
                i.consent,
                keys.custody,
                keys.ready,
                keys.signers,
                i.admission,
                &self.pairing,
            ),
            None => self
                .service
                .start_epochs(i.consent, i.admission, &self.pairing, permit),
        }
    }
    pub(crate) fn finish(&mut self) -> controller::ControllerEnd {
        self.control.close();
        if let Some(i) = self.inputs.take() {
            // 未进入 start：只接管并撤销原密钥，绝不借清理执行 Enable。
            let attached = match i.keys {
                Some(keys) => self.service.attach_start_resources(
                    i.consent,
                    keys.custody,
                    keys.ready,
                    keys.signers,
                ),
                None => self.service.attach_epoch_consent(i.consent),
            };
            if attached.is_err() {
                return controller::ControllerEnd {
                    durable: Err(StoreError::Unknown),
                    retired: false,
                };
            }
        }
        self.service.finish_owned()
    }
}

impl<S: Sealer, B: PairingBackend, P: PairingSealer> Drop for PendingStart<S, B, P> {
    fn drop(&mut self) {
        self.control.close();
        if let Some(i) = self.inputs.take() {
            // 即使尚未启动或发布线程失败，原 key owner 也必须在 OS custody 之前析构。
            let _ = match i.keys {
                Some(keys) => self.service.attach_start_resources(
                    i.consent,
                    keys.custody,
                    keys.ready,
                    keys.signers,
                ),
                None => self.service.attach_epoch_consent(i.consent),
            };
        }
        self.service.request_close();
    }
}
#[cfg(test)]
impl<S: Sealer, B: PairingBackend, P: PairingSealer> PendingStart<S, B, P> {
    pub(crate) fn test_permit_hook(
        &mut self,
        hook: impl FnOnce(&crate::remote_host::unattended::StartPermit) + Send + 'static,
    ) {
        self.permit_hook = Some(Box::new(hook));
    }
    pub(crate) fn test_epoch_trace(&mut self, trace: crate::native_crypto::EpochTestTrace) {
        self.service.epoch_trace = Some(trace);
    }
    pub(crate) fn test_after_enable(&mut self, action: impl FnOnce() + Send + 'static) {
        self.service.test_after_enable(action);
    }
}
