use super::*;
use crate::grant_tests::{admin, scope, signed};
use crate::test_identities as identities;
use crate::*;
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
fn identity(client: bool) -> LocalIdentity {
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
async fn bound_pair(
    client_grant: bool,
    host_grant: bool,
) -> (
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    Result<ManagedChannel<tokio::io::DuplexStream>, BindError>,
    CancelHandle,
) {
    let (c, h) = tokio::io::duplex(65536);
    let (cancel, token) = cancellation();
    let (c, h) = tokio::join!(
        connect(
            Unbound::new(c),
            identity(true),
            pin(false),
            Duration::from_secs(2),
            token.clone()
        ),
        accept(
            Unbound::new(h),
            identity(false),
            pin(true),
            Duration::from_secs(2),
            token
        )
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
    let (c, h) = tokio::join!(
        async {
            if client_grant {
                bind_client_grant_channel(c.unwrap(), trust).await
            } else {
                bind_client_channel(c.unwrap(), trust).await
            }
        },
        async {
            if host_grant {
                bind_host_grant_channel(h.unwrap(), snapshot).await
            } else {
                bind_host_channel(h.unwrap(), snapshot).await
            }
        }
    );
    (c, h, cancel)
}
struct PublicSigner {
    calls: usize,
}
impl NativeGrantSigner for PublicSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        self.calls += 1;
        Box::pin(async move { Ok(signed(grant.payload(), grant.issuer_kid())) })
    }
}
fn policy() -> NativeGrantPolicy {
    NativeGrantPolicy {
        scope: scope(),
        role: DataRole::Controller,
        duration: Duration::from_secs(4),
    }
}
fn trust() -> ClientGrantTrust {
    ClientGrantTrust {
        issuer: admin(),
        principal: [4; 32],
        scope: scope(),
        role: DataRole::Controller,
    }
}
fn clock(now: &Arc<AtomicU64>) -> Arc<dyn GrantClock> {
    let now = now.clone();
    Arc::new(move || Ok(now.load(Ordering::Acquire)))
}
#[tokio::test]
async fn grant_management_actual_tls_exchange_installs_distinct_client_access_and_reuses_original_expiry()
 {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    assert!(c.grant_management_enabled());
    let client_trust = trust();
    let hn = Arc::new(AtomicU64::new(1000));
    let cn = Arc::new(AtomicU64::new(200));
    let mut gate = GrantGate::for_binding(h.context(), admin(), [4; 32], clock(&hn)).unwrap();
    let mut state = ClientGrantState::new(clock(&cn));
    assert!(state.remote_grant().is_none());
    let mut signer = PublicSigner { calls: 0 };
    let (c, h) = tokio::join!(
        c.grant_client(
            &client_trust,
            &mut signer,
            &mut state,
            Duration::from_secs(1)
        ),
        h.grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    let (mut c, mut h) = (c.unwrap(), h.unwrap());
    let remote = state.remote_grant().unwrap();
    assert_eq!(remote.check().unwrap(), 4000);
    assert!(h.install_remote(&remote).is_err());
    let (c, h) = (c.start().unwrap(), h.start().unwrap());
    assert_eq!(c.authority().local_role(), Role::Client);
    assert_eq!(h.authority().local_role(), Role::Host);
    let target = scope();
    let (a, b) = tokio::join!(
        c.send_control(&target, 4, b"PUBLIC_WIRE_GRANT", Duration::from_secs(1)),
        h.receive(&target, 4, Duration::from_secs(1))
    );
    a.unwrap();
    assert_eq!(b.unwrap().payload(), b"PUBLIC_WIRE_GRANT");
    c.close().await;
    h.wait_closed().await;
    hn.store(1500, Ordering::Release);
    cn.store(700, Ordering::Release);
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = tokio::join!(
        c.unwrap().grant_client(
            &client_trust,
            &mut signer,
            &mut state,
            Duration::from_secs(1)
        ),
        h.unwrap()
            .grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    let (_c, _h) = (c.unwrap(), h.unwrap());
    assert_eq!(signer.calls, 1);
    assert_eq!(remote.check().unwrap(), 3500);
    cn.store(4200, Ordering::Release);
    hn.store(5000, Ordering::Release);
    assert!(remote.check().is_err());
    assert!(state.remote_grant().is_none());
}
#[tokio::test]
async fn grant_management_capability_does_not_downgrade_to_legacy() {
    for (c, h) in [(true, false), (false, true)] {
        let (c, h, _cancel) = bound_pair(c, h).await;
        assert!(c.is_err() || h.is_err());
    }
}
#[tokio::test]
async fn grant_management_wrong_scope_cannot_prompt_signer_or_enable_data() {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
    let mut signer = PublicSigner { calls: 0 };
    let mut wrong = trust();
    wrong.scope.run = "OTHER".into();
    let (c, h) = tokio::join!(
        c.grant_client(&wrong, &mut signer, &mut state, Duration::from_secs(1)),
        h.grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    assert!(c.is_err());
    assert!(h.is_err());
    assert_eq!(signer.calls, 0);
    assert!(state.remote_grant().is_none());
}

async fn management_read(channel: &mut ManagedChannel<tokio::io::DuplexStream>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let io = channel.io.as_mut().unwrap();
    let mut header = [0; 8];
    io.read_exact(&mut header).await.unwrap();
    let h = cc_desk_remote_record::Header::decode(header, 1).unwrap();
    assert_eq!(h.kind, cc_desk_remote_record::Kind::Control);
    let mut body = vec![0; h.payload_len];
    io.read_exact(&mut body).await.unwrap();
    body
}
async fn management_write(channel: &mut ManagedChannel<tokio::io::DuplexStream>, payload: &[u8]) {
    use tokio::io::AsyncWriteExt;
    let bytes =
        cc_desk_remote_record::encode(cc_desk_remote_record::Kind::Control, 1, payload).unwrap();
    channel
        .io
        .as_mut()
        .unwrap()
        .write_all(&bytes)
        .await
        .unwrap();
    channel.io.as_mut().unwrap().flush().await.unwrap();
}
#[tokio::test]
async fn grant_management_lost_acceptance_reuses_frozen_material_without_new_presence_or_anchor() {
    use cc_desk_remote_record::grant_management::{self as wire, Message};
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, mut h) = (c.unwrap(), h.unwrap());
    let hn = Arc::new(AtomicU64::new(1000));
    let cn = Arc::new(AtomicU64::new(200));
    let mut gate = GrantGate::for_binding(h.context(), admin(), [4; 32], clock(&hn)).unwrap();
    let mut state = ClientGrantState::new(clock(&cn));
    let mut signer = PublicSigner { calls: 0 };
    let trust = trust();
    let host = async {
        let Message::Request { nonce, reuse } =
            wire::decode(&management_read(&mut h).await).unwrap()
        else {
            panic!("request")
        };
        assert!(!reuse);
        let g = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
            .unwrap();
        let binding = h.binding.iter().map(|b| format!("{b:02x}")).collect();
        management_write(
            &mut h,
            &wire::encode(&Message::Challenge {
                nonce,
                binding,
                grant: g,
            })
            .unwrap(),
        )
        .await;
        let signed = management_read(&mut h).await;
        gate.accept(&signed).unwrap();
        drop(h); // Actual acceptance, lost acknowledgement.
    };
    let (client, ()) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
        host
    );
    assert!(client.is_err());
    assert!(state.remote_grant().is_none());
    assert_eq!(signer.calls, 1);
    hn.store(1500, Ordering::Release);
    cn.store(700, Ordering::Release);
    let (c, h, _cancel2) = bound_pair(true, true).await;
    let (c, h) = tokio::join!(
        c.unwrap()
            .grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
        h.unwrap()
            .grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    assert!(c.is_ok());
    assert!(h.is_ok());
    assert_eq!(signer.calls, 1);
    assert_eq!(state.remote_grant().unwrap().check().unwrap(), 3500);
}
struct AlteredSigner {
    host_clock: Option<Arc<AtomicU64>>,
    corrupt: bool,
}
impl NativeGrantSigner for AlteredSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        g: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async move {
            if let Some(now) = &self.host_clock {
                now.store(31000, Ordering::Release)
            }
            let mut b = signed(g.payload(), g.issuer_kid());
            if self.corrupt {
                let n = b.len();
                b[n - 1] ^= 1;
            }
            Ok(b)
        })
    }
}
#[tokio::test]
async fn grant_management_wrong_issuer_corrupt_signature_and_host_30_second_equality_refuse() {
    for case in 0..3 {
        let (c, h, _cancel) = bound_pair(true, true).await;
        let (c, h) = (c.unwrap(), h.unwrap());
        let hn = Arc::new(AtomicU64::new(1000));
        let mut gate = GrantGate::for_binding(h.context(), admin(), [4; 32], clock(&hn)).unwrap();
        let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
        let mut trust = trust();
        if case == 0 {
            trust.issuer = P256Spki::new(&identities::identity(false, false).1).unwrap();
        }
        let mut signer = AlteredSigner {
            host_clock: (case == 2).then_some(hn),
            corrupt: case == 1,
        };
        let (c, h) = tokio::join!(
            c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
            h.grant_host(&mut gate, policy(), Duration::from_secs(1))
        );
        assert!(c.is_err(), "case{case}");
        assert!(h.is_err(), "case{case}");
        assert!(state.remote_grant().is_none());
    }
}
#[tokio::test]
async fn grant_management_remote_mode_rejects_pregrant_native_clone_and_wrong_endpoint_role() {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let mut h = h.unwrap();
    let mut c = c.unwrap();
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let value = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let native = gate.accept(&signed(&value, gate.issuer_kid())).unwrap();
    assert!(c.install(&native).is_err());
    assert!(c.start().is_err());
    assert!(h.start().is_err());
    let (c, h, _cancel2) = bound_pair(true, true).await;
    assert!(
        c.unwrap()
            .grant_host(&mut gate, policy(), Duration::from_secs(1))
            .await
            .is_err()
    );
    let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
    let mut signer = PublicSigner { calls: 0 };
    assert!(
        h.unwrap()
            .grant_client(&trust(), &mut signer, &mut state, Duration::from_secs(1))
            .await
            .is_err()
    );
    assert_eq!(signer.calls, 0);
}

