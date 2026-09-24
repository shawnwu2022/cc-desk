//! Run-scoped ordered terminal bytes with application credit and parsed acknowledgments.
//! This module owns transport accounting only; D15 owns process exit and drain lifecycle.
#![allow(dead_code)] // D15 wires the producer/pump into the production supervisor.

use crate::cli::output_route::OutputRoute;
use crate::cli::profiles::error;
use crate::cli::run_registry::RunKey;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

pub(crate) const OUTPUT_FRAME_BYTES_MAX: usize = 16 * 1024;
pub(crate) const OUTPUT_RUN_HIGH_WATER: usize = 256 * 1024;
pub(crate) const OUTPUT_RUN_LOW_WATER: usize = 64 * 1024;
pub(crate) const OUTPUT_APP_PAYLOAD_BUDGET: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportLimits {
    frame_bytes: usize,
    run_high_water: usize,
    run_low_water: usize,
    app_payload_budget: usize,
}

impl TransportLimits {
    pub(crate) fn new(
        frame_bytes: usize,
        run_high_water: usize,
        run_low_water: usize,
        app_payload_budget: usize,
    ) -> Result<Self, SafeError> {
        if frame_bytes == 0
            || frame_bytes > run_high_water
            || run_low_water > run_high_water
            || run_high_water > app_payload_budget
        {
            return Err(SafeError::invalid("transportLimits"));
        }
        Ok(Self {
            frame_bytes,
            run_high_water,
            run_low_water,
            app_payload_budget,
        })
    }
}

impl Default for TransportLimits {
    fn default() -> Self {
        Self::new(
            OUTPUT_FRAME_BYTES_MAX,
            OUTPUT_RUN_HIGH_WATER,
            OUTPUT_RUN_LOW_WATER,
            OUTPUT_APP_PAYLOAD_BUDGET,
        )
        .expect("fixed output transport limits are valid")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputFrame {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) stream_epoch: WireU64,
    pub(crate) offset: WireU64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OutputAck {
    run_id: String,
    generation: u32,
    stream_epoch: WireU64,
    through_offset: WireU64,
}

/// Backend-only lifecycle sink. Dispatch acceptance advances sent bytes; only a
/// renderer parser ACK advances parsed bytes. Revocation is terminal for this
/// output stream and must wake blocked credit waiters.
pub(crate) trait OutputProgress: Send + Sync {
    fn sent_through(&self, stream_epoch: WireU64, through: WireU64);
    fn parsed_through(&self, stream_epoch: WireU64, through: WireU64);
    fn degraded(&self, stream_epoch: WireU64);
}

#[derive(Default)]
struct BudgetState {
    used: usize,
    per_stream: HashMap<u64, usize>,
    waiters: VecDeque<u64>,
}

struct PayloadBudget {
    limit: usize,
    state: Mutex<BudgetState>,
    changed: Condvar,
}

impl PayloadBudget {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            state: Mutex::new(BudgetState::default()),
            changed: Condvar::new(),
        }
    }

    fn reserve(
        &self,
        stream: u64,
        amount: usize,
        degraded: &AtomicBool,
    ) -> Result<(), SafeError> {
        if amount == 0 || amount > self.limit {
            return Err(error("OUTPUT_APP_BUDGET"));
        }
        let mut state = self.state.lock();
        let mut queued = false;
        loop {
            if degraded.load(Ordering::SeqCst) {
                if queued {
                    state.waiters.retain(|candidate| *candidate != stream);
                    self.changed.notify_all();
                }
                return Err(error("OUTPUT_STREAM_DEGRADED"));
            }
            let fits = state
                .used
                .checked_add(amount)
                .is_some_and(|used| used <= self.limit);
            let turn = if queued {
                state.waiters.front().is_some_and(|id| *id == stream)
            } else {
                state.waiters.is_empty()
            };
            if fits && turn {
                if queued {
                    let popped = state.waiters.pop_front();
                    debug_assert_eq!(popped, Some(stream));
                }
                state.used += amount;
                *state.per_stream.entry(stream).or_default() += amount;
                self.changed.notify_all();
                return Ok(());
            }
            if !queued {
                state.waiters.push_back(stream);
                queued = true;
            }
            self.changed.wait(&mut state);
        }
    }

