use super::*;
use crate::grant_tests::{admin, scope, signed};
use crate::test_identities as identities;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
fn local(client: bool) -> LocalIdentity {
    let (k, p) = identities::identity(client, false);
    LocalIdentity::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
fn pin(client: bool) -> PeerPin {
    let (_, p) = identities::identity(client, false);
    PeerPin::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
pub(crate) async fn pair() -> (
    ManagedChannel<tokio::io::DuplexStream>,
    ManagedChannel<tokio::io::DuplexStream>,
    CancelHandle,
) {
    pair_capacity(65536).await
}
pub(crate) async fn pair_capacity(
    capacity: usize,
) -> (
    ManagedChannel<tokio::io::DuplexStream>,
    ManagedChannel<tokio::io::DuplexStream>,
    CancelHandle,
) {
    let (c, h) = tokio::io::duplex(capacity);
    let (cancel, t) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            t.clone()
        ),
        accept(
            Unbound::new(h),
            local(false),
            pin(true),
            Duration::from_secs(2),
            t
        )
    );
    let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
    let (c, h) = tokio::join!(
        bind_client_channel(
            c.unwrap(),
            ClientTrust {
                host_key_epoch: 5,
                client_key_epoch: 7,
                origin: origin.clone()
            }
        ),
        bind_host_channel(
            h.unwrap(),
            HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin
            }
        )
    );
    (c.unwrap(), h.unwrap(), cancel)
}
#[tokio::test]
async fn real_bound_io_requires_exact_signed_grant_and_scope() {
    let (mut c, mut h, _cancel) = pair().await;
    let target = scope();
    assert!(h.start().is_err());
    let now = Arc::new(AtomicU64::new(1000));
    let source = now.clone();
    let mut gate = GrantGate::for_binding(
        h.context(),
        admin(),
        [4; 32],
        Arc::new(move || Ok(source.load(Ordering::Acquire))),
    )
    .unwrap();
    let challenge = gate
        .challenge(target.clone(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let access = gate.accept(&signed(&challenge, gate.issuer_kid())).unwrap();
    c.install(&access).unwrap();
    h.install(&access).unwrap();
    let c = c.start().unwrap();
    let h = h.start().unwrap();
    let mut wrong = target.clone();
    wrong.run = "other".into();
    assert!(
        c.send_control(&wrong, 4, b"public", Duration::from_secs(1))
            .await
            .is_err()
    );
    let (sent, received) = tokio::join!(
        c.send_control(&target, 4, b"PUBLIC_MUTATION", Duration::from_secs(1)),
        h.receive(&target, 4, Duration::from_secs(1))
    );
    sent.unwrap();
    assert_eq!(received.unwrap().payload(), b"PUBLIC_MUTATION");
    let (sent, received) = tokio::join!(
        h.send_projection(&target, 4, b"PUBLIC_PROJECTION", Duration::from_secs(1)),
        c.receive(&target, 4, Duration::from_secs(1))
    );
    sent.unwrap();
    assert_eq!(received.unwrap().payload(), b"PUBLIC_PROJECTION");
    now.store(5000, Ordering::Release);
    assert!(
        h.send_projection(&target, 4, b"expired", Duration::from_secs(1))
            .await
            .is_err()
    );
    assert!(
        c.send_control(&target, 4, b"expired", Duration::from_secs(1))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn received_record_retains_exact_live_host_channel_proof() {
    let (mut c, mut h, _cancel) = pair().await;
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let p = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let g = gate.accept(&signed(&p, gate.issuer_kid())).unwrap();
    c.install(&g).unwrap();
    h.install(&g).unwrap();
    let c = c.start().unwrap();
    let h = h.start().unwrap();
    let proof = h.authority();
    assert_eq!(proof.local_role(), Role::Host);
    assert_eq!(proof.principal_id(), [4; 32]);
    assert!(proof.check().is_ok());
    let target = scope();
    c.send_control(&target, 4, b"proof", Duration::from_secs(1))
        .await
        .unwrap();
    let record = h.receive(&target, 4, Duration::from_secs(1)).await.unwrap();
    assert!(record.source().same_channel(&proof));
    let (mut c2, mut h2, _cancel2) = pair().await;
    c2.install(&g).unwrap();
    h2.install(&g).unwrap();
    let c2 = c2.start().unwrap();
    let h2 = h2.start().unwrap();
    assert!(!record.source().same_channel(&h2.authority()));
    assert!(
        proof
            .clock_domain()
            .same_origin(&h2.authority().clock_domain())
    );
    h.close().await;
    assert!(proof.check().is_err());
    assert!(record.source().check().is_err());
    c.wait_closed().await;
    h2.close().await;
    c2.wait_closed().await;
}
#[tokio::test]
async fn observer_revocation_and_fresh_binding_do_not_inherit_old_stream_permission() {
    let (mut c, mut h, _cancel) = pair().await;
    let now = Arc::new(AtomicU64::new(1000));
    let mut gate = GrantGate::for_binding(
        h.context(),
        admin(),
        [4; 32],
        Arc::new(move || Ok(now.load(Ordering::Acquire))),
    )
    .unwrap();
    let g = gate
        .challenge(scope(), DataRole::Observer, Duration::from_secs(4))
        .unwrap();
    let access = gate.accept(&signed(&g, gate.issuer_kid())).unwrap();
    c.install(&access).unwrap();
    h.install(&access).unwrap();
    let c = c.start().unwrap();
    let h = h.start().unwrap();
    assert!(
        c.send_control(&scope(), 4, b"forbidden", Duration::from_secs(1))
            .await
            .is_err()
    );
    let (mut c2, mut h2, _cancel2) = pair().await;
    assert_ne!(h.binding_id(), h2.binding_id());
    c2.install(&access).unwrap();
    h2.install(&access).unwrap();
    let _c2 = c2.start().unwrap();
    let h2 = h2.start().unwrap();
    gate.revoke();
    assert!(
        h.send_projection(&scope(), 4, b"revoked", Duration::from_secs(1))
            .await
            .is_err()
    );
    assert!(
        h2.send_projection(&scope(), 4, b"revoked", Duration::from_secs(1))
            .await
            .is_err()
    );
}
