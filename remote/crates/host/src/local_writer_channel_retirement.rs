//! 原 Host/channel 的局部终结事实，不是新连接许可或完整任务回收证明。
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelRetirement {
    Pending,
    Retired,
    Unknown,
}
pub struct ChannelRetirementToken {
    owner: Arc<()>,
    index: usize,
}
// 仅原 Host 的已安装终局事实；不能单独批准新连接，也不暴露 fence。
pub struct RetiredInstalledChannel {
    owner: Arc<()>,
    index: usize,
    attempt: InstallAttempt,
    fence: u64,
}
pub(super) struct Entry {
    authority: BoundDataAuthority,
    attempts: Vec<InstallAttempt>,
    unknown: bool,
    readiness_issued: bool,
    readiness_consumed: bool,
}
impl NativeHost {
    pub fn begin_channel_retirement(
        &mut self,
        channel: &RunningChannel,
    ) -> Result<ChannelRetirementToken, NativeHostError> {
        let authority = channel.authority();
        if let Some(index) = self.channel_retirements.iter().position(|entry| {
            entry.authority.same_channel(&authority)
                && entry.authority.grant_id() == authority.grant_id()
        }) {
            return Ok(ChannelRetirementToken {
                owner: self.retirement_identity.clone(),
                index,
            });
        }
        if self.channel_retirements.len() >= MAX_ATTEMPTS {
            return Err(NativeHostError::Capacity);
        }
        let same = |original: &BoundDataAuthority| {
            original.same_channel(&authority)
                && original.grant_id() == authority.grant_id()
                && original.scope() == authority.scope()
        };
        let known = self
            .anchor
            .as_ref()
            .is_some_and(|original| same(&original.authority()))
            || self.attempts.iter().any(|a| same(&a.authority))
            || self
                .peer_installs
                .iter()
                .any(|job| same(&job.channel().authority()));
        if !known {
            return Err(NativeHostError::Authority);
        }
        let mut attempts = Vec::new();
        for attempt in &self.attempts {
            if same(&attempt.authority) {
                // reserve 的二次 sealed 检查与原 entries 锁一起封住迟到入队。
                attempt.pending.seal();
                attempts.push(attempt.id);
            }
        }
        authority.request_close();
        let index = self.channel_retirements.len();
        self.channel_retirements.push(Entry {
            authority,
            attempts,
            unknown: false,
            readiness_issued: false,
            readiness_consumed: false,
        });
        Ok(ChannelRetirementToken {
            owner: self.retirement_identity.clone(),
            index,
        })
    }
    pub fn observe_channel_retirement(
        &mut self,
        token: &ChannelRetirementToken,
    ) -> ChannelRetirement {
        if !Arc::ptr_eq(&self.retirement_identity, &token.owner) {
            return ChannelRetirement::Unknown;
        }
        let Some(entry) = self.channel_retirements.get(token.index) else {
            return ChannelRetirement::Unknown;
        };
        if entry.unknown {
            return ChannelRetirement::Unknown;
        }
        let authority = entry.authority.clone();
        let attempts = entry.attempts.clone();
        let mut pending = self.retire_channel_tasks(&authority);
        let mut unknown = self.unknown;
        for attempt in &mut self.attempts {
            if !attempt.authority.same_channel(&authority) {
                continue;
            }
            if !attempts.contains(&attempt.id)
                || attempt.authority.grant_id() != authority.grant_id()
            {
                unknown = true;
                continue;
            }
            Self::poll(attempt);
            pending |= matches!(attempt.state, InstallState::Pending(_));
            unknown |= matches!(attempt.state, InstallState::Unknown);
            match attempt.pending.observe() {
                custody::Observation::Pending => pending = true,
                custody::Observation::Unknown => unknown = true,
                custody::Observation::Drained => {
                    pending |= attempt.pending.prune_drained().is_err();
                }
            }
        }
        if unknown {
            self.channel_retirements[token.index].unknown = true;
            self.unknown = true;
            ChannelRetirement::Unknown
        } else if pending {
            ChannelRetirement::Pending
        } else {
            ChannelRetirement::Retired
        }
    }
}

impl NativeHost {
    /// 完整 App receipt 尚需原 PeerPump/socket/材料 join；此处只签本地事实。
    pub fn retired_installed_channel(
        &mut self,
        token: &ChannelRetirementToken,
    ) -> Result<Option<RetiredInstalledChannel>, NativeHostError> {
        if self.closing || self.unknown {
            return Err(NativeHostError::Closed);
        }
        match self.observe_channel_retirement(token) {
            ChannelRetirement::Pending => return Ok(None),
            ChannelRetirement::Unknown => return Err(NativeHostError::Unknown),
            ChannelRetirement::Retired => {}
        }
        let entry = &self.channel_retirements[token.index];
        if entry.readiness_issued || entry.attempts.len() != 1 {
            return Err(NativeHostError::Rejected);
        }
        let latest = self
            .attempts
            .iter()
            .rfind(|a| a.authority.scope() == entry.authority.scope())
            .ok_or(NativeHostError::Rejected)?;
        if latest.id != entry.attempts[0]
            || !latest.authority.same_channel(&entry.authority)
            || latest.authority.grant_id() != entry.authority.grant_id()
        {
            return Err(NativeHostError::Rejected);
        }
        let InstallState::Installed { fence, .. } = latest.state else {
            return Err(NativeHostError::Rejected);
        };
        let ready = RetiredInstalledChannel {
            owner: self.retirement_identity.clone(),
            index: token.index,
            attempt: latest.id,
            fence,
        };
        self.channel_retirements[token.index].readiness_issued = true;
        Ok(Some(ready))
    }
    /// 消费原本地事实仍重新核 exact Host/channel/attempt/fence；不重新消费 producer。
    pub fn begin_native_resource_rebind_retired(
        &mut self,
        ready: RetiredInstalledChannel,
        channel: Arc<RunningChannel>,
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<(), NativeHostError> {
        if self.closing || self.unknown || !Arc::ptr_eq(&self.retirement_identity, &ready.owner) {
            return Err(NativeHostError::Authority);
        }
        let token = ChannelRetirementToken {
            owner: ready.owner.clone(),
            index: ready.index,
        };
        if self.observe_channel_retirement(&token) != ChannelRetirement::Retired {
            return Err(NativeHostError::Unknown);
        }
        let entry = &self.channel_retirements[ready.index];
        if !entry.readiness_issued
            || entry.readiness_consumed
            || channel.authority().scope() != entry.authority.scope()
        {
            return Err(NativeHostError::Rejected);
        }
        let latest = self
            .attempts
            .iter()
            .rfind(|a| a.authority.scope() == entry.authority.scope())
            .ok_or(NativeHostError::Rejected)?;
        if latest.id != ready.attempt
            || !latest.authority.same_channel(&entry.authority)
            || !matches!(latest.state,InstallState::Installed{fence,..} if fence==ready.fence)
        {
            return Err(NativeHostError::Rejected);
        }
        // 任何后续失败都不返还该事实；调用者必须关闭失败的新代并继续保管原任务。
        self.channel_retirements[ready.index].readiness_consumed = true;
        self.begin_native_resource_rebind(channel, authorize)
    }
}