    fn release(&self, stream: u64, amount: usize) {
        if amount == 0 {
            return;
        }
        let mut state = self.state.lock();
        let Some(current) = state.per_stream.get_mut(&stream) else {
            // Concurrent revocation may already have released the reservation.
            return;
        };
        debug_assert!(*current >= amount);
        let released = amount.min(*current);
        *current -= released;
        if *current == 0 {
            state.per_stream.remove(&stream);
        }
        state.used = state.used.saturating_sub(released);
        self.changed.notify_all();
    }

    fn remove_stream(&self, stream: u64) {
        let mut state = self.state.lock();
        if let Some(outstanding) = state.per_stream.remove(&stream) {
            state.used = state.used.saturating_sub(outstanding);
        }
        state.waiters.retain(|candidate| *candidate != stream);
        self.changed.notify_all();
    }

    fn used(&self) -> usize {
        self.state.lock().used
    }
}

struct StreamState {
    sent: u64,
    acked: u64,
    frame_ends: VecDeque<u64>,
    throttled: bool,
}

struct TransportCore {
    limits: TransportLimits,
    next_epoch: AtomicU64,
    streams: Mutex<HashMap<(String, u32), Weak<TerminalStream>>>,
    budget: PayloadBudget,
}

pub(crate) struct TerminalTransports {
    core: Arc<TransportCore>,
}

impl Default for TerminalTransports {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalTransports {
    pub(crate) fn new() -> Self {
        Self::with_limits(TransportLimits::default())
    }

    pub(crate) fn with_limits(limits: TransportLimits) -> Self {
        Self {
            core: Arc::new(TransportCore {
                limits,
                next_epoch: AtomicU64::new(1),
                streams: Mutex::new(HashMap::new()),
                budget: PayloadBudget::new(limits.app_payload_budget),
            }),
        }
    }

    pub(crate) fn attach(
        &self,
        owner: CallerIdentity,
        run: RunKey,
        route: Arc<OutputRoute<OutputFrame>>,
    ) -> Result<Arc<TerminalStream>, SafeError> {
        self.attach_inner(owner, run, route, None)
    }

    pub(crate) fn attach_observed(
        &self,
        owner: CallerIdentity,
        run: RunKey,
        route: Arc<OutputRoute<OutputFrame>>,
        progress: Arc<dyn OutputProgress>,
    ) -> Result<Arc<TerminalStream>, SafeError> {
        self.attach_inner(owner, run, route, Some(Arc::downgrade(&progress)))
    }

    fn attach_inner(
        &self,
        owner: CallerIdentity,
        run: RunKey,
        route: Arc<OutputRoute<OutputFrame>>,
        progress: Option<Weak<dyn OutputProgress>>,
    ) -> Result<Arc<TerminalStream>, SafeError> {
        if run.run_id.is_empty() || run.run_id.contains('\0') || run.generation == 0 {
            return Err(SafeError::invalid("run"));
        }
        let key = (run.run_id.clone(), run.generation);
        let mut streams = self.core.streams.lock();
        if streams.get(&key).and_then(Weak::upgrade).is_some() {
            return Err(error("OUTPUT_STREAM_BUSY"));
        }
        streams.remove(&key);

        let epoch = self
            .core
            .next_epoch
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1)
            })
            .map_err(|_| error("OUTPUT_STREAM_EPOCH_EXHAUSTED"))?;
        let stream_epoch = WireU64::parse(&epoch.to_string())
            .expect("backend-generated stream epoch is canonical");
        let stream = Arc::new(TerminalStream {
            id: epoch,
            key: key.clone(),
            owner,
            run,
            stream_epoch,
            route,
            progress,
            core: Arc::downgrade(&self.core),
            send_gate: Mutex::new(()),
            state: Mutex::new(StreamState {
                sent: 0,
                acked: 0,
                frame_ends: VecDeque::new(),
                throttled: false,
            }),
            degraded: AtomicBool::new(false),
            changed: Condvar::new(),
        });
        streams.insert(key, Arc::downgrade(&stream));
        drop(streams);

        let weak = Arc::downgrade(&stream);
        stream.route.on_revoke(Arc::new(move || {
            if let Some(stream) = weak.upgrade() {
                stream.on_route_revoked();
            }
        }));
        Ok(stream)
    }

    pub(crate) fn ack(&self, caller: &CallerIdentity, ack: &OutputAck) -> Result<u64, SafeError> {
        let key = (ack.run_id.clone(), ack.generation);
        let stream = {
            let streams = self.core.streams.lock();
            let Some(stream) = streams.get(&key).and_then(Weak::upgrade) else {
                if streams.keys().any(|(run_id, _)| run_id == &ack.run_id) {
                    return Err(error("STALE_GENERATION"));
                }
                return Err(error("RUN_NOT_FOUND"));
            };
            stream
        };
        if &stream.owner != caller {
            return Err(error("FORBIDDEN"));
        }
        if stream.stream_epoch != ack.stream_epoch {
            return Err(error("STALE_OUTPUT_STREAM"));
        }
        stream.apply_ack(ack.through_offset.get())
    }

    pub(crate) fn budgeted_bytes(&self) -> usize {
        self.core.budget.used()
    }
}

