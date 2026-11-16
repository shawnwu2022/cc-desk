//! Original native receiver custody, not a second protocol router or executor.
//! Only private Host pending.wait(Duration::ZERO) closures may be registered.
use super::*;
use std::sync::atomic::AtomicUsize;
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
pub(in crate::local_writer::owner) struct Budget(AtomicUsize);
impl Budget {
    pub(in crate::local_writer::owner) fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
    fn take(&self) -> Result<(), Reject> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < LIMIT).then_some(used + 1)
            })
            .map(|_| ())
            .map_err(|_| Reject::Full)
    }
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
    budget: Arc<Budget>,
}
impl<T> Drop for Cell<T> {
    fn drop(&mut self) {
        // 共享额度覆盖旧 Wait 持有的未消费结果；析构不获取 Owner 锁。
        self.budget.0.fetch_sub(1, Ordering::AcqRel);
    }
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
    budget: Arc<Budget>,
}
impl Owner {
    pub(in crate::local_writer::owner) fn new() -> Self {
        Self::with_budget(Arc::new(Budget::new()))
    }
    pub(in crate::local_writer::owner) fn with_budget(budget: Arc<Budget>) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            sealed: AtomicBool::new(false),
            budget,
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
    pub(in crate::local_writer::owner) fn prune_drained(&self) -> Result<(), Reject> {
        // 只删除已明确终局的索引；原 Wait 继续持有同一 Cell 和未取结果。
        let snapshot = self
            .entries
            .try_lock()
            .map_err(|_| Reject::Pending)?
            .clone();
        let finished: Vec<_> = snapshot
            .iter()
            .filter(|entry| entry.observe() == Observation::Drained)
            .cloned()
            .collect();
        let mut entries = self.entries.try_lock().map_err(|_| Reject::Pending)?;
        entries.retain(|entry| !finished.iter().any(|old| Arc::ptr_eq(entry, old)));
        Ok(())
    }
    pub(in crate::local_writer::owner) fn reserve<T: Send + 'static>(
        &self,
    ) -> Result<Reservation<T>, Reject> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        self.prune_drained()?;
        let mut entries = self.entries.try_lock().map_err(|_| Reject::Pending)?;
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        if entries.len() >= LIMIT {
            return Err(Reject::Full);
        }
        self.budget.take()?;
        let cell = Arc::new(Cell {
            budget: self.budget.clone(),
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
