//! 独立最终批准；A3 的准备回执不等于本模块的持久 Enable 同意。
use super::*;
use crate::native_crypto::{
    host::{FrozenHostBinding, FrozenHostSupplier, HostKeyProfileIdentity},
    host_profile::NativeHostProfile,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Publication {
    Waiting,
    Starting,
    Active,
    Closed,
}

// 取消只推进不可复位的位；短门不持有存储、run 或密钥锁。
pub(crate) struct FinalControl {
    closed: AtomicBool,
    publication: Mutex<Publication>,
}
impl FinalControl {
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        *self.publication.lock() = Publication::Closed;
    }
    pub(crate) fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(crate) fn active(&self) -> bool {
        !self.closed() && *self.publication.lock() == Publication::Active
    }
}

struct FinalDescription {
    preparation: UnattendedPreparation,
    key_binding: FrozenHostBinding,
    supplier: Option<FrozenHostSupplier>,
}
pub(crate) struct FinalApproval {
    machine: Arc<ApprovalMachine<FinalDescription>>,
    control: Arc<FinalControl>,
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
    ) -> Result<Self, SafeError> {
        approved.check(store, admission)?;
        let mut preparation = approved.preparation;
        // 固定短材料窗口，不把等待确认的时间重新加到 cap 上。
        let anchor = Instant::now();
        let cap = preparation.end.min(anchor + Duration::from_secs(60));
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
        format!(
            "允许为此设备持久保存当前单会话委托？这不是之前的准备步骤。
设备: {}
Run: {}
Generation: {}
权限: {:?}
实际截止 (Unix ms): {}
同意后会持久写入 Enabled，并仅捕获此 run。进程结束后不恢复；当前实现不开启网络。关闭立即禁止本机新准入，持久撤销与资源退休分别确认；写入结果 Unknown 时保持关闭，不能保证记录已撤销。此批准不延长现有密钥材料截止，也不等于 30 天可用。\n完整记录指纹: {fingerprint}\n精确公开设备与 origin/RP/Relay 路由: {}",
            public_text(&p.candidate.record().device_id), public_text(&p.scope.run),
            public_text(&p.scope.generation), p.role, p.cutoff, public_text(&record)
        )
    }
}
impl FinalApproval {
    pub(crate) fn show<B: Backend, S: Sealer, R: tauri::Runtime>(
        approved: ApprovedPreparation,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<Self, SafeError> {
        let description = FinalDescription::new(approved, store, admission)?;
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
                closed: AtomicBool::new(false),
                publication: Mutex::new(Publication::Waiting),
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
        let description = FinalDescription::new(approved, store, admission)?;
        let cap = description.preparation.end;
        Ok(Self {
            machine: ApprovalMachine::pending(description, Instant::now(), cap),
            control: Arc::new(FinalControl {
                closed: AtomicBool::new(false),
                publication: Mutex::new(Publication::Waiting),
            }),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_answer(&self, yes: bool) {
        self.machine.complete(yes, Instant::now());
    }
}
impl FinalConsent {
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
