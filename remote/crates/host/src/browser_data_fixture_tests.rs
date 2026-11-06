//! 公开TEST_ONLY身份/自产内存writer，真实Host/Journal/dispatcher；不运行PTY或CLI。
use crate::{
    journal::Journal,
    native_host::{NativeHost, NativeRunProducer, WriterRetirement},
};
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
fn host_key() -> (Arc<dyn rustls::sign::SigningKey>, Vec<u8>) {
    let decode = |s: &str| -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    };
    let vector: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../security/tests/vectors/signatures/rfc6979-p256.json"
    ))
    .unwrap();
    let point = decode(vector["public_key_sec1"].as_str().unwrap());
    // 公开RFC6979 TEST_ONLY标量；SEC1由原rustls/ring解析和签名，不实现另一套TLS。
    let mut sec1 = decode(
        "30770201010420c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721a00a06082a8648ce3d030107a144034200",
    );
    sec1.extend_from_slice(&point);
    let signing = rustls::crypto::ring::sign::any_supported_type(
        &rustls::pki_types::PrivateKeyDer::Sec1(sec1.into()),
    )
    .unwrap();
    let mut spki = decode("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(point);
    (signing, spki)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn public(which: u8) -> P256Spki {
    let point = match which {
        2 => {
            "047cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1"
        }
        3 => {
            "045ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd6c8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032"
        }
        _ => panic!("TEST_ONLY scalar"),
    };
    let value = format!("3059301306072a8648ce3d020106082a8648ce3d030107034200{point}");
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
struct MemoryWriter(Arc<Mutex<(usize, Vec<u8>)>>, bool);
impl Write for MemoryWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut w = self.0.lock().unwrap();
        w.0 += 1;
        w.1.extend_from_slice(bytes);
        if self.1 {
            Err(std::io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(bytes.len())
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[allow(non_snake_case)]
#[tokio::test]
async fn BrowserData_OriginalHostFixture_001() {
    let mode = std::env::var("CC_DESK_TEST_ONLY_DATA_MODE").unwrap_or_else(|_| "selftest".into());
    assert!(
        [
            "selftest",
            "normal",
            "reconnect",
            "unknown",
            "expired",
            "revoke"
        ]
        .contains(&mode.as_str())
    );
    let dir = tempfile::tempdir().unwrap();
    let writes = Arc::new(Mutex::new((0, Vec::new())));
    let mut producer = NativeRunProducer::start(
        Journal::open(&dir.path().join("TEST_ONLY.sqlite")).unwrap(),
        MemoryWriter(writes.clone(), mode == "unknown"),
        "PUBLIC_TEST_RUN".into(),
        7,
        3,
        10,
    )
    .unwrap();
    let scope = producer.native_scope();
    let mut output = producer.take_output().unwrap();
    output.feed(0, b"TEST_ONLY");
    if mode == "selftest" {
        assert!(producer.local_input().commit(b"TEST_ONLY", || true).is_ok());
        producer.stop();
        producer.join(Duration::from_secs(1)).unwrap();
        assert_eq!(writes.lock().unwrap().0, 1);
        return;
    }
    let control = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (signing, spki) = host_key();
    emit(
        serde_json::json!({"kind":"ready","port":listener.local_addr().unwrap().port(),"hostSpki":hex(&spki),"scope":scope,"controlPort":control.local_addr().unwrap().port()}),
    );
    let clock_start = std::time::Instant::now();
    let clock: Arc<dyn GrantClock> =
        Arc::new(move || Ok(1000 + clock_start.elapsed().as_millis() as u64));
    let mut original_host: Option<NativeHost> = None;
    let mut resource = Some(producer);
    let mut old_fence = 0;
    let mut dispatched = 0;
    for connection_index in 0..if mode == "reconnect" || mode == "unknown" {
        2
    } else {
        1
    } {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(4), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let (_cancel, token) = cancellation();
        let pending = accept(
            Unbound::new(socket),
            LocalIdentity::new(Role::Host, P256Spki::new(&spki).unwrap(), signing.clone()).unwrap(),
            PeerPin::new(Role::Client, public(2)),
            Duration::from_secs(2),
            token,
        )
        .await
        .unwrap();
        let channel = bind_host_peer_channel(
            pending,
            HostSnapshot {
                host_key_epoch: 9007199254740993,
                client_key_epoch: u64::MAX,
                activation: [0x44; 32],
                incarnation: 11,
                policy_epoch: 13,
                issuer_epoch: 17,
                origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
            },
        )
        .await
        .unwrap();
        let mut gate = GrantGate::for_binding(
            channel.context(),
            crate::bound_test_support::admin(),
            [4; 32],
            clock.clone(),
        )
        .unwrap();
        gate.require_browser(
            BrowserProfile::enrolled(
                "https://desk.invalid",
                "desk.invalid",
                vec![7; 32],
                public(3),
                false,
                false,
            )
            .unwrap(),
        )
        .unwrap();
        let mut channel = channel
            .grant_host(
                &mut gate,
                NativeGrantPolicy {
                    scope: scope.clone(),
                    role: DataRole::Controller,
                    duration: if mode == "expired" {
                        Duration::from_millis(700)
                    } else {
                        Duration::from_secs(6)
                    },
                },
                Duration::from_secs(3),
            )
            .await
            .unwrap();
        let running = Arc::new(
            channel
                .start_peer_pending(tokio::time::Instant::now() + Duration::from_secs(1))
                .unwrap(),
        );
        if let Some(host) = &mut original_host {
            let a = running.authority();
            let decode = |text: &str| -> Vec<u8> {
                (0..text.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                    .collect()
            };
            let target = crate::journal::Target {
                host: decode(&a.context().host_endpoint_id).try_into().unwrap(),
                session: decode(&scope.session).try_into().unwrap(),
                run: scope.run.clone(),
                generation: scope.generation.parse().unwrap(),
                incarnation: a.context().incarnation.parse().unwrap(),
            };
            let device = decode(&a.context().client_endpoint_id).try_into().unwrap();
            host.begin_peer_rebind(target, device, old_fence, running.clone(), [9; 16])
                .unwrap();
        } else {
            let producer = resource.take().unwrap();
            let mut host = NativeHost::for_native_resource(running.clone(), &producer).unwrap();
            host.adopt_native_producer(producer, running.clone(), Box::new(|| true))
                .unwrap_or_else(|_| panic!("original producer admission"));
            original_host = Some(host);
        }
        let host = original_host.as_mut().unwrap();
        let end = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut connection = loop {
            host.drive_peer_installs();
            if let Ok(connection) = host.peer_connection(&running) {
                break connection;
            }
            assert!(tokio::time::Instant::now() < end);
            tokio::task::yield_now().await;
        };
        emit(
            serde_json::json!({"kind":"installed","fence":running.installed_fence().unwrap().to_string(),"source":"OriginalNativeHostCoordinator"}),
        );
        old_fence = running.installed_fence().unwrap();
        while let Ok(record) = running
            .receive(&scope, old_fence, Duration::from_secs(1))
            .await
        {
            let result = connection
                .dispatch_received(&record, Duration::from_secs(1))
                .await;
            dispatched += 1;
            emit(serde_json::json!({"kind":"dispatched","count":dispatched,"ok":result.is_ok()}));
            if mode == "revoke" && dispatched == 3 {
                use tokio::io::AsyncReadExt;
                let (mut command, _) =
                    tokio::time::timeout(Duration::from_secs(2), control.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut byte = [0];
                command.read_exact(&mut byte).await.unwrap();
                assert_eq!(byte, [1]);
                gate.revoke();
                emit(serde_json::json!({"kind":"revoked"}));
            }
            if result.is_err() {
                break;
            }
        }
        drop(connection);
        running.wait_closed_checked().await.unwrap();
        emit(serde_json::json!({"kind":"disconnected","index":connection_index}));
    }
    let host = original_host.as_mut().unwrap();
    host.begin_shutdown();
    host.retire_peer_installs().await;
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    while host.observe_shutdown() == WriterRetirement::Pending {
        assert!(tokio::time::Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
    host.network_retirement()
        .unwrap()
        .wait_checked()
        .await
        .unwrap();
    let writes = writes.lock().unwrap();
    emit(
        serde_json::json!({"kind":"effects","writeCount":writes.0,"bytes":hex(&writes.1),"dispatched":dispatched}),
    );
    drop(output);
}
