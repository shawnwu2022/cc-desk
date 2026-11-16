use super::*;
fn poll<T>(
    rx: mpsc::Receiver<T>,
) -> impl FnMut(Duration) -> Result<Result<T, Reject>, WaitError> + Send
where
    T: Send + 'static,
{
    move |budget| {
        assert!(budget.is_zero());
        rx.recv_timeout(budget).map(Ok).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
#[test]
fn native_pending_unpolled_drop_retains_original_receiver_001() {
    let owner = Owner::new();
    let (tx, rx) = mpsc::sync_channel(1);
    let waiter = owner.reserve::<u64>().unwrap().bind(poll(rx));
    drop(waiter);
    assert_eq!(owner.observe(), Observation::Pending);
    tx.send(31).unwrap(); // Fails if original receiver was dropped with waiter.
    assert_eq!(owner.observe(), Observation::Drained);
}
#[tokio::test]
async fn native_pending_cached_result_is_same_typed_original_002() {
    let owner = Owner::new();
    let (tx, rx) = mpsc::sync_channel(1);
    let waiter = owner.reserve::<u64>().unwrap().bind(poll(rx));
    tx.send(73).unwrap();
    assert_eq!(owner.observe(), Observation::Drained);
    assert_eq!(
        waiter
            .until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await,
        Ok(73)
    );
}
#[tokio::test]
async fn native_pending_deadline_and_late_result_retains_receiver_003() {
    let owner = Owner::new();
    let (tx, rx) = mpsc::sync_channel(1);
    let waiter = owner.reserve::<u64>().unwrap().bind(poll(rx));
    assert_eq!(
        waiter.until(tokio::time::Instant::now()).await,
        Err(Reject::Continuity)
    );
    assert_eq!(owner.observe(), Observation::Pending);
    tx.send(19).unwrap();
    assert_eq!(owner.observe(), Observation::Drained);
}
#[test]
fn native_pending_capacity_reserved_before_side_effect_004() {
    let owner = Owner::new();
    let mut held = Vec::new();
    for _ in 0..LIMIT {
        held.push(owner.reserve::<u64>().unwrap());
    }
    let mut effects = 0;
    if let Ok(reservation) = owner.reserve::<u64>() {
        effects += 1;
        drop(reservation);
    }
    assert_eq!(effects, 0);
    assert!(matches!(owner.reserve::<u64>(), Err(Reject::Full)));
    drop(held);
    assert_eq!(owner.observe(), Observation::Drained);
    assert!(owner.reserve::<u64>().is_ok());
}
#[test]
fn native_pending_closed_receiver_remains_unknown_005() {
    let owner = Owner::new();
    let (tx, rx) = mpsc::sync_channel::<u64>(1);
    let waiter = owner.reserve().unwrap().bind(poll(rx));
    drop(tx);
    drop(waiter);
    assert_eq!(owner.observe(), Observation::Unknown);
    assert_eq!(owner.observe(), Observation::Unknown);
}
#[test]
fn native_pending_reservation_without_enqueue_is_reclaimable_006() {
    let owner = Owner::new();
    let reservation = owner.reserve::<u64>().unwrap();
    assert_eq!(owner.observe(), Observation::Pending);
    drop(reservation);
    assert_eq!(owner.observe(), Observation::Drained);
}
#[tokio::test]
async fn native_pending_different_operations_cannot_swap_results_007() {
    let owner = Owner::new();
    let (a, ar) = mpsc::sync_channel(1);
    let (b, br) = mpsc::sync_channel(1);
    let aw = owner.reserve::<u64>().unwrap().bind(poll(ar));
    let bw = owner.reserve::<u64>().unwrap().bind(poll(br));
    b.send(2).unwrap();
    a.send(1).unwrap();
    assert_eq!(owner.observe(), Observation::Drained);
    let end = tokio::time::Instant::now() + Duration::from_secs(1);
    assert_eq!(aw.until(end).await, Ok(1));
    assert_eq!(bw.until(end).await, Ok(2));
}
#[test]
fn native_pending_seal_rejects_new_but_retains_reserved_008() {
    let owner = Owner::new();
    let reservation = owner.reserve::<u64>().unwrap();
    owner.seal();
    assert!(matches!(owner.reserve::<u64>(), Err(Reject::Sealed)));
    let (tx, rx) = mpsc::sync_channel(1);
    let waiter = reservation.bind(poll(rx));
    drop(waiter);
    assert_eq!(owner.observe(), Observation::Pending);
    tx.send(5).unwrap();
    assert_eq!(owner.observe(), Observation::Drained);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn ChannelCustody_SharedBudgetPreservesUntakenResult_009() {
    let budget = Arc::new(Budget::new());
    let a = Owner::with_budget(budget.clone());
    let b = Owner::with_budget(budget);
    let mut held = Vec::new();
    for _ in 0..(LIMIT - 1) {
        held.push(a.reserve::<u64>().unwrap());
    }
    let (send, receive) = mpsc::sync_channel(1);
    let waiter = b.reserve::<u64>().unwrap().bind(poll(receive));
    assert!(matches!(b.reserve::<u64>(), Err(Reject::Full)));
    send.send(29).unwrap();
    assert_eq!(b.observe(), Observation::Drained);
    // 仅终局尚未从索引退休，不会偷偷释放其他 owner 的共享额度。
    assert!(matches!(a.reserve::<u64>(), Err(Reject::Full)));
    b.seal();
    b.prune_drained().unwrap();
    assert!(matches!(a.reserve::<u64>(), Err(Reject::Full)));
    assert!(matches!(b.reserve::<u64>(), Err(Reject::Sealed)));
    assert_eq!(
        waiter
            .until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await,
        Ok(29)
    );
    let extra = a.reserve::<u64>().unwrap();
    drop(extra);
    drop(held);
    a.prune_drained().unwrap();
    assert!(a.reserve::<u64>().is_ok());
}

#[tokio::test]
#[allow(non_snake_case)]
async fn ChannelCustody_AllReadyUntakenResultsKeepSharedCredits_010() {
    let budget = Arc::new(Budget::new());
    let a = Owner::with_budget(budget.clone());
    let b = Owner::with_budget(budget);
    let mut waits = Vec::new();
    for value in 0..LIMIT {
        let (send, receive) = mpsc::sync_channel(1);
        waits.push(a.reserve::<usize>().unwrap().bind(poll(receive)));
        send.send(value).unwrap();
    }
    assert_eq!(a.observe(), Observation::Drained);
    a.seal();
    a.prune_drained().unwrap();
    assert!(matches!(b.reserve::<usize>(), Err(Reject::Full)));
    let last = waits.pop().unwrap();
    assert_eq!(
        last.until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await,
        Ok(LIMIT - 1)
    );
    let slot = b.reserve::<usize>().unwrap();
    assert!(matches!(b.reserve::<usize>(), Err(Reject::Full)));
    drop(waits);
    assert!(b.reserve::<usize>().is_ok());
    drop(slot);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn HistoryCustody_ReadyExternalWaitKeepsCreditAndHistory_011() {
    let owner = Owner::new();
    let wait = owner.reserve::<u32>().unwrap().bind(|_| Ok(Ok(19)));
    owner.seal();
    assert_eq!(owner.observe(), Observation::Drained);
    owner.prune_drained().unwrap();
    assert!(!owner.history_reclaimable());
    assert_eq!(owner.budget.0.load(Ordering::Acquire), 1);
    assert_eq!(
        wait.until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await,
        Ok(19)
    );
    assert!(owner.history_reclaimable());
    assert_eq!(owner.budget.0.load(Ordering::Acquire), 0);
}
#[test]
#[allow(non_snake_case)]
fn HistoryCustody_DroppedPendingWaitDoesNotReleaseHistory_012() {
    let owner = Owner::new();
    let wait = owner
        .reserve::<u32>()
        .unwrap()
        .bind(|_| Err(WaitError::Uncertain));
    owner.seal();
    drop(wait);
    assert!(!owner.history_reclaimable());
    assert_eq!(owner.budget.0.load(Ordering::Acquire), 1);
}