pub(crate) struct TerminalStream {
    id: u64,
    key: (String, u32),
    owner: CallerIdentity,
    run: RunKey,
    stream_epoch: WireU64,
    route: Arc<OutputRoute<OutputFrame>>,
    progress: Option<Weak<dyn OutputProgress>>,
    core: Weak<TransportCore>,
    send_gate: Mutex<()>,
    state: Mutex<StreamState>,
    degraded: AtomicBool,
    changed: Condvar,
}

impl std::fmt::Debug for TerminalStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TerminalStream(<redacted>)")
    }
}

impl TerminalStream {
    pub(crate) fn stream_epoch(&self) -> WireU64 {
        self.stream_epoch
    }

    pub(crate) fn sent_offset(&self) -> WireU64 {
        wire(self.state.lock().sent)
    }

    fn progress(&self) -> Option<Arc<dyn OutputProgress>> {
        self.progress.as_ref().and_then(Weak::upgrade)
    }

    fn core(&self) -> Result<Arc<TransportCore>, SafeError> {
        self.core
            .upgrade()
            .ok_or_else(|| error("OUTPUT_STREAM_DEGRADED"))
    }

    fn wait_local_capacity(&self, core: &TransportCore, amount: usize) -> Result<(), SafeError> {
        let mut state = self.state.lock();
        loop {
            if self.degraded.load(Ordering::SeqCst) {
                return Err(error("OUTPUT_STREAM_DEGRADED"));
            }
            let outstanding = state.sent.saturating_sub(state.acked) as usize;
            if state.throttled {
                if outstanding <= core.limits.run_low_water {
                    state.throttled = false;
                } else {
                    self.changed.wait(&mut state);
                    continue;
                }
            }
            if outstanding
                .checked_add(amount)
                .is_none_or(|value| value > core.limits.run_high_water)
            {
                state.throttled = true;
                self.changed.wait(&mut state);
                continue;
            }
            return Ok(());
        }
    }

