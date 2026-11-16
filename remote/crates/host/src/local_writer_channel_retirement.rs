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
pub(super) struct Entry {
    authority: BoundDataAuthority,
    attempts: Vec<InstallAttempt>,
    unknown: bool,
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
