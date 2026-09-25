use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::WireU64;
use crate::terminal_input::{
    write_host_frame, HostWriteResult, InputAbortRequest, InputBeginRequest, InputChunkRequest,
    InputCommitRequest, InputStager, InputWriteState, INPUT_ACTION_BYTES_MAX,
    INPUT_UPLOAD_CHUNK_MAX,
};
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

fn wire(value: u64) -> WireU64 {
    WireU64::parse(&value.to_string()).unwrap()
}

fn caller(instance: &str, epoch: u64) -> CallerIdentity {
    CallerIdentity {
        instance_id: instance.to_string(),
        window_label: "main".to_string(),
        webview_epoch: wire(epoch),
    }
}

fn begin(seq: u64, total: u64) -> InputBeginRequest {
    InputBeginRequest {
        run_id: "run-a".to_string(),
        generation: 1,
        input_seq: wire(seq),
        mode_epoch: wire(7),
        total_bytes: wire(total),
    }
}

fn chunk(seq: u64, offset: u64, bytes: Vec<u8>) -> InputChunkRequest {
    InputChunkRequest {
        run_id: "run-a".to_string(),
        generation: 1,
        input_seq: wire(seq),
        offset: wire(offset),
        bytes,
    }
}

fn commit(seq: u64) -> InputCommitRequest {
    InputCommitRequest {
        run_id: "run-a".to_string(),
        generation: 1,
        input_seq: wire(seq),
    }
}

struct FaultWriter {
    written: Vec<u8>,
    max_per_write: usize,
    fail_after: Option<usize>,
    fail_flush: bool,
}

impl FaultWriter {
    fn new(max_per_write: usize) -> Self {
        Self {
            written: Vec::new(),
            max_per_write,
            fail_after: None,
            fail_flush: false,
        }
    }
}

impl Write for FaultWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.fail_after.is_some_and(|limit| self.written.len() >= limit) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected"));
        }
        let remaining_before_failure = self
            .fail_after
            .map(|limit| limit.saturating_sub(self.written.len()))
            .unwrap_or(usize::MAX);
        let count = buf
            .len()
            .min(self.max_per_write)
            .min(remaining_before_failure);
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected"));
        }
        self.written.extend_from_slice(&buf[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush injected"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn D17_Writer_AllBytesAndFlushProduceHostWritten_001() {
    let mut writer = FaultWriter::new(3);
    let result = write_host_frame(&mut writer, b"abcdef");

    assert_eq!(writer.written, b"abcdef");
    assert_eq!(result.state, InputWriteState::HostWritten);
    assert_eq!(result.confirmed_bytes, 6);
}

#[test]
fn D17_Writer_PartialFailureReportsConfirmedPrefixOnly_002() {
    let mut writer = FaultWriter::new(3);
    writer.fail_after = Some(3);
    let result = write_host_frame(&mut writer, b"abcdef");

    assert_eq!(writer.written, b"abc");
    assert_eq!(result.state, InputWriteState::PartialOrUnknown);
    assert_eq!(result.confirmed_bytes, 3);
}

#[test]
fn D17_Writer_FlushFailureIsPartialOrUnknownEvenAfterAllWrites_003() {
    let mut writer = FaultWriter::new(16);
    writer.fail_flush = true;
    let result = write_host_frame(&mut writer, b"abcdef");

    assert_eq!(writer.written, b"abcdef");
    assert_eq!(result.state, InputWriteState::PartialOrUnknown);
    assert_eq!(result.confirmed_bytes, 6);
}

#[test]
fn D17_Staging_ExactChunksCommitOnceAndDuplicateCommitDoesNotReplay_004() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    let payload: Vec<u8> = (0..(INPUT_UPLOAD_CHUNK_MAX + 7))
        .map(|index| (index % 251) as u8)
        .collect();

    stager.begin(&owner, &begin(1, payload.len() as u64)).unwrap();
    stager
        .chunk(
            &owner,
            &chunk(1, 0, payload[..INPUT_UPLOAD_CHUNK_MAX].to_vec()),
        )
        .unwrap();
    stager
        .chunk(
            &owner,
            &chunk(
                1,
                INPUT_UPLOAD_CHUNK_MAX as u64,
                payload[INPUT_UPLOAD_CHUNK_MAX..].to_vec(),
            ),
        )
        .unwrap();

    let calls = AtomicUsize::new(0);
    let receipt = stager
        .commit(&owner, &commit(1), |bytes| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(bytes, payload);
            Ok(HostWriteResult {
                state: InputWriteState::HostWritten,
                confirmed_bytes: bytes.len() as u64,
            })
        })
        .unwrap();
    assert_eq!(receipt.state, InputWriteState::HostWritten);
    assert_eq!(receipt.confirmed_bytes, payload.len().to_string());

    let replay = stager
        .commit(&owner, &commit(1), |_bytes| {
            calls.fetch_add(1, Ordering::SeqCst);
            panic!("duplicate commit replayed writer")
        })
        .unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn D17_Staging_IncompleteOrOversizeRejectsBeforeWriter_005() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);

    let oversized = begin(1, (INPUT_ACTION_BYTES_MAX + 1) as u64);
    assert_eq!(stager.begin(&owner, &oversized).unwrap_err().code, "INPUT_TOO_LARGE");

    stager.begin(&owner, &begin(2, 4)).unwrap();
    stager.chunk(&owner, &chunk(2, 0, vec![1, 2])).unwrap();
    let calls = AtomicUsize::new(0);
    let failure = stager
        .commit(&owner, &commit(2), |_bytes| {
            calls.fetch_add(1, Ordering::SeqCst);
            unreachable!()
        })
        .unwrap_err();
    assert_eq!(failure.code, "INPUT_UPLOAD_INCOMPLETE");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn D17_Staging_ChunkBoundsOffsetAndOwnerIsolation_006() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    let other = caller("doc-b", 1);
    stager.begin(&owner, &begin(1, 4)).unwrap();

    assert_eq!(
        stager
            .chunk(&owner, &chunk(1, 1, vec![1]))
            .unwrap_err()
            .code,
        "INPUT_OFFSET_MISMATCH"
    );
    assert_eq!(
        stager
            .chunk(&other, &chunk(1, 0, vec![1]))
            .unwrap_err()
            .code,
        "INPUT_UPLOAD_NOT_FOUND"
    );

    let too_large = vec![0; INPUT_UPLOAD_CHUNK_MAX + 1];
    assert_eq!(
        stager
            .chunk(&owner, &chunk(1, 0, too_large))
            .unwrap_err()
            .code,
        "INPUT_CHUNK_TOO_LARGE"
    );
}

