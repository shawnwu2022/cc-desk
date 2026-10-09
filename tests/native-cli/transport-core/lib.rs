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
        use parking_lot::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        type RevokeHook = Arc<dyn Fn() + Send + Sync>;

        pub(crate) struct OutputRoute<T> {
            sink: Arc<dyn Fn(T) -> Result<(), SafeError> + Send + Sync>,
            revoked: AtomicBool,
            revoke_hooks: Mutex<Vec<RevokeHook>>,
        }

        impl<T> OutputRoute<T> {
            pub(crate) fn new(
                sink: impl Fn(T) -> Result<(), SafeError> + Send + Sync + 'static,
            ) -> Self {
                Self {
                    sink: Arc::new(sink),
                    revoked: AtomicBool::new(false),
                    revoke_hooks: Mutex::new(Vec::new()),
                }
            }

            pub(crate) fn send(&self, value: T) -> Result<(), SafeError> {
                if self.revoked.load(Ordering::SeqCst) {
                    return Err(SafeError {
                        code: "OUTPUT_ROUTE_LOST".into(),
                        field: None,
                        index: None,
                        retryable: false,
                    });
                }
                (self.sink)(value)
            }

            pub(crate) fn on_revoke(&self, hook: RevokeHook) {
                let mut pending = Some(hook);
                {
                    let mut hooks = self.revoke_hooks.lock();
                    if !self.revoked.load(Ordering::SeqCst) {
                        hooks.push(pending.take().expect("pending revoke hook"));
                    }
                }
                if let Some(hook) = pending {
                    hook();
                }
            }

            pub(crate) fn revoke(&self) {
                if self.revoked.swap(true, Ordering::SeqCst) {
                    return;
                }
                let hooks = std::mem::take(&mut *self.revoke_hooks.lock());
                for hook in hooks {
                    hook();
                }
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
    use std::collections::HashSet;
    use std::io::{self, Read};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant};

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

    // 达到 12 字节高水位后，先 ACK 至余量 8 仍禁止读取，余量 4 时才恢复。
    #[test]
    fn D14_Core_HighWaterLatchesUntilLow_010() {
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 12, 4, 16).unwrap());
        let events = Arc::new(Mutex::new(Vec::new()));
        let stream = hub
            .attach(owner(), run("latched"), route(events.clone()))
            .unwrap();
        for value in 1..=3 {
            stream.send(&[value; 4]).unwrap();
        }
        let epoch = stream.stream_epoch().to_string();
        assert_eq!(hub.budgeted_bytes(), 12);
        assert_eq!(hub.ack(&owner(), &ack("latched", &epoch, "4")).unwrap(), 4);

        let reads = Arc::new(AtomicUsize::new(0));
        let (done_tx, done_rx) = mpsc::channel();
        let worker = {
            let stream = stream.clone();
            let reads = reads.clone();
            std::thread::spawn(move || {
                done_tx
                    .send(stream.pump_once(&mut Probe { reads }))
                    .unwrap();
            })
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !stream.waiting_for_local_capacity() && !worker.is_finished() {
            assert!(
                Instant::now() < deadline,
                "pump must reach a credit wait or complete"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "a small ACK after reaching high water must not allow another read"
        );
        assert_eq!(
            events.lock().len(),
            3,
            "the fourth frame must wait for low water"
        );
        assert_eq!(
            hub.budgeted_bytes(),
            8,
            "blocked reads must not reserve app credit"
        );

        assert_eq!(hub.ack(&owner(), &ack("latched", &epoch, "8")).unwrap(), 4);
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            4,
            "reaching low water must resume the pending read"
        );
        worker.join().unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(events.lock()[3]["offset"], "12");
        assert_eq!(events.lock()[3]["bytes"], json!([7, 7, 7, 7]));
        assert_eq!(hub.ack(&owner(), &ack("latched", &epoch, "16")).unwrap(), 8);
        assert_eq!(hub.budgeted_bytes(), 0);
    }

    // 32 个并发 run 争用 16 字节预算，28 个排队后逐帧 ACK，最终全部完成且预算归零。
    #[test]
    fn D27_Core_32RunsShare16ByteBudget_011() {
        const RUNS: usize = 32;
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 16).unwrap());
        let (frames_tx, frames_rx) = mpsc::channel();
        let barrier = Arc::new(Barrier::new(RUNS + 1));
        let mut streams = Vec::new();
        let mut epochs = Vec::new();
        for index in 0..RUNS {
            let frames_tx = frames_tx.clone();
            let output = Arc::new(OutputRoute::new(move |frame: OutputFrame| {
                frames_tx.send((index, frame)).unwrap();
                Ok(())
            }));
            let stream = hub
                .attach(owner(), run(&format!("stress-{index}")), output)
                .unwrap();
            epochs.push(stream.stream_epoch().to_string());
            streams.push(stream);
        }
        drop(frames_tx);
        let workers: Vec<_> = streams
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, stream)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    stream.send(&[index as u8; 4])
                })
            })
            .collect();
        barrier.wait();

        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.waiting_streams() != RUNS - 4 {
            assert!(
                Instant::now() < deadline,
                "28 runs must queue behind the four reserved frames"
            );
            assert!(
                hub.budgeted_bytes() <= 16,
                "global payload must never exceed 16 bytes"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            hub.budgeted_bytes(),
            16,
            "four frames must fill the shared budget"
        );

        let mut seen = HashSet::new();
        for _ in 0..RUNS {
            let (index, frame) = frames_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("each run must receive credit after preceding frames are ACKed");
            assert!(seen.insert(index), "each run must emit exactly one frame");
            assert_eq!(frame.run_id, format!("stress-{index}"));
            assert_eq!(frame.generation, 1);
            assert_eq!(frame.stream_epoch.to_string(), epochs[index]);
            assert_eq!(frame.offset.to_string(), "0");
            assert_eq!(frame.bytes, vec![index as u8; 4]);
            assert!(
                hub.budgeted_bytes() <= 16,
                "global payload must never exceed 16 bytes"
            );
            assert_eq!(
                hub.ack(&owner(), &ack(&frame.run_id, &epochs[index], "4"))
                    .unwrap(),
                4
            );
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(seen.len(), RUNS);
        assert_eq!(hub.waiting_streams(), 0);
        assert_eq!(hub.budgeted_bytes(), 0);
    }

    #[test]
    fn D27_Core_GlobalBudgetWaitersMakeFifoProgress_005() {
        let hub = Arc::new(TerminalTransports::with_limits(
            TransportLimits::new(4, 8, 4, 8).unwrap(),
        ));
        let a = hub
            .attach(owner(), run("a"), route(Arc::new(Mutex::new(Vec::new()))))
            .unwrap();
        let b_events = Arc::new(Mutex::new(Vec::new()));
        let b = hub.attach(owner(), run("b"), route(b_events.clone())).unwrap();
        let c_events = Arc::new(Mutex::new(Vec::new()));
        let c_stream = hub.attach(owner(), run("c"), route(c_events.clone())).unwrap();

        a.send(&[1, 1, 1, 1]).unwrap();
        a.send(&[2, 2, 2, 2]).unwrap();
        assert_eq!(hub.budgeted_bytes(), 8);

        let b_worker = {
            let b = b.clone();
            std::thread::spawn(move || b.send(&[3, 3, 3, 3]))
        };
        for _ in 0..100 {
            if hub.waiting_streams() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(hub.waiting_streams(), 1);
        let c_worker = {
            let c_stream = c_stream.clone();
            std::thread::spawn(move || c_stream.send(&[4, 4, 4, 4]))
        };
        for _ in 0..100 {
            if hub.waiting_streams() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(hub.waiting_streams(), 2);
        assert!(b_events.lock().is_empty());
        assert!(c_events.lock().is_empty());

        let epoch = a.stream_epoch().to_string();
        hub.ack(&owner(), &ack("a", &epoch, "4")).unwrap();
        for _ in 0..100 {
            if b_events.lock().len() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(b_events.lock().len(), 1);
        assert!(c_events.lock().is_empty());

        let b_epoch = b.stream_epoch().to_string();
        hub.ack(&owner(), &ack("b", &b_epoch, "4")).unwrap();
        assert!(b_worker.join().unwrap().is_ok());
        for _ in 0..100 {
            if c_events.lock().len() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(c_events.lock().len(), 1);
        assert!(c_worker.join().unwrap().is_ok());
    }

    #[test]
    fn D27_Core_RouteRevocationWakesBlockedSenderAndReleasesBudget_006() {
        let hub = Arc::new(TerminalTransports::with_limits(
            TransportLimits::new(4, 8, 4, 8).unwrap(),
        ));
        let a_route = route(Arc::new(Mutex::new(Vec::new())));
        let a = hub.attach(owner(), run("a"), a_route.clone()).unwrap();
        let b = hub
            .attach(owner(), run("b"), route(Arc::new(Mutex::new(Vec::new()))))
            .unwrap();

        a.send(&[1, 1, 1, 1]).unwrap();
        a.send(&[2, 2, 2, 2]).unwrap();
        assert_eq!(hub.budgeted_bytes(), 8);

        let worker = {
            let b = b.clone();
            std::thread::spawn(move || b.send(&[9, 9, 9, 9]))
        };
        for _ in 0..100 {
            if hub.waiting_streams() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(hub.waiting_streams(), 1);

        a_route.revoke();
        for _ in 0..100 {
            if hub.budgeted_bytes() <= 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert_eq!(hub.budgeted_bytes(), 4);
    }

    #[test]
    fn D27_Core_ManyRunsStayWithinLowBudgetAndConvergeToZero_007() {
        let hub = TerminalTransports::with_limits(
            TransportLimits::new(4, 8, 4, 16).unwrap(),
        );
        for index in 0..64 {
            let id = format!("run-{index}");
            let events = Arc::new(Mutex::new(Vec::new()));
            let stream = hub.attach(owner(), run(&id), route(events.clone())).unwrap();
            stream.send(&[index as u8, 1, 2, 3]).unwrap();
            assert!(hub.budgeted_bytes() <= 16);
            let epoch = stream.stream_epoch().to_string();
            hub.ack(&owner(), &ack(&id, &epoch, "4")).unwrap();
            assert_eq!(hub.budgeted_bytes(), 0);
            assert_eq!(events.lock().len(), 1);
        }
    }

    #[test]
    fn D27_Core_WrongOwnerAckCannotReleaseAnotherRunsCredit_008() {
        let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 8).unwrap());
        let stream = hub
            .attach(owner(), run("owned"), route(Arc::new(Mutex::new(Vec::new()))))
            .unwrap();
        stream.send(&[1, 2, 3, 4]).unwrap();
        let epoch = stream.stream_epoch().to_string();

        let mut wrong = owner();
        wrong.instance_id = "other-instance".into();
        assert_eq!(
            hub.ack(&wrong, &ack("owned", &epoch, "4"))
                .unwrap_err()
                .code,
            "FORBIDDEN"
        );
        assert_eq!(hub.budgeted_bytes(), 4);

        assert_eq!(hub.ack(&owner(), &ack("owned", &epoch, "4")).unwrap(), 4);
        assert_eq!(hub.budgeted_bytes(), 0);
    }

    #[test]
    fn D27_Core_DroppingBlockedOwnerReleasesBudgetForPeer_009() {
        let hub = Arc::new(TerminalTransports::with_limits(
            TransportLimits::new(4, 4, 0, 4).unwrap(),
        ));
        let a = hub
            .attach(owner(), run("a"), route(Arc::new(Mutex::new(Vec::new()))))
            .unwrap();
        let b_events = Arc::new(Mutex::new(Vec::new()));
        let b = hub.attach(owner(), run("b"), route(b_events.clone())).unwrap();
        a.send(&[1, 1, 1, 1]).unwrap();

        let worker = {
            let b = b.clone();
            std::thread::spawn(move || b.send(&[2, 2, 2, 2]))
        };
        for _ in 0..100 {
            if hub.waiting_streams() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(hub.waiting_streams(), 1);
        assert!(b_events.lock().is_empty());

        drop(a);
        for _ in 0..100 {
            if b_events.lock().len() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(b_events.lock().len(), 1);
        assert!(worker.join().unwrap().is_ok());
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
