//! Test-only launch progress, independent of PTY output and final acceptance reports.
#![cfg_attr(not(windows), allow(dead_code))]

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

const MAX_RECORDS: usize = 64;
const RECORD_BYTES: usize = 16;
const MAX_BYTES: usize = MAX_RECORDS * RECORD_BYTES;
pub(super) const FILE_NAME: &str = "launch-stages.bin";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Mode {
    Ready = 1,
    Closed = 2,
}

impl Mode {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Closed => "closed",
        }
    }
}

macro_rules! codes {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        #[repr(u8)]
        pub(super) enum Code { $($variant),+ }

        const ALL_CODES: &[Code] = &[$(Code::$variant),+];

        impl Code {
            fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
        }
    };
}

// Only this compile-time table can supply diagnostic text; no caller string is stored.
codes! {
    WorkerStarted => "worker_started",
    InitializeStarted => "initialize_started",
    InitializeComplete => "initialize_complete",
    AppBuildStarted => "app_build_started",
    AppSetupStarted => "app_setup_started",
    MainInitialized => "main_initialized",
    MainPageLoaded => "main_page_loaded",
    PeerPageLoaded => "peer_page_loaded",
    AppBuilt => "app_built",
    RunReturnStarted => "run_return_started",
    RunReturned => "run_returned",
    CleanupStarted => "cleanup_started",
    ChildCleanupStarted => "child_cleanup_started",
    RootWaitStarted => "root_wait_started",
    RootWaitComplete => "root_wait_complete",
    RegistryRetired => "registry_retired",
    ChildCleanupComplete => "child_cleanup_complete",
    ReaderJoinStarted => "reader_join_started",
    ReaderJoinComplete => "reader_join_complete",
    CleanupComplete => "cleanup_complete",
    CleanupFailed => "cleanup_failed",
    ReportWritten => "report_written",
    UnknownObservation => "unknown_observation",
    UnknownFailure => "unknown_failure",
    CancelledRequestFenced => "cancelled-request-fenced",
    SingleChild => "single-child",
    ReceiptRecovered => "receipt-recovered",
    ProfileSnapshotFrozen => "profile-snapshot-frozen",
    PeerRejected => "peer-rejected",
    StaleGenerationRejected => "stale-generation-rejected",
    BytesReceived => "bytes-received",
    DestroyRevoked => "destroy-revoked",
    OwnedChildReaped => "owned-child-reaped",
    UnreadyRejectedBeforeIo => "unready-rejected-before-io",
    UnreadyCancelledWithoutIo => "unready-cancelled-without-io",
    ObservationOrder => "OBSERVATION_ORDER",
    EvalFailed => "EVAL_FAILED",
    CancelWrongOwner => "CANCEL_WRONG_OWNER",
    CancelReceiptMissing => "CANCEL_RECEIPT_MISSING",
    CancelNotFenced => "CANCEL_NOT_FENCED",
    BadRecoveredReceipt => "BAD_RECOVERED_RECEIPT",
    ChildNotReady => "CHILD_NOT_READY",
    DuplicateChild => "DUPLICATE_CHILD",
    ChildInputsChanged => "CHILD_INPUTS_CHANGED",
    OwnerAccessFailed => "OWNER_ACCESS_FAILED",
    ReplayChanged => "REPLAY_CHANGED",
    SnapshotLost => "SNAPSHOT_LOST",
    FrozenRunChanged => "FROZEN_RUN_CHANGED",
    PeerBuildFailed => "PEER_BUILD_FAILED",
    PeerAccessGranted => "PEER_ACCESS_GRANTED",
    MainContinueFailed => "MAIN_CONTINUE_FAILED",
    StaleAccessGranted => "STALE_ACCESS_GRANTED",
    AckWrongOwner => "ACK_WRONG_OWNER",
    ActualPtyBytesChanged => "ACTUAL_PTY_BYTES_CHANGED",
    DestroyFailed => "DESTROY_FAILED",
    DestroyTimeout => "DESTROY_TIMEOUT",
    RevokedAccessOrOwnerLost => "REVOKED_ACCESS_OR_OWNER_LOST",
    ReapFailed => "REAP_FAILED",
    GateWrongOwner => "GATE_WRONG_OWNER",
    GateDidIo => "GATE_DID_IO",
    UnreadyCancelMissing => "UNREADY_CANCEL_MISSING",
    UnreadyCancelNotFenced => "UNREADY_CANCEL_NOT_FENCED",
    ScriptInitial => "SCRIPT_FAILED:initial",
    ScriptCancelBeforeStart => "SCRIPT_FAILED:cancel-before-start",
    ScriptGate => "SCRIPT_FAILED:gate",
    ScriptConcurrentStart => "SCRIPT_FAILED:concurrent-start",
    ScriptStatus => "SCRIPT_FAILED:status",
    ScriptReplayAfterDelete => "SCRIPT_FAILED:replay-after-delete",
    ScriptStaleRun => "SCRIPT_FAILED:stale-run",
    ScriptNativeBytes => "SCRIPT_FAILED:native-bytes",
    ScriptPeer => "SCRIPT_FAILED:peer",
    ScriptUnknown => "SCRIPT_FAILED:unknown",
}