#[test]
fn D17_Staging_ExactDuplicateChunkIsIdempotentButConflictFails_007() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    stager.begin(&owner, &begin(1, 3)).unwrap();
    stager.chunk(&owner, &chunk(1, 0, vec![1, 2])).unwrap();
    stager.chunk(&owner, &chunk(1, 0, vec![1, 2])).unwrap();

    assert_eq!(
        stager
            .chunk(&owner, &chunk(1, 0, vec![1, 9]))
            .unwrap_err()
            .code,
        "INPUT_CHUNK_CONFLICT"
    );
}

#[test]
fn D17_Staging_PartialReceiptFreezesLaterUserInput_008() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    stager.begin(&owner, &begin(1, 6)).unwrap();
    stager
        .chunk(&owner, &chunk(1, 0, b"abcdef".to_vec()))
        .unwrap();

    let receipt = stager
        .commit(&owner, &commit(1), |_bytes| {
            Ok(HostWriteResult {
                state: InputWriteState::PartialOrUnknown,
                confirmed_bytes: 3,
            })
        })
        .unwrap();
    assert_eq!(receipt.state, InputWriteState::PartialOrUnknown);
    assert_eq!(receipt.confirmed_bytes, "3");

    assert_eq!(
        stager.begin(&owner, &begin(2, 1)).unwrap_err().code,
        "INPUT_FROZEN"
    );

    let replay = stager
        .commit(&owner, &commit(1), |_bytes| panic!("partial commit replayed"))
        .unwrap();
    assert_eq!(replay, receipt);
}

#[test]
fn D17_Staging_AbortIsZeroWriteAndAllowsHigherSequence_009() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    stager.begin(&owner, &begin(1, 4)).unwrap();
    stager.chunk(&owner, &chunk(1, 0, vec![1, 2])).unwrap();
    stager
        .abort(
            &owner,
            &InputAbortRequest {
                run_id: "run-a".to_string(),
                generation: 1,
                input_seq: wire(1),
            },
        )
        .unwrap();

    stager.begin(&owner, &begin(2, 1)).unwrap();
    stager.chunk(&owner, &chunk(2, 0, vec![9])).unwrap();
    let receipt = stager
        .commit(&owner, &commit(2), |bytes| {
            assert_eq!(bytes, &[9]);
            Ok(HostWriteResult {
                state: InputWriteState::HostWritten,
                confirmed_bytes: 1,
            })
        })
        .unwrap();
    assert_eq!(receipt.state, InputWriteState::HostWritten);
}

#[test]
fn D17_Staging_StaleSequenceCannotBeReopenedAfterFinalization_010() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    stager.begin(&owner, &begin(2, 1)).unwrap();
    stager.chunk(&owner, &chunk(2, 0, vec![7])).unwrap();
    stager
        .commit(&owner, &commit(2), |bytes| {
            Ok(HostWriteResult {
                state: InputWriteState::HostWritten,
                confirmed_bytes: bytes.len() as u64,
            })
        })
        .unwrap();

    assert_eq!(
        stager.begin(&owner, &begin(1, 1)).unwrap_err().code,
        "INPUT_SEQ_STALE"
    );
}


#[test]
fn D17_Staging_WriterPanicFreezesWithoutReplay_011() {
    let stager = InputStager::new();
    let owner = caller("doc-a", 1);
    stager.begin(&owner, &begin(1, 3)).unwrap();
    stager.chunk(&owner, &chunk(1, 0, vec![1, 2, 3])).unwrap();

    let receipt = stager
        .commit(&owner, &commit(1), |_bytes| -> Result<HostWriteResult, crate::cli::types::SafeError> {
            panic!("injected writer panic after ownership transfer")
        })
        .unwrap();
    assert_eq!(receipt.state, InputWriteState::PartialOrUnknown);
    assert_eq!(receipt.confirmed_bytes, "0");
    assert_eq!(
        stager.begin(&owner, &begin(2, 1)).unwrap_err().code,
        "INPUT_FROZEN"
    );
    assert_eq!(
        stager
            .commit(&owner, &commit(1), |_bytes| panic!("panic receipt replayed writer"))
            .unwrap(),
        receipt
    );
}
