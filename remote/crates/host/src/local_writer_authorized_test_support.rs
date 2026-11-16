use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{DataRole, SessionScope},
};
use cc_desk_remote_security::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
#[path = "../../security/tests/support/identities.rs"]
#[allow(dead_code)]
mod identities;
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
pub(super) fn host() -> [u8; 32] {
    let (_, p) = identities::identity(false, false);
    P256Spki::new(&p).unwrap().endpoint_id(Role::Host)
}
pub(super) fn device() -> [u8; 32] {
    let (_, p) = identities::identity(true, false);
    P256Spki::new(&p).unwrap().endpoint_id(Role::Client)
}
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn admin_point() -> Vec<u8> {
    hex(
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    )
}
pub(super) fn admin() -> P256Spki {
    let mut b = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    b.extend(admin_point());
    P256Spki::new(&b).unwrap()
}
pub(super) fn signed(g: &cc_desk_remote_record::grant::CanonicalGrant, kid: [u8; 32]) -> Vec<u8> {
    use coset::{CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
    let mut scalar = [0; 32];
    scalar[31] = 1; // PUBLIC scalar1 fixture, never enroll for real access.
    let rng = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &scalar,
        &admin_point(),
        &rng,
    )
    .unwrap();
    CoseSign1Builder::new()
        .protected(
            HeaderBuilder::new()
                .algorithm(iana::Algorithm::ES256)
                .content_type("application/cc-desk-grant+json".into())
                .key_id(kid.to_vec())
                .build(),
        )
        .payload(cc_desk_remote_record::grant::encode(g).unwrap())
        .create_signature(b"CC-DESK-DATA-GRANT-v1\0", |m| {
            key.sign(&rng, m).unwrap().as_ref().to_vec()
        })
        .build()
        .to_tagged_vec()
        .unwrap()
}
pub(super) async fn pair() -> (
    ManagedChannel<tokio::io::DuplexStream>,
    ManagedChannel<tokio::io::DuplexStream>,
    CancelHandle,
) {
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            Duration::from_secs(2),
            token.clone()
        ),
        accept(
            Unbound::new(h),
            local(false),
            pin(true),
            Duration::from_secs(2),
            token
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
pub(super) fn scope() -> SessionScope {
    SessionScope {
        session: "02".repeat(16),
        run: "TEST_ONLY_BOUND_PTY".into(),
        generation: "3".into(),
    }
}
pub(super) async fn channels(
    now: Arc<AtomicU64>,
) -> (
    RunningChannel,
    RunningChannel,
    GrantGate,
    CancelHandle,
    VerifiedGrant,
) {
    channels_as(now, [4; 32], DataRole::Controller).await
}
pub(super) async fn channels_as(
    now: Arc<AtomicU64>,
    principal: [u8; 32],
    role: DataRole,
) -> (
    RunningChannel,
    RunningChannel,
    GrantGate,
    CancelHandle,
    VerifiedGrant,
) {
    let (mut c, mut h, cancel) = pair().await;
    let mut gate = GrantGate::for_binding(
        h.context(),
        admin(),
        principal,
        Arc::new(move || Ok(now.load(Ordering::Acquire))),
    )
    .unwrap();
    let p = gate
        .challenge(scope(), role, Duration::from_secs(10))
        .unwrap();
    let grant = gate.accept(&signed(&p, gate.issuer_kid())).unwrap();
    c.install(&grant).unwrap();
    h.install(&grant).unwrap();
    (c.start().unwrap(), h.start().unwrap(), gate, cancel, grant)
}
