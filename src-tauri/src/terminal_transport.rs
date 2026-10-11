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
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

pub(crate) const OUTPUT_FRAME_BYTES_MAX: usize = 16 * 1024;
pub(crate) const OUTPUT_RUN_HIGH_WATER: usize = 256 * 1024;
pub(crate) const OUTPUT_RUN_LOW_WATER: usize = 64 * 1024;
pub(crate) const OUTPUT_APP_PAYLOAD_BUDGET: usize = 16 * 1024 * 1024;
// renderer 停止 ACK 且未 revoke 时的兜底窗口：高水位阻塞超过该窗口且无任何
// ACK 进展，按既有 degraded 路径释放全局预算并唤醒阻塞者。窗口取值需宽于
// renderer 合理的解析/节流停顿，代价是跨系统休眠恢复可能误判一次降级。
pub(crate) const OUTPUT_ACK_STALENESS_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportLimits {
    frame_bytes: usize,
    run_high_water: usize,
    run_low_water: usize,
    app_payload_budget: usize,
    ack_staleness: Duration,
}

impl TransportLimits {
    pub(crate) fn new(
        frame_bytes: usize,
        run_high_water: usize,
        run_low_water: usize,
        app_payload_budget: usize,
    ) -> Result<Self, SafeError> {
        Self::with_ack_staleness(
            frame_bytes,
            run_high_water,
            run_low_water,
            app_payload_budget,
            OUTPUT_ACK_STALENESS_TIMEOUT,
        )
    }

