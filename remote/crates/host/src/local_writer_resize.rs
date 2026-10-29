//! Internal fake-only Resize candidate. No native master implementation or route.
use super::*;
use cc_desk_remote_record::lifecycle::{Request as Operation, State as Epochs};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Size {
    pub(super) rows: u16,
    pub(super) columns: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    KnownNotApplied,
    Unknown,
}
/// Only deterministic, bounded fake implementations are supplied in this slice.
pub(super) trait Backend: Send {
    fn resize(&mut self, size: Size) -> Result<Size, Fault>;
}
pub(super) struct State {
    backend: Box<dyn Backend>,
    records: Vec<(Operation, Arc<view_gate::PeerScope>, Result<Applied, Error>)>,
    floor: u64,
}
impl State {
    #[cfg(test)]
    pub(super) fn fake(backend: impl Backend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            records: Vec::new(),
            floor: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Applied {
    pub(super) request: Operation,
    pub(super) after: Epochs,
    pub(super) size: Size,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Rejected(Reject),
    Conflict,
    NotApplied,
    Uncertain,
}
pub(super) struct Request {
    operation: Operation,
    scope: Arc<view_gate::PeerScope>,
    end: Instant,
    cancelled: Arc<AtomicBool>,
    observed: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Result<Applied, Error>>,
    _credit: Arc<Credit>,
}
pub(super) struct Pending {
    end: Instant,
    cancelled: Arc<AtomicBool>,
    sealed: Arc<AtomicBool>,
    observed: Arc<AtomicBool>,
    reply: mpsc::Receiver<Result<Applied, Error>>,
    _credit: Arc<Credit>,
}
impl Pending {
    pub(super) fn wait(&self) -> Result<Result<Applied, Error>, WaitError> {
        let result = self
            .reply
            .recv_timeout(self.end.saturating_duration_since(Instant::now()));
        if result.is_err() || Instant::now() >= self.end {
            self.cancelled.store(true, Ordering::Release);
            self.sealed.store(true, Ordering::Release);
            return Err(WaitError::Uncertain);
        }
        self.observed.store(true, Ordering::Release);
        result.map_err(|_| WaitError::Uncertain)
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.observed.load(Ordering::Acquire) {
            self.cancelled.store(true, Ordering::Release);
            self.sealed.store(true, Ordering::Release);
        }
    }
}
pub(super) fn enqueue(
    input: &LocalInputHandle,
    scope: Arc<view_gate::PeerScope>,
    operation: Operation,
    budget: Duration,
) -> Result<Pending, Reject> {
    if budget.is_zero() || budget > Duration::from_secs(1) {
        return Err(Reject::Bounds);
    }
    let end = Instant::now() + budget;
    let credit = input.credits.reserve()?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(1);
    input
        .tx
        .try_send(Command::Resize(Request {
            operation,
            scope,
            end,
            cancelled: cancelled.clone(),
            observed: observed.clone(),
            reply: tx,
            _credit: credit.clone(),
        }))
        .map_err(|e| match e {
            mpsc::TrySendError::Full(_) => Reject::Full,
            mpsc::TrySendError::Disconnected(_) => Reject::Closed,
        })?;
    Ok(Pending {
        end,
        cancelled,
        sealed: input.sealed.clone(),
        observed,
        reply: rx,
        _credit: credit,
    })
}
impl Request {
    pub(super) fn reject(self, error: Reject) {
        let _ = self.reply.try_send(Err(Error::Rejected(error)));
    }
    pub(super) fn process(
        self,
        state: Option<&mut State>,
        replies: &mut Option<ReplyState>,
        gate: &mut Option<view_gate::ViewGate>,
        sealed: &AtomicBool,
    ) {
        let result = (|| {
            let state = state.ok_or(Error::Rejected(Reject::Unsupported))?;
            let replies = replies
                .as_mut()
                .ok_or(Error::Rejected(Reject::Unsupported))?;
            let gate = gate.as_mut().ok_or(Error::Rejected(Reject::Unsupported))?;
            gate.resize_ready().map_err(Error::Rejected)?;
            let epochs = replies.epochs().ok_or(Error::Rejected(Reject::Sync))?;
            let current = gate
                .resize_state(&self.scope, epochs)
                .map_err(Error::Rejected)?;
            if let Some((old, scope, result)) = state.records.iter().find(|(r, _, _)| {
                r.sequence == self.operation.sequence || r.operation == self.operation.operation
            }) {
                return if old == &self.operation && Arc::ptr_eq(scope, &self.scope) {
                    // Live authority checked above; historical facts never grant input.
                    result.clone()
                } else {
                    Err(Error::Conflict)
                };
            }
            if self.operation.kind != cc_desk_remote_record::lifecycle::Kind::Resize
                || cc_desk_remote_record::lifecycle::encode(&self.operation).is_err()
            {
                return Err(Error::Rejected(Reject::Bounds));
            }
            if self.operation.state != current || self.operation.sequence <= state.floor {
                return Err(Error::Rejected(Reject::Epoch));
            }
            if state.records.len() == 64 {
                return Err(Error::Rejected(Reject::Full));
            }
            let (rows, columns) = self.operation.size.unwrap();
            if usize::from(rows) > cc_desk_remote_terminal::MAX_TERMINAL_ROWS
                || usize::from(columns) > cc_desk_remote_terminal::MAX_TERMINAL_COLUMNS
                || usize::from(columns) < cc_desk_remote_terminal::MIN_TERMINAL_COLUMNS
            {
                return Err(Error::Rejected(Reject::Unsupported));
            }
            let size = Size { rows, columns };
            let (changed, after_epochs) = replies.resize_preflight(rows, columns).map_err(|e| {
                if e == Reject::Epoch {
                    gate.invalidate_for_resize();
                    sealed.store(true, Ordering::Release);
                }
                Error::Rejected(e)
            })?;
            // No native API is supplied. This synchronous call is for bounded fakes only.
            // Reserve uncertainty before any effect; cache/floor never reset on resize.
            let index = state.records.len();
            state.floor = self.operation.sequence;
            state.records.push((
                self.operation.clone(),
                self.scope.clone(),
                Err(Error::Uncertain),
            ));
            gate.invalidate_for_resize();
            gate.resize_feedback = Some((self.end, self.observed.clone()));
            if sealed.load(Ordering::Acquire)
                || self.cancelled.load(Ordering::Acquire)
                || Instant::now() >= self.end
            {
                sealed.store(true, Ordering::Release);
                return Err(Error::Uncertain);
            }
            // Revalidate after invalidation, immediately before the possible effect.
            if gate.resize_state(&self.scope, epochs).is_err() {
                let result = Err(Error::Rejected(Reject::Policy));
                state.records[index].2 = result.clone();
                return result;
            }
            if sealed.load(Ordering::Acquire)
                || self.cancelled.load(Ordering::Acquire)
                || Instant::now() >= self.end
            {
                sealed.store(true, Ordering::Release);
                return Err(Error::Uncertain);
            }
            let effect = if changed {
                state.backend.resize(size)
            } else {
                Ok(size)
            };
            let result = match effect {
                Ok(actual) if actual == size => {
                    // Truth maintenance still runs if authority expires during the effect.
                    if replies.resize_commit(rows, columns).is_err()
                        || replies.epochs() != Some(after_epochs)
                    {
                        Err(Error::Uncertain)
                    } else {
                        let after = Epochs {
                            dimension: after_epochs.dimension_epoch,
                            ..current
                        };
                        Ok(Applied {
                            request: self.operation.clone(),
                            after,
                            size,
                        })
                    }
                }
                Err(Fault::KnownNotApplied) => Err(Error::NotApplied),
                _ => Err(Error::Uncertain),
            };
            state.records[index].2 = result.clone();
            if result == Err(Error::Uncertain)
                || sealed.load(Ordering::Acquire)
                || self.cancelled.load(Ordering::Acquire)
                || Instant::now() >= self.end
            {
                sealed.store(true, Ordering::Release);
                return Err(Error::Uncertain);
            }
            gate.resize_state(&self.scope, after_epochs)
                .map_err(Error::Rejected)?;
            if sealed.load(Ordering::Acquire)
                || self.cancelled.load(Ordering::Acquire)
                || Instant::now() >= self.end
            {
                sealed.store(true, Ordering::Release);
                return Err(Error::Uncertain);
            }
            result
        })();
        // No success response can install/ACK a snapshot or issue a window.
        if self.reply.try_send(result).is_err() {
            sealed.store(true, Ordering::Release);
        }
    }
}
