use super::*;
async fn wire_record(
    c: &cc_desk_remote_security::RunningChannel,
    h: &cc_desk_remote_security::RunningChannel,
    fence: u64,
    b: &[u8],
) -> cc_desk_remote_security::DataRecord {
    c.send_control(&support::scope(), fence, b, Duration::from_secs(1))
        .await
        .unwrap();
    h.receive(&support::scope(), fence, Duration::from_secs(1))
        .await
        .unwrap()
}
pub(super) struct Flow<'a> {
    pub c: &'a cc_desk_remote_security::RunningChannel,
    pub h: &'a cc_desk_remote_security::RunningChannel,
    pub writes: &'a Arc<AtomicUsize>,
    pub now: &'a Arc<AtomicU64>,
    pub gate: &'a mut cc_desk_remote_security::GrantGate,
    pub output: &'a mut HostOutputHandle,
    pub entered: &'a mpsc::Receiver<()>,
    pub release: &'a mpsc::SyncSender<()>,
    pub grant: &'a cc_desk_remote_security::VerifiedGrant,
    pub stale: &'a cc_desk_remote_record::projection::Stamp,
}
pub(super) async fn connection_flow(conn: &mut connection::Connection<'_>, f: Flow<'_>, mode: u8) {
    let (c, h, writes) = (f.c, f.h, f.writes);
    let early = wire_record(
        c,
        h,
        5,
        br#"{"message":"Window","profile":1,"request":"1"}"#,
    )
    .await;
    assert_eq!(
        conn.window(&early, Duration::from_secs(1)).await,
        Err(Reject::Sync)
    );
    drop(early);
    let stale = cc_desk_remote_record::projection::encode_ack(
        &cc_desk_remote_record::projection::Ack {
            profile: 1,
            stamp: f.stale.clone(),
        },
        1,
    )
    .unwrap();
    let old = wire_record(c, h, 5, &stale).await;
    assert_eq!(
        conn.acknowledge(&old, Duration::from_secs(1)).await,
        Err(Reject::Sync)
    );
    drop(old);
    let fenced = wire_record(c, h, 4, &stale).await;
    assert_eq!(
        conn.acknowledge(&fenced, Duration::from_secs(1)).await,
        Err(Reject::Target)
    );
    drop(fenced);
    let (mut other_c, mut other_h, _cancel) = support::pair().await;
    other_c.install(f.grant).unwrap();
    other_h.install(f.grant).unwrap();
    let other_c = other_c.start().unwrap();
    let other_h = other_h.start().unwrap();
    let foreign = wire_record(&other_c, &other_h, 5, &stale).await;
    assert_eq!(
        conn.acknowledge(&foreign, Duration::from_secs(1)).await,
        Err(Reject::Target)
    );
    drop(foreign);
    other_h.close().await;
    other_c.close().await;
    let request = wire_record(
        c,
        h,
        5,
        br#"{"message":"Snapshot","profile":1,"request":"2"}"#,
    )
    .await;
    if (9..=12).contains(&mode) {
        let window = wire_record(
            c,
            h,
            5,
            br#"{"message":"Window","profile":1,"request":"3"}"#,
        )
        .await;
        if mode == 10 {
            assert!(
                conn.snapshot(&request, h, Duration::from_millis(50))
                    .await
                    .is_err()
            );
        } else {
            let mut sending = Box::pin(conn.snapshot(&request, h, Duration::from_secs(1)));
            let scope = support::scope();
            let rec = tokio::select! {v=c.receive(&scope,5,Duration::from_secs(1))=>v.unwrap(),v=&mut sending=>panic!("snapshot ended before partial observation: {v:?}")};
            let msg = cc_desk_remote_record::projection::decode(rec.payload(), 1).unwrap();
            let cc_desk_remote_record::projection::Content::SnapshotPart(p) = msg.content else {
                panic!("part")
            };
            assert!(p.cells.len() < usize::from(p.state.rows) * usize::from(p.state.columns));
            let early = cc_desk_remote_record::projection::encode_ack(
                &cc_desk_remote_record::projection::Ack {
                    profile: 1,
                    stamp: p.stamp,
                },
                1,
            )
            .unwrap();
            drop(rec);
            if mode == 9 {
                let ack = wire_record(c, h, 5, &early).await;
                drop(sending);
                assert!(
                    conn.acknowledge(&ack, Duration::from_secs(1))
                        .await
                        .is_err()
                );
            } else {
                if mode == 11 {
                    f.now.store(11000, Ordering::Release);
                } else {
                    f.gate.revoke();
                }
                assert!(sending.await.is_err());
            }
        }
        assert!(conn.window(&window, Duration::from_secs(1)).await.is_err());
        assert_eq!(writes.load(Ordering::Acquire), 0);
        return;
    }
    let received = async {
        let mut end = 0;
        let mut stamp = None;
        loop {
            let rec = c
                .receive(&support::scope(), 5, Duration::from_secs(1))
                .await
                .unwrap();
            let msg = cc_desk_remote_record::projection::decode(rec.payload(), 1).unwrap();
            let cc_desk_remote_record::projection::Content::SnapshotPart(p) = msg.content else {
                panic!("snapshot")
            };
            assert_eq!(p.start as usize, end);
            end += p.cells.len();
            if let Some(s) = &stamp {
                assert!(*s == p.stamp)
            }
            stamp = Some(p.stamp);
            if end == usize::from(p.state.rows) * usize::from(p.state.columns) {
                break;
            }
        }
        cc_desk_remote_record::projection::encode_ack(
            &cc_desk_remote_record::projection::Ack {
                profile: 1,
                stamp: stamp.unwrap(),
            },
            1,
        )
        .unwrap()
    };
    let (sent, ack) = tokio::join!(conn.snapshot(&request, h, Duration::from_secs(1)), received);
    sent.unwrap();
    drop(request);
    let ack = wire_record(c, h, 5, &ack).await;
    if mode == 13 {
        let blocked = f.output.feed_at(0, b"\x1b[5n".to_vec()).unwrap();
        f.entered.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut pending = Box::pin(conn.acknowledge(&ack, Duration::from_secs(1)));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(pending);
        f.release.send(()).unwrap();
        blocked.wait(Duration::from_secs(1)).unwrap();
        let window = wire_record(
            c,
            h,
            5,
            br#"{"message":"Window","profile":1,"request":"3"}"#,
        )
        .await;
        assert_eq!(
            conn.window(&window, Duration::from_secs(1)).await,
            Err(Reject::Pending)
        );
        assert_eq!(writes.load(Ordering::Acquire), 1);
        return;
    }
    let mut changed = cc_desk_remote_record::projection::decode_ack(ack.payload(), 1).unwrap();
    changed.stamp.sync_epoch = "999".into();
    let changed = wire_record(
        c,
        h,
        5,
        &cc_desk_remote_record::projection::encode_ack(&changed, 1).unwrap(),
    )
    .await;
    assert_eq!(
        conn.acknowledge(&changed, Duration::from_secs(1)).await,
        Err(Reject::Sync)
    );
    drop(changed);
    conn.acknowledge(&ack, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        conn.acknowledge(&ack, Duration::from_secs(1)).await,
        Err(Reject::Sync)
    );
    drop(ack);
    let wr = wire_record(
        c,
        h,
        5,
        br#"{"message":"Window","profile":1,"request":"3"}"#,
    )
    .await;
    if mode == 14 {
        let blocked = f.output.feed_at(0, b"\x1b[5n".to_vec()).unwrap();
        f.entered.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut pending = Box::pin(conn.window(&wr, Duration::from_secs(1)));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(pending);
        f.release.send(()).unwrap();
        blocked.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(
            conn.window(&wr, Duration::from_secs(1)).await,
            Err(Reject::Pending)
        );
        assert_eq!(writes.load(Ordering::Acquire), 1);
        return;
    }
    let (token, not_after_host_ms) = conn.window(&wr, Duration::from_secs(1)).await.unwrap();
    drop(wr);
    let b = cc_desk_remote_record::input::encode(&cc_desk_remote_record::input::Input {
        operation: [1; 16],
        sequence: 1,
        window: token,
        not_after_host_ms,
        bytes: b"PUBLIC_TEST_ONLY".to_vec(),
    })
    .unwrap();
    let r = wire_record(c, h, 5, &b).await;
    if mode == 16 {
        f.now.store(7000, Ordering::Release);
        let value = conn.input(&r, Duration::from_secs(1)).await;
        assert!(!matches!(value,Ok(WriteResult::Receipt(ref r))if r.outcome==Outcome::HostWritten));
        assert_eq!(writes.load(Ordering::Acquire), 0);
        return;
    }
    if mode == 17 {
        f.gate.revoke();
        assert_eq!(
            conn.input(&r, Duration::from_secs(1)).await,
            Err(Reject::Policy)
        );
        assert_eq!(writes.load(Ordering::Acquire), 0);
        return;
    }
    if mode == 15 {
        let blocked = f.output.feed_at(0, b"\x1b[5n".to_vec()).unwrap();
        f.entered.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut pending = Box::pin(conn.input(&r, Duration::from_secs(1)));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(pending);
        f.release.send(()).unwrap();
        blocked.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(
            conn.input(&r, Duration::from_secs(1)).await,
            Err(Reject::Pending)
        );
        let end = Instant::now() + Duration::from_secs(1);
        while writes.load(Ordering::Acquire) != 2 {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        return;
    }
    for _ in 0..2 {
        let WriteResult::Receipt(receipt) = conn.input(&r, Duration::from_secs(1)).await.unwrap()
        else {
            panic!("receipt")
        };
        assert_eq!(receipt.outcome, Outcome::HostWritten);
    }
    assert_eq!(writes.load(Ordering::Acquire), 1);
}