#[tokio::test]
async fn grant_management_tls_identity_issuer_is_rejected_before_native_presence() {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
    let mut signer = PublicSigner { calls: 0 };
    let mut trust = trust();
    trust.issuer = P256Spki::new(&identities::identity(false, false).1).unwrap();
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
        h.grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    assert!(c.is_err());
    assert!(h.is_err());
    assert_eq!(signer.calls, 0);
}

#[tokio::test]
async fn grant_management_clock_rollback_permanently_closes_remote_access() {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let cn = Arc::new(AtomicU64::new(200));
    let mut state = ClientGrantState::new(clock(&cn));
    let mut signer = PublicSigner { calls: 0 };
    let trust = trust();
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
        h.grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    let (mut c, mut h) = (c.unwrap(), h.unwrap());
    let remote = state.remote_grant().unwrap();
    let (c, h) = (c.start().unwrap(), h.start().unwrap());
    cn.store(199, Ordering::Release);
    assert!(remote.check().is_err());
    cn.store(201, Ordering::Release);
    assert!(remote.check().is_err());
    assert!(
        c.send_control(&scope(), 4, b"AFTER_ROLLBACK", Duration::from_secs(1))
            .await
            .is_err()
    );
    c.close().await;
    h.wait_closed().await;
}
#[tokio::test]
async fn grant_management_acceptance_nonce_digest_binding_and_order_are_exact() {
    use cc_desk_remote_record::grant_management::{self as wire, Message};
    for case in 0..4 {
        let (c, h, _cancel) = bound_pair(true, true).await;
        let (c, mut h) = (c.unwrap(), h.unwrap());
        let mut gate =
            GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
        let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
        let mut signer = PublicSigner { calls: 0 };
        let trust = trust();
        let host = async {
            let Message::Request { nonce, reuse } =
                wire::decode(&management_read(&mut h).await).unwrap()
            else {
                panic!("request")
            };
            assert!(!reuse);
            let grant = gate
                .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
                .unwrap();
            let binding: String = h.binding.iter().map(|b| format!("{b:02x}")).collect();
            let challenge = Message::Challenge {
                nonce: nonce.clone(),
                binding: binding.clone(),
                grant,
            };
            management_write(&mut h, &wire::encode(&challenge).unwrap()).await;
            let signed = management_read(&mut h).await;
            let accepted = gate.accept(&signed).unwrap();
            let digest: String = ring::digest::digest(&ring::digest::SHA256, &signed)
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let grant_id = accepted.id().iter().map(|b| format!("{b:02x}")).collect();
            let reply = if case == 2 {
                challenge
            } else {
                Message::Accepted {
                    nonce: if case == 0 { "44".repeat(32) } else { nonce },
                    binding: if case == 3 { "55".repeat(32) } else { binding },
                    grant_id,
                    digest: if case == 1 { "66".repeat(32) } else { digest },
                }
            };
            management_write(&mut h, &wire::encode(&reply).unwrap()).await;
        };
        let (client, ()) = tokio::join!(
            c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
            host
        );
        assert!(client.is_err(), "case{case}");
        assert!(state.remote_grant().is_none());
    }
}
struct SlowSigner;
impl NativeGrantSigner for SlowSigner {
    fn confirm_and_sign<'a>(
        &'a mut self,
        g: &'a VerifiedCanonicalGrant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>>
    {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(signed(g.payload(), g.issuer_kid()))
        })
    }
}
#[tokio::test(start_paused = true)]
async fn grant_management_absolute_deadline_drops_cooperative_presence_and_channel() {
    let (c, h, _cancel) = bound_pair(true, true).await;
    let (c, h) = (c.unwrap(), h.unwrap());
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let mut state = ClientGrantState::new(Arc::new(|| Ok(200)));
    let mut signer = SlowSigner;
    let trust = trust();
    let (c, h) = tokio::join!(
        c.grant_client(&trust, &mut signer, &mut state, Duration::from_secs(1)),
        h.grant_host(&mut gate, policy(), Duration::from_secs(1))
    );
    assert!(c.is_err());
    assert!(h.is_err());
    assert!(state.remote_grant().is_none());
}

