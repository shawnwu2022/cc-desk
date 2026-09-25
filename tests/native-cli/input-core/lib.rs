//! Compile the actual D17 terminal input source on all three OS families.
#![allow(dead_code, non_snake_case)]

#[path = "../../../src-tauri/src/cli/types.rs"]
mod types;

mod cli {
    pub(crate) mod types {
        pub(crate) use crate::types::*;
    }

    pub(crate) mod profiles {
        use crate::types::SafeError;

        pub(crate) fn error(code: &'static str) -> SafeError {
            SafeError {
                code: code.to_string(),
                field: None,
                index: None,
                retryable: false,
            }
        }
    }

    pub(crate) mod snapshot {
        use crate::types::WireU64;

        #[derive(Clone, PartialEq, Eq)]
        pub(crate) struct CallerIdentity {
            pub(crate) instance_id: String,
            pub(crate) window_label: String,
            pub(crate) webview_epoch: WireU64,
        }
    }
}

#[path = "../../../src-tauri/src/terminal_input.rs"]
mod terminal_input;

#[cfg(test)]
mod tests {
    use super::cli::snapshot::CallerIdentity;
    use super::terminal_input::{
        write_host_frame, HostWriteResult, InputBeginRequest, InputChunkRequest,
        InputCommitRequest, InputStager, InputWriteState, INPUT_ACTION_BYTES_MAX,
        INPUT_RUN_STAGING_BYTES_MAX, INPUT_UPLOAD_CHUNK_MAX,
    };
    use super::types::WireU64;
    use std::io::{self, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn wire(value: u64) -> WireU64 {
        WireU64::parse(&value.to_string()).unwrap()
    }

    fn owner(id: &str) -> CallerIdentity {
        CallerIdentity {
            instance_id: id.into(),
            window_label: "main".into(),
            webview_epoch: wire(1),
        }
    }

    fn begin(seq: u64, total: u64) -> InputBeginRequest {
        InputBeginRequest {
            run_id: "run-a".into(),
            generation: 1,
            input_seq: wire(seq),
            mode_epoch: wire(1),
            total_bytes: wire(total),
        }
    }

    fn chunk(seq: u64, offset: u64, bytes: Vec<u8>) -> InputChunkRequest {
        InputChunkRequest {
            run_id: "run-a".into(),
            generation: 1,
            input_seq: wire(seq),
            offset: wire(offset),
            bytes,
        }
    }

    fn commit(seq: u64) -> InputCommitRequest {
        InputCommitRequest {
            run_id: "run-a".into(),
            generation: 1,
            input_seq: wire(seq),
        }
    }

    struct PartialWriter {
        bytes: Vec<u8>,
        fail_after: usize,
    }

    impl Write for PartialWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.bytes.len() >= self.fail_after {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected"));
            }
            let count = buf.len().min(self.fail_after - self.bytes.len());
            self.bytes.extend_from_slice(&buf[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn D17_Core_ProductionBudgetsArePinned_001() {
        assert_eq!(INPUT_UPLOAD_CHUNK_MAX, 64 * 1024);
        assert_eq!(INPUT_ACTION_BYTES_MAX, 8 * 1024 * 1024);
        assert_eq!(INPUT_RUN_STAGING_BYTES_MAX, 16 * 1024 * 1024);
    }

    #[test]
    fn D17_Core_PartialWriterReportsConfirmedPrefix_002() {
        let mut writer = PartialWriter {
            bytes: Vec::new(),
            fail_after: 3,
        };
        let result = write_host_frame(&mut writer, b"abcdef");
        assert_eq!(writer.bytes, b"abc");
        assert_eq!(result.state, InputWriteState::PartialOrUnknown);
        assert_eq!(result.confirmed_bytes, 3);
    }

    #[test]
    fn D17_Core_ProductionStagerIsOwnerScopedAndCommitIsIdempotent_003() {
        let stager = InputStager::new();
        let a = owner("a");
        let b = owner("b");
        stager.begin(&a, &begin(1, 3)).unwrap();
        assert_eq!(
            stager.chunk(&b, &chunk(1, 0, vec![1])).unwrap_err().code,
            "INPUT_UPLOAD_NOT_FOUND"
        );
        stager.chunk(&a, &chunk(1, 0, vec![1, 2, 3])).unwrap();

        let calls = AtomicUsize::new(0);
        let first = stager
            .commit(&a, &commit(1), |bytes| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(HostWriteResult {
                    state: InputWriteState::HostWritten,
                    confirmed_bytes: bytes.len() as u64,
                })
            })
            .unwrap();
        let second = stager
            .commit(&a, &commit(1), |_| panic!("writer replayed"))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn D17_Core_PartialCommitFreezesLaterInput_004() {
        let stager = InputStager::new();
        let a = owner("a");
        stager.begin(&a, &begin(1, 2)).unwrap();
        stager.chunk(&a, &chunk(1, 0, vec![1, 2])).unwrap();
        let receipt = stager
            .commit(&a, &commit(1), |_| {
                Ok(HostWriteResult {
                    state: InputWriteState::PartialOrUnknown,
                    confirmed_bytes: 1,
                })
            })
            .unwrap();
        assert_eq!(receipt.state, InputWriteState::PartialOrUnknown);
        assert_eq!(stager.begin(&a, &begin(2, 1)).unwrap_err().code, "INPUT_FROZEN");
    }

    #[test]
    fn D17_Core_Native42LargeChunkedPastePartialWriteIsNotReplayed_005() {
        let stager = InputStager::new();
        let a = owner("a");
        let payload: Vec<u8> = (0..(INPUT_UPLOAD_CHUNK_MAX + 17))
            .map(|index| (index % 251) as u8)
            .collect();

        stager
            .begin(&a, &begin(1, payload.len() as u64))
            .unwrap();
        stager
            .chunk(
                &a,
                &chunk(1, 0, payload[..INPUT_UPLOAD_CHUNK_MAX].to_vec()),
            )
            .unwrap();
        stager
            .chunk(
                &a,
                &chunk(
                    1,
                    INPUT_UPLOAD_CHUNK_MAX as u64,
                    payload[INPUT_UPLOAD_CHUNK_MAX..].to_vec(),
                ),
            )
            .unwrap();

        let fail_after = INPUT_UPLOAD_CHUNK_MAX + 3;
        let mut writer = PartialWriter {
            bytes: Vec::new(),
            fail_after,
        };
        let calls = AtomicUsize::new(0);
        let receipt = stager
            .commit(&a, &commit(1), |bytes| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(write_host_frame(&mut writer, bytes))
            })
            .unwrap();

        assert_eq!(receipt.state, InputWriteState::PartialOrUnknown);
        assert_eq!(receipt.confirmed_bytes, fail_after.to_string());
        assert_eq!(writer.bytes, payload[..fail_after]);
        assert_eq!(stager.begin(&a, &begin(2, 1)).unwrap_err().code, "INPUT_FROZEN");

        let replay = stager
            .commit(&a, &commit(1), |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                panic!("NATIVE-42 payload replayed")
            })
            .unwrap();
        assert_eq!(replay, receipt);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

}
