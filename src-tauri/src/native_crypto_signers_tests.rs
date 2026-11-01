use super::super::{owner::*, signers::*};
use super::*;
use cc_desk_remote_security::*;
use cc_desk_remote_transport::wss::{RelayAdmissionIdentity, RelayRole};
use rustls::sign::SigningKey;
use std::sync::Arc;
use std::time::{Duration, Instant};
#[path = "../../remote/crates/security/tests/support/identities.rs"]
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)] // Unchanged fixed public fixture in its original edition.
mod identities;
#[path = "../../remote/crates/client/src/tests/support.rs"]
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)] // Existing public generator point is the wrong-issuer negative.
mod issuer_fixture;

fn fixture_spki() -> (Vec<u8>, Vec<u8>) {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    (point, spki)
}
fn retire(owner: &mut KeyCustodian) -> bool {
    owner.shutdown();
    let end = Instant::now() + Duration::from_secs(5);
    while owner.observe() == KeyRetirement::Pending && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    matches!(
        owner.observe(),
        KeyRetirement::Retired | KeyRetirement::NoKeyCreated
    )
}

struct PublicPresence(bool);
impl NativeGrantPresence for PublicPresence {
    fn approve<'a>(&'a self, _: &'a VerifiedCanonicalGrant, exact: [u8; 32]) -> PresenceFuture<'a> {
        Box::pin(async move {
            if self.0 {
                Ok(exact)
            } else {
                Ok([0; 32])
            }
        })
    }
}
async fn grant_trial(
    signer: &mut dyn NativeGrantSigner,
    issuer: P256Spki,
    endpoint_fixture: bool,
) -> (bool, Option<u64>) {
    use cc_desk_remote_record::{
        appbind::CanonicalOrigin,
        grant::{DataRole, SessionScope},
    };
    let local = |client| {
        // Existing public COSE fixture serves both endpoint roles here;
        // it is distinct from the native RFC6979 issuer. This is not
        // evidence for production multi-role profile enrollment.
        let (key, spki) = identities::identity(endpoint_fixture, false);
        LocalIdentity::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&spki).unwrap(),
            key.key.clone(),
        )
        .unwrap()
    };
    let pin = |client| {
        let (_, spki) = identities::identity(endpoint_fixture, false);
        PeerPin::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&spki).unwrap(),
        )
    };
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let budget = Duration::from_secs(2);
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            local(true),
            pin(false),
            budget,
            token.clone()
        ),
        accept(Unbound::new(h), local(false), pin(true), budget, token)
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
        GrantGate::for_binding(h.context(), issuer.clone(), [4; 32], clock.clone()).unwrap();
    let scope = SessionScope {
        session: "02".repeat(16),
        run: "PUBLIC_CNG_ADAPTER_TEST".into(),
        generation: "3".into(),
    };
    let trust = ClientGrantTrust {
        issuer,
        principal: [4; 32],
        scope: scope.clone(),
        role: DataRole::Controller,
    };
    let policy = NativeGrantPolicy {
        scope,
        role: DataRole::Controller,
        duration: Duration::from_secs(10),
    };
    let mut state = ClientGrantState::new(clock);
    let (client, host) = tokio::join!(
        c.grant_client(&trust, signer, &mut state, budget),
        h.grant_host(&mut gate, policy, budget)
    );
    let accepted = client.is_ok() && host.is_ok();
    drop((client, host));
    drop(gate);
    cancel.cancel();
    (
        accepted,
        state.remote_grant().map(|grant| grant.expires_host_ms()),
    )
}
#[tokio::test]
#[allow(non_snake_case)]
async fn CngAdapters_GrantActualExchangeRequiresExactNativePresence_002() {
    for mode in 0..4 {
        let approved = mode == 2;
        let (point, spki) = fixture_spki();
        let issuer = if mode == 3 {
            issuer_fixture::admin()
        } else {
            P256Spki::new(&spki).unwrap()
        };
        let (mut owner, handle) = KeyCustodian::start(
            move || import_public_rfc6979_fixture_for(&point, SigningPurpose::Issuer),
            SigningPurpose::Issuer,
            spki,
        )
        .unwrap();
        let presence: Arc<dyn NativeGrantPresence> = if mode == 0 {
            Arc::new(UnavailablePresence)
        } else {
            Arc::new(PublicPresence(mode != 1))
        };
        let mut signer = CngGrantSigner::new(handle, presence).unwrap();
        let accepted = grant_trial(&mut signer, issuer, true).await;
        drop(signer);
        let retired = retire(&mut owner);
        assert!(retired);
        assert_eq!(accepted.0, approved);
        assert_eq!(owner.native_sign_count(), usize::from(approved));
    }
}

