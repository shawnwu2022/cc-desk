use cc_desk_remote_transport::records::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

const BUDGET: Duration = Duration::from_secs(2);

#[tokio::test]
async fn real_tcp_fragmented_and_coalesced_records_preserve_exact_boundaries() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let mut reader = RecordReader::new(socket, 1).unwrap();
    let mut bytes = cc_desk_remote_record::encode(Kind::Control, 1, b"TEST_ONLY_A").unwrap();
    bytes.extend(cc_desk_remote_record::encode(Kind::Projection, 1, b"TEST_ONLY_B").unwrap());
    for byte in &bytes[..7] {
        peer.write_all(&[*byte]).await.unwrap();
    }
    peer.write_all(&bytes[7..]).await.unwrap();
    peer.shutdown().await.unwrap();
    let first = reader.read(BUDGET).await.unwrap().unwrap();
    let second = reader.read(BUDGET).await.unwrap().unwrap();
    assert_eq!(
        (first.kind, first.minor, first.payload),
        (Kind::Control, 1, b"TEST_ONLY_A".to_vec())
    );
    assert_eq!(
        (second.kind, second.minor, second.payload),
        (Kind::Projection, 1, b"TEST_ONLY_B".to_vec())
    );
    assert!(reader.read(BUDGET).await.unwrap().is_none());
    assert!(reader.is_closed());
}

#[tokio::test]
async fn malformed_header_rejects_without_waiting_for_attacker_payload() {
    for header in [
        [255, 255, 255, 255, 0, 2, 0, 0],
        [0, 0, 64, 5, 0, 2, 0, 0],
        [0, 0, 0, 4, 0, 2, 0, 1],
        [0, 0, 0, 4, 0, 9, 0, 0],
    ] {
        let (mut peer, io) = tokio::io::duplex(32);
        peer.write_all(&header).await.unwrap();
        let mut reader = RecordReader::new(io, 0).unwrap();
        assert!(matches!(
            timeout(Duration::from_secs(1), reader.read(BUDGET))
                .await
                .unwrap(),
            Err(RecordError::Wire(_))
        ));
        assert!(reader.is_closed());
        assert!(matches!(
            reader.read(BUDGET).await,
            Err(RecordError::Closed)
        ));
    }
}

