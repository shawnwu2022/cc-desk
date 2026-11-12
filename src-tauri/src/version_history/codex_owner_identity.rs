use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(super) struct IdentityState {
    pub(super) ids: Option<BTreeSet<uuid::Uuid>>,
    pub(super) observed: bool,
}
pub(crate) struct CodexOwnerIdentity(pub(super) Mutex<IdentityState>);
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
