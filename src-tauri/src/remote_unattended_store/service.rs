//! 内部单 run 提交协调器；无 UI、IPC、网络或恢复入口。
use super::*;
use crate::{
    native_crypto::host::{CustodyObservation, HostKeyCustody, HostKeysReady, HostSignerSet},
    remote_host_approval::unattended::final_approval::FinalConsent,
    remote_native_pairing_store::{
        Backend as PairingBackend, PairingStore, Sealer as PairingSealer,
    },
    remote_unattended::capability::{CloseObservation, Delegation, VerifiedBinding},
    run_supervisor::{delegated::DelegatedRunOwner, NativeRunAdmission},
    version_history::windows::unattended_store::{OwnerCustody, WindowsUnattendedBackend},
};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::Clock)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::Clock)
}

pub(crate) struct SingleRunService<S: Sealer> {
    store: Store<WindowsUnattendedBackend, S>,
    consent: Option<FinalConsent>,
    keys: Option<HostKeyCustody>,
    signers: Option<HostSignerSet>,
    ready: Option<HostKeysReady>,
    captured: Option<DelegatedRunOwner>,
    policy: Option<Arc<Delegation>>,
    binding: Option<VerifiedBinding>,
    snapshot: Option<LockedSnapshot>,
    attempted: bool,
    durable_closed: bool,
    ticket: Option<crate::run_supervisor::delegated::DelegatedRunAdmission>,
    #[cfg(test)]
    after_enable: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    before_publish: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    before_capture: Option<Box<dyn FnOnce() + Send>>,
    uncertain: bool,
    // 最后释放：原key/通知线程与captured/ticket字段的析构先完成。
    custody: Option<OwnerCustody>,
    process_slot: Option<ProcessSlot>,
}
impl<S: Sealer> SingleRunService<S> {
    pub(crate) fn new(backend: WindowsUnattendedBackend, sealer: S) -> Result<Self> {
        let process_slot = ProcessSlot::claim()?;
        let custody = backend.claim_owner_custody()?;
        Ok(Self {
            store: Store::new(backend, sealer)?,
            custody: Some(custody),
            process_slot: Some(process_slot),
            consent: None,
            keys: None,
            signers: None,
            ready: None,
            captured: None,
            policy: None,
            binding: None,
            snapshot: None,
            attempted: false,
            durable_closed: false,
            ticket: None,
            #[cfg(test)]
            after_enable: None,
            #[cfg(test)]
            before_publish: None,
            #[cfg(test)]
            before_capture: None,
            uncertain: false,
        })
    }
    fn permit(&self, expected: Expected, issued_ms: u64, cutoff_ms: u64) -> Result<NativePermit> {
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        Ok(NativePermit {
            final_control: Some(consent.control()),
            mode: Some(ExecutionMode::SingleRunNoRestore),
            instance: self.store.instance.clone(),
            expected,
            issued_ms,
            cutoff_ms,
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: consent.end(),
        })
    }
    fn check_start<B: PairingBackend, P: PairingSealer>(
        &self,
        pairing: &PairingStore<B, P>,
        admission: &NativeRunAdmission,
    ) -> Result<()> {
        self.custody
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check(&self.store.backend)?;
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        consent
            .check_start(pairing, admission)
            .map_err(|_| StoreError::Unauthorized)?;
        self.ready
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check(
                self.keys.as_ref().ok_or(StoreError::Unauthorized)?,
                consent.key_binding(),
            )
            .map_err(|_| StoreError::Unauthorized)
    }
    // 先接管原 key/签名资源再做任何可能失败的磁盘工作；错误后仍由此对象收口。
    pub(crate) fn start<B: PairingBackend, P: PairingSealer>(
        &mut self,
        consent: FinalConsent,
        keys: HostKeyCustody,
        ready: HostKeysReady,
        signers: HostSignerSet,
        admission: NativeRunAdmission,
        pairing: &PairingStore<B, P>,
    ) -> Result<()> {
        if self.attempted {
            return Err(StoreError::Conflict);
        }
        self.attempted = true;
        self.consent = Some(consent);
        self.keys = Some(keys);
        self.ready = Some(ready);
        self.signers = Some(signers);
        let result = self.start_inner(admission, pairing);
        if let Err(error) = result {
            if error == StoreError::Unknown {
                self.uncertain = true;
            }
            self.request_close();
            let compensation = self.persist_close();
            return Err(compensation.err().unwrap_or(error));
        }
        Ok(())
    }
    fn start_inner<B: PairingBackend, P: PairingSealer>(
        &mut self,
        admission: NativeRunAdmission,
        pairing: &PairingStore<B, P>,
    ) -> Result<()> {
        self.check_start(pairing, &admission)?;
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        let binding = VerifiedBinding::single_run(consent).map_err(|_| StoreError::Invalid)?;
        let digest = binding
            .single_run_target()
            .map_err(|_| StoreError::Invalid)?;
        let target = Target {
            installation: digest.installation,
            device: digest.device,
            target: digest.run,
            binding_fingerprint: digest.binding,
        };
        let initial = Expected {
            store_id: *uuid::Uuid::new_v4().as_bytes(),
            revision: 0,
            target,
        };
        let now = now_ms()?;
        let disabled = self.store.initialize(self.permit(initial, now, now)?)?;
        self.snapshot = Some(disabled);
        self.check_start(pairing, &admission)?;
        let expected = self
            .snapshot
            .as_ref()
            .ok_or(StoreError::Invalid)?
            .expected();
        let now = now_ms()?;
        let cutoff = self
            .consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .cutoff();
        let enabled = self.store.enable(self.permit(expected, now, cutoff)?, now);
        let enabled = match enabled {
            Ok(value) => value,
            Err(error) => {
                if error == StoreError::Unknown {
                    self.uncertain = true;
                }
                return Err(error);
            }
        };
        self.snapshot = Some(enabled);
        #[cfg(test)]
        if let Some(action) = self.after_enable.take() {
            action();
        }
        self.check_start(pairing, &admission)?;
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        let revision = self
            .snapshot
            .as_ref()
            .ok_or(StoreError::Invalid)?
            .0
            .authority_revision;
        let policy = Delegation::single_run(consent, &binding, revision)
            .map_err(|_| StoreError::Unauthorized)?;
        self.policy = Some(policy.clone());
        #[cfg(test)]
        if let Some(action) = self.before_capture.take() {
            action();
        }
        let captured = DelegatedRunOwner::capture_single_run(admission, consent, policy)
            .map_err(|_| StoreError::Unauthorized)?;
        self.captured = Some(captured);
        self.binding = Some(binding);
        self.check_durable_current()?;
        self.check_before_publication(pairing)?;
        #[cfg(test)]
        if let Some(action) = self.before_publish.take() {
            action();
        }
        self.consent
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .publish()
            .map_err(|_| StoreError::Unauthorized)?;
        Ok(())
    }
    // 原事务锁内精确核对已提交版本；不把历史 Enabled 当成持续许可。
    fn check_durable_current(&self) -> Result<()> {
        let expected = self.snapshot.as_ref().ok_or(StoreError::Invalid)?;
        let mut local = self.store.lock()?;
        if local.blocked {
            return Err(StoreError::Poisoned);
        }
        let mut tx = self.store.backend.begin()?;
        let current = self.store.read(&mut tx, &mut local)?;
        Store::<WindowsUnattendedBackend, S>::cas(&current, &expected.expected())?;
        if current != expected.0 || current.status != Status::Enabled {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    fn check_before_publication<B: PairingBackend, P: PairingSealer>(
        &self,
        pairing: &PairingStore<B, P>,
    ) -> Result<()> {
        self.custody
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check(&self.store.backend)?;
        let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
        consent
            .check_before_publication(pairing)
            .map_err(|_| StoreError::Unauthorized)?;
        self.ready
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check(
                self.keys.as_ref().ok_or(StoreError::Unauthorized)?,
                consent.key_binding(),
            )
            .map_err(|_| StoreError::Unauthorized)?;
        self.captured
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .check_captured()
            .map_err(|_| StoreError::Unauthorized)?;
        consent
            .check_before_publication(pairing)
            .map_err(|_| StoreError::Unauthorized)
    }
    // 只报告一次发布状态；effect 每次仍重新核设备、key、run、截止和持久版本。
    pub(crate) fn is_published(&self) -> bool {
        !self.uncertain && self.consent.as_ref().is_some_and(|c| c.control().active())
    }
    pub(crate) fn request_close(&self) {
        if let Some(consent) = &self.consent {
            consent.control().close();
        }
        if let Some(policy) = &self.policy {
            policy.request_revoke();
        }
        if let Some(captured) = &self.captured {
            captured.request_revoke();
        }
        if let Some(keys) = &self.keys {
            keys.revoke();
        }
    }
    fn persist_close(&mut self) -> Result<()> {
        if self.uncertain {
            return Err(StoreError::Unknown);
        }
        if self.durable_closed {
            return Ok(());
        }
        let Some(snapshot) = &self.snapshot else {
            return Ok(());
        };
        if snapshot.0.status != Status::Enabled {
            return Ok(());
        }
        let expected = snapshot.expected();
        let now = now_ms().unwrap_or(snapshot.0.observed_wall_ms);
        match self.store.revoke(&expected, now) {
            Ok(closed) => {
                self.snapshot = Some(closed);
                self.durable_closed = true;
                Ok(())
            }
            Err(error) => {
                self.uncertain = true;
                Err(error)
            }
        }
    }
    pub(crate) fn stop(&mut self) -> Result<()> {
        self.request_close();
        self.persist_close()
    }
    pub(crate) fn advance_retirement(&mut self) -> Option<(CustodyObservation, CloseObservation)> {
        self.ticket.take();
        self.signers.take();
        self.keys.as_ref().map(|keys| {
            let effects = self
                .policy
                .as_ref()
                .map_or(CloseObservation::EffectsClosed, |p| p.observe_close());
            (keys.advance(), effects)
        })
    }
}
impl<S: Sealer> Drop for SingleRunService<S> {
    fn drop(&mut self) {
        // Drop 不冒称 durable 撤销；历史 Enabled 重开仍无恢复转换。
        // 原 HostKeyCustody 自己保留/回收原通知与 key 线程，不转移 JoinHandle。
        self.request_close();
        self.signers.take();
        self.ticket.take();
        let effects = self
            .policy
            .as_ref()
            .map_or(CloseObservation::EffectsClosed, |p| p.observe_close());
        if effects != CloseObservation::EffectsClosed {
            if let Some(keys) = &self.keys {
                let _ = keys.advance();
            }
            let retained = Quarantine {
                _custody: self.custody.take().expect("service owns custody"),
                _slot: self.process_slot.take().expect("service owns process slot"),
                keys: self.keys.take(),
                _captured: self.captured.take(),
                _policy: self.policy.take(),
                _snapshot: self.snapshot.take(),
                _reason: effects,
            };
            // 唯一槽在 new 时预留；已有隔离后不可能再构造另一个 service。
            *QUARANTINE.lock() = Some(retained);
        }
    }
}

impl<S: Sealer> SingleRunService<S> {
    fn refresh<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
    ) -> Result<()> {
        let outcome = (|| {
            if !self.is_published() {
                return Err(StoreError::Unauthorized);
            }
            self.custody
                .as_ref()
                .ok_or(StoreError::Unauthorized)?
                .check(&self.store.backend)?;
            let consent = self.consent.as_ref().ok_or(StoreError::Unauthorized)?;
            consent
                .check_runtime(pairing)
                .map_err(|_| StoreError::Unauthorized)?;
            self.ready
                .as_ref()
                .ok_or(StoreError::Unauthorized)?
                .check(
                    self.keys.as_ref().ok_or(StoreError::Unauthorized)?,
                    consent.key_binding(),
                )
                .map_err(|_| StoreError::Unauthorized)?;
            self.captured
                .as_ref()
                .ok_or(StoreError::Unauthorized)?
                .check_captured()
                .map_err(|_| StoreError::Unauthorized)?;
            let expected = self
                .snapshot
                .as_ref()
                .ok_or(StoreError::Invalid)?
                .expected();
            let current = self.store.observe(&expected, now_ms()?);
            match current {
                Ok(snapshot) => self.snapshot = Some(snapshot),
                Err(error) => {
                    if matches!(
                        error,
                        StoreError::CapacityRevoked | StoreError::Clock | StoreError::Expired
                    ) {
                        self.durable_closed = true;
                    }
                    if error == StoreError::Unknown {
                        self.uncertain = true;
                    }
                    return Err(error);
                }
            }
            consent
                .check_runtime(pairing)
                .map_err(|_| StoreError::Unauthorized)
        })();
        if let Err(error) = outcome {
            self.request_close();
            return Err(self.persist_close().err().unwrap_or(error));
        }
        Ok(())
    }
    fn admit<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
    ) -> Result<()> {
        self.refresh(pairing)?;
        if self.ticket.is_none() {
            self.ticket = Some(
                self.captured
                    .as_ref()
                    .ok_or(StoreError::Unauthorized)?
                    .admit(
                        self.binding.as_ref().ok_or(StoreError::Unauthorized)?,
                        supervisor,
                    )
                    .map_err(|_| StoreError::Unauthorized)?,
            );
        }
        Ok(())
    }
    pub(crate) fn resize<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
        rows: u16,
        columns: u16,
    ) -> Result<()> {
        self.admit(pairing, supervisor)?;
        self.ticket
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .resize(
                self.binding.as_ref().ok_or(StoreError::Unauthorized)?,
                rows,
                columns,
            )
            .map_err(|_| StoreError::Unauthorized)
    }
    pub(crate) fn write_frame<B: PairingBackend, P: PairingSealer>(
        &mut self,
        pairing: &PairingStore<B, P>,
        supervisor: &crate::run_supervisor::NativeRunSupervisor,
        bytes: &[u8],
    ) -> Result<cc_desk_remote_host::native_host::NativeFrameResult> {
        self.admit(pairing, supervisor)?;
        self.ticket
            .as_ref()
            .ok_or(StoreError::Unauthorized)?
            .write_frame(
                self.binding.as_ref().ok_or(StoreError::Unauthorized)?,
                bytes,
            )
            .map_err(|_| StoreError::Unauthorized)
    }
    #[cfg(test)]
    pub(crate) fn test_after_enable(&mut self, action: impl FnOnce() + Send + 'static) {
        self.after_enable = Some(Box::new(action));
    }
    #[cfg(test)]
    pub(crate) fn test_before_capture(&mut self, action: impl FnOnce() + Send + 'static) {
        self.before_capture = Some(Box::new(action));
    }
    #[cfg(test)]
    pub(crate) fn test_before_publish(&mut self, action: impl FnOnce() + Send + 'static) {
        self.before_publish = Some(Box::new(action));
    }
    #[cfg(test)]
    pub(crate) fn test_durable_revoked(&self) -> bool {
        self.durable_closed
            || self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.0.status == Status::Revoked)
    }
    #[cfg(test)]
    pub(crate) fn test_snapshot_enabled(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|s| s.0.status == Status::Enabled)
    }
}

