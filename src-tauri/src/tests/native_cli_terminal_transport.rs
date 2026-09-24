use crate::cli::output_route::{OutputRoute, OutputRoutes};
use crate::cli::run_registry::RunKey;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::WireU64;
use crate::terminal_transport::{
    OutputAck, OutputFrame, OutputProgress, TerminalTransports, TransportLimits,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};

fn caller() -> CallerIdentity {
    CallerIdentity {
        instance_id: "backend-d14".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("7").unwrap(),
    }
}

fn run(id: &str, generation: u32) -> RunKey {
    RunKey {
        run_id: id.into(),
        generation,
    }
}

fn ack(run_id: &str, generation: u32, stream_epoch: &str, through: &str) -> OutputAck {
    serde_json::from_value(json!({
        "runId": run_id,
        "generation": generation,
        "streamEpoch": stream_epoch,
        "throughOffset": through
    }))
    .unwrap()
}

fn collecting_route(events: Arc<Mutex<Vec<Value>>>) -> Arc<OutputRoute<OutputFrame>> {
    Arc::new(
        OutputRoutes::new(1)
            .bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    let InvokeResponseBody::Json(text) = body else {
                        panic!("json output frame expected");
                    };
                    events.lock().push(serde_json::from_str(&text).unwrap());
                    Ok(())
                }))
            })
            .unwrap(),
    )
}

fn limits() -> TransportLimits {
    TransportLimits::new(4, 8, 4, 16).unwrap()
}

#[test]
fn D14_Transport_FramesAreBoundedOrderedAndByteExact_001() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let hub = TerminalTransports::with_limits(limits());
    let stream = hub
        .attach(caller(), run("run-a", 1), collecting_route(events.clone()))
        .unwrap();

    stream.send(&[0, 255, 27, 65]).unwrap();
    stream.send(&[66]).unwrap();

    assert_eq!(
        *events.lock(),
        vec![
            json!({"runId":"run-a","generation":1,"streamEpoch":"1","offset":"0","bytes":[0,255,27,65]}),
            json!({"runId":"run-a","generation":1,"streamEpoch":"1","offset":"4","bytes":[66]}),
        ]
    );
    assert_eq!(
        stream.send(&[1, 2, 3, 4, 5]).unwrap_err().code,
        "OUTPUT_FRAME_TOO_LARGE"
    );
}

#[test]
fn D14_Transport_AckOwnerEpochBoundaryAndCreditAreExact_002() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let hub = TerminalTransports::with_limits(limits());
    let owner = caller();
    let stream = hub
        .attach(owner.clone(), run("run-a", 3), collecting_route(events))
        .unwrap();
    stream.send(&[1, 2, 3, 4]).unwrap();
    stream.send(&[5, 6, 7, 8]).unwrap();

    let epoch = stream.stream_epoch().to_string();
    assert_eq!(hub.ack(&owner, &ack("run-a", 3, &epoch, "4")).unwrap(), 4);
    assert_eq!(hub.ack(&owner, &ack("run-a", 3, &epoch, "4")).unwrap(), 0);
    assert_eq!(
        hub.ack(&owner, &ack("run-a", 3, &epoch, "0"))
            .unwrap_err()
            .code,
        "OUTPUT_ACK_BACKWARD"
    );
    assert_eq!(
        hub.ack(&owner, &ack("run-a", 3, &epoch, "6"))
            .unwrap_err()
            .code,
        "OUTPUT_ACK_NOT_FRAME_BOUNDARY"
    );
    assert_eq!(
        hub.ack(&owner, &ack("run-a", 3, &epoch, "9"))
            .unwrap_err()
            .code,
        "OUTPUT_ACK_BEYOND_SENT"
    );
    assert_eq!(
        hub.ack(&owner, &ack("run-a", 3, "999", "8"))
            .unwrap_err()
            .code,
        "STALE_OUTPUT_STREAM"
    );

    let mut peer = owner.clone();
    peer.window_label = "peer".into();
    assert_eq!(
        hub.ack(&peer, &ack("run-a", 3, &epoch, "8"))
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    assert_eq!(hub.ack(&owner, &ack("run-a", 3, &epoch, "8")).unwrap(), 4);
}

struct ProbeReader {
    reads: Arc<AtomicUsize>,
    bytes: Vec<u8>,
}
impl Read for ProbeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let count = self.bytes.len().min(buffer.len());
        buffer[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes.drain(..count);
        Ok(count)
    }
}