#[tokio::test]
async fn slow_drip_uses_one_total_deadline_then_drops_stream() {
    let (mut peer, io) = tokio::io::duplex(32);
    let bytes = cc_desk_remote_record::encode(Kind::Control, 0, b"TEST_ONLY").unwrap();
    let worker = tokio::spawn(async move {
        for byte in bytes {
            if peer.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let mut reader = RecordReader::new(io, 0).unwrap();
    let result = timeout(
        Duration::from_secs(1),
        reader.read(Duration::from_millis(85)),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(RecordError::Deadline)));
    assert!(reader.is_closed());
    worker.abort();
    let _ = worker.await;
}

#[tokio::test]
async fn cancelled_partial_read_cannot_be_resumed_as_a_fresh_record() {
    let (mut peer, io) = tokio::io::duplex(32);
    peer.write_all(&[0, 0, 0]).await.unwrap();
    let mut reader = RecordReader::new(io, 0).unwrap();
    assert!(
        timeout(Duration::from_millis(20), reader.read(BUDGET))
            .await
            .is_err()
    );
    assert!(reader.is_closed());
    assert!(matches!(
        reader.read(BUDGET).await,
        Err(RecordError::Closed)
    ));
    let mut b = [0];
    assert_eq!(
        timeout(BUDGET, peer.read(&mut b)).await.unwrap().unwrap(),
        0
    );
}

#[tokio::test]
async fn truncated_record_is_an_error_and_never_returns_partial_payload() {
    for bytes in [vec![0], vec![0, 0, 0, 6, 0, 2, 0, 0, b'x']] {
        let (mut peer, io) = tokio::io::duplex(32);
        peer.write_all(&bytes).await.unwrap();
        peer.shutdown().await.unwrap();
        let mut reader = RecordReader::new(io, 0).unwrap();
        assert!(matches!(reader.read(BUDGET).await, Err(RecordError::Io)));
        assert!(reader.is_closed());
    }
}

#[tokio::test]
async fn writer_roundtrip_enforces_payload_budget_before_emitting_bytes() {
    let (io, other) = tokio::io::duplex(32776);
    let mut writer = RecordWriter::new(io, 0).unwrap();
    assert_eq!(
        writer.write(Kind::Control, &vec![0; 16385], BUDGET).await,
        Err(RecordError::Wire(cc_desk_remote_record::Error::Length))
    );
    assert!(!writer.is_closed());
    writer
        .write(Kind::Control, b"TEST_ONLY", BUDGET)
        .await
        .unwrap();
    let mut reader = RecordReader::new(other, 0).unwrap();
    assert_eq!(
        reader.read(BUDGET).await.unwrap().unwrap().payload,
        b"TEST_ONLY"
    );
}

#[tokio::test]
async fn blocked_write_deadline_and_cancellation_close_partial_streams() {
    for cancel in [false, true] {
        let (io, mut peer) = tokio::io::duplex(1);
        let mut writer = RecordWriter::new(io, 0).unwrap();
        let payload = vec![b'x'; 16384];
        if cancel {
            assert!(
                timeout(
                    Duration::from_millis(20),
                    writer.write(Kind::Control, &payload, BUDGET)
                )
                .await
                .is_err()
            );
        } else {
            assert_eq!(
                writer
                    .write(Kind::Control, &payload, Duration::from_millis(30))
                    .await,
                Err(RecordError::Deadline)
            );
        }
        assert!(writer.is_closed());
        let mut partial = Vec::new();
        timeout(BUDGET, peer.read_to_end(&mut partial))
            .await
            .unwrap()
            .unwrap();
        assert!(partial.len() < payload.len() + 8);
        assert_eq!(
            writer.write(Kind::Control, b"new", BUDGET).await,
            Err(RecordError::Closed)
        );
    }
}

#[tokio::test]
async fn invalid_budget_is_rejected_before_consuming_stream() {
    let (io, _peer) = tokio::io::duplex(1);
    let mut reader = RecordReader::new(io, 0).unwrap();
    assert!(matches!(
        reader.read(Duration::ZERO).await,
        Err(RecordError::Limit)
    ));
    assert!(!reader.is_closed());
    let (io, _peer) = tokio::io::duplex(1);
    let mut writer = RecordWriter::new(io, 0).unwrap();
    assert_eq!(
        writer.write(Kind::Control, b"", Duration::MAX).await,
        Err(RecordError::Limit)
    );
    assert!(!writer.is_closed());
}

#[tokio::test]
async fn appbind_uses_fixed_base_minor_independently_of_data_minor() {
    let (io, other) = tokio::io::duplex(32);
    let mut writer = RecordWriter::new(io, 1).unwrap();
    let mut reader = RecordReader::new(other, 1).unwrap();
    writer.write(Kind::AppBind, b"{}", BUDGET).await.unwrap();
    let record = reader.read(BUDGET).await.unwrap().unwrap();
    assert_eq!(
        (record.kind, record.minor, record.payload),
        (Kind::AppBind, 0, b"{}".to_vec())
    );
}

#[tokio::test]
async fn test_only_records_cross_actual_websocket_mux_without_message_boundary_assumptions() {
    use cc_desk_remote_protocol::{Subframe, decode_outer, encode_outer};
    use cc_desk_remote_transport::{TransportError, bounded_tunnel, drive, websocket_config};
    use futures_util::{SinkExt, StreamExt};
    use std::collections::HashMap;
    use tokio_tungstenite::{
        accept_async_with_config, client_async_with_config, tungstenite::Message,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        timeout(BUDGET, async {
            client_async_with_config(
                "ws://test.invalid/",
                tokio::net::TcpStream::connect(address).await.unwrap(),
                Some(websocket_config()),
            )
            .await
            .unwrap()
            .0
        })
        .await
        .unwrap()
    });
    let (socket, address) = timeout(BUDGET, listener.accept()).await.unwrap().unwrap();
    assert!(address.ip().is_loopback());
    let ws = timeout(
        BUDGET,
        accept_async_with_config(socket, Some(websocket_config())),
    )
    .await
    .unwrap()
    .unwrap();
    let mut peer = client.await.unwrap();
    let (app, route) = bounded_tunnel();
    let (cancel, rx) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(drive(ws, HashMap::from([([7; 16], route)]), rx, BUDGET));
    struct AbortOnDrop(tokio::task::JoinHandle<Result<(), TransportError>>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let mut worker = AbortOnDrop(worker);
    let (read, write) = tokio::io::split(app);
    let mut reader = RecordReader::new(read, 0).unwrap();
    let mut writer = RecordWriter::new(write, 0).unwrap();
    let mut bytes = cc_desk_remote_record::encode(Kind::Control, 0, b"TEST_ONLY_CONTROL").unwrap();
    bytes.extend(cc_desk_remote_record::encode(Kind::Projection, 0, &vec![b'x'; 32768]).unwrap());
    // Inner header split across outer messages; second inner record coalesced into
    // the same stream, then split at a distinct boundary. No real terminal data/TLS.
    for part in [&bytes[..1], &bytes[1..7], &bytes[7..16384], &bytes[16384..]] {
        let outer = encode_outer(&[Subframe {
            tunnel: [7; 16],
            payload: part,
        }])
        .unwrap();
        timeout(BUDGET, peer.send(Message::Binary(outer.into())))
            .await
            .unwrap()
            .unwrap();
    }
    let first = reader.read(BUDGET).await.unwrap().unwrap();
    assert_eq!(first.payload, b"TEST_ONLY_CONTROL");
    assert!(!format!("{first:?}").contains("TEST_ONLY_CONTROL"));
    assert_eq!(
        reader.read(BUDGET).await.unwrap().unwrap().payload,
        vec![b'x'; 32768]
    );
    writer
        .write(Kind::Control, b"TEST_ONLY_REPLY", BUDGET)
        .await
        .unwrap();
    let message = timeout(BUDGET, peer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    let outer = decode_outer(&message).unwrap();
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].tunnel, [7; 16]);
    assert_eq!(
        outer[0].payload,
        cc_desk_remote_record::encode(Kind::Control, 0, b"TEST_ONLY_REPLY").unwrap()
    );
    cancel.send(true).unwrap();
    assert_eq!(
        timeout(BUDGET, &mut worker.0).await.unwrap().unwrap(),
        Err(TransportError::Cancelled)
    );
    assert!(reader.read(BUDGET).await.unwrap().is_none());
}
