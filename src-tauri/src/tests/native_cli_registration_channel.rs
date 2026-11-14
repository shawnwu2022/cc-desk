//! TEST_ONLY: actual Edge producer bytes through loopback mutual-RPK TLS, no real authenticator.
use super::*;
use crate::remote_host_controller::NativeRegistrationSession;
use cc_desk_remote_security::registration_channel::{
    registration_accept, registration_connect, RegistrationOutcome,
};
use cc_desk_remote_security::{cancellation, LocalIdentity, Unbound};
use rustls::sign::{Signer, SigningKey};

#[derive(Clone)]
struct SyntheticKey {
    pair: Arc<ring::signature::EcdsaKeyPair>,
    public: Vec<u8>,
}
impl std::fmt::Debug for SyntheticKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TEST_ONLY_PUBLISHED_SCALAR")
    }
}
impl Signer for SyntheticKey {
    fn sign(&self, bytes: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.pair
            .sign(&ring::rand::SystemRandom::new(), bytes)
            .map(|s| s.as_ref().to_vec())
            .map_err(|_| rustls::Error::General("TEST_SIGN".into()))
    }
    fn scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ECDSA_NISTP256_SHA256
    }
}
impl SigningKey for SyntheticKey {
    fn choose_scheme(&self, offered: &[rustls::SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&self.scheme())
            .then(|| Box::new(self.clone()) as Box<dyn Signer>)
    }
    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
    fn public_key(&self) -> Option<rustls::pki_types::SubjectPublicKeyInfoDer<'_>> {
        Some(self.public.as_slice().into())
    }
}
fn local(role: Role, n: usize) -> LocalIdentity {
    let public = key_bytes(n);
    let mut scalar = [0; 32];
    scalar[31] = n as u8;
    let pair = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        &scalar,
        &public[26..],
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    LocalIdentity::new(
        role,
        key(n),
        Arc::new(SyntheticKey {
            pair: Arc::new(pair),
            public,
        }),
    )
    .unwrap()
}
async fn response() -> Vec<u8> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        line.trim_end().as_bytes().to_vec()
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "TEST_ONLY protocol oracle requires an actual browser producer on stdin"]
async fn NativeRegistration_ChannelOracle_901() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_MODE").unwrap();
    assert!([
        "yes",
        "deny",
        "document",
        "foreign",
        "host-mismatch",
        "client-mismatch",
        "install-failed",
        "lost-ack",
        "lost-decision",
        "cancel-before-install",
        "rebind"
    ]
    .contains(&mode.as_str()));
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
    let document = crate::cli::document::DocumentAuthority::new(
        fixture.service.registry().clone(),
        url.clone(),
    )
    .unwrap();
    let mut resources = tauri::ResourceTable::default();
    let binding = Arc::new(document.attach(&mut resources).unwrap());
    document.started(&url);
    document.finished(&url);
    fixture.caller = document.test_caller();
    let host = Arc::new(RemoteHostService::default());
    let status = fixture.start();
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    let ready = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < ready);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut relay =
        NetworkRelayFixture::start_with_host_point(Some(key_bytes(1)[26..].to_vec())).await;
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, &status.run)
                .unwrap(),
            &status.run,
        )
        .unwrap();
    let controller = RemoteHostController::default();
    let opened = Arc::new([
        std::sync::atomic::AtomicUsize::new(0),
        std::sync::atomic::AtomicUsize::new(0),
    ]);
    let context = BrowserRegistrationContext {
        origin: "https://desk.invalid".into(),
        rp_id: "desk.invalid".into(),
        user_handle: vec![7; 32],
        owner_binding: [0; 32],
        tls_key: key(1),
        grant_key: key(2),
        relay_key: key(3),
        allow_backup_eligible: false,
        allow_backed_up: false,
    };
    let owner =
        NativeRegistrationOwner::begin(&binding, &fixture.caller, &admission, context).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (_cancel, token) = cancellation();
    let client_token = token.clone();
    let client_mode = mode.clone();
    let decision_loss = Arc::new(tokio::sync::Notify::new());
    let client_loss = decision_loss.clone();
    let mut client = Some(tokio::spawn(async move {
        let io = tokio::net::TcpStream::connect(address).await.unwrap();
        let n = if client_mode == "client-mismatch" {
            2
        } else {
            1
        };
        let mut channel = registration_connect(
            Unbound::new(io),
            local(Role::Client, n),
            network_pin(false),
            Duration::from_secs(5),
            Duration::from_secs(60),
            client_token,
        )
        .await
        .unwrap()
        .into_channel()
        .unwrap();
        let request = match channel.client_create().await {
            Ok(r) => r,
            Err(_) if client_mode == "client-mismatch" => return None,
            Err(e) => panic!("create: {e:?}"),
        };
        emit(&request);
        let created = response().await;
        emit(&channel.client_created(&created).await.unwrap());
        let asserted = response().await;
        let outcome = if client_mode == "lost-decision" {
            tokio::select! { biased; _ = client_loss.notified() => None, result = channel.client_asserted(&asserted) => Some(result.unwrap()) }
        } else {
            let result = channel.client_asserted(&asserted).await;
            if ["cancel-before-install", "rebind"].contains(&client_mode.as_str()) {
                assert!(result.is_err());
                return None;
            }
            Some(result.unwrap())
        };
        if client_mode == "lost-ack" || client_mode == "lost-decision" {
            channel.close();
        } else {
            channel.client_ack().await.unwrap();
        }
        outcome
    }));
    let (io, _) = listener.accept().await.unwrap();
    drop(listener);
    let n = if mode == "client-mismatch" { 2 } else { 1 };
    let channel = registration_accept(
        Unbound::new(io),
        network_local(false),
        PeerPin::new(Role::Client, key(n)),
        Duration::from_secs(5),
        Duration::from_secs(60),
        token,
    )
    .await
    .unwrap()
    .into_channel()
    .unwrap();
    let session =
        NativeRegistrationSession::new(owner, channel, &binding, &fixture.caller, &admission);
    let mut acknowledged = false;
    let mut receipt = None;
    let mut peer_outcome = None;
    if mode == "client-mismatch" {
        assert!(session.is_err());
    } else {
        let mut session = session.unwrap();
        session
            .verify(&binding, &fixture.caller, &admission)
            .await
            .unwrap();
        if mode == "rebind" {
            assert!(session
                .test_rebind(&binding, &fixture.caller, &admission)
                .is_err());
            assert!(session
                .install(&binding, &fixture.caller, &admission, &controller)
                .is_err());
        } else {
            assert!(session
                .install(&binding, &fixture.caller, &admission, &controller)
                .is_err());
            assert!(!controller.test_has_configuration());
            let mut fields =
                super::super::approval_keys::description(&relay, &admission, Arc::new(|| Ok(1000)));
            fields.phone_pin = PeerPin::new(Role::Client, key(1));
            fields.issuer = key(2);
            fields.browser = BrowserProfile::enrolled(
                "https://desk.invalid",
                "desk.invalid",
                vec![7; 32],
                key(4),
                false,
                false,
            )
            .unwrap();
            let cap = Instant::now() + Duration::from_secs(60);
            let frozen = if mode == "host-mismatch" {
                fields.endpoint_id = key(2).endpoint_id(Role::Host);
                FrozenHostApproval::description(fields, &admission, cap).unwrap()
            } else {
                let supplier = crate::native_crypto::test_existing_host_supplier(
                    cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin)
                        .unwrap(),
                    cap,
                    opened.clone(),
                );
                FrozenHostApproval::supplied(fields, &admission, cap, supplier).unwrap()
            };
            let approval =
                session.test_request_approval(&binding, &fixture.caller, &admission, frozen);
            if mode == "host-mismatch" {
                assert!(approval.is_err());
                acknowledged = session.reject().await.is_ok();
            } else {
                let approval = approval.unwrap();
                assert!(session
                    .install(&binding, &fixture.caller, &admission, &controller)
                    .is_err());
                approval.test_complete(mode != "deny");
                if mode == "cancel-before-install" {
                    _cancel.cancel();
                }
                let mut caller = fixture.caller.clone();
                if mode == "foreign" {
                    caller.window_label.push_str("-FOREIGN");
                }
                if mode == "document" {
                    document.started(&url);
                    document.finished(&url);
                }
                if mode == "install-failed" {
                    controller.begin_shutdown();
                }
                let installed = session.install(&binding, &caller, &admission, &controller);
                let expected = ["yes", "lost-ack", "lost-decision"].contains(&mode.as_str());
                assert_eq!(installed.is_ok(), expected);
                assert!(session
                    .install(&binding, &caller, &admission, &controller)
                    .is_err());
                match installed {
                    Ok(mut installed) => {
                        receipt = Some(installed.outcome().clone());
                        if mode == "lost-decision" {
                            decision_loss.notify_one();
                            peer_outcome = client.take().unwrap().await.unwrap();
                        }
                        acknowledged = installed.deliver().await.is_ok();
                        assert_eq!(acknowledged, mode == "yes");
                        assert_eq!(Some(installed.outcome().clone()), receipt);
                        assert!(installed.deliver().await.is_err());
                    }
                    Err(_) => {
                        acknowledged = session.reject().await.is_ok();
                    }
                }
            }
        }
    }
    if let Some(client) = client {
        peer_outcome = client.await.unwrap();
    }
    let installed = controller.test_has_configuration();
    assert_eq!(
        installed,
        ["yes", "lost-ack", "lost-decision"].contains(&mode.as_str())
    );
    if mode != "document" {
        assert_eq!(
            controller
                .current(&binding, &fixture.caller, &admission)
                .unwrap(),
            HostControlReply::Locked
        );
    }
    if installed {
        assert!(
            matches!(&receipt,Some(RegistrationOutcome::RunLocalCandidateInstalled{receipt,scope}) if receipt=="1" && *scope==admission.scope().unwrap())
        );
        if mode != "lost-decision" {
            assert_eq!(peer_outcome, receipt);
        }
    } else if !["client-mismatch", "cancel-before-install", "rebind"].contains(&mode.as_str()) {
        assert_eq!(peer_outcome, Some(RegistrationOutcome::Rejected));
        assert!(acknowledged);
    }
    assert_eq!(host.test_network_attempts(), 0);
    assert_eq!(opened[0].load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(opened[1].load(std::sync::atomic::Ordering::Acquire), 0);
    drop(controller);
    drop(admission);
    assert!(cleanup.close());
    assert!(relay.close().await);
    emit(&serde_json::to_vec(&serde_json::json!({"mode":mode,"nativeCandidateInstalled":installed,"locked":true,"hostNetworkAttempts":0,"hostKeyOpens":0,"registrationTlsConnections":1,"acknowledged":acknowledged,"receipt":receipt,"peerOutcome":peer_outcome,"genuineGuiApproval":false,"persistentTrust":false})).unwrap());
}