#[test]
#[allow(non_snake_case)]
fn CngAdapters_ActualRustlsTraitsSignAndFreeOriginal_001() {
    let point = hex(vector()["public_key_sec1"].as_str().unwrap());
    let public = point.clone();
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(&point);
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki.clone(),
    )
    .unwrap();
    let key = CngTlsKey::new(handle, Instant::now() + Duration::from_secs(2));
    let mut exchanged = false;
    let result = key.and_then(|key| {
        if key.public_key().as_ref().map(|p| p.as_ref()) != Some(spki.as_slice())
            || key
                .choose_scheme(&[rustls::SignatureScheme::RSA_PSS_SHA256])
                .is_some()
        {
            return Err(SignFailure::Unknown);
        }
        let signed = key
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .ok_or(SignFailure::Unknown)?
            .sign(b"PUBLIC_NATIVE_TLS_TRAIT")
            .map_err(|_| SignFailure::Unknown)?;
        let host =
            LocalIdentity::new(Role::Host, P256Spki::new(&spki).unwrap(), Arc::new(key)).unwrap();
        let (client_key, client_spki) = identities::identity(true, false);
        let client = LocalIdentity::new(
            Role::Client,
            P256Spki::new(&client_spki).unwrap(),
            client_key.key.clone(),
        )
        .unwrap();
        let host_pin = PeerPin::new(Role::Host, P256Spki::new(&spki).unwrap());
        let client_pin = PeerPin::new(Role::Client, P256Spki::new(&client_spki).unwrap());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (a, b) = tokio::io::duplex(65536);
        let (cancel, token) = cancellation();
        exchanged = runtime.block_on(async {
            let (c, h) = tokio::join!(
                connect(
                    Unbound::new(a),
                    client,
                    host_pin,
                    Duration::from_secs(1),
                    token.clone()
                ),
                accept(
                    Unbound::new(b),
                    host,
                    client_pin,
                    Duration::from_secs(1),
                    token
                )
            );
            c.is_ok() && h.is_ok()
        });
        cancel.cancel();
        Ok(signed)
    });
    owner.shutdown();
    let end = Instant::now() + Duration::from_secs(5);
    while owner.observe() == KeyRetirement::Pending && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(owner.observe(), KeyRetirement::Retired);
    assert!(exchanged);
    assert_eq!(owner.native_sign_count(), 2);
    let signature = result.unwrap();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, &public)
        .verify(b"PUBLIC_NATIVE_TLS_TRAIT", &signature)
        .unwrap();
}

