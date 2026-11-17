//! 同一服务的短 key 代际；父授权、捕获 run 和持久版本不重建。
use super::*;
use crate::native_crypto::host::{FrozenHostBinding, FrozenHostSupplier, NotifyHooks};
use std::time::{Duration, Instant};

pub(super) struct KeyEpoch {
    source: crate::remote_host_approval::unattended::final_approval::EpochSource,
    binding: Option<FrozenHostBinding>,
    generation: u64,
}
impl<S: Sealer> SingleRunService<S> {
    pub(super) fn current_key_binding(&self) -> Result<&FrozenHostBinding> {
        if let Some(epoch) = &self.epoch {
            return epoch.binding.as_ref().ok_or(StoreError::Unauthorized);
        }
        Ok(self
            .consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .key_binding())
    }
    pub(super) fn attach_epoch_consent(&mut self, consent: FinalConsent) -> Result<()> {
        if self.attempted {
            return Err(StoreError::Conflict);
        }
        self.attempted = true;
        self.consent = Some(consent);
        Ok(())
    }
    pub(super) fn start_epochs<B: PairingBackend, P: PairingSealer>(
        &mut self,
        consent: FinalConsent,
        admission: NativeRunAdmission,
        pairing: &PairingStore<B, P>,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()> {
        self.attach_epoch_consent(consent)?;
        let result = (|| {
            let source = self
                .consent
                .as_mut()
                .unwrap()
                .claim_epochs(permit)
                .map_err(|_| StoreError::Unauthorized)?;
            self.epoch = Some(KeyEpoch {
                source,
                binding: None,
                generation: 0,
            });
            self.consent
                .as_ref()
                .unwrap()
                .check_start(pairing, &admission)
                .map_err(|_| StoreError::Unauthorized)?;
            self.open_epoch(pairing, permit)?;
            self.start_inner(admission, pairing)
        })();
        if let Err(error) = result {
            self.request_close();
            return Err(self.persist_close().err().unwrap_or(error));
        }
        Ok(())
    }
    fn check_epoch_parent<B: PairingBackend, P: PairingSealer>(
        &self,
        pairing: &PairingStore<B, P>,
    ) -> Result<()> {
        self.custody
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check(&self.store.backend)?;
        self.consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check_runtime(pairing)
            .map_err(|_| StoreError::Unauthorized)?;
        if let Some(captured) = &self.captured {
            captured
                .check_captured()
                .map_err(|_| StoreError::Unauthorized)?;
        }
        if self.snapshot.is_some() {
            self.check_durable_current()?;
        }
        self.consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check_runtime(pairing)
            .map_err(|_| StoreError::Unauthorized)
    }
    fn open_epoch<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()> {
        self.check_epoch_parent(pairing)?;
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        let source = &self.epoch.as_ref().ok_or(StoreError::Unauthorized)?.source;
        let token = consent
            .epoch_token(source, permit)
            .map_err(|_| StoreError::Unauthorized)?;
        let supplier: FrozenHostSupplier = consent
            .epoch_supplier(source, permit, pairing)
            .map_err(|_| StoreError::Unauthorized)?;
        #[cfg(test)]
        let supplier = if let Some(fixture) = &self.epoch_anonymous {
            let names = supplier.key_references().ok_or(StoreError::Invalid)?;
            crate::native_crypto::host_profile::NativeHostProfile::new(
                supplier.binding().clone(),
                &names[0],
                &names[1],
            )
            .map_err(|_| StoreError::Invalid)?
            .test_anonymous(fixture.scalars, fixture.opened.clone())
            .freeze()
        } else {
            supplier
        };
        let binding = supplier.binding().clone();
        let generation = self
            .epoch
            .as_ref()
            .map_or(Some(1), |e| e.generation.checked_add(1))
            .ok_or(StoreError::Invalid)?;
        #[cfg(test)]
        let supplier = if let Some(trace) = &self.epoch_trace {
            crate::native_crypto::epoch_test_supplier(supplier, token.clone(), generation, trace)
        } else {
            supplier
        };
        // 先由同一服务保留原 custody；任何 prepare 失败/恐慌仍沿原 owner 退休。
        self.keys = Some(Arc::new(HostKeyCustody::new(token, binding.profile_cap)));
        let epoch = self.epoch.as_mut().unwrap();
        epoch.binding = Some(binding);
        epoch.generation = generation;
        let keys = self.keys.as_ref().unwrap();
        #[cfg(test)]
        if let Some(trace) = &self.epoch_trace {
            *trace.observer.lock() = Some(keys.observer());
        }
        keys.notification_builder()
            .map_err(|_| StoreError::Unknown)?
            .start(NotifyHooks::default())
            .map_err(|_| StoreError::Unknown)?;
        self.signers = Some(
            keys.publisher()
                .map_err(|_| StoreError::Unknown)?
                .prepare(supplier)
                .map_err(|_| StoreError::Unknown)?,
        );
        loop {
            self.check_epoch_parent(pairing)?;
            match self.keys.as_ref().unwrap().take_ready() {
                Ok(Some(ready)) => {
                    self.ready = Some(ready);
                    #[cfg(test)]
                    if let Some(trace) = &self.epoch_trace {
                        trace.events.lock().push((generation, "ready"));
                    }
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => return Err(StoreError::Unauthorized),
            }
        }
        self.check_epoch_parent(pairing)?;
        self.ready
            .as_ref()
            .unwrap()
            .check(self.keys.as_ref().unwrap(), self.current_key_binding()?)
            .map_err(|_| StoreError::Unauthorized)
    }
    pub(super) fn epoch_end(&self) -> Option<Instant> {
        self.epoch
            .as_ref()
            .and_then(|e| e.binding.as_ref().map(|b| b.profile_cap))
    }
    pub(super) fn advance_epoch<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<()> {
        let Some(end) = self.epoch_end() else {
            return Ok(());
        };
        self.consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check_epoch_source(&self.epoch.as_ref().unwrap().source, permit)
            .map_err(|_| StoreError::Unauthorized)?;
        if Instant::now() < end {
            return Ok(());
        }
        self.check_epoch_parent(pairing)?;
        self.ready.take();
        self.signers.take();
        let keys = self.keys.as_ref().ok_or(StoreError::Unknown)?;
        keys.revoke();
        loop {
            let observation = keys.advance();
            if observation.unknown {
                // 未知原 key 不能被替代；先撤父资格，原 Join 仍由本 worker 保留。
                self.request_close();
            }
            if !observation.outstanding {
                if observation.unknown || !observation.disposal_complete {
                    return Err(StoreError::Unknown);
                }
                break;
            }
            // 取消不等待这里的锁；本线程仍负责收齐原 Join，不以超时替代退休。
            std::thread::sleep(Duration::from_millis(2));
        }
        #[cfg(test)]
        if let Some(trace) = &self.epoch_trace {
            assert!(keys.observer().test_notify_joins() > 0);
            trace
                .events
                .lock()
                .push((self.epoch.as_ref().unwrap().generation, "original_joined"));
        }
        #[cfg(test)]
        if let Some(trace) = &self.epoch_trace {
            let action = trace.after_join.lock().take();
            if let Some(action) = action {
                action();
            }
        }
        // 保留已知退休的旧 custody 到新代接管，取消尾检失败仍有原退休证据。
        self.check_epoch_parent(pairing)?;
        self.open_epoch(pairing, permit)
    }
}
