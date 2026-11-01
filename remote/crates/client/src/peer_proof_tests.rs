use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::*;
use std::sync::{Arc, atomic::AtomicU64};
#[path = "tests/support.rs"]
#[allow(clippy::duplicate_mod)]
// Independent TEST_ONLY fixtures; no production module is duplicated.
mod support;
const BUDGET: Duration = Duration::from_secs(1);
#[path = "../../security/tests/support/identities.rs"]
#[allow(dead_code)]
#[allow(clippy::duplicate_mod)]
// Reuse the fixed public signing fixtures in this isolated test module.
mod identities;
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
async fn peer_channels(
    now: Arc<AtomicU64>,
) -> (RunningChannel, RunningChannel, GrantGate, CancelHandle) {
    peer_channels_clock(Arc::new(move || {
        Ok(now.load(std::sync::atomic::Ordering::Acquire))
    }))
    .await
}
async fn peer_channels_clock(
    clock: Arc<dyn GrantClock>,
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
    let mut gate =
        GrantGate::for_binding(h.context(), support::admin(), [4; 32], clock.clone()).unwrap();
    let mut state = ClientGrantState::new(clock);
    let mut signer = PublicSigner;
    let trust = ClientGrantTrust {
        issuer: support::admin(),
        principal: [4; 32],
        scope: support::scope(),
        role: DataRole::Controller,
    };
    let policy = NativeGrantPolicy {
        scope: support::scope(),
        role: DataRole::Controller,
        duration: Duration::from_secs(10),
    };
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, BUDGET),
        h.grant_host(&mut gate, policy, BUDGET)
    );
    let (mut c, mut h) = (c.unwrap(), h.unwrap());
    (
        c.start_peer_pending(Instant::now() + BUDGET).unwrap(),
        h.start_peer_pending(Instant::now() + BUDGET).unwrap(),
        gate,
        cancel,
    )
}

// Fixture uses real mutually authenticated TLS and actual grant_client/grant_host.
use super::*;
#[tokio::test]
async fn constructor_native_clock_panic_returns_original_owner_for_join() {
    let panics = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = panics.clone();
    let clock: Arc<dyn GrantClock> = Arc::new(move || {
        assert!(
            !flag.load(std::sync::atomic::Ordering::Acquire),
            "TEST_ONLY constructor clock panic"
        );
        Ok(1000)
    });
    let (c, h, _gate, _cancel) = peer_channels_clock(clock).await;
    let r = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(r, 7));
    done.unwrap();
    panics.store(true, std::sync::atomic::Ordering::Release);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        NativeClient::from_installed_retained(c, proof.unwrap())
    }));
    panics.store(false, std::sync::atomic::Ordering::Release);
    let retained = match result {
        Ok(Err(original)) => {
            let (error, channel) = *original;
            channel.close().await;
            error == DataError::Uncertain
        }
        Ok(Ok(client)) => {
            client.close().await;
            false
        }
        Err(_) => false,
    };
    h.close().await;
    assert!(
        retained,
        "native validation panic must return the exact original joinable channel"
    );
}

#[tokio::test]
async fn installed_constructor_failure_keeps_the_original_channel_joinable() {
    let (c, h, _gate, _cancel) = peer_channels(Arc::new(AtomicU64::new(1000))).await;
    let r = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(r, 7));
    done.unwrap();
    c.authority().request_close();
    let rejected = match NativeClient::from_installed_retained(c, proof.unwrap()) {
        Err(original) => original,
        Ok(_) => panic!("closed owner accepted"),
    };
    let (_, original) = *rejected;
    original.close().await;
    h.close().await;
}

#[tokio::test]
async fn peer_install_old_constructor_refuses_pending_and_ready_and_proof_is_consumed() {
    let (c, h, _gate, _cancel) = peer_channels(Arc::new(AtomicU64::new(1000))).await;
    assert!(matches!(NativeClient::new(c, 7), Err(DataError::Context)));
    h.close().await;
    let (c, h, _gate, _cancel) = peer_channels(Arc::new(AtomicU64::new(1000))).await;
    let r = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(r, 7));
    done.unwrap();
    let proof = proof.unwrap();
    assert!(matches!(NativeClient::new(c, 7), Err(DataError::Context)));
    drop(proof);
    h.close().await;
    let (c, h, _gate, _cancel) = peer_channels(Arc::new(AtomicU64::new(1000))).await;
    let r = h.host_install_request().await.unwrap();
    let (proof, done) = tokio::join!(c.client_installed(), h.complete_host_install(r, 7));
    done.unwrap();
    let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
    assert_eq!(client.fence, 7);
    assert_eq!(client.channel.installed_fence().unwrap(), 7);
    // Constructor does not send a snapshot ACK or grant an input window.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            h.receive(&client.scope, 7, Duration::from_secs(1))
        )
        .await
        .is_err()
    );
    client.close().await;
    h.wait_closed().await;
}