#[cfg(feature = "remote-client-ipc")]
#[tokio::test]
#[allow(non_snake_case)]
async fn CngAdapters_AppExitBeforeActivationKeepsOriginalNoKeyJoin_004() {
    use crate::remote_client_ipc::{ExitDecision, RemoteClientService};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct PublicProfile {
        spki: Vec<u8>,
        calls: Arc<AtomicUsize>,
    }
    impl NativeKeyProfile for PublicProfile {
        fn binding(&self, _: SigningPurpose) -> Result<Vec<u8>, SignFailure> {
            Ok(self.spki.clone())
        }
        fn open(&self, purpose: SigningPurpose) -> Result<NativeOwnedSoftwareKey, CryptoError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (point, _) = fixture_spki();
            import_public_rfc6979_fixture_for(&point, purpose)
        }
    }
    let absent = RemoteClientService::default();
    assert!(absent
        .install_key_profile(None, SigningPurpose::Endpoint)
        .is_err());
    let binding = NativeSignerBinding {
        approved_until: Some(Instant::now() + Duration::from_secs(20)),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://relay.invalid/")
            .unwrap(),
        relay_id: [1; 16],
        relay_role: RelayRole::Phone,
        tls_end: Instant::now() + Duration::from_secs(2),
    };
    assert!(absent.native_signers(None, binding, None).is_err());
    assert_eq!(absent.request_exit(13), ExitDecision::Proceed);
    let service = RemoteClientService::default();
    let (_, spki) = fixture_spki();
    let calls = Arc::new(AtomicUsize::new(0));
    let profile = Arc::new(PublicProfile {
        spki,
        calls: calls.clone(),
    });
    let mut first = None;
    let registered =
        service.install_key_profile_for_test(profile, SigningPurpose::Endpoint, || {
            first = Some(service.request_exit(37));
        });
    let mut actual = None;
    let result = service
        .shutdown_and_exit(37, |code| actual = Some(code))
        .await;
    assert!(registered.is_err());
    assert_eq!(first, Some(ExitDecision::Wait(37)));
    assert!(result.is_ok());
    assert_eq!(actual, Some(37));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.test_key_observations(),
        vec![KeyRetirement::NoKeyCreated]
    );
    assert_eq!(service.request_exit(0), ExitDecision::Prevent);
    assert_eq!(service.request_exit(37), ExitDecision::Proceed);

    let service = RemoteClientService::default();
    let (_, spki) = fixture_spki();
    let first = service
        .install_key_profile(
            Some(Arc::new(PublicProfile {
                spki: spki.clone(),
                calls: Arc::new(AtomicUsize::new(0)),
            })),
            SigningPurpose::Endpoint,
        )
        .unwrap();
    let second_calls = Arc::new(AtomicUsize::new(0));
    let second = service.install_key_profile(
        Some(Arc::new(PublicProfile {
            spki,
            calls: second_calls.clone(),
        })),
        SigningPurpose::Issuer,
    );
    let conflict = second.err().map(|error| error.code);
    drop(first);
    service.begin_shutdown();
    let retired = service.shutdown().await;
    assert!(retired.is_ok());
    assert_eq!(conflict.as_deref(), Some("NATIVE_KEY_ROLE_CONFLICT"));
    assert_eq!(second_calls.load(Ordering::SeqCst), 0);

    // Full native tuple preflight refuses reused public keys before the FIRST
    // role is opened; it cannot turn the single public fixture into a profile.
    let service = RemoteClientService::default();
    let (_, spki) = fixture_spki();
    let calls = Arc::new(AtomicUsize::new(0));
    let binding = NativeSignerBinding {
        approved_until: Some(Instant::now() + Duration::from_secs(20)),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://relay.invalid/")
            .unwrap(),
        relay_id: [1; 16],
        relay_role: RelayRole::Phone,
        tls_end: Instant::now() + Duration::from_secs(2),
    };
    let result = service.native_signers(
        Some(Arc::new(PublicProfile {
            spki,
            calls: calls.clone(),
        })),
        binding,
        Some(Arc::new(UnavailablePresence)),
    );
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(service.request_exit(19), ExitDecision::Proceed);
    positive_native_bundle().await;
}

