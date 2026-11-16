//! Original native receiver custody, not a second protocol router or executor.
//! Only private Host pending.wait(Duration::ZERO) closures may be registered.
use super::*;
use std::sync::{OnceLock, TryLockError};
const LIMIT: usize = 64;
const POLL: Duration = Duration::from_millis(2);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::local_writer::owner) enum Observation {
    Pending,
    Drained,
    Unknown,
}
trait Observe: Send + Sync {
    fn observe(&self) -> Observation;
}
type Poll<T> = Box<dyn FnMut(Duration) -> Result<Result<T, Reject>, WaitError> + Send>;
enum State<T> {
    Waiting(Poll<T>),
    Ready(Option<Result<T, Reject>>),
    Unknown,
}
struct Cell<T> {
    // Set once synchronously, before exposing the waiting future. No callback
    // can replace/rebind this receiver to another operation or owner.
    pending: OnceLock<Mutex<State<T>>>,
    abandoned: AtomicBool,
}
impl<T: Send + 'static> Observe for Cell<T> {
    fn observe(&self) -> Observation {
        let Some(pending) = self.pending.get() else {
            return if self.abandoned.load(Ordering::Acquire) {
                Observation::Drained
            } else {
                Observation::Pending
            };
        };
        let mut state = match pending.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Observation::Pending,
            Err(TryLockError::Poisoned(_)) => return Observation::Unknown,
        };
        if let State::Waiting(poll) = &mut *state {
            match poll(Duration::ZERO) {
                Ok(value) => *state = State::Ready(Some(value)),
                Err(WaitError::Uncertain) => return Observation::Pending,
                Err(WaitError::Closed) => *state = State::Unknown,
            }
        }
        match &*state {
            State::Ready(_) => Observation::Drained,
            State::Unknown => Observation::Unknown,
            State::Waiting(_) => Observation::Pending,
        }
    }
}
pub(in crate::local_writer::owner) struct Owner {
    entries: Mutex<Vec<Arc<dyn Observe>>>,
    sealed: AtomicBool,
}
impl Owner {
    pub(in crate::local_writer::owner) fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            sealed: AtomicBool::new(false),
        }
    }
    pub(in crate::local_writer::owner) fn seal(&self) {
        self.sealed.store(true, Ordering::Release);
    }
    pub(in crate::local_writer::owner) fn observe(&self) -> Observation {
        let entries = match self.entries.try_lock() {
            Ok(entries) => entries.clone(),
            Err(TryLockError::WouldBlock) => return Observation::Pending,
            Err(TryLockError::Poisoned(_)) => return Observation::Unknown,
        };
        let mut pending = false;
        let mut unknown = false;
        for entry in entries {
            match entry.observe() {
                Observation::Unknown => unknown = true,
                Observation::Pending => pending = true,
                Observation::Drained => (),
            }
        }
        if pending {
            Observation::Pending
        } else if unknown {
            Observation::Unknown
        } else {
            Observation::Drained
        }
    }
    pub(in crate::local_writer::owner) fn reserve<T: Send + 'static>(
        &self,
    ) -> Result<Reservation<T>, Reject> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        // Reap outside the registry lock: no actor receiver is polled under it.
        let entries = self
            .entries
            .try_lock()
            .map_err(|_| Reject::Pending)?
            .clone();
        let mut finished = Vec::new();
        for entry in &entries {
            if entry.observe() == Observation::Drained {
                finished.push(entry.clone());
            }
        }
        let mut entries = self.entries.try_lock().map_err(|_| Reject::Pending)?;
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        entries.retain(|entry| !finished.iter().any(|old| Arc::ptr_eq(entry, old)));
        if entries.len() >= LIMIT {
            return Err(Reject::Full);
        }
        let cell = Arc::new(Cell {
            pending: OnceLock::new(),
            abandoned: AtomicBool::new(false),
        });
        entries.push(cell.clone());
        Ok(Reservation { cell, bound: false })
    }
}
pub(in crate::local_writer::owner) struct Reservation<T> {
    cell: Arc<Cell<T>>,
    bound: bool,
}
impl<T: Send + 'static> Reservation<T> {
    pub(in crate::local_writer::owner) fn bind(
        mut self,
        f: impl FnMut(Duration) -> Result<Result<T, Reject>, WaitError> + Send + 'static,
    ) -> Wait<T> {
        // This consuming, private capability is issued once per fresh Cell.
        // There is no setter or second path that can initialize the OnceLock.
        let set = self
            .cell
            .pending
            .set(Mutex::new(State::Waiting(Box::new(f))));
        assert!(set.is_ok(), "private reservation bound twice");
        self.bound = true;
        Wait {
            cell: self.cell.clone(),
        }
    }
}
impl<T> Drop for Reservation<T> {
    fn drop(&mut self) {
        if !self.bound {
            self.cell.abandoned.store(true, Ordering::Release);
        }
    }
}
pub(in crate::local_writer::owner) struct Wait<T> {
    cell: Arc<Cell<T>>,
}
impl<T: Send + 'static> Wait<T> {
    pub(in crate::local_writer::owner) async fn until(
        self,
        end: tokio::time::Instant,
    ) -> Result<T, Reject> {
        loop {
            if tokio::time::Instant::now() >= end {
                return Err(Reject::Continuity);
            }
            match self.cell.observe() {
                Observation::Unknown => return Err(Reject::Thread),
                Observation::Drained => {
                    let pending = self.cell.pending.get().ok_or(Reject::Continuity)?;
                    match pending.try_lock() {
                        Ok(mut state) => {
                            let result = match &mut *state {
                                State::Ready(value) => value.take().ok_or(Reject::Continuity)?,
                                _ => Err(Reject::Continuity),
                            };
                            if tokio::time::Instant::now() >= end {
                                return Err(Reject::Continuity);
                            }
                            return result;
                        }
                        Err(TryLockError::WouldBlock) => (),
                        Err(TryLockError::Poisoned(_)) => return Err(Reject::Thread),
                    }
                }
                Observation::Pending => (),
            }
            tokio::time::sleep_until(std::cmp::min(end, tokio::time::Instant::now() + POLL)).await;
        }
    }
}
#[cfg(test)]
#[path = "local_writer_connection_pending_tests.rs"]
mod tests;
