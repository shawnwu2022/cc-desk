//! Run-scoped bounded terminal output credit and cumulative parser acknowledgments.
//! Channel acceptance is delivery only; renderer ACK is the consumption boundary.
#![allow(dead_code)] // D15 installs the reader/supervisor that consumes this transport.

use super::profiles::error;
use super::run_registry::RunKey;
use super::types::{SafeError, WireU64};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024;
pub(crate) const RUN_HIGH_WATERMARK: usize = 256 * 1024;
pub(crate) const RUN_LOW_WATERMARK: usize = 64 * 1024;
pub(crate) const APPLICATION_PAYLOAD_BUDGET: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OutputFrame {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) stream_epoch: WireU64,
    /// Inclusive start offset of this frame in the raw PTY byte stream.
    pub(crate) offset: WireU64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OutputAck {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) stream_epoch: WireU64,
    /// Exclusive cumulative byte offset parsed by xterm.
    pub(crate) through_offset: WireU64,
}

#[derive(Debug)]
struct BudgetState {
    used: usize,
}

#[derive(Debug)]
struct BudgetInner {
    capacity: usize,
    state: Mutex<BudgetState>,
}

#[derive(Debug, Clone)]
pub(crate) struct TransportBudget {
    inner: Arc<BudgetInner>,
}

impl TransportBudget {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(BudgetInner {
                capacity,
                state: Mutex::new(BudgetState { used: 0 }),
            }),
        }
    }

    pub(crate) fn reserved_bytes(&self) -> usize {
        self.inner.state.lock().used
    }

    fn try_take(&self, bytes: usize) -> Result<(), SafeError> {
        let mut state = self.inner.state.lock();
        let next = state
            .used
            .checked_add(bytes)
            .ok_or_else(|| error("OUTPUT_BUDGET_EXHAUSTED"))?;
        if next > self.inner.capacity {
            return Err(error("OUTPUT_BUDGET_EXHAUSTED"));
        }
        state.used = next;
        Ok(())
    }

    fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let mut state = self.inner.state.lock();
        debug_assert!(state.used >= bytes);
        state.used = state.used.saturating_sub(bytes);
    }
}

#[derive(Debug)]
struct FlowState {
    reserved: usize,
    outstanding: usize,
    sent_offset: u64,
    acked_offset: u64,
    frame_ends: BTreeSet<u64>,
    paused: bool,
    degraded: bool,
}

#[derive(Debug)]
struct FlowInner {
    run: RunKey,
    epoch: WireU64,
    budget: TransportBudget,
    state: Mutex<FlowState>,
}

impl Drop for FlowInner {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        self.budget
            .release(state.reserved.saturating_add(state.outstanding));
        state.reserved = 0;
        state.outstanding = 0;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RunOutputFlow {
    inner: Arc<FlowInner>,
}

impl RunOutputFlow {
    pub(crate) fn new(run: RunKey, epoch: WireU64, budget: TransportBudget) -> Self {
        Self {
            inner: Arc::new(FlowInner {
                run,
                epoch,
                budget,
                state: Mutex::new(FlowState {
                    reserved: 0,
                    outstanding: 0,
                    sent_offset: 0,
                    acked_offset: 0,
                    frame_ends: BTreeSet::new(),
                    paused: false,
                    degraded: false,
                }),
            }),
        }
    }

    /// Reserve a complete maximum-sized read before touching the PTY reader.
    /// D15 may wait/retry this operation; D14 deliberately exposes no discard path.
    pub(crate) fn try_reserve(&self) -> Result<OutputPermit, SafeError> {
        let mut state = self.inner.state.lock();
        if state.degraded {
            return Err(error("OUTPUT_DEGRADED"));
        }
        if state.paused {
            return Err(error("OUTPUT_BACKPRESSURE"));
        }
        let run_used = state
            .reserved
            .checked_add(state.outstanding)
            .and_then(|value| value.checked_add(MAX_FRAME_BYTES))
            .ok_or_else(|| error("OUTPUT_BACKPRESSURE"))?;
        if run_used > RUN_HIGH_WATERMARK {
            state.paused = true;
            return Err(error("OUTPUT_BACKPRESSURE"));
        }

        // Lock order is always flow -> application budget throughout this module.
        self.inner.budget.try_take(MAX_FRAME_BYTES)?;
        state.reserved += MAX_FRAME_BYTES;
        Ok(OutputPermit {
            inner: self.inner.clone(),
            reservation: MAX_FRAME_BYTES,
            committed: false,
        })
    }