#[test]
fn D14_Transport_BackpressureHappensBeforeReadAndPeerKeepsProgress_003() {
    let owner = caller();
    let hub = Arc::new(TerminalTransports::with_limits(limits()));
    let a_events = Arc::new(Mutex::new(Vec::new()));
    let b_events = Arc::new(Mutex::new(Vec::new()));
    let a = hub
        .attach(
            owner.clone(),
            run("run-a", 1),
            collecting_route(a_events.clone()),
        )
        .unwrap();
    let b = hub
        .attach(
            owner.clone(),
            run("run-b", 1),
            collecting_route(b_events.clone()),
        )
        .unwrap();

    a.send(&[1, 1, 1, 1]).unwrap();
    a.send(&[2, 2, 2, 2]).unwrap(); // run-a is at its 8-byte high-water mark
    b.send(&[9, 9, 9, 9]).unwrap(); // a slow run does not globally stall a peer
    assert_eq!(b_events.lock().len(), 1);

    let reads = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let worker = {
        let a = a.clone();
        let reads = reads.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut reader = ProbeReader {
                reads,
                bytes: vec![3, 3, 3, 3],
            };
            barrier.wait();
            a.pump_once(&mut reader)
        })
    };
    barrier.wait();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "reader ran before transport credit was reserved"
    );

    let epoch = a.stream_epoch().to_string();
    assert_eq!(hub.ack(&owner, &ack("run-a", 1, &epoch, "4")).unwrap(), 4);
    assert_eq!(worker.join().unwrap().unwrap(), 4);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(a_events.lock().len(), 3);
}

#[test]
fn D14_Transport_LostChannelIsFinalAndReleasesBudget_004() {
    let calls = Arc::new(AtomicUsize::new(0));
    let sent = calls.clone();
    let failing = Arc::new(
        OutputRoutes::new(1)
            .bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |_| {
                    sent.fetch_add(1, Ordering::SeqCst);
                    Err(std::io::Error::other("private channel failure").into())
                }))
            })
            .unwrap(),
    );
    let hub = TerminalTransports::with_limits(TransportLimits::new(4, 8, 4, 8).unwrap());
    let stream = hub.attach(caller(), run("run-a", 1), failing).unwrap();

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

#[derive(Default)]
struct ProgressProbe {
    events: Mutex<Vec<String>>,
}

impl OutputProgress for ProgressProbe {
    fn sent_through(&self, stream_epoch: WireU64, through: WireU64) {
        self.events
            .lock()
            .push(format!("sent:{stream_epoch}:{through}"));
    }

    fn parsed_through(&self, stream_epoch: WireU64, through: WireU64) {
        self.events
            .lock()
            .push(format!("parsed:{stream_epoch}:{through}"));
    }

    fn degraded(&self, stream_epoch: WireU64) {
        self.events.lock().push(format!("degraded:{stream_epoch}"));
    }
}

fn revocable_route(
    events: Arc<Mutex<Vec<Value>>>,
) -> (Arc<OutputRoutes>, Arc<OutputRoute<OutputFrame>>) {
    let routes = Arc::new(OutputRoutes::new(1));
    let route = Arc::new(
        routes
            .bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    let InvokeResponseBody::Json(text) = body else {
                        panic!("json output frame expected");
                    };
                    events.lock().push(serde_json::from_str(&text).unwrap());
                    Ok(())
                }))
            })
            .unwrap(),
    );
    (routes, route)
}

#[test]
fn D15_Transport_ProgressFollowsAcceptedSendParsedAckAndRevocation_005() {
    let owner = caller();
    let hub = TerminalTransports::with_limits(limits());
    let probe = Arc::new(ProgressProbe::default());
    let (routes, route) = revocable_route(Arc::new(Mutex::new(Vec::new())));
    let stream = hub
        .attach_observed(owner.clone(), run("run-a", 1), route, probe.clone())
        .unwrap();
    let epoch = stream.stream_epoch().to_string();

    stream.send(&[1, 2, 3, 4]).unwrap();
    assert_eq!(
        *probe.events.lock(),
        vec![format!("sent:{epoch}:4")],
        "lifecycle sent offset must advance only after the frame was accepted"
    );

    hub.ack(&owner, &ack("run-a", 1, &epoch, "4")).unwrap();
    assert_eq!(
        *probe.events.lock(),
        vec![format!("sent:{epoch}:4"), format!("parsed:{epoch}:4")]
    );

    routes.revoke();
    assert_eq!(
        *probe.events.lock(),
        vec![
            format!("sent:{epoch}:4"),
            format!("parsed:{epoch}:4"),
            format!("degraded:{epoch}"),
        ]
    );
}

#[test]
fn D15_Transport_RevokeWakesProducerBlockedOnRunCredit_006() {
    let owner = caller();
    let hub = Arc::new(TerminalTransports::with_limits(limits()));
    let probe = Arc::new(ProgressProbe::default());
    let (routes, route) = revocable_route(Arc::new(Mutex::new(Vec::new())));
    let stream = hub
        .attach_observed(owner, run("run-a", 1), route, probe.clone())
        .unwrap();

    stream.send(&[1, 1, 1, 1]).unwrap();
    stream.send(&[2, 2, 2, 2]).unwrap();

    let (tx, rx) = mpsc::channel();
    let blocked = stream.clone();
    let worker = std::thread::spawn(move || {
        let result = blocked.send(&[3, 3, 3, 3]).map_err(|failure| failure.code);
        tx.send(result).unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        rx.try_recv().is_err(),
        "producer unexpectedly crossed the high-water mark"
    );

    routes.revoke();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err("OUTPUT_STREAM_DEGRADED".into()),
        "document/channel loss must wake a producer already blocked on credit"
    );
    worker.join().unwrap();
    assert!(probe
        .events
        .lock()
        .iter()
        .any(|event| event.starts_with("degraded:")));
    assert_eq!(hub.budgeted_bytes(), 0);
}
