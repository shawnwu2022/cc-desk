//! 真实 channel、原 actor 及原任务句柄；不用真实 PTY 或用户凭据。
#![allow(non_snake_case)]
use super::*;
use crate::native_host::{ChannelRetirement, NativeLocalInput, NativeOutputIngress};
struct Fixture {
    host: NativeHost,
    input: NativeLocalInput,
    client: RunningChannel,
    channel: Arc<RunningChannel>,
    _directory: tempfile::TempDir,
    _output: NativeOutputIngress,
    _gate: GrantGate,
    _cancel: CancelHandle,
}
impl Fixture {
    async fn new(authorize: Box<dyn FnOnce() -> bool + Send>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut producer = NativeRunProducer::start(
            Journal::open(&directory.path().join("retirement.sqlite")).unwrap(),
            Vec::<u8>::new(),
            "TEST_ONLY_CHANNEL_RETIREMENT".into(),
            3,
            3,
            10,
        )
        .unwrap();
        let input = producer.local_input();
        let output = producer.take_output().unwrap();
        let (client, channel, _gate, _cancel) =
            peer_channels_scope(Arc::new(|| Ok(1000)), producer.native_scope()).await;
        let channel = Arc::new(channel);
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some(channel.authority().principal_id());
        let mut host = NativeHost::from_owner(owner);
        host.anchor = Some(channel.clone());
        host.adopt_native_producer(producer, channel.clone(), authorize)
            .unwrap_or_else(|_| panic!("initial original producer"));
        Self {
            host,
            input,
            client,
            channel,
            _directory: directory,
            _output: output,
            _gate,
            _cancel,
        }
    }
    async fn installed(&mut self) {
        let end = Instant::now() + Duration::from_secs(3);
        while !matches!(self.host.peer_installs[0].stage, Stage::Done(_)) {
            self.host.drive_peer_installs();
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert_eq!(
            self.client
                .client_installed()
                .await
                .unwrap()
                .consume_for(&self.client)
                .unwrap(),
            1
        );
    }
    async fn finish(mut self) {
        self.host.begin_shutdown();
        self.host.retire_peer_installs().await;
        let end = Instant::now() + Duration::from_secs(3);
        while self.host.observe_shutdown() == WriterRetirement::Pending {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        self.host.network_retirement().unwrap().wait().await;
        self.client.close().await;
    }
}
#[tokio::test]
async fn ChannelRetirement_CloseBeforeBindKeepsOriginalActor_001() {
    let (entered, seen) = std::sync::mpsc::sync_channel(1);
    let (release, resume) = std::sync::mpsc::sync_channel(1);
    let mut f = Fixture::new(Box::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(3)).unwrap();
        true
    }))
    .await;
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(f.channel.authority().check().is_ok());
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Pending
    );
    release.send(()).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while f.host.observe_channel_retirement(&token) == ChannelRetirement::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Retired
    );
    assert_eq!(
        f.host.observe_install(f.host.attempts[0].id),
        InstallObservation::Unknown
    );
    assert!(f.input.commit(b"local", || true).unwrap().complete);
    f.finish().await;
}
#[tokio::test]
async fn ChannelRetirement_SealsLateReservationAndRetainsOldResult_002() {
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    let owner = f.host.attempts[0].pending.clone();
    let reserved = owner.reserve::<u64>().unwrap();
    // 模拟已过 authority 检查、尚未 reserve 的原 dispatch。
    assert!(f.channel.authority().check().is_ok());
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    assert!(matches!(owner.reserve::<u64>(), Err(Reject::Sealed)));
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let waiter = reserved.bind(Box::new(move |budget: Duration| {
        assert!(budget.is_zero());
        receive.recv_timeout(budget).map(Ok).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            _ => WaitError::Closed,
        })
    }));
    drop(waiter);
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Pending
    );
    // 取消观察不销毁 Host 中的原 receiver。
    drop(token);
    send.send(17u64).unwrap();
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while f.host.observe_channel_retirement(&token) == ChannelRetirement::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Retired
    );
    let foreign_owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    let mut foreign = NativeHost::from_owner(foreign_owner);
    assert_eq!(
        foreign.observe_channel_retirement(&token),
        ChannelRetirement::Unknown
    );
    assert!(f.input.commit(b"local", || true).unwrap().complete);
    f.finish().await;
}
#[tokio::test]
async fn ChannelRetirement_ActualOriginalJoinAndPanicAreSticky_003() {
    for panic in [false, true] {
        let mut f = Fixture::new(Box::new(|| true)).await;
        f.installed().await;
        let (release, resume) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            resume.await.unwrap();
            assert!(!panic, "TEST_ONLY original response panic");
            Ok(())
        });
        f.host.peer_installs[0].stage = Stage::Response {
            attempt: f.host.attempts[0].id,
            task,
        };
        let token = f.host.begin_channel_retirement(&f.channel).unwrap();
        assert_eq!(
            f.host.observe_channel_retirement(&token),
            ChannelRetirement::Pending
        );
        drop(token);
        assert!(
            matches!(&f.host.peer_installs[0].stage,Stage::Response{task,..} if !task.is_finished())
        );
        release.send(()).unwrap();
        let token = f.host.begin_channel_retirement(&f.channel).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        while f.host.observe_channel_retirement(&token) == ChannelRetirement::Pending {
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        let expected = if panic {
            ChannelRetirement::Unknown
        } else {
            ChannelRetirement::Retired
        };
        assert_eq!(f.host.observe_channel_retirement(&token), expected);
        assert_eq!(f.host.observe_channel_retirement(&token), expected);
        assert!(matches!(f.host.peer_installs[0].stage, Stage::Failed));
        assert!(f.input.commit(b"local", || true).unwrap().complete);
        f.finish().await;
    }
}

#[tokio::test]
async fn ChannelRetirement_LostOriginalBindReplyIsUnknown_004() {
    let (entered, seen) = std::sync::mpsc::sync_channel(1);
    let mut f = Fixture::new(Box::new(move || {
        entered.send(()).unwrap();
        panic!("TEST_ONLY original bind actor panic");
    }))
    .await;
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while f.host.observe_channel_retirement(&token) == ChannelRetirement::Pending {
        assert!(Instant::now() < end);
        tokio::task::yield_now().await;
    }
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Unknown
    );
    assert_eq!(
        f.host.observe_channel_retirement(&token),
        ChannelRetirement::Unknown
    );
    assert!(f.host.unknown);
    f.finish().await;
}