    pub(crate) fn with_ack_staleness(
        frame_bytes: usize,
        run_high_water: usize,
        run_low_water: usize,
        app_payload_budget: usize,
        ack_staleness: Duration,
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
            ack_staleness,
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

    fn reserve(&self, stream: u64, amount: usize, degraded: &AtomicBool) -> Result<(), SafeError> {
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

    #[cfg(test)]
    fn waiter_count(&self) -> usize {
        self.state.lock().waiters.len()
    }
}

struct StreamState {
    sent: u64,
    acked: u64,
    frame_ends: VecDeque<u64>,
    throttled: bool,
    #[cfg(test)]
    waiting_for_capacity: bool,
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
                #[cfg(test)]
                waiting_for_capacity: false,
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

    #[cfg(test)]
    pub(crate) fn waiting_streams(&self) -> usize {
        self.core.budget.waiter_count()
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

    #[cfg(test)]
    pub(crate) fn waiting_for_local_capacity(&self) -> bool {
        self.state.lock().waiting_for_capacity
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
        let mut last_acked = state.acked;
        let mut deadline = Instant::now() + core.limits.ack_staleness;
        loop {
            if self.degraded.load(Ordering::SeqCst) {
                return Err(error("OUTPUT_STREAM_DEGRADED"));
            }
            let outstanding = state.sent.saturating_sub(state.acked) as usize;
            if state.throttled {
                if outstanding <= core.limits.run_low_water {
                    state.throttled = false;
                } else if self.wait_for_ack_progress(&mut state, &mut last_acked, &mut deadline) {
                    // 先释放 state 锁再降级；mark_degraded 内部需要重新加锁。
                    drop(state);
                    self.mark_degraded(core);
                    return Err(error("OUTPUT_STREAM_DEGRADED"));
                } else {
                    continue;
                }
            }
            if outstanding
                .checked_add(amount)
                .is_none_or(|value| value > core.limits.run_high_water)
            {
                state.throttled = true;
                if self.wait_for_ack_progress(&mut state, &mut last_acked, &mut deadline) {
                    drop(state);
                    self.mark_degraded(core);
                    return Err(error("OUTPUT_STREAM_DEGRADED"));
                }
                continue;
            }
            return Ok(());
        }
    }

    /// 在节流/高水位等待中观察 ACK 进展：任何解析进展都会重置兜底窗口。
    /// 返回 true 表示窗口耗尽且无进展，调用方须释放 state 锁后按 revoke
    /// 同款 degraded 路径收敛，避免 renderer 停止 ACK 永久占用 reader 与预算。
    fn wait_for_ack_progress(
        &self,
        state: &mut parking_lot::MutexGuard<'_, StreamState>,
        last_acked: &mut u64,
        deadline: &mut Instant,
    ) -> bool {
        #[cfg(test)]
        {
            state.waiting_for_capacity = true;
        }
        if state.acked != *last_acked {
            *last_acked = state.acked;
            *deadline = Instant::now() + self.staleness();
        }
        self.changed
            .wait_for(state, deadline.saturating_duration_since(Instant::now()));
        #[cfg(test)]
        {
            state.waiting_for_capacity = false;
        }
        if self.degraded.load(Ordering::SeqCst) {
            return false;
        }
        state.acked == *last_acked && Instant::now() >= *deadline
    }

    fn staleness(&self) -> Duration {
        self.core
            .upgrade()
            .map_or(OUTPUT_ACK_STALENESS_TIMEOUT, |core| {
                core.limits.ack_staleness
            })
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
            if state.sent.saturating_sub(state.acked) as usize >= core.limits.run_high_water {
                // Latch before an ACK can arrive ahead of the next capacity check.
                state.throttled = true;
            }
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
        self.pump_once_with_end(reader, |_| false)
    }

    pub(crate) fn pump_once_with_end(
        &self,
        reader: &mut dyn Read,
        is_stream_end: impl Fn(&io::Error) -> bool,
    ) -> Result<usize, SafeError> {
        let core = self.core()?;
        let _gate = self.send_gate.lock();
        let capacity = core.limits.frame_bytes;
        self.wait_local_capacity(&core, capacity)?;
        core.budget.reserve(self.id, capacity, &self.degraded)?;

        let mut bytes = vec![0_u8; capacity];
        let count = match reader.read(&mut bytes) {
            Ok(count) => count,
            Err(failure) => {
                core.budget.release(self.id, capacity);
                if is_stream_end(&failure) {
                    return Ok(0);
                }
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

#[cfg(test)]
mod staleness_tests {
    use super::*;
    use crate::cli::output_route::OutputRoutes;
    use crate::cli::types::WireU64;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::ipc::{Channel, InvokeResponseBody};

    fn caller() -> CallerIdentity {
        CallerIdentity {
            instance_id: "backend-test".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("7").unwrap(),
        }
    }

    fn run(id: &str, generation: u32) -> RunKey {
        RunKey {
            run_id: id.into(),
            generation,
        }
    }

    fn ack(run_id: &str, generation: u32, stream_epoch: &str, through: &str) -> OutputAck {
        serde_json::from_value(serde_json::json!({
            "runId": run_id,
            "generation": generation,
            "streamEpoch": stream_epoch,
            "throughOffset": through
        }))
        .unwrap()
    }

    fn silent_route() -> Arc<OutputRoute<OutputFrame>> {
        Arc::new(
            OutputRoutes::new(1)
                .bind(1, Box::new(|| Ok(())), || Ok(Channel::new(move |_| Ok(()))))
                .unwrap(),
        )
    }

    // renderer 停止 ACK 且未 revoke：高水位阻塞必须在兜底窗口后按 degraded
    // 收敛并释放全局预算，不得永久占用 reader 与预算。
    #[test]
    fn Transport_Staleness_StalledAckConvergesAndReleasesBudget_007() {
        let hub = Arc::new(TerminalTransports::with_limits(
            TransportLimits::with_ack_staleness(4, 8, 4, 16, Duration::from_millis(80)).unwrap(),
        ));
        let stream = hub
            .attach(caller(), run("run-a", 1), silent_route())
            .unwrap();
        stream.send(&[1, 1, 1, 1]).unwrap();
        stream.send(&[2, 2, 2, 2]).unwrap(); // 达到 8 字节高水位

        let (tx, rx) = mpsc::channel();
        let blocked = stream.clone();
        let worker = std::thread::spawn(move || {
            let result = blocked.send(&[3]).map_err(|failure| failure.code);
            tx.send(result).unwrap();
        });
        let outcome = rx.recv_timeout(Duration::from_secs(2));
        worker.join().unwrap();
        // 修复后为 OUTPUT_STREAM_DEGRADED；修复前永久阻塞导致超时失败。
        assert_eq!(
            outcome.unwrap(),
            Err("OUTPUT_STREAM_DEGRADED".to_string()),
            "无 ACK 阻塞必须在有界时间内收敛"
        );
        assert_eq!(hub.budgeted_bytes(), 0, "无 ACK 预算必须随降级释放");
    }

    // 正常 ACK 流不受兜底影响：ACK 进展持续重置窗口，预算随 ACK 归零。
    #[test]
    fn Transport_Staleness_HealthyAckFlowIsUnaffected_008() {
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 16).unwrap());
        let owner = caller();
        let stream = hub
            .attach(owner.clone(), run("run-b", 1), silent_route())
            .unwrap();
        let epoch = stream.stream_epoch().to_string();
        for index in 0..50i64 {
            let offset = index * 4;
            stream.send(&[index as u8; 4]).unwrap();
            hub.ack(&owner, &ack("run-b", 1, &epoch, &(offset + 4).to_string()))
                .unwrap();
        }
        assert_eq!(hub.budgeted_bytes(), 0);
        // 流仍健康：继续可发送且错误代次的 ACK 被拒绝。
        stream.send(&[9; 4]).unwrap();
        assert_eq!(
            hub.ack(&owner, &ack("run-b", 1, "999", "4"))
                .unwrap_err()
                .code,
            "STALE_OUTPUT_STREAM"
        );
        assert_eq!(
            hub.ack(&owner, &ack("run-b", 2, &epoch, "4"))
                .unwrap_err()
                .code,
            "STALE_GENERATION"
        );
    }
}
