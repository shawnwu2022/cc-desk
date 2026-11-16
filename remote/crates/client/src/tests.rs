use super::*;
use cc_desk_remote_record::control_io as wire;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::*;
use std::sync::{Arc, atomic::AtomicU64};
mod support;
const BUDGET: Duration = Duration::from_secs(1);

#[tokio::test]
async fn native_cursor_exchange_correlates_request_and_rejects_a_window_as_cursor() {
    for wrong in [false, true] {
        let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
        let client = NativeClient::new(c, 4).unwrap();
        let server = async {
            h.receive(&support::scope(), 4, BUDGET).await.unwrap();
            let response = if wrong {
                wire::Message::WindowGranted {
                    profile: 1,
                    request: "1".into(),
                    window: "09".repeat(32),
                    not_after_host_ms: "2000".into(),
                }
            } else {
                wire::Message::InputCursorState {
                    profile: 1,
                    request: "1".into(),
                    next_sequence: Some("42".into()),
                    pending: None,
                }
            };
            h.send_projection(
                &support::scope(),
                4,
                &wire::encode(&response).unwrap(),
                BUDGET,
            )
            .await
            .unwrap();
        };
        let (result, ()) = tokio::join!(
            client.exchange(
                br#"{"message":"InputCursor","profile":1,"request":"1"}"#,
                BUDGET
            ),
            server
        );
        if wrong {
            assert_eq!(result.unwrap_err(), DataError::Context);
        } else {
            assert!(
                matches!(wire::decode(&result.unwrap()).unwrap(),wire::Message::InputCursorState{next_sequence:Some(n),..} if n=="42")
            );
        }
        client.close().await;
        h.close().await;
    }
}
#[path = "../../security/tests/support/identities.rs"]
#[allow(dead_code)]
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
pub(super) async fn channels(
    now: Arc<AtomicU64>,
) -> (RunningChannel, RunningChannel, GrantGate, CancelHandle) {
    let (mut c, mut h, gate, cancel) = managed_channels(now, DataRole::Controller, false).await;
    (c.start().unwrap(), h.start().unwrap(), gate, cancel)
}
async fn managed_channels(
    now: Arc<AtomicU64>,
    role: DataRole,
    lease: bool,
) -> (
    ManagedChannel<tokio::io::DuplexStream>,
    ManagedChannel<tokio::io::DuplexStream>,
    GrantGate,
    CancelHandle,
) {
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
    let trust = ClientTrust {
        host_key_epoch: 5,
        client_key_epoch: 7,
        origin: origin.clone(),
    };
    let snapshot = HostSnapshot {
        host_key_epoch: 5,
        client_key_epoch: 7,
        activation: [0x33; 32],
        incarnation: 11,
        policy_epoch: 13,
        issuer_epoch: 17,
        origin,
    };
    let (c, h) = if lease {
        tokio::join!(
            bind_client_control_lease_channel(c.unwrap(), trust),
            bind_host_control_lease_channel(h.unwrap(), snapshot)
        )
    } else {
        tokio::join!(
            bind_client_grant_channel(c.unwrap(), trust),
            bind_host_grant_channel(h.unwrap(), snapshot)
        )
    };
    let (c, h) = (c.unwrap(), h.unwrap());
    let clock: Arc<dyn GrantClock> =
        Arc::new(move || Ok(now.load(std::sync::atomic::Ordering::Acquire)));
    let mut gate =
        GrantGate::for_binding(h.context(), support::admin(), [4; 32], clock.clone()).unwrap();
    let mut state = ClientGrantState::new(clock);
    let mut signer = PublicSigner;
    let trust = ClientGrantTrust {
        issuer: support::admin(),
        principal: [4; 32],
        scope: support::scope(),
        role,
    };
    let policy = NativeGrantPolicy {
        scope: support::scope(),
        role,
        duration: Duration::from_secs(10),
    };
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, BUDGET),
        h.grant_host(&mut gate, policy, BUDGET)
    );
    (c.unwrap(), h.unwrap(), gate, cancel)
}