#[cfg(feature = "remote-client-ipc")]
async fn positive_native_bundle() {
    use crate::remote_client_ipc::RemoteClientService;
    use base64::Engine;
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Closed selection of repository-published keys; no generated secret,
    // persisted name, user profile or production key loader is involved.
    fn parts(purpose: SigningPurpose) -> (Vec<u8>, Vec<u8>) {
        match purpose {
            SigningPurpose::Endpoint => (
                fixture_spki().0,
                hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"),
            ),
            SigningPurpose::Relay => {
                let mut scalar = vec![0; 32];
                scalar[31] = 1;
                (hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"),scalar)
            }
            SigningPurpose::Issuer => {
                let v: serde_json::Value = serde_json::from_slice(include_bytes!(
                    "../../remote/crates/security/tests/vectors/cose/public-sign1-02.json"
                ))
                .unwrap();
                let mut point = vec![4];
                for name in ["x", "y"] {
                    point.extend(
                        base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .decode(v[name].as_str().unwrap())
                            .unwrap(),
                    );
                }
                (
                    point,
                    base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM")
                        .unwrap(),
                )
            }
        }
    }
    struct PublishedThree(Arc<AtomicUsize>);
    impl NativeKeyProfile for PublishedThree {
        fn binding(&self, purpose: SigningPurpose) -> Result<Vec<u8>, SignFailure> {
            let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
            spki.extend(parts(purpose).0);
            Ok(spki)
        }
        fn open(&self, purpose: SigningPurpose) -> Result<NativeOwnedSoftwareKey, CryptoError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let (point, scalar) = parts(purpose);
            import_known_public_fixture(&point, &scalar, purpose)
        }
    }
    let service = Arc::new(RemoteClientService::default());
    let opened = Arc::new(AtomicUsize::new(0));
    let profile = Arc::new(PublishedThree(opened.clone()));
    let missing = NativeSignerBinding {
        approved_until: Some(Instant::now() + Duration::from_secs(20)),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://relay.invalid/")
            .unwrap(),
        relay_id: [1; 16],
        relay_role: RelayRole::Host,
        tls_end: Instant::now() + Duration::from_secs(2),
    };
    let absent = service
        .native_signers(Some(profile.clone()), missing, None)
        .err()
        .map(|error| error.code);
    assert_eq!(absent.as_deref(), Some("NATIVE_PRESENCE_UNAVAILABLE"));
    assert_eq!(opened.load(Ordering::SeqCst), 0);
    assert!(service.test_key_observations().is_empty());
    let no_cap = NativeSignerBinding {
        approved_until: None,
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted("wss://relay.invalid/")
            .unwrap(),
        relay_id: [1; 16],
        relay_role: RelayRole::Host,
        tls_end: Instant::now() + Duration::from_secs(2),
    };
    assert!(service
        .native_signers(
            Some(profile.clone()),
            no_cap,
            Some(Arc::new(PublicPresence(true)))
        )
        .is_err());
    assert_eq!(opened.load(Ordering::SeqCst), 0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!(
        "wss://foobar.com:{}/relay",
        listener.local_addr().unwrap().port()
    );
    let binding = NativeSignerBinding {
        approved_until: Some(Instant::now() + Duration::from_secs(20)),
        origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&origin).unwrap(),
        relay_id: [1; 16],
        relay_role: RelayRole::Host,
        tls_end: Instant::now() + Duration::from_secs(5),
    };
    let mut bundle = service
        .native_signers(
            Some(profile.clone()),
            binding,
            Some(Arc::new(PublicPresence(true))),
        )
        .unwrap();
    let first_tag = bundle.attempt.clone();
    let endpoint_spki = profile.binding(SigningPurpose::Endpoint).unwrap();
    let tls_first = native_tls_trial(bundle.endpoint.clone(), &endpoint_spki).await;
    let issuer = P256Spki::new(&profile.binding(SigningPurpose::Issuer).unwrap()).unwrap();
    let old_grant = grant_trial(bundle.grant.as_mut(), issuer.clone(), false).await;
    let fresh = service.fresh_native_signers(Instant::now() + Duration::from_secs(2));
    assert!(fresh.is_ok());
    assert!(bundle
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .is_none());
    let mut bundle = fresh.unwrap();
    let same_policy = bundle.attempt.same_policy(&first_tag)
        && bundle.attempt.approved_until() == first_tag.approved_until();
    let tls_second = native_tls_trial(bundle.endpoint.clone(), &endpoint_spki).await;
    let public_matches = bundle.endpoint.public_key().as_ref().map(|p| p.as_ref())
        == Some(
            profile
                .binding(SigningPurpose::Endpoint)
                .unwrap()
                .as_slice(),
        );
    let signed = bundle
        .endpoint
        .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
        .unwrap()
        .sign(b"PUBLIC_NATIVE_BUNDLE");
    let endpoint_verified = signed.is_ok_and(|signature| {
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ECDSA_P256_SHA256_ASN1,
            parts(SigningPurpose::Endpoint).0,
        )
        .verify(b"PUBLIC_NATIVE_BUNDLE", &signature)
        .is_ok()
    });
    let relay = relay_trial(
        listener,
        origin,
        &bundle.relay,
        parts(SigningPurpose::Relay).0,
    )
    .await;
    let grant = grant_trial(bundle.grant.as_mut(), issuer, false).await;
    drop(bundle);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut renewers = Vec::new();
    for _ in 0..2 {
        let service = service.clone();
        let barrier = barrier.clone();
        renewers.push(std::thread::spawn(move || {
            barrier.wait();
            service.fresh_native_signers(Instant::now() + Duration::from_secs(2))
        }));
    }
    barrier.wait();
    let renewed: Vec<_> = renewers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().ok())
        .collect();
    let live = renewed
        .iter()
        .filter(|set| set.attempt.is_current())
        .count();
    drop(renewed);
    service.begin_shutdown();
    let retired = service.shutdown().await;
    assert!(retired.is_ok());
    assert!(
        public_matches
            && endpoint_verified
            && relay.0
            && relay.2
            && grant.0
            && old_grant.0
            && tls_first
            && tls_second
            && same_policy
    );
    assert_eq!(grant.1, Some(11000));
    assert_eq!(old_grant.1, grant.1);
    assert_eq!(live, 1);
    assert_eq!(opened.load(Ordering::SeqCst), 3);
    assert_eq!(
        service.test_key_observations(),
        vec![KeyRetirement::Retired; 3]
    );
}

