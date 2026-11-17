use super::*;
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{Request, Response},
};
async fn rendezvous_trial(mode: u8, budget: Duration) -> Result<AdmittedSocket, ConnectError> {
    rendezvous_observed_trial(mode, budget, None).await
}
async fn rendezvous_observed_trial(
    mode: u8,
    budget: Duration,
    wait: Option<&RendezvousWait>,
) -> Result<AdmittedSocket, ConnectError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("wss://foobar.com:{}/relay", address.port());
    let server_endpoint = endpoint.clone();
    let server = tokio::spawn(async move {
        let socket = listener.accept().await.unwrap().0;
        let tls = tls().accept(socket).await.unwrap();
        #[allow(clippy::result_large_err)]
        let callback = move |req: &Request, mut response: Response| {
            assert_eq!(
                req.headers()
                    .get("sec-websocket-protocol")
                    .map(|h| h.to_str().unwrap()),
                Some("cc-desk-relay-rendezvous-v1")
            );
            if mode != 1 {
                response.headers_mut().insert(
                    "sec-websocket-protocol",
                    if mode == 2 {
                        "unknown"
                    } else if mode == 8 {
                        "cc-desk-relay-rendezvous-v1, unknown"
                    } else {
                        "cc-desk-relay-rendezvous-v1"
                    }
                    .parse()
                    .unwrap(),
                );
            }
            if mode == 7 {
                response.headers_mut().append(
                    "sec-websocket-protocol",
                    "cc-desk-relay-rendezvous-v1".parse().unwrap(),
                );
            }
            Ok(response)
        };
        let Ok(mut ws) = tokio_tungstenite::accept_hdr_async_with_config(
            tls,
            callback,
            Some(crate::websocket_config()),
        )
        .await
        else {
            return;
        };
        let challenge = [&[1, 0][..], &[3; 32], &[4; 32]].concat();
        if ws.send(Message::Binary(challenge.into())).await.is_err() {
            return;
        }
        let Some(Ok(Message::Binary(proof))) = ws.next().await else {
            return;
        };
        let mut transcript = b"CC_DESK_RELAY_ADMISSION\0\x01".to_vec();
        transcript.extend_from_slice(&(server_endpoint.len() as u16).to_be_bytes());
        transcript.extend_from_slice(server_endpoint.as_bytes());
        transcript.extend_from_slice(&[3; 32]);
        transcript.extend_from_slice(&[4; 32]);
        transcript.extend_from_slice(&[1; 16]);
        transcript.push(1);
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point())
            .verify(&transcript, &proof[19..])
            .unwrap();
        ws.send(Message::Binary(vec![1, 2].into())).await.unwrap();
        let Some(Ok(Message::Binary(join))) = ws.next().await else {
            return;
        };
        assert_eq!(join.as_ref(), [&[2, 0][..], &[9; 16]].concat());
        if mode == 5 {
            return;
        }
        if mode == 6 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let mut ready = if mode == 9 {
            vec![1, 1, 0, 1]
        } else if mode == 4 {
            vec![1, 2]
        } else {
            [&[2, 1][..], &[if mode == 3 { 8 } else { 9 }; 16]].concat()
        };
        if mode == 11 {
            ready.push(0);
        }
        let message = if mode == 10 {
            Message::Text("Ready".into())
        } else {
            Message::Binary(ready.into())
        };
        let _ = ws.send(message).await;
    });
    let result = if let Some(wait) = wait {
        test_only_connect_admitted_loopback_observed(
            &endpoint,
            address,
            budget,
            &identity(),
            RelayMode::RendezvousV1,
            [9; 16],
            wait,
        )
        .await
    } else {
        test_only_connect_admitted_loopback_with_mode(
            &endpoint,
            address,
            budget,
            &identity(),
            RelayMode::RendezvousV1,
            [9; 16],
        )
        .await
    };
    server.await.unwrap();
    result
}
#[tokio::test]
#[allow(non_snake_case)]
async fn Rendezvous_OriginalAdmissionThenExactJoinReady_001() {
    assert!(rendezvous_trial(0, Duration::from_secs(2)).await.is_ok());
}
#[tokio::test]
#[allow(non_snake_case)]
async fn Rendezvous_NoDowngradeOrWrongReady_002() {
    for mode in [1, 2, 3, 4, 5, 7, 8, 9, 10, 11] {
        assert!(
            rendezvous_trial(mode, Duration::from_secs(2))
                .await
                .is_err()
        );
    }
}
#[tokio::test]
#[allow(non_snake_case)]
async fn Rendezvous_OriginalDeadlineIncludesPeerWait_003() {
    assert!(matches!(
        rendezvous_trial(6, Duration::from_millis(100)).await,
        Err(ConnectError::Deadline)
    ));
}

#[tokio::test]
async fn rendezvous_observed_real_tls_never_reclassifies_eof_or_bad_ready() {
    for mode in [0, 3, 5, 6] {
        let budget = Duration::from_millis(100);
        let end = Instant::now() + budget;
        let wait = RendezvousWait::new(end);
        let result = rendezvous_observed_trial(mode, budget, Some(&wait)).await;
        match mode {
            0 => assert!(result.is_ok()),
            6 => assert!(matches!(result, Err(ConnectError::Deadline))),
            _ => assert!(matches!(result, Err(ConnectError::Admission))),
        }
        drop(result);
        tokio::time::sleep_until(end).await;
        assert_eq!(wait.take_expired().is_some(), mode == 6);
    }
}
