use crate::cli::run_registry::RunKey;
use crate::cli::terminal_transport::{
    OutputAck, RunOutputFlow, TransportBudget, APPLICATION_PAYLOAD_BUDGET, MAX_FRAME_BYTES,
    RUN_HIGH_WATERMARK, RUN_LOW_WATERMARK,
};
use crate::cli::types::WireU64;

fn run(id: &str, generation: u32) -> RunKey {
    RunKey { run_id: id.into(), generation }
}

fn ack(run: &RunKey, epoch: &str, through: usize) -> OutputAck {
    OutputAck {
        run_id: run.run_id.clone(),
        generation: run.generation,
        stream_epoch: WireU64::parse(epoch).unwrap(),
        through_offset: WireU64::parse(&through.to_string()).unwrap(),
    }
}

#[test]
fn D14_Transport_FrameBoundariesAndExactBytes_001() {
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let run = run("bytes-run", 3);
    let flow = RunOutputFlow::new(run.clone(), WireU64::parse("11").unwrap(), budget.clone());
    let payload = vec![0, 255, 27, 91, 50, 48, 48, 126, 0xf0, 0x9f, 0x98, 0x80];

    let permit = flow.try_reserve().unwrap();
    let frame = permit.commit(payload.clone()).unwrap();

    assert_eq!(frame.run_id, run.run_id);
    assert_eq!(frame.generation, 3);
    assert_eq!(frame.stream_epoch.to_string(), "11");
    assert_eq!(frame.offset.to_string(), "0");
    assert_eq!(frame.bytes, payload);
    assert_eq!(flow.sent_offset(), payload.len() as u64);
    assert_eq!(flow.outstanding_bytes(), payload.len());
    assert_eq!(budget.reserved_bytes(), payload.len());
}

#[test]
fn D14_Transport_AckDuplicateRegressionOverflowAndBoundary_002() {
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let run = run("ack-run", 1);
    let flow = RunOutputFlow::new(run.clone(), WireU64::parse("7").unwrap(), budget.clone());

    let first = flow.try_reserve().unwrap().commit(vec![1; 7]).unwrap();
    let second = flow.try_reserve().unwrap().commit(vec![2; 5]).unwrap();
    assert_eq!(first.offset.to_string(), "0");
    assert_eq!(second.offset.to_string(), "7");

    flow.ack(&ack(&run, "7", 7)).unwrap();
    assert_eq!(flow.outstanding_bytes(), 5);
    assert_eq!(budget.reserved_bytes(), 5);

    flow.ack(&ack(&run, "7", 7)).unwrap();
    assert_eq!(budget.reserved_bytes(), 5, "duplicate ACK released credit twice");

    assert_eq!(flow.ack(&ack(&run, "7", 6)).unwrap_err().code, "ACK_REGRESSION");
    assert_eq!(flow.ack(&ack(&run, "7", 8)).unwrap_err().code, "ACK_NOT_FRAME_BOUNDARY");
    assert_eq!(flow.ack(&ack(&run, "7", 13)).unwrap_err().code, "ACK_OUT_OF_RANGE");

    flow.ack(&ack(&run, "7", 12)).unwrap();
    assert_eq!(flow.outstanding_bytes(), 0);
    assert_eq!(budget.reserved_bytes(), 0);
}

#[test]
fn D14_Transport_OldEpochAndWrongRunRejected_003() {
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let run = run("owner-run", 2);
    let flow = RunOutputFlow::new(run.clone(), WireU64::parse("19").unwrap(), budget);
    flow.try_reserve().unwrap().commit(vec![1, 2, 3]).unwrap();

    assert_eq!(flow.ack(&ack(&run, "18", 3)).unwrap_err().code, "STALE_STREAM_EPOCH");
    let other = run("other-run", 2);
    assert_eq!(flow.ack(&ack(&other, "19", 3)).unwrap_err().code, "FORBIDDEN");
    let stale_generation = run("owner-run", 1);
    assert_eq!(flow.ack(&ack(&stale_generation, "19", 3)).unwrap_err().code, "STALE_GENERATION");
    assert_eq!(flow.outstanding_bytes(), 3);
}

