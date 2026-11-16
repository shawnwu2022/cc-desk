//! Own-resource composition, not phone/WebView/Job-tree qualification.
use super::*;
use crate::cli::native_runtime::NativeRuntime;
use crate::remote_host::RemoteHostService;
use crate::run_supervisor::{NativeRunPin, NativeShutdown, NativeShutdownObservation};
use crate::tests::remote_channels as channels;
use cc_desk_remote_host::native_host::WriterRetirement;

use crate::tests::remote_channels as network_grants;
use crate::tests::remote_channels::identities as network_identities;

fn network_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn network_relay_material(host: bool) -> (Vec<u8>, Vec<u8>) {
    if host {
        let vector: Value = serde_json::from_slice(include_bytes!(
            "../../../remote/crates/security/tests/vectors/signatures/rfc6979-p256.json"
        ))
        .unwrap();
        return (
            network_hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"),
            network_hex(vector["public_key_sec1"].as_str().unwrap()),
        );
    }
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let vector: Value = serde_json::from_slice(include_bytes!(
        "../../../remote/crates/security/tests/vectors/cose/public-sign1-02.json"
    ))
    .unwrap();
    let mut point = vec![4];
    for name in ["x", "y"] {
        point.extend(B64.decode(vector[name].as_str().unwrap()).unwrap());
    }
    (
        B64.decode("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM")
            .unwrap(),
        point,
    )
}
struct NetworkRelaySigner {
    origin: String,
    id: [u8; 16],
    role: cc_desk_remote_transport::wss::RelayRole,
}
impl cc_desk_remote_transport::wss::RelaySigner for NetworkRelaySigner {
    fn sign<'a>(
        &'a self,
        r: &'a cc_desk_remote_transport::wss::SigningRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<[u8; 64], cc_desk_remote_transport::wss::SignError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            use cc_desk_remote_transport::wss::SignError;
            if r.origin() != self.origin || r.key_id() != self.id || r.role() != self.role {
                return Err(SignError);
            }
            let (scalar, point) =
                network_relay_material(self.role == cc_desk_remote_transport::wss::RelayRole::Host);
            let rng = ring::rand::SystemRandom::new();
            let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &scalar,
                &point,
                &rng,
            )
            .unwrap();
            Ok(key
                .sign(&rng, r.transcript())
                .unwrap()
                .as_ref()
                .try_into()
                .unwrap())
        })
    }
}
fn network_relay_identity(
    origin: &str,
    host: bool,
) -> cc_desk_remote_transport::wss::RelayAdmissionIdentity {
    use cc_desk_remote_transport::wss::*;
    let id = if host { [1; 16] } else { [2; 16] };
    let role = if host {
        RelayRole::Host
    } else {
        RelayRole::Phone
    };
    RelayAdmissionIdentity::new(
        id,
        role,
        Arc::new(NetworkRelaySigner {
            origin: origin.into(),
            id,
            role,
        }),
    )
}
fn network_browser_profile() -> cc_desk_remote_security::BrowserProfile {
    cc_desk_remote_security::BrowserProfile::enrolled(
        "https://desk.invalid",
        "desk.invalid",
        vec![7; 32],
        network_grants::admin(),
        false,
        false,
    )
    .unwrap()
}
struct NetworkBrowserSigner;
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkBrowserApproval {
    version: u16,
    cose: String,
    credential_id: String,
    authenticator_data: String,
    client_data_json: String,
    signature: String,
}
#[test]
fn network_browser_approval_preserves_original_canonical_encoding() {
    let approval = NetworkBrowserApproval {
        version: 1,
        cose: "c".into(),
        credential_id: "i".into(),
        authenticator_data: "a".into(),
        client_data_json: "j".into(),
        signature: "s".into(),
    };
    let canonical = serde_json::to_vec(&approval).unwrap();
    let legacy = serde_json::to_vec(&serde_json::to_value(&approval).unwrap()).unwrap();
    assert_eq!(canonical,br#"{"version":1,"cose":"c","credentialId":"i","authenticatorData":"a","clientDataJson":"j","signature":"s"}"#);
    assert_ne!(legacy, canonical);
    eprintln!("TEST_ONLY_APPROVAL_ENCODING legacy_first=authenticatorData canonical_first=version exact_equal=false");
}
impl cc_desk_remote_security::NativeGrantSigner for NetworkBrowserSigner {
    fn browser_profile(&self) -> Option<cc_desk_remote_security::BrowserProfile> {
        Some(network_browser_profile())
    }
    fn confirm_and_sign<'a>(
        &'a mut self,
        r: &'a cc_desk_remote_security::VerifiedCanonicalGrant,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<u8>, cc_desk_remote_security::GrantError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
            let challenge = r.browser_challenge(&network_browser_profile())?;
            let client=serde_json::to_vec(&json!({"type":"webauthn.get","challenge":B64.encode(challenge),"origin":"https://desk.invalid","crossOrigin":false})).unwrap();
            let mut auth = ring::digest::digest(&ring::digest::SHA256, b"desk.invalid")
                .as_ref()
                .to_vec();
            auth.push(5);
            auth.extend([0; 4]);
            let mut signed = auth.clone();
            signed.extend(ring::digest::digest(&ring::digest::SHA256, &client).as_ref());
            let mut scalar = [0; 32];
            scalar[31] = 1;
            let point=network_hex("046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5");
            let rng = ring::rand::SystemRandom::new();
            let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
                &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
                &scalar,
                &point,
                &rng,
            )
            .unwrap();
            Ok(serde_json::to_vec(&NetworkBrowserApproval {
                version: 1,
                cose: B64.encode(network_grants::signed(r.payload(), r.issuer_kid())),
                credential_id: B64.encode([7; 32]),
                authenticator_data: B64.encode(auth),
                client_data_json: B64.encode(client),
                signature: B64.encode(key.sign(&rng, &signed).unwrap().as_ref()),
            })
            .unwrap())
        })
    }
}
struct NetworkRelayFixture {
    child: std::process::Child,
    reader: Option<std::thread::JoinHandle<()>>,
    root: tempfile::TempDir,
    origin: String,
    address: std::net::SocketAddr,
}
impl NetworkRelayFixture {
    async fn start() -> Self {
        Self::start_with_host_point(None).await
    }
    async fn start_with_host_point(host_point: Option<Vec<u8>>) -> Self {
        Self::start_with_lifetime(host_point, 30_000).await
    }
    async fn start_with_lifetime(host_point: Option<Vec<u8>>, lifetime_ms: u64) -> Self {
        assert!((30_000..=120_000).contains(&lifetime_ms));
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserve.local_addr().unwrap();
        drop(reserve);
        let origin = format!("wss://foobar.com:{}/relay", address.port());
        let root = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let point = |host| {
            host_point
                .as_ref()
                .filter(|_| host)
                .cloned()
                .unwrap_or_else(|| network_relay_material(host).1)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        fs::write(root.path().join("policy.json"),serde_json::to_vec(&json!({"schema":1,"policy_id":"TEST_ONLY_NATIVE_NETWORK","revision":1,"issued_at_unix_ms":now,"not_before_unix_ms":now-1000,"expires_unix_ms":now+lifetime_ms,"listen":address.to_string(),"signed_wss_endpoint":origin,"browser_origins":[],"devices":[{"id":"01".repeat(16),"public_key_sec1":point(true),"role":"Host"},{"id":"02".repeat(16),"public_key_sec1":point(false),"role":"Phone"}],"routes":[{"tunnel":"03".repeat(16),"host":"01".repeat(16),"phone":"02".repeat(16)}]})).unwrap()).unwrap();
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let relay = std::env::var_os("CC_DESK_TEST_RELAY_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| manifest.join("../../cc-desk-relay"));
        let executable = std::env::var_os("CC_DESK_TEST_RELAY_EXE")
            .map(std::path::PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    relay.join(path)
                }
            })
            .unwrap_or_else(|| manifest.join("../target-browser-profile/debug/cc-desk-relay.exe"));
        assert!(
            relay.join("tests/fixtures/chain.pem").is_file(),
            "set CC_DESK_TEST_RELAY_ROOT to the fixture checkout"
        );
        assert!(
            executable.is_file(),
            "set CC_DESK_TEST_RELAY_EXE to the built fixture executable"
        );
        let mut child = std::process::Command::new(executable)
            .arg("--policy")
            .arg(root.path().join("policy.json"))
            .arg("--tls-chain")
            .arg(relay.join("tests/fixtures/chain.pem"))
            .arg("--tls-key")
            .arg(relay.join("tests/fixtures/end.key"))
            .arg("--tls-ca")
            .arg(relay.join("tests/fixtures/root.pem"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let reader = std::thread::spawn(move || {
            use std::io::BufRead;
            let mut line = String::new();
            let mut reader = std::io::BufReader::new(stdout);
            let _ = reader.read_line(&mut line);
            let _ = tx.send(line);
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        let mut owned = Self {
            child,
            reader: Some(reader),
            root,
            origin,
            address,
        };
        let ready = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .unwrap()
            .unwrap();
        if !ready.starts_with("WSS_LISTEN=") {
            use std::io::Read;
            let mut detail = String::new();
            owned
                .child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut detail)
                .unwrap();
            panic!("TEST_ONLY_RELAY_START {ready:?} {detail}");
        }
        owned
    }
    async fn close(&mut self) -> bool {
        let _ = fs::remove_file(self.root.path().join("policy.json"));
        let end = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(code) = self.child.try_wait().unwrap() {
                if let Some(reader) = self.reader.take() {
                    reader.join().unwrap();
                }
                return code.success();
            }
            if Instant::now() >= end {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for NetworkRelayFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.path().join("policy.json"));
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn network_local(client: bool) -> cc_desk_remote_security::LocalIdentity {
    use cc_desk_remote_security::*;
    let (k, p) = network_identities::identity(client, false);
    LocalIdentity::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
impl RemoteHostService {
    // 材料析构测试只复用原公开测试身份；此方法不进入生产批准入口。
    pub(crate) fn test_public_source_material_identity() -> cc_desk_remote_security::LocalIdentity {
        network_local(false)
    }
}
fn network_pin(client: bool) -> cc_desk_remote_security::PeerPin {
    use cc_desk_remote_security::*;
    let (_, p) = network_identities::identity(client, false);
    PeerPin::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
async fn network_client(
    relay: &NetworkRelayFixture,
    scope: cc_desk_remote_record::grant::SessionScope,
) -> Result<
    (
        cc_desk_remote_client::NativeClient,
        cc_desk_remote_transport::OwnedTunnel,
        cc_desk_remote_security::CancelHandle,
    ),
    &'static str,
> {
    network_client_mode(
        relay,
        scope,
        cc_desk_remote_transport::wss::RelayMode::Legacy,
    )
    .await
}
async fn network_client_mode(
    relay: &NetworkRelayFixture,
    scope: cc_desk_remote_record::grant::SessionScope,
    mode: cc_desk_remote_transport::wss::RelayMode,
) -> Result<
    (
        cc_desk_remote_client::NativeClient,
        cc_desk_remote_transport::OwnedTunnel,
        cc_desk_remote_security::CancelHandle,
    ),
    &'static str,
> {
    use cc_desk_remote_security::*;
    let budget = Duration::from_secs(8);
    let end = tokio::time::Instant::now() + budget;
    let remaining = || {
        end.checked_duration_since(tokio::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("phone setup deadline")
    };
    let socket = cc_desk_remote_transport::wss::test_only_connect_admitted_loopback_with_mode(
        &relay.origin,
        relay.address,
        budget,
        &network_relay_identity(&relay.origin, false),
        mode,
        [3; 16],
    )
    .await
    .map_err(|_| "phone relay")?;
    eprintln!("TEST_ONLY_PHONE_OUTER_ACCEPTED");
    let (stream, mux) =
        cc_desk_remote_transport::start_admitted(socket, [3; 16], Duration::from_secs(2))
            .map_err(|_| "phone mux")?;
    let (cancel, token) = cancellation();
    let result: Result<_, &'static str> = async {
        eprintln!("TEST_ONLY_PHONE_TLS_START");
        let pending = connect(
            Unbound::new(stream),
            network_local(true),
            network_pin(false),
            remaining()?,
            token,
        )
        .await
        .map_err(|error| {
            eprintln!("TEST_ONLY_PHONE_TLS_ERROR {error:?}");
            "phone TLS"
        })?;
        let channel = bind_client_peer_channel(
            pending,
            ClientTrust {
                host_key_epoch: 5,
                client_key_epoch: 7,
                origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin)
                    .unwrap(),
            },
        )
        .await
        .map_err(|_| "phone binding")?;
        let trust = ClientGrantTrust {
            issuer: network_grants::admin(),
            principal: [4; 32],
            scope,
            role: cc_desk_remote_record::grant::DataRole::Controller,
        };
        let mut signer = NetworkBrowserSigner;
        let mut state = ClientGrantState::new(Arc::new(|| Ok(1000)));
        let mut channel = channel
            .grant_client(&trust, &mut signer, &mut state, remaining()?)
            .await
            .map_err(|error| {
                eprintln!("TEST_ONLY_PHONE_GRANT_ERROR {error:?}");
                "phone browser grant"
            })?;
        let channel = channel.start_peer_pending(end).map_err(|_| "phone peer")?;
        let installed = channel
            .client_installed()
            .await
            .map_err(|_| "phone install")?;
        cc_desk_remote_client::NativeClient::from_installed(channel, installed)
            .map_err(|_| "phone client")
    }
    .await;
    match result {
        Ok(client) => Ok((client, mux, cancel)),
        Err(error) => {
            cancel.cancel();
            mux.request_close();
            let _ = mux.wait_closed().await;
            Err(error)
        }
    }
}
type NetworkTestPeer = (
    cc_desk_remote_client::NativeClient,
    cc_desk_remote_transport::OwnedTunnel,
    cc_desk_remote_security::CancelHandle,
);
async fn open_registered_network_peer(
    fixture: &Fixture,
    run: &RunKey,
    host: &Arc<RemoteHostService>,
    runtime: &Arc<NativeRuntime>,
    relay: &NetworkRelayFixture,
    source: (
        Arc<crate::remote_host_network::ApprovedHostSource>,
        Option<tokio::sync::oneshot::Sender<crate::remote_host_network::TestAttemptEnd>>,
    ),
    current: Option<&crate::remote_host_network::HostSourceToken>,
) -> Result<
    (
        Arc<crate::remote_host_network::HostNetworkAttempt>,
        NetworkTestPeer,
        crate::remote_host_network::HostSourceToken,
    ),
    &'static str,
> {
    use crate::remote_host_network::HostAttemptMaterials;
    let (source, termination) = source;
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, run)
                .map_err(|_| "registered access")?,
            run,
        )
        .map_err(|_| "registered admission")?;
    let scope = admission.scope().map_err(|_| "registered scope")?;
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let token = if let Some(token) = current {
        let material = HostAttemptMaterials {
            stages: None,
            local: network_local(false),
            relay: network_relay_identity(&relay.origin, true),
            outer_accepted: Some(Box::new((accepted, None))),
            termination,
        };
        host.replenish_native_source(runtime, &admission, token, material)
            .map_err(|_| "registered fresh material")?;
        token.clone()
    } else {
        let mut inputs = network_inputs(relay, scope.clone(), source);
        inputs.outer_accepted = Some(Box::new((accepted, None)));
        inputs.termination = termination;
        host.register_native_source(runtime, &admission, inputs)
            .map_err(|_| "registered source install")?
    };
    let attempt = host
        .test_begin_registered_network(runtime.clone(), admission)
        .map_err(|_| "registered source begin")?;
    if ready.await.is_err() {
        let _ = attempt.wait_closed().await;
        return Err("registered outer acceptance");
    }
    let (phone, admitted) = tokio::join!(network_client(relay, scope), attempt.wait_admitted());
    match (phone, admitted) {
        (Ok(peer), Ok(())) => Ok((attempt, peer, token)),
        (Ok(peer), Err(_)) => {
            let _ = retire_network_peer(peer, &attempt).await;
            Err("registered handoff")
        }
        (Err(error), _) => {
            attempt.request_close();
            let _ = attempt.wait_closed().await;
            Err(error)
        }
    }
}
fn network_inputs(
    relay: &NetworkRelayFixture,
    scope: cc_desk_remote_record::grant::SessionScope,
    source: Arc<crate::remote_host_network::ApprovedHostSource>,
) -> crate::remote_host_network::ApprovedHostInputs {
    use cc_desk_remote_security::*;
    crate::remote_host_network::ApprovedHostInputs {
        stages: None,
        address: relay.address,
        relay: network_relay_identity(&relay.origin, true),
        tunnel: [3; 16],
        relay_mode: cc_desk_remote_transport::wss::RelayMode::Legacy,
        local: network_local(false),
        phone_pin: network_pin(true),
        binding: HostSnapshot {
            host_key_epoch: 5,
            client_key_epoch: 7,
            activation: [0x33; 32],
            incarnation: 11,
            policy_epoch: 13,
            issuer_epoch: 17,
            origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&relay.origin)
                .unwrap(),
        },
        browser: network_browser_profile(),
        issuer: network_grants::admin(),
        principal: [4; 32],
        policy: NativeGrantPolicy {
            scope,
            role: cc_desk_remote_record::grant::DataRole::Controller,
            duration: Duration::from_secs(10),
        },
        source,
        setup_budget: Duration::from_secs(8),
        io_budget: Duration::from_secs(2),
        outer_accepted: None,
        termination: None,
    }
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ApprovalPureRevokeClosesIdle_011() {
    use crate::remote_host_network::*;
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
        tokio::task::yield_now().await;
    }
    let mut relay = NetworkRelayFixture::start().await;
    let (_source_watch, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(|| Ok(1000)),
        revoked,
        expires_host_ms: 11000,
    });
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
    let scope = admission.scope().unwrap();
    let (accepted, outer_ready) = tokio::sync::oneshot::channel();
    let (terminated, ending) = tokio::sync::oneshot::channel();
    let mut inputs = network_inputs(&relay, scope.clone(), source);
    inputs.outer_accepted = Some(Box::new((accepted, None)));
    inputs.termination = Some(terminated);
    let (approval, material) =
        crate::remote_host_approval::NativeHostApproval::test_from_inputs(inputs, &admission, true);
    assert!(approval
        .register_once(&runtime, &admission, &host, material)
        .is_ok());
    let attempt = host
        .test_begin_registered_network(runtime.clone(), admission)
        .unwrap();
    let mut peer = None;
    let outcome: Result<bool, &'static str> = async {
        outer_ready
            .await
            .map_err(|_| "pure revoke outer acceptance")?;
        let (phone, admitted) =
            tokio::join!(network_client(&relay, scope), attempt.wait_admitted());
        peer = Some(phone?);
        admitted.map_err(|_| "pure revoke handoff")?;
        if tokio::time::timeout(Duration::from_millis(100), attempt.wait_closed())
            .await
            .is_ok()
        {
            return Err("channel already ended before pure revoke");
        }
        // 原 peer/run 仍保管且原 grant 尚未到期；之后不发消息、不 poll UI 或授权。
        // 已完成原握手，之后不发消息、不 poll UI、不通知 watch 或显式退出。
        approval.test_revoke_only();
        if approval.test_watch_revoked() {
            return Err("pure revoke notified watch");
        }
        let joined = tokio::time::timeout(Duration::from_secs(1), attempt.wait_closed()).await;
        Ok(matches!(joined, Ok(Ok(()))))
    }
    .await;
    let joined_without_exit = outcome == Ok(true);
    // RED 也先收回全部原 owner，再报告失败。
    attempt.request_close();
    if let Some(peer) = peer.take() {
        let _ = retire_network_peer(peer, &attempt).await;
    }
    let _ = attempt.wait_closed().await;
    let end = ending.await.unwrap();
    let known = host.test_join_network().await;
    let physical = cleanup.close();
    let relay_closed = relay.close().await;
    eprintln!("TEST_ONLY_APPROVAL_PURE_REVOKE outcome={outcome:?} cause={:?} joined_without_exit={joined_without_exit} watch_notified={} original_network_joined={known}", end.cause, approval.test_watch_revoked());
    assert!(physical && relay_closed && known);
    assert!(
        joined_without_exit,
        "idle approved network ignored its original token: {outcome:?}"
    );
}
async fn retire_network_peer(
    peer: NetworkTestPeer,
    attempt: &crate::remote_host_network::HostNetworkAttempt,
) -> bool {
    let (client, mux, cancel) = peer;
    cancel.cancel();
    mux.request_close();
    let (c, m) = tokio::join!(client.close(), mux.wait_closed());
    c != cc_desk_remote_security::DataError::Uncertain
        && m.is_ok()
        && attempt.wait_closed().await.is_ok()
}
async fn open_network_peer(
    fixture: &Fixture,
    run: &RunKey,
    host: &Arc<RemoteHostService>,
    runtime: &Arc<NativeRuntime>,
    relay: &NetworkRelayFixture,
    source: Arc<crate::remote_host_network::ApprovedHostSource>,
) -> Result<
    (
        Arc<crate::remote_host_network::HostNetworkAttempt>,
        NetworkTestPeer,
    ),
    &'static str,
> {
    open_network_peer_observed(fixture, run, host, runtime, relay, source, None).await
}
async fn open_network_peer_observed(
    fixture: &Fixture,
    run: &RunKey,
    host: &Arc<RemoteHostService>,
    runtime: &Arc<NativeRuntime>,
    relay: &NetworkRelayFixture,
    source: Arc<crate::remote_host_network::ApprovedHostSource>,
    termination: Option<tokio::sync::oneshot::Sender<crate::remote_host_network::TestAttemptEnd>>,
) -> Result<
    (
        Arc<crate::remote_host_network::HostNetworkAttempt>,
        NetworkTestPeer,
    ),
    &'static str,
> {
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, run)
                .map_err(|_| "access")?,
            run,
        )
        .map_err(|_| "admission")?;
    let scope = admission.scope().map_err(|_| "scope")?;
    let (outer_accepted, accepted) = tokio::sync::oneshot::channel();
    let mut inputs = network_inputs(relay, scope.clone(), source);
    inputs.outer_accepted = Some(Box::new((outer_accepted, None)));
    inputs.termination = termination;
    let attempt = host
        .test_begin_admitted_network(runtime.clone(), admission, inputs)
        .map_err(|_| "native network entry")?;
    // 只观察真实 Relay Accepted；原建立截止不重置，尚无 Data 或安装权限。
    if accepted.await.is_err() {
        let result = attempt.wait_admitted().await;
        eprintln!("TEST_ONLY_HOST_BEFORE_PHONE {result:?}");
        let _ = attempt.wait_closed().await;
        return Err("host outer acceptance");
    }
    let (c, admitted) = tokio::join!(network_client(relay, scope), attempt.wait_admitted());
    eprintln!(
        "TEST_ONLY_NETWORK_SETUP host={admitted:?} phone={:?}",
        c.as_ref().map(|_| ()).map_err(|e| *e)
    );
    match (c, admitted) {
        (Ok(peer), Ok(())) => Ok((attempt, peer)),
        (Ok(peer), Err(_)) => {
            let _ = retire_network_peer(peer, &attempt).await;
            Err("native network handoff")
        }
        (Err(error), _) => {
            attempt.request_close();
            let _ = attempt.wait_closed().await;
            Err(error)
        }
    }
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_NetworkBootstrapUsesOriginalRelayAndOwnNode_003() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::{control_io as wire, input, projection};
    use cc_desk_remote_security::*;
    const BUDGET: Duration = Duration::from_secs(1);
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let (revoke, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(|| Ok(1000)),
        revoked,
        expires_host_ms: 11000,
    });
    let mut peer = None;
    let outcome:Result<(),&'static str>=async {
        let admission=fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller,&status.run).map_err(|_|"access")?,&status.run).map_err(|_|"admission")?;
        let scope=admission.scope().map_err(|_|"scope")?;
        let mut wrong=network_inputs(&relay,scope.clone(),source.clone());wrong.policy.scope.session="a5".repeat(16);
        if !matches!(host.test_begin_admitted_network(runtime.clone(),admission,wrong),Err(HostNetworkError::Authority)){return Err("wrong approved scope was silently rebound")}
        let(attempt,connected)=open_network_peer(&fixture,&status.run,&host,&runtime,&relay,source.clone()).await?;
        peer=Some(connected);
        let rejected=peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,Duration::from_secs(2)).await;
        if !matches!(rejected,Err(DataError::Bounds)){return Err("original snapshot budget bound changed")}
        eprintln!("TEST_ONLY_SNAPSHOT_BUDGET rejected=Bounds before_send=true");
        let snapshot=peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|error|{eprintln!("TEST_ONLY_SNAPSHOT_ERROR {error:?}");"original network snapshot"})?;
        if snapshot.stamp.target.run!=status.run.run_id || host.test_pin_count()!=1{return Err("wrong native resource")}
        let mut grid=String::new();for bytes in &snapshot.parts {let projection::Content::SnapshotPart(part)=projection::decode(bytes,1).map_err(|_|"snapshot codec")?.content else{return Err("snapshot part")};grid.extend(part.cells.iter().map(|cell|cell.scalar));}
        if !grid.contains("OWNED_READY"){return Err("original Node prefix missing")}
        let ack=projection::encode_ack(&projection::Ack{profile:1,stamp:snapshot.stamp.clone()},1).map_err(|_|"ACK codec")?;
        let acknowledged=peer.as_ref().unwrap().0.exchange(&ack,BUDGET).await.map_err(|_|"exact ACK")?;
        if !matches!(wire::decode(&acknowledged),Ok(wire::Message::ProjectionAcknowledged{stamp,..}) if stamp==snapshot.stamp){return Err("ACK identity")}
        let window=peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#,BUDGET).await.map_err(|_|"window")?;
        let wire::Message::WindowGranted{window,not_after_host_ms,..}=wire::decode(&window).map_err(|_|"window codec")? else{return Err("window reply")};
        let token:[u8;32]=network_hex(&window).try_into().map_err(|_|"window token")?;
        let deadline=not_after_host_ms.parse().map_err(|_|"window deadline")?;
        let frame=input::encode(&input::Input{operation:[0x21;16],sequence:1,window:token,not_after_host_ms:deadline,bytes:b"nat".to_vec()}).map_err(|_|"input codec")?;
        let receipt=peer.as_ref().unwrap().0.exchange(&frame,BUDGET).await.map_err(|_|"network input")?;
        if !matches!(wire::decode(&receipt),Ok(wire::Message::InputReceipt{operation,sequence,outcome:wire::Outcome::HostWritten,proof:Some(_),..}) if operation=="21".repeat(16)&&sequence=="1"){return Err("exact HostWritten receipt")}
        if !retire_network_peer(peer.take().unwrap(),&attempt).await{return Err("first original network join")}
        let(attempt2,connected2)=open_network_peer(&fixture,&status.run,&host,&runtime,&relay,source.clone()).await?;peer=Some(connected2);
        let snapshot2=peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|_|"rebound snapshot")?;
        if snapshot2.stamp.target!=snapshot.stamp.target||snapshot2.stamp.binding==snapshot.stamp.binding||host.test_pin_count()!=1{return Err("rebind copied resource or clock domain")}
        if peer.as_ref().unwrap().0.exchange(&ack,BUDGET).await.is_ok(){return Err("old ACK accepted after rebind")}
        if peer.as_ref().unwrap().0.exchange(&frame,BUDGET).await.is_ok(){return Err("old input advanced after rejected old ACK")}
        if !retire_network_peer(peer.take().unwrap(),&attempt2).await{return Err("second original network join")}
        let(attempt3,connected3)=open_network_peer(&fixture,&status.run,&host,&runtime,&relay,source.clone()).await?;peer=Some(connected3);
        let snapshot3=peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|_|"fresh third snapshot")?;
        let ack3=projection::encode_ack(&projection::Ack{profile:1,stamp:snapshot3.stamp.clone()},1).map_err(|_|"fresh ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack3,BUDGET).await.map_err(|_|"fresh ACK")?;
        let old_window=input::encode(&input::Input{operation:[0x22;16],sequence:2,window:token,not_after_host_ms:deadline,bytes:b"bad".to_vec()}).map_err(|_|"old window codec")?;
        if let Ok(reply)=peer.as_ref().unwrap().0.exchange(&old_window,BUDGET).await {
            if matches!(wire::decode(&reply),Ok(wire::Message::InputReceipt{outcome:wire::Outcome::HostWritten,..})){return Err("old window wrote through fresh ACK")}
        }
        if !retire_network_peer(peer.take().unwrap(),&attempt3).await{return Err("third original network join")}
        let fresh=fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller,&status.run).map_err(|_|"pre-revoke access")?,&status.run).map_err(|_|"pre-revoke admission")?;
        revoke.send_replace(true);revoke.send_replace(false);
        if !matches!(host.test_begin_admitted_network(runtime.clone(),fresh,network_inputs(&relay,scope,source.clone())),Err(HostNetworkError::Authority)){return Err("revoked source revived")}
        let access=fixture.service.access(&fixture.caller,&status.run).map_err(|_|"original local access")?;
        let written=access.write_frame(b"ive").map_err(|_|"original local continuation")?;
        if written.state!=crate::terminal_input::InputWriteState::HostWritten||written.confirmed_bytes!=3{return Err("local continuation changed")}
        drop(access);fixture.wait_lifecycle(|state|state.process()==ProcessLifecycle::Exited);
        let reports:Vec<Value>=fs::read_dir(fixture.root.path().join("work")).map_err(|_|"own report directory")?.filter_map(Result::ok).filter(|e|e.path().extension().is_some_and(|e|e=="json")).map(|e|serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap()).collect();
        if !reports.iter().any(|r|r["input"]==json!(b"native".to_vec())){return Err("Node observed replay or wrong exact bytes")}
        eprintln!("TEST_ONLY_NETWORK_DATA relay_proof_rpk_browser_grant_peer=true exact_ack=true host_written=3 reconnects=2 original_pins=1 old_ack_rejected=true old_window_rejected=true source_revocation_sticky=true exact_Node_bytes=6");
        Ok(())
    }.await;
    revoke.send_replace(true);
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        client.close().await;
        mux.request_close();
        let _ = mux.wait_closed().await;
    }
    let retired = cleanup.close();
    let network = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(
        retired && network && relay_closed,
        "all original test owners must retire"
    );
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ProviderReentryHasNoServiceOrCanonicalLock_004() {
    use cc_desk_remote_security::{GrantClock, GrantError};
    use std::sync::atomic::AtomicBool;
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    );
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
    let scope = admission.scope().unwrap();
    let canonical_probe = admission.test_canonical_probe();
    let caller_thread = Arc::new(Mutex::new(None));
    let service_locked = Arc::new(AtomicBool::new(false));
    let canonical_locked = Arc::new(AtomicBool::new(false));
    let reentered = Arc::new(AtomicBool::new(false));
    let callback_thread = caller_thread.clone();
    let weak_host = Arc::downgrade(&host);
    let observed_service = service_locked.clone();
    let observed_canonical = canonical_locked.clone();
    let observed_reentry = reentered.clone();
    let original_supervisor = fixture.supervisor.clone();
    let run = status.run.clone();
    let expected_scope = scope.clone();
    let clock: Arc<dyn GrantClock> = Arc::new(move || {
        if callback_thread.lock().as_ref() == Some(&std::thread::current().id()) {
            let service = weak_host.upgrade().ok_or(GrantError::Revoked)?;
            let service_free = service.test_state_is_unlocked();
            let canonical_free = canonical_probe();
            if !service_free {
                observed_service.store(true, Ordering::Release);
            }
            if !canonical_free {
                observed_canonical.store(true, Ordering::Release);
            }
            if !service_free || !canonical_free {
                return Err(GrantError::Clock);
            }
            if !service.network_admission_open()
                || original_supervisor
                    .pin_shutdown(&run)
                    .native_scope()
                    .ok()
                    .as_ref()
                    != Some(&expected_scope)
            {
                return Err(GrantError::Revoked);
            }
            observed_reentry.store(true, Ordering::Release);
        }
        Ok(1000)
    });
    let (c, h, gate, cancel) = channels::channels_with_clock(scope, clock).await;
    cleanup.proofs.extend([c.authority(), h.authority()]);
    let h = Arc::new(h);
    *caller_thread.lock() = Some(std::thread::current().id());
    let adopted = match host.adopt_admitted(admission, h.clone(), &runtime) {
        Ok(()) => true,
        Err(rejected) => {
            drop(rejected);
            false
        }
    };
    *caller_thread.lock() = None;
    let installed = if adopted {
        c.client_installed().await.is_ok()
    } else {
        false
    };
    cancel.cancel();
    c.authority().request_close();
    h.authority().request_close();
    let _ = tokio::join!(c.wait_closed(), h.wait_closed());
    let retired = cleanup.close();
    let network = host.test_join_network().await;
    drop(gate);
    eprintln!("TEST_ONLY_PROVIDER_REENTRY service_locked={} canonical_locked={} reentry={} installed={installed}", service_locked.load(Ordering::Acquire), canonical_locked.load(Ordering::Acquire), reentered.load(Ordering::Acquire));
    assert!(retired && network);
    assert!(
        adopted
            && installed
            && reentered.load(Ordering::Acquire)
            && !service_locked.load(Ordering::Acquire)
            && !canonical_locked.load(Ordering::Acquire),
        "provider ran under an ownership lock"
    );
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_LiveNetworkClockPanicKeepsUnknownAfterActualJoin_006() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::projection;
    use std::sync::atomic::AtomicBool;
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let (revoke, revoked) = tokio::sync::watch::channel(false);
    let panic_next = Arc::new(AtomicBool::new(false));
    let panic_seen = Arc::new(AtomicBool::new(false));
    let armed = panic_next.clone();
    let observed = panic_seen.clone();
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(move || {
            if armed.swap(false, Ordering::AcqRel) {
                observed.store(true, Ordering::Release);
                panic!("TEST_ONLY_LIVE_NETWORK_CLOCK_PANIC");
            }
            Ok(1000)
        }),
        revoked,
        expires_host_ms: 11000,
    });
    let mut peer = None;
    let outcome: Result<(), &'static str> = async {
        let (attempt, connected) = open_network_peer(&fixture, &status.run, &host, &runtime, &relay, source.clone()).await?;
        peer = Some(connected);
        let snapshot = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "live snapshot")?;
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: snapshot.stamp.clone() }, 1).map_err(|_| "live ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "live exact ACK")?;
        panic_next.store(true, Ordering::Release);
        if peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, Duration::from_secs(1)).await.is_ok() { return Err("clock fault returned a live Window"); }
        if !panic_seen.load(Ordering::Acquire) { return Err("original pump/engine did not sample the armed clock"); }
        let fresh = fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller, &status.run).map_err(|_| "fault local access")?, &status.run).map_err(|_| "fault fresh admission")?;
        let scope = fresh.scope().map_err(|_| "fault fresh scope")?;
        match host.test_begin_admitted_network(runtime.clone(), fresh, network_inputs(&relay, scope, source.clone())) {
            Err(HostNetworkError::Unknown) => {}
            Ok(late) => { late.request_close(); let _ = late.wait_closed().await; return Err("late clock fault reopened custody"); }
            Err(_) => return Err("late clock fault lost Unknown classification"),
        }
        if attempt.wait_closed().await != Err(HostNetworkError::Unknown) { return Err("original joined fault was reported known"); }
        let (client, mux, cancel) = peer.take().unwrap();
        cancel.cancel(); mux.request_close();
        let (client_terminal, wire_terminal) = tokio::join!(client.close(), mux.wait_closed());
        if host.test_join_network().await { return Err("joined Unknown custody was reported retired"); }
        let access = fixture.service.access(&fixture.caller, &status.run).map_err(|_| "fault local continuation")?;
        let written = access.write_frame(b"native").map_err(|_| "fault local frame")?;
        if written.state != crate::terminal_input::InputWriteState::HostWritten || written.confirmed_bytes != 6 { return Err("fault changed local native input"); }
        drop(access);
        fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
        let reports: Vec<Value> = fs::read_dir(fixture.root.path().join("work")).map_err(|_| "fault reports")?.filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).map(|entry| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap()).collect();
        if !reports.iter().any(|report| report["input"] == json!(b"native".to_vec())) { return Err("fault lost exact local Node input"); }
        eprintln!("TEST_ONLY_LIVE_CLOCK_FAULT fresh_begin=Unknown original_attempt_join=Unknown custody_known=false local_Node_bytes=6 phone_terminal={client_terminal:?} wire_terminal={wire_terminal:?}");
        Ok(())
    }.await;
    revoke.send_replace(true);
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
    }
    let physical = cleanup.close();
    let known = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(
        physical && relay_closed,
        "all original self-owned native/relay resources must close"
    );
    assert!(!known, "Unknown must survive actual network joins");
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_LiveClockFaultWithoutFreshAdmissionNaturallyJoins_007() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::projection;
    use std::sync::{atomic::AtomicBool, OnceLock};
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let (revoke, revoked) = tokio::sync::watch::channel(false);
    let panic_next = Arc::new(AtomicBool::new(false));
    let fault_at = Arc::new(OnceLock::new());
    let armed = panic_next.clone();
    let recorded_fault = fault_at.clone();
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(move || {
            if armed.swap(false, Ordering::AcqRel) {
                let _ = recorded_fault.set(tokio::time::Instant::now());
                panic!("TEST_ONLY_NATURAL_CLOCK_PANIC");
            }
            Ok(1000)
        }),
        revoked,
        expires_host_ms: 11000,
    });
    let (selected, ending) = tokio::sync::oneshot::channel();
    let mut peer = None;
    let outcome: Result<(), &'static str> = async {
        let (attempt, connected) = open_network_peer_observed(&fixture, &status.run, &host, &runtime, &relay, source, Some(selected)).await?;
        peer = Some(connected);
        let snapshot = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "natural snapshot")?;
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: snapshot.stamp }, 1).map_err(|_| "natural ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "natural ACK")?;
        panic_next.store(true, Ordering::Release);
        let window = peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, Duration::from_secs(1)).await;
        if window.is_ok() { return Err("natural fault returned Window"); }
        let fault = *fault_at.get().ok_or("natural clock was not sampled")?;
        // 这里只观察原 handle；不发 stop、不创建新准入，不把观察 timeout 当作退休。
        let joined = tokio::time::timeout(Duration::from_secs(4), attempt.wait_closed()).await;
        let observed_at = tokio::time::Instant::now();
        let joined = match joined {
            Ok(result) => result,
            Err(_) => { eprintln!("TEST_ONLY_NATURAL_TIMEOUT fault_to_observer_ms={} retained_original_handle=true", observed_at.duration_since(fault).as_millis()); return Err("natural original join observation timed out"); }
        };
        if joined != Err(HostNetworkError::Unknown) { return Err("natural original join lost Unknown"); }
        let end = ending.await.map_err(|_| "natural task ending missing")?;
        let lease = end.lease_end.ok_or("natural lease missing")?;
        let mux = end.mux.ok_or("natural mux ending missing")?;
        eprintln!("TEST_ONLY_NATURAL_END cause={:?} fault_to_select_ms={:?} fault_to_join_ms={} lease_remaining_at_fault_ms={:?} selected_before_lease={} mux_cause={:?} mux_result={} fault_to_mux_ms={:?} window_error={:?} no_fresh_admission=true no_external_stop_before_join=true", end.cause, end.selected_at.checked_duration_since(fault).map(|value| value.as_millis()), observed_at.duration_since(fault).as_millis(), lease.checked_duration_since(fault).map(|value| value.as_millis()), end.selected_at < lease, mux.cause, mux.result, mux.selected_at.checked_duration_since(fault).map(|value| value.as_millis()), window.err());
        if end.cause != TestAttemptCause::Channel || end.selected_at >= lease { return Err("natural task ended through stop or lease rather than original channel"); }
        // 从此处开始才允许显式回收 Phone、mux、source 和 custody。
        let (client, mux, cancel) = peer.take().unwrap();
        cancel.cancel(); mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
        if host.test_join_network().await { return Err("natural Unknown custody became known"); }
        let access = fixture.service.access(&fixture.caller, &status.run).map_err(|_| "natural local access")?;
        let written = access.write_frame(b"native").map_err(|_| "natural local frame")?;
        if written.state != crate::terminal_input::InputWriteState::HostWritten || written.confirmed_bytes != 6 { return Err("natural fault changed local input"); }
        drop(access);
        fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
        let reports: Vec<Value> = fs::read_dir(fixture.root.path().join("work")).map_err(|_| "natural reports")?.filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).map(|entry| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap()).collect();
        if !reports.iter().any(|report| report["input"] == json!(b"native".to_vec())) { return Err("natural Node input mismatch"); }
        eprintln!("TEST_ONLY_NATURAL_LOCAL continuation_bytes=6 exact_Node=true");
        Ok(())
    }.await;
    revoke.send_replace(true);
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
    }
    let physical = cleanup.close();
    let known = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(
        physical && relay_closed,
        "all original self-owned resources must close"
    );
    assert!(
        !known,
        "natural fault must retain Unknown after actual joins"
    );
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_RegisteredSourceConsumesFreshMaterialsAndRebinds_008() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::{control_io as wire, input, projection};
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let (_revoke, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(|| Ok(1000)),
        revoked,
        expires_host_ms: 11000,
    });
    let mut peer = None;
    let outcome: Result<(), &'static str> = async {
        let absent = fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller, &status.run).map_err(|_| "absent access")?, &status.run).map_err(|_| "absent admission")?;
        if !matches!(host.test_begin_registered_network(runtime.clone(), absent), Err(HostNetworkError::Locked)) || host.test_network_attempts() != 0 { return Err("absent source started network"); }
        let admit = || fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller, &status.run).map_err(|_| "slot boundary access")?, &status.run).map_err(|_| "slot boundary admission");
        let original = admit()?;
        let scope = original.scope().map_err(|_| "slot boundary scope")?;
        // Fixture-only callback crosses the concrete bridge with original admission.
        let approval_host = Arc::new(RemoteHostService::default());
        let approval_reference_count = original.test_resource_strong_count();
        let (approval, material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        if original.test_resource_strong_count() != approval_reference_count { return Err("approval retained original run"); }
        approval.register_once(&runtime, &original, &approval_host, material).map_err(|_| "typed approval registration")?;
        let (_, second_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        if approval.register_once(&runtime, &original, &approval_host, second_material).is_ok() { return Err("approval reused"); }
        approval.test_revoke_only();
        let (publication, published) = tokio::sync::watch::channel(None);
        let (_stop, stopped) = tokio::sync::watch::channel(false);
        let committed = approval_host.commit_network_handoff(&publication, &stopped, tokio::time::Instant::now() + Duration::from_secs(30));
        let refused_before_full_cancel = committed.is_err() && published.borrow().is_none() && !approval_host.network_admission_open();
        eprintln!("TEST_ONLY_APPROVAL_FINAL_GATE commit={committed:?} publication={:?} refused_before_full_cancel={refused_before_full_cancel}", *published.borrow());
        approval.cancel();
        if !refused_before_full_cancel { return Err("pure revoked source passed final publication gate"); }
        if !matches!(approval_host.test_begin_registered_network(runtime.clone(), admit()?), Err(HostNetworkError::Closed)) { return Err("cancelled approval opened network"); }
        let (denied, denied_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, false);
        if denied.register_once(&runtime, &original, &Arc::new(RemoteHostService::default()), denied_material).is_ok() { return Err("denied approval registered"); }
        let (mismatch, mut wrong_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        wrong_material.local = network_local(true);
        if mismatch.register_once(&runtime, &original, &Arc::new(RemoteHostService::default()), wrong_material).is_ok() { return Err("unapproved local identity registered"); }
        let (expired, expired_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        expired.test_expire();
        if expired.register_once(&runtime, &original, &Arc::new(RemoteHostService::default()), expired_material).is_ok() { return Err("expired approval registered"); }
        let (bad_clock, clock_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(
            network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        bad_clock.test_fail_clock();
        if bad_clock.register_once(&runtime, &original, &Arc::new(RemoteHostService::default()), clock_material).is_ok() { return Err("failed approval clock registered"); }
        // 独立真实 run/admission 不能消费原 run 的批准。
        let other_transports = Arc::new(TerminalTransports::new());
        let other_supervisor = Arc::new(NativeRunSupervisor::new(other_transports.clone()));
        let mut other_fixture = Fixture::with_runtime_environment("input", other_supervisor, other_transports, true);
        let document_url:tauri::Url="http://tauri.localhost/".parse().unwrap();
        let document=crate::cli::document::DocumentAuthority::new(other_fixture.service.registry().clone(),document_url.clone()).map_err(|_| "controller document")?;
        let mut document_resources=tauri::ResourceTable::default();
        let document_binding=Arc::new(document.attach(&mut document_resources).map_err(|_| "controller binding")?);
        document.started(&document_url);document.finished(&document_url);
        other_fixture.caller=document.test_caller();
        let other_status = other_fixture.start();
        let other_host = Arc::new(RemoteHostService::default());
        let other_runtime = NativeRuntime::with_owned_components_for_test(other_fixture.service.clone(), other_fixture.transports.clone(), other_fixture.supervisor.clone());
        let mut other_cleanup = Cleanup { supervisor:other_fixture.supervisor.clone(),host:other_host.clone(),run:other_status.run.clone(),pin:Some(other_fixture.supervisor.pin_shutdown(&other_status.run)),stop:None,proofs:Vec::new() };
        let other_ready = Instant::now() + Duration::from_secs(10);
        while other_fixture.child_reports() == 0 || other_fixture.events.lock().is_empty() {
            if Instant::now() >= other_ready { return Err("independent approval run not ready"); }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let other_admission = other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller, &other_status.run).map_err(|_| "independent access")?, &other_status.run).map_err(|_| "independent admission")?;
        if other_status.run != status.run { return Err("independent fixture must preserve textual run key"); }
        let (bound_owner, bound_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(network_inputs(&relay, scope.clone(), source.clone()), &original, true);
        if bound_owner.register_once(&other_runtime, &other_admission, &other_host, bound_material).is_ok() || other_host.test_network_attempts() != 0 { return Err("approval crossed real run"); }
        let original_token = bound_owner.test_native_token();
        if original_token.is_revoked() { return Err("foreign request revoked original approval"); }
        let claim_host = Arc::new(RemoteHostService::default());
        let claim_reservation = claim_host.reserve_activation().map_err(|_| "original claim reservation")?;
        let original_claim = bound_owner.claim_activation(&original).map_err(|_| "foreign request consumed original approval")?;
        let original_ticket = original_claim.validate(&original).map_err(|_| "original claim no longer valid")?;
        if original_token.is_revoked() { return Err("original validated claim lost authority"); }
        drop(original_ticket);
        drop(claim_reservation);
        // 空配置完整鉴权后 Locked；错误 run 不得消耗原配置。
        use crate::remote_host_controller::{RemoteHostController,HostControlReply};
        let other_scope_for_controller=other_admission.scope().map_err(|_| "controller scope")?;
        let make_description=|| {
            let (owner,material)=crate::remote_host_approval::NativeHostApproval::test_from_inputs(network_inputs(&relay,other_scope_for_controller.clone(),source.clone()),&other_admission,true);
            (owner.test_take_description(),material)
        };
        let controller=Arc::new(RemoteHostController::default());
        if controller.current(&document_binding,&other_fixture.caller,&other_admission).map_err(|_| "empty controller")? != HostControlReply::Locked {return Err("empty controller not locked");}
        let (entered_tx,entered_rx)=std::sync::mpsc::channel();let (release_tx,release_rx)=std::sync::mpsc::channel();
        let (_,worker_material)=make_description();
        let (description,_unused)=make_description();
        controller.test_install_preparation(&document_binding,other_fixture.caller.clone(),&other_admission,description,std::time::Instant::now()+Duration::from_secs(60),Box::new(move ||{let _=entered_tx.send(());let _=release_rx.recv_timeout(Duration::from_secs(5));worker_material})).map_err(|_| "controller config")?;
        if controller.test_request(&document_binding,&other_fixture.caller,&original,|_,_|panic!("wrong run must not show")).is_ok(){return Err("controller accepted foreign run");}
        if controller.cancel(&document_binding,&other_fixture.caller,&original).err().map(|e|e.code)!=Some("FORBIDDEN".into()) {return Err("foreign configuration cancel not forbidden");}

        let requested=controller.test_request(&document_binding,&other_fixture.caller,&other_admission,|description,_|Ok(Arc::new(crate::remote_host_approval::NativeHostApproval::test_record_description(description,&other_admission)))).map_err(|_| "original configuration consumed by foreign request")?;
        if requested != HostControlReply::ApprovalRecorded {return Err("native approval not recorded");}
        // 原工作线程先保管，fixture factory 等待受控释放；取消不能使其脱离保管。
        let worker_admission=other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller,&other_status.run).map_err(|_| "worker access")?,&other_status.run).map_err(|_| "worker admission")?;
        controller.test_prepare_owned(&document_binding,&other_fixture.caller,worker_admission,Arc::new(NativeRuntime::with_owned_components_for_test(other_fixture.service.clone(),other_fixture.transports.clone(),other_fixture.supervisor.clone())),other_host.clone()).map_err(|_| "fixture operation start")?;
        let entered=entered_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        let pending=controller.test_finish_preparation().is_none();
        let consumed=controller.current(&document_binding,&other_fixture.caller,&other_admission).map_err(|_| "preparing state")?==HostControlReply::Consumed;
        controller.cancel(&document_binding,&other_fixture.caller,&other_admission).map_err(|_| "cancel preparing")?;
        let _=release_tx.send(());
        let join_end = Instant::now() + Duration::from_secs(10);
        let joined = loop {
            if let Some(result) = controller.test_finish_preparation() {
                break Some(result);
            }
            if Instant::now() >= join_end {
                break None;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        if !entered || !pending || !consumed || joined!=Some(false) || other_host.test_network_attempts()!=0 {return Err("cancelled factory publication/custody");}
        // 工厂在批准前绑定。准备成功后，查询不登记；显式推进才验证并消费原结果。
        for mode in 0..3 {
            let prepared_controller=RemoteHostController::default();let prepared_host=Arc::new(RemoteHostService::default());
            let (mut description,material)=make_description();
            if mode==1 {description.relay_binding.0=[0x7f;16];}
            let samples=Arc::new(std::sync::atomic::AtomicUsize::new(0));let sample_count=samples.clone();let clock=description.clock.clone();
            description.clock=Arc::new(move ||{sample_count.fetch_add(1,std::sync::atomic::Ordering::AcqRel);clock.now_ms()});
            let (factory_tx,factory_rx)=std::sync::mpsc::sync_channel(1);
            let prepared_owner=Arc::new(parking_lot::Mutex::new(std::sync::Weak::<crate::remote_host_approval::NativeHostApproval>::new()));
            let expiring_owner=prepared_owner.clone();
            prepared_controller.test_install_preparation(&document_binding,other_fixture.caller.clone(),&other_admission,description,Instant::now()+Duration::from_secs(60),Box::new(move||{
                if mode==2 {if let Some(owner)=expiring_owner.lock().upgrade(){owner.test_expire();owner.poll_expiry();}}
                let _=factory_tx.send(std::thread::current().id());material
            })).map_err(|_|"prepared config")?;
            prepared_controller.test_request(&document_binding,&other_fixture.caller,&other_admission,|description,_|{
                let owner=Arc::new(crate::remote_host_approval::NativeHostApproval::test_record_description(description,&other_admission));
                *prepared_owner.lock()=Arc::downgrade(&owner);Ok(owner)
            }).map_err(|_|"prepared approval")?;
            let prepare_admission=other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller,&other_status.run).map_err(|_|"prepared access")?,&other_status.run).map_err(|_|"prepared admission")?;
            prepared_controller.test_prepare_owned(&document_binding,&other_fixture.caller,prepare_admission,Arc::new(NativeRuntime::with_owned_components_for_test(other_fixture.service.clone(),other_fixture.transports.clone(),other_fixture.supervisor.clone())),prepared_host.clone()).map_err(|_|"prepared start")?;
            let factory_thread=factory_rx.recv_timeout(Duration::from_secs(5)).map_err(|_|"prepared factory not called")?;
            if factory_thread==std::thread::current().id() {return Err("factory used observer thread");}
            let current=prepared_controller.current(&document_binding,&other_fixture.caller,&other_admission).map_err(|_|"prepared current")?;
            if mode!=2 && current!=HostControlReply::Consumed {return Err("query registered prepared result");}
            if samples.load(std::sync::atomic::Ordering::Acquire)!=1 {return Err("query sampled external clock");}
            let duplicate=other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller,&other_status.run).map_err(|_|"duplicate access")?,&other_status.run).map_err(|_|"duplicate admission")?;
            if prepared_controller.test_prepare_owned(&document_binding,&other_fixture.caller,duplicate,Arc::new(NativeRuntime::with_owned_components_for_test(other_fixture.service.clone(),other_fixture.transports.clone(),other_fixture.supervisor.clone())),prepared_host.clone()).is_ok(){return Err("duplicate preparation accepted");}
            let finish_end=Instant::now()+Duration::from_secs(5);
            let completed = loop {
                if let Some(result) = prepared_controller.test_finish_preparation() {
                    break result;
                }
                if Instant::now() >= finish_end {
                    return Err("prepared result join timeout");
                }
                std::thread::yield_now();
            };
            if completed!=(mode==0) || prepared_host.test_network_attempts()!=0 {return Err("prepared identity or expiry boundary");}
            if mode==0 && (samples.load(std::sync::atomic::Ordering::Acquire)!=2 || prepared_controller.current(&document_binding,&other_fixture.caller,&other_admission).map_err(|_|"registered current")?!=HostControlReply::Registered){return Err("explicit prepared registration missing");}
            prepared_controller.begin_shutdown();
        }
        // show 尚未返回时，同 owner cancel 必须使迟到结果不可发布。
        let delayed=RemoteHostController::default();let (description,_unused)=make_description();
        delayed.install_native_configuration(&document_binding,other_fixture.caller.clone(),&other_admission,description,std::time::Instant::now()+Duration::from_secs(60)).map_err(|_| "delayed config")?;
        let result=delayed.test_request(&document_binding,&other_fixture.caller,&other_admission,|description,_|{
            if delayed.test_request(&document_binding,&other_fixture.caller,&original,|_,_|panic!("foreign pending show")).err().map(|e|e.code)!=Some("FORBIDDEN".into()) {
                return Err(crate::cli::profiles::error("TEST_FOREIGN_PENDING"));
            }
            delayed.cancel(&document_binding,&other_fixture.caller,&other_admission)?;Ok(Arc::new(crate::remote_host_approval::NativeHostApproval::test_record_description(description,&other_admission)))}).map_err(|_| "delayed request")?;
        if result!=HostControlReply::Cancelled {return Err("cancelled show published");}
        // show 在原调用线程等待；另一条真实线程按同 document/run 取消。
        let concurrent=Arc::new(RemoteHostController::default());
        let (description,_unused)=make_description();
        concurrent.install_native_configuration(&document_binding,other_fixture.caller.clone(),&other_admission,description,std::time::Instant::now()+Duration::from_secs(60)).map_err(|_| "thread config")?;
        let cancel_admission=other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller,&other_status.run).map_err(|_| "cancel access")?,&other_status.run).map_err(|_| "cancel admission")?;
        let (entered_tx,entered_rx)=std::sync::mpsc::channel();
        let (release_tx,release_rx)=std::sync::mpsc::channel();
        let cancelling=concurrent.clone();let cancelling_binding=document_binding.clone();let cancelling_caller=other_fixture.caller.clone();
        let cancel_thread=std::thread::spawn(move || {
            let result=if entered_rx.recv_timeout(Duration::from_secs(5)).is_ok() {
                cancelling.cancel(&cancelling_binding,&cancelling_caller,&cancel_admission).ok()==Some(HostControlReply::Cancelled)
            }else{false};
            let _=release_tx.send(());result
        });
        let published=concurrent.test_request(&document_binding,&other_fixture.caller,&other_admission,|description,_|{
            let _=entered_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).map_err(|_|crate::cli::profiles::error("TEST_TIMEOUT"))?;
            Ok(Arc::new(crate::remote_host_approval::NativeHostApproval::test_record_description(description,&other_admission)))
        });
        let thread_cancelled=cancel_thread.join().map_err(|_| "show cancellation panic")?;
        if !thread_cancelled || published.ok()!=Some(HostControlReply::Cancelled) {return Err("cross-thread show published after cancel");}
        concurrent.begin_shutdown();
        controller.begin_shutdown();delayed.begin_shutdown();
        // 真实 take_activation 的 clock 尚未返回时，没有准备线程也不能绕过 service 退出。
        let clock_controller=Arc::new(RemoteHostController::default());
        let clock_host=Arc::new(RemoteHostService::default());
        let (clock_entered,clock_observed)=std::sync::mpsc::sync_channel(1);
        let (clock_release,clock_held)=std::sync::mpsc::sync_channel(1);
        let (mut clock_description,clock_material)=make_description();
        let source_clock=clock_description.clock.clone();
        let clock_held=parking_lot::Mutex::new(clock_held);
        let weak_clock_host=Arc::downgrade(&clock_host);
        let reentry_rejected=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reentry_result=reentry_rejected.clone();
        clock_description.clock=Arc::new(move || {
            if let Some(host)=weak_clock_host.upgrade() {
                reentry_result.store(host.reserve_activation().is_err(),std::sync::atomic::Ordering::Release);
            }
            let _=clock_entered.send(());
            clock_held.lock().recv_timeout(Duration::from_secs(5)).map_err(|_|cc_desk_remote_security::GrantError::Clock)?;
            source_clock.now_ms()
        });
        let factory_calls=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls=factory_calls.clone();
        clock_controller.test_install_preparation(&document_binding,other_fixture.caller.clone(),&other_admission,clock_description,Instant::now()+Duration::from_secs(60),Box::new(move ||{calls.fetch_add(1,std::sync::atomic::Ordering::AcqRel);clock_material})).map_err(|_|"clock configuration")?;
        clock_controller.test_request(&document_binding,&other_fixture.caller,&other_admission,|description,_|Ok(Arc::new(crate::remote_host_approval::NativeHostApproval::test_record_description(description,&other_admission)))).map_err(|_|"clock approval")?;
        let foreign=fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller,&status.run).map_err(|_|"foreign prepare access")?,&status.run).map_err(|_|"foreign prepare admission")?;
        let worker_runtime=Arc::new(NativeRuntime::with_owned_components_for_test(other_fixture.service.clone(),other_fixture.transports.clone(),other_fixture.supervisor.clone()));
        if clock_controller.test_prepare_owned(&document_binding,&other_fixture.caller,foreign,worker_runtime.clone(),clock_host.clone()).err().map(|e|e.code)!=Some("FORBIDDEN".into()) || clock_host.test_activation_observation().is_some() {return Err("foreign prepare touched operation");}
        let clock_admission=other_fixture.supervisor.admit_remote_run(other_fixture.service.access(&other_fixture.caller,&other_status.run).map_err(|_|"clock access")?,&other_status.run).map_err(|_|"clock admission")?;
        let worker_controller=clock_controller.clone();let worker_binding=document_binding.clone();let worker_caller=other_fixture.caller.clone();let worker_host=clock_host.clone();
        struct ActivationCaller(Option<std::thread::JoinHandle<Result<(),crate::cli::types::SafeError>>>);
        impl Drop for ActivationCaller {fn drop(&mut self){if let Some(worker)=self.0.take(){let _=worker.join();}}}
        let mut caller_worker=ActivationCaller(Some(std::thread::spawn(move ||worker_controller.test_prepare_owned(&worker_binding,&worker_caller,clock_admission,worker_runtime,worker_host))));
        clock_observed.recv_timeout(Duration::from_secs(5)).map_err(|_|"clock did not enter")?;
        if !reentry_rejected.load(std::sync::atomic::Ordering::Acquire) || !clock_host.test_activation_observation().is_some_and(|o|o.outstanding) {return Err("clock reservation missing or reentered");}
        if clock_host.request_exit(19,false)!=crate::remote_host::ExitDecision::Wait(19) {return Err("clock exit intent");}
        // 本子场景没有网络或 pin，只隔离检验 activation gate，不声称运行树已退休。
        let exit_directory=tempfile::tempdir().map_err(|_|"empty exit directory")?;
        let exit_launch=Arc::new(crate::cli::launch_service::LaunchService::new(crate::cli::storage::WorkspaceRepository::open(exit_directory.path().join("workspace.json")).map_err(|_|"empty exit workspace")?,Some(Default::default()),None));
        let (exit_tx,mut exit_rx)=tokio::sync::oneshot::channel();
        clock_host.start_exit(19,Arc::new(NativeRuntime::new(exit_launch)),Arc::new(crate::remote_client_ipc::RemoteClientService::default()),move|code|{let _=exit_tx.send(code);});
        tokio::time::timeout(Duration::from_secs(5),async{while !clock_host.test_activation_exit_observed(){tokio::task::yield_now().await;}}).await.map_err(|_|"activation waiter not observed")?;
        if !matches!(exit_rx.try_recv(),Err(tokio::sync::oneshot::error::TryRecvError::Empty)) {return Err("exit overtook clock");}
        let _=clock_release.send(());
        caller_worker.0.take().unwrap().join().map_err(|_|"clock caller panic")?.map_err(|_|"clock caller failed")?;
        if tokio::time::timeout(Duration::from_secs(5),exit_rx).await.map_err(|_|"clock exit pending")?.map_err(|_|"clock exit lost")?!=19 || factory_calls.load(std::sync::atomic::Ordering::Acquire)!=0 || clock_host.test_network_attempts()!=0 || !clock_host.test_activation_observation().is_some_and(|o|o.retired()) {return Err("cancelled clock operation retirement");}
        clock_controller.begin_shutdown();
        // 原 admission 仍在手，但原 document 已撤销，不能注册自身批准。
        let other_scope = other_admission.scope().map_err(|_| "independent scope")?;
        let (stale_owner, stale_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(network_inputs(&relay, other_scope, source.clone()), &other_admission, true);
        other_fixture.service.registry().revoke_window(&other_fixture.caller).map_err(|_| "revoke independent document")?;
        if stale_owner.register_once(&other_runtime, &other_admission, &other_host, stale_material).is_ok() || other_host.test_network_attempts() != 0 { return Err("revoked document reused retained admission"); }
        drop(other_admission);
        if !other_cleanup.close() || !other_host.test_join_network().await { return Err("independent run did not retire"); }
        // 重复固定交错：外部 clock 返回前撤销，或注册完成但 token 尚未发布时撤销。
        for after_register in [false, true] {
            for _ in 0..16 {
                use cc_desk_remote_transport::wss::{RelayAdmissionIdentity, RelayRole};
                let signer = Arc::new(NetworkRelaySigner {origin:relay.origin.clone(),id:[1;16],role:RelayRole::Host});
                let signer_lifetime = Arc::downgrade(&signer);
                let mut inputs = network_inputs(&relay, scope.clone(), source.clone());
                inputs.relay = RelayAdmissionIdentity::new([1;16],RelayRole::Host,signer);
                let (race, material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(inputs, &original, true);
                let race = Arc::new(race);
                let weak = Arc::downgrade(&race);
                if after_register {
                    race.test_before_publish(Box::new(move || weak.upgrade().unwrap().cancel()));
                } else {
                    race.test_clock(Arc::new(move || { weak.upgrade().unwrap().cancel(); Ok(1000) }));
                }
                let race_host = Arc::new(RemoteHostService::default());
                if race.register_once(&runtime, &original, &race_host, material).is_ok() { return Err("interleaved cancel registered"); }
                if signer_lifetime.strong_count() != 0 || race_host.test_network_attempts() != 0 { return Err("cancel retained material or opened network"); }
                race.cancel();
                let (_, retry_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(network_inputs(&relay, scope.clone(), source.clone()), &original, true);
                if race.register_once(&runtime, &original, &race_host, retry_material).is_ok() { return Err("cancelled approval revived"); }
                if !race_host.test_join_network().await { return Err("cancel owner did not retire"); }
            }
        }
        eprintln!("TEST_ONLY_APPROVAL_V2 same_run_key_independent_resource_rejected=true stale_document_admission_rejected=true cancel_before_and_after_register=32 material_released=true network_attempts=0 single_use_sticky=true");
        // 真正跨线程取消：登记线程在边界等待，另一条保管的线程取消后才释放边界。
        for after_register in [false, true] {
            for _ in 0..8 {
                use cc_desk_remote_transport::wss::{RelayAdmissionIdentity, RelayRole};
                let signer = Arc::new(NetworkRelaySigner { origin: relay.origin.clone(), id: [1;16], role: RelayRole::Host });
                let signer_lifetime = Arc::downgrade(&signer);
                let mut inputs = network_inputs(&relay, scope.clone(), source.clone());
                inputs.relay = RelayAdmissionIdentity::new([1;16], RelayRole::Host, signer);
                let (race, material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(inputs, &original, true);
                let race = Arc::new(race);
                let (entered_tx, entered_rx) = std::sync::mpsc::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let cancel_owner = race.clone();
                let cancel_thread = std::thread::spawn(move || {
                    let arrived = entered_rx.recv_timeout(Duration::from_secs(5)).is_ok();
                    if arrived { cancel_owner.cancel(); }
                    let _ = release_tx.send(());
                    arrived
                });
                if after_register {
                    race.test_before_publish(Box::new(move || {
                        let _ = entered_tx.send(());
                        assert!(release_rx.recv_timeout(Duration::from_secs(5)).is_ok(), "cancel thread release");
                    }));
                } else {
                    let release_rx = parking_lot::Mutex::new(release_rx);
                    race.test_clock(Arc::new(move || {
                        let _ = entered_tx.send(());
                        release_rx.lock().recv_timeout(Duration::from_secs(5)).map_err(|_| cc_desk_remote_security::GrantError::Clock)?;
                        Ok(1000)
                    }));
                }
                let race_host = Arc::new(RemoteHostService::default());
                let rejected = race.register_once(&runtime, &original, &race_host, material).is_err();
                // 边界等待最多五秒；取消线程始终归还原 JoinHandle，不以标志冒充 join。
                let cancelled = cancel_thread.join().map_err(|_| "cancel thread panic")?;
                if !rejected || !cancelled || signer_lifetime.strong_count()!=0 || race_host.test_network_attempts()!=0 {
                    return Err("cross-thread cancellation retained material or registered");
                }
                let (_, retry_material) = crate::remote_host_approval::NativeHostApproval::test_from_inputs(network_inputs(&relay, scope.clone(), source.clone()), &original, true);
                if race.register_once(&runtime, &original, &race_host, retry_material).is_ok() { return Err("cross-thread cancellation revived"); }
                if !race_host.test_join_network().await { return Err("cross-thread original owner did not retire"); }
            }
        }
        eprintln!("TEST_ONLY_APPROVAL_THREADS boundary_cases=16 original_threads_joined=true material_released=true attempts=0");
        let pending = Arc::new(RemoteHostService::default());
        let mut wrong_scope = network_inputs(&relay, scope.clone(), source.clone());
        wrong_scope.policy.scope.generation.push('0');
        if !matches!(pending.register_native_source(&runtime, &original, wrong_scope), Err(rejected) if rejected.0 == HostNetworkError::Authority) { return Err("wrong scope registration accepted"); }
        let token = pending.register_native_source(&runtime, &original, network_inputs(&relay, scope.clone(), source.clone())).map_err(|_| "boundary install")?;
        let mut changed = network_inputs(&relay, scope.clone(), Arc::new(ApprovedHostSource { clock: Arc::new(|| Ok(1000)), revoked: source.revoked.clone(), expires_host_ms: 11000 }));
        changed.binding.activation = [0x44; 32];
        if !matches!(pending.register_native_source(&runtime, &original, changed), Err(rejected) if rejected.0 == HostNetworkError::Locked) { return Err("activation/source replacement accepted"); }
        let material = || HostAttemptMaterials { stages: None, local: network_local(false), relay: network_relay_identity(&relay.origin, true), outer_accepted: None, termination: None };
        if !matches!(pending.replenish_native_source(&runtime, &original, &token, material()), Err(rejected) if rejected.0 == HostNetworkError::Locked) { return Err("occupied material overwritten"); }
        let other = Arc::new(RemoteHostService::default());
        let foreign = other.register_native_source(&runtime, &original, network_inputs(&relay, scope.clone(), source.clone())).map_err(|_| "foreign install")?;
        if !matches!(pending.replenish_native_source(&runtime, &original, &foreign, material()), Err(rejected) if rejected.0 == HostNetworkError::Locked) { return Err("foreign token accepted"); }
        pending.revoke_native_source(&token).map_err(|_| "pending revoke")?;
        other.revoke_native_source(&foreign).map_err(|_| "foreign cleanup")?;
        if pending.test_source_material_pending() || other.test_source_material_pending() { return Err("external revoke retained unused material"); }
        if pending.replenish_native_source(&runtime, &original, &token, material()).is_ok() { return Err("revoked registration refilled"); }
        let token_owner = Arc::new(RemoteHostService::default());
        let holder = Arc::new(parking_lot::Mutex::new(None::<HostSourceToken>));
        let captured = holder.clone();
        let noncyclic = Arc::new(ApprovedHostSource { clock: Arc::new(move || { let _held = captured.lock(); Ok(1000) }), revoked: source.revoked.clone(), expires_host_ms: 11000 });
        let weak_source = Arc::downgrade(&noncyclic);
        let token = token_owner.register_native_source(&runtime, &original, network_inputs(&relay, scope.clone(), noncyclic)).map_err(|_| "weak token install")?;
        *holder.lock() = Some(token);
        drop(token_owner);
        if weak_source.upgrade().is_some() { return Err("opaque token retained its source through a provider cycle"); }
        for mode in 0..4 {
            let rejected = Arc::new(RemoteHostService::default());
            let weak = Arc::downgrade(&rejected);
            let callback_token = Arc::new(parking_lot::Mutex::new(None::<HostSourceToken>));
            let held_token = callback_token.clone();
            let (_lease, revoked) = tokio::sync::watch::channel(mode == 1);
            let source = Arc::new(ApprovedHostSource { clock: Arc::new(move || {
                if mode == 2 { if let (Some(owner), Some(token)) = (weak.upgrade(), held_token.lock().take()) { owner.revoke_native_source(&token).unwrap(); } }
                if mode == 3 { if let Some(owner) = weak.upgrade() { owner.request_exit(98, false); } }
                Ok(1000)
            }), revoked, expires_host_ms: if mode == 0 { 1000 } else { 11000 } });
            let token = rejected.register_native_source(&runtime, &original, network_inputs(&relay, scope.clone(), source)).map_err(|_| "prestart install")?;
            *callback_token.lock() = Some(token.clone());
            let result = rejected.test_begin_registered_network(runtime.clone(), admit()?);
            callback_token.lock().take();
            if !matches!(result, Err(HostNetworkError::Authority | HostNetworkError::Closed)) || rejected.test_network_attempts() != 0 || rejected.test_source_material_pending() { return Err("invalid source reached original task/socket"); }
        }
        let preflight = Arc::new(RemoteHostService::default());
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let released = parking_lot::Mutex::new(released);
        let preflight_source = Arc::new(ApprovedHostSource { clock: Arc::new(move || {
            entered_tx.send(()).map_err(|_| cc_desk_remote_security::GrantError::Clock)?;
            released.lock().recv_timeout(Duration::from_secs(5)).map_err(|_| cc_desk_remote_security::GrantError::Clock)?;
            Err(cc_desk_remote_security::GrantError::Revoked)
        }), revoked: source.revoked.clone(), expires_host_ms: 11000 });
        let token = preflight.register_native_source(&runtime, &original, network_inputs(&relay, scope.clone(), preflight_source)).map_err(|_| "preflight install")?;
        let background_admission = admit()?;
        let contender_admission = admit()?;
        let background_owner = preflight.clone();
        let background_runtime = runtime.clone();
        let held = std::thread::spawn(move || background_owner.test_begin_registered_network(background_runtime, background_admission));
        let entered = entered.recv_timeout(Duration::from_secs(2)).is_ok();
        let pending = preflight.source_material_returned().is_none();
        let (refill_rejected, contender_rejected) = if entered {
            let refill = matches!(preflight.replenish_native_source(&runtime, &original, &token, material()), Err(rejected) if rejected.0 == HostNetworkError::Locked);
            let contender = matches!(preflight.test_begin_registered_network(runtime.clone(), contender_admission), Err(HostNetworkError::Closed));
            (refill, contender)
        } else { (false, false) };
        let _ = release.send(());
        let ended = held.join().map_err(|_| "preflight provider thread")?;
        let returned = preflight.source_material_returned() == Some(Ok(()));
        preflight.revoke_native_source(&token).map_err(|_| "preflight cleanup")?;
        if !entered || !pending || !refill_rejected || !contender_rejected || !returned || !matches!(ended, Err(HostNetworkError::Authority)) || preflight.test_network_attempts() != 0 { return Err("preflight was unbounded or lost material custody"); }
        eprintln!("TEST_ONLY_REGISTERED_PREFLIGHT provider_held=Pending refill_rejected=true contender_rejected=true actual_return=Known original_tasks=0");
        eprintln!("TEST_ONLY_REGISTERED_PRESTART absent_scope_replacement_occupied_foreign_revoked_expired_callback_close=true original_tasks=0 unused_material_released=true token_provider_cycle=false");
        let (attempt, connected, token) = open_registered_network_peer(&fixture, &status.run, &host, &runtime, &relay, (source.clone(), None), None).await?;
        peer = Some(connected);
        let before = host.test_network_attempts();
        let missing = fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller, &status.run).map_err(|_| "missing material access")?, &status.run).map_err(|_| "missing material admission")?;
        if !matches!(host.test_begin_registered_network(runtime.clone(), missing), Err(HostNetworkError::Locked)) || host.test_network_attempts() != before { return Err("consumed material was reused"); }
        let snapshot = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "registered snapshot")?;
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: snapshot.stamp.clone() }, 1).map_err(|_| "registered ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "registered ACK")?;
        let window = peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, Duration::from_secs(1)).await.map_err(|_| "registered Window")?;
        let wire::Message::WindowGranted { window, not_after_host_ms, .. } = wire::decode(&window).map_err(|_| "registered window codec")? else { return Err("registered window grant"); };
        let bytes = input::encode(&input::Input { operation: [0x31; 16], sequence: 1, window: network_hex(&window).try_into().map_err(|_| "registered token")?, not_after_host_ms: not_after_host_ms.parse().map_err(|_| "registered deadline")?, bytes: b"nat".to_vec() }).map_err(|_| "registered input codec")?;
        let receipt = peer.as_ref().unwrap().0.exchange(&bytes, Duration::from_secs(1)).await.map_err(|_| "registered native input")?;
        if !matches!(wire::decode(&receipt), Ok(wire::Message::InputReceipt { outcome: wire::Outcome::HostWritten, proof: Some(_), .. })) { return Err("registered HostWritten missing"); }
        if !retire_network_peer(peer.take().unwrap(), &attempt).await { return Err("registered first join"); }
        let (attempt2, connected2, _) = open_registered_network_peer(&fixture, &status.run, &host, &runtime, &relay, (source.clone(), None), Some(&token)).await?;
        peer = Some(connected2);
        let rebound = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "registered rebound snapshot")?;
        if rebound.stamp.target != snapshot.stamp.target || rebound.stamp.binding == snapshot.stamp.binding || host.test_pin_count() != 1 { return Err("registered rebind replaced original resource"); }
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: rebound.stamp }, 1).map_err(|_| "registered fresh ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "registered fresh ACK")?;
        peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, Duration::from_secs(1)).await.map_err(|_| "registered fresh Window")?;
        let unused = admit()?;
        host.replenish_native_source(&runtime, &unused, &token, material()).map_err(|_| "registered unused fresh material")?;
        if !host.test_source_material_pending() { return Err("registered fresh custody missing"); }
        host.revoke_native_source(&token).map_err(|_| "registered revoke")?;
        if host.test_source_material_pending() { return Err("external revoke retained fresh material"); }
        let _ = attempt2.wait_closed().await;
        let local = fixture.service.access(&fixture.caller, &status.run).map_err(|_| "registered local access")?;
        if local.write_frame(b"ive").map_err(|_| "registered local input")?.confirmed_bytes != 3 { return Err("registered local continuation"); }
        drop(local);
        fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
        let reports: Vec<Value> = fs::read_dir(fixture.root.path().join("work")).map_err(|_| "registered reports")?.filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).map(|entry| serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap()).collect();
        if !reports.iter().any(|report| report["input"] == json!(b"native".to_vec())) { return Err("registered exact Node input"); }
        eprintln!("TEST_ONLY_REGISTERED_SOURCE same_source=true original_pin=1 fresh_consumed=true original_ACK_Window_HostWritten=true exact_Node_bytes=6");
        Ok(())
    }.await;
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
    }
    let physical = cleanup.close();
    let _ = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(physical && relay_closed);
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_RegisteredProviderRevokeNaturallyJoinsWithoutInlineWake_009() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::projection;
    use futures_util::task::{waker, ArcWake};
    use std::{
        future::Future,
        sync::{
            atomic::{AtomicBool, AtomicUsize},
            OnceLock,
        },
        task::{Context, Poll},
    };
    struct Probe {
        inside: Arc<AtomicBool>,
        inline: AtomicUsize,
    }
    impl ArcWake for Probe {
        fn wake_by_ref(this: &Arc<Self>) {
            if this.inside.load(Ordering::Acquire) {
                this.inline.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let (_lease, revoked) = tokio::sync::watch::channel(false);
    let callback_token = Arc::new(parking_lot::Mutex::new(None::<HostSourceToken>));
    let held_token = callback_token.clone();
    let weak = Arc::downgrade(&host);
    let inside = Arc::new(AtomicBool::new(false));
    let in_provider = inside.clone();
    let sampled = Arc::new(AtomicBool::new(false));
    let observed = sampled.clone();
    let fault_at = Arc::new(OnceLock::new());
    let recorded = fault_at.clone();
    let deferred = Arc::new(AtomicBool::new(false));
    let held_during_callback = deferred.clone();
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(move || {
            let token = held_token.lock().take();
            if let Some(token) = token {
                let _ = recorded.set(tokio::time::Instant::now());
                in_provider.store(true, Ordering::Release);
                let owner = weak.upgrade().unwrap();
                owner.revoke_native_source(&token).unwrap();
                held_during_callback.store(owner.test_source_material_pending(), Ordering::Release);
                in_provider.store(false, Ordering::Release);
                observed.store(true, Ordering::Release);
            }
            Ok(1000)
        }),
        revoked,
        expires_host_ms: 11000,
    });
    let (selected, ending) = tokio::sync::oneshot::channel();
    let mut peer = None;
    let outcome: Result<(), &'static str> = async {
        let (attempt, connected, token) = open_registered_network_peer(&fixture, &status.run, &host, &runtime, &relay, (source, Some(selected)), None).await?;
        peer = Some(connected);
        let snapshot = peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#, Duration::from_secs(1)).await.map_err(|_| "registered revoke snapshot")?;
        let ack = projection::encode_ack(&projection::Ack { profile: 1, stamp: snapshot.stamp }, 1).map_err(|_| "registered revoke ACK codec")?;
        peer.as_ref().unwrap().0.exchange(&ack, Duration::from_secs(1)).await.map_err(|_| "registered revoke ACK")?;
        let fresh = fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller, &status.run).map_err(|_| "registered pending access")?, &status.run).map_err(|_| "registered pending admission")?;
        host.replenish_native_source(&runtime, &fresh, &token, HostAttemptMaterials { stages: None, local: network_local(false), relay: network_relay_identity(&relay.origin, true), outer_accepted: None, termination: None }).map_err(|_| "registered pending capability")?;
        drop(fresh);
        let mut stop = host.test_network_stop_receiver().ok_or("registered original stop missing")?;
        let probe = Arc::new(Probe { inside: inside.clone(), inline: AtomicUsize::new(0) });
        let notification = stop.changed();
        tokio::pin!(notification);
        let wake = waker(probe.clone());
        if !matches!(notification.as_mut().poll(&mut Context::from_waker(&wake)), Poll::Pending) { return Err("registered stop already closed"); }
        *callback_token.lock() = Some(token);
        let window = peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, Duration::from_secs(1)).await;
        if window.is_ok() || !sampled.load(Ordering::Acquire) || probe.inline.load(Ordering::Acquire) != 0 || !deferred.load(Ordering::Acquire) { return Err("provider revoke allowed an effect, inline wake or inline material drop"); }
        let joined = tokio::time::timeout(Duration::from_secs(4), attempt.wait_closed()).await.map_err(|_| "registered revoke natural join timed out")?;
        if joined != Ok(()) { return Err("registered explicit revocation lost its known terminal result"); }
        let end = ending.await.map_err(|_| "registered revoke terminal missing")?;
        let fault = *fault_at.get().ok_or("registered revoke time missing")?;
        let lease = end.lease_end.ok_or("registered revoke lease missing")?;
        if end.cause != TestAttemptCause::Channel || end.selected_at < fault || end.selected_at >= lease { return Err("registered revoke ended through an external stop or lease"); }
        if host.network_admission_open() { return Err("revoked registration remained eligible"); }
        if host.test_source_material_pending() { return Err("registered task did not drain unused capability"); }
        eprintln!("TEST_ONLY_REGISTERED_CALLBACK_REVOKE inline_wake=0 deferred_material_drained=true original_join=Known cause={:?} fault_to_select_ms={} lease_remaining_ms={} no_fresh_admission_after_fault=true no_external_stop_before_join=true", end.cause, end.selected_at.duration_since(fault).as_millis(), lease.duration_since(fault).as_millis());
        let local = fixture.service.access(&fixture.caller, &status.run).map_err(|_| "registered revoke local access")?;
        let written = local.write_frame(b"native").map_err(|_| "registered revoke local input")?;
        if written.confirmed_bytes != 6 || written.state != crate::terminal_input::InputWriteState::HostWritten { return Err("registered revoke local input changed"); }
        drop(local);
        fixture.wait_lifecycle(|state| state.process() == ProcessLifecycle::Exited);
        Ok(())
    }.await;
    callback_token.lock().take();
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
    }
    let physical = cleanup.close();
    let _ = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(physical && relay_closed);
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
async fn owned_session_exit_joins_original_owners() {
    use crate::cli::{document::DocumentAuthority, run_registry::RunRegistry};
    use crate::platform::owned_job_launch::JobLaunchObservation;
    use crate::remote_client_ipc::RemoteClientService;
    use crate::remote_host::ExitDecision;
    use cc_desk_remote_client::NativeClient;
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _cleanup = RuntimeStop(runtime.clone()); // Original slot cleanup precedes launch.
    let events = fixture.events.clone();
    let status = runtime
        .start_managed_run(&fixture.caller, &fixture.request, |_| {
            fixture.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    assert_eq!(
        runtime.owned_job_slot_for_test().observe(),
        JobLaunchObservation::Empty,
        "original Job/reader must transfer to supervisor before response"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
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
    let (c, h, _grant, _cancel) = channels::channels(admission.scope().unwrap()).await;
    let c_authority = c.authority();
    let h = Arc::new(h);
    let host = Arc::new(RemoteHostService::default());
    host.adopt_admitted(admission, h.clone(), &runtime)
        .map_err(|rejected| {
            let (error, original) = *rejected;
            eprintln!(
                "TEST_ONLY_ADOPT_FAILURE code={} scope_error={:?} authority={:?} output={:?}",
                error.code,
                original.scope().err().map(|failure| failure.code),
                h.authority().check(),
                fixture
                    .supervisor
                    .snapshot(&status.run)
                    .map(|record| record.output())
            );
            error.code
        })
        .unwrap();
    let proof = c.client_installed().await.unwrap();
    let client = NativeClient::from_installed(c, proof).unwrap();
    let snapshot = client
        .snapshot(
            br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(snapshot.stamp.target.run, status.run.run_id);
    assert_eq!(
        snapshot.stamp.target.generation,
        status.run.generation.to_string()
    );
    // Explicit TEST_ONLY document fixture uses the existing original publication
    // path; it does not bypass the production signer/profile factory.
    let authority = DocumentAuthority::new(
        Arc::new(RunRegistry::new(2)),
        "http://tauri.localhost/index.html".parse().unwrap(),
    )
    .unwrap();
    let caller = authority.test_caller();
    let mut table = tauri::ResourceTable::default();
    let binding = Arc::new(authority.attach(&mut table).unwrap());
    let url = "http://tauri.localhost/index.html".parse().unwrap();
    authority.started(&url);
    authority.finished(&url);
    let client_service = Arc::new(RemoteClientService::default());
    client_service
        .test_only_install_original_client(caller, &binding, client)
        .unwrap();
    // This real late access pins the original PTY after root/worker settlement.
    let late_access = fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap();
    assert_eq!(host.request_exit(83, false), ExitDecision::Wait(83));
    let (tx, mut exited) = tokio::sync::oneshot::channel();
    host.start_exit(83, runtime.clone(), client_service.clone(), move |code| {
        let _ = tx.send(code);
    });
    assert_eq!(host.request_exit(84, false), ExitDecision::Prevent);
    let deadline = Instant::now() + Duration::from_secs(10);
    let held = loop {
        let facts = host.test_original_managed_facts(&status.run);
        if facts.as_ref().is_some_and(|facts| {
            facts.direct_child_reaped && facts.wait_workers_joined && facts.physical_writer_joined
        }) {
            break facts.unwrap();
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let prevented = exited.try_recv().is_err();
    drop(late_access);
    let code = tokio::time::timeout(Duration::from_secs(10), exited)
        .await
        .unwrap()
        .unwrap();
    assert!(host.test_join_exit_worker().await);
    let facts = host.test_original_managed_facts(&status.run).unwrap();
    assert!(
        prevented && !held.pinned_conpty_close_returned && !held.stop_job_joined,
        "late access was not retained: {held:?}"
    );
    assert_eq!(held.job_active_processes, Some(0));
    assert!(!held.job_query_is_historical && !held.original_job_handle_closed);
    assert_eq!(facts.job_active_processes, Some(0));
    assert!(
        facts.original_resources_retired()
            && facts.original_job_handle_closed
            && facts.job_query_is_historical
    );
    assert!(
        facts.pinned_conpty_close_returned
            && facts.original_reader_served_close
            && facts.stream_capacity_retired
    );
    assert!(!fixture.supervisor.active_for_test(&status.run));
    assert_eq!(fixture.transports.budgeted_bytes(), 0);
    assert!(fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .is_err());
    let record = fixture.supervisor.snapshot(&status.run).unwrap();
    assert_eq!(record.output(), OutputLifecycle::Incomplete);
    assert!(
        record.final_offset().is_none(),
        "ambiguous raw zero is not EOF/parsed ACK"
    );
    assert!(h.authority().check().is_err() && c_authority.check().is_err());
    assert!(h.wait_closed_checked().await.is_ok());
    assert!(client_service.shutdown().await.is_ok());
    assert_eq!(code, 83);
    assert_eq!(host.request_exit(83, false), ExitDecision::Proceed);
    assert_eq!(host.request_exit(84, false), ExitDecision::Prevent);
    let mut next = fixture.request.clone();
    next.request_id = "TEST_ONLY_CLOSED_EXIT_REQUEST".into();
    next.tab_id = "TEST_ONLY_CLOSED_EXIT_TAB".into();
    next.run_id = "TEST_ONLY_CLOSED_EXIT_RUN".into();
    assert!(runtime
        .start_managed_run(&fixture.caller, &next, |_| panic!(
            "closed admission reused retired capacity"
        ))
        .is_err());
    host.start_exit(83, runtime, client_service, |_| {
        panic!("duplicate exit callback")
    });
    eprintln!("TEST_ONLY_OWNED_SESSION_RELEASED held={held:?} final={facts:?} first_exit_code={code} host_client_network_joined=true capacity=0 incomplete=true");
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ExitDuringHandoffRetainsAndReleasesCoveredLatePin_005() {
    use crate::remote_client_ipc::RemoteClientService;
    use crate::remote_host::ExitDecision;
    struct Stop(Arc<NativeRuntime>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _stop = Stop(runtime.clone());
    let events = fixture.events.clone();
    let status = runtime
        .start_managed_run(&fixture.caller, &fixture.request, |_| {
            fixture.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let ready = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < ready);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let access = fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap();
    let admission = fixture
        .supervisor
        .admit_remote_run(access.clone(), &status.run)
        .unwrap();
    let (client, channel, gate, cancel) = channels::channels(admission.scope().unwrap()).await;
    let channel = Arc::new(channel);
    let service = Arc::new(RemoteHostService::default());
    let closing = service.clone();
    let original_runtime = runtime.clone();
    let client_service = Arc::new(RemoteClientService::default());
    let (exit, mut exited) = tokio::sync::oneshot::channel();
    let (retirement_entered, release_retirement) = service.test_hold_retirement();
    struct ReleaseRetirement(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for ReleaseRetirement {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }
    let mut release_retirement = ReleaseRetirement(Some(release_retirement));
    let observed_gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let record_gate = observed_gate.clone();
    service.test_after_handoff(Box::new(move || {
        assert_eq!(closing.request_exit(91, false), ExitDecision::Wait(91));
        closing.start_exit(91, original_runtime, client_service, move |code| {
            let _ = exit.send(code);
        });
        // 原 stop inventory 已移出后才返回 hook，随后才登记真正 late pin。
        record_gate.store(
            retirement_entered
                .recv_timeout(Duration::from_secs(5))
                .is_ok(),
            std::sync::atomic::Ordering::Release,
        );
    }));
    let committed = service
        .adopt_admitted(admission, channel.clone(), &runtime)
        .is_ok();
    let retirement_held = observed_gate.load(std::sync::atomic::Ordering::Acquire);
    let exit_unpublished = matches!(
        exited.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    );
    let observed_pin_count = service.test_pin_count();
    let late_pin_retained = observed_pin_count == 1;
    eprintln!("TEST_ONLY_HANDOFF_005_SAMPLE committed={committed} observed_pin_count={observed_pin_count} late_pin_retained={late_pin_retained}");
    if let Some(release) = release_retirement.0.take() {
        let _ = release.send(());
    }
    drop(access);
    let code = tokio::time::timeout(Duration::from_secs(10), exited)
        .await
        .unwrap()
        .unwrap();
    let joined = service.test_join_exit_worker().await;
    cancel.cancel();
    client.authority().request_close();
    channel.authority().request_close();
    let _ = tokio::join!(client.wait_closed(), channel.wait_closed());
    drop(gate);
    let facts = service.test_original_managed_facts(&status.run).unwrap();
    let released = service.test_pin_count() == 0;
    runtime.shutdown();
    eprintln!("TEST_ONLY_HANDOFF_005_FINAL committed={committed} late_pin_retained={late_pin_retained} joined={joined} code={code} released={released} facts={facts:?}");
    assert!(
        retirement_held && exit_unpublished,
        "original retirement gate must hold before observing late pin"
    );
    assert!(committed && late_pin_retained && joined && code == 91);
    assert!(
        facts.original_resources_retired()
            && facts.job_query_is_historical
            && facts.original_job_handle_closed
    );
    assert!(
        released,
        "covered late pin must be released after original inventory retirement"
    );
}
struct Cleanup {
    supervisor: Arc<NativeRunSupervisor>,
    host: Arc<RemoteHostService>,
    run: RunKey,
    pin: Option<NativeRunPin>,
    stop: Option<NativeShutdown>,
    proofs: Vec<cc_desk_remote_security::BoundDataAuthority>,
}
impl Cleanup {
    fn close(&mut self) -> bool {
        self.host.test_begin_host_shutdown();
        for proof in &self.proofs {
            proof.request_close();
        }
        if self.stop.is_none() {
            let pin = self
                .pin
                .take()
                .unwrap_or_else(|| self.supervisor.pin_shutdown(&self.run));
            self.stop = self.supervisor.begin_shutdown_owned(vec![pin]).ok();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let writer = self.host.test_writer_retirement();
            let native = self.stop.as_mut().map(|s| s.observe_run(&self.run));
            if writer == WriterRetirement::Joined
                && matches!(
                    native,
                    Some(NativeShutdownObservation::OwnedSessionReleased)
                        | Some(NativeShutdownObservation::Unknown {
                            direct_child_reaped: true,
                            output_reader_joined: true,
                            wait_workers_joined: true,
                            original_io_drop_returned: true,
                            stop_job_joined: true,
                            ..
                        })
                )
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ActualAdmissionInstallsOriginalOwnNode_001() {
    use cc_desk_remote_client::NativeClient;
    use cc_desk_remote_record::{control_io as wire, input, projection};
    const BUDGET: Duration = Duration::from_secs(1);
    let check = |ok, error| if ok { Ok(()) } else { Err(error) };
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    );
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: fixture.request_run(),
        pin: None,
        stop: None,
        proofs: Vec::new(),
    };
    let status = fixture.start();
    cleanup.pin = Some(fixture.supervisor.pin_shutdown(&status.run));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(
            Instant::now() < deadline,
            "fresh own Node did not initialize"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut original_client = None;
    let mut client = None;
    let mut original_host = None;
    let mut grant = None;
    let mut cancel = None;
    let outcome:Result<(),&'static str>=async{
        let access=fixture.service.access(&fixture.caller,&status.run).map_err(|_|"original access")?;
        let admission=fixture.supervisor.admit_remote_run(access.clone(),&status.run).map_err(|_|"original admission")?;
        let scope=admission.scope().map_err(|_|"original scope")?;
        let mut wrong_scope=scope.clone();wrong_scope.session="a5".repeat(16);
        let(wc,wh,_wrong_gate,_wrong_cancel)=channels::channels(wrong_scope).await;
        cleanup.proofs.extend([wc.authority(),wh.authority()]);
        let wh=Arc::new(wh);
        let wrong=host.adopt_admitted(admission,wh.clone(),&runtime);
        wc.authority().request_close();wh.authority().request_close();
        let _=tokio::join!(wc.wait_closed(),wh.wait_closed());
        let admission=match wrong{
            Err(rejected)=>{let(_,original)=*rejected;original},
            Ok(())=>return Err("wrong scope consumed native admission"),
        };
        check(admission.scope().map_err(|_|"rejected original scope")?==scope,"wrong scope changed original custody")?;
        let clock:Arc<dyn cc_desk_remote_security::GrantClock>=Arc::new(||Ok(1000));
        let(c,h,gate,stop)=channels::channels_with_clock(scope.clone(),clock.clone()).await;
        cleanup.proofs.extend([c.authority(),h.authority()]);
        grant=Some(gate);cancel=Some(stop);original_client=Some(c);original_host=Some(Arc::new(h));
        host.adopt_admitted(admission,original_host.as_ref().unwrap().clone(),&runtime).map_err(|rejected|{
            let(_,original)=*rejected;drop(original);"verified native adoption"
        })?;
        let installed=original_client.as_ref().unwrap().client_installed().await.map_err(|_|"retained service install proof")?;
        client=Some(NativeClient::from_installed(original_client.take().unwrap(),installed).map_err(|_|"original installed client")?);
        let client=client.as_ref().unwrap();
        let snapshot=client.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|_|"fresh snapshot delivery")?;
        check(snapshot.stamp.target.run==status.run.run_id && snapshot.stamp.target.generation==status.run.generation.to_string(),"snapshot original target")?;
        let mut grid=String::new();
        for bytes in &snapshot.parts{
            let projection::Content::SnapshotPart(part)=projection::decode(bytes,1).map_err(|_|"snapshot codec")?.content else{return Err("snapshot part");};
            grid.extend(part.cells.iter().map(|cell|cell.scalar));
        }
        check(grid.contains("OWNED_READY"),"canonical prefix before admission missing")?;
        let ack=projection::encode_ack(&projection::Ack{profile:1,stamp:snapshot.stamp.clone()},1).map_err(|_|"ack codec")?;
        let acked=client.exchange(&ack,BUDGET).await.map_err(|_|"exact snapshot ACK")?;
        check(matches!(wire::decode(&acked),Ok(wire::Message::ProjectionAcknowledged{stamp,..}) if stamp==snapshot.stamp),"ACK echo")?;
        let window=client.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#,BUDGET).await.map_err(|_|"input window")?;
        let wire::Message::WindowGranted{window,not_after_host_ms,..}=wire::decode(&window).map_err(|_|"window codec")? else{return Err("window reply");};
        let mut token=[0;32];
        for(i,pair)in window.as_bytes().as_chunks::<2>().0.iter().enumerate(){token[i]=u8::from_str_radix(std::str::from_utf8(pair).map_err(|_|"window hex")?,16).map_err(|_|"window value")?;}
        let frame=input::encode(&input::Input{operation:[0x21;16],sequence:1,window:token,not_after_host_ms:not_after_host_ms.parse().map_err(|_|"window deadline")?,bytes:b"nat".to_vec()}).map_err(|_|"input codec")?;
        let receipt=client.exchange(&frame,BUDGET).await.map_err(|_|"original remote input")?;
        let wire::Message::InputReceipt{operation,sequence,outcome:wire::Outcome::HostWritten,proof:Some(proof),..}=wire::decode(&receipt).map_err(|_|"receipt codec")? else{return Err("exact written receipt");};
        check(operation=="21".repeat(16) && sequence=="1","receipt operation identity")?;
        for request in ["3","4"]{
            let query=wire::encode(&wire::Message::ReceiptQuery{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone()}).map_err(|_|"query codec")?;
            let found=client.exchange(&query,BUDGET).await.map_err(|_|"original receipt query")?;
            check(wire::decode(&found)==Ok(wire::Message::ReceiptStatus{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone(),status:wire::Status::Known,outcome:Some(wire::Outcome::HostWritten),proof:Some(proof.clone())}),"query original proof")?;
        }
        for(request,expected)in [("5",wire::Disposition::Advanced),("6",wire::Disposition::AlreadySettled)]{
            let settle=wire::encode(&wire::Message::ReceiptAck{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone(),outcome:wire::Outcome::HostWritten,proof:proof.clone()}).map_err(|_|"settlement codec")?;
            let settled=client.exchange(&settle,BUDGET).await.map_err(|_|"receipt settlement")?;
            check(matches!(wire::decode(&settled),Ok(wire::Message::ReceiptSettled{disposition,..}) if disposition==expected),"exact settlement/no replay")?;
        }
        let next=client.snapshot(br#"{"message":"Snapshot","profile":1,"request":"7"}"#,BUDGET).await.map_err(|_|"fresh second snapshot")?;
        check(next.stamp!=snapshot.stamp,"fresh delivery lineage")?;
        // A new complete snapshot was delivered but has NOT been acknowledged.
        // Replaying the old original input must not reach physical IO.
        check(client.exchange(&frame,BUDGET).await.is_err(),"input before new exact ACK was accepted")?;
        let access2=fixture.service.access(&fixture.caller,&status.run).map_err(|_|"fresh access")?;
        let admission2=fixture.supervisor.admit_remote_run(access2,&status.run).map_err(|_|"fresh original admission")?;
        let(c2,h2,_grant2,_cancel2)=channels::channels_with_clock(scope.clone(),clock).await;
        let h2=Arc::new(h2);cleanup.proofs.extend([c2.authority(),h2.authority()]);
        host.adopt_admitted(admission2,h2.clone(),&runtime).map_err(|_|"same original resource rebind")?;
        let proof2=c2.client_installed().await.map_err(|_|"fresh original install")?;
        let client2=NativeClient::from_installed(c2,proof2).map_err(|_|"fresh original client")?;
        let snapshot2=client2.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|_|"rebound snapshot")?;
        check(snapshot2.stamp.target==snapshot.stamp.target && host.test_pin_count()==1,"same producer/one original pin")?;
        check(client2.close().await!=cc_desk_remote_security::DataError::Uncertain,"fresh client close uncertain")?;
        check(h2.wait_closed().await!=cc_desk_remote_security::DataError::Uncertain,"fresh Host channel close uncertain")?;
        check(original_host.as_ref().unwrap().authority().check().is_err(),"failed peer remains live")?;
        let local=access.write_frame(b"ive").map_err(|_|"phone loss stopped original local input")?;
        check(local.state==crate::terminal_input::InputWriteState::HostWritten && local.confirmed_bytes==3,"exact local continuation")?;
        drop(access);
        fixture.wait_lifecycle(|state|state.process()==ProcessLifecycle::Exited);
        let reports:Vec<Value>=fs::read_dir(fixture.root.path().join("work")).map_err(|_|"own report directory")?.filter_map(Result::ok).filter(|e|e.path().extension().is_some_and(|e|e=="json")).map(|e|serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap()).collect();
        check(reports.iter().any(|r|r["input"]==json!(b"native".to_vec())),"own Node did not consume exactly3 remote+3 local bytes")?;
        Ok(())
    }.await;
    let retired = cleanup.close();
    let network_joined = host.test_join_network().await;
    if let Some(client) = &client {
        let _ = client.close().await;
    }
    if let Some(channel) = &original_client {
        let _ = channel.close().await;
    }
    if let Some(channel) = &original_host {
        let _ = channel.wait_closed().await;
    }
    drop(grant);
    drop(cancel);
    assert!(
        retired && network_joined,
        "original own root/reader/writer/IO/network must retire before assertion"
    );
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ManagedOriginalJobAndRemoteOwnersRetireTogether_002() {
    // One permanent Host admission per process: the complete approved exit
    // scenario extends this original case instead of creating a second Host.
    owned_session_exit_joins_original_owners().await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_LiveSourceClockReentryRejectsSecondAttempt_010() {
    use crate::remote_host_network::*;
    use cc_desk_remote_record::projection;
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
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
    let mut relay = NetworkRelayFixture::start().await;
    let queued = Arc::new(parking_lot::Mutex::new(
        None::<crate::run_supervisor::NativeRunAdmission>,
    ));
    let result = Arc::new(parking_lot::Mutex::new(
        None::<Result<(), HostNetworkError>>,
    ));
    let pending = queued.clone();
    let returned = result.clone();
    let weak_host = Arc::downgrade(&host);
    let weak_runtime = Arc::downgrade(&runtime);
    let (_lease, revoked) = tokio::sync::watch::channel(false);
    let source = Arc::new(ApprovedHostSource {
        clock: Arc::new(move || {
            let admission = pending.lock().take();
            if let Some(admission) = admission {
                let nested = weak_host
                    .upgrade()
                    .unwrap()
                    .test_begin_registered_network(weak_runtime.upgrade().unwrap(), admission)
                    .map(|_| ());
                *returned.lock() = Some(nested);
            }
            // 故意吞掉嵌套失败，外层仍必须保持 sticky 失效。
            Ok(1000)
        }),
        revoked,
        expires_host_ms: 11000,
    });
    let mut peer = None;
    let outcome:Result<(),&'static str>=async {
        let (attempt,connected,token)=open_registered_network_peer(&fixture,&status.run,&host,&runtime,&relay,(source,None),None).await?;
        peer=Some(connected);
        let snapshot=peer.as_ref().unwrap().0.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,Duration::from_secs(1)).await.map_err(|_|"reentry snapshot")?;
        let ack=projection::encode_ack(&projection::Ack{profile:1,stamp:snapshot.stamp},1).map_err(|_|"reentry ack codec")?;
        peer.as_ref().unwrap().0.exchange(&ack,Duration::from_secs(1)).await.map_err(|_|"reentry ack")?;
        let fresh=fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller,&status.run).map_err(|_|"reentry access")?,&status.run).map_err(|_|"reentry admission")?;
        use cc_desk_remote_transport::wss::{RelayAdmissionIdentity,RelayRole};
        let signer=Arc::new(NetworkRelaySigner{origin:relay.origin.clone(),id:[1;16],role:RelayRole::Host});let material_lifetime=Arc::downgrade(&signer);
        host.replenish_native_source(&runtime,&fresh,&token,HostAttemptMaterials{stages:None,local:network_local(false),relay:RelayAdmissionIdentity::new([1;16],RelayRole::Host,signer),outer_accepted:None,termination:None}).map_err(|_|"reentry refill")?;
        *queued.lock()=Some(fresh);
        let window=peer.as_ref().unwrap().0.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#,Duration::from_secs(1)).await;
        if window.is_ok() || !matches!(*result.lock(),Some(Err(_))) {return Err("live recursion did not reject");}
        if host.test_network_attempts()!=1 || material_lifetime.strong_count()!=0 || host.test_source_material_pending(){return Err("live recursion created task or retained fresh material");}
        let joined=tokio::time::timeout(Duration::from_secs(4),attempt.wait_closed()).await.map_err(|_|"original reentry task did not join")?;
        if joined==Err(HostNetworkError::Unknown){return Err("original reentry task unknown");}
        let later=fixture.supervisor.admit_remote_run(fixture.service.access(&fixture.caller,&status.run).map_err(|_|"later access")?,&status.run).map_err(|_|"later admission")?;
        if host.test_begin_registered_network(runtime.clone(),later).is_ok() || host.test_network_attempts()!=1 {return Err("recursive failure revived");}
        eprintln!("TEST_ONLY_LIVE_CLOCK_REENTRY nested={:?} second_attempt=false fresh_released=true original_join={joined:?} sticky=true",*result.lock());
        Ok(())
    }.await;
    queued.lock().take();
    if let Some((client, mux, cancel)) = peer {
        cancel.cancel();
        mux.request_close();
        let _ = tokio::join!(client.close(), mux.wait_closed());
    }
    let physical = cleanup.close();
    let network = host.test_join_network().await;
    let relay_closed = relay.close().await;
    assert!(
        physical && network && relay_closed,
        "original native/network/relay owners must retire"
    );
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[path = "native_cli_remote_approval_keys.rs"]
mod approval_keys;

#[path = "native_cli_remote_rendezvous.rs"]
mod rendezvous;

#[path = "native_cli_remote_start.rs"]
mod owned_start;
#[cfg(windows)]
#[path = "native_cli_remote_pairing_scope.rs"]
mod pairing_scope;
#[cfg(windows)]
#[path = "native_cli_registration_approval.rs"]
mod registration_approval;

#[path = "native_cli_delegated.rs"]
mod delegated;
