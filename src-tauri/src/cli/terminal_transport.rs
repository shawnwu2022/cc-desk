//! D14 output transport contract scaffold.
//! Behavioral credit/ACK rules are intentionally implemented after RED tests.
#![allow(dead_code)]

use super::profiles::error;
use super::run_registry::RunKey;
use super::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024;
pub(crate) const RUN_HIGH_WATERMARK: usize = 256 * 1024;
pub(crate) const RUN_LOW_WATERMARK: usize = 64 * 1024;
pub(crate) const APPLICATION_PAYLOAD_BUDGET: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OutputFrame {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) stream_epoch: WireU64,
    pub(crate) offset: WireU64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OutputAck {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
    pub(crate) stream_epoch: WireU64,
    pub(crate) through_offset: WireU64,
}

#[derive(Debug, Clone)]
pub(crate) struct TransportBudget {
    capacity: usize,
}
impl TransportBudget {
    pub(crate) fn new(capacity: usize) -> Self { Self { capacity } }
    pub(crate) fn reserved_bytes(&self) -> usize { let _ = self.capacity; 0 }
}

#[derive(Debug)]
pub(crate) struct RunOutputFlow {
    run: RunKey,
    epoch: WireU64,
    budget: TransportBudget,
    degraded: Arc<AtomicBool>,
}
impl RunOutputFlow {
    pub(crate) fn new(run: RunKey, epoch: WireU64, budget: TransportBudget) -> Self {
        Self { run, epoch, budget, degraded: Arc::new(AtomicBool::new(false)) }
    }
    pub(crate) fn try_reserve(&self) -> Result<OutputPermit, SafeError> {
        if self.degraded.load(Ordering::SeqCst) {
            return Err(error("OUTPUT_DEGRADED"));
        }
        Ok(OutputPermit {
            run: self.run.clone(),
            epoch: self.epoch,
            budget: self.budget.clone(),
        })
    }
    pub(crate) fn ack(&self, _ack: &OutputAck) -> Result<(), SafeError> { Ok(()) }
    pub(crate) fn sent_offset(&self) -> u64 { 0 }
    pub(crate) fn outstanding_bytes(&self) -> usize { 0 }
    pub(crate) fn is_paused(&self) -> bool { false }
    pub(crate) fn degrade(&self) { self.degraded.store(true, Ordering::SeqCst); }
    pub(crate) fn is_degraded(&self) -> bool { self.degraded.load(Ordering::SeqCst) }
}

#[derive(Debug)]
pub(crate) struct OutputPermit {
    run: RunKey,
    epoch: WireU64,
    budget: TransportBudget,
}
impl OutputPermit {
    pub(crate) fn commit(self, bytes: Vec<u8>) -> Result<OutputFrame, SafeError> {
        let _ = self.budget;
        Ok(OutputFrame {
            run_id: self.run.run_id,
            generation: self.run.generation,
            stream_epoch: self.epoch,
            offset: WireU64::parse("0")?,
            bytes,
        })
    }
}