    fn mark_degraded(&self, core: &TransportCore) {
        if self.degraded.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let mut state = self.state.lock();
            state.throttled = false;
            state.frame_ends.clear();
            self.changed.notify_all();
        }
        core.budget.remove_stream(self.id);
        if let Some(progress) = self.progress() {
            progress.degraded(self.stream_epoch);
        }
    }

    fn on_route_revoked(&self) {
        if let Ok(core) = self.core() {
            self.mark_degraded(&core);
        } else {
            self.degraded.store(true, Ordering::SeqCst);
            self.changed.notify_all();
        }
    }

    fn dispatch_reserved(
        &self,
        core: &TransportCore,
        bytes: &[u8],
        reserved: usize,
    ) -> Result<(), SafeError> {
        debug_assert_eq!(bytes.len(), reserved);
        let (offset, end) = {
            let mut state = self.state.lock();
            if self.degraded.load(Ordering::SeqCst) {
                return Err(error("OUTPUT_STREAM_DEGRADED"));
            }
            let offset = state.sent;
            let Some(end) = offset.checked_add(bytes.len() as u64) else {
                drop(state);
                self.mark_degraded(core);
                return Err(error("OUTPUT_OFFSET_EXHAUSTED"));
            };
            state.sent = end;
            state.frame_ends.push_back(end);
            (offset, end)
        };
        let frame = OutputFrame {
            run_id: self.run.run_id.clone(),
            generation: self.run.generation,
            stream_epoch: self.stream_epoch,
            offset: WireU64::parse(&offset.to_string()).expect("internal offset is canonical"),
            bytes: bytes.to_vec(),
        };
        match self.route.send(frame) {
            Ok(()) => {
                debug_assert!(end >= offset);
                if let Some(progress) = self.progress() {
                    progress.sent_through(self.stream_epoch, wire(end));
                }
                Ok(())
            }
            Err(failure) => {
                self.mark_degraded(core);
                Err(failure)
            }
        }
    }

    pub(crate) fn send(&self, bytes: &[u8]) -> Result<(), SafeError> {
        let core = self.core()?;
        if bytes.is_empty() {
            return Err(error("OUTPUT_FRAME_EMPTY"));
        }
        if bytes.len() > core.limits.frame_bytes {
            return Err(error("OUTPUT_FRAME_TOO_LARGE"));
        }
        let _gate = self.send_gate.lock();
        self.wait_local_capacity(&core, bytes.len())?;
        core.budget.reserve(self.id, bytes.len(), &self.degraded)?;
        self.dispatch_reserved(&core, bytes, bytes.len())
    }

    /// Reserve a whole frame before reading. A short read releases the unused
    /// reservation before dispatch; EOF/error releases it all.
    pub(crate) fn pump_once(&self, reader: &mut dyn Read) -> Result<usize, SafeError> {
        let core = self.core()?;
        let _gate = self.send_gate.lock();
        let capacity = core.limits.frame_bytes;
        self.wait_local_capacity(&core, capacity)?;
        core.budget.reserve(self.id, capacity, &self.degraded)?;

        let mut bytes = vec![0_u8; capacity];
        let count = match reader.read(&mut bytes) {
            Ok(count) => count,
            Err(_) => {
                core.budget.release(self.id, capacity);
                return Err(error("OUTPUT_READ_FAILED"));
            }
        };
        if count == 0 {
            core.budget.release(self.id, capacity);
            return Ok(0);
        }
        if count < capacity {
            core.budget.release(self.id, capacity - count);
        }
        bytes.truncate(count);
        self.dispatch_reserved(&core, &bytes, count)?;
        Ok(count)
    }

    fn apply_ack(&self, through: u64) -> Result<u64, SafeError> {
        let core = self.core()?;
        let released = {
            let mut state = self.state.lock();
            if self.degraded.load(Ordering::SeqCst) {
                return Err(error("OUTPUT_STREAM_DEGRADED"));
            }
            if through < state.acked {
                return Err(error("OUTPUT_ACK_BACKWARD"));
            }
            if through > state.sent {
                return Err(error("OUTPUT_ACK_BEYOND_SENT"));
            }
            if through == state.acked {
                return Ok(0);
            }
            if !state.frame_ends.iter().any(|boundary| *boundary == through) {
                return Err(error("OUTPUT_ACK_NOT_FRAME_BOUNDARY"));
            }
            let released = through - state.acked;
            state.acked = through;
            while state
                .frame_ends
                .front()
                .is_some_and(|boundary| *boundary <= through)
            {
                state.frame_ends.pop_front();
            }
            if state.throttled
                && state.sent.saturating_sub(state.acked) as usize <= core.limits.run_low_water
            {
                state.throttled = false;
            }
            self.changed.notify_all();
            released
        };
        let amount = usize::try_from(released).expect("unacked output is bounded by usize limits");
        core.budget.release(self.id, amount);
        if let Some(progress) = self.progress() {
            progress.parsed_through(self.stream_epoch, wire(through));
        }
        Ok(released)
    }
}

fn wire(value: u64) -> WireU64 {
    WireU64::parse(&value.to_string()).expect("internal output offset is canonical")
}

impl Drop for TerminalStream {
    fn drop(&mut self) {
        let Some(core) = self.core.upgrade() else {
            return;
        };
        core.budget.remove_stream(self.id);
        let mut streams = core.streams.lock();
        if streams
            .get(&self.key)
            .is_some_and(|stream| std::ptr::eq(stream.as_ptr(), self as *const Self))
        {
            streams.remove(&self.key);
        }
    }
}
