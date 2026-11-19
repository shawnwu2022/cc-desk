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

// Fixed numeric CSI structure only, bounded before formatting; never raw PTY bytes.
pub(crate) fn csi(
    params: &alacritty_terminal::vte::Params,
    prefix: u8,
    intermediates: &[u8],
    action: char,
    ignore: bool,
) {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let Ok(event) = COUNT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        (n < 8).then_some(n + 1)
    }) else {
        return;
    };
    let intermediate_count = intermediates.len();
    let intermediate_known =
        intermediate_count <= 2 && intermediates.iter().all(|b| (0x20..=0x2f).contains(b));
    let intermediate_values = if intermediate_known {
        intermediates
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    } else {
        "unknown".into()
    };
    let count = params.iter().count();
    let mut truncated = count > 16;
    let mut groups = String::new();
    for (i, p) in params.iter().take(16).enumerate() {
        if i > 0 {
            groups.push('|');
        }
        groups.push_str(&p.len().to_string());
        groups.push(':');
        truncated |= p.len() > 6;
        for (j, n) in p.iter().take(6).enumerate() {
            if j > 0 {
                groups.push(',');
            }
            groups.push_str(&n.to_string());
        }
    }
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "CC_CANONICAL_CSI event={event} final={} class={prefix} intermediate_count={intermediate_count} intermediate_known={intermediate_known} intermediate_values={intermediate_values} ignore={} groups={count} truncated={} values={groups}",
        action as u32,
        u8::from(ignore),
        u8::from(truncated)
    );
}
