use super::RunningChannel;
use crate::{BoundDataAuthority, DataError, GrantClock, GrantClockDomain, Role};
use cc_desk_remote_record::grant::DataRole;
use std::sync::Arc;
// 只表示原已安装通道的窄事实；不可由 DTO 构造，也不授权任何输入。
pub struct LiveKeepaliveFact {
    authority: BoundDataAuthority,
    peer: Arc<crate::peer_install::Shared>,
    fence: u64,
}
impl RunningChannel {
    pub fn live_keepalive_fact(
        &self,
        expected_role: Role,
    ) -> Result<Option<LiveKeepaliveFact>, DataError> {
        let authority = self.authority();
        authority.check()?;
        if authority.local_role() != expected_role
            || authority.data_role() != DataRole::Controller
            || !authority.data_keepalive_negotiated()
        {
            return Err(DataError::Role);
        }
        let Some(fence) = self.peer.cached_live_fence()? else {
            authority.check()?;
            return Ok(None);
        };
        let fact = LiveKeepaliveFact {
            authority,
            peer: self.peer.clone(),
            fence,
        };
        fact.validate_for(self, expected_role)?;
        Ok(Some(fact))
    }
}
impl LiveKeepaliveFact {
    pub fn validate_for(
        &self,
        channel: &RunningChannel,
        expected_role: Role,
    ) -> Result<(), DataError> {
        self.authority.check()?;
        if !self.authority.same_channel(&channel.authority)
            || !Arc::ptr_eq(&self.peer, &channel.peer)
            || self.authority.local_role() != expected_role
            || self.authority.data_role() != DataRole::Controller
            || !self.authority.data_keepalive_negotiated()
        {
            return Err(DataError::Context);
        }
        if self.peer.cached_live_fence()? != Some(self.fence) {
            return Err(DataError::Fence);
        }
        channel.authority.check()?;
        self.authority.check()
    }
    pub fn uses_clock(&self, clock: &Arc<dyn GrantClock>) -> bool {
        self.authority
            .clock_domain()
            .same_origin(&GrantClockDomain::new(clock.clone()))
    }
    pub fn remaining_ms_for(
        &self,
        channel: &RunningChannel,
        expected_role: Role,
    ) -> Result<u64, DataError> {
        self.validate_for(channel, expected_role)?;
        let now = self
            .authority
            .clock_domain()
            .now_ms()
            .map_err(DataError::Grant)?;
        let remaining = self
            .authority
            .expires_host_ms()
            .checked_sub(now)
            .filter(|n| *n > 0)
            .ok_or(DataError::Deadline)?;
        self.validate_for(channel, expected_role)?;
        Ok(remaining)
    }
    #[cfg(test)]
    pub(crate) fn test_stale_fence(mut self) -> Self {
        self.fence = self.fence.checked_add(1).unwrap();
        self
    }
}
