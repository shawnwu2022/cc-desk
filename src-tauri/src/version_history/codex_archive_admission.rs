//! 运行身份用于保守拒绝关联会话，不证明有效数据根或完整写入范围。
use super::maintenance::AdmissionGate;
use crate::cli::{profiles::error, types::SafeError};
pub(crate) use crate::version_history::codex_owner_identity::CodexOwnerIdentity;
use std::collections::BTreeSet;
use std::sync::Arc;

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
