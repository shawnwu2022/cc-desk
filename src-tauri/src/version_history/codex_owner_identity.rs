use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct IdentityState {
    pub(super) requested_id: Option<uuid::Uuid>,
    pub(super) ids: Option<BTreeSet<uuid::Uuid>>,
    pub(super) observed: bool,
}
pub(crate) struct CodexOwnerIdentity(pub(super) Mutex<IdentityState>);
impl CodexOwnerIdentity {
    pub(crate) fn new(initial: Option<uuid::Uuid>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(IdentityState {
            requested_id: initial,
            ids: Some(BTreeSet::new()),
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
    /// 等待 hook 不证明占用任何目标；溢出或不一致状态仍不能降级为等待身份。
    pub(super) fn archive_snapshot(&self) -> Result<IdentityState, SafeError> {
        let state = self.0.lock();
        match (&state.ids, state.observed) {
            (Some(ids), false) if ids.is_empty() => Ok(state.clone()),
            (Some(ids), true) if !ids.is_empty() => Ok(state.clone()),
            _ => Err(error("ARCHIVE_BUSY")),
        }
    }
    pub(crate) fn sessions(&self) -> Result<BTreeSet<uuid::Uuid>, SafeError> {
        let state = self.archive_snapshot()?;
        if !state.observed {
            return Err(error("ARCHIVE_BUSY"));
        }
        state.ids.ok_or_else(|| error("ARCHIVE_BUSY"))
    }
}
