//! Controller 唯一保管启动驱动；service 仅借用 join Core，查询不推进。
use super::activation::{ActivationObservation, ActivationOperation};
use parking_lot::Mutex;
use std::{
    sync::{mpsc, Arc, Weak},
    thread::JoinHandle,
    time::Duration,
};

#[derive(Clone, Copy)]
enum Fact {
    Reserved,
    Publishing,
    NoWorker,
    Running,
    Joining,
    Joined,
    Quarantined,
}
struct Facts {
    phase: Fact,
    unknown: bool,
}
#[derive(Clone)]
struct DriverObservation(Arc<Mutex<Facts>>);
impl DriverObservation {
    fn snapshot(&self) -> ActivationObservation {
        let facts = self.0.lock();
        ActivationObservation {
            outstanding: !matches!(facts.phase, Fact::NoWorker | Fact::Joined),
            unknown: facts.unknown,
        }
    }
    fn publish(&self, phase: Fact, unknown: bool) {
        let mut facts = self.0.lock();
        facts.unknown |= unknown;
        facts.phase = phase;
    }
}
struct DriverEnd {
    unknown: bool,
}
enum DriverPhase {
    Reserved,
    Publishing,
    NoWorker,
    Running(JoinHandle<DriverEnd>),
    Joining,
    Joined,
    Quarantined {
        _payload: std::mem::ManuallyDrop<Box<dyn std::any::Any + Send>>,
    },
}
/// 不负责最后 Drop；唯一 DriverOwner 在原 join 前始终保留本 Core。
struct DriverJoinCore {
    phase: Mutex<DriverPhase>,
    observation: DriverObservation,
}
impl DriverJoinCore {
    fn advance(&self) -> ActivationObservation {
        let original = {
            let mut phase = self.phase.lock();
            if matches!(&*phase, DriverPhase::Running(original) if original.is_finished()) {
                let DriverPhase::Running(original) =
                    std::mem::replace(&mut *phase, DriverPhase::Joining)
                else {
                    unreachable!("same lock")
                };
                self.observation.publish(Fact::Joining, false);
                Some(original)
            } else {
                None
            }
        };
        if let Some(original) = original {
            let result = original.join();
            let mut phase = self.phase.lock();
            match result {
                Ok(end) => {
                    *phase = DriverPhase::Joined;
                    self.observation.publish(Fact::Joined, end.unknown);
                }
                Err(payload) => {
                    // 原线程已退出，但未知 payload 仍是未释放责任；观察者绝不析构或重试。
                    *phase = DriverPhase::Quarantined {
                        _payload: std::mem::ManuallyDrop::new(payload),
                    };
                    self.observation.publish(Fact::Quarantined, true);
                }
            }
        }
        self.observation.snapshot()
    }
    fn abandon(&self) {
        let mut phase = self.phase.lock();
        if matches!(*phase, DriverPhase::Reserved | DriverPhase::Publishing) {
            *phase = DriverPhase::NoWorker;
            self.observation
                .publish(Fact::NoWorker, std::thread::panicking());
        }
    }
}
#[derive(Clone)]
pub(crate) struct DriverLink {
    core: Weak<DriverJoinCore>,
    observation: DriverObservation,
}
impl DriverLink {
    pub(crate) fn snapshot(&self) -> ActivationObservation {
        self.observation.snapshot()
    }
    pub(crate) fn advance(&self) -> ActivationObservation {
        if let Some(core) = self.core.upgrade() {
            core.advance();
        }
        // Weak 已失效只能读取持久事实；不能把失去引用当成实际 join。
        self.snapshot()
    }
}
/// 唯一阻塞 Drop 外壳仅在 controller；driver/service/operation 都不能强持它。
pub(crate) struct DriverOwner {
    core: Arc<DriverJoinCore>,
    operation: Arc<ActivationOperation>,
}
impl DriverOwner {
    pub(crate) fn new(operation: Arc<ActivationOperation>) -> (Self, DriverStart) {
        let core = Arc::new(DriverJoinCore {
            phase: Mutex::new(DriverPhase::Reserved),
            observation: DriverObservation(Arc::new(Mutex::new(Facts {
                phase: Fact::Reserved,
                unknown: false,
            }))),
        });
        (
            Self {
                core: core.clone(),
                operation: operation.clone(),
            },
            DriverStart {
                core,
                operation,
                settled: false,
                hooks: DriverHooks::default(),
            },
        )
    }
    pub(crate) fn link(&self) -> DriverLink {
        DriverLink {
            core: Arc::downgrade(&self.core),
            observation: self.core.observation.clone(),
        }
    }
    pub(crate) fn cancel(&self) {
        self.operation.cancel();
    }
}
impl Drop for DriverOwner {
    fn drop(&mut self) {
        self.operation.cancel();
        loop {
            let operation = self.operation.retire();
            let driver = self.core.advance();
            if !operation.outstanding && !driver.outstanding {
                break;
            }
            // 先共同推进原 key/prepare/release，绝不先 blocking join driver。
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }
}
#[derive(Default)]
pub(crate) struct DriverHooks {
    #[cfg(test)]
    pub(crate) fail_spawn: bool,
    #[cfg(test)]
    pub(crate) before_publish: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(crate) after_work: Option<Box<dyn FnOnce() + Send>>,
}
pub(crate) struct DriverStart {
    core: Arc<DriverJoinCore>,
    operation: Arc<ActivationOperation>,
    settled: bool,
    hooks: DriverHooks,
}
impl DriverStart {
    #[cfg(test)]
    pub(crate) fn test_hooks(mut self, hooks: DriverHooks) -> Self {
        self.hooks = hooks;
        self
    }
    pub(crate) fn spawn(mut self) -> std::io::Result<DriverGate> {
        let hooks = std::mem::take(&mut self.hooks);
        self.spawn_with(hooks)
    }
    fn spawn_with(mut self, _hooks: DriverHooks) -> std::io::Result<DriverGate> {
        {
            let mut phase = self.core.phase.lock();
            assert!(matches!(*phase, DriverPhase::Reserved));
            *phase = DriverPhase::Publishing;
            self.core.observation.publish(Fact::Publishing, false);
        }
        #[cfg(test)]
        if _hooks.fail_spawn {
            return Err(std::io::Error::other("TEST_ONLY_DRIVER_SPAWN"));
        }
        let (release, start) = mpsc::sync_channel(1);
        let operation = self.operation.clone();
        #[cfg(test)]
        let tail = _hooks.after_work;
        let original = std::thread::Builder::new()
            .name("remote-host-register".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if start.recv().is_err() {
                        operation.cancel();
                    }
                    loop {
                        if operation.cancelled() {
                            operation.retire();
                        } else {
                            operation.finish_preparation();
                        }
                        // 成功登记时 key 仍活着；不能等整个 operation retired 才返回业务结果。
                        if operation.preparation_result().is_some() {
                            break;
                        }
                        std::thread::park_timeout(Duration::from_millis(1));
                    }
                    #[cfg(test)]
                    if let Some(tail) = tail {
                        tail();
                    }
                }));
                let unknown = match outcome {
                    Ok(()) => false,
                    Err(payload) => {
                        operation.cancel();
                        // 任意 payload 仅在原线程释放；阻塞仍由原 handle 计 Pending。
                        // 若再次 panic，join 观察者隔离原 payload，不能在退出栈释放。
                        drop(payload);
                        true
                    }
                };
                // 结算已完成；最后捕获引用的析构仍由原 driver handle 保管。
                drop(operation);
                DriverEnd { unknown }
            })?;
        let publication = DriverPublication {
            core: self.core.clone(),
            operation: self.operation.clone(),
            original: Some(original),
            release: Some(release),
        };
        #[cfg(test)]
        if let Some(hook) = _hooks.before_publish {
            hook();
        }
        let gate = publication.commit();
        self.settled = true;
        Ok(gate)
    }
}
impl Drop for DriverStart {
    fn drop(&mut self) {
        if !self.settled {
            self.operation.cancel();
            self.core.abandon();
        }
    }
}
struct DriverPublication {
    core: Arc<DriverJoinCore>,
    operation: Arc<ActivationOperation>,
    original: Option<JoinHandle<DriverEnd>>,
    release: Option<mpsc::SyncSender<()>>,
}
impl DriverPublication {
    fn publish(&mut self, unknown: bool) {
        if let Some(original) = self.original.take() {
            let mut phase = self.core.phase.lock();
            *phase = DriverPhase::Running(original);
            self.core.observation.publish(Fact::Running, unknown);
        }
    }
    fn commit(mut self) -> DriverGate {
        self.publish(false);
        DriverGate {
            operation: self.operation.clone(),
            release: self.release.take(),
        }
    }
}
impl Drop for DriverPublication {
    fn drop(&mut self) {
        if self.original.is_some() {
            self.operation.cancel();
            self.publish(std::thread::panicking());
        }
        // 先保管原句柄，再断开 gate；不能在 unwind 中遗失已创建线程。
    }
}
pub(crate) struct DriverGate {
    operation: Arc<ActivationOperation>,
    release: Option<mpsc::SyncSender<()>>,
}
impl DriverGate {
    pub(crate) fn open(mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
impl Drop for DriverGate {
    fn drop(&mut self) {
        if self.release.is_some() {
            self.operation.cancel();
        }
    }
}

#[cfg(all(test, windows))]
#[path = "remote_host_driver_tests.rs"]
mod tests;
