//! 单 run 本机批准准备；没有到网络、持久委托或 producer 的转换。
use super::*;
use crate::{
    cli::{profiles::error, types::SafeError},
    remote_native_pairing_store::{Backend, InstalledDeviceCandidate, PairingStore, Sealer},
    run_supervisor::{NativeRunAdmission, NativeRunNetworkChecks, NativeRunNetworkOrigin},
};
use cc_desk_remote_host::native_host::NativeSessionScope;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_DELEGATION_MS: u64 = 30 * 24 * 60 * 60 * 1000;
#[derive(Debug, Clone, Copy)]
pub(crate) enum RequestedRole {
    View,
    Control,
}
fn wall_ms() -> Result<u64, SafeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| v.as_millis().try_into().ok())
        .ok_or_else(|| error("REMOTE_UNATTENDED_CLOCK"))
}
// 仅锁存本实例实际观察的回退/失败；不是跨重启或未观察间隙的回滚检测。
struct ClockObservation {
    wall: u64,
    mono: Instant,
    invalid: bool,
}
impl ClockObservation {
    fn observe(
        &mut self,
        wall: Result<u64, SafeError>,
        mono: Instant,
        cutoff: u64,
        end: Instant,
    ) -> Result<(), SafeError> {
        let valid = wall.ok().filter(|wall| {
            !self.invalid && *wall >= self.wall && *wall < cutoff && mono >= self.mono && mono < end
        });
        let Some(wall) = valid else {
            self.invalid = true;
            return Err(error("REMOTE_UNATTENDED_CLOCK"));
        };
        self.wall = wall;
        self.mono = mono;
        Ok(())
    }
}
// 原 RunAccess 检查闭包保留原 document/caller；不能用同 run 的新文档替代。
pub(crate) struct UnattendedPreparation {
    candidate: InstalledDeviceCandidate,
    origin: NativeRunNetworkOrigin,
    checks: NativeRunNetworkChecks,
    scope: NativeSessionScope,
    role: RequestedRole,
    clock: Mutex<ClockObservation>,
    cutoff: u64,
    end: Instant,
}
impl UnattendedPreparation {
    pub(crate) fn new<B: Backend, S: Sealer>(
        store: &PairingStore<B, S>,
        candidate: InstalledDeviceCandidate,
        admission: &NativeRunAdmission,
        role: RequestedRole,
        cutoff: u64,
    ) -> Result<Self, SafeError> {
        let anchor = Instant::now();
        let wall_floor = wall_ms()?;
        let duration = cutoff
            .checked_sub(wall_floor)
            .filter(|v| *v > 0 && *v <= MAX_DELEGATION_MS)
            .ok_or_else(|| error("REMOTE_UNATTENDED_DEADLINE"))?;
        let end = anchor
            .checked_add(Duration::from_millis(duration))
            .ok_or_else(|| error("REMOTE_UNATTENDED_DEADLINE"))?;
        let (origin, checks) = admission.network_checks()?;
        let value = Self {
            candidate,
            origin,
            checks,
            scope: admission.scope()?,
            role,
            clock: Mutex::new(ClockObservation {
                wall: wall_floor,
                mono: anchor,
                invalid: false,
            }),
            cutoff,
            end,
        };
        value.check(store, admission)?;
        Ok(value)
    }
    fn check_current(&self, admission: &NativeRunAdmission) -> Result<(), SafeError> {
        if !(self.checks)() || !self.origin.matches(admission) || admission.scope()? != self.scope {
            return Err(error("REMOTE_UNATTENDED_STALE"));
        }
        // 在短锁内采样与推进，避免并发旧样本倒序；不跨存储 I/O 持锁。
        let mut clock = self.clock.lock();
        clock.observe(wall_ms(), Instant::now(), self.cutoff, self.end)
    }
    fn check<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        self.check_current(admission)?;
        store
            .revalidate_installed(&self.candidate)
            .map_err(|_| error("REMOTE_UNATTENDED_DEVICE_CHANGED"))?;
        self.check_current(admission)
    }
    fn render(&self) -> String {
        let record = self.candidate.record();
        // 完整冻结公开记录含确切密钥指纹来源、路由和 epochs；不是当前密钥可用证明。
        let record = serde_json::to_string(record).expect("validated public record serializes");
        format!("准备当前会话的无人值守本机批准。此步骤不会开启网络、持久授权或跨重启恢复。后续持久 owner 和当前密钥核验未完成时保持关闭。\nRun: {}\nGeneration: {}\nRole: {:?}\nAbsolute cutoff (Unix ms): {}\nStore revision: {}\nExact public pairing: {}", public_text(&self.scope.run), public_text(&self.scope.generation), self.role, self.cutoff, self.candidate.expected().revision, public_text(&record))
    }
    pub(crate) fn show<B: Backend, S: Sealer, R: tauri::Runtime>(
        self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
    ) -> Result<UnattendedApproval, SafeError> {
        self.check(store, admission)?;
        let text = self.render();
        self.check(store, admission)?;
        if DIALOG_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(error("REMOTE_APPROVAL_BUSY"));
        }
        let end = self.end;
        Ok(UnattendedApproval {
            gate: Mutex::new(()),
            machine: ApprovalMachine::show(app, window, self, text, "当前会话本机批准准备", end),
        })
    }
}
pub(crate) struct UnattendedApproval {
    gate: Mutex<()>,
    machine: Arc<ApprovalMachine<UnattendedPreparation>>,
}
// 仅记录短消费窗口内的本机批准，不授予当前或未来连接权。
pub(crate) struct ApprovedPreparation {
    preparation: UnattendedPreparation,
    machine: std::sync::Weak<ApprovalMachine<UnattendedPreparation>>,
    end: Instant,
}
impl UnattendedApproval {
    pub(crate) fn waiting(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn cancel(&self) {
        let _gate = self.gate.lock();
        self.machine.cancel();
    }
    pub(crate) fn consume<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<Option<ApprovedPreparation>, SafeError> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        permit
            .consume(|preparation, end| {
                // 读盘不阻塞取消；短门只线性化最终发布与取消。
                preparation.check(store, admission)?;
                preparation.check_current(admission)?;
                let _gate = self.gate.lock();
                let fact = ApprovedPreparation {
                    preparation,
                    machine: Arc::downgrade(&self.machine),
                    end,
                };
                fact.check_lease()?;
                Ok(fact)
            })
            .transpose()
    }
    #[cfg(test)]
    pub(crate) fn test_pending(preparation: UnattendedPreparation) -> Self {
        let end = preparation.end;
        Self {
            gate: Mutex::new(()),
            machine: ApprovalMachine::pending(preparation, Instant::now(), end),
        }
    }
    #[cfg(test)]
    pub(crate) fn test_answer(&self, yes: bool, late: bool) {
        self.machine.complete(
            yes,
            Instant::now() + if late { PENDING } else { Duration::ZERO },
        );
    }
}
impl ApprovedPreparation {
    #[cfg(test)]
    pub(crate) fn test_limit_end(&mut self, end: Instant) {
        self.end = self.end.min(end);
    }
    fn check_lease(&self) -> Result<(), SafeError> {
        let machine = self
            .machine
            .upgrade()
            .ok_or_else(|| error("REMOTE_UNATTENDED_CANCELLED"))?;
        if machine.lease.is_revoked() || machine.state.lock().closed || Instant::now() >= self.end {
            return Err(error("REMOTE_UNATTENDED_CANCELLED"));
        }
        Ok(())
    }
    pub(crate) fn check<B: Backend, S: Sealer>(
        &self,
        store: &PairingStore<B, S>,
        admission: &NativeRunAdmission,
    ) -> Result<(), SafeError> {
        self.check_lease()?;
        self.preparation.check(store, admission)?;
        self.check_lease()
    }
}
#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    #[allow(non_snake_case)]
    fn UnattendedPreparation_ObservedClockFailureIsSticky_008() {
        let anchor = Instant::now();
        let end = anchor + Duration::from_secs(10);
        for mode in 0..4 {
            let mut clock = ClockObservation {
                wall: 100,
                mono: anchor,
                invalid: false,
            };
            clock
                .observe(Ok(120), anchor + Duration::from_secs(1), 200, end)
                .unwrap();
            let (wall, mono) = match mode {
                0 => (Ok(110), anchor + Duration::from_secs(2)),
                1 => (Ok(130), anchor),
                2 => (
                    Err(error("TEST_ONLY_CLOCK_FAILURE")),
                    anchor + Duration::from_secs(2),
                ),
                _ => (Ok(200), anchor + Duration::from_secs(2)),
            };
            assert!(clock.observe(wall, mono, 200, end).is_err());
            assert!(clock
                .observe(Ok(150), anchor + Duration::from_secs(3), 200, end)
                .is_err());
        }
    }
}

#[path = "remote_unattended_final_approval.rs"]
pub(crate) mod final_approval;