#[tokio::test]
async fn grant_management_old_cose_cannot_override_current_native_policy_or_fresh_challenge() {
    use cc_desk_remote_record::grant_management::{self as wire, Message};
    use tokio::io::AsyncReadExt;
    for reuse in [true, false] {
        for case in 0..4 {
            if reuse && case == 3 {
                continue;
            }
            let (c, h, _cancel) = bound_pair(true, true).await;
            let (mut c, h) = (c.unwrap(), h.unwrap());
            let mut gate =
                GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000)))
                    .unwrap();
            let old = gate
                .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
                .unwrap();
            let old_bytes = signed(&old, gate.issuer_kid());
            let native = gate.accept(&old_bytes).unwrap();
            let mut current = policy();
            if case == 0 {
                current.scope.run = "OTHER".into();
            }
            if case == 1 {
                current.role = DataRole::Observer;
            }
            if case == 2 {
                current.duration = Duration::from_secs(1);
            }
            let peer = async {
                management_write(
                    &mut c,
                    &wire::encode(&Message::Request {
                        nonce: "77".repeat(32),
                        reuse,
                    })
                    .unwrap(),
                )
                .await;
                if !reuse {
                    assert!(matches!(
                        wire::decode(&management_read(&mut c).await).unwrap(),
                        Message::Challenge { .. }
                    ));
                }
                management_write(&mut c, &old_bytes).await;
                let mut header = [0; 8];
                c.io.as_mut().unwrap().read_exact(&mut header).await.is_ok()
            };
            let (received, host) = tokio::join!(
                peer,
                h.grant_host(&mut gate, current, Duration::from_secs(1))
            );
            assert!(host.is_err(), "reuse={reuse} case={case}");
            assert!(!received);
            assert!(native.check().is_ok());
        }
    }
}

