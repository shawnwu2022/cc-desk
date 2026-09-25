//! Native input staging and host-writer receipts.
//!
//! Upload chunks are only staging. A complete user frame enters the PTY writer
//! under one exclusive OwnedPty writer lock supplied by RunAccess. Host-written
//! means all bytes plus flush completed; it never claims CLI consumption.

use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub(crate) const INPUT_UPLOAD_CHUNK_MAX: usize = 64 * 1024;
pub(crate) const INPUT_ACTION_BYTES_MAX: usize = 8 * 1024 * 1024;
pub(crate) const INPUT_RUN_STAGING_BYTES_MAX: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputBeginRequest {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) input_seq: WireU64,
    pub(crate) mode_epoch: WireU64,
    pub(crate) total_bytes: WireU64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputChunkRequest {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) input_seq: WireU64,
    pub(crate) offset: WireU64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputCommitRequest {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) input_seq: WireU64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputAbortRequest {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) input_seq: WireU64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProtocolInputRequest {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InputWriteState {
    HostWritten,
    PartialOrUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputWriteReceipt {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) input_seq: String,
    pub(crate) mode_epoch: String,
    pub(crate) state: InputWriteState,
    pub(crate) confirmed_bytes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolWriteReceipt {
    pub(crate) state: InputWriteState,
    pub(crate) confirmed_bytes: String,
}

impl ProtocolWriteReceipt {
    pub(crate) fn from_host(result: HostWriteResult) -> Self {
        Self {
            state: result.state,
            confirmed_bytes: result.confirmed_bytes.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostWriteResult {
    pub(crate) state: InputWriteState,
    pub(crate) confirmed_bytes: u64,
}

/// Write one logical frame without allowing another writer user to interleave.
/// The caller owns the outer per-PTY writer lock for this entire function.
pub(crate) fn write_host_frame(writer: &mut (dyn Write + Send), bytes: &[u8]) -> HostWriteResult {
    let mut confirmed = 0usize;
    while confirmed < bytes.len() {
        match writer.write(&bytes[confirmed..]) {
            Ok(0) => {
                return HostWriteResult {
                    state: InputWriteState::PartialOrUnknown,
                    confirmed_bytes: confirmed as u64,
                };
            }
            Ok(count) if count <= bytes.len() - confirmed => {
                confirmed += count;
            }
            Ok(_) => {
                return HostWriteResult {
                    state: InputWriteState::PartialOrUnknown,
                    confirmed_bytes: confirmed as u64,
                };
            }
            Err(failure) if failure.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                return HostWriteResult {
                    state: InputWriteState::PartialOrUnknown,
                    confirmed_bytes: confirmed as u64,
                };
            }
        }
    }

    loop {
        match writer.flush() {
            Ok(()) => {
                return HostWriteResult {
                    state: InputWriteState::HostWritten,
                    confirmed_bytes: confirmed as u64,
                };
            }
            Err(failure) if failure.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                return HostWriteResult {
                    state: InputWriteState::PartialOrUnknown,
                    confirmed_bytes: confirmed as u64,
                };
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct OwnerRunKey {
    instance_id: String,
    window_label: String,
    webview_epoch: u64,
    run_id: String,
    generation: u32,
}

impl OwnerRunKey {
    fn new(caller: &CallerIdentity, run_id: &str, generation: u32) -> Result<Self, SafeError> {
        validate_run(run_id, generation)?;
        Ok(Self {
            instance_id: caller.instance_id.clone(),
            window_label: caller.window_label.clone(),
            webview_epoch: caller.webview_epoch.get(),
            run_id: run_id.to_string(),
            generation,
        })
    }
}

struct InputStage {
    input_seq: u64,
    mode_epoch: u64,
    total_bytes: usize,
    bytes: Vec<u8>,
    committing: bool,
}

#[derive(Default)]
struct RunInputState {
    stage: Option<InputStage>,
    last_closed_seq: u64,
    last_receipt: Option<InputWriteReceipt>,
    frozen: bool,
}

#[derive(Default)]
pub(crate) struct InputStager {
    runs: Mutex<HashMap<OwnerRunKey, RunInputState>>,
}

impl InputStager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin(
        &self,
        caller: &CallerIdentity,
        request: &InputBeginRequest,
    ) -> Result<(), SafeError> {
        let key = OwnerRunKey::new(caller, &request.run_id, request.generation)?;
        let seq = validate_positive(request.input_seq, "inputSeq")?;
        let mode_epoch = validate_positive(request.mode_epoch, "modeEpoch")?;
        let total_bytes =
            usize::try_from(request.total_bytes.get()).map_err(|_| error("INPUT_TOO_LARGE"))?;
        if total_bytes == 0 {
            return Err(error("INPUT_EMPTY"));
        }
        if total_bytes > INPUT_ACTION_BYTES_MAX {
            return Err(error("INPUT_TOO_LARGE"));
        }
        if total_bytes > INPUT_RUN_STAGING_BYTES_MAX {
            return Err(error("INPUT_STAGING_BUDGET"));
        }

        let mut runs = self.runs.lock();
        let state = runs.entry(key).or_default();
        if state.frozen {
            return Err(error("INPUT_FROZEN"));
        }
        if seq <= state.last_closed_seq {
            return Err(error("INPUT_SEQ_STALE"));
        }
        if let Some(stage) = &state.stage {
            if stage.input_seq == seq
                && stage.mode_epoch == mode_epoch
                && stage.total_bytes == total_bytes
            {
                return Ok(());
            }
            return Err(error("INPUT_UPLOAD_BUSY"));
        }

        state.stage = Some(InputStage {
            input_seq: seq,
            mode_epoch,
            total_bytes,
            bytes: Vec::new(),
            committing: false,
        });
        Ok(())
    }

    pub(crate) fn chunk(
        &self,
        caller: &CallerIdentity,
        request: &InputChunkRequest,
    ) -> Result<(), SafeError> {
        let key = OwnerRunKey::new(caller, &request.run_id, request.generation)?;
        let seq = validate_positive(request.input_seq, "inputSeq")?;
        let offset =
            usize::try_from(request.offset.get()).map_err(|_| error("INPUT_OFFSET_MISMATCH"))?;
        if request.bytes.is_empty() {
            return Err(error("INPUT_CHUNK_EMPTY"));
        }
        if request.bytes.len() > INPUT_UPLOAD_CHUNK_MAX {
            return Err(error("INPUT_CHUNK_TOO_LARGE"));
        }

        let mut runs = self.runs.lock();
        let state = runs
            .get_mut(&key)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
        if state.frozen {
            return Err(error("INPUT_FROZEN"));
        }
        let stage = state
            .stage
            .as_mut()
            .filter(|stage| stage.input_seq == seq)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
        if stage.committing {
            return Err(error("INPUT_COMMITTING"));
        }

        if offset < stage.bytes.len() {
            let end = offset
                .checked_add(request.bytes.len())
                .ok_or_else(|| error("INPUT_CHUNK_CONFLICT"))?;
            if end <= stage.bytes.len() && stage.bytes[offset..end] == request.bytes {
                return Ok(());
            }
            return Err(error("INPUT_CHUNK_CONFLICT"));
        }
        if offset != stage.bytes.len() {
            return Err(error("INPUT_OFFSET_MISMATCH"));
        }
        let next = stage
            .bytes
            .len()
            .checked_add(request.bytes.len())
            .ok_or_else(|| error("INPUT_UPLOAD_OVERFLOW"))?;
        if next > stage.total_bytes {
            return Err(error("INPUT_UPLOAD_OVERFLOW"));
        }
        stage
            .bytes
            .try_reserve(request.bytes.len())
            .map_err(|_| error("INPUT_MEMORY_UNAVAILABLE"))?;
        stage.bytes.extend_from_slice(&request.bytes);
        Ok(())
    }

    pub(crate) fn abort(
        &self,
        caller: &CallerIdentity,
        request: &InputAbortRequest,
    ) -> Result<(), SafeError> {
        let key = OwnerRunKey::new(caller, &request.run_id, request.generation)?;
        let seq = validate_positive(request.input_seq, "inputSeq")?;
        let mut runs = self.runs.lock();
        let state = runs
            .get_mut(&key)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
        if state.frozen {
            return Err(error("INPUT_FROZEN"));
        }
        let stage = state
            .stage
            .as_ref()
            .filter(|stage| stage.input_seq == seq)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
        if stage.committing {
            return Err(error("INPUT_COMMITTING"));
        }
        state.stage = None;
        state.last_closed_seq = state.last_closed_seq.max(seq);
        Ok(())
    }

    pub(crate) fn commit(
        &self,
        caller: &CallerIdentity,
        request: &InputCommitRequest,
        write: impl FnOnce(&[u8]) -> Result<HostWriteResult, SafeError>,
    ) -> Result<InputWriteReceipt, SafeError> {
        let key = OwnerRunKey::new(caller, &request.run_id, request.generation)?;
        let seq = validate_positive(request.input_seq, "inputSeq")?;

        let (mode_epoch, payload) = {
            let mut runs = self.runs.lock();
            let state = runs
            .get_mut(&key)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
            if let Some(receipt) = &state.last_receipt {
                if receipt.input_seq == seq.to_string() {
                    return Ok(receipt.clone());
                }
            }
            if state.frozen {
                return Err(error("INPUT_FROZEN"));
            }
            if seq <= state.last_closed_seq {
                return Err(error("INPUT_SEQ_STALE"));
            }

            let stage = state
                .stage
                .as_mut()
                .filter(|stage| stage.input_seq == seq)
                .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
            if stage.committing {
                return Err(error("INPUT_COMMITTING"));
            }
            if stage.bytes.len() != stage.total_bytes {
                return Err(error("INPUT_UPLOAD_INCOMPLETE"));
            }
            stage.committing = true;
            (stage.mode_epoch, std::mem::take(&mut stage.bytes))
        };

        let write_result = match catch_unwind(AssertUnwindSafe(|| write(&payload))) {
            Ok(Ok(result)) => result,
            Ok(Err(failure)) => {
                let mut runs = self.runs.lock();
                if let Some(stage) = runs
                    .get_mut(&key)
                    .and_then(|state| state.stage.as_mut())
                    .filter(|stage| stage.input_seq == seq && stage.committing)
                {
                    stage.bytes = payload;
                    stage.committing = false;
                }
                return Err(failure);
            }
            Err(_) => HostWriteResult {
                state: InputWriteState::PartialOrUnknown,
                confirmed_bytes: 0,
            },
        };

        if write_result.confirmed_bytes > payload.len() as u64
            || (write_result.state == InputWriteState::HostWritten
                && write_result.confirmed_bytes != payload.len() as u64)
        {
            let mut runs = self.runs.lock();
            if let Some(stage) = runs
                .get_mut(&key)
                .and_then(|state| state.stage.as_mut())
                .filter(|stage| stage.input_seq == seq && stage.committing)
            {
                stage.bytes = payload;
                stage.committing = false;
            }
            return Err(error("INPUT_WRITE_RESULT_INVALID"));
        }

        let receipt = InputWriteReceipt {
            run_id: request.run_id.clone(),
            generation: request.generation,
            input_seq: seq.to_string(),
            mode_epoch: mode_epoch.to_string(),
            state: write_result.state,
            confirmed_bytes: write_result.confirmed_bytes.to_string(),
        };

        let mut runs = self.runs.lock();
        let state = runs
            .get_mut(&key)
            .ok_or_else(|| error("INPUT_UPLOAD_NOT_FOUND"))?;
        let matches = state
            .stage
            .as_ref()
            .is_some_and(|stage| stage.input_seq == seq && stage.committing);
        if !matches {
            return Err(error("INPUT_UPLOAD_STATE_LOST"));
        }
        state.stage = None;
        state.last_closed_seq = state.last_closed_seq.max(seq);
        state.last_receipt = Some(receipt.clone());
        if receipt.state == InputWriteState::PartialOrUnknown {
            state.frozen = true;
        }
        Ok(receipt)
    }


    pub(crate) fn validate_protocol(
        &self,
        caller: &CallerIdentity,
        request: &ProtocolInputRequest,
    ) -> Result<(), SafeError> {
        let _key = OwnerRunKey::new(caller, &request.run_id, request.generation)?;
        if request.bytes.is_empty() {
            return Err(error("INPUT_CHUNK_EMPTY"));
        }
        if request.bytes.len() > INPUT_UPLOAD_CHUNK_MAX {
            return Err(error("INPUT_CHUNK_TOO_LARGE"));
        }
        Ok(())
    }
}

fn validate_run(run_id: &str, generation: u32) -> Result<(), SafeError> {
    if run_id.is_empty() || run_id.contains('\0') {
        return Err(SafeError::invalid("runId"));
    }
    if generation == 0 {
        return Err(SafeError::invalid("generation"));
    }
    Ok(())
}

fn validate_positive(value: WireU64, field: &str) -> Result<u64, SafeError> {
    let value = value.get();
    if value == 0 {
        return Err(SafeError::invalid(field));
    }
    Ok(value)
}
