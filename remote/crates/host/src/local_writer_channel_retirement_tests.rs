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
    clock: Arc<dyn cc_desk_remote_security::GrantClock>,
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
        let clock: Arc<dyn cc_desk_remote_security::GrantClock> = Arc::new(|| Ok(1000));
        let (client, channel, _gate, _cancel) =
            peer_channels_scope(clock.clone(), producer.native_scope()).await;
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
            clock,
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

#[tokio::test]
async fn ChannelReadiness_InstalledExactHostAndSingleIssue_001() {
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    let ready = loop {
        match f.host.retired_installed_channel(&token).unwrap() {
            Some(ready) => break ready,
            None => {
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            }
        }
    };
    assert!(f.host.retired_installed_channel(&token).is_err());
    let other_owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    let mut other = NativeHost::from_owner(other_owner);
    assert!(
        other
            .begin_native_resource_rebind_retired(ready, f.channel.clone(), Box::new(|| true))
            .is_err()
    );
    assert!(f.input.commit(b"local", || true).unwrap().complete);
    f.finish().await;
}
#[tokio::test]
async fn ChannelReadiness_RejectedFirstInstallNeverQualifies_002() {
    let mut f = Fixture::new(Box::new(|| false)).await;
    let token = f.host.begin_channel_retirement(&f.channel).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        match f.host.retired_installed_channel(&token) {
            Ok(None) => {
                assert!(Instant::now() < end);
                tokio::task::yield_now().await;
            }
            Err(_) => break,
            Ok(Some(_)) => panic!("rejected installation minted readiness"),
        }
    }
    assert!(f.input.commit(b"local", || true).unwrap().complete);
    f.finish().await;
}

#[tokio::test]
async fn ChannelHistory_OriginalHostSurvives130JoinedGenerations_001() {
    history_generations(false).await;
}
#[tokio::test]
async fn ChannelHistory_RetainedTokensKeep64Capacity_005() {
    history_generations(true).await;
}
async fn history_generations(retain_tokens: bool) {
    let mut retained = Vec::new();
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    let first = f.host.attempts[0].id;
    let mut failure = None;
    for generation in 2..=130u64 {
        let scope = f.channel.authority().scope().clone();
        let token = f.host.begin_channel_retirement(&f.channel).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        let ready = loop {
            match f.host.retired_installed_channel(&token).unwrap() {
                Some(r) => break r,
                None => {
                    assert!(Instant::now() < end);
                    tokio::task::yield_now().await;
                }
            }
        };
        f.client.close().await;
        let _ = f.channel.wait_closed_checked().await;
        let (client, channel, gate, cancel) = peer_channels_scope(f.clock.clone(), scope).await;
        let channel = Arc::new(channel);
        if let Err(error) =
            f.host
                .begin_native_resource_rebind_retired(ready, channel.clone(), Box::new(|| true))
        {
            channel.authority().request_close();
            client.close().await;
            let _ = channel.wait_closed_checked().await;
            failure = Some((generation, error));
            break;
        }
        if retain_tokens {
            retained.push(token);
        } else {
            drop(token);
        }
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            f.host.drive_peer_installs();
            if matches!(f.host.peer_installs.last().unwrap().stage, Stage::Done(_)) {
                break;
            }
            assert!(Instant::now() < end);
            tokio::task::yield_now().await;
        }
        assert_eq!(
            client
                .client_installed()
                .await
                .unwrap()
                .consume_for(&client)
                .unwrap(),
            generation
        );
        assert_eq!(f.host.attempts.last().unwrap().id.sequence, generation);
        if !retain_tokens && generation > 3 {
            assert_eq!(f.host.observe_install(first), InstallObservation::Unknown);
            assert!(f.host.attempts.len() <= 3);
        }
        f.client = client;
        f.channel = channel;
        f._gate = gate;
        f._cancel = cancel;
        assert!(f.input.commit(b"x", || true).unwrap().complete);
        assert!(
            f.host.attempts.len() <= 64
                && f.host.peer_installs.len() <= 64
                && f.host.channel_retirements.len() <= 64
        );
    }
    for token in &retained {
        assert_eq!(
            f.host.observe_channel_retirement(token),
            ChannelRetirement::Retired
        );
    }
    f.finish().await;
    if retain_tokens {
        assert!(matches!(failure, Some((65, NativeHostError::Capacity))));
    } else {
        assert!(failure.is_none(), "original history exhausted: {failure:?}");
    }
}

// 真实等待者持有原 task 锁时，GC 的单次取消观察不能替换其唤醒源。
#[tokio::test]
async fn ChannelHistory_JoinObserverSurvivesGcPoll_002() {
    use std::future::Future;
    use std::task::{Context, Poll, Wake, Waker};
    struct Signal(std::sync::atomic::AtomicBool);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    let signal = Arc::new(Signal(std::sync::atomic::AtomicBool::new(false)));
    let waker = Waker::from(signal.clone());
    let mut wait = Box::pin(f.channel.wait_closed_checked());
    assert!(matches!(
        wait.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    assert_eq!(super::super::history::poll_channel_join(&f.channel), None);
    f.channel.authority().request_close();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !signal.0.load(std::sync::atomic::Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), wait)
            .await
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        super::super::history::poll_channel_join(&f.channel),
        Some(true)
    );
    f.finish().await;
}

// GC 先观察 Pending 并丢弃 future，后来的原等待者仍能取得同一真实 join。
#[tokio::test]
async fn ChannelHistory_CancelGcPollKeepsOriginalJoin_003() {
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    assert_eq!(super::super::history::poll_channel_join(&f.channel), None);
    let channel = f.channel.clone();
    let waiter = tokio::spawn(async move { channel.wait_closed_checked().await });
    tokio::task::yield_now().await;
    f.channel.authority().request_close();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), waiter)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        super::super::history::poll_channel_join(&f.channel),
        Some(true)
    );
    f.finish().await;
}

#[tokio::test]
async fn ChannelHistory_BrandedIdsAndExhaustionNeverAlias_004() {
    let mut f = Fixture::new(Box::new(|| true)).await;
    f.installed().await;
    let owner =
        WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
    let mut other = NativeHost::from_owner(owner);
    let original = f.host.attempts[0].id;
    let other_id = other.allocate_attempt_id().unwrap();
    assert_eq!(original.sequence, other_id.sequence);
    assert_ne!(original, other_id);
    assert_eq!(
        f.host.observe_install(other_id),
        InstallObservation::Unknown
    );
    assert_eq!(other.observe_install(original), InstallObservation::Unknown);
    let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
    assert_eq!(allocate_host_brand(&counter), Some(u64::MAX - 1));
    assert_eq!(allocate_host_brand(&counter), None);
    assert_eq!(counter.load(std::sync::atomic::Ordering::Acquire), u64::MAX);
    other.next_attempt_id = u64::MAX;
    assert_eq!(other.allocate_attempt_id(), Err(NativeHostError::Capacity));
    f.host.next_retirement_id = u64::MAX;
    assert!(matches!(
        f.host.begin_channel_retirement(&f.channel),
        Err(NativeHostError::Capacity)
    ));
    assert!(f.channel.authority().check().is_ok());
    assert!(f.host.channel_retirements.is_empty());
    f.finish().await;
}
