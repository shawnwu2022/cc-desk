//! 单次启动的原准备与材料释放线程保管，不提供生产 profile 或启动入口。

use parking_lot::Mutex;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::JoinHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationObservation {
    pub(crate) outstanding: bool,
    pub(crate) unknown: bool,
}
impl ActivationObservation {
    pub(crate) fn retired(self) -> bool {
        !self.outstanding && !self.unknown
    }
}

pub(crate) struct ActivationSignal {
    cancelled: AtomicBool,
    #[cfg(windows)]
    revocation: Mutex<Option<Arc<crate::native_crypto::owner::NativeRevocation>>>,
}
impl ActivationSignal {
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(windows)]
        if let Some(token) = self.revocation.lock().as_ref() {
            token.revoke();
        }
    }
    #[cfg(windows)]
    fn bind_revocation(&self, token: Arc<crate::native_crypto::owner::NativeRevocation>) {
        let mut bound = self.revocation.lock();
        assert!(bound.is_none(), "one original key token");
        if self.cancelled() {
            token.revoke();
        }
        *bound = Some(token);
    }
}
struct SignalUnwind(Arc<ActivationSignal>);
impl Drop for SignalUnwind {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.cancel();
        }
    }
}
enum Phase {
    Reserved,
    Publishing,
    NotSpawned,
    Running(JoinHandle<PreparationResult>),
    Joining,
    Prepared(PreparedRegistration),
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "production activation caller is not installed")
    )]
    Completing,
    ReleasePublishing,
    ReleaseRunning(JoinHandle<()>),
    ReleaseJoining,
    ReleaseFailed,
    Joined,
}
pub(crate) type PreparedRegistration = Box<dyn FnOnce() -> bool + Send>;
type ReleaseMailbox = Arc<Mutex<Option<PreparedRegistration>>>;
pub(crate) enum PreparationResult {
    Complete(bool),
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "production preparation supplier is not installed")
    )]
    Prepared(PreparedRegistration),
}
struct State {
    phase: Phase,
    unknown: bool,
    result: Option<bool>,
    release_mailbox: Option<ReleaseMailbox>,
}
pub(crate) struct ActivationOperation {
    state: Mutex<State>,
    signal: Arc<ActivationSignal>,
    #[cfg(windows)]
    keys: Mutex<Option<crate::native_crypto::host::HostKeyCustody>>,
}

