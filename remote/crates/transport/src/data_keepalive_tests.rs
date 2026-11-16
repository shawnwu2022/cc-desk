#![allow(non_snake_case)]
use super::*;
use cc_desk_remote_protocol::{Subframe, encode_outer};
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
async fn DataKeepalive_ActiveBothSidesNoTlsAndFixedEnd_002() {
    let (a, b) = sockets().await;
    let now = Instant::now();
    let end = now + Duration::from_secs(5);
    let (mut app, a, activate_a) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(2),
        now + Duration::from_secs(1),
        end,
        std::future::pending(),
    )
    .unwrap();
    let (_other, b, activate_b) = start_data_keepalive(
        b,
        [9; 16],
        Duration::from_secs(2),
        now + Duration::from_secs(1),
        end,
        std::future::pending(),
    )
    .unwrap();
    activate_a.activate(end).unwrap();
    activate_b.activate(end).unwrap();
    tick(0).await;
    for _ in 0..8 {
        tick(500).await;
        assert!(
            a.join
                .lock()
                .await
                .as_ref()
                .is_some_and(|j| !j.is_finished())
        );
    }
    let mut byte = [0];
    assert!(
        tokio::time::timeout(Duration::from_millis(1), app.read(&mut byte))
            .await
            .is_err()
    );
    tick(1000).await;
    let (x, y) = tokio::join!(a.wait_closed(), b.wait_closed());
    assert!(x.is_ok() && y.is_ok());
}
#[tokio::test(start_paused = true)]
async fn DataKeepalive_DormantLateClosedAndOversizeRefuse_003() {
    for variant in 0..3 {
        let (a, _b) = sockets().await;
        let now = Instant::now();
        let (_app, owner, activation) = start_data_keepalive(
            a,
            [9; 16],
            Duration::from_secs(2),
            now + Duration::from_millis(200),
            now + Duration::from_secs(3),
            std::future::pending(),
        )
        .unwrap();
        if variant == 0 {
            tick(200).await;
        } else if variant == 1 {
            owner.request_close();
        }
        let end = now + Duration::from_secs(if variant == 2 { 4 } else { 2 });
        assert!(activation.activate(end).is_err());
        owner.request_close();
        owner.wait_closed().await.unwrap();
    }
    let (a, _b) = sockets().await;
    let now = Instant::now();
    assert!(
        start_data_keepalive(
            a,
            [9; 16],
            Duration::from_millis(1999),
            now + Duration::from_secs(1),
            now + Duration::from_secs(2),
            std::future::pending()
        )
        .is_err()
    );
}
#[tokio::test(start_paused = true)]
async fn DataKeepalive_FixedEndJoinsBlockedWriter_004() {
    let (a, _peer) = sockets().await;
    let now = Instant::now();
    let end = now + Duration::from_secs(1);
    let (mut app, owner, activation) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(30),
        end,
        end,
        std::future::pending(),
    )
    .unwrap();
    activation.activate(end).unwrap();
    let writer = tokio::spawn(async move { app.write_all(&vec![5; 512 * 1024]).await });
    tick(0).await;
    assert!(!writer.is_finished());
    tick(1000).await;
    owner.wait_closed().await.unwrap();
    assert!(writer.await.unwrap().is_err());
    assert_eq!(Instant::now(), end);
}
#[tokio::test(start_paused = true)]
async fn DataKeepalive_CancelledObservationRetainsOriginalJoin_005() {
    let (a, _b) = sockets().await;
    let now = Instant::now();
    let (_app, owner, activation) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(2),
        now + Duration::from_secs(1),
        now + Duration::from_secs(3),
        std::future::pending(),
    )
    .unwrap();
    activation.activate(now + Duration::from_secs(3)).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(1), owner.wait_closed())
            .await
            .is_err()
    );
    assert!(owner.join.lock().await.is_some());
    owner.request_close();
    owner.wait_closed().await.unwrap();
    assert_eq!(owner.joined.load(std::sync::atomic::Ordering::Acquire), 1);
}
#[tokio::test(start_paused = true)]
async fn DataKeepalive_OriginalCancellationRetiresDormantAndClosesHandle_006() {
    let (a, _b) = sockets().await;
    let now = Instant::now();
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let (_app, owner, activation) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(2),
        now + Duration::from_secs(1),
        now + Duration::from_secs(3),
        async {
            let _ = cancelled.await;
        },
    )
    .unwrap();
    cancel.send(()).unwrap();
    owner.wait_closed().await.unwrap();
    assert!(activation.activate(now + Duration::from_secs(2)).is_err());
}
#[tokio::test(start_paused = true)]
async fn DataKeepalive_EmptyHasExactTunnelAndNoEchoStorm_007() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (a, mut peer) = sockets().await;
    let now = Instant::now();
    let end = now + Duration::from_secs(3);
    let (_app, owner, activation) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(2),
        now + Duration::from_secs(1),
        end,
        std::future::pending(),
    )
    .unwrap();
    activation.activate(end).unwrap();
    tick(0).await;
    peer.send(Message::Binary(
        encode_outer(&[Subframe {
            tunnel: [9; 16],
            payload: &[],
        }])
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tick(0).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(1), peer.next())
            .await
            .is_err()
    );
    tick(500).await;
    let Message::Binary(bytes) = peer.next().await.unwrap().unwrap() else {
        panic!("binary")
    };
    assert!(!bytes.is_empty());
    let frames = cc_desk_remote_protocol::decode_outer(&bytes).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].tunnel, [9; 16]);
    assert!(frames[0].payload.is_empty());
    owner.request_close();
    owner.wait_closed().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn DataKeepalive_DormantBlockedSendActivationTightensOriginalFlush_008() {
    let (a, _peer) = sockets().await;
    let now = Instant::now();
    let end = now + Duration::from_secs(1);
    let (mut app, owner, activation) = start_data_keepalive(
        a,
        [9; 16],
        Duration::from_secs(30),
        now + Duration::from_secs(5),
        now + Duration::from_secs(6),
        std::future::pending(),
    )
    .unwrap();
    let writer = tokio::spawn(async move { app.write_all(&vec![5; 512 * 1024]).await });
    tick(0).await;
    assert!(!writer.is_finished());
    activation.activate(end).unwrap();
    tick(1000).await;
    assert!(
        owner.join.lock().await.as_ref().unwrap().is_finished(),
        "activation must tighten the already-pending original send/flush"
    );
    owner.wait_closed().await.unwrap();
    assert!(writer.await.unwrap().is_err());
    assert_eq!(Instant::now(), end);
}
