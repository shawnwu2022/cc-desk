use super::*;
use crate::{LocalIdentity, P256Spki, PeerPin, Unbound, accept, cancellation, connect};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
fn local(client: bool) -> LocalIdentity {
    let (k, p) = crate::test_identities::identity(client, false);
    LocalIdentity::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
fn pin(client: bool) -> PeerPin {
    let (_, p) = crate::test_identities::identity(client, false);
    PeerPin::new(
        if client { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
fn snapshot() -> HostSnapshot {
    HostSnapshot {
        host_key_epoch: 9007199254740993,
        client_key_epoch: u64::MAX,
        activation: [0x44; 32],
        incarnation: 11,
        policy_epoch: 13,
        issuer_epoch: 17,
        origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
    }
}
fn scalar_one_public() -> P256Spki {
    let value = "3059301306072a8648ce3d020106082a8648ce3d030107034200046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
    let bytes: Vec<_> = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect();
    P256Spki::new(&bytes).unwrap()
}
fn emit(value: serde_json::Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn public_scalar(which: u8) -> P256Spki {
    let point = match which {
        2 => {
            "047cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1"
        }
        3 => {
            "045ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd6c8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032"
        }
        _ => panic!("fixed public TEST_ONLY scalar"),
    };
    let value = format!("3059301306072a8648ce3d020106082a8648ce3d030107034200{point}");
    let b: Vec<_> = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect();
    P256Spki::new(&b).unwrap()
}
#[allow(non_snake_case)]
#[tokio::test]
async fn BrowserGrant_LoopbackFixture_002() {
    let Ok(mode) = std::env::var("CC_DESK_TEST_ONLY_GRANT_MODE") else {
        // 普通cargo test执行原Gate/COSE/WebAuthn校验，不等待外部client。
        let mut gate = crate::grant_tests::browser_test_gate();
        gate.require_browser(crate::browser_approval_tests::profile())
            .unwrap();
        gate.browser_attempt([8; 32], [9; 32]).unwrap();
        let grant = gate
            .challenge(
                crate::grant_tests::scope(),
                cc_desk_remote_record::grant::DataRole::Controller,
                Duration::from_secs(4),
            )
            .unwrap();
        let proof = crate::browser_approval_tests::proof_challenge(
            &grant,
            gate.issuer_kid(),
            crate::browser_approval_tests::profile()
                .challenge(&grant, gate.issuer_kid(), ([8; 32], [9; 32]))
                .unwrap(),
            0,
        );
        gate.accept(&proof).unwrap().check().unwrap();
        return;
    };
    assert!(["normal", "expired", "revoked", "postdata"].contains(&mode.as_str()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (_, host_public) = crate::test_identities::identity(false, false);
    emit(
        serde_json::json!({"kind":"ready","port":listener.local_addr().unwrap().port(),"hostSpki":hex(&host_public)}),
    );
    let (stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let (_handle, token) = cancellation();
    let pending = accept(
        Unbound::new(stream),
        local(false),
        PeerPin::new(Role::Client, public_scalar(2)),
        Duration::from_secs(2),
        token,
    )
    .await
    .unwrap();
    let channel = crate::bind_host_grant_channel(pending, snapshot())
        .await
        .unwrap();
    emit(serde_json::json!({"kind":"bound"}));
    let start = std::time::Instant::now();
    let clock: Arc<dyn crate::GrantClock> =
        Arc::new(move || Ok(1000 + start.elapsed().as_millis() as u64));
    let mut gate = crate::GrantGate::for_binding(
        channel.context(),
        crate::grant_tests::admin(),
        [4; 32],
        clock,
    )
    .unwrap();
    gate.require_browser(
        crate::BrowserProfile::enrolled(
            "https://desk.invalid",
            "desk.invalid",
            vec![7; 32],
            public_scalar(3),
            false,
            false,
        )
        .unwrap(),
    )
    .unwrap();
    if mode == "revoked" {
        gate.revoke();
    }
    let result = channel
        .grant_host(
            &mut gate,
            crate::NativeGrantPolicy {
                scope: crate::grant_tests::scope(),
                role: cc_desk_remote_record::grant::DataRole::Controller,
                duration: if mode == "expired" {
                    Duration::from_millis(50)
                } else {
                    Duration::from_secs(4)
                },
            },
            Duration::from_secs(3),
        )
        .await;
    match result {
        Ok(mut channel) => {
            emit(serde_json::json!({"kind":"grantAccepted"}));
            if mode == "postdata" {
                let owner = channel.start().unwrap();
                owner
                    .send_projection(
                        &crate::grant_tests::scope(),
                        4,
                        b"PUBLIC_TEST_ONLY_UNSUPPORTED_DATA",
                        Duration::from_secs(1),
                    )
                    .await
                    .unwrap();
                emit(serde_json::json!({"kind":"postDataSent"}));
                // 真实peer拒绝后才退出；不以本地sleep/drop制造EOF。
                let exit = owner.wait_closed_checked().await.unwrap();
                assert!(
                    matches!(exit, crate::DataError::Io | crate::DataError::Closed),
                    "{exit:?}"
                );
                emit(serde_json::json!({"kind":"postDataPeerClosed","exit":format!("{exit:?}")}));
            } else {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            drop(channel);
        }
        Err(error) => {
            emit(serde_json::json!({"kind":"grantRejected","error":format!("{error:?}")}))
        }
    }
}
#[allow(non_snake_case)]
#[tokio::test]
async fn BrowserAppBind_LoopbackFixture_001() {
    let Ok(mode) = std::env::var("CC_DESK_TEST_ONLY_APPBIND_MODE") else {
        // 普通cargo test没有外部配置时仍执行真实原生双方建立/关闭，不忽略或假通过。
        let (c, h) = tokio::io::duplex(65536);
        let (_handle, token) = cancellation();
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
        let snapshot = snapshot();
        let trust = ClientTrust {
            host_key_epoch: snapshot.host_key_epoch,
            client_key_epoch: snapshot.client_key_epoch,
            origin: snapshot.origin.clone(),
        };
        let (c, h) = tokio::join!(
            bind_client(c.unwrap(), trust),
            bind_host(h.unwrap(), snapshot)
        );
        let c = c.unwrap();
        let h = h.unwrap();
        assert_eq!(c.selection().minor(), 0);
        assert_eq!(c.context().origin(), "wss://relay.invalid/");
        assert_eq!(
            c.context().host_endpoint_id(),
            h.context().host_endpoint_id()
        );
        let _ = c.close().await;
        let _ = h.close().await;
        return;
    };
    assert!(
        [
            "normal",
            "mac",
            "direction",
            "identity",
            "role",
            "epoch",
            "clientepoch",
            "origin",
            "silent",
            "postdata",
            "replay"
        ]
        .contains(&mode.as_str())
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (_, host_public) = crate::test_identities::identity(false, false);
    emit(
        serde_json::json!({"kind":"ready","port":listener.local_addr().unwrap().port(),"hostSpki":hex(&host_public)}),
    );
    let capture = Arc::new(Mutex::new(None::<Mac>));
    for round in 0..if mode == "replay" { 2 } else { 1 } {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let (_handle, token) = cancellation();
        let pending = accept(
            Unbound::new(stream),
            local(false),
            PeerPin::new(Role::Client, scalar_one_public()),
            Duration::from_secs(2),
            token,
        )
        .await
        .unwrap();
        emit(serde_json::json!({"kind":"tlsAccepted","round":round}));
        if mode == "silent" {
            tokio::time::sleep(Duration::from_secs(2)).await;
            drop(pending);
            continue;
        }
        let mut snapshot = snapshot();
        if mode == "epoch" {
            snapshot.host_key_epoch += 1;
        }
        if mode == "clientepoch" {
            snapshot.client_key_epoch -= 1;
        }
        if mode == "origin" {
            snapshot.origin = CanonicalOrigin::trusted("wss://other.invalid/").unwrap();
        }
        let hooks = if ["mac", "identity", "role", "replay"].contains(&mode.as_str()) {
            let selected = mode.clone();
            let capture = capture.clone();
            Hooks {
                alter: Some(Arc::new(move |mut message| {
                    match &mut message {
                        Message::HostOffer(h) if selected == "identity" => {
                            h.context.host_endpoint_id = "00".repeat(32)
                        }
                        Message::HostConfirm(h) if selected == "mac" => {
                            h.mac = Mac::from_bytes([0; 32])
                        }
                        Message::HostConfirm(h) if selected == "role" => {
                            h.selection.requested_role = RequestedRole::Controller
                        }
                        Message::HostConfirm(h) if selected == "replay" => {
                            let mut saved = capture.lock().unwrap();
                            if round == 0 {
                                *saved = Some(h.mac.clone());
                            } else {
                                h.mac = saved.clone().unwrap();
                            }
                        }
                        _ => (),
                    }
                    message
                })),
                ..Hooks::default()
            }
        } else {
            Hooks {
                client_direction_host_mac: mode == "direction",
                ..Hooks::default()
            }
        };
        if mode == "postdata" {
            let mut e = establish_host(pending, snapshot, Hooks::default(), vec![0], 0)
                .await
                .unwrap();
            emit(serde_json::json!({"kind":"bound","round":round}));
            e.io.write_all(&[1]).await.unwrap();
            e.io.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(e);
        } else {
            // 正例走原public Host工厂；所有mutants仅存在cfg(test)，复用原Hooks和握手。
            let result = if mode == "normal" {
                bind_host(pending, snapshot).await
            } else {
                bind_host_with(pending, snapshot, hooks).await
            };
            match result {
                Ok(mut owner) => {
                    emit(
                        serde_json::json!({"kind":"bound","round":round,"origin":owner.context().origin(),"minor":owner.selection().minor()}),
                    );
                    let _ = tokio::time::timeout(Duration::from_secs(3), owner.wait_closed()).await;
                    let _ = owner.close().await;
                }
                Err(error) => emit(
                    serde_json::json!({"kind":"bindRejected","round":round,"error":format!("{error:?}")}),
                ),
            }
        }
    }
    emit(serde_json::json!({"kind":"closed"}));
}