#[tokio::test]
async fn native_bridge_owns_actual_client_channel_and_correlates_reply() {
    let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
    let native = NativeClient::new(c, 4).expect("actual Client-role owner");
    let server = async {
        let r = h.receive(&support::scope(), 4, BUDGET).await.unwrap();
        assert_eq!(
            r.payload(),
            br#"{"message":"Window","profile":1,"request":"1"}"#
        );
        let bytes = wire::encode(&wire::Message::WindowGranted {
            profile: 1,
            request: "1".into(),
            window: "09".repeat(32),
            not_after_host_ms: "2000".into(),
        })
        .unwrap();
        h.send_projection(&support::scope(), 4, &bytes, BUDGET)
            .await
            .unwrap();
    };
    let (reply, ()) = tokio::join!(
        native.exchange(br#"{"message":"Window","profile":1,"request":"1"}"#, BUDGET),
        server
    );
    assert!(
        matches!(wire::decode(&reply.unwrap()).unwrap(),wire::Message::WindowGranted {request,..} if request=="1")
    );
    native.close().await;
    h.close().await;
}

#[tokio::test]
async fn native_bridge_rejects_wrong_reply_and_retires_only_original_owner() {
    let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
    let native = NativeClient::new(c, 4).unwrap();
    let server = async {
        h.receive(&support::scope(), 4, BUDGET).await.unwrap();
        let bytes = wire::encode(&wire::Message::WindowGranted {
            profile: 1,
            request: "2".into(),
            window: "09".repeat(32),
            not_after_host_ms: "2000".into(),
        })
        .unwrap();
        h.send_projection(&support::scope(), 4, &bytes, BUDGET)
            .await
            .unwrap();
    };
    let (reply, ()) = tokio::join!(
        native.exchange(br#"{"message":"Window","profile":1,"request":"1"}"#, BUDGET),
        server
    );
    assert_eq!(reply.unwrap_err(), DataError::Context);
    assert!(native.channel.is_closed() || native.channel.authority().check().is_err());
    native.close().await;
    h.close().await;
}

fn projection(native: &NativeClient) -> Vec<u8> {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json"
    ))
    .unwrap();
    let a = native.channel.authority();
    value["body"]["stamp"]["target"] = serde_json::json!({"host":a.context().host_endpoint_id,"session":a.scope().session,"run":a.scope().run,"generation":a.scope().generation,"incarnation":a.context().incarnation});
    value["body"]["stamp"]["binding"] = a
        .binding_id()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into();
    serde_json::to_vec(&value).unwrap()
}

