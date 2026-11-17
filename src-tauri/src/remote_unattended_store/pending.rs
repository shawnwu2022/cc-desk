//! App 登记前只持有原批准和资源；不执行初始化、Enable 或 capture。
use super::*;
use crate::cli::native_runtime::NativeRuntime;

pub(crate) struct StartKeys {
    pub(crate) custody: HostKeyCustody,
    pub(crate) ready: HostKeysReady,
    pub(crate) signers: HostSignerSet,
}
struct Inputs {
    consent: FinalConsent,
    admission: NativeRunAdmission,
    keys: StartKeys,
}
pub(crate) struct PendingStart<S: Sealer, B: PairingBackend, P: PairingSealer> {
    service: SingleRunService<S>,
    inputs: Option<Inputs>,
    pairing: Arc<PairingStore<B, P>>,
    control: Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    end: std::time::Instant,
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
                keys,
            }),
            pairing,
            control,
            end,
        })
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
        let i = self.inputs.take().ok_or(StoreError::Conflict)?;
        self.service.start(
            i.consent,
            i.keys.custody,
            i.keys.ready,
            i.keys.signers,
            i.admission,
            &self.pairing,
        )
    }
    pub(crate) fn finish(&mut self) -> controller::ControllerEnd {
        self.control.close();
        if let Some(i) = self.inputs.take() {
            // 未进入 start：只接管并撤销原密钥，绝不借清理执行 Enable。
            if self
                .service
                .attach_start_resources(i.consent, i.keys.custody, i.keys.ready, i.keys.signers)
                .is_err()
            {
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
            let _ = self.service.attach_start_resources(
                i.consent,
                i.keys.custody,
                i.keys.ready,
                i.keys.signers,
            );
        }
        self.service.request_close();
    }
}
#[cfg(test)]
impl<S: Sealer, B: PairingBackend, P: PairingSealer> PendingStart<S, B, P> {
    pub(crate) fn test_after_enable(&mut self, action: impl FnOnce() + Send + 'static) {
        self.service.test_after_enable(action);
    }
}
