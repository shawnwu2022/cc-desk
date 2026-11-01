use super::*;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::*;
use std::sync::Arc;
#[path = "tests/support.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod fixture;
#[path = "../../security/tests/support/identities.rs"]
#[allow(dead_code, clippy::duplicate_mod)]
mod identities;
struct GrantSigner;
impl NativeGrantSigner for GrantSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        g: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(fixture::signed(g.payload(), g.issuer_kid())) })
    }
}
struct NoRelay;
struct PanicSigner;
impl NativeGrantSigner for PanicSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        _: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async { panic!("TEST_ONLY native grant poll panic") })
    }
}
impl transport::wss::RelaySigner for NoRelay {
    fn sign<'a>(
        &'a self,
        _: &'a transport::wss::SigningRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<[u8; 64], transport::wss::SignError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { panic!("TEST_ONLY memory fixture must not dial relay") })
    }
}
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
fn profile() -> NativeProfile {
    NativeProfile {
        address: "127.0.0.1:0".parse().unwrap(),
        relay: transport::wss::RelayAdmissionIdentity::new(
            [1; 16],
            transport::wss::RelayRole::Phone,
            Arc::new(NoRelay),
        ),
        tunnel: [9; 16],
        local: local(true),
        host_pin: pin(false),
        binding: ClientTrust {
            host_key_epoch: 5,
            client_key_epoch: 7,
            origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
        },
        grant: ClientGrantTrust {
            issuer: fixture::admin(),
            principal: [4; 32],
            scope: fixture::scope(),
            role: DataRole::Controller,
        },
        signer: Box::new(GrantSigner),
        state: ClientGrantState::new(Arc::new(|| Ok(1000))),
        setup_budget: Duration::from_secs(1),
        io_budget: Duration::from_secs(1),
    }
}
#[tokio::test]
async fn bootstrap_fixed_memory_mux_performs_actual_rpk_grant_and_install_then_joins() {
    let ((stream, mux), (host_stream, host_mux)) =
        transport::test_only_memory_mux_pair([9; 16], Duration::from_secs(1))
            .await
            .unwrap();
    let (stop, rx) = watch::channel(false);
    let (_cancel, token) = cancellation();
    let host = async {
        let pending = accept(
            Unbound::new(host_stream),
            local(false),
            pin(true),
            Duration::from_secs(1),
            token,
        )
        .await
        .unwrap();
        let managed = bind_host_peer_channel(
            pending,
            HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
            },
        )
        .await
        .unwrap();
        let mut gate = GrantGate::for_binding(
            managed.context(),
            fixture::admin(),
            [4; 32],
            Arc::new(|| Ok(1000)),
        )
        .unwrap();
        let mut managed = managed
            .grant_host(
                &mut gate,
                NativeGrantPolicy {
                    scope: fixture::scope(),
                    role: DataRole::Controller,
                    duration: Duration::from_secs(10),
                },
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        let channel = managed
            .start_peer_pending(Instant::now() + Duration::from_secs(1))
            .unwrap();
        let request = channel.host_install_request().await.unwrap();
        channel.complete_host_install(request, 7).await.unwrap();
        (channel, gate)
    };
    let (client, host) = tokio::join!(
        finish_channel(
            profile(),
            rx,
            Instant::now() + Duration::from_secs(1),
            stream,
            mux
        ),
        host
    );
    let client = client.unwrap();
    let (host, _gate) = host;
    assert!(client.channel.authority().check().is_ok());
    assert!(client.mux.is_some());
    let reply = async {
        let record = host
            .receive(&fixture::scope(), 7, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            record.payload(),
            br#"{"message":"Window","profile":1,"request":"1"}"#
        );
        let bytes = cc_desk_remote_record::control_io::encode(
            &cc_desk_remote_record::control_io::Message::WindowGranted {
                profile: 1,
                request: "1".into(),
                window: "09".repeat(32),
                not_after_host_ms: "2000".into(),
            },
        )
        .unwrap();
        host.send_projection(&fixture::scope(), 7, &bytes, Duration::from_secs(1))
            .await
            .unwrap();
    };
    let (received, ()) = tokio::join!(
        client.exchange(
            br#"{"message":"Window","profile":1,"request":"1"}"#,
            Duration::from_secs(1)
        ),
        reply
    );
    assert!(received.is_ok());
    client.request_close();
    host.authority().request_close();
    host_mux.request_close();
    let (c, _, h) = tokio::join!(client.retire(), host.close(), host_mux.wait_closed());
    assert!(c.is_ok() && h.is_ok());
    drop(stop);
}
#[tokio::test]
async fn bootstrap_pre_cancelled_profile_never_dials_or_signs() {
    let (stop, rx) = watch::channel(true);
    assert!(matches!(
        connect_native(profile(), rx).await,
        Err(BootstrapError::Cancelled)
    ));
    drop(stop);
}
#[tokio::test]
async fn bootstrap_native_poll_panic_retires_original_mux_and_reports_unknown() {
    let ((stream, mux), (host_stream, host_mux)) =
        transport::test_only_memory_mux_pair([9; 16], Duration::from_secs(1))
            .await
            .unwrap();
    let original_mux = mux.test_only_retirement();
    let (_stop, rx) = watch::channel(false);
    let (_cancel, token) = cancellation();
    let mut p = profile();
    p.signer = Box::new(PanicSigner);
    let client = tokio::spawn(finish_channel(
        p,
        rx,
        Instant::now() + Duration::from_secs(1),
        stream,
        mux,
    ));
    let host = async {
        let pending = accept(
            Unbound::new(host_stream),
            local(false),
            pin(true),
            Duration::from_secs(1),
            token,
        )
        .await
        .unwrap();
        let managed = bind_host_peer_channel(
            pending,
            HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
            },
        )
        .await
        .unwrap();
        let mut gate = GrantGate::for_binding(
            managed.context(),
            fixture::admin(),
            [4; 32],
            Arc::new(|| Ok(1000)),
        )
        .unwrap();
        managed
            .grant_host(
                &mut gate,
                NativeGrantPolicy {
                    scope: fixture::scope(),
                    role: DataRole::Controller,
                    duration: Duration::from_secs(10),
                },
                Duration::from_secs(1),
            )
            .await
            .is_err()
    };
    let (result, host_failed) = tokio::join!(client, host);
    host_mux.request_close();
    assert!(host_mux.wait_closed().await.is_ok());
    assert!(
        original_mux.known_joined(),
        "original client driver must actually be joined before asserting Unknown"
    );
    assert!(host_failed);
    assert!(
        matches!(result, Ok(Err(BootstrapError::RetirementUnknown))),
        "native poll panic must retain cleanup and become Unknown rather than unwind the owning constructor"
    );
}
#[tokio::test]
async fn bootstrap_cancel_during_install_signals_inner_and_mux_before_join() {
    let ((stream, mux), (host_stream, host_mux)) =
        transport::test_only_memory_mux_pair([9; 16], Duration::from_secs(1))
            .await
            .unwrap();
    let (stop, rx) = watch::channel(false);
    let (_cancel, token) = cancellation();
    let (entered, started) = tokio::sync::oneshot::channel();
    let client = tokio::spawn(finish_channel(
        profile(),
        rx,
        Instant::now() + Duration::from_secs(1),
        stream,
        mux,
    ));
    let host = tokio::spawn(async move {
        let pending = accept(
            Unbound::new(host_stream),
            local(false),
            pin(true),
            Duration::from_secs(1),
            token,
        )
        .await
        .unwrap();
        let managed = bind_host_peer_channel(
            pending,
            HostSnapshot {
                host_key_epoch: 5,
                client_key_epoch: 7,
                activation: [0x33; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
            },
        )
        .await
        .unwrap();
        let mut gate = GrantGate::for_binding(
            managed.context(),
            fixture::admin(),
            [4; 32],
            Arc::new(|| Ok(1000)),
        )
        .unwrap();
        let mut managed = managed
            .grant_host(
                &mut gate,
                NativeGrantPolicy {
                    scope: fixture::scope(),
                    role: DataRole::Controller,
                    duration: Duration::from_secs(10),
                },
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        let channel = managed
            .start_peer_pending(Instant::now() + Duration::from_secs(1))
            .unwrap();
        let _request = channel.host_install_request().await.unwrap();
        entered.send(()).unwrap();
        channel.wait_closed().await;
    });
    started.await.unwrap();
    stop.send_replace(true);
    let result = client.await;
    host_mux.request_close();
    let (_, closed) = tokio::join!(host, host_mux.wait_closed());
    assert!(closed.is_ok());
    assert!(matches!(result, Ok(Err(BootstrapError::Cancelled))));
}
