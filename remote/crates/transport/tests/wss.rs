use cc_desk_remote_transport::wss::{ConnectError, connect};
use std::time::Duration;
#[tokio::test]
async fn unsafe_origins_reject_before_network_access() {
    for origin in [
        "ws://localhost/",
        "https://localhost/",
        "wss://u:p@localhost/",
        "wss://localhost/?token=secret",
        "wss://localhost/#fragment",
    ] {
        assert!(matches!(
            connect(
                origin,
                "127.0.0.1:443".parse().unwrap(),
                Duration::from_secs(1)
            )
            .await,
            Err(ConnectError::Origin)
        ));
    }
}
#[tokio::test]
async fn unbounded_budget_rejects_before_network_access() {
    assert!(matches!(
        connect(
            "wss://localhost/",
            "127.0.0.1:443".parse().unwrap(),
            Duration::MAX
        )
        .await,
        Err(ConnectError::Limit)
    ));
}
#[tokio::test]
async fn invalid_tls_name_cannot_cause_a_tcp_connection() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let origin = format!("wss://foo-.com:{}/", addr.port());
    assert!(matches!(
        connect(&origin, addr, Duration::from_millis(200)).await,
        Err(ConnectError::Origin)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), l.accept())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn silent_tls_peer_uses_one_absolute_setup_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("wss://localhost:{}/", address.port());
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    let start = std::time::Instant::now();
    assert!(matches!(
        connect(&url, address, Duration::from_millis(100)).await,
        Err(ConnectError::Deadline)
    ));
    assert!(start.elapsed() < Duration::from_millis(250));
    server.abort();
    let _ = server.await;
}