#[tokio::test]
async fn grant_management_three_malformed_submissions_consume_pending_budget_not_old_grant() {
    use cc_desk_remote_record::grant_management::{self as wire, Message};
    use tokio::io::AsyncReadExt;
    let (_, h, _seed_cancel) = bound_pair(true, true).await;
    let h = h.unwrap();
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let old = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let native = gate.accept(&signed(&old, gate.issuer_kid())).unwrap();
    drop(h);
    let pending = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let mut malformed = signed(&pending, gate.issuer_kid());
    let end = malformed.len() - 1;
    malformed[end] ^= 1;
    for attempt in 0..3 {
        let (c, h, _cancel) = bound_pair(true, true).await;
        let (mut c, h) = (c.unwrap(), h.unwrap());
        let peer = async {
            management_write(
                &mut c,
                &wire::encode(&Message::Request {
                    nonce: "77".repeat(32),
                    reuse: true,
                })
                .unwrap(),
            )
            .await;
            management_write(&mut c, &malformed).await;
            let mut header = [0; 8];
            assert!(
                c.io.as_mut()
                    .unwrap()
                    .read_exact(&mut header)
                    .await
                    .is_err()
            );
        };
        let ((), host) = tokio::join!(
            peer,
            h.grant_host(&mut gate, policy(), Duration::from_secs(1))
        );
        assert!(host.is_err());
        assert!(native.check().is_ok());
        if attempt < 2 {
            assert!(matches!(
                gate.challenge(scope(), DataRole::Controller, Duration::from_secs(4)),
                Err(GrantError::Busy)
            ));
        }
    }
    assert!(gate.accept(&signed(&pending, gate.issuer_kid())).is_err());
    assert!(
        gate.challenge(scope(), DataRole::Controller, Duration::from_secs(4))
            .is_ok()
    );
    assert!(native.check().is_ok());
}

#[tokio::test]
async fn grant_management_current_policy_rejection_does_not_consume_pending_signature_budget() {
    let (_, h, _cancel) = bound_pair(true, true).await;
    let h = h.unwrap();
    let mut gate =
        GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
    let old = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let old_bytes = signed(&old, gate.issuer_kid());
    let native = gate.accept(&old_bytes).unwrap();
    let mut different = scope();
    different.run = "OTHER".into();
    let pending = gate
        .challenge(
            different.clone(),
            DataRole::Observer,
            Duration::from_secs(4),
        )
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            gate.verify_management(
                &old_bytes,
                &different,
                DataRole::Observer,
                Duration::from_secs(4),
                Some(&pending)
            ),
            Err(GrantError::Policy)
        );
        assert!(native.check().is_ok());
    }
    assert!(matches!(
        gate.challenge(different, DataRole::Observer, Duration::from_secs(4)),
        Err(GrantError::Busy)
    ));
    assert!(gate.accept(&signed(&pending, gate.issuer_kid())).is_ok());
}