#[tokio::test]
async fn native_bridge_snapshot_requires_exact_live_target_and_complete_coverage() {
    for wrong in [false, true] {
        let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
        let native = NativeClient::new(c, 4).unwrap();
        let mut bytes = projection(&native);
        if wrong {
            let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            v["body"]["stamp"]["target"]["run"] = "OTHER".into();
            bytes = serde_json::to_vec(&v).unwrap();
        }
        let server = async {
            h.receive(&support::scope(), 4, BUDGET).await.unwrap();
            h.send_projection(&support::scope(), 4, &bytes, BUDGET)
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(
            native.snapshot(
                br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
                BUDGET
            ),
            server
        );
        if wrong {
            assert_eq!(result.err(), Some(DataError::Context));
        } else {
            let batch = result.unwrap();
            assert_eq!(batch.parts.len(), 1);
            assert_eq!(batch.stamp.target.run, support::scope().run);
        }
        native.close().await;
        h.close().await;
    }
}

#[tokio::test]
async fn native_bridge_cancelled_wait_closes_original_channel_and_concurrent_call_does_not_queue() {
    let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
    let native = NativeClient::new(c, 4).unwrap();
    let mut pending =
        Box::pin(native.exchange(br#"{"message":"Window","profile":1,"request":"1"}"#, BUDGET));
    let scope = support::scope();
    tokio::select! {r=&mut pending=>panic!("unexpected completion {r:?}"),r=h.receive(&scope,4,BUDGET)=>{r.unwrap();}}
    assert_eq!(
        native
            .exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, BUDGET)
            .await
            .err(),
        Some(DataError::Full)
    );
    drop(pending);
    assert!(native.channel.authority().check().is_err());
    native.close().await;
    h.close().await;
}

#[tokio::test]
async fn request_close_invalidates_original_native_owner_synchronously() {
    let now = Arc::new(AtomicU64::new(1000));
    let (c, h, _gate, _cancel) = channels(now).await;
    let client = NativeClient::new(c, 4).unwrap();
    client.request_close();
    assert!(client.channel.authority().check().is_err());
    client.close().await;
    h.wait_closed().await;
}

#[test]
#[allow(non_snake_case)]
fn ControlLease_ClientReplyCorrelatesEveryIdentityField_012() {
    let request = serde_json::json!({"message":"LeaseRenew","profile":1,"request":"7","lease":"08".repeat(16),"fence":"4","renewalSequence":"2"});
    let expected = expected(&serde_json::to_vec(&request).unwrap()).unwrap();
    let reply = serde_json::json!({"message":"LeaseRenewed","profile":1,"request":"7","lease":"08".repeat(16),"fence":"4","renewalSequence":"2","notAfterHostMs":"16000"});
    assert!(expected.matches(&wire::decode(&serde_json::to_vec(&reply).unwrap()).unwrap()));
    for field in ["request", "lease", "fence", "renewalSequence"] {
        let mut bad = reply.clone();
        bad[field] = if field == "lease" {
            "09".repeat(16).into()
        } else {
            "9".into()
        };
        assert!(!expected.matches(&wire::decode(&serde_json::to_vec(&bad).unwrap()).unwrap()));
    }
}
#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_ClientOldCapabilityDoesNotSendRequest_013() {
    let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
    let client = NativeClient::new(c, 4).unwrap();
    assert!(
        client
            .exchange(
                br#"{"message":"LeaseState","profile":1,"request":"1"}"#,
                BUDGET
            )
            .await
            .is_err()
    );
    assert!(
        h.receive(&support::scope(), 4, Duration::from_millis(20))
            .await
            .is_err()
    );
    client.close().await;
    h.close().await;
}

#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_ClientGrantBoundsAndNegotiatedObserver_014() {
    for until in [1000, 10999, 11000, 11001] {
        let (mut c, mut h, _gate, _cancel) =
            managed_channels(Arc::new(AtomicU64::new(1000)), DataRole::Controller, true).await;
        let end = tokio::time::Instant::now() + BUDGET;
        let (c, h) = (
            c.start_peer_pending(end).unwrap(),
            h.start_peer_pending(end).unwrap(),
        );
        let request = h.host_install_request().await.unwrap();
        let (proof, sent) = tokio::join!(c.client_installed(), h.complete_host_install(request, 4));
        sent.unwrap();
        let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
        let server = async {
            h.receive(&support::scope(), 4, BUDGET).await.unwrap();
            let reply = wire::Message::LeaseStateReply {
                profile: 1,
                request: "7".into(),
                lease: "08".repeat(16),
                fence: "4".into(),
                next_renewal_sequence: "1".into(),
                not_after_host_ms: until.to_string(),
            };
            h.send_projection(&support::scope(), 4, &wire::encode(&reply).unwrap(), BUDGET)
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(
            client.exchange(
                br#"{"message":"LeaseState","profile":1,"request":"7"}"#,
                BUDGET
            ),
            server
        );
        assert_eq!(result.is_ok(), until > 1000 && until <= 11000);
        client.close().await;
        h.close().await;
    }
    let (mut c, mut h, _gate, _cancel) =
        managed_channels(Arc::new(AtomicU64::new(1000)), DataRole::Observer, true).await;
    let end = tokio::time::Instant::now() + BUDGET;
    let (c, h) = (
        c.start_peer_pending(end).unwrap(),
        h.start_peer_pending(end).unwrap(),
    );
    let request = h.host_install_request().await.unwrap();
    let (proof, sent) = tokio::join!(c.client_installed(), h.complete_host_install(request, 4));
    sent.unwrap();
    let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
    assert!(
        client
            .exchange(
                br#"{"message":"LeaseState","profile":1,"request":"7"}"#,
                BUDGET
            )
            .await
            .is_err()
    );
    assert!(
        h.receive(&support::scope(), 4, Duration::from_millis(20))
            .await
            .is_err()
    );
    client.close().await;
    h.close().await;
}

#[path = "snapshot_pull_probe.rs"]
mod snapshot_pull_probe;
