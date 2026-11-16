use super::*;
use crate::{
    GrantGate, RunningChannel,
    data_tests::pair,
    grant_tests::{admin, scope, signed},
};
use std::sync::atomic::AtomicU64;
fn gate(h: &ManagedChannel<tokio::io::DuplexStream>, now: Arc<AtomicU64>) -> GrantGate {
    GrantGate::for_binding(
        h.context(),
        admin(),
        [4; 32],
        Arc::new(move || Ok(now.load(Ordering::Acquire))),
    )
    .unwrap()
}
fn access(g: &mut GrantGate) -> VerifiedGrant {
    let value = g
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    g.accept(&signed(&value, g.issuer_kid())).unwrap()
}
async fn raw(c: &mut ManagedChannel<tokio::io::DuplexStream>, header: DataHeader) {
    let bytes = grant::encode_data(&header, b"PUBLIC_UNTRUSTED_OPERATION").unwrap();
    let bytes = cc_desk_remote_record::encode(Kind::Data, 1, &bytes).unwrap();
    c.io.as_mut().unwrap().write_all(&bytes).await.unwrap();
    c.io.as_mut().unwrap().flush().await.unwrap();
}
async fn running() -> (
    RunningChannel,
    RunningChannel,
    GrantGate,
    crate::CancelHandle,
) {
    let (mut c, mut h, cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let a = access(&mut g);
    c.install(&a).unwrap();
    h.install(&a).unwrap();
    (c.start().unwrap(), h.start().unwrap(), g, cancel)
}
#[tokio::test]
async fn replay_old_fence_binding_grant_scope_or_direction_cannot_dispatch() {
    for case in 0..6 {
        let (mut c, mut h, _cancel) = pair().await;
        let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
        let a = access(&mut g);
        h.install(&a).unwrap();
        let h = h.start().unwrap();
        let mut header = DataHeader {
            binding: c.binding,
            grant: a.id(),
            scope: a.scope_digest(),
            fence: 4,
            sequence: 1,
            kind: Kind::Control,
        };
        match case {
            0 => {
                raw(&mut c, header.clone()).await;
                h.receive(&scope(), 4, Duration::from_secs(1))
                    .await
                    .unwrap();
            }
            1 => header.fence = 3,
            2 => header.binding[0] ^= 1,
            3 => header.grant[0] ^= 1,
            4 => header.scope[0] ^= 1,
            _ => header.kind = Kind::Projection,
        }
        raw(&mut c, header).await;
        assert!(
            h.receive(&scope(), 4, Duration::from_secs(1))
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(1), h.wait_closed())
            .await
            .unwrap();
        assert!(h.is_closed());
    }
}
#[tokio::test]
async fn captured_plaintext_from_a_previous_tls_channel_cannot_replay_after_attach() {
    let (c, h, _cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let a = access(&mut g);
    let old = DataHeader {
        binding: c.binding,
        grant: a.id(),
        scope: a.scope_digest(),
        fence: 4,
        sequence: 1,
        kind: Kind::Control,
    };
    let (mut c2, mut h2, _cancel2) = pair().await;
    h2.install(&a).unwrap();
    let h2 = h2.start().unwrap();
    raw(&mut c2, old).await;
    assert!(
        h2.receive(&scope(), 4, Duration::from_secs(1))
            .await
            .is_err()
    );
    h2.wait_closed().await;
}
#[tokio::test]
async fn pending_reads_do_not_block_either_send_direction() {
    let (c, h, _g, _cancel) = running().await;
    let target = scope();
    let mut hr = Box::pin(h.receive(&target, 4, Duration::from_secs(1)));
    let mut cr = Box::pin(c.receive(&target, 4, Duration::from_secs(1)));
    assert!(futures_util::poll!(hr.as_mut()).is_pending());
    assert!(futures_util::poll!(cr.as_mut()).is_pending());
    let (a, b) = tokio::join!(
        c.send_control(&target, 4, b"control", Duration::from_secs(1)),
        h.send_projection(&target, 4, b"projection", Duration::from_secs(1))
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(hr.await.unwrap().payload(), b"control");
    assert_eq!(cr.await.unwrap().payload(), b"projection");
    h.close().await;
    c.wait_closed().await;
}
#[tokio::test]
async fn cancelling_only_a_receive_wait_keeps_the_live_transport() {
    let (c, h, _g, _cancel) = running().await;
    let target = scope();
    let mut pending = Box::pin(h.receive(&target, 4, Duration::from_secs(1)));
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    drop(pending);
    c.send_control(&target, 4, b"retained", Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        h.receive(&target, 4, Duration::from_secs(1))
            .await
            .unwrap()
            .payload(),
        b"retained"
    );
    assert!(!h.is_closed());
    h.close().await;
    c.wait_closed().await;
}
#[tokio::test]
async fn revoke_or_cancel_wakes_an_already_pending_receive_and_retires_io() {
    for revoke in [false, true] {
        let (mut c, mut h, cancel) = pair().await;
        let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
        let a = access(&mut g);
        h.install(&a).unwrap();
        let h = h.start().unwrap();
        c.io.as_mut()
            .unwrap()
            .write_all(&[0, 0, 0, 120])
            .await
            .unwrap();
        c.io.as_mut().unwrap().flush().await.unwrap();
        let target = scope();
        let mut wait = Box::pin(h.receive(&target, 4, Duration::from_secs(1)));
        assert!(futures_util::poll!(wait.as_mut()).is_pending());
        if revoke {
            g.revoke()
        } else {
            cancel.cancel()
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(500), wait)
                .await
                .unwrap()
                .is_err()
        );
        h.wait_closed().await;
        assert!(h.is_closed());
    }
}
#[tokio::test]
async fn immutable_grant_deadline_bounds_a_silent_bound_reader() {
    let (_c, mut h, _cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let p = g
        .challenge(scope(), DataRole::Controller, Duration::from_millis(50))
        .unwrap();
    let a = g.accept(&signed(&p, g.issuer_kid())).unwrap();
    h.install(&a).unwrap();
    let h = h.start().unwrap();
    tokio::time::timeout(Duration::from_millis(500), h.wait_closed())
        .await
        .unwrap();
    assert!(h.is_closed());
}
#[tokio::test]
async fn first_partial_byte_arms_the_record_deadline_without_another_io_event() {
    let (mut c, mut h, _cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let p = g
        .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
        .unwrap();
    let a = g.accept(&signed(&p, g.issuer_kid())).unwrap();
    h.install(&a).unwrap();
    let h = h.start().unwrap();
    c.io.as_mut().unwrap().write_all(&[0]).await.unwrap();
    c.io.as_mut().unwrap().flush().await.unwrap();
    tokio::task::yield_now().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::task::yield_now().await;
    assert!(h.is_closed());
    assert_eq!(h.wait_closed().await, DataError::Deadline);
}
#[tokio::test]
async fn idle_cancellation_joins_owner_and_retires_both_transports() {
    let (c, h, _g, cancel) = running().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_millis(500), async {
        h.wait_closed().await;
        c.wait_closed().await;
    })
    .await
    .unwrap();
    assert!(h.is_closed() && c.is_closed());
}
#[tokio::test]
async fn retained_records_bound_ingress_without_blocking_outbound_progress() {
    let (c, h, _g, _cancel) = running().await;
    let target = scope();
    let mut held = Vec::new();
    for _ in 0..crate::DATA_QUEUE_SLOTS {
        c.send_control(&target, 4, b"bounded", Duration::from_secs(1))
            .await
            .unwrap();
        held.push(h.receive(&target, 4, Duration::from_secs(1)).await.unwrap());
    }
    c.send_control(&target, 4, b"fifth", Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        h.receive(&target, 4, Duration::from_millis(20))
            .await
            .unwrap_err(),
        DataError::Deadline
    );
    assert!(!h.is_closed());
    h.send_projection(&target, 4, b"still-writable", Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        c.receive(&target, 4, Duration::from_secs(1))
            .await
            .unwrap()
            .payload(),
        b"still-writable"
    );
    drop(held.remove(0));
    assert_eq!(
        h.receive(&target, 4, Duration::from_secs(1))
            .await
            .unwrap()
            .payload(),
        b"fifth"
    );
    h.close().await;
    c.wait_closed().await;
}
#[tokio::test]
async fn outgoing_credit_caps_all_admitted_requests_before_copy_and_close_wakes_waits() {
    let (mut c, mut h, _cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let a = access(&mut g);
    c.install(&a).unwrap();
    h.install(&a).unwrap();
    let c = c.start().unwrap();
    let target = scope();
    let bytes = vec![b'x'; 16384 - grant::DATA_PREFIX];
    let mut pending = Vec::new();
    for _ in 0..crate::DATA_QUEUE_SLOTS {
        let mut future = Box::pin(c.send_control(&target, 4, &bytes, Duration::from_secs(1)));
        assert!(futures_util::poll!(future.as_mut()).is_pending());
        pending.push(future);
    }
    assert_eq!(
        c.send_control(&target, 4, &bytes, Duration::from_secs(1))
            .await,
        Err(DataError::Full)
    );
    c.close().await;
    for p in pending {
        assert!(p.await.is_err());
    }
    assert!(c.is_closed());
}
#[tokio::test]
async fn revoke_wakes_real_tls_write_backpressure_and_drops_owned_stream() {
    let (mut c, mut h, _cancel) = pair().await;
    let mut g = gate(&h, Arc::new(AtomicU64::new(1000)));
    let a = access(&mut g);
    c.install(&a).unwrap();
    h.install(&a).unwrap();
    let c = c.start().unwrap();
    let target = scope();
    let bytes = vec![b'x'; 16384 - grant::DATA_PREFIX];
    for _ in 0..3 {
        c.send_control(&target, 4, &bytes, Duration::from_secs(1))
            .await
            .unwrap();
    }
    let mut pending = Box::pin(c.send_control(&target, 4, &bytes, Duration::from_secs(1)));
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    tokio::task::yield_now().await;
    g.revoke();
    assert!(
        tokio::time::timeout(Duration::from_millis(500), pending)
            .await
            .unwrap()
            .is_err()
    );
    c.wait_closed().await;
    assert!(c.is_closed());
}