/// 失败事实不代替原句柄生命周期。只有原 join 或明确未创建才是终态。
impl ActivationOperation {
    #[cfg(test)]
    pub(crate) fn test_reservation() -> ActivationReservation {
        ActivationReservation::new()
    }
    pub(crate) fn cancel(&self) {
        self.signal.cancel();
    }
    /// Reserved 槽先覆盖无线程 custody，才把锁外创建能力交给唯一 reservation。
    #[cfg(windows)]
    fn begin_keys(
        &self,
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
    ) -> Result<
        crate::native_crypto::host::NotificationBuilder,
        crate::native_crypto::owner::SignFailure,
    > {
        let state = self.state.lock();
        let mut keys = self.keys.lock();
        if !matches!(state.phase, Phase::Reserved) || keys.is_some() {
            return Err(crate::native_crypto::owner::SignFailure::Closed);
        }
        self.signal.bind_revocation(token.clone());
        let custody = crate::native_crypto::host::HostKeyCustody::new(token, cap);
        if self.signal.cancelled() {
            custody.revoke();
        }
        *keys = Some(custody);
        keys.as_ref()
            .expect("canonical reservation")
            .notification_builder()
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_key_observer(&self) -> Arc<crate::native_crypto::host::HostKeyCore> {
        self.keys.lock().as_ref().unwrap().observer()
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_prepare_keys(
        &self,
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
        hooks: crate::native_crypto::host::NotifyHooks,
    ) -> Result<
        crate::native_crypto::host::HostKeyPublisher,
        crate::native_crypto::owner::SignFailure,
    > {
        self.begin_keys(token, cap)?.start(hooks)?;
        self.keys.lock().as_ref().unwrap().publisher()
    }
    pub(crate) fn snapshot(&self) -> ActivationObservation {
        let state = self.state.lock();
        let observation = ActivationObservation {
            outstanding: !matches!(state.phase, Phase::NotSpawned | Phase::Joined),
            unknown: state.unknown,
        };
        #[cfg(windows)]
        {
            let mut observation = observation;
            if let Some(keys) = self.keys.lock().as_ref() {
                let facts = keys.snapshot();
                observation.outstanding |= facts.outstanding;
                observation.unknown |= facts.unknown;
            }
            observation
        }
        #[cfg(not(windows))]
        observation
    }
    pub(crate) fn advance_join(&self) -> ActivationObservation {
        #[cfg(windows)]
        if let Some(keys) = self.keys.lock().as_ref() {
            keys.advance();
        }
        let original = {
            let mut state = self.state.lock();
            if matches!(&state.phase, Phase::Running(worker) if worker.is_finished()) {
                match std::mem::replace(&mut state.phase, Phase::Joining) {
                    Phase::Running(worker) => Some(worker),
                    _ => unreachable!("same lock"),
                }
            } else {
                None
            }
        };
        if let Some(original) = original {
            // 已结束原线程的同步 join 没有 await 取消窗口；Joining 期间其他观察仍 Pending。
            let result = original.join();
            if matches!(&result, Ok(PreparationResult::Complete(false)) | Err(_)) {
                self.signal.cancel();
            }
            let mut state = self.state.lock();
            match result {
                Ok(PreparationResult::Prepared(completion)) => {
                    state.phase = Phase::Prepared(completion)
                }
                Ok(PreparationResult::Complete(result)) => {
                    state.phase = Phase::Joined;
                    state.result = Some(result);
                }
                Err(_) => {
                    state.phase = Phase::Joined;
                    state.unknown = true;
                    state.result = Some(false);
                }
            }
        }
        self.advance_release_join();
        self.snapshot()
    }
    fn advance_release_join(&self) {
        let original = {
            let mut state = self.state.lock();
            if matches!(&state.phase, Phase::ReleaseRunning(worker) if worker.is_finished()) {
                match std::mem::replace(&mut state.phase, Phase::ReleaseJoining) {
                    Phase::ReleaseRunning(worker) => Some((
                        worker,
                        state
                            .release_mailbox
                            .as_ref()
                            .expect("original mailbox")
                            .clone(),
                    )),
                    _ => unreachable!("same lock"),
                }
            } else {
                None
            }
        };
        if let Some((original, mailbox)) = original {
            let result = original.join();
            let empty = mailbox.lock().is_none();
            let retired_mailbox = {
                let mut state = self.state.lock();
                state.unknown |= result.is_err() || !empty;
                if empty {
                    state.phase = Phase::Joined;
                    state.result = Some(false);
                    state.release_mailbox.take()
                } else {
                    // 未释放材料仍留在原 operation；不能在 async waiter 栈析构。
                    state.phase = Phase::ReleaseFailed;
                    None
                }
            };
            drop(retired_mailbox);
        }
    }
    /// 显式启动推进，不由 current/snapshot 查询调用。
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "production activation caller is not installed")
    )]
    pub(crate) fn finish_preparation(&self) -> ActivationObservation {
        self.advance_join();
        let completion = {
            let mut state = self.state.lock();
            if matches!(state.phase, Phase::Prepared(_)) {
                match std::mem::replace(&mut state.phase, Phase::Completing) {
                    Phase::Prepared(completion) => Some(completion),
                    _ => unreachable!("same lock"),
                }
            } else {
                None
            }
        };
        if let Some(completion) = completion {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if self.signal.cancelled() {
                    drop(completion);
                    false
                } else {
                    completion()
                }
            }));
            if !matches!(&result, Ok(true)) {
                self.signal.cancel();
            }
            let mut state = self.state.lock();
            state.phase = Phase::Joined;
            match result {
                Ok(result) => state.result = Some(result),
                Err(_) => {
                    state.unknown = true;
                    state.result = Some(false);
                }
            }
        }
        self.snapshot()
    }
    /// 退出只释放未登记结果，绝不执行其登记能力。
    pub(crate) fn retire(&self) -> ActivationObservation {
        self.cancel();
        self.advance_join();
        self.start_release(ReleaseHooks::default());
        self.snapshot()
    }
    fn start_release(&self, _hooks: ReleaseHooks) {
        let mailbox = {
            let mut state = self.state.lock();
            if !matches!(state.phase, Phase::Prepared(_)) {
                return;
            }
            let Phase::Prepared(completion) =
                std::mem::replace(&mut state.phase, Phase::ReleasePublishing)
            else {
                unreachable!("same lock")
            };
            let mailbox = Arc::new(Mutex::new(Some(completion)));
            state.release_mailbox = Some(mailbox.clone());
            mailbox
        };
        #[cfg(test)]
        if _hooks.fail {
            self.release_spawn_failed();
            return;
        }
        let (release, start) = mpsc::sync_channel(1);
        // 失败闭包只丢 mailbox 的共享引用；材料始终另有 operation 保管。
        let spawned = std::thread::Builder::new()
            .name("remote-host-release".into())
            .spawn(move || {
                let _ = start.recv();
                #[cfg(test)]
                if let Some(before) = _hooks.before_take {
                    before();
                }
                let completion = { mailbox.lock().take() };
                drop(completion);
                #[cfg(test)]
                if let Some(after) = _hooks.after_drop {
                    after();
                }
            });
        let original = match spawned {
            Ok(original) => original,
            Err(_) => {
                self.release_spawn_failed();
                return;
            }
        };
        let publication = ReleasePublication {
            operation: self,
            original: Some(original),
            release: Some(release),
        };
        #[cfg(test)]
        if let Some(before) = _hooks.before_publish {
            before();
        }
        publication.commit();
    }
    fn release_spawn_failed(&self) {
        let mut state = self.state.lock();
        state.phase = Phase::ReleaseFailed;
        state.unknown = true;
    }
    #[cfg(test)]
    pub(crate) fn result(&self) -> Option<bool> {
        self.finish_preparation();
        let state = self.state.lock();
        // 登记结果与仍活着的 key/通知/释放责任是两种事实。
        state.result.map(|result| !state.unknown && result)
    }
    #[cfg(test)]
    pub(crate) fn worker_fact(&self) -> WorkerFact {
        match self.state.lock().phase {
            Phase::NotSpawned => WorkerFact::NotSpawned,
            Phase::Joined
            | Phase::Prepared(_)
            | Phase::Completing
            | Phase::ReleasePublishing
            | Phase::ReleaseRunning(_)
            | Phase::ReleaseJoining
            | Phase::ReleaseFailed => WorkerFact::Joined,
            _ => WorkerFact::Outstanding,
        }
    }
}
impl Drop for ActivationOperation {
    fn drop(&mut self) {
        self.cancel();
        // 无 exit waiter 时仍共同推进，不能先等 prepare/notify 而遗弃兄弟 key。
        #[cfg(windows)]
        if self.keys.get_mut().is_some() {
            while self.retire().outstanding {
                std::thread::park_timeout(std::time::Duration::from_millis(1));
            }
            return;
        }
        // 未装配 key 的首片保管路径保持原有最后 owner 语义。
        match std::mem::replace(&mut self.state.get_mut().phase, Phase::Joining) {
            Phase::Running(original) => {
                let _ = original.join();
            }
            Phase::ReleaseRunning(original) => {
                let _ = original.join();
            }
            _ => {}
        }
    }
}

