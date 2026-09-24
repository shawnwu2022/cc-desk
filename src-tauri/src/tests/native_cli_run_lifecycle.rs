use crate::cli::run_registry::RunKey;
use crate::run_lifecycle::{LifecycleRecord, OutputLifecycle, ProcessLifecycle};

fn run(id: &str, generation: u32) -> RunKey {
    RunKey {
        run_id: id.into(),
        generation,
    }
}

#[test]
fn D15_Lifecycle_ProcessExitDoesNotMeanOutputDrained_001() {
    let mut state = LifecycleRecord::new(run("run-a", 1));
    state.process_running().unwrap();
    state.output_started("9").unwrap();
    state.sent_through("8").unwrap();

    state.process_exited().unwrap();
    assert_eq!(state.process(), ProcessLifecycle::Exited);
    assert_eq!(state.output(), OutputLifecycle::Draining);
    assert!(!state.can_retire());

    state.output_end("8").unwrap();
    assert_eq!(state.output(), OutputLifecycle::Draining);
    assert!(!state.can_retire());

    state.parsed_through("4").unwrap();
    assert_eq!(state.output(), OutputLifecycle::Draining);
    assert!(!state.can_retire());

    state.parsed_through("8").unwrap();
    assert_eq!(state.output(), OutputLifecycle::Drained);
    assert!(state.can_retire());
}

#[test]
fn D15_Lifecycle_OutputEndCannotInventBytesOrMoveBackward_002() {
    let mut state = LifecycleRecord::new(run("run-a", 1));
    state.process_running().unwrap();
    state.output_started("3").unwrap();
    state.sent_through("12").unwrap();

    assert_eq!(
        state.output_end("13").unwrap_err().code,
        "OUTPUT_END_BEYOND_SENT"
    );
    assert_eq!(
        state.parsed_through("13").unwrap_err().code,
        "OUTPUT_ACK_BEYOND_SENT"
    );
    state.parsed_through("8").unwrap();
    assert_eq!(
        state.parsed_through("7").unwrap_err().code,
        "OUTPUT_ACK_BACKWARD"
    );
    state.output_end("12").unwrap();
    assert_eq!(
        state.output_end("11").unwrap_err().code,
        "OUTPUT_END_CONFLICT"
    );
}

#[test]
fn D15_Lifecycle_DegradedAndIncompleteNeverBecomeFalseDrained_003() {
    let mut degraded = LifecycleRecord::new(run("run-a", 1));
    degraded.process_running().unwrap();
    degraded.output_started("1").unwrap();
    degraded.sent_through("4").unwrap();
    degraded.mark_degraded().unwrap();
    degraded.process_exited().unwrap();
    degraded.output_end("4").unwrap();
    degraded.parsed_through("4").unwrap();
    assert_eq!(degraded.output(), OutputLifecycle::Degraded);
    assert!(!degraded.can_retire_as_complete());

    let mut incomplete = LifecycleRecord::new(run("run-b", 2));
    incomplete.process_running().unwrap();
    incomplete.output_started("2").unwrap();
    incomplete.sent_through("4").unwrap();
    incomplete.mark_incomplete().unwrap();
    incomplete.process_exited().unwrap();
    assert_eq!(incomplete.output(), OutputLifecycle::Incomplete);
    assert!(!incomplete.can_retire_as_complete());
}

#[test]
fn D15_Lifecycle_StaleStreamEpochCannotFinishNewStream_004() {
    let mut state = LifecycleRecord::new(run("run-a", 2));
    state.process_running().unwrap();
    state.output_started("10").unwrap();
    state.sent_through("5").unwrap();
    assert_eq!(
        state.output_end_for("9", "5").unwrap_err().code,
        "STALE_OUTPUT_STREAM"
    );
    assert_eq!(
        state.parsed_through_for("9", "5").unwrap_err().code,
        "STALE_OUTPUT_STREAM"
    );
    assert_eq!(state.output(), OutputLifecycle::Open);
}

#[test]
fn D15_Lifecycle_HandoffFailureBeforeStreamCanRetireIncomplete_005() {
    let mut state = LifecycleRecord::new(run("run-a", 1));
    state.process_running().unwrap();
    state.mark_incomplete().unwrap();
    state.process_exited().unwrap();

    assert_eq!(state.output(), OutputLifecycle::Incomplete);
    assert!(state.can_retire());
    assert!(!state.can_retire_as_complete());
}
