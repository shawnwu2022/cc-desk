//! Temporary verification-branch diagnostics; default off, no text/payload/identity fields.
// stage 1: initial rows/cols; 2: prepare failure (storage/unavailable/other).
// stage 3: terminal safe fault (OSC/unsupported/combining/batch/query/event/epoch).
// stage 4: resize (code=local-only, a=rows,b=cols).
// stage 5: projection loss (unsupported/rate/partial/unknown/other).
// stage 6: ingress sealed. No raw bytes, arbitrary errors, paths or identities.
use std::{
    io::Write,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
pub fn emit(stage: u8, code: u8, a: u64, b: u64) {
    static START: OnceLock<Instant> = OnceLock::new();
    static COUNTS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
    let Some(count) = COUNTS.get(usize::from(stage)) else {
        return;
    };
    let start = START.get_or_init(Instant::now);
    // Eight events per fixed category: resize traffic cannot consume a fault's first slot.
    if count
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < 8).then_some(n + 1)
        })
        .is_err()
    {
        return;
    }
    let _ = writeln!(
        std::io::stderr().lock(),
        "CC_CANONICAL_DIAG stage={stage} code={code} a={a} b={b} elapsed_ms={}",
        start.elapsed().as_millis()
    );
}

use std::sync::atomic::{AtomicBool, AtomicU64};
static READ_BYTES: AtomicU64 = AtomicU64::new(0);
static READ_CALLS: AtomicU64 = AtomicU64::new(0);
static PROCESSED_BYTES: AtomicU64 = AtomicU64::new(0);
static PROCESSED_CALLS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicBool = AtomicBool::new(false);
fn add(counter: &AtomicU64, n: u64) {
    if counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(n))
        .is_err()
    {
        OVERFLOW.store(true, Ordering::Relaxed);
    }
}
pub fn observed_read(bytes: usize) {
    add(&READ_BYTES, bytes as u64);
    add(&READ_CALLS, 1);
}
pub fn processed(bytes: usize) {
    if STOP_REQUESTED.load(Ordering::SeqCst) {
        add(&POST_STOP_PROCESSED, bytes as u64);
    }
    add(&PROCESSED_BYTES, bytes as u64);
    add(&PROCESSED_CALLS, 1);
}
/// Original reader ingress and full successful parser batches, not IO/descendant retirement.
pub fn processing_totals() -> (u64, u64, u64, u64, bool) {
    (
        READ_BYTES.load(Ordering::Relaxed),
        READ_CALLS.load(Ordering::Relaxed),
        PROCESSED_BYTES.load(Ordering::Relaxed),
        PROCESSED_CALLS.load(Ordering::Relaxed),
        !OVERFLOW.load(Ordering::Relaxed),
    )
}

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static SEAL_EVENTS: AtomicU64 = AtomicU64::new(0);
static QUEUED_BYTES: AtomicU64 = AtomicU64::new(0);
static SEALED_REJECTED_BYTES: AtomicU64 = AtomicU64::new(0);
static UNQUALIFIED_BYTES: AtomicU64 = AtomicU64::new(0);
static POST_STOP_QUEUED: AtomicU64 = AtomicU64::new(0);
static POST_STOP_PROCESSED: AtomicU64 = AtomicU64::new(0);
/// Observation fence before the real stop store; not an admission condition.
/// It conservatively counts racing in-flight completion as post-request.
pub fn stop_requested() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}
pub fn stop_sealed() {
    add(&SEAL_EVENTS, 1);
}
pub fn queued(bytes: usize) {
    if STOP_REQUESTED.load(Ordering::SeqCst) {
        add(&POST_STOP_QUEUED, bytes as u64);
    }
    add(&QUEUED_BYTES, bytes as u64);
}
pub fn sealed_rejected(bytes: usize) {
    add(&SEALED_REJECTED_BYTES, bytes as u64);
}
pub fn unqualified(bytes: usize) {
    add(&UNQUALIFIED_BYTES, bytes as u64);
}
/// Single-test-process causal accounting only, not proof of all IO retirement.
pub fn admission_totals() -> (u64, u64, u64, u64, u64, u64) {
    (
        QUEUED_BYTES.load(Ordering::Relaxed),
        SEALED_REJECTED_BYTES.load(Ordering::Relaxed),
        UNQUALIFIED_BYTES.load(Ordering::Relaxed),
        SEAL_EVENTS.load(Ordering::Relaxed),
        POST_STOP_QUEUED.load(Ordering::Relaxed),
        POST_STOP_PROCESSED.load(Ordering::Relaxed),
    )
}
