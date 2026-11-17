use super::*;
use std::sync::atomic::Ordering;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Meter {
    bytes: AtomicU64,
    records: AtomicU64,
    fragments: AtomicU64,
}
async fn forward(
    mut reader: tokio::io::ReadHalf<tokio::io::DuplexStream>,
    mut writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    meter: Option<Arc<Meter>>,
    fragment: usize,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 16384];
    let mut header = [0u8; 5];
    let mut header_used = 0;
    let mut remaining = 0;
    loop {
        let n = reader.read(&mut buffer[..fragment]).await?;
        if n == 0 {
            return writer.shutdown().await;
        }
        if let Some(m) = &meter {
            for byte in &buffer[..n] {
                if remaining > 0 {
                    remaining -= 1;
                    if remaining == 0 {
                        m.records.fetch_add(1, Ordering::AcqRel);
                    }
                } else {
                    header[header_used] = *byte;
                    header_used += 1;
                    if header_used == 5 {
                        remaining = u16::from_be_bytes([header[3], header[4]]) as usize;
                        assert!(remaining > 0);
                        header_used = 0;
                    }
                }
            }
            m.bytes.fetch_add(n as u64, Ordering::AcqRel);
            m.fragments.fetch_add(1, Ordering::AcqRel);
        }
        writer.write_all(&buffer[..n]).await?;
    }
}

#[tokio::test]
async fn snapshot_pull_max_batch_real_tls_ciphertext_and_fragmentation() {
    use cc_desk_remote_record::snapshot_pull as pull;
    for fragment in [16384, 257] {
        let (c, proxy_c) = tokio::io::duplex(65536);
        let (proxy_h, h) = tokio::io::duplex(65536);
        let (cr, cw) = tokio::io::split(proxy_c);
        let (hr, hw) = tokio::io::split(proxy_h);
        let meter = Arc::new(Meter::default());
        let read_meter = meter.clone();
        let proxy = tokio::spawn(async move {
            tokio::try_join!(
                forward(cr, hw, None, 16384),
                forward(hr, cw, Some(read_meter), fragment)
            )
        });
        let proxy = Proxy(proxy);
        let (mut c, mut h, _gate, _cancel) = managed_channels_io(
            Arc::new(AtomicU64::new(1000)),
            DataRole::Controller,
            false,
            true,
            c,
            h,
        )
        .await;
        let before_install = meter.bytes.load(Ordering::Acquire);
        let end = tokio::time::Instant::now() + BUDGET;
        let c = c.start_peer_pending(end).unwrap();
        let host = h.start_peer_pending(end).unwrap();
        let request = host.host_install_request().await.unwrap();
        let (proof, installed) =
            tokio::join!(c.client_installed(), host.complete_host_install(request, 4));
        installed.unwrap();
        let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&projection(&client)).unwrap();
        value["body"]["state"]["rows"] = 1.into();
        value["body"]["state"]["columns"] = 2.into();
        let cell = value["body"]["cells"][0].clone();
        value["body"]["cells"] = serde_json::json!([cell]);
        let baseline = meter.bytes.load(Ordering::Acquire);
        let records_before = meter.records.load(Ordering::Acquire);
        println!(
            "fragment={fragment} handshake/grant ciphertext={before_install} Installed phase ciphertext={}",
            baseline - before_install
        );
        let server = async {
            let begin = host.receive(&support::scope(), 4, BUDGET).await.unwrap();
            assert!(matches!(
                pull::decode(begin.payload()),
                Ok(pull::Message::SnapshotBegin { .. })
            ));
            drop(begin);
            let mut previous = baseline;
            for index in 0..2 {
                value["body"]["start"] = index.into();
                let mut part = serde_json::to_vec(&value).unwrap();
                part.resize(pull::MAX_PART, b' ');
                let batch = pull::encode_batch(&pull::Batch {
                    request: 1,
                    transfer: [7; 32],
                    batch: index,
                    through: index + 1,
                    last: index == 1,
                    part: &part,
                })
                .unwrap();
                assert_eq!(batch.len(), pull::MAX_PAYLOAD);
                host.send_projection(&support::scope(), 4, &batch, BUDGET)
                    .await
                    .unwrap();
                if index == 0 {
                    let next = host.receive(&support::scope(), 4, BUDGET).await.unwrap();
                    assert!(matches!(
                        pull::decode(next.payload()),
                        Ok(pull::Message::SnapshotNext { .. })
                    ));
                    let current = meter.bytes.load(Ordering::Acquire);
                    let ciphertext = current - previous;
                    assert!(ciphertext > 32768 + 8 && ciphertext <= 65536);
                    assert_eq!(ciphertext, 32842);
                    assert_eq!(meter.records.load(Ordering::Acquire) - records_before, 3);
                    println!("fragment={fragment} first Batch ciphertext={ciphertext}");
                    previous = current;
                }
            }
            previous
        };
        let (snapshot, second_start) = tokio::join!(client.snapshot_pull(1, BUDGET), server);
        assert_eq!(snapshot.unwrap().parts.len(), 2);
        let second = meter.bytes.load(Ordering::Acquire) - second_start;
        assert!(second > 32768 + 8 && second <= 65536);
        assert_eq!(second, 32842);
        assert_eq!(meter.records.load(Ordering::Acquire) - records_before, 6);
        println!(
            "fragment={fragment} second Batch ciphertext={second} TLS records={} total transport reads={}",
            meter.records.load(Ordering::Acquire) - records_before,
            meter.fragments.load(Ordering::Acquire)
        );
        assert!(client.retire().await.is_ok());
        host.close().await;
        assert!(host.wait_closed_checked().await.is_ok());
        join_proxy(proxy, BUDGET).await.unwrap();
    }
}

struct Proxy(tokio::task::JoinHandle<std::io::Result<((), ())>>);
impl Drop for Proxy {
    fn drop(&mut self) {
        // 意外 panic/取消时至少请求停止；正常和超时路径都显式等待 join。
        self.0.abort();
    }
}
async fn join_proxy(mut proxy: Proxy, budget: Duration) -> Result<(), &'static str> {
    match tokio::time::timeout(budget, &mut proxy.0).await {
        Ok(result) => {
            result
                .map_err(|_| "proxy task failed")?
                .map_err(|_| "proxy IO failed")?;
            Ok(())
        }
        Err(_) => {
            proxy.0.abort();
            let _joined = (&mut proxy.0).await;
            Err("proxy timeout after joined cancellation")
        }
    }
}
#[tokio::test]
async fn snapshot_pull_probe_timeout_joins_cancelled_proxy() {
    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = dropped.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let proxy = tokio::spawn(async move {
        let _guard = Dropped(flag);
        ready.send(()).unwrap();
        std::future::pending::<()>().await;
        Ok(((), ()))
    });
    started.await.unwrap();
    assert!(
        join_proxy(Proxy(proxy), Duration::from_millis(1))
            .await
            .is_err()
    );
    assert!(
        dropped.load(Ordering::Acquire),
        "timed-out proxy must already be dropped/joined"
    );
}
