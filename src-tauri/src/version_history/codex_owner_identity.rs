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
    /// Only a single authenticated hook identity can label a catalogue row.
    /// A requested resume ID by itself is not provider evidence.
    #[cfg(all(windows, feature = "remote-observer"))]
    pub(crate) fn catalogue_primary(&self) -> Option<uuid::Uuid> {
        let snapshot = self.archive_snapshot().ok()?;
        let ids = snapshot.ids?;
        let id = *ids.first()?;
        (snapshot.observed
            && ids.len() == 1
            && !id.is_nil()
            && snapshot
                .requested_id
                .is_none_or(|requested| requested == id))
        .then_some(id)
    }

    pub(crate) fn sessions(&self) -> Result<BTreeSet<uuid::Uuid>, SafeError> {
        let state = self.archive_snapshot()?;
        if !state.observed {
            return Err(error("ARCHIVE_BUSY"));
        }
        state.ids.ok_or_else(|| error("ARCHIVE_BUSY"))
    }
}

#[cfg(all(test, windows, feature = "remote-observer"))]
mod catalogue_tests {
    use super::*;

    #[test]
    fn codex_catalogue_requires_a_single_root_observed_for_the_original_attempt() {
        let expected = uuid::Uuid::from_u128(1);
        let other = uuid::Uuid::from_u128(2);
        let owner = CodexOwnerIdentity::new(Some(expected));
        assert_eq!(owner.catalogue_primary(), None);
        owner.observe(other);
        assert_eq!(owner.catalogue_primary(), None);

        let owner = CodexOwnerIdentity::new(Some(expected));
        owner.observe(expected);
        owner.observe(expected);
        assert_eq!(owner.catalogue_primary(), Some(expected));
        owner.observe(other);
        assert_eq!(owner.catalogue_primary(), None);

        let owner = CodexOwnerIdentity::new(None);
        owner.observe(uuid::Uuid::nil());
        assert_eq!(owner.catalogue_primary(), None);
    }
}
