//! Public TEST_ONLY identities and real TLS/AppBind/grant exchange over duplex.
use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{DataRole, SessionScope},
};
use cc_desk_remote_security::*;
use std::{sync::Arc, time::Duration};
#[path = "../../../remote/crates/security/tests/support/identities.rs"]
#[rustfmt::skip]
// Keep this shared2024 fixture in its own edition's established formatting.
#[allow(
    dead_code,
    reason = "Existing fixed public TEST_ONLY signing vectors; never a product provider"
)]
mod identities;
#[path = "../../../remote/crates/client/src/tests/support.rs"]
#[rustfmt::skip]
// Import fixed vectors without formatting another workspace's source.
#[allow(
    dead_code,
    reason = "Existing public TEST_ONLY issuer/scalar; unused fixed scope is intentionally not adopted"
)]
mod support;
const BUDGET: Duration = Duration::from_secs(1);
struct PublicSigner;
impl NativeGrantSigner for PublicSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        request: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(support::signed(request.payload(), request.issuer_kid())) })
    }
}
pub(crate) async fn channels(
    scope: SessionScope,
) -> (RunningChannel, RunningChannel, GrantGate, CancelHandle) {
    let local = |client| {
        let (k, p) = identities::identity(client, false);
        LocalIdentity::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
            k.key.clone(),
        )
        .unwrap()
    };
    let pin = |client| {
        let (_, p) = identities::identity(client, false);
        PeerPin::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
        )
    };
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            BUDGET,
            token.clone()
        ),
        accept(Unbound::new(h), local(false), pin(true), BUDGET, token)
    );
    let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
    let (c, h) = tokio::join!(
        bind_client_peer_channel(
            c.unwrap(),
            ClientTrust {
                host_key_epoch: 5,
                client_key_epoch: 7,
                origin: origin.clone()
            }
        ),
        bind_host_peer_channel(
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
    let (c, h) = (c.unwrap(), h.unwrap());
    let clock: Arc<dyn GrantClock> = Arc::new(|| Ok(1000));
    let mut gate =
        GrantGate::for_binding(h.context(), support::admin(), [4; 32], clock.clone()).unwrap();
    let mut state = ClientGrantState::new(clock);
    let mut signer = PublicSigner;
    let trust = ClientGrantTrust {
        issuer: support::admin(),
        principal: [4; 32],
        scope: scope.clone(),
        role: DataRole::Controller,
    };
    let policy = NativeGrantPolicy {
        scope,
        role: DataRole::Controller,
        duration: Duration::from_secs(10),
    };
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, BUDGET),
        h.grant_host(&mut gate, policy, BUDGET)
    );
    let (mut c, mut h) = (c.unwrap(), h.unwrap());
    (
        c.start_peer_pending(tokio::time::Instant::now() + BUDGET)
            .unwrap(),
        h.start_peer_pending(tokio::time::Instant::now() + BUDGET)
            .unwrap(),
        gate,
        cancel,
    )
}
