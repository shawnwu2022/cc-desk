//! 运行身份用于保守拒绝关联会话，不证明有效数据根或完整写入范围。
use super::maintenance::AdmissionGate;
use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;

struct IdentityState {
    ids: Option<BTreeSet<uuid::Uuid>>,
    observed: bool,
}
pub(crate) struct CodexOwnerIdentity(Mutex<IdentityState>);
impl CodexOwnerIdentity {
    pub(crate) fn new(initial: Option<uuid::Uuid>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(IdentityState {
            ids: Some(initial.into_iter().collect()),
            observed: false,
        })))
    }
    /// 只能追加经过认证的当前 run 身份；Stop 不表示进程已退出，也不清除旧身份。
    pub(crate) fn observe(&self, id: uuid::Uuid) {
        let mut state = self.0.lock();
        state.observed = true;
        if let Some(ids) = state.ids.as_mut() {
            if ids.len() == 64 && !ids.contains(&id) {
                state.ids = None;
            } else {
                ids.insert(id);
            }
        }
    }
    pub(crate) fn sessions(&self) -> Result<BTreeSet<uuid::Uuid>, SafeError> {
        let state = self.0.lock();
        if !state.observed {
            return Err(error("ARCHIVE_BUSY"));
        }
        state
            .ids
            .as_ref()
            .filter(|ids| !ids.is_empty())
            .cloned()
            .ok_or_else(|| error("ARCHIVE_BUSY"))
    }
}

pub(crate) struct CodexArchiveOwners {
    pub(super) gate: AdmissionGate,
    pub(super) epoch: u64,
    pub(super) transaction_id: String,
    pub(super) identities: Vec<Arc<CodexOwnerIdentity>>,
    pub(super) sessions: BTreeSet<uuid::Uuid>,
}
impl CodexArchiveOwners {
    pub(crate) fn sessions(&self) -> &BTreeSet<uuid::Uuid> {
        &self.sessions
    }
    pub(crate) fn current(&self) -> Result<(), SafeError> {
        self.with_current(|| ())
    }
    /// 仅在核验与提交发送状态时持锁；不得在 action 中执行 I/O 或等待 RPC。
    pub(crate) fn with_current<T>(&self, action: impl FnOnce() -> T) -> Result<T, SafeError> {
        self.gate
            .with_archive_epoch(self.epoch, &self.transaction_id, || {
                let held: Vec<_> = self
                    .identities
                    .iter()
                    .map(|identity| identity.0.lock())
                    .collect();
                let mut current = BTreeSet::new();
                for state in &held {
                    if !state.observed {
                        return Err(error("ARCHIVE_BUSY"));
                    }
                    let ids = state
                        .ids
                        .as_ref()
                        .filter(|ids| !ids.is_empty())
                        .ok_or_else(|| error("ARCHIVE_BUSY"))?;
                    current.extend(ids.iter().copied());
                }
                if current != self.sessions {
                    return Err(error("ARCHIVE_BUSY"));
                }
                Ok(action())
            })
    }
}
#[cfg(test)]
#[path = "../tests/native_cli_archive_admission.rs"]
mod tests;
