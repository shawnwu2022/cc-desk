//! Compile the actual D14 transport source on all three OS families.
//! GUI delivery is a tiny test seam; transport accounting is the production module.
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

    pub(crate) mod run_registry {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub(crate) struct RunKey {
            pub(crate) run_id: String,
            pub(crate) generation: u32,
        }
    }

    pub(crate) mod snapshot {
        use crate::types::WireU64;

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub(crate) struct CallerIdentity {
            pub(crate) instance_id: String,
            pub(crate) window_label: String,
            pub(crate) webview_epoch: WireU64,
        }
    }

    pub(crate) mod output_route {
        use crate::types::SafeError;
        use std::sync::Arc;

        pub(crate) struct OutputRoute<T> {
            sink: Arc<dyn Fn(T) -> Result<(), SafeError> + Send + Sync>,
        }

        impl<T> OutputRoute<T> {
            pub(crate) fn new(
                sink: impl Fn(T) -> Result<(), SafeError> + Send + Sync + 'static,
            ) -> Self {
                Self {
                    sink: Arc::new(sink),
                }
            }

            pub(crate) fn send(&self, value: T) -> Result<(), SafeError> {
                (self.sink)(value)
            }
        }
    }
}

#[path = "../../../src-tauri/src/terminal_transport.rs"]
mod terminal_transport;

#[cfg(test)]
mod tests {
    use super::cli::output_route::OutputRoute;
    use super::cli::run_registry::RunKey;
    use super::cli::snapshot::CallerIdentity;
    use super::terminal_transport::{
        OutputAck, OutputFrame, TerminalTransports, TransportLimits, OUTPUT_APP_PAYLOAD_BUDGET,
        OUTPUT_FRAME_BYTES_MAX, OUTPUT_RUN_HIGH_WATER, OUTPUT_RUN_LOW_WATER,
    };
    use super::types::{SafeError, WireU64};
    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use std::io::{self, Read};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    fn owner() -> CallerIdentity {
        CallerIdentity {
            instance_id: "core".into(),
            window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        }
    }

    fn run(id: &str) -> RunKey {
        RunKey {
            run_id: id.into(),
            generation: 1,
        }
    }

    fn ack(run_id: &str, epoch: &str, through: &str) -> OutputAck {
        serde_json::from_value(json!({
            "runId": run_id,
            "generation": 1,
            "streamEpoch": epoch,
            "throughOffset": through,
        }))
        .unwrap()
    }

    fn route(events: Arc<Mutex<Vec<Value>>>) -> Arc<OutputRoute<OutputFrame>> {
        Arc::new(OutputRoute::new(move |frame| {
            events.lock().push(serde_json::to_value(frame).unwrap());
            Ok(())
        }))
    }

    #[test]
    fn D14_Core_ProductionBudgetsArePinned_001() {
        assert_eq!(OUTPUT_FRAME_BYTES_MAX, 16 * 1024);
        assert_eq!(OUTPUT_RUN_HIGH_WATER, 256 * 1024);
        assert_eq!(OUTPUT_RUN_LOW_WATER, 64 * 1024);
        assert_eq!(OUTPUT_APP_PAYLOAD_BUDGET, 16 * 1024 * 1024);
    }

    #[test]
    fn D14_Core_OrderedBytesAndCumulativeAck_002() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 16).unwrap());
        let stream = hub.attach(owner(), run("a"), route(events.clone())).unwrap();
        stream.send(&[0, 255, 1, 2]).unwrap();
        stream.send(&[3, 4]).unwrap();
        let epoch = stream.stream_epoch().to_string();

        assert_eq!(
            *events.lock(),
            vec![
                json!({"runId":"a","generation":1,"streamEpoch":epoch,"offset":"0","bytes":[0,255,1,2]}),
                json!({"runId":"a","generation":1,"streamEpoch":epoch,"offset":"4","bytes":[3,4]}),
            ]
        );
        assert_eq!(hub.ack(&owner(), &ack("a", &epoch, "4")).unwrap(), 4);
        assert_eq!(hub.ack(&owner(), &ack("a", &epoch, "4")).unwrap(), 0);
        assert_eq!(
            hub.ack(&owner(), &ack("a", &epoch, "5")).unwrap_err().code,
            "OUTPUT_ACK_NOT_FRAME_BOUNDARY"
        );
        assert_eq!(hub.ack(&owner(), &ack("a", &epoch, "6")).unwrap(), 2);
        assert_eq!(hub.budgeted_bytes(), 0);
    }

    struct Probe {
        reads: Arc<AtomicUsize>,
    }

    impl Read for Probe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            buffer[..4].copy_from_slice(&[7, 7, 7, 7]);
            Ok(4)
        }
    }

    #[test]
    fn D14_Core_CreditIsReservedBeforeReadWithoutBlockingPeer_003() {
        let hub = Arc::new(TerminalTransports::with_limits(
            TransportLimits::new(4, 8, 4, 16).unwrap(),
        ));
        let a = hub
            .attach(owner(), run("a"), route(Arc::new(Mutex::new(Vec::new()))))
            .unwrap();
        let b_events = Arc::new(Mutex::new(Vec::new()));
        let b = hub.attach(owner(), run("b"), route(b_events.clone())).unwrap();
        a.send(&[1, 1, 1, 1]).unwrap();
        a.send(&[2, 2, 2, 2]).unwrap();
        b.send(&[9, 9, 9, 9]).unwrap();
        assert_eq!(b_events.lock().len(), 1);

        let reads = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(2));
        let worker = {
            let a = a.clone();
            let reads = reads.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut reader = Probe { reads };
                barrier.wait();
                a.pump_once(&mut reader)
            })
        };
        barrier.wait();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(reads.load(Ordering::SeqCst), 0);

        let epoch = a.stream_epoch().to_string();
        hub.ack(&owner(), &ack("a", &epoch, "4")).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), 4);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn D14_Core_RouteLossIsFinalAndReleasesPayload_004() {
        let calls = Arc::new(AtomicUsize::new(0));
        let sent = calls.clone();
        let failing = Arc::new(OutputRoute::new(move |_frame: OutputFrame| {
            sent.fetch_add(1, Ordering::SeqCst);
            Err(SafeError {
                code: "OUTPUT_ROUTE_LOST".into(),
                field: None,
                index: None,
                retryable: false,
            })
        }));
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 8).unwrap());
        let stream = hub.attach(owner(), run("a"), failing).unwrap();

        assert_eq!(
            stream.send(&[1, 2, 3, 4]).unwrap_err().code,
            "OUTPUT_ROUTE_LOST"
        );
        assert_eq!(
            stream.send(&[5]).unwrap_err().code,
            "OUTPUT_STREAM_DEGRADED"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(hub.budgeted_bytes(), 0);
    }
}
