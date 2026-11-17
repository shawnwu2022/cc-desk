//! 单次原连接的等待阶段；不代表安装、授权或资源退休。
use super::ConnectError;
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Created,
    Connecting,
    PendingReady,
    Ready,
    Failed,
    Consumed,
}

pub struct RendezvousWait {
    end: Instant,
    brand: Arc<()>,
    phase: Mutex<Phase>,
}

pub struct ExpiredRendezvousWait {
    brand: Arc<()>,
    end: Instant,
}
impl ExpiredRendezvousWait {
    pub fn matches(&self, original: &RendezvousWait) -> bool {
        Arc::ptr_eq(&self.brand, &original.brand) && self.end == original.end
    }
}

impl RendezvousWait {
    pub fn new(end: Instant) -> Self {
        Self {
            end,
            brand: Arc::new(()),
            phase: Mutex::new(Phase::Created),
        }
    }
    pub(super) fn begin(&self, end: Instant) -> Result<Instant, ConnectError> {
        let mut phase = self.phase.lock().map_err(|_| ConnectError::Admission)?;
        if *phase != Phase::Created || self.end > end || Instant::now() >= self.end {
            *phase = Phase::Failed;
            return Err(ConnectError::Admission);
        }
        *phase = Phase::Connecting;
        Ok(self.end)
    }
    pub(super) fn pending(&self) -> Result<(), ConnectError> {
        let mut phase = self.phase.lock().map_err(|_| ConnectError::Admission)?;
        if *phase != Phase::Connecting || Instant::now() >= self.end {
            *phase = Phase::Failed;
            return Err(ConnectError::Deadline);
        }
        *phase = Phase::PendingReady;
        Ok(())
    }
    pub(super) fn ready(&self) -> Result<(), ConnectError> {
        let mut phase = self.phase.lock().map_err(|_| ConnectError::Admission)?;
        if *phase != Phase::PendingReady {
            return Err(ConnectError::Admission);
        }
        if Instant::now() >= self.end {
            return Err(ConnectError::Deadline);
        }
        *phase = Phase::Ready;
        Ok(())
    }
    pub(super) fn failed(&self) {
        if let Ok(mut phase) = self.phase.lock() {
            *phase = Phase::Failed;
        }
    }
    // 调用方仍必须证明原 future/网络/材料已退出、无 handoff，才能转换为本次退休事实。
    pub fn take_expired(&self) -> Option<ExpiredRendezvousWait> {
        let mut phase = self.phase.lock().ok()?;
        if *phase != Phase::PendingReady || Instant::now() < self.end {
            return None;
        }
        *phase = Phase::Consumed;
        Some(ExpiredRendezvousWait {
            brand: self.brand.clone(),
            end: self.end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test(start_paused = true)]
    async fn expired_observation_is_bound_to_original_instance_and_end() {
        let end = Instant::now() + Duration::from_secs(8);
        let original = RendezvousWait::new(end);
        let foreign = RendezvousWait::new(end);
        original.begin(end).unwrap();
        original.pending().unwrap();
        tokio::time::advance(Duration::from_secs(8)).await;
        let expired = original.take_expired().unwrap();
        assert!(expired.matches(&original));
        assert!(!expired.matches(&foreign));
    }
    #[tokio::test(start_paused = true)]
    async fn pending_deadline_is_once_only() {
        let end = Instant::now() + Duration::from_secs(8);
        let wait = RendezvousWait::new(end);
        wait.begin(end).unwrap();
        wait.pending().unwrap();
        assert!(wait.take_expired().is_none());
        tokio::time::advance(Duration::from_secs(8)).await;
        assert!(wait.take_expired().is_some());
        assert!(wait.take_expired().is_none());
    }
    #[tokio::test(start_paused = true)]
    async fn observed_failure_and_ready_never_become_timeout() {
        for ready in [false, true] {
            let end = Instant::now() + Duration::from_secs(8);
            let wait = RendezvousWait::new(end);
            wait.begin(end).unwrap();
            wait.pending().unwrap();
            if ready {
                wait.ready().unwrap();
            } else {
                wait.failed();
            }
            tokio::time::advance(Duration::from_secs(8)).await;
            assert!(wait.take_expired().is_none());
        }
    }
    #[tokio::test(start_paused = true)]
    async fn pre_admission_or_reused_observer_cannot_issue_wait() {
        for reuse in [false, true] {
            let end = Instant::now() + Duration::from_secs(8);
            let wait = RendezvousWait::new(end);
            wait.begin(end).unwrap();
            if reuse {
                wait.pending().unwrap();
                assert!(wait.begin(end).is_err());
            }
            tokio::time::advance(Duration::from_secs(8)).await;
            assert!(wait.take_expired().is_none());
        }
    }
    #[tokio::test(start_paused = true)]
    async fn late_ready_is_not_delivered() {
        let end = Instant::now() + Duration::from_secs(8);
        let wait = RendezvousWait::new(end);
        wait.begin(end).unwrap();
        wait.pending().unwrap();
        tokio::time::advance(Duration::from_secs(8)).await;
        assert_eq!(wait.ready(), Err(ConnectError::Deadline));
        assert!(wait.take_expired().is_some());
    }
}
