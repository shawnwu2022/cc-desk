//! Process/output lifecycle state independent from PTY transport ownership.
#![allow(dead_code)] // D15 supervisor wiring follows this state-machine checkpoint.

use crate::cli::profiles::error;
use crate::cli::run_registry::RunKey;
use crate::cli::types::{SafeError, WireU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessLifecycle {
    Starting,
    Running,
    Exited,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputLifecycle {
    Open,
    Draining,
    Drained,
    Incomplete,
    Degraded,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleRecord {
    run: RunKey,
    process: ProcessLifecycle,
    output: OutputLifecycle,
    stream_epoch: Option<WireU64>,
    sent: u64,
    parsed: u64,
    final_offset: Option<u64>,
}

impl LifecycleRecord {
    pub(crate) fn new(run: RunKey) -> Self {
        Self {
            run,
            process: ProcessLifecycle::Starting,
            output: OutputLifecycle::Open,
            stream_epoch: None,
            sent: 0,
            parsed: 0,
            final_offset: None,
        }
    }

    pub(crate) fn run(&self) -> &RunKey {
        &self.run
    }

    pub(crate) fn process(&self) -> ProcessLifecycle {
        self.process
    }

    pub(crate) fn output(&self) -> OutputLifecycle {
        self.output
    }

    pub(crate) fn final_offset(&self) -> Option<WireU64> {
        self.final_offset.map(wire)
    }

    pub(crate) fn parsed_offset(&self) -> WireU64 {
        wire(self.parsed)
    }

    pub(crate) fn process_running(&mut self) -> Result<(), SafeError> {
        match self.process {
            ProcessLifecycle::Starting | ProcessLifecycle::Running => {
                self.process = ProcessLifecycle::Running;
                Ok(())
            }
            _ => Err(error("RUN_STATE_CONFLICT")),
        }
    }

    pub(crate) fn process_exited(&mut self) -> Result<(), SafeError> {
        match self.process {
            ProcessLifecycle::Starting | ProcessLifecycle::Running | ProcessLifecycle::Exited => {
                self.process = ProcessLifecycle::Exited;
                if self.output == OutputLifecycle::Open {
                    self.output = OutputLifecycle::Draining;
                }
                self.refresh_drained();
                Ok(())
            }
            ProcessLifecycle::Failed => Err(error("RUN_STATE_CONFLICT")),
        }
    }

    pub(crate) fn process_failed(&mut self) {
        self.process = ProcessLifecycle::Failed;
        if !matches!(
            self.output,
            OutputLifecycle::Degraded | OutputLifecycle::Incomplete
        ) {
            self.output = OutputLifecycle::Incomplete;
        }
    }

    pub(crate) fn output_started(&mut self, epoch: &str) -> Result<(), SafeError> {
        let epoch = WireU64::parse(epoch)?;
        if epoch.get() == 0 {
            return Err(SafeError::invalid("streamEpoch"));
        }
        match self.stream_epoch {
            None => {
                self.stream_epoch = Some(epoch);
                Ok(())
            }
            Some(current) if current == epoch => Ok(()),
            Some(_) => Err(error("STALE_OUTPUT_STREAM")),
        }
    }

    pub(crate) fn sent_through(&mut self, through: &str) -> Result<(), SafeError> {
        self.require_stream()?;
        let through = WireU64::parse(through)?.get();
        if through < self.sent {
            return Err(error("OUTPUT_OFFSET_BACKWARD"));
        }
        if self.final_offset.is_some() && through != self.sent {
            return Err(error("OUTPUT_ALREADY_ENDED"));
        }
        self.sent = through;
        Ok(())
    }

    pub(crate) fn output_end(&mut self, final_offset: &str) -> Result<(), SafeError> {
        let epoch = self.require_stream()?;
        self.output_end_for(&epoch.to_string(), final_offset)
    }

    pub(crate) fn output_end_for(
        &mut self,
        epoch: &str,
        final_offset: &str,
    ) -> Result<(), SafeError> {
        self.check_epoch(epoch)?;
        let final_offset = WireU64::parse(final_offset)?.get();
        if final_offset > self.sent {
            return Err(error("OUTPUT_END_BEYOND_SENT"));
        }
        if let Some(current) = self.final_offset {
            if current != final_offset {
                return Err(error("OUTPUT_END_CONFLICT"));
            }
        } else {
            self.final_offset = Some(final_offset);
        }
        if self.output == OutputLifecycle::Open {
            self.output = OutputLifecycle::Draining;
        }
        self.refresh_drained();
        Ok(())
    }

    pub(crate) fn parsed_through(&mut self, through: &str) -> Result<(), SafeError> {
        let epoch = self.require_stream()?;
        self.parsed_through_for(&epoch.to_string(), through)
    }

    pub(crate) fn parsed_through_for(
        &mut self,
        epoch: &str,
        through: &str,
    ) -> Result<(), SafeError> {
        self.check_epoch(epoch)?;
        let through = WireU64::parse(through)?.get();
        if through < self.parsed {
            return Err(error("OUTPUT_ACK_BACKWARD"));
        }
        if through > self.sent {
            return Err(error("OUTPUT_ACK_BEYOND_SENT"));
        }
        self.parsed = through;
        self.refresh_drained();
        Ok(())
    }

    pub(crate) fn mark_degraded(&mut self) -> Result<(), SafeError> {
        self.require_stream()?;
        self.output = OutputLifecycle::Degraded;
        Ok(())
    }

    pub(crate) fn mark_incomplete(&mut self) -> Result<(), SafeError> {
        self.output = OutputLifecycle::Incomplete;
        Ok(())
    }

    pub(crate) fn can_retire(&self) -> bool {
        self.process == ProcessLifecycle::Exited
            && matches!(
                self.output,
                OutputLifecycle::Drained | OutputLifecycle::Degraded | OutputLifecycle::Incomplete
            )
    }

    pub(crate) fn can_retire_as_complete(&self) -> bool {
        self.process == ProcessLifecycle::Exited && self.output == OutputLifecycle::Drained
    }

    fn require_stream(&self) -> Result<WireU64, SafeError> {
        self.stream_epoch
            .ok_or_else(|| error("OUTPUT_STREAM_NOT_READY"))
    }

    fn check_epoch(&self, epoch: &str) -> Result<(), SafeError> {
        let epoch = WireU64::parse(epoch)?;
        if self.stream_epoch != Some(epoch) {
            return Err(error("STALE_OUTPUT_STREAM"));
        }
        Ok(())
    }

    fn refresh_drained(&mut self) {
        if matches!(
            self.output,
            OutputLifecycle::Degraded | OutputLifecycle::Incomplete
        ) {
            return;
        }
        if self
            .final_offset
            .is_some_and(|final_offset| self.parsed == final_offset)
        {
            self.output = OutputLifecycle::Drained;
        } else if self.final_offset.is_some() || self.process == ProcessLifecycle::Exited {
            self.output = OutputLifecycle::Draining;
        }
    }
}

fn wire(value: u64) -> WireU64 {
    WireU64::parse(&value.to_string()).expect("internal lifecycle offsets are canonical")
}
