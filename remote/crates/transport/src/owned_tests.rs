use super::*;
#[tokio::test]
async fn mux_owner_cancelled_observer_retains_original_driver_until_join() {
    let (socket, peer) = tokio::io::duplex(65536);
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
        socket,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        Some(websocket_config()),
    )
    .await;
    let (_stream, owner) = start_owned(ws, [9; 16], std::time::Duration::from_secs(1)).unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(5), owner.wait_closed())
            .await
            .is_err()
    );
    assert!(owner.join.lock().await.is_some());
    owner.request_close();
    assert!(owner.wait_closed().await.is_ok());
    assert!(owner.join.lock().await.is_none());
    assert!(owner.wait_closed().await.is_ok());
    drop(peer);
}