#[cfg(feature = "remote-client-ipc")]
async fn native_tls_trial(endpoint: Arc<dyn SigningKey>, spki: &[u8]) -> bool {
    let host = LocalIdentity::new(Role::Host, P256Spki::new(spki).unwrap(), endpoint).unwrap();
    let (key, client_spki) = identities::identity(true, false);
    let client = LocalIdentity::new(
        Role::Client,
        P256Spki::new(&client_spki).unwrap(),
        key.key.clone(),
    )
    .unwrap();
    let (a, b) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(a),
            client,
            PeerPin::new(Role::Host, P256Spki::new(spki).unwrap()),
            Duration::from_secs(1),
            token.clone()
        ),
        accept(
            Unbound::new(b),
            host,
            PeerPin::new(Role::Client, P256Spki::new(&client_spki).unwrap()),
            Duration::from_secs(1),
            token
        )
    );
    let accepted = c.is_ok() && h.is_ok();
    drop((c, h));
    cancel.cancel();
    accepted
}

async fn relay_trial(
    listener: tokio::net::TcpListener,
    origin: String,
    identity: &RelayAdmissionIdentity,
    public: Vec<u8>,
) -> (bool, bool, bool) {
    use cc_desk_remote_transport::wss::{self, ConnectError};
    use futures_util::{SinkExt, StreamExt};
    let address = listener.local_addr().unwrap();
    let server_origin = origin.clone();
    let public = public.clone();
    let server = tokio::spawn(async move {
        let certs = rustls_pemfile::certs(
            &mut &include_bytes!("../../remote/crates/transport/tests/fixtures/chain.pem")[..],
        )
        .unwrap()
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();
        let key = rustls_pemfile::pkcs8_private_keys(
            &mut &include_bytes!("../../remote/crates/transport/tests/fixtures/end.key")[..],
        )
        .unwrap()
        .remove(0);
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key.into()))
        .unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(socket)
            .await
            .unwrap();
        let mut ws = tokio_tungstenite::accept_async_with_config(
            tls,
            Some(cc_desk_remote_transport::websocket_config()),
        )
        .await
        .unwrap();
        let mut challenge = vec![1, 0];
        challenge.extend([3; 32]);
        challenge.extend([4; 32]);
        ws.send(tokio_tungstenite::tungstenite::Message::Binary(
            challenge.into(),
        ))
        .await
        .unwrap();
        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(proof))) = ws.next().await {
            let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
            transcript.extend((server_origin.len() as u16).to_be_bytes());
            transcript.extend(server_origin.as_bytes());
            transcript.extend([3; 32]);
            transcript.extend([4; 32]);
            transcript.extend([1; 16]);
            transcript.push(1);
            let valid = proof.len() == 83
                && proof[..19] == [&[1, 1][..], &[1; 16], &[1][..]].concat()
                && ring::signature::UnparsedPublicKey::new(
                    &ring::signature::ECDSA_P256_SHA256_FIXED,
                    &public,
                )
                .verify(&transcript, &proof[19..])
                .is_ok();
            if valid {
                ws.send(tokio_tungstenite::tungstenite::Message::Binary(
                    vec![1, 2].into(),
                ))
                .await
                .unwrap();
            }
            valid
        } else {
            false
        }
    });
    let result = wss::test_only_connect_admitted_loopback(
        &origin,
        address,
        Duration::from_secs(2),
        identity,
    )
    .await;
    let accepted = result.is_ok();
    let denied = matches!(result, Err(ConnectError::Signer));
    drop(result);
    let mut server = server;
    let verified = match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
        Ok(Ok(value)) => value,
        _ => {
            server.abort();
            let _ = server.await;
            false
        }
    };
    (accepted, denied, verified)
}
#[tokio::test]
#[allow(non_snake_case)]
async fn CngAdapters_RelayActualLoopbackChecksOriginIdRoleBeforeSign_003() {
    let (point, spki) = fixture_spki();
    let public = point.clone();
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture_for(&point, SigningPurpose::Relay),
        SigningPurpose::Relay,
        spki,
    )
    .unwrap();
    let mut results = Vec::new();
    for mode in 0..4 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = format!("wss://foobar.com:{}/relay", address.port());
        let bound_origin = if mode == 1 {
            "wss://another.invalid/"
        } else {
            &origin
        };
        let signer = CngRelaySigner::new(
            handle.clone(),
            cc_desk_remote_record::appbind::CanonicalOrigin::trusted(bound_origin).unwrap(),
            if mode == 2 { [9; 16] } else { [1; 16] },
            if mode == 3 {
                RelayRole::Phone
            } else {
                RelayRole::Host
            },
        )
        .unwrap();
        let identity = RelayAdmissionIdentity::new([1; 16], RelayRole::Host, Arc::new(signer));
        let (accepted, denied, verified) =
            relay_trial(listener, origin, &identity, public.clone()).await;
        drop(identity);
        results.push((mode, accepted, denied, verified));
    }
    drop(handle);
    let retired = retire(&mut owner);
    assert!(retired);
    assert_eq!(owner.native_sign_count(), 1);
    for (mode, accepted, denied, verified) in results {
        assert_eq!((accepted, verified), (mode == 0, mode == 0));
        if mode != 0 {
            assert!(denied);
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn CngAdapters_SyncTlsOriginalHorizonReturnsUnknownWithoutDroppingOwner_005() {
    let (point, spki) = fixture_spki();
    let (entered, wait) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let (mut owner, handle) = KeyCustodian::start_for_test(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
        WorkerHooks {
            before_claim: None,
            after_native: Some(Box::new(move || {
                let _ = entered.send(());
                let _ = gate.recv();
            })),
        },
    )
    .unwrap();
    let key = CngTlsKey::new(handle, Instant::now() + Duration::from_millis(150)).unwrap();
    let (reply, result) = std::sync::mpsc::sync_channel(1);
    let caller = std::thread::spawn(move || {
        let signed = key
            .choose_scheme(&[rustls::SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap()
            .sign(b"PUBLIC_HORIZON");
        let _ = reply.send(signed.is_err());
    });
    let started = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let unknown = result.recv_timeout(Duration::from_secs(2));
    let retained = owner.holds_original_join();
    let pending = owner.observe();
    let _ = release.send(()); // Release BEFORE assertion, including timeout failures.
    let joined = caller.join().is_ok();
    let retired = retire(&mut owner);
    assert!(started && unknown == Ok(true) && retained && joined && retired);
    assert_eq!(pending, KeyRetirement::Pending);
    assert_eq!(owner.request_fact(), RequestFact::Unknown);
    assert!(owner.late_discarded());
}

#[cfg(feature = "remote-client-ipc")]
#[tokio::test]
#[allow(non_snake_case)]
async fn CngAdapters_AppExitWaitsForOriginalStartedKeyWorker_006() {
    use crate::remote_client_ipc::{ExitDecision, RemoteClientService};
    struct BlockedPublicProfile {
        spki: Vec<u8>,
        entered: std::sync::mpsc::SyncSender<()>,
        gate: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl NativeKeyProfile for BlockedPublicProfile {
        fn binding(&self, _: SigningPurpose) -> Result<Vec<u8>, SignFailure> {
            Ok(self.spki.clone())
        }
        fn open(&self, purpose: SigningPurpose) -> Result<NativeOwnedSoftwareKey, CryptoError> {
            let _ = self.entered.send(());
            let _ = self.gate.lock().unwrap().recv(); // Outside opaque CNG and app locks.
            let (point, _) = fixture_spki();
            import_public_rfc6979_fixture_for(&point, purpose)
        }
    }
    let service = Arc::new(RemoteClientService::default());
    let (_, spki) = fixture_spki();
    let (entered, wait) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let handle = service
        .install_key_profile(
            Some(Arc::new(BlockedPublicProfile {
                spki,
                entered,
                gate: std::sync::Mutex::new(gate),
            })),
            SigningPurpose::Endpoint,
        )
        .unwrap();
    let started = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let decision = service.request_exit(73);
    let (complete, mut finished) = tokio::sync::oneshot::channel();
    service.start_exit_waiter(73, move |code| {
        let _ = complete.send(code);
    });
    service.start_exit_waiter(73, |_| panic!("duplicate exit waiter"));
    let pending = tokio::time::timeout(Duration::from_millis(20), &mut finished)
        .await
        .is_err();
    let observation = service.test_key_observations();
    let wrong_code = service.request_exit(0);
    let _ = release.send(()); // Always unblock original ownership before assertions.
    let result = tokio::time::timeout(Duration::from_secs(3), finished).await;
    drop(handle);
    assert!(started && pending);
    assert_eq!(decision, ExitDecision::Wait(73));
    assert_eq!(observation, vec![KeyRetirement::Pending]);
    assert_eq!(wrong_code, ExitDecision::Prevent);
    assert_eq!(result, Ok(Ok(73)));
    assert_eq!(
        service.test_key_observations(),
        vec![KeyRetirement::Retired]
    );
    assert_eq!(service.request_exit(73), ExitDecision::Proceed);
}

#[cfg(feature = "remote-client-ipc")]
fn pause_gate() -> (
    Box<dyn FnOnce() + Send>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::SyncSender<()>,
) {
    let (entered, wait) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    (
        Box::new(move || {
            let _ = entered.send(());
            let _ = gate.recv();
        }),
        wait,
        release,
    )
}

#[cfg(feature = "remote-client-ipc")]
#[test]
#[allow(non_snake_case)]
fn CngFresh_CommonAdmissionGateQueuedCancelAndOldTicketIdentity_007() {
    let (point, spki) = fixture_spki();
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    let fence = SigningFence::new(Instant::now() + Duration::from_secs(20));
    let first = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let (hook, wait, release) = pause_gate();
    first.hook_admission(hook);
    let caller = handle.clone();
    let thread = std::thread::spawn(move || {
        caller
            .begin_attempt(b"OLD_PAUSED".to_vec(), SignatureEncoding::Fixed, first)
            .is_err()
    });
    let entered = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let second = fence.advance(
        std::slice::from_ref(&handle),
        Instant::now() + Duration::from_secs(5),
    );
    let _ = release.send(());
    let stale = thread.join().unwrap();
    let second = second.unwrap();
    let no_old_effect = owner.native_sign_count() == 0;
    let (hook, wait, release) = pause_gate();
    second.hook_claim(hook);
    let queued = handle
        .begin_attempt(b"QUEUED_CANCEL".to_vec(), SignatureEncoding::Fixed, second)
        .unwrap();
    let queued_entered = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let cancelled = queued.cancel();
    drop(queued);
    let busy = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .is_err();
    let _ = release.send(());
    let end = Instant::now() + Duration::from_secs(2);
    while owner.request_fact() != RequestFact::Delivered && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(2));
    }
    let no_queued_effect = owner.native_sign_count() == 0;
    let third = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let mut old = handle
        .begin_attempt(b"OLD_DELIVERED".to_vec(), SignatureEncoding::Fixed, third)
        .unwrap();
    let original = runtime.block_on(&mut old);
    let fourth = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let (hook, wait, release) = pause_gate();
    fourth.hook_returned(hook);
    let current = handle
        .begin_attempt(b"CURRENT".to_vec(), SignatureEncoding::Fixed, fourth)
        .unwrap();
    let current_entered = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let old_cancel = old.cancel();
    drop(old);
    let current_fact = owner.request_fact();
    let _ = release.send(());
    let signature = runtime.block_on(current);
    let retired = retire(&mut owner);
    assert!(
        retired
            && entered
            && stale
            && no_old_effect
            && queued_entered
            && busy
            && no_queued_effect
            && current_entered
    );
    assert_eq!(cancelled, RequestFact::CancelledNoEffect);
    assert_eq!(old_cancel, RequestFact::Delivered);
    assert_eq!(current_fact, RequestFact::Started);
    assert!(original.is_ok() && signature.is_ok());
    assert_eq!(owner.native_sign_count(), 2);
}

#[cfg(feature = "remote-client-ipc")]
#[test]
#[allow(non_snake_case)]
fn CngFresh_StartedUnknownNeverReopensOriginalKey_008() {
    let (point, spki) = fixture_spki();
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    let fence = SigningFence::new(Instant::now() + Duration::from_secs(20));
    let attempt = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let (hook, wait, release) = pause_gate();
    attempt.hook_returned(hook);
    let ticket = handle
        .begin_attempt(
            b"STARTED_UNKNOWN".to_vec(),
            SignatureEncoding::Fixed,
            attempt,
        )
        .unwrap();
    let entered = wait.recv_timeout(Duration::from_secs(2)).is_ok();
    let before = owner.request_fact();
    let active_refused = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .is_err();
    let unknown = ticket.cancel();
    drop(ticket);
    let pending = owner.observe();
    let _ = release.send(());
    let retired = retire(&mut owner);
    let refused = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(5),
        )
        .is_err();
    assert!(retired && entered && active_refused && refused);
    assert_eq!(before, RequestFact::Started);
    assert_eq!(unknown, RequestFact::Unknown);
    assert_eq!(pending, KeyRetirement::Pending);
    assert_eq!(owner.request_fact(), RequestFact::Unknown);
    assert_eq!(owner.native_sign_count(), 1);
}

#[cfg(feature = "remote-client-ipc")]
#[test]
#[allow(non_snake_case)]
fn CngFresh_OriginalSigningApprovalExpiryCannotBeExtended_009() {
    let (point, spki) = fixture_spki();
    let (mut owner, handle) = KeyCustodian::start(
        move || import_public_rfc6979_fixture(&point),
        SigningPurpose::Endpoint,
        spki,
    )
    .unwrap();
    let approved_until = Instant::now() + Duration::from_millis(300);
    let fence = SigningFence::new(approved_until);
    let attempt = fence
        .advance(std::slice::from_ref(&handle), approved_until)
        .unwrap();
    let original = handle
        .begin_attempt(
            b"APPROVED_ONLY".to_vec(),
            SignatureEncoding::Fixed,
            attempt.clone(),
        )
        .unwrap()
        .wait_blocking();
    let extended = fence
        .advance(
            std::slice::from_ref(&handle),
            approved_until + Duration::from_secs(1),
        )
        .is_err();
    while Instant::now() < approved_until {
        std::thread::sleep(Duration::from_millis(2));
    }
    let expired = fence
        .advance(
            std::slice::from_ref(&handle),
            Instant::now() + Duration::from_secs(1),
        )
        .is_err();
    let old = handle
        .begin_attempt(b"EXPIRED".to_vec(), SignatureEncoding::Fixed, attempt)
        .is_err();
    let retired = retire(&mut owner);
    assert!(retired && original.is_ok() && extended && expired && old);
    assert_eq!(owner.native_sign_count(), 1);
}