// 固定一个进程槽：先预留才能启动，隔离占用后所有新 service 都被拒绝。
static PROCESS_SLOT: AtomicBool = AtomicBool::new(false);
struct ProcessSlot;
impl ProcessSlot {
    fn claim() -> Result<Self> {
        PROCESS_SLOT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| StoreError::Busy)
    }
}
impl Drop for ProcessSlot {
    fn drop(&mut self) {
        PROCESS_SLOT.store(false, Ordering::Release);
    }
}
struct Quarantine {
    _custody: OwnerCustody,
    _slot: ProcessSlot,
    keys: Option<HostKeyCustody>,
    _captured: Option<DelegatedRunOwner>,
    _policy: Option<Arc<Delegation>>,
    _snapshot: Option<LockedSnapshot>,
    _reason: CloseObservation,
}
static QUARANTINE: parking_lot::Mutex<Option<Quarantine>> = parking_lot::Mutex::new(None);

// 仅推进原句柄已完成的 join；不释放隔离槽、不把 Unknown 提升成成功。
pub(crate) fn advance_quarantine() -> Option<CustodyObservation> {
    let mut slot = QUARANTINE.lock();
    let q = slot.as_mut()?;
    // 原 writer 已 join 且委托调用静止后才放资源引用；Unknown 与独占隔离仍保留。
    if q._captured
        .as_ref()
        .is_some_and(DelegatedRunOwner::closed_capture_releasable)
    {
        q._captured.take();
    }
    q.keys.as_ref().map(HostKeyCustody::advance)
}
#[cfg(test)]
impl<S: Sealer> SingleRunService<S> {
    pub(crate) fn test_partial_result(&self) {
        self.policy
            .as_ref()
            .unwrap()
            .test_single_run_partial_result();
    }
    pub(crate) fn test_loaded_enabled_is_locked(&self) -> bool {
        let mut local = self.store.lock().unwrap();
        let mut tx = self.store.backend.begin().unwrap();
        let locked = self.store.read(&mut tx, &mut local).unwrap();
        locked.mode == Some(ExecutionMode::SingleRunNoRestore)
            && locked.status == Status::Enabled
            && !self.is_published()
    }
}

