use std::{cell::{Cell, RefCell}, io, time::Duration};

#[test]
fn bounded_job_drain_preserves_original_worker_window() {
    let ms = Duration::from_millis;
    assert_eq!(policy::owned_job_remaining_ms(ms(0), ms(0)), 5000);
    assert_eq!(policy::owned_job_remaining_ms(ms(299_997), ms(1)), 3);
    assert_eq!(policy::owned_job_remaining_ms(ms(1), ms(5000)), 0);
    assert_eq!(policy::owned_job_remaining_ms(ms(300_000), ms(0)), 0);
    assert_eq!(policy::owned_job_remaining_ms(Duration::MAX, ms(0)), 0);
}

#[test]
fn accounting_lag_requires_a_successful_empty_observation() {
    let remaining = Cell::new(30);
    let queries = Cell::new(0);
    let pauses = RefCell::new(Vec::new());
    policy::await_owned_job_empty(
        || remaining.get(),
        |ms| { pauses.borrow_mut().push(ms); remaining.set(remaining.get() - ms); },
        || { queries.set(queries.get() + 1); Ok(if queries.get() == 1 { 1 } else { 0 }) },
    ).unwrap();
    assert_eq!(queries.get(), 2);
    assert_eq!(*pauses.borrow(), [10]);
}

#[test]
fn remaining_members_fail_at_bounded_deadline() {
    let remaining = Cell::new(15);
    let queries = Cell::new(0);
    let error = policy::await_owned_job_empty(
        || remaining.get(), |ms| remaining.set(remaining.get() - ms),
        || { queries.set(queries.get() + 1); Ok(2) },
    ).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(queries.get(), 2);
    assert_eq!(remaining.get(), 0);
}

#[test]
fn accounting_query_error_cannot_become_empty() {
    let error = policy::await_owned_job_empty(
        || 20, |_| panic!("query error must not retry or pause"),
        || Err(io::Error::new(io::ErrorKind::PermissionDenied, "query rejected")),
    ).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn expired_budget_and_late_zero_cannot_pass() {
    let error = policy::await_owned_job_empty(
        || 0, |_| panic!("expired pause"), || panic!("expired query"),
    ).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    let remaining = Cell::new(1);
    let error = policy::await_owned_job_empty(
        || remaining.get(), |_| panic!("late zero must fail immediately"),
        || { remaining.set(0); Ok(0) },
    ).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
