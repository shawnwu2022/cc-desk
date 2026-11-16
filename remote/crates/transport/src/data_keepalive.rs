use crate::TransportError;
use std::sync::{Arc, Mutex};
use tokio::time::Instant;
const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
#[derive(Clone, Copy)]
enum State {
    Dormant,
    Active(Instant),
    Closed,
}
pub(crate) struct Policy {
    state: Mutex<State>,
    setup_end: Instant,
    max_end: Instant,
    pub(crate) tunnel: [u8; 16],
    changed: tokio::sync::Notify,
}
// 仅原 transport 实例的一次保活开关，不证明认证、角色或 Installed。
pub struct DataKeepaliveActivation {
    policy: Arc<Policy>,
}
impl DataKeepaliveActivation {
    pub fn activate(self, end: Instant) -> Result<(), TransportError> {
        let mut state = self
            .policy
            .state
            .lock()
            .map_err(|_| TransportError::Closed)?;
        let now = Instant::now();
        if !matches!(*state, State::Dormant)
            || now >= self.policy.setup_end
            || end <= now
            || end > self.policy.max_end
        {
            return Err(TransportError::Limit);
        }
        *state = State::Active(end);
        self.policy.changed.notify_one();
        Ok(())
    }
}
impl Policy {
    pub(crate) fn new(
        setup_end: Instant,
        max_end: Instant,
        tunnel: [u8; 16],
        budget: std::time::Duration,
    ) -> Result<(Arc<Self>, DataKeepaliveActivation), TransportError> {
        let now = Instant::now();
        if budget < std::time::Duration::from_secs(2)
            || setup_end <= now
            || setup_end > max_end
            || max_end.duration_since(now) > std::time::Duration::from_secs(900)
        {
            return Err(TransportError::Limit);
        }
        let policy = Arc::new(Self {
            state: Mutex::new(State::Dormant),
            setup_end,
            max_end,
            tunnel,
            changed: tokio::sync::Notify::new(),
        });
        Ok((policy.clone(), DataKeepaliveActivation { policy }))
    }
    pub(crate) fn end(&self) -> Instant {
        match self.state.lock() {
            Ok(state) => match *state {
                State::Dormant => self.setup_end,
                State::Active(end) => end,
                State::Closed => Instant::now(),
            },
            Err(_) => Instant::now(),
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|s| matches!(*s, State::Active(_)))
    }
    pub(crate) fn interval(&self) -> std::time::Duration {
        INTERVAL
    }
    pub(crate) async fn changed(&self) {
        self.changed.notified().await;
    }
    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = State::Closed;
        }
        self.changed.notify_one();
    }
}
pub(crate) struct CloseOnExit(pub(crate) Option<Arc<Policy>>);
impl Drop for CloseOnExit {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            p.close();
        }
    }
}
