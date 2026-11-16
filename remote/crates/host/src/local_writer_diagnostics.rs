//! Test-only decision metadata; never compiled into product builds.
use super::*;
thread_local! {static WRITE_ATTEMPTS:std::cell::Cell<Option<u64>>=const{std::cell::Cell::new(None)};}
pub(super) fn record_write_attempt() {
    WRITE_ATTEMPTS.with(|v| v.set(Some(v.get().unwrap_or(0).saturating_add(1))));
}
#[test]
fn counter_without_fixture_instrumentation_is_unknown_not_zero() {
    std::thread::spawn(|| {
        assert_eq!(WRITE_ATTEMPTS.with(|v| v.get()), None);
        record_write_attempt();
        assert_eq!(WRITE_ATTEMPTS.with(|v| v.get()), Some(1));
        record_write_attempt();
        assert_eq!(WRITE_ATTEMPTS.with(|v| v.get()), Some(2));
    })
    .join()
    .unwrap();
}
#[derive(Debug)]
pub(super) enum Stage {
    Validate,
    View,
    Final,
}
#[derive(Debug)]
pub(super) enum Decision {
    Reject(Reject),
    Outcome(Outcome),
    Journal(JournalError),
    Partial,
    Unknown,
}
#[derive(Debug)]
enum Status {
    Known(Outcome),
    Missing,
    TooOld,
    Error(JournalError),
}
pub(super) fn emit(
    request: &Request,
    stage: Stage,
    decision: Decision,
    replies: Option<&ReplyState>,
    journal: &Journal,
) {
    if request.key.run != "TEST_ONLY_BOUND_PTY" {
        return;
    }
    let status = match journal.query(&request.key) {
        Ok(ReceiptLookup::Known(r)) => Status::Known(r.outcome),
        Ok(ReceiptLookup::Missing) => Status::Missing,
        Ok(ReceiptLookup::TooOld) => Status::TooOld,
        Err(e) => Status::Error(e),
    };
    let expected = request.view.as_ref().map(|v| v.diagnostic_epochs());
    let current = replies.and_then(ReplyState::epochs);
    let attempts = WRITE_ATTEMPTS.with(|v| v.get());
    // Only sequence and closed enums/epochs: never IDs, bytes, digest, path/token.
    eprintln!(
        "TEST_ONLY_WRITER_DECISION seq={} stage={stage:?} decision={decision:?} expected={expected:?} current={current:?} exact_receipt={status:?} observed_fixture_write_attempts={attempts:?}",
        request.key.sequence
    );
}