/// 调用者持 guard，worker 不能持它或 operation，避免自 join 和引用环。
pub(crate) struct ActivationReservation {
    operation: Arc<ActivationOperation>,
    settled: bool,
}
#[derive(Default)]
struct SpawnHooks {
    #[cfg(test)]
    fail: bool,
    #[cfg(test)]
    before_publish: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    after_work: Option<Box<dyn FnOnce() + Send>>,
}
#[derive(Default)]
struct ReleaseHooks {
    #[cfg(test)]
    fail: bool,
    #[cfg(test)]
    before_publish: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    before_take: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    after_drop: Option<Box<dyn FnOnce() + Send>>,
}
struct ReleasePublication<'a> {
    operation: &'a ActivationOperation,
    original: Option<JoinHandle<()>>,
    release: Option<mpsc::SyncSender<()>>,
}
impl ReleasePublication<'_> {
    fn commit(mut self) {
        self.operation.state.lock().phase =
            Phase::ReleaseRunning(self.original.take().expect("original release worker"));
        let _ = self.release.take().expect("original release gate").send(());
    }
}
impl Drop for ReleasePublication<'_> {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            let mut state = self.operation.state.lock();
            state.unknown |= std::thread::panicking();
            state.phase = Phase::ReleaseRunning(original);
        }
        // unwind 也先交回原 handle，随后断开 gate 让原线程释放 mailbox。
    }
}
struct Publication {
    operation: Arc<ActivationOperation>,
    original: Option<JoinHandle<PreparationResult>>,
    release: Option<mpsc::SyncSender<()>>,
}
impl Publication {
    fn commit(mut self) {
        self.operation.state.lock().phase =
            Phase::Running(self.original.take().expect("original worker"));
        if !self.operation.signal.cancelled() {
            let _ = self.release.take().expect("original gate").send(());
        }
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            self.operation.cancel();
            let mut state = self.operation.state.lock();
            state.unknown |= std::thread::panicking();
            state.phase = Phase::Running(original);
        }
        // gate 最后释放，unwind 也先保管真实 handle。
    }
}
impl ActivationReservation {
    #[cfg(test)]
    pub(crate) fn test_fail_spawn(self) -> std::io::Result<()> {
        self.start(
            |_| PreparationResult::Complete(false),
            SpawnHooks {
                fail: true,
                ..Default::default()
            },
        )
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_prepare_keys(
        &mut self,
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
        hooks: crate::native_crypto::host::NotifyHooks,
    ) -> Result<
        crate::native_crypto::host::HostKeyPublisher,
        crate::native_crypto::owner::SignFailure,
    > {
        self.operation.test_prepare_keys(token, cap, hooks)
    }
    #[cfg(windows)]
    #[allow(
        dead_code,
        reason = "Host approval/supplier installation is a separate slice"
    )]
    pub(crate) fn prepare_keys(
        &mut self,
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        cap: std::time::Instant,
    ) -> Result<
        crate::native_crypto::host::HostKeyPublisher,
        crate::native_crypto::owner::SignFailure,
    > {
        self.operation
            .begin_keys(token, cap)?
            .start(crate::native_crypto::host::NotifyHooks::default())?;
        self.operation.keys.lock().as_ref().unwrap().publisher()
    }
    pub(super) fn new() -> Self {
        Self {
            operation: Arc::new(ActivationOperation {
                state: Mutex::new(State {
                    phase: Phase::Reserved,
                    unknown: false,
                    result: None,
                    release_mailbox: None,
                }),
                signal: Arc::new(ActivationSignal {
                    cancelled: AtomicBool::new(false),
                    #[cfg(windows)]
                    revocation: Mutex::new(None),
                }),
                #[cfg(windows)]
                keys: Mutex::new(None),
            }),
            settled: false,
        }
    }
    pub(crate) fn operation(&self) -> Arc<ActivationOperation> {
        self.operation.clone()
    }
    #[cfg(test)]
    pub(crate) fn spawn(
        self,
        work: impl FnOnce(Arc<ActivationSignal>) -> bool + Send + 'static,
    ) -> std::io::Result<()> {
        self.spawn_prepared(move |signal| PreparationResult::Complete(work(signal)))
    }
    #[allow(dead_code, reason = "production material supplier is not installed")]
    pub(crate) fn spawn_prepared(
        self,
        work: impl FnOnce(Arc<ActivationSignal>) -> PreparationResult + Send + 'static,
    ) -> std::io::Result<()> {
        self.start(work, SpawnHooks::default())
    }
    fn start(
        mut self,
        work: impl FnOnce(Arc<ActivationSignal>) -> PreparationResult + Send + 'static,
        _hooks: SpawnHooks,
    ) -> std::io::Result<()> {
        self.operation.state.lock().phase = Phase::Publishing;
        #[cfg(test)]
        if _hooks.fail {
            return Err(std::io::Error::other("TEST_ONLY_SPAWN_FAILURE"));
        }
        let (release, start) = mpsc::sync_channel::<()>(1);
        let signal = self.operation.signal.clone();
        let original = std::thread::Builder::new()
            .name("remote-host-prepare".into())
            .spawn(move || {
                if start.recv().is_err() || signal.cancelled() {
                    signal.cancel();
                    return PreparationResult::Complete(false);
                }
                let result = {
                    // worker 只持 signal/token；失败即撤销，不反持 operation/custody。
                    let _failure = SignalUnwind(signal.clone());
                    work(signal.clone())
                };
                if matches!(&result, PreparationResult::Complete(false)) {
                    signal.cancel();
                }
                let _tail_failure = SignalUnwind(signal);
                #[cfg(test)]
                if let Some(after) = _hooks.after_work {
                    after();
                }
                result
            })?;
        let publication = Publication {
            operation: self.operation.clone(),
            original: Some(original),
            release: Some(release),
        };
        #[cfg(test)]
        if let Some(before) = _hooks.before_publish {
            before();
        }
        publication.commit();
        self.settled = true;
        Ok(())
    }
}
impl Drop for ActivationReservation {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        self.operation.cancel();
        let mut state = self.operation.state.lock();
        state.unknown |= std::thread::panicking();
        if matches!(state.phase, Phase::Reserved | Phase::Publishing) {
            state.phase = Phase::NotSpawned;
            state.result = Some(false);
        }
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkerFact {
    Outstanding,
    NotSpawned,
    Joined,
}

#[cfg(test)]
pub(crate) struct FixtureThread<T>(Option<JoinHandle<T>>);
#[cfg(test)]
impl<T: Send + 'static> FixtureThread<T> {
    pub(crate) fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self(Some(std::thread::spawn(work)))
    }
    pub(crate) fn join(mut self) -> std::thread::Result<T> {
        self.0.take().expect("original fixture thread").join()
    }
}
#[cfg(test)]
impl<T> Drop for FixtureThread<T> {
    fn drop(&mut self) {
        if let Some(original) = self.0.take() {
            let _ = original.join();
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn joined(operation: &ActivationOperation) {
        let until = Instant::now() + Duration::from_secs(3);
        while operation.advance_join().outstanding
            && operation.worker_fact() == WorkerFact::Outstanding
        {
            assert!(Instant::now() < until, "original worker did not finish");
            std::thread::yield_now();
        }
    }
    fn retired(operation: &ActivationOperation) -> ActivationObservation {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let result = operation.retire();
            if !result.outstanding {
                return result;
            }
            assert!(
                Instant::now() < until,
                "original release worker did not join"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn HostActivation_ReservationBeforeWorker_001() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        assert!(operation.advance_join().outstanding);
        assert!(!operation.advance_join().unknown);
        drop(reservation);
        assert!(!operation.advance_join().outstanding);
        assert!(!operation.advance_join().unknown);
        assert_eq!(operation.worker_fact(), WorkerFact::NotSpawned);
    }

    #[test]
    fn HostActivation_CancelBeforeGate_002() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        operation.cancel();
        reservation
            .spawn(|_| panic!("cancelled factory must not run"))
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while operation.advance_join().outstanding {
            assert!(
                std::time::Instant::now() < until,
                "original worker did not finish"
            );
            std::thread::yield_now();
        }
        assert!(!operation.advance_join().unknown);
        assert_eq!(operation.worker_fact(), WorkerFact::Joined);
    }

    #[test]
    fn HostActivation_SpawnFailure_003() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let result = reservation.start(
            |_| panic!("factory"),
            SpawnHooks {
                fail: true,
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(operation.advance_join().retired());
        assert_eq!(operation.worker_fact(), WorkerFact::NotSpawned);
    }

    #[test]
    fn HostActivation_CloseDuringPublication_004() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let (spawned, observed) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let publisher = FixtureThread::spawn(move || {
            reservation.start(
                |_| panic!("factory"),
                SpawnHooks {
                    before_publish: Some(Box::new(move || {
                        spawned.send(()).unwrap();
                        let _ = held.recv_timeout(Duration::from_secs(3));
                    })),
                    ..Default::default()
                },
            )
        });
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        operation.cancel();
        assert!(operation.advance_join().outstanding);
        release.send(()).unwrap();
        publisher.join().unwrap().unwrap();
        joined(&operation);
        assert_eq!(operation.worker_fact(), WorkerFact::Joined);
        assert!(operation.advance_join().retired());
    }

    #[test]
    fn HostActivation_HeldThreadTail_005() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let (returned, observed) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        reservation
            .start(
                |_| PreparationResult::Complete(true),
                SpawnHooks {
                    after_work: Some(Box::new(move || {
                        returned.send(()).unwrap();
                        let _ = held.recv_timeout(Duration::from_secs(3));
                    })),
                    ..Default::default()
                },
            )
            .unwrap();
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        operation.cancel();
        assert!(operation.advance_join().outstanding);
        assert_eq!(operation.result(), None);
        release.send(()).unwrap();
        joined(&operation);
        assert_eq!(operation.result(), Some(true));
    }

    #[test]
    fn HostActivation_PanicAndRejection_006() {
        for panics in [true, false] {
            let reservation = ActivationReservation::new();
            let operation = reservation.operation();
            reservation
                .spawn(move |_| {
                    assert!(!panics, "TEST_ONLY_FACTORY_PANIC");
                    false
                })
                .unwrap();
            joined(&operation);
            assert_eq!(operation.advance_join().unknown, panics);
            assert_eq!(operation.advance_join().retired(), !panics);
            operation.cancel();
            assert_eq!(operation.advance_join().unknown, panics);
            assert_eq!(operation.worker_fact(), WorkerFact::Joined);
        }
    }

    #[test]
    fn HostActivation_ReservationUnwind_007() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _held = reservation;
            panic!("TEST_ONLY_CLOCK_PANIC");
        }));
        assert!(!operation.advance_join().outstanding);
        assert!(operation.advance_join().unknown);
        assert_eq!(operation.worker_fact(), WorkerFact::NotSpawned);
    }

    #[test]
    fn HostActivation_NoOwnerCycle_008() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let weak = Arc::downgrade(&operation);
        reservation.spawn(|_| false).unwrap();
        joined(&operation);
        drop(operation);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn HostActivation_PublicationUnwind_009() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reservation.start(
                |_| panic!("factory"),
                SpawnHooks {
                    before_publish: Some(Box::new(|| panic!("TEST_ONLY_PUBLICATION_PANIC"))),
                    ..Default::default()
                },
            )
        }));
        joined(&operation);
        assert_eq!(operation.worker_fact(), WorkerFact::Joined);
        assert!(operation.snapshot().unknown);
    }
    #[test]
    fn HostActivation_ReadOnlyPreparedResult_010() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let calls = Arc::new(AtomicBool::new(false));
        let registered = calls.clone();
        reservation
            .spawn_prepared(move |_| {
                PreparationResult::Prepared(Box::new(move || {
                    registered.store(true, Ordering::Release);
                    true
                }))
            })
            .unwrap();
        joined(&operation);
        assert!(operation.snapshot().outstanding);
        assert!(!calls.load(Ordering::Acquire));
        assert!(operation.finish_preparation().retired());
        assert!(calls.load(Ordering::Acquire));
    }
    #[test]
    fn HostActivation_ResultRetirement_011() {
        struct Material {
            entered: mpsc::SyncSender<()>,
            release: mpsc::Receiver<()>,
        }
        impl Drop for Material {
            fn drop(&mut self) {
                let _ = self.entered.send(());
                let _ = self.release.recv_timeout(Duration::from_secs(3));
            }
        }
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let (entered, observed) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        reservation
            .spawn_prepared(move |_| {
                let material = Material {
                    entered,
                    release: held,
                };
                PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    panic!("shutdown must not register")
                }))
            })
            .unwrap();
        joined(&operation);
        let retiring = operation.clone();
        let worker = FixtureThread::spawn(move || retired(&retiring));
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(operation.snapshot().outstanding);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().retired());
    }
    #[test]
    fn HostActivation_ResultDropPanic_012() {
        struct Material;
        impl Drop for Material {
            fn drop(&mut self) {
                panic!("TEST_ONLY_MATERIAL_DROP_PANIC");
            }
        }
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        reservation
            .spawn_prepared(|_| {
                let material = Material;
                PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    true
                }))
            })
            .unwrap();
        joined(&operation);
        let result = retired(&operation);
        assert!(!result.outstanding);
        assert!(result.unknown);
    }
    #[test]
    fn HostActivation_ReleaseSpawnFailure_018() {
        struct Material(Arc<AtomicBool>);
        impl Drop for Material {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let material = Material(dropped.clone());
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        reservation
            .spawn_prepared(move |_| {
                PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    true
                }))
            })
            .unwrap();
        joined(&operation);
        operation.cancel();
        operation.start_release(ReleaseHooks {
            fail: true,
            ..Default::default()
        });
        for _ in 0..3 {
            assert_eq!(
                operation.retire(),
                ActivationObservation {
                    outstanding: true,
                    unknown: true
                }
            );
            assert!(
                !dropped.load(Ordering::Acquire),
                "spawn failure must retain materials outside the caller stack"
            );
        }
        assert!(matches!(operation.state.lock().phase, Phase::ReleaseFailed));
        drop(operation);
        assert!(dropped.load(Ordering::Acquire));
    }
    #[test]
    fn HostActivation_ReleaseThreadTail_019() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        reservation
            .spawn_prepared(|_| {
                PreparationResult::Prepared(Box::new(|| panic!("must not register")))
            })
            .unwrap();
        joined(&operation);
        operation.cancel();
        let (entered, observed) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        operation.start_release(ReleaseHooks {
            after_drop: Some(Box::new(move || {
                entered.send(()).unwrap();
                let _ = held.recv_timeout(Duration::from_secs(3));
            })),
            ..Default::default()
        });
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(operation.snapshot().outstanding);
        assert!(
            operation.advance_join().outstanding,
            "material release is not original thread join"
        );
        release.send(()).unwrap();
        assert!(retired(&operation).retired());
    }
    #[test]
    fn HostActivation_ReleasePublicationUnwind_020() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let dropped = Arc::new(AtomicBool::new(false));
        struct Material(Arc<AtomicBool>);
        impl Drop for Material {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let material = Material(dropped.clone());
        reservation
            .spawn_prepared(move |_| {
                PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    panic!("must not register")
                }))
            })
            .unwrap();
        joined(&operation);
        operation.cancel();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            operation.start_release(ReleaseHooks {
                before_publish: Some(Box::new(|| panic!("TEST_ONLY_RELEASE_PUBLICATION_PANIC"))),
                ..Default::default()
            });
        }));
        assert!(result.is_err());
        let result = retired(&operation);
        assert!(!result.outstanding);
        assert!(result.unknown);
        assert!(dropped.load(Ordering::Acquire));
    }
    #[test]
    fn HostActivation_ReleasePanicBeforeTake_021() {
        let reservation = ActivationReservation::new();
        let operation = reservation.operation();
        let dropped = Arc::new(AtomicBool::new(false));
        struct Material(Arc<AtomicBool>);
        impl Drop for Material {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let material = Material(dropped.clone());
        reservation
            .spawn_prepared(move |_| {
                PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    true
                }))
            })
            .unwrap();
        joined(&operation);
        operation.cancel();
        operation.start_release(ReleaseHooks {
            before_take: Some(Box::new(|| panic!("TEST_ONLY_RELEASE_BEFORE_TAKE_PANIC"))),
            ..Default::default()
        });
        let until = Instant::now() + Duration::from_secs(3);
        while !matches!(operation.state.lock().phase, Phase::ReleaseFailed) {
            assert!(Instant::now() < until);
            operation.advance_join();
            std::thread::yield_now();
        }
        assert_eq!(
            operation.retire(),
            ActivationObservation {
                outstanding: true,
                unknown: true
            }
        );
        assert!(
            !dropped.load(Ordering::Acquire),
            "joined release thread did not consume its mailbox"
        );
        assert!(operation
            .state
            .lock()
            .release_mailbox
            .as_ref()
            .unwrap()
            .lock()
            .is_some());
        drop(operation);
        assert!(dropped.load(Ordering::Acquire));
    }
}
