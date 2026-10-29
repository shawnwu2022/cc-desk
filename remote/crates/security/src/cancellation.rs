use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone)]
pub struct CancelHandle {
    state: Arc<AtomicBool>,
    notify: tokio::sync::watch::Sender<()>,
}
#[derive(Clone)]
pub struct CancelToken {
    state: Arc<AtomicBool>,
    notify: tokio::sync::watch::Receiver<()>,
}
pub fn cancellation() -> (CancelHandle, CancelToken) {
    let state = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::watch::channel(());
    (
        CancelHandle {
            state: state.clone(),
            notify: tx,
        },
        CancelToken { state, notify: rx },
    )
}
impl CancelHandle {
    pub fn cancel(&self) {
        self.state.store(true, Ordering::Release);
        // Every returned call publishes a wake, even if another caller set the bit.
        let _ = self.notify.send(());
    }
}
impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) || self.notify.has_changed().is_err()
    }
    pub(crate) async fn cancelled(&mut self) {
        if !self.is_cancelled() {
            let _ = self.notify.changed().await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn clones_and_repeated_cancel_cannot_reopen() {
        let (handle, mut token) = cancellation();
        let other_handle = handle.clone();
        let before = token.clone();
        assert!(!token.is_cancelled());
        handle.cancel();
        other_handle.cancel();
        let after = token.clone();
        assert!(token.is_cancelled() && before.is_cancelled() && after.is_cancelled());
        token.cancelled().await;
    }
    #[tokio::test]
    async fn closing_all_handles_is_cancellation() {
        let (handle, mut token) = cancellation();
        let other = handle.clone();
        drop(handle);
        assert!(!token.is_cancelled());
        drop(other);
        assert!(token.is_cancelled());
        token.cancelled().await;
    }
    #[tokio::test]
    async fn every_returned_cancel_publishes_wake() {
        let (handle, mut token) = cancellation();
        let second = handle.clone();
        let pending = token.cancelled();
        tokio::pin!(pending);
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        // Exact valid instruction boundary: A set the sticky bit but has not sent.
        // This is deterministic state injection, not a scheduling stress claim.
        handle.state.store(true, Ordering::Release);
        second.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), pending)
                .await
                .is_ok(),
            "returned cancellation must wake an already parked token"
        );
    }
}
