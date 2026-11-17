//! 独立最终批准；A3 的准备回执不等于本模块的持久 Enable 同意。
use super::*;
use crate::native_crypto::owner::NativeRevocation;
use crate::native_crypto::{
    host::{FrozenHostBinding, FrozenHostSupplier, HostKeyProfileIdentity},
    host_profile::NativeHostProfile,
};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Publication {
    Waiting,
    Starting,
    Active,
    Closed,
}

// 取消只推进不可复位的位；短门不持有存储、run 或密钥锁。
pub(crate) struct FinalControl {
    closed: Arc<NativeRevocation>,
    publication: Mutex<Publication>,
    closed_notify: parking_lot::Condvar,
    work: std::sync::atomic::AtomicBool,
}
impl FinalControl {
    pub(crate) fn close(&self) {
        self.closed.revoke();
        *self.publication.lock() = Publication::Closed;
        self.closed_notify.notify_all();
    }
    // 同一短门下检查条件并等待；不跨等待持有存储、run 或 key 锁。
    pub(crate) fn wait_until_closed(&self, end: Instant) {
        let mut publication = self.publication.lock();
        while !self.closed() && Instant::now() < end && !self.work.swap(false, Ordering::AcqRel) {
            self.closed_notify.wait_until(&mut publication, end);
        }
    }
    // 只唤醒原托管 worker，不改变权限或截止；同短门避免丢通知。
    pub(crate) fn notify_work(&self) {
        let _publication = self.publication.lock();
        self.work.store(true, Ordering::Release);
        self.closed_notify.notify_all();
    }
    pub(crate) fn closed(&self) -> bool {
        self.closed.is_revoked()
    }
    pub(crate) fn active(&self) -> bool {
        !self.closed() && *self.publication.lock() == Publication::Active
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalMode {
    Local,
    ShortNetwork,
    NetworkSession,
    ReconnectSession,
}
struct FinalDescription {
    mode: FinalMode,
    preparation: UnattendedPreparation,
    key_binding: FrozenHostBinding,
    supplier: Option<FrozenHostSupplier>,
}
pub(crate) struct FinalApproval {
    machine: Arc<ApprovalMachine<FinalDescription>>,
    control: Arc<FinalControl>,
}
pub(crate) struct EpochSource {
    control: Arc<FinalControl>,
    original: FrozenHostSupplier,
}
pub(crate) struct FinalConsent {
    description: FinalDescription,
    machine: Arc<ApprovalMachine<FinalDescription>>,
    control: Arc<FinalControl>,
    end: Instant,
}
impl FinalDescription {
    fn new<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        mode: FinalMode,
    ) -> Result<Self, SafeError> {
        approved.check(store, admission)?;
        if mode != FinalMode::Local && !matches!(approved.preparation.role, RequestedRole::Control)
        {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        let mut preparation = approved.preparation;
        // 固定短材料窗口，不把等待确认的时间重新加到 cap 上。
        let anchor = Instant::now();
        let seconds = match mode {
            FinalMode::Local | FinalMode::NetworkSession | FinalMode::ReconnectSession => 60,
            FinalMode::ShortNetwork => 10,
        };
        let cap = preparation.end.min(anchor + Duration::from_secs(seconds));
        let wall = wall_ms()?;
        let remaining: u64 = cap
            .saturating_duration_since(Instant::now())
            .as_millis()
            .try_into()
            .map_err(|_| error("REMOTE_UNATTENDED_DEADLINE"))?;
        let cutoff = preparation.cutoff.min(
            wall.checked_add(remaining)
                .ok_or_else(|| error("REMOTE_UNATTENDED_DEADLINE"))?,
        );
        if remaining == 0 || cutoff <= wall {
            return Err(error("REMOTE_UNATTENDED_DEADLINE"));
        }
        preparation.end = cap;
        preparation.cutoff = cutoff;
        let record = preparation.candidate.record();
        let key_binding = FrozenHostBinding {
            native_profile: HostKeyProfileIdentity {
                id: record.native.profile_id,
                revision: record.native.profile_revision,
            },
            endpoint_spki: record
                .native
                .endpoint_spki
                .as_slice()
                .try_into()
                .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?,
            relay_spki: record
                .native
                .relay_spki
                .as_slice()
                .try_into()
                .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?,
            origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&record.route.origin)
                .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?,
            relay_id: record.route.relay_id,
            profile_cap: cap,
        };
        let supplier = NativeHostProfile::new(
            key_binding.clone(),
            &record.native.endpoint_key_name,
            &record.native.relay_key_name,
        )
        .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?
        .freeze();
        preparation.check(store, admission)?;
        Ok(Self {
            mode,
            preparation,
            key_binding,
            supplier: Some(supplier),
        })
    }
    fn render(&self) -> String {
        let p = &self.preparation;
        let record = serde_json::to_string(p.candidate.record()).expect("validated public record");
        let fingerprint = crate::remote_native_pairing_store::record_digest(p.candidate.record())
            .expect("validated record digest")
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let mode = match self.mode {
            FinalMode::Local => "当前批准仅限本机委托，不开启网络。",
            FinalMode::ShortNetwork => "允许经下列 Relay 为此设备开启一次短网络会话（最多 10 秒，且不超过所示实际截止），可读取该 run 终端快照并输入；不自动重连、不延长授权。",
            FinalMode::NetworkSession => "允许此设备在所示绝对截止内连接当前 run，读取终端快照和输入；签名材料仅短期持有，认证后的会话可持续到该截止。本阶段不自动重连，不跨 App 重启恢复。",
            FinalMode::ReconnectSession => "允许此设备在所示固定绝对截止内重连同一 run，读取终端快照和输入。每次重连重新认证，旧连接及材料确认退休后才启动新连接；不延长截止，不跨 App 重启恢复。",
        };
        format!(
            "允许为此设备持久保存当前单会话委托？这不是之前的准备步骤。
设备: {}
Run: {}
Generation: {}
权限: {:?}
实际截止 (Unix ms): {}
同意后会持久写入 Enabled，并仅捕获此 run。进程结束后不恢复；{mode}关闭立即禁止本机新准入，持久撤销与资源退休分别确认；写入结果 Unknown 时保持关闭，不能保证记录已撤销。此批准不延长现有密钥材料截止，也不等于 30 天可用。\n完整记录指纹: {fingerprint}\n精确公开设备与 origin/RP/Relay 路由: {}",
            public_text(&p.candidate.record().device_id), public_text(&p.scope.run),
            public_text(&p.scope.generation), p.role, p.cutoff, public_text(&record)
        )
    }
}
impl FinalApproval {
    pub(crate) fn matches_control(&self, control: &Arc<FinalControl>) -> bool {
        Arc::ptr_eq(&self.control, control)
    }
    pub(crate) fn waiting(&self) -> bool {
        self.machine.waiting()
    }

