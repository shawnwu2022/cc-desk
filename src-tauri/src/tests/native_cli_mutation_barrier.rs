//! One-shot observation after RunAccess admission, before the real PTY lock.
use super::OwnedPty;
use std::sync::mpsc;
use std::time::Duration;

pub(crate) struct Hook {
    entered: mpsc::SyncSender<bool>,
    release: mpsc::Receiver<()>,
}

pub(crate) struct Pause<'a> {
    pty: &'a OwnedPty,
    entered: mpsc::Receiver<bool>,
    release: mpsc::SyncSender<()>,
}

pub(crate) fn pause_next(pty: &OwnedPty) -> Pause<'_> {
    let (entered, receive) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    assert!(pty
        .mutation_barrier
        .lock()
        .replace(Hook {
            entered,
            release: wait
        })
        .is_none());
    Pause {
        pty,
        entered: receive,
        release,
    }
}

impl Pause<'_> {
    /// Also observes that the original mutation mutex is actually held.
    pub(crate) fn entered_while_locked(&self) {
        assert!(self.entered.recv_timeout(Duration::from_secs(10)).unwrap());
    }

    pub(crate) fn release(&self) {
        let _ = self.release.try_send(());
    }
}

impl Drop for Pause<'_> {
    fn drop(&mut self) {
        self.release();
        self.pty.mutation_barrier.lock().take();
    }
}

pub(crate) fn before_lock(pty: &OwnedPty) {
    let hook = pty.mutation_barrier.lock().take();
    if let Some(hook) = hook {
        let _ = hook.entered.send(pty.mutation.try_lock().is_none());
        // Disconnection also releases this owned fixture; never block forever.
        let _ = hook.release.recv_timeout(Duration::from_secs(10));
    }
}
