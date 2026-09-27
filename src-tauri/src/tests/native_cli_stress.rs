use crate::cli::output_route::{OutputRoute, OutputRoutes};
use crate::cli::run_registry::RunKey;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::WireU64;
use crate::terminal_transport::{OutputAck, OutputFrame, TerminalTransports, TransportLimits};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};

fn caller() -> CallerIdentity {
    CallerIdentity {
        instance_id: "backend-d27".into(),
        window_label: "main".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    }
}

fn run(index: usize) -> RunKey {
    RunKey {
        run_id: format!("stress-run-{index}"),
        generation: 1,
    }
}

fn ack(index: usize, epoch: &str) -> OutputAck {
    serde_json::from_value(json!({
        "runId": format!("stress-run-{index}"),
        "generation": 1,
        "streamEpoch": epoch,
        "throughOffset": "4"
    }))
    .unwrap()
}

fn notifying_route(
    index: usize,
    tx: mpsc::Sender<(usize, Value)>,
) -> Arc<OutputRoute<OutputFrame>> {
    Arc::new(
        OutputRoutes::new(1)
            .bind(1, Box::new(|| Ok(())), move || {
                let tx = tx.clone();
                Ok(Channel::new(move |body| {
                    let InvokeResponseBody::Json(text) = body else {
                        panic!("json output frame expected");
                    };
                    tx.send((index, serde_json::from_str(&text).unwrap()))
                        .unwrap();
                    Ok(())
                }))
            })
            .unwrap(),
    )
}

#[test]
fn D27_Transport_ThirtyTwoRunsProgressUnderTinyGlobalBudget_001() {
    const RUNS: usize = 32;
    let owner = caller();
    let hub = Arc::new(TerminalTransports::with_limits(
        TransportLimits::new(4, 8, 4, 16).unwrap(),
    ));
    let (tx, rx) = mpsc::channel();

    let mut streams = Vec::new();
    let mut epochs = Vec::new();
    for index in 0..RUNS {
        let stream = hub
            .attach(
                owner.clone(),
                run(index),
                notifying_route(index, tx.clone()),
            )
            .unwrap();
        epochs.push(stream.stream_epoch().to_string());
        streams.push(stream);
    }

    let mut workers = Vec::new();
    for (index, stream) in streams.iter().cloned().enumerate() {
        workers.push(std::thread::spawn(move || stream.send(&[index as u8; 4])));
    }

    let mut seen = HashSet::new();
    for _ in 0..RUNS {
        let (index, frame) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("all runs must eventually receive global budget");
        assert!(seen.insert(index), "a run emitted more than one frame");
        assert_eq!(frame["runId"], format!("stress-run-{index}"));
        assert_eq!(frame["bytes"], json!(vec![index as u8; 4]));
        assert!(hub.budgeted_bytes() <= 16);
        hub.ack(&owner, &ack(index, &epochs[index])).unwrap();
    }

    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    assert_eq!(seen.len(), RUNS);
    assert_eq!(hub.budgeted_bytes(), 0);
}

#[test]
fn D27_Transport_BrokenRunReleasesBudgetAndPeerStillProgresses_002() {
    let owner = caller();
    let hub = TerminalTransports::with_limits(TransportLimits::new(4, 4, 4, 4).unwrap());

    let failing = Arc::new(
        OutputRoutes::new(1)
            .bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |_| {
                    Err(std::io::Error::other("synthetic route failure").into())
                }))
            })
            .unwrap(),
    );
    let broken = hub.attach(owner.clone(), run(0), failing).unwrap();
    assert_eq!(
        broken.send(&[1, 1, 1, 1]).unwrap_err().code,
        "OUTPUT_ROUTE_LOST"
    );
    assert_eq!(hub.budgeted_bytes(), 0);

    let (tx, rx) = mpsc::channel();
    let peer = hub
        .attach(owner.clone(), run(1), notifying_route(1, tx))
        .unwrap();
    let epoch = peer.stream_epoch().to_string();
    peer.send(&[9, 9, 9, 9]).unwrap();
    let (index, frame) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(index, 1);
    assert_eq!(frame["bytes"], json!([9, 9, 9, 9]));
    hub.ack(&owner, &ack(1, &epoch)).unwrap();
    assert_eq!(hub.budgeted_bytes(), 0);
}

#[test]
fn D27_Transport_QueuedRunsDoNotBypassAppBudget_003() {
    let owner = caller();
    let hub = Arc::new(TerminalTransports::with_limits(
        TransportLimits::new(4, 8, 4, 8).unwrap(),
    ));
    let (tx, rx) = mpsc::channel();

    let mut streams = Vec::new();
    let mut epochs = Vec::new();
    for index in 0..4 {
        let stream = hub
            .attach(
                owner.clone(),
                run(index),
                notifying_route(index, tx.clone()),
            )
            .unwrap();
        epochs.push(stream.stream_epoch().to_string());
        streams.push(stream);
    }

    streams[0].send(&[0; 4]).unwrap();
    streams[1].send(&[1; 4]).unwrap();
    assert_eq!(hub.budgeted_bytes(), 8);

    let mut workers = Vec::new();
    for (index, stream) in streams.iter().cloned().enumerate().take(4).skip(2) {
        workers.push(std::thread::spawn(move || stream.send(&[index as u8; 4])));
    }

    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(hub.budgeted_bytes(), 8);

    hub.ack(&owner, &ack(0, &epochs[0])).unwrap();
    let (first, _) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (second, _) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let initial: HashSet<_> = [0usize, 1usize].into_iter().collect();
    let observed: HashSet<_> = [first, second].into_iter().collect();
    assert_eq!(observed, initial);

    let (released_index, _) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(released_index == 2 || released_index == 3);
    assert!(hub.budgeted_bytes() <= 8);

    hub.ack(&owner, &ack(1, &epochs[1])).unwrap();
    let (other_index, _) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_ne!(other_index, released_index);
    assert!(other_index == 2 || other_index == 3);

    hub.ack(&owner, &ack(released_index, &epochs[released_index]))
        .unwrap();
    hub.ack(&owner, &ack(other_index, &epochs[other_index]))
        .unwrap();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    assert_eq!(hub.budgeted_bytes(), 0);
}