    pub(crate) fn show<B: Backend, S: Sealer, R: tauri::Runtime>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<Self, SafeError> {
        Self::show_mode(approved, store, admission, app, window, FinalMode::Local)
    }
    pub(crate) fn show_network<B: Backend, S: Sealer, R: tauri::Runtime>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<Self, SafeError> {
        Self::show_mode(
            approved,
            store,
            admission,
            app,
            window,
            FinalMode::ShortNetwork,
        )
    }
    fn show_mode<B: Backend, S: Sealer, R: tauri::Runtime>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        mode: FinalMode,
    ) -> Result<Self, SafeError> {
        let description = FinalDescription::new(approved, store, admission, mode)?;
        let text = description.render();
        description.preparation.check(store, admission)?;
        if DIALOG_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(error("REMOTE_APPROVAL_BUSY"));
        }
        let cap = description.preparation.end;
        Ok(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                description,
                text,
                "当前单会话持久委托最终确认",
                cap,
            ),
            control: Arc::new(FinalControl {
                closed: NativeRevocation::new(),
                publication: Mutex::new(Publication::Waiting),
                closed_notify: parking_lot::Condvar::new(),
                work: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }
    pub(crate) fn cancel(&self) {
        self.control.close();
        self.machine.cancel();
    }
    pub(crate) fn consume<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Option<FinalConsent>, SafeError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|description, end| {
                description.preparation.check(store, admission)?;
                let mut publication = self.control.publication.lock();
                if self.control.closed()
                    || *publication != Publication::Waiting
                    || self.machine.lease.is_revoked()
                    || Instant::now() >= end
                {
                    return Err(error("REMOTE_UNATTENDED_CANCELLED"));
                }
                *publication = Publication::Starting;
                Ok(FinalConsent {
                    description,
                    machine: self.machine.clone(),
                    control: self.control.clone(),
                    end,
                })
            })
            .transpose()
    }
    #[cfg(test)]
    pub(crate) fn test_pending<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        Self::test_pending_mode(approved, store, admission, FinalMode::Local)
    }
    #[cfg(test)]
    pub(crate) fn test_pending_network<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        Self::test_pending_mode(approved, store, admission, FinalMode::ShortNetwork)
    }
    #[cfg(test)]
    pub(crate) fn test_pending_session<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        Self::test_pending_mode(approved, store, admission, FinalMode::NetworkSession)
    }
    #[cfg(test)]
    pub(crate) fn test_pending_reconnect<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Self, SafeError> {
        Self::test_pending_mode(approved, store, admission, FinalMode::ReconnectSession)
    }
    #[cfg(test)]
    fn test_pending_mode<B: Backend, S: Sealer>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        mode: FinalMode,
    ) -> Result<Self, SafeError> {
        let description = FinalDescription::new(approved, store, admission, mode)?;
        let cap = description.preparation.end;
        Ok(Self {
            machine: ApprovalMachine::pending(description, Instant::now(), cap),
            control: Arc::new(FinalControl {
                closed: NativeRevocation::new(),
                publication: Mutex::new(Publication::Waiting),
                closed_notify: parking_lot::Condvar::new(),
                work: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_answer(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
impl FinalConsent {
    pub(crate) fn permits_network(&self) -> bool {
        self.description.mode != FinalMode::Local
    }
    pub(crate) fn permits_reconnect(&self) -> bool {
        self.description.mode == FinalMode::ReconnectSession
    }
    pub(crate) fn permits_authenticated_data(&self) -> bool {
        matches!(
            self.description.mode,
            FinalMode::NetworkSession | FinalMode::ReconnectSession
        )
    }
    pub(crate) fn check_start<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        self.check_window()?;
        self.description.preparation.check(store, admission)?;
        self.check_window()
    }
    fn check_window(&self) -> Result<(), SafeError> {
        if self.control.closed() || self.machine.lease.is_revoked() || Instant::now() >= self.end {
            return Err(error("REMOTE_UNATTENDED_CANCELLED"));
        }
        Ok(())
    }
    // 调用前必须在门外完成所有真实对象尾检；这里不承诺未来对象持续有效。
    pub(crate) fn publish(&self) -> Result<(), SafeError> {
        let mut publication = self.control.publication.lock();
        self.check_window()?;
        if *publication != Publication::Starting {
            return Err(error("REMOTE_UNATTENDED_CANCELLED"));
        }
        *publication = Publication::Active;
        Ok(())
    }
    pub(crate) fn control(&self) -> Arc<FinalControl> {
        self.control.clone()
    }
    pub(crate) fn claim_epochs(
        &mut self,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<EpochSource, SafeError> {
        self.check_window()?;
        if !permit.matches(&self.control) {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        let original = self
            .description
            .supplier
            .take()
            .ok_or_else(|| error("REMOTE_UNATTENDED_STALE"))?;
        Ok(EpochSource {
            control: self.control.clone(),
            original,
        })
    }
    pub(crate) fn check_epoch_source(
        &self,
        source: &EpochSource,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<(), SafeError> {
        self.check_window()?;
        if !Arc::ptr_eq(&source.control, &self.control) || !permit.matches(&self.control) {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        Ok(())
    }
    // 子代只能沿本次真实批准和原 App 槽派生；不反向撤父，不接受外部 key/binding。
    pub(crate) fn epoch_token(
        &self,
        source: &EpochSource,
        permit: &crate::remote_host::unattended::StartPermit,
    ) -> Result<Arc<NativeRevocation>, SafeError> {
        self.check_epoch_source(source, permit)?;
        let lease = self
            .machine
            .lease
            .native
            .root_witness()
            .ok_or_else(|| error("REMOTE_UNATTENDED_STALE"))?;
        let control = self
            .control
            .closed
            .root_witness()
            .ok_or_else(|| error("REMOTE_UNATTENDED_STALE"))?;
        Ok(NativeRevocation::child([lease, control]))
    }
    pub(crate) fn epoch_supplier<B: Backend, S: Sealer>(
        &self,
        source: &EpochSource,
        permit: &crate::remote_host::unattended::StartPermit,
        store: &PairingStore<B, S>,
    ) -> Result<FrozenHostSupplier, SafeError> {
        self.check_epoch_source(source, permit)?;
        self.check_runtime(store)?;
        let mut binding = source.original.binding().clone();
        binding.profile_cap = self.end().min(Instant::now() + Duration::from_secs(10));
        let record = self.candidate().record();
        let supplier = NativeHostProfile::new(
            binding,
            &record.native.endpoint_key_name,
            &record.native.relay_key_name,
        )
        .map_err(|_| error("REMOTE_HOST_CONFIGURATION"))?
        .freeze();
        self.check_runtime(store)?;
        self.check_epoch_source(source, permit)?;
        Ok(supplier)
    }
    pub(crate) fn key_binding(&self) -> &FrozenHostBinding {
        &self.description.key_binding
    }
    pub(crate) fn take_supplier(&mut self) -> Result<FrozenHostSupplier, SafeError> {
        self.check_window()?;
        self.description
            .supplier
            .take()
            .ok_or_else(|| error("REMOTE_UNATTENDED_STALE"))
    }
}

impl FinalConsent {
    pub(crate) fn candidate(&self) -> &InstalledDeviceCandidate {
        &self.description.preparation.candidate
    }
    pub(crate) fn scope(&self) -> &NativeSessionScope {
        &self.description.preparation.scope
    }
    pub(crate) fn role(&self) -> RequestedRole {
        self.description.preparation.role
    }
    pub(crate) fn cutoff(&self) -> u64 {
        self.description.preparation.cutoff
    }
    pub(crate) fn end(&self) -> Instant {
        self.description.preparation.end.min(self.end)
    }
    pub(crate) fn check_before_publication<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
    ) -> Result<(), SafeError> {
        if !(self.description.preparation.checks)() {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        self.check_runtime(store)?;
        if !(self.description.preparation.checks)() {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        self.check_window()
    }
    pub(crate) fn check_runtime<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
    ) -> Result<(), SafeError> {
        self.check_window()?;
        store
            .revalidate_installed(self.candidate())
            .map_err(|_| error("REMOTE_UNATTENDED_DEVICE_CHANGED"))?;
        let p = &self.description.preparation;
        p.clock
            .lock()
            .observe(wall_ms(), Instant::now(), p.cutoff, p.end)?;
        self.check_window()
    }
}

impl FinalConsent {
    pub(crate) fn matches_original(&self, admission: &NativeRunAdmission) -> bool {
        self.description.preparation.origin.matches(admission)
    }
}

// 本代原 key 事实与同父委托的原 socket attempt 共同构成短签名准入。
// 没有 caller token、公开构造器或 Clone；不能从历史 Enabled 恢复。
pub(crate) struct DelegatedSigningAuthorization {
    host_identity: crate::remote_host::delegated::HostLifetimeIdentity,
    record: crate::remote_native_pairing_store::PublicDeviceRecord,
    scope: NativeSessionScope,
    key: crate::native_crypto::host::ReadySigningIdentity,
    attempt: crate::remote_host::delegated::DelegatedAttemptIdentity,
    budget: crate::remote_unattended::capability::network::NetworkBudget,
    control: Arc<FinalControl>,
    end: Instant,
    data_end: Instant,
}
impl DelegatedSigningAuthorization {
    pub(crate) fn host_identity(&self) -> &crate::remote_host::delegated::HostLifetimeIdentity {
        &self.host_identity
    }
    #[cfg(test)]
    pub(crate) fn test_exact_binding(&self) -> bool {
        self.matches_attempt(&self.attempt)
            && !self.matches_attempt(&self.attempt.test_foreign_attempt())
            && !self.matches_attempt(&self.attempt.test_foreign_session())
            && self.key.test_authorization_caps(self)
    }
    pub(crate) fn record(&self) -> &crate::remote_native_pairing_store::PublicDeviceRecord {
        &self.record
    }
    pub(crate) fn scope(&self) -> &NativeSessionScope {
        &self.scope
    }
    pub(crate) fn matches(&self, token: &Arc<NativeRevocation>, cap: Instant) -> bool {
        self.control.active()
            && Instant::now() < self.end
            && self.budget.check().is_ok()
            && self.key.matches(token, cap)
    }
    pub(crate) fn matches_attempt(
        &self,
        attempt: &crate::remote_host::delegated::DelegatedAttemptIdentity,
    ) -> bool {
        self.attempt.matches(attempt) && self.host_identity.matches_attempt(attempt)
    }
    pub(crate) fn data_end(&self) -> Instant {
        self.data_end
    }
    pub(crate) fn end(&self) -> Instant {
        self.end
    }
}
impl FinalConsent {
    pub(crate) fn network_signing_authorization(
        &self,
        permit: &crate::remote_host::unattended::StartPermit,
        start: &crate::remote_host::delegated::DelegatedSocketStart,
        ready: &crate::native_crypto::host::HostKeysReady,
        keys: &crate::native_crypto::host::HostKeyCustody,
        binding: &FrozenHostBinding,
    ) -> Result<DelegatedSigningAuthorization, SafeError> {
        self.check_window()?;
        let original = self.key_binding();
        if !self.permits_network()
            || !matches!(self.role(), RequestedRole::Control)
            || !permit.matches(&self.control)
            || !self.control.active()
            || !start.matches_final_control(&self.control)
            || binding.native_profile != original.native_profile
            || binding.endpoint_spki != original.endpoint_spki
            || binding.relay_spki != original.relay_spki
            || binding.origin != original.origin
            || binding.relay_id != original.relay_id
            || binding.profile_cap > original.profile_cap
        {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        let key = ready
            .signing_identity(keys, binding)
            .map_err(|_| error("REMOTE_UNATTENDED_STALE"))?;
        if !key.has_roots(&self.machine.lease.native, &self.control.closed) {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        let budget = start.budget()?;
        let budget_end = budget.deadline().min(self.end());
        let end = budget_end.min(key.cap());
        self.check_window()?;
        if !self.control.active() || Instant::now() >= end {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        Ok(DelegatedSigningAuthorization {
            host_identity: start.host_lifetime_identity()?,
            record: self.candidate().record().clone(),
            scope: self.scope().clone(),
            key,
            attempt: start.identity(),
            budget,
            control: self.control.clone(),
            end,
            data_end: if self.permits_authenticated_data() {
                budget_end
            } else {
                end
            },
        })
    }
}