struct WriterState {
    file: Option<File>,
    sequence: u8,
}

pub(super) struct Diagnostics {
    started: Instant,
    mode: Mode,
    state: Mutex<WriterState>,
}

impl Diagnostics {
    pub(super) fn new(path: &Path, mode: Mode) -> Self {
        Self {
            started: Instant::now(),
            mode,
            state: Mutex::new(WriterState {
                file: File::create(path).ok(),
                sequence: 0,
            }),
        }
    }

    pub(super) fn mark(&self, code: Code) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.sequence as usize >= MAX_RECORDS {
            return;
        }
        let sequence = state.sequence + 1;
        let elapsed = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let record = encode(self.mode, sequence, elapsed, code);
        let Some(file) = state.file.as_mut() else {
            return;
        };
        // Unbuffered, immediate fixed-size writes survive a later worker kill. On IO failure
        // stop writing; a partial final record is rejected by the bounded reader.
        if file.write_all(&record).and_then(|_| file.flush()).is_err() {
            state.file = None;
            return;
        }
        state.sequence = sequence;
    }

    pub(super) fn observation(&self, name: &str) {
        let allowed = &ALL_CODES
            [Code::CancelledRequestFenced as usize..=Code::UnreadyCancelledWithoutIo as usize];
        self.mark(
            allowed
                .iter()
                .copied()
                .find(|code| code.name() == name)
                .unwrap_or(Code::UnknownObservation),
        );
    }

    pub(super) fn failure(&self, name: &str) {
        let allowed = &ALL_CODES[Code::ObservationOrder as usize..=Code::ScriptUnknown as usize];
        self.mark(
            allowed
                .iter()
                .copied()
                .find(|code| code.name() == name)
                .unwrap_or(Code::UnknownFailure),
        );
    }
}

fn encode(mode: Mode, sequence: u8, elapsed: u64, code: Code) -> [u8; RECORD_BYTES] {
    let mut record = [0; RECORD_BYTES];
    record[..4].copy_from_slice(b"D11L");
    record[4] = 1;
    record[5] = mode as u8;
    record[6] = sequence;
    record[7] = code as u8;
    record[8..16].copy_from_slice(&elapsed.to_le_bytes());
    record
}

#[derive(Debug, PartialEq, Eq)]
enum SnapshotError {
    Unavailable,
    Invalid,
    TooLarge,
}

impl SnapshotError {
    fn name(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Invalid => "invalid",
            Self::TooLarge => "too_large",
        }
    }
}

pub(super) fn snapshot(path: &Path, mode: Mode) -> String {
    File::open(path)
        .map_err(|_| SnapshotError::Unavailable)
        .and_then(|file| snapshot_from(file, mode))
        .unwrap_or_else(|error| format!("mode={} diagnostics={}", mode.name(), error.name()))
}

fn snapshot_from(reader: impl Read, mode: Mode) -> Result<String, SnapshotError> {
    let mut bytes = Vec::with_capacity(MAX_BYTES + 1);
    reader
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| SnapshotError::Unavailable)?;
    if bytes.len() > MAX_BYTES {
        return Err(SnapshotError::TooLarge);
    }
    if !bytes.len().is_multiple_of(RECORD_BYTES) {
        return Err(SnapshotError::Invalid);
    }
    let state = match bytes.len() {
        0 => "empty",
        MAX_BYTES => "capped",
        _ => "ok",
    };
    let mut output = format!("mode={} diagnostics={state}", mode.name());
    let mut last_elapsed = 0;
    for (index, record) in bytes.chunks_exact(RECORD_BYTES).enumerate() {
        let code = ALL_CODES
            .get(record[7] as usize)
            .ok_or(SnapshotError::Invalid)?;
        let elapsed = u64::from_le_bytes(record[8..16].try_into().unwrap());
        if &record[..4] != b"D11L"
            || record[4] != 1
            || record[5] != mode as u8
            || record[6] as usize != index + 1
            || elapsed < last_elapsed
        {
            return Err(SnapshotError::Invalid);
        }
        last_elapsed = elapsed;
        let _ = write!(
            output,
            "\nsequence={} elapsed_ms={elapsed} code={}",
            index + 1,
            code.name()
        );
    }
    Ok(output)
}

#[path = "native_cli_launch_diagnostics_tests.rs"]
mod behavior;