#[test]
fn D14_Transport_HighLowWatermarkResumesOnlyAtLow_004() {
    assert_eq!(RUN_HIGH_WATERMARK % MAX_FRAME_BYTES, 0);
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let run = run("pressure-run", 1);
    let flow = RunOutputFlow::new(run.clone(), WireU64::parse("2").unwrap(), budget);

    for _ in 0..(RUN_HIGH_WATERMARK / MAX_FRAME_BYTES) {
        flow.try_reserve().unwrap().commit(vec![9; MAX_FRAME_BYTES]).unwrap();
    }
    assert_eq!(flow.outstanding_bytes(), RUN_HIGH_WATERMARK);
    assert_eq!(flow.try_reserve().unwrap_err().code, "OUTPUT_BACKPRESSURE");
    assert!(flow.is_paused());

    let through = RUN_HIGH_WATERMARK - RUN_LOW_WATERMARK;
    flow.ack(&ack(&run, "2", through)).unwrap();
    assert_eq!(flow.outstanding_bytes(), RUN_LOW_WATERMARK);
    assert!(!flow.is_paused());
    drop(flow.try_reserve().unwrap());
}

#[test]
fn D14_Transport_GlobalBudgetAndPermitRollback_005() {
    let budget = TransportBudget::new(MAX_FRAME_BYTES);
    let a = RunOutputFlow::new(run("a", 1), WireU64::parse("1").unwrap(), budget.clone());
    let b = RunOutputFlow::new(run("b", 1), WireU64::parse("2").unwrap(), budget.clone());

    let held = a.try_reserve().unwrap();
    assert_eq!(budget.reserved_bytes(), MAX_FRAME_BYTES);
    assert_eq!(b.try_reserve().unwrap_err().code, "OUTPUT_BUDGET_EXHAUSTED");

    drop(held);
    assert_eq!(budget.reserved_bytes(), 0);
    let frame = b.try_reserve().unwrap().commit(vec![4; 17]).unwrap();
    assert_eq!(frame.bytes.len(), 17);
    assert_eq!(budget.reserved_bytes(), 17);
}

#[test]
fn D14_Transport_DegradedReleasesPayloadAndNeverReopens_006() {
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let flow = RunOutputFlow::new(run("lost", 1), WireU64::parse("8").unwrap(), budget.clone());
    flow.try_reserve().unwrap().commit(vec![7; 100]).unwrap();
    assert_eq!(budget.reserved_bytes(), 100);

    flow.degrade();
    assert!(flow.is_degraded());
    assert_eq!(flow.outstanding_bytes(), 0);
    assert_eq!(budget.reserved_bytes(), 0);
    assert_eq!(flow.try_reserve().unwrap_err().code, "OUTPUT_DEGRADED");
}

#[test]
fn D14_Transport_TwoRunsDoNotShareCreditOrOffsets_007() {
    let budget = TransportBudget::new(APPLICATION_PAYLOAD_BUDGET);
    let a_run = run("run-a", 1);
    let b_run = run("run-b", 1);
    let a = RunOutputFlow::new(a_run.clone(), WireU64::parse("3").unwrap(), budget.clone());
    let b = RunOutputFlow::new(b_run.clone(), WireU64::parse("4").unwrap(), budget.clone());

    let af = a.try_reserve().unwrap().commit(vec![1; 10]).unwrap();
    let bf = b.try_reserve().unwrap().commit(vec![2; 6]).unwrap();
    assert_eq!(af.offset.to_string(), "0");
    assert_eq!(bf.offset.to_string(), "0");

    a.ack(&ack(&a_run, "3", 10)).unwrap();
    assert_eq!(a.outstanding_bytes(), 0);
    assert_eq!(b.outstanding_bytes(), 6);
    assert_eq!(budget.reserved_bytes(), 6);
}
