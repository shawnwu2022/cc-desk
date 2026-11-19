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
