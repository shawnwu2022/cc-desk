//! Explicit diagnostic only: fixed numeric rejection metadata, never terminal text.
use std::sync::atomic::{AtomicUsize, Ordering};
pub(crate) fn rejected(code: u8, a: u64, b: u64) {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    if COUNT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < 8).then_some(n + 1)
        })
        .is_err()
    {
        return;
    }
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "CC_CANONICAL_DIAG stage=7 code={code} a={a} b={b} elapsed_ms=0"
    );
}
