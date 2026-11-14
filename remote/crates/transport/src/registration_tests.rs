#![allow(non_snake_case)]
use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

async fn sockets() -> (
    tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) {
    let (a, b) = tokio::io::duplex(65536);
    use tokio_tungstenite::tungstenite::protocol::Role;
    (
        tokio_tungstenite::WebSocketStream::from_raw_socket(
            a,
            Role::Client,
            Some(websocket_config()),
        )
        .await,
        tokio_tungstenite::WebSocketStream::from_raw_socket(
            b,
            Role::Server,
            Some(websocket_config()),
        )
        .await,
    )
}
async fn tick(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_KeepaliveIsNotPayloadAndTotalNeverRenews_001() {
    let (a, b) = sockets().await;
    let end = Instant::now() + Duration::from_secs(100);
    let budget = Duration::from_secs(30);
    let (mut app_a, owner_a) =
        start_owned_registration(a, [9; 16], budget, end, std::future::pending())
            .expect("registration owner");
    let (mut app_b, owner_b) =
        start_owned_registration(b, [9; 16], budget, end, std::future::pending()).unwrap();
    for _ in 0..8 {
        tick(5000).await;
    }
    let mut byte = [0];
    {
        let read = app_b.read(&mut byte);
        tokio::pin!(read);
        assert!(
            futures_util::poll!(read).is_pending(),
            "empty keepalive must not reach TLS IO"
        );
    }
    app_a.write_all(b"proof").await.unwrap();
    let mut got = [0; 5];
    app_b.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"proof");
    tokio::time::sleep_until(end).await;
    owner_a.wait_closed().await.unwrap();
    owner_b.wait_closed().await.unwrap();
    assert_eq!(
        Instant::now(),
        end,
        "keepalive cannot extend the original total deadline"
    );
    assert_eq!(app_a.read(&mut byte).await.unwrap(), 0);
    assert_eq!(app_b.read(&mut byte).await.unwrap(), 0);
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_OutboundNeverRenewsInboundIdle_002() {
    use futures_util::StreamExt;
    let (a, mut peer) = sockets().await;
    let start = Instant::now();
    let budget = Duration::from_secs(3);
    let (_app, owner) = start_owned_registration(
        a,
        [9; 16],
        budget,
        start + Duration::from_secs(20),
        std::future::pending(),
    )
    .unwrap();
    let reader = tokio::spawn(async move {
        let mut count = 0;
        while let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b))) = peer.next().await {
            let frames = cc_desk_remote_protocol::decode_outer(&b).unwrap();
            assert_eq!(frames.len(), 1);
            assert!(frames[0].payload.is_empty());
            count += 1;
        }
        count
    });
    owner.wait_closed().await.unwrap();
    assert_eq!(Instant::now(), start + budget);
    assert!(reader.await.unwrap() > 0);
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_OriginalCancelBeforeIoRetires_003() {
    use futures_util::StreamExt;
    let (a, mut peer) = sockets().await;
    let start = Instant::now();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tx.send(()).unwrap();
    let (_app, owner) = start_owned_registration(
        a,
        [9; 16],
        Duration::from_secs(3),
        start + Duration::from_secs(20),
        async {
            let _ = rx.await;
        },
    )
    .unwrap();
    owner.wait_closed().await.unwrap();
    assert_eq!(Instant::now(), start);
    assert!(!matches!(
        peer.next().await,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_)))
    ));
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_OriginalCancelClosesBlockedWrite_004() {
    let (a, _peer) = sockets().await;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let (mut app, owner) = start_owned_registration(
        a,
        [9; 16],
        Duration::from_secs(3),
        Instant::now() + Duration::from_secs(20),
        async {
            let _ = rx.await;
        },
    )
    .unwrap();
    let writer = tokio::spawn(async move { app.write_all(&vec![5; 512 * 1024]).await });
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
    assert!(!writer.is_finished());
    tx.send(()).unwrap();
    owner.wait_closed().await.unwrap();
    assert!(writer.await.unwrap().is_err());
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_InvalidLifetimeAndTinyInterval_005() {
    for (budget, total) in [
        (Duration::ZERO, Duration::from_secs(20)),
        (Duration::from_nanos(1), Duration::from_secs(20)),
        (Duration::from_secs(31), Duration::from_secs(20)),
        (Duration::from_secs(1), Duration::ZERO),
        (Duration::from_secs(1), Duration::from_secs(121)),
    ] {
        let (a, _peer) = sockets().await;
        assert!(matches!(
            start_owned_registration(
                a,
                [9; 16],
                budget,
                Instant::now() + total,
                std::future::pending()
            ),
            Err(TransportError::Limit)
        ));
    }
}
#[tokio::test(start_paused = true)]
async fn RegistrationRelay_LegacyOwnerNeverSendsHeartbeat_006() {
    use futures_util::StreamExt;
    let (a, mut peer) = sockets().await;
    let start = Instant::now();
    let budget = Duration::from_secs(3);
    let (_app, owner) = start_owned(a, [9; 16], budget).unwrap();
    owner.wait_closed().await.unwrap();
    assert_eq!(Instant::now(), start + budget);
    assert!(!matches!(
        peer.next().await,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_)))
    ));
}
