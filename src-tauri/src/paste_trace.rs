//! Opt-in field diagnostic wrapper. Does not modify or retry PTY input.
//! Reference text exists only in this request's memory, never in diagnostic logs.

use serde::Deserialize;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTrace {
    paste_id: String,
    seq: u32,
    bytes: usize,
    clipboard: bool,
    age_ms: u64,
    expected: Option<String>,
}

#[derive(Default)]
struct TraceBudget {
    started: Option<Instant>,
    used: u32,
}

impl TraceBudget {
    fn take(&mut self, now: Instant) -> Option<u32> {
        let start = *self.started.get_or_insert(now);
        if self.used >= 256 || now.duration_since(start) >= Duration::from_secs(60) {
            return None;
        }
        self.used += 1;
        Some(self.used)
    }
}

static BUDGET: LazyLock<parking_lot::Mutex<TraceBudget>> =
    LazyLock::new(|| parking_lot::Mutex::new(TraceBudget::default()));

// Only this text-free structure may be formatted into diagnostic logs.
#[derive(Debug, PartialEq, Eq)]
struct InputSummary {
    complete: bool,
    body_bytes: usize,
    expected_bytes: Option<usize>,
    exact: Option<bool>,
    first_difference: Option<usize>,
    common_suffix: Option<usize>,
    controls: [usize; 9],
}

fn summarize(data: &str, expected: Option<&str>) -> InputSummary {
    let framed = data
        .strip_prefix("\x1b[200~")
        .and_then(|text| text.strip_suffix("\x1b[201~"));
    let body = framed.unwrap_or(data);
    let mut result = InputSummary {
        complete: framed.is_some(),
        body_bytes: body.len(),
        expected_bytes: expected.map(str::len),
        exact: expected.map(|text| text == body),
        first_difference: None,
        common_suffix: None,
        controls: [0; 9],
    };
    for byte in body.bytes() {
        if let Some(index) = [b'\n', b'\r', b'\t', 27, 8, 127, 3, 21, 23]
            .iter()
            .position(|value| *value == byte)
        {
            result.controls[index] += 1;
        }
    }
    if let Some(expected) = expected.filter(|text| *text != body) {
        result.first_difference = Some(
            body.bytes()
                .zip(expected.bytes())
                .take_while(|(a, b)| a == b)
                .count(),
        );
        result.common_suffix = Some(
            body.bytes()
                .rev()
                .zip(expected.bytes().rev())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    result
}

fn safe_source(source: Option<&str>) -> &'static str {
    match source {
        Some("terminal-ondata") => "terminal-ondata",
        Some("xterm-ondata-paste") => "xterm-ondata-paste",
        Some("clipboard-keyboard") => "clipboard-keyboard",
        Some("clipboard-dom") => "clipboard-dom",
        _ => "other",
    }
}

#[tauri::command]
pub async fn pty_input(
    id: String,
    data: String,
    source: Option<String>,
    trace: Option<InputTrace>,
) -> Result<bool, String> {
    let diagnostic = if option_env!("CC_DESK_PASTE_TRACE") == Some("1") {
        trace.and_then(|trace| {
            let pty = uuid::Uuid::parse_str(&id).ok()?;
            let paste = uuid::Uuid::parse_str(&trace.paste_id).ok()?;
            if trace.seq == 0 || trace.seq > 256 {
                return None;
            }
            let recv_seq = BUDGET.lock().take(Instant::now())?;
            let expected = trace
                .expected
                .as_deref()
                .filter(|text| trace.clipboard && text.len() <= 2 * 1024 * 1024);
            let summary = summarize(&data, expected);
            log::info!(
                "[paste_trace] recv pty={} paste={} send_seq={} recv_seq={} source={} wire_bytes={} ipc_size_match={} clipboard={} age_ms={} summary={:?}",
                pty, paste, trace.seq, recv_seq, safe_source(source.as_deref()),
                data.len(), trace.bytes == data.len(), trace.clipboard,
                trace.age_ms, summary
            );
            Some((paste, trace.seq, recv_seq, Instant::now()))
        })
    } else {
        None
    };
    // Preserve the original command, writer locking, payload bytes and errors.
    // No extra IPC call, async wait, transport rewrite, or automatic resend.
    let result = crate::commands::pty_input(id, data, source).await;
    if let Some((paste, send_seq, recv_seq, start)) = diagnostic {
        log::info!(
            "[paste_trace] done paste={} send_seq={} recv_seq={} success={} elapsed_us={}",
            paste,
            send_seq,
            recv_seq,
            matches!(result, Ok(true)),
            start.elapsed().as_micros()
        );
    }
    result
}

#[cfg(test)]
#[allow(non_snake_case)]
#[path = "tests/paste_trace.rs"]
mod tests;
