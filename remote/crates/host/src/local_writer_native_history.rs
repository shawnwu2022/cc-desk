//! 仅回收已消费退休凭据且有成功后继的旧历史；不改变授权或安全容量。
use super::*;
use std::future::Future;
use std::task::{Context, Poll, Waker};
pub(super) fn poll_channel_join(channel: &RunningChannel) -> Option<bool> {
    // 原等待持 Tokio 槽锁跨 join；Pending future 析构只放锁，不取原 handle。
    let wait = channel.wait_closed_checked();
    let mut wait = std::pin::pin!(wait);
    let waker = Waker::noop();
    match wait.as_mut().poll(&mut Context::from_waker(waker)) {
        Poll::Pending => None,
        Poll::Ready(result) => Some(
            matches!(result,Ok(reason) if reason!=cc_desk_remote_security::DataError::Uncertain),
        ),
    }
}
impl NativeHost {
    pub(super) fn prune_retired_history(&mut self) {
        if self.closing || self.unknown {
            return;
        }
        let mut remove = Vec::new();
        let mut ids = Vec::new();
        let mut authorities = Vec::new();
        for entry in &self.channel_retirements {
            // pin 只作外部持有背压；retired/原 join 才是完成事实。
            if entry.unknown
                || !entry.retired
                || !entry.readiness_consumed
                || Arc::strong_count(&entry.pin) != 1
            {
                continue;
            }
            let Some(latest) = self
                .attempts
                .iter()
                .rfind(|a| a.authority.scope() == entry.authority.scope())
            else {
                continue;
            };
            if entry.attempts.contains(&latest.id)
                || !matches!(latest.state, InstallState::Installed { .. })
                || !self
                    .peer_installs
                    .iter()
                    .any(|job| job.completed_attempt(latest.id))
            {
                continue;
            }
            let old: Vec<_> = self
                .attempts
                .iter()
                .filter(|a| entry.attempts.contains(&a.id))
                .collect();
            if old.is_empty() || old.len() != entry.attempts.len() {
                continue;
            }
            if old.iter().any(|a| {
                !matches!(
                    a.state,
                    InstallState::Installed { .. } | InstallState::Rejected
                ) || Arc::strong_count(&a.pending) != 1
                    || !a.pending.history_reclaimable()
            }) {
                continue;
            }
            let same = |a: &BoundDataAuthority| {
                a.same_channel(&entry.authority) && a.grant_id() == entry.authority.grant_id()
            };
            if self
                .peer_installs
                .iter()
                .any(|job| same(&job.channel().authority()) && !job.reclaimable())
            {
                continue;
            }
            let mut pending = false;
            for attempt in &old {
                match poll_channel_join(&attempt.channel) {
                    Some(true) => {}
                    None => pending = true,
                    Some(false) => {
                        self.unknown = true;
                        return;
                    }
                }
            }
            if pending {
                continue;
            }
            remove.push(entry.id);
            ids.extend(entry.attempts.iter().copied());
            authorities.push(entry.authority.clone());
        }
        // 保留最新精确 scope/fence；旧数字身份永不复用，未知状态永不进入此列表。
        self.attempts.retain(|a| !ids.contains(&a.id));
        self.peer_installs.retain(|job| {
            !authorities.iter().any(|a| {
                let original = job.channel().authority();
                original.same_channel(a) && original.grant_id() == a.grant_id()
            })
        });
        self.channel_retirements
            .retain(|entry| !remove.contains(&entry.id));
    }
}
