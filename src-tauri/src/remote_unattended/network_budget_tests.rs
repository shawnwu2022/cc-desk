//! 固定原生预算边界，不用这些受控时钟测试冒充真实等待。
#![allow(non_snake_case)]
use super::*;
#[test]
fn NetworkBudget_Local60AndNetwork900StayDistinct_001() {
    for network in [false, true] {
        let (policy, current) = Delegation::test_new([7; 32], false);
        policy.test_budget_total(1_200_000);
        let before = policy.sample().unwrap();
        let _attempt = if network {
            Some(policy.begin_network(&current, || true).unwrap())
        } else {
            let _local = policy.begin(&current).unwrap();
            None
        };
        let state = policy.owner.state.lock().unwrap();
        let live = state.live.as_ref().unwrap();
        let expected = if network { 900_000 } else { 60_000 };
        assert_eq!(live.wall_end, before.wall_ms + expected);
        assert_eq!(
            live.end.duration_since(before.monotonic),
            Duration::from_millis(expected)
        );
    }
}
#[test]
fn NetworkBudget_AttemptElapsedKeepsLiveAndRequiresReceipt_002() {
    let (policy, current) = Delegation::test_new([7; 32], false);
    policy.test_budget_total(1_200_000);
    let attempt = policy.begin_network(&current, || true).unwrap();
    let clock = Arc::new(NetworkClock::new());
    clock.bind(&attempt).unwrap();
    let budget = clock.budget(attempt.clone()).unwrap();
    policy.test_budget_advance(60_001);
    assert!(budget.check().is_ok());
    policy.test_budget_advance(839_999);
    assert_eq!(clock.now_ms(), Err(GrantError::Closed));
    assert!(!clock.is_closed());
    assert!(!policy.closed.load(Ordering::Acquire));
    assert_eq!(
        policy
            .owner
            .state
            .lock()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .phase,
        Phase::Retiring
    );
    assert!(matches!(
        policy.begin_network(&current, || true),
        Err(Denied::Busy)
    ));
    assert!(clock.bind(&attempt).is_err());
    assert!(budget.check().is_err());
}
#[test]
fn NetworkBudget_TotalCutoffAndRollbackArePermanent_003() {
    for rollback in [false, true] {
        let (policy, current) = Delegation::test_new([7; 32], false);
        let attempt = policy.begin_network(&current, || true).unwrap();
        let clock = Arc::new(NetworkClock::new());
        clock.bind(&attempt).unwrap();
        let budget = clock.budget(attempt).unwrap();
        assert_eq!(budget.wall_end, 31000);
        if rollback {
            policy.test_clock(false);
        } else {
            policy.test_budget_advance(30_000);
        }
        assert!(budget.check().is_err());
        assert!(clock.is_closed());
        policy.test_restore_clock();
        assert!(clock.now_ms().is_err());
        assert!(policy.begin_network(&current, || true).is_err());
    }
}
#[test]
fn NetworkBudget_LateBindingRetainsOriginalFixedEnds_004() {
    let (policy, current) = Delegation::test_new([7; 32], false);
    policy.test_budget_total(1_200_000);
    let attempt = policy.begin_network(&current, || true).unwrap();
    let clock = Arc::new(NetworkClock::new());
    clock.bind(&attempt).unwrap();
    let original = clock.budget(attempt.clone()).unwrap();
    policy.test_budget_advance(7_900);
    let later = clock.budget(attempt).unwrap();
    assert_eq!(original.wall_end, later.wall_end);
    assert_eq!(original.end, later.end);
    assert_eq!(later.wall_end - policy.sample().unwrap().wall_ms, 892_100);
    let foreign = Arc::new(NetworkClock::new());
    assert!(foreign.budget(original.attempt.clone()).is_err());
}
