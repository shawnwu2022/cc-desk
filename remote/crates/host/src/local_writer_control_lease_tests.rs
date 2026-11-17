use super::*;
use cc_desk_remote_record::grant::DataRole;
struct Fixture {
    c: RunningChannel,
    h: RunningChannel,
    conn: Connection<'static>,
    owner: WriterHostOwner,
    target: Target,
    custody: Arc<custody::Owner>,
    _output: HostOutputHandle,
    _dir: tempfile::TempDir,
    _gate: cc_desk_remote_security::GrantGate,
    _cancel: cc_desk_remote_security::CancelHandle,
    now: Arc<AtomicU64>,
}
async fn fixture<W: Write + Send + 'static>(writer: W, enabled: bool) -> Fixture {
    let now = Arc::new(AtomicU64::new(1000));
    let (c, h, gate, cancel) =
        support::lease_channels(now.clone(), enabled, DataRole::Controller).await;
    let mut owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    owner.principal = Some([4; 32]);
    let target = Target {
        host: support::host(),
        session: [2; 16],
        run: support::scope().run,
        generation: 3,
        incarnation: 11,
    };
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_authorized(
            Journal::open(&dir.path().join("TEST_ONLY_lease.sqlite")).unwrap(),
            writer,
            SessionStart {
                target: target.clone(),
                device: support::device(),
                writer_epoch: 9,
                key: DigestKey::from_backend_bytes([0x5a; 32]),
                rows: 3,
                columns: 10,
            },
            h.authority(),
            [8; 16],
            4,
        )
        .unwrap();
    let custody = Arc::new(custody::Owner::new());
    let conn =
        Connection::new_owned(Arc::new(started.peer), h.authority(), 4, custody.clone()).unwrap();
    Fixture {
        c,
        h,
        conn,
        owner,
        target,
        custody,
        _output: started.output,
        _dir: dir,
        _gate: gate,
        _cancel: cancel,
        now,
    }
}
impl Fixture {
    async fn exchange(&mut self, v: serde_json::Value) -> wire::Message {
        exchange(
            &mut self.conn,
            &self.c,
            &self.h,
            &serde_json::to_vec(&v).unwrap(),
        )
        .await
    }
    async fn rejected(&mut self, v: serde_json::Value) {
        let r = received(&self.c, &self.h, 4, &serde_json::to_vec(&v).unwrap()).await;
        assert!(
            self.conn
                .dispatch_wire(&r, &self.h, Duration::from_secs(1))
                .await
                .is_err()
        );
        assert!(self.h.authority().check().is_err());
    }
    async fn close(mut self) {
        self.custody.seal();
        assert_eq!(self.custody.observe(), custody::Observation::Drained);
        self.owner.seal(&self.target).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while !self.owner.try_reap(&self.target).unwrap() {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        self.c.authority().request_close();
        self.h.authority().request_close();
        let (c, h) = tokio::join!(self.c.wait_closed_checked(), self.h.wait_closed_checked());
        assert!(c.is_ok() && h.is_ok());
    }
}
fn renew(request: u64, sequence: u64) -> serde_json::Value {
    serde_json::json!({"message":"LeaseRenew","profile":1,"request":request.to_string(),"lease":"08".repeat(16),"fence":"4","renewalSequence":sequence.to_string()})
}
#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_RealWireExactStateIdempotentAndFixedEnd_006() {
    let writes = Arc::new(AtomicUsize::new(0));
    let mut f = fixture(Count(writes.clone()), true).await;
    assert!(
        matches!(f.exchange(serde_json::json!({"message":"LeaseState","profile":1,"request":"1"})).await,wire::Message::LeaseStateReply{lease,fence,next_renewal_sequence,not_after_host_ms,..} if lease=="08".repeat(16)&&fence=="4"&&next_renewal_sequence=="1"&&not_after_host_ms=="16000")
    );
    f.now.store(5000, Ordering::Release);
    assert!(
        matches!(f.exchange(renew(2,1)).await,wire::Message::LeaseRenewed{not_after_host_ms,..} if not_after_host_ms=="20000")
    );
    f.now.store(6000, Ordering::Release);
    assert!(
        matches!(f.exchange(renew(3,1)).await,wire::Message::LeaseRenewed{not_after_host_ms,..} if not_after_host_ms=="20000")
    );
    for sequence in 2..=9 {
        f.now
            .store(5000 + (sequence - 1) * 10000, Ordering::Release);
        let reply = f.exchange(renew(sequence + 2, sequence)).await;
        let wire::Message::LeaseRenewed {
            not_after_host_ms, ..
        } = reply
        else {
            panic!()
        };
        assert!(not_after_host_ms.parse::<u64>().unwrap() <= 91000);
    }
    assert_eq!(writes.load(Ordering::Acquire), 0);
    f.now.store(91000, Ordering::Release);
    assert!(f.h.authority().check().is_err());
    f.close().await;
}
#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_OldCapabilityWrongLeaseFenceGapAndExpiryRefused_007() {
    for case in 0..5 {
        let mut f = fixture(std::io::sink(), case != 0).await;
        let mut v = renew(1, 1);
        match case {
            1 => v["lease"] = "09".repeat(16).into(),
            2 => v["fence"] = "5".into(),
            3 => v["renewalSequence"] = "2".into(),
            4 => f.now.store(16000, Ordering::Release),
            _ => {}
        }
        f.rejected(v).await;
        f.close().await;
    }
}

