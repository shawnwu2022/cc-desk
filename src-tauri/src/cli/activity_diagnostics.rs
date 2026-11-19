use super::snapshot::CallerIdentity;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Arc, time::Instant};
use tauri::http::HeaderMap;

const HEADER: &str = "x-cc-desk-diag-trace";
const LIMIT: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TraceId([u8; 16]);

impl TraceId {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        if text.len() != 32
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let mut bytes = [0; 16];
        for (value, pair) in bytes.iter_mut().zip(text.as_bytes().as_chunks::<2>().0) {
            *value = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Self(bytes))
    }

    fn random() -> Self {
        Self(*uuid::Uuid::new_v4().as_bytes())
    }

    fn wire(self) -> String {
        uuid::Uuid::from_bytes(self.0).simple().to_string()
    }
}

impl Serialize for TraceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&uuid::Uuid::from_bytes(self.0).simple())
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Stage {
    Backend,
    Provider,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Status {
    Started,
    Ok,
    Failed,
    Unsupported,
    Unknown,
    Returned,
    Blocked,
    Stale,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Event {
    /// 仅操作随机号；采集令牌不进入报告。
    id: TraceId,
    stage: Stage,
    status: Status,
    ms: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub(crate) dropped: u32,
    pub(crate) events: Vec<Event>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Control {
    Start { serial: u64 },
    Stop { serial: u64 },
    Snapshot,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum ControlReply {
    Started { grant: String },
    Stopped,
    Snapshot { report: Report },
}

#[derive(Default)]
struct State {
    owner: Option<CallerIdentity>,
    serial: u64,
    grant: Option<TraceId>,
    sealed: bool,
    dropped: u32,
    events: VecDeque<Event>,
}

#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<State>>);

impl Capture {
    pub(crate) fn control(&self, caller: &CallerIdentity, query: Control) -> Option<ControlReply> {
        match query {
            Control::Start { serial } => self
                .start(caller, serial)
                .map(|grant| ControlReply::Started { grant }),
            Control::Stop { serial } => self.stop(caller, serial).then_some(ControlReply::Stopped),
            Control::Snapshot => self
                .snapshot(caller)
                .map(|report| ControlReply::Snapshot { report }),
        }
    }

    pub(crate) fn start(&self, caller: &CallerIdentity, serial: u64) -> Option<String> {
        let mut state = self.0.lock();
        if state.sealed
            || serial <= state.serial
            || state.owner.as_ref().is_some_and(|owner| owner != caller)
        {
            return None;
        }
        let grant = TraceId::random();
        state.owner = Some(caller.clone());
        state.serial = serial;
        state.grant = Some(grant);
        state.dropped = 0;
        state.events.clear();
        Some(grant.wire())
    }

    pub(crate) fn stop(&self, caller: &CallerIdentity, serial: u64) -> bool {
        let mut state = self.0.lock();
        if state.sealed
            || serial < state.serial
            || state.owner.as_ref().is_some_and(|owner| owner != caller)
        {
            return false;
        }
        if serial == state.serial {
            return state.grant.is_none();
        }
        state.owner = Some(caller.clone());
        state.serial = serial;
        state.grant = None;
        true
    }

    pub(crate) fn seal(&self) {
        let mut state = self.0.lock();
        state.sealed = true;
        state.grant = None;
        state.events.clear();
    }

    pub(crate) fn snapshot(&self, caller: &CallerIdentity) -> Option<Report> {
        let state = self.0.lock();
        if state.sealed || state.owner.as_ref() != Some(caller) {
            return None;
        }
        Some(Report {
            dropped: state.dropped,
            events: state.events.iter().copied().collect(),
        })
    }

    pub(crate) fn trace_from_headers(
        &self,
        caller: &CallerIdentity,
        headers: &HeaderMap,
    ) -> Option<Trace> {
        let entries = headers.get_all(HEADER);
        let mut values = entries.iter();
        let value = values.next()?.to_str().ok()?;
        if values.next().is_some() {
            return None;
        }
        self.trace(caller, Some(value))
    }

    pub(crate) fn trace(&self, caller: &CallerIdentity, header: Option<&str>) -> Option<Trace> {
        let header = header?;
        if header.len() != 65 {
            return None;
        }
        let (grant, id) = header.split_once(':')?;
        let (grant, id) = (TraceId::parse(grant)?, TraceId::parse(id)?);
        let state = self.0.try_lock()?;
        if state.sealed || state.owner.as_ref() != Some(caller) || state.grant != Some(grant) {
            return None;
        }
        Some(Trace {
            capture: self.clone(),
            grant,
            id,
            started: Instant::now(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Trace {
    capture: Capture,
    grant: TraceId,
    id: TraceId,
    started: Instant,
}

impl Trace {
    pub(crate) fn record(&self, stage: Stage, status: Status) {
        let Some(mut state) = self.capture.0.try_lock() else {
            return;
        };
        if state.sealed || state.grant != Some(self.grant) {
            return;
        }
        if state.events.len() == LIMIT {
            state.events.pop_front();
            state.dropped = state.dropped.saturating_add(1);
        }
        state.events.push_back(Event {
            id: self.id,
            stage,
            status,
            ms: self.started.elapsed().as_millis().min(u32::MAX.into()) as u32,
        });
    }
}

pub(crate) fn observe<T>(
    trace: Option<&Trace>,
    stage: Stage,
    action: impl FnOnce() -> Result<T, super::types::SafeError>,
) -> Result<T, super::types::SafeError> {
    if let Some(trace) = trace {
        trace.record(stage, Status::Started);
    }
    let result = action();
    if let Some(trace) = trace {
        let status = match &result {
            Ok(_) if matches!(stage, Stage::Backend) => Status::Returned,
            Ok(_) => Status::Ok,
            Err(error) => match error.code.as_str() {
                "NATIVE_ARCHIVE_UNSUPPORTED"
                | "NATIVE_RENAME_UNSUPPORTED"
                | "NATIVE_DELETE_UNSUPPORTED"
                | "RECYCLE_UNSUPPORTED" => Status::Unsupported,
                "ARCHIVE_OUTCOME_UNKNOWN"
                | "RENAME_OUTCOME_UNKNOWN"
                | "DELETE_OUTCOME_UNKNOWN"
                | "LAUNCH_STATE_UNKNOWN"
                | "RECYCLE_OUTCOME_UNKNOWN" => Status::Unknown,
                "FORBIDDEN" | "RUN_SUPERVISOR_STOPPING" | "DELETE_CONFIRMATION_REQUIRED" => {
                    Status::Blocked
                }
                "SCOPE_STALE" | "SCOPE_REVOKED" | "SOURCE_CHANGED" => Status::Stale,
                _ => Status::Failed,
            },
        };
        trace.record(stage, status);
    }
    result
}
