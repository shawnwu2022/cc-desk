//! Native input staging and writer receipts. D17 implementation follows RED tests.

use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};
use std::io::Write;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostWriteResult {
    pub(crate) state: InputWriteState,
    pub(crate) confirmed_bytes: u64,
}

pub(crate) fn write_host_frame(
    _writer: &mut (dyn Write + Send),
    _bytes: &[u8],
) -> HostWriteResult {
    HostWriteResult {
        state: InputWriteState::PartialOrUnknown,
        confirmed_bytes: 0,
    }
}

#[derive(Default)]
pub(crate) struct InputStager;

impl InputStager {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn begin(
        &self,
        _caller: &CallerIdentity,
        _request: &InputBeginRequest,
    ) -> Result<(), SafeError> {
        Err(error("NATIVE_INPUT_NOT_READY"))
    }

    pub(crate) fn chunk(
        &self,
        _caller: &CallerIdentity,
        _request: &InputChunkRequest,
    ) -> Result<(), SafeError> {
        Err(error("NATIVE_INPUT_NOT_READY"))
    }

    pub(crate) fn abort(
        &self,
        _caller: &CallerIdentity,
        _request: &InputAbortRequest,
    ) -> Result<(), SafeError> {
        Err(error("NATIVE_INPUT_NOT_READY"))
    }

    pub(crate) fn commit(
        &self,
        _caller: &CallerIdentity,
        _request: &InputCommitRequest,
        _write: impl FnOnce(&[u8]) -> Result<HostWriteResult, SafeError>,
    ) -> Result<InputWriteReceipt, SafeError> {
        Err(error("NATIVE_INPUT_NOT_READY"))
    }
}