struct Count(Arc<AtomicUsize>);
impl Write for Count {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_ViewCannotCreateControllerActor_008() {
    let (c, h, _gate, _cancel) =
        support::lease_channels(Arc::new(AtomicU64::new(1000)), true, DataRole::Observer).await;
    assert!(h.authority().control_lease_renew_enabled());
    let mut owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    owner.principal = Some([4; 32]);
    let target = Target {
        host: support::host(),
        session: [2; 16],
        run: support::scope().run,
        generation: 3,
        incarnation: 11,
    };
    let dir = tempfile::tempdir().unwrap();
    let result = owner.start_authorized(
        Journal::open(&dir.path().join("TEST_ONLY_view.sqlite")).unwrap(),
        std::io::sink(),
        SessionStart {
            target,
            device: support::device(),
            writer_epoch: 9,
            key: DigestKey::from_backend_bytes([0x5a; 32]),
            rows: 3,
            columns: 10,
        },
        h.authority(),
        [8; 16],
        4,
    );
    assert!(result.is_err());
    c.authority().request_close();
    h.authority().request_close();
    let (c, h) = tokio::join!(c.wait_closed_checked(), h.wait_closed_checked());
    assert!(c.is_ok() && h.is_ok());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn ControlLease_CancelledObserverKeepsReceiverAndRejectsQueuedRevoked_009() {
    struct Block {
        writes: Arc<AtomicUsize>,
        entered: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Write for Block {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.writes.fetch_add(1, Ordering::AcqRel);
            self.entered.send(()).unwrap();
            self.resume
                .recv_timeout(Duration::from_secs(3))
                .map_err(std::io::Error::other)?;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let writes = Arc::new(AtomicUsize::new(0));
    let (entered, seen) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    let mut f = fixture(
        Block {
            writes: writes.clone(),
            entered,
            resume,
        },
        true,
    )
    .await;
    let (window, not_after_host_ms) = synchronized(&mut f.conn, &f.c, &f.h).await;
    let frame = input::encode(&input::Input {
        operation: [2; 16],
        sequence: 1,
        window,
        not_after_host_ms,
        bytes: b"TEST_ONLY".to_vec(),
    })
    .unwrap();
    let input = received(&f.c, &f.h, 4, &frame).await;
    // 原输入先进入真实 writer，保留其独立结果，不从网络取消中丢弃。
    let input_wait = f
        .conn
        .peer
        .submit_received(&input, f.conn.view.as_ref().unwrap())
        .unwrap();
    seen.recv_timeout(Duration::from_secs(1)).unwrap();
    let r = received(&f.c, &f.h, 4, &serde_json::to_vec(&renew(3, 1)).unwrap()).await;
    let mut observation = Box::pin(f.conn.dispatch_wire(&r, &f.h, Duration::from_secs(1)));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let polled = std::future::Future::poll(observation.as_mut(), &mut context);
    assert!(polled.is_pending(), "TEST_ONLY_LEASE_FIRST_POLL {polled:?}");
    assert_eq!(f.custody.observe(), custody::Observation::Pending);
    drop(observation);
    assert!(f.h.authority().check().is_err());
    release.send(()).unwrap();
    let _original_result = input_wait.wait(Duration::from_secs(1)).unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    while f.custody.observe() == custody::Observation::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(f.custody.observe(), custody::Observation::Drained);
    assert_eq!(writes.load(Ordering::Acquire), 1);
    f.close().await;
}

#[tokio::test]
#[allow(non_snake_case)]
async fn ControlLease_LostApplicationReplyRetriesSameSequenceWithNewRequest_010() {
    let writes = Arc::new(AtomicUsize::new(0));
    let mut f = fixture(Count(writes.clone()), true).await;
    f.now.store(5000, Ordering::Release);
    let r = received(&f.c, &f.h, 4, &serde_json::to_vec(&renew(1, 1)).unwrap()).await;
    f.conn
        .dispatch_wire(&r, &f.h, Duration::from_secs(1))
        .await
        .unwrap();
    // 模拟应用未交付回复；不伪造传输超时后仍可使用已封闭通道。
    let original =
        f.c.receive(&support::scope(), 4, Duration::from_secs(1))
            .await
            .unwrap();
    drop(original);
    f.now.store(6000, Ordering::Release);
    assert!(
        matches!(f.exchange(renew(2,1)).await,wire::Message::LeaseRenewed{request,renewal_sequence,not_after_host_ms,..} if request=="2"&&renewal_sequence=="1"&&not_after_host_ms=="20000")
    );
    assert!(
        matches!(f.exchange(serde_json::json!({"message":"LeaseState","profile":1,"request":"3"})).await,wire::Message::LeaseStateReply{next_renewal_sequence,not_after_host_ms,..} if next_renewal_sequence=="2"&&not_after_host_ms=="20000")
    );
    assert_eq!(writes.load(Ordering::Acquire), 0);
    f.rejected(renew(3, 2)).await;
    f.close().await;
}
