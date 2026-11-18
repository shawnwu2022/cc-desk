use super::*;

#[tokio::test]
async fn snapshot_pull_raw_part_budget_exact_and_one_over_real_channel() {
    use cc_desk_remote_record::snapshot_pull as pull;
    for excess in [0usize, 1] {
        let (mut c, mut h, _gate, _cancel) = managed_channels_mode(
            Arc::new(AtomicU64::new(1000)),
            DataRole::Controller,
            false,
            true,
        )
        .await;
        let end = tokio::time::Instant::now() + BUDGET;
        let c = c.start_peer_pending(end).unwrap();
        let host = h.start_peer_pending(end).unwrap();
        let request = host.host_install_request().await.unwrap();
        let (proof, installed) =
            tokio::join!(c.client_installed(), host.complete_host_install(request, 4));
        installed.unwrap();
        let client = NativeClient::from_installed(c, proof.unwrap()).unwrap();
        let count = MAX_SNAPSHOT_BYTES.div_ceil(pull::MAX_PART);
        let mut value: serde_json::Value = serde_json::from_slice(&projection(&client)).unwrap();
        value["body"]["state"]["rows"] = 1.into();
        value["body"]["state"]["columns"] = count.into();
        let cell = value["body"]["cells"][0].clone();
        value["body"]["cells"] = serde_json::json!([cell]);
        let server = async {
            let begin = host.receive(&support::scope(), 4, BUDGET).await.unwrap();
            assert!(matches!(
                pull::decode(begin.payload()),
                Ok(pull::Message::SnapshotBegin { .. })
            ));
            drop(begin);
            let mut raw_total = 0;
            let mut batch_payload_total = 0;
            for index in 0..count {
                value["body"]["start"] = index.into();
                let mut part = serde_json::to_vec(&value).unwrap();
                let size = if index + 1 == count {
                    MAX_SNAPSHOT_BYTES - raw_total + excess
                } else {
                    pull::MAX_PART
                };
                assert!(part.len() <= size && size <= pull::MAX_PART);
                // 合法 JSON 空白只用于精确触及接收累计边界，不冒充 Host encoder 输出。
                part.resize(size, b' ');
                raw_total += part.len();
                let bytes = pull::encode_batch(&pull::Batch {
                    request: 1,
                    transfer: [7; 32],
                    batch: index as u32,
                    through: index as u32 + 1,
                    last: index + 1 == count,
                    part: &part,
                })
                .unwrap();
                pull::decode_validated_batch(&bytes).unwrap();
                batch_payload_total += bytes.len();
                host.send_projection(&support::scope(), 4, &bytes, BUDGET)
                    .await
                    .unwrap();
                if index + 1 != count {
                    let next = host.receive(&support::scope(), 4, BUDGET).await.unwrap();
                    let pull::Message::SnapshotNext { batch, through, .. } =
                        pull::decode(next.payload()).unwrap()
                    else {
                        panic!()
                    };
                    assert_eq!(batch, (index + 1).to_string());
                    assert_eq!(through, batch);
                }
            }
            assert_eq!(raw_total, MAX_SNAPSHOT_BYTES + excess);
            assert_eq!(batch_payload_total, raw_total + count * 57);
            assert!(batch_payload_total > MAX_SNAPSHOT_BYTES);
        };
        let (result, ()) = tokio::join!(client.snapshot_pull(1, BUDGET), server);
        if excess == 0 {
            let snapshot = result.unwrap();
            assert_eq!(snapshot.parts.len(), count);
            assert_eq!(
                snapshot.parts.iter().map(Vec::len).sum::<usize>(),
                MAX_SNAPSHOT_BYTES
            );
        } else {
            assert!(matches!(result, Err(DataError::Bounds)));
            assert!(client.channel.authority().check().is_err());
        }
        assert!(client.retire().await.is_ok());
        host.close().await;
        assert!(host.wait_closed_checked().await.is_ok());
    }
}