#[cfg(test)]
pub(crate) fn test_quarantine_state() -> Option<(bool, CloseObservation)> {
    QUARANTINE.lock().as_ref().map(|q| {
        (
            q._captured.is_some(),
            q._policy.as_ref().unwrap().observe_close(),
        )
    })
}

#[cfg(test)]
pub(crate) fn test_external_revoke<S: Sealer>(backend: WindowsUnattendedBackend, sealer: S) -> u64 {
    let store = Store::new(backend, sealer).unwrap();
    let (expected, now) = {
        let mut local = store.lock().unwrap();
        let mut tx = store.backend.begin().unwrap();
        let current = store.read(&mut tx, &mut local).unwrap();
        (
            LockedSnapshot(current.clone()).expected(),
            current.observed_wall_ms,
        )
    };
    store.revoke(&expected, now).unwrap().0.revision
}
#[cfg(test)]
pub(crate) fn test_file_revoked_at<S: Sealer>(
    backend: WindowsUnattendedBackend,
    sealer: S,
    revision: u64,
) -> bool {
    let store = Store::new(backend, sealer).unwrap();
    let mut local = store.lock().unwrap();
    let mut tx = store.backend.begin().unwrap();
    let current = store.read(&mut tx, &mut local).unwrap();
    current.status == Status::Revoked && current.revision == revision
}

#[cfg(test)]
pub(crate) fn test_file_disabled_at<S: Sealer>(
    backend: WindowsUnattendedBackend,
    sealer: S,
    revision: u64,
) -> bool {
    let store = Store::new(backend, sealer).unwrap();
    let mut local = store.lock().unwrap();
    let mut tx = store.backend.begin().unwrap();
    let current = store.read(&mut tx, &mut local).unwrap();
    current.status == Status::Disabled && current.revision == revision
}

#[path = "controller.rs"]
pub(crate) mod controller;
