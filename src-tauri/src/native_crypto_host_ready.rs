//! 原两个 worker 的当前加载收据；不等于可签名、授权或恢复许可。
use super::*;
use crate::native_crypto::owner::HostOpenedKey;

pub(crate) struct HostKeysReady {
    core: std::sync::Weak<HostKeyCore>,
    keys: [HostOpenedKey; 2],
}
fn live(core: &HostKeyCore, state: &CoreState) -> Result<(), SignFailure> {
    if state.sealed
        || state.unknown
        || state.disposing
        || state.disposal_complete
        || core.token.is_revoked()
        || Instant::now() >= state.cap
    {
        return Err(SignFailure::Closed);
    }
    Ok(())
}
impl HostKeyCustody {
    #[cfg(test)]
    pub(crate) fn test_worker_opened(&self, index: usize) -> bool {
        let state = self.core.state.lock();
        state
            .slots
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|slot| slot.signal.host_opened().ok().flatten().is_some())
    }
    // 只做非等待观察；原应用任务决定有界等待，不增加 CNG worker 或丢弃原 Join。
    pub(crate) fn take_ready(&self) -> Result<Option<super::HostKeysReady>, SignFailure> {
        let mut state = self.core.state.lock();
        live(&self.core, &state)?;
        if state.ready_issued {
            return Err(SignFailure::Closed);
        }
        if !state.publisher_closed {
            return Ok(None);
        }
        let Some(first) = state.slots[0].as_ref() else {
            return Ok(None);
        };
        let Some(second) = state.slots[1].as_ref() else {
            return Ok(None);
        };
        let Some(first) = first.signal.host_opened()? else {
            return Ok(None);
        };
        let Some(second) = second.signal.host_opened()? else {
            return Ok(None);
        };
        let receipt = HostKeysReady {
            core: Arc::downgrade(&self.core),
            keys: [first, second],
        };
        receipt.check_locked(&self.core, &state)?;
        state.ready_issued = true;
        Ok(Some(receipt))
    }
}
impl HostKeysReady {
    fn check_locked(&self, core: &HostKeyCore, state: &CoreState) -> Result<(), SignFailure> {
        live(core, state)?;
        let binding = &state.supplier.as_ref().ok_or(SignFailure::Closed)?.binding;
        for (index, purpose, spki) in [
            (0, SigningPurpose::Endpoint, &binding.endpoint_spki),
            (1, SigningPurpose::Relay, &binding.relay_spki),
        ] {
            let slot = state.slots[index].as_ref().ok_or(SignFailure::Closed)?;
            self.keys[index].check(&slot.signal, purpose, spki, &core.token, state.cap)?;
        }
        live(core, state)
    }
    pub(crate) fn check(
        &self,
        owner: &HostKeyCustody,
        expected: &FrozenHostBinding,
    ) -> Result<(), SignFailure> {
        if !std::sync::Weak::ptr_eq(&self.core, &Arc::downgrade(&owner.core)) {
            return Err(SignFailure::Closed);
        }
        let core = self.core.upgrade().ok_or(SignFailure::Closed)?;
        let state = core.state.lock();
        let binding = &state.supplier.as_ref().ok_or(SignFailure::Closed)?.binding;
        if binding.native_profile != expected.native_profile
            || binding.endpoint_spki != expected.endpoint_spki
            || binding.relay_spki != expected.relay_spki
            || binding.origin != expected.origin
            || binding.relay_id != expected.relay_id
            || binding.profile_cap != expected.profile_cap
        {
            return Err(SignFailure::Closed);
        }
        self.check_locked(&core, &state)
    }
    pub(crate) fn consume(
        self,
        owner: &HostKeyCustody,
        expected: &FrozenHostBinding,
    ) -> Result<(), SignFailure> {
        self.check(owner, expected)
    }
}
