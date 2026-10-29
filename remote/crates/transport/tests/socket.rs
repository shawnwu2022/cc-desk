use cc_desk_remote_protocol::{Subframe, encode_outer};
use cc_desk_remote_transport::{TransportError, bounded_tunnel, drive, websocket_config};
use futures_util::{SinkExt, StreamExt};
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream, accept_async_with_config, client_async_with_config, tungstenite::Message,
};
async fn sockets() -> (WebSocketStream<TcpStream>, WebSocketStream<TcpStream>) {
    sockets_config(websocket_config()).await
}
async fn sockets_config(
    config: tokio_tungstenite::tungstenite::protocol::WebSocketConfig,
) -> (WebSocketStream<TcpStream>, WebSocketStream<TcpStream>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let client = tokio::spawn(async move {
        client_async_with_config(
            "ws://test.invalid/",
            TcpStream::connect(addr).await.unwrap(),
            Some(websocket_config()),
        )
        .await
        .unwrap()
        .0
    });
    let (s, peer) = l.accept().await.unwrap();
    assert!(peer.ip().is_loopback());
    let server = accept_async_with_config(s, Some(config)).await.unwrap();
    (client.await.unwrap(), server)
}
#[tokio::test]
async fn drive_refuses_callers_unbounded_websocket_config() {
    let (_peer, ws) = sockets_config(Default::default()).await;
    let (_tx, rx) = watch::channel(false);
    assert_eq!(
        drive(ws, HashMap::new(), rx, Duration::from_millis(50)).await,
        Err(TransportError::Limit)
    );
}
#[tokio::test]
async fn overflowing_deadline_budget_is_rejected_without_panicking() {
    let (_peer, ws) = sockets().await;
    let (_tx, rx) = watch::channel(false);
    assert_eq!(
        drive(ws, HashMap::new(), rx, Duration::MAX).await,
        Err(TransportError::Limit)
    );
}
#[tokio::test]
async fn binary_socket_roundtrip_and_cancel_releases_tunnel() {
    let (mut peer, ws) = sockets().await;
    let (mut app, route) = bounded_tunnel();
    let (cancel, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(
        ws,
        HashMap::from([([1; 16], route)]),
        rx,
        Duration::from_secs(1),
    ));
    let b = encode_outer(&[Subframe {
        tunnel: [1; 16],
        payload: b"ciphertext",
    }])
    .unwrap();
    peer.send(Message::Binary(b.into())).await.unwrap();
    let mut buf = [0; 10];
    timeout(Duration::from_secs(2), app.read_exact(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf, b"ciphertext");
    app.write_all(b"reply").await.unwrap();
    let msg = timeout(Duration::from_secs(2), peer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let bytes = msg.into_data();
    let frames = cc_desk_remote_protocol::decode_outer(&bytes).unwrap();
    assert_eq!(frames[0].tunnel, [1; 16]);
    assert_eq!(frames[0].payload, b"reply");
    cancel.send(true).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::Cancelled)
    );
    assert_eq!(
        timeout(Duration::from_secs(1), app.read(&mut buf))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn unknown_route_never_delivers_bytes() {
    let (mut peer, ws) = sockets().await;
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(ws, HashMap::new(), rx, Duration::from_secs(1)));
    let b = encode_outer(&[Subframe {
        tunnel: [2; 16],
        payload: b"opaque",
    }])
    .unwrap();
    peer.send(Message::Binary(b.into())).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::UnknownTunnel)
    );
}
#[tokio::test]
async fn text_is_rejected_without_interpreting_terminal_content() {
    let (mut peer, ws) = sockets().await;
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(ws, HashMap::new(), rx, Duration::from_secs(1)));
    peer.send(Message::Text("not-binary".into())).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::Wire)
    );
}
#[tokio::test]
async fn stalled_tunnel_has_finite_ingress_and_fails_closed() {
    let (mut peer, ws) = sockets().await;
    let (_app, route) = bounded_tunnel();
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(
        ws,
        HashMap::from([([1; 16], route)]),
        rx,
        Duration::from_secs(2),
    ));
    let payload = vec![0; 65512];
    let b = encode_outer(&[Subframe {
        tunnel: [1; 16],
        payload: &payload,
    }])
    .unwrap();
    for _ in 0..12 {
        if peer.send(Message::Binary(b.clone().into())).await.is_err() {
            break;
        }
    }
    assert_eq!(
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::Backpressure)
    );
}
#[tokio::test]
async fn unknown_second_route_does_not_partially_deliver_first_frame() {
    let (mut peer, ws) = sockets().await;
    let (mut app, route) = bounded_tunnel();
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(
        ws,
        HashMap::from([([1; 16], route)]),
        rx,
        Duration::from_secs(1),
    ));
    let b = encode_outer(&[
        Subframe {
            tunnel: [1; 16],
            payload: b"valid",
        },
        Subframe {
            tunnel: [2; 16],
            payload: b"unknown",
        },
    ])
    .unwrap();
    peer.send(Message::Binary(b.into())).await.unwrap();
    assert_eq!(worker.await.unwrap(), Err(TransportError::UnknownTunnel));
    let mut b = [0; 1];
    assert_eq!(app.read(&mut b).await.unwrap(), 0);
}
#[tokio::test]
async fn idle_connection_deadline_releases_pending_reader() {
    let (_peer, ws) = sockets().await;
    let (mut app, route) = bounded_tunnel();
    let (_tx, rx) = watch::channel(false);
    let r = timeout(
        Duration::from_secs(1),
        drive(
            ws,
            HashMap::from([([1; 16], route)]),
            rx,
            Duration::from_millis(100),
        ),
    )
    .await
    .unwrap();
    assert_eq!(r, Err(TransportError::Deadline));
    let mut b = [0; 1];
    assert_eq!(app.read(&mut b).await.unwrap(), 0);
}
#[tokio::test]
async fn aborting_outer_driver_drops_all_owned_tunnel_workers() {
    let (mut peer, ws) = sockets().await;
    let (mut app, route) = bounded_tunnel();
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(
        ws,
        HashMap::from([([1; 16], route)]),
        rx,
        Duration::from_secs(2),
    ));
    let b = encode_outer(&[Subframe {
        tunnel: [1; 16],
        payload: b"x",
    }])
    .unwrap();
    peer.send(Message::Binary(b.into())).await.unwrap();
    let mut b = [0; 1];
    timeout(Duration::from_secs(1), app.read_exact(&mut b))
        .await
        .unwrap()
        .unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert_eq!(
        timeout(Duration::from_secs(1), app.read(&mut b))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn message_over_limit_is_rejected_by_real_websocket_parser() {
    let (mut peer, ws) = sockets().await;
    let (_tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(ws, HashMap::new(), rx, Duration::from_secs(1)));
    let _ = peer.send(Message::Binary(vec![0; 65537].into())).await;
    assert_eq!(
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::Wire)
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inbound_flood_does_not_starve_outbound_reply() {
    flood_case().await;
}
#[tokio::test]
async fn current_thread_flood_does_not_starve_route_producers() {
    flood_case().await;
}
async fn flood_case() {
    let (peer, ws) = sockets().await;
    let (mut app, route) = bounded_tunnel();
    let (tx, rx) = watch::channel(false);
    let worker = tokio::spawn(drive(
        ws,
        HashMap::from([([1; 16], route)]),
        rx,
        Duration::from_secs(2),
    ));
    let (mut sender, mut reader) = peer.split();
    let (ready, started) = tokio::sync::oneshot::channel();
    let flood = tokio::spawn(async move {
        let mut ready = Some(ready);
        let until = tokio::time::Instant::now() + Duration::from_millis(800);
        let mut sent = 0;
        while tokio::time::Instant::now() < until {
            if sender
                .send(Message::Binary(vec![1, 1, 0, 0].into()))
                .await
                .is_err()
            {
                break;
            }
            sent += 1;
            if sent == 100 {
                let _ = ready.take().unwrap().send(());
            }
        }
        sent
    });
    started.await.unwrap();
    app.write_all(b"reply").await.unwrap();
    let msg = timeout(Duration::from_millis(400), reader.next()).await;
    tx.send(true).unwrap();
    let _ = worker.await.unwrap();
    let sent = flood.await.unwrap();
    assert!(sent >= 100);
    assert!(msg.is_ok(), "outbound starved during inbound flood");
    let bytes = msg.unwrap().unwrap().unwrap().into_data();
    assert_eq!(
        cc_desk_remote_protocol::decode_outer(&bytes).unwrap()[0].payload,
        b"reply"
    );
}