    pub(crate) fn ack(&self, ack: &OutputAck) -> Result<(), SafeError> {
        if ack.run_id != self.inner.run.run_id {
            return Err(error("FORBIDDEN"));
        }
        if ack.generation != self.inner.run.generation {
            return Err(error("STALE_GENERATION"));
        }
        if ack.stream_epoch != self.inner.epoch {
            return Err(error("STALE_STREAM_EPOCH"));
        }

        let through = ack.through_offset.get();
        let mut state = self.inner.state.lock();
        if state.degraded {
            return Err(error("OUTPUT_DEGRADED"));
        }
        if through == state.acked_offset {
            return Ok(());
        }
        if through < state.acked_offset {
            return Err(error("ACK_REGRESSION"));
        }
        if through > state.sent_offset {
            return Err(error("ACK_OUT_OF_RANGE"));
        }
        if !state.frame_ends.contains(&through) {
            return Err(error("ACK_NOT_FRAME_BOUNDARY"));
        }

        let released_u64 = through - state.acked_offset;
        let released = usize::try_from(released_u64).map_err(|_| error("ACK_OUT_OF_RANGE"))?;
        if released > state.outstanding {
            return Err(error("ACK_OUT_OF_RANGE"));
        }
        state.acked_offset = through;
        state.outstanding -= released;
        state.frame_ends.retain(|end| *end > through);
        self.inner.budget.release(released);
        if state.paused && state.reserved.saturating_add(state.outstanding) <= RUN_LOW_WATERMARK {
            state.paused = false;
        }
        Ok(())
    }

    /// Permanent renderer/channel loss is terminal for this stream epoch.
    /// Already accepted bytes are not replayed and all payload credit is released.
    pub(crate) fn degrade(&self) {
        let mut state = self.inner.state.lock();
        if state.degraded {
            return;
        }
        state.degraded = true;
        state.paused = false;
        let released = state.outstanding;
        state.outstanding = 0;
        state.frame_ends.clear();
        self.inner.budget.release(released);
    }

    pub(crate) fn sent_offset(&self) -> u64 {
        self.inner.state.lock().sent_offset
    }

    pub(crate) fn outstanding_bytes(&self) -> usize {
        self.inner.state.lock().outstanding
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.inner.state.lock().paused
    }

    pub(crate) fn is_degraded(&self) -> bool {
        self.inner.state.lock().degraded
    }

    pub(crate) fn run(&self) -> &RunKey {
        &self.inner.run
    }

    pub(crate) fn stream_epoch(&self) -> WireU64 {
        self.inner.epoch
    }
}

#[derive(Debug)]
pub(crate) struct OutputPermit {
    inner: Arc<FlowInner>,
    reservation: usize,
    committed: bool,
}

impl OutputPermit {
    pub(crate) fn commit(mut self, bytes: Vec<u8>) -> Result<OutputFrame, SafeError> {
        if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
            return Err(SafeError::invalid("bytes"));
        }

        let mut state = self.inner.state.lock();
        if state.degraded {
            return Err(error("OUTPUT_DEGRADED"));
        }
        if state.reserved < self.reservation {
            return Err(error("OUTPUT_STATE_INVALID"));
        }

        let start = state.sent_offset;
        let end = start
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| error("OUTPUT_OFFSET_EXHAUSTED"))?;
        state.reserved -= self.reservation;
        state.outstanding = state
            .outstanding
            .checked_add(bytes.len())
            .ok_or_else(|| error("OUTPUT_STATE_INVALID"))?;
        if state.reserved.saturating_add(state.outstanding) >= RUN_HIGH_WATERMARK {
            state.paused = true;
        }
        state.sent_offset = end;
        state.frame_ends.insert(end);

        // The maximum reservation already counted against the application budget.
        self.inner
            .budget
            .release(self.reservation.saturating_sub(bytes.len()));
        self.committed = true;

        Ok(OutputFrame {
            run_id: self.inner.run.run_id.clone(),
            generation: self.inner.run.generation,
            stream_epoch: self.inner.epoch,
            offset: WireU64::parse(&start.to_string())?,
            bytes,
        })
    }
}

impl Drop for OutputPermit {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut state = self.inner.state.lock();
        if state.reserved >= self.reservation {
            state.reserved -= self.reservation;
            self.inner.budget.release(self.reservation);
        }
    }
}
