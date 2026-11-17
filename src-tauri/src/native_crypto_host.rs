//! Host 两用途保管共用原批准 token；existing-key 适配已有，生产配置和启动调用者未接入。
#![allow(
    dead_code,
    reason = "Host approval/supplier installation is a separate slice"
)]
use super::{owner::*, SigningPurpose};
#[cfg(test)]
use super::{CryptoError, NativeOwnedSoftwareKey};
#[path = "native_crypto_host_ready.rs"]
mod ready;
#[path = "native_crypto_host_stages.rs"]
mod stages;
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_transport::wss::RelayAdmissionIdentity;
use parking_lot::{Condvar, Mutex};
pub(crate) use ready::HostKeysReady;
pub(crate) use stages::HostSigningStages;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) enum Opener {
    Existing(super::existing::ExistingKeyLoader),
    #[cfg(test)]
    Fixture(Box<dyn FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send>),
}
pub(super) type OpenerMailbox = Arc<Mutex<Option<Opener>>>;
/// 元数据与两个不可替换 opener 一起移动；没有 Issuer opener 或可变 profile 引用。
pub(crate) struct FrozenHostSupplier {
    binding: FrozenHostBinding,
    pub(super) key_references: Option<[String; 2]>,
    openers: [OpenerMailbox; 2],
}
#[derive(Clone)]
pub(crate) struct FrozenHostBinding {
    pub(crate) native_profile: HostKeyProfileIdentity,
    pub(crate) endpoint_spki: [u8; 91],
    pub(crate) relay_spki: [u8; 91],
    pub(crate) origin: CanonicalOrigin,
    pub(crate) relay_id: [u8; 16],
    pub(crate) profile_cap: Instant,
}
/// 原生密钥供应配置域，既不是 CLI Profile，也不是 BrowserProfile。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostKeyProfileIdentity {
    pub(crate) id: [u8; 16],
    pub(crate) revision: u64,
}
impl FrozenHostBinding {
    pub(crate) fn validate(&self) -> Result<(), SignFailure> {
        if self.native_profile.id == [0; 16]
            || self.native_profile.revision == 0
            || self.endpoint_spki == self.relay_spki
            || self.relay_id == [0; 16]
            || self.origin.as_str().len() > 512
            || cc_desk_remote_security::P256Spki::new(&self.endpoint_spki).is_err()
            || cc_desk_remote_security::P256Spki::new(&self.relay_spki).is_err()
        {
            return Err(SignFailure::Bounds);
        }
        Ok(())
    }
}
impl FrozenHostSupplier {
    // 仅兼容原公开匿名 fixture；生产冻结构造由 NativeHostProfile 提供。
    #[cfg(test)]
    pub(crate) fn test_new(
        binding: FrozenHostBinding,
        endpoint: Box<dyn FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send>,
        relay: Box<dyn FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send>,
    ) -> Self {
        Self::from_openers(binding, Opener::Fixture(endpoint), Opener::Fixture(relay))
    }
    pub(super) fn from_openers(
        binding: FrozenHostBinding,
        endpoint: Opener,
        relay: Opener,
    ) -> Self {
        Self {
            binding,
            key_references: None,
            openers: [
                Arc::new(Mutex::new(Some(endpoint))),
                Arc::new(Mutex::new(Some(relay))),
            ],
        }
    }
    pub(crate) fn key_references(&self) -> Option<&[String; 2]> {
        self.key_references.as_ref()
    }
    pub(crate) fn binding(&self) -> &FrozenHostBinding {
        &self.binding
    }
}
pub(crate) struct HostSignerSet {
    pub(crate) stages: HostSigningStages,
    pub(crate) endpoint: Arc<dyn rustls::sign::SigningKey>,
    pub(crate) relay: RelayAdmissionIdentity,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct CustodyObservation {
    pub(crate) outstanding: bool,
    pub(crate) unknown: bool,
    pub(crate) keys: [Option<KeyObservation>; 2],
    pub(crate) publisher_closed: bool,
    pub(crate) disposal_complete: bool,
}
struct Slot {
    owner: KeyCustodian,
    signal: KeyRequestHandle,
    sealed: bool,
    #[cfg(test)]
    _last_drop: Option<LastDrop>,
}
#[cfg(test)]
struct LastDrop(Box<dyn FnOnce() + Send>);
#[cfg(test)]
impl Drop for LastDrop {
    fn drop(&mut self) {
        let f = std::mem::replace(&mut self.0, Box::new(|| {}));
        f();
    }
}
struct CoreState {
    ready_issued: bool,
    slots: [Option<Box<Slot>>; 2],
    facts: [Option<KeyObservation>; 2],
    notices: VecDeque<KeyShutdownNotice>,
    supplier: Option<FrozenHostSupplier>,
    publisher_closed: bool,
    sealed: bool,
    notifying: bool,
    disposing: bool,
    disposal_complete: bool,
    unknown: bool,
    cap: Instant,
}
pub(crate) struct HostKeyCore {
    state: Mutex<CoreState>,
    changed: Condvar,
    token: Arc<NativeRevocation>,
    #[cfg(test)]
    notify_joins: std::sync::atomic::AtomicUsize,
}
impl HostKeyCore {
    #[cfg(test)]
    pub(crate) fn test_notify_joins(&self) -> usize {
        self.notify_joins.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn snapshot(&self) -> CustodyObservation {
        let s = self.state.lock();
        CustodyObservation {
            outstanding: !s.disposal_complete,
            unknown: s.unknown,
            keys: s.facts,
            publisher_closed: s.publisher_closed,
            disposal_complete: s.disposal_complete,
        }
    }
    fn seal_locked(&self, s: &mut CoreState) {
        self.token.revoke();
        s.sealed = true;
        for slot in s.slots.iter_mut().flatten() {
            if !slot.sealed {
                s.notices.push_back(slot.signal.prepare_shutdown());
                slot.sealed = true;
            }
        }
    }
    fn seal(&self) {
        self.seal_locked(&mut self.state.lock());
        self.changed.notify_all();
    }
    fn advance(&self) {
        let mut s = self.state.lock();
        if self.token.is_revoked() || Instant::now() >= s.cap {
            self.seal_locked(&mut s);
        }
        for index in 0..2 {
            if let Some(slot) = &mut s.slots[index] {
                let fact = slot.owner.observe_complete();
                s.unknown |= fact.retirement == KeyRetirement::Unknown;
                s.facts[index] = Some(fact);
            }
        }
    }
    fn close_publisher(&self, success: bool) {
        let mut s = self.state.lock();
        s.publisher_closed = true;
        if !success {
            self.seal_locked(&mut s);
        }
        drop(s);
        self.changed.notify_all();
    }
    fn failed(&self) {
        self.token.revoke();
        let mut s = self.state.lock();
        s.unknown = true;
        self.seal_locked(&mut s);
        drop(s);
        self.changed.notify_all();
    }
    fn run(&self, _hooks: NotifyHooks) {
        #[cfg(test)]
        let mut hooks = _hooks;
        loop {
            self.advance();
            let notice = {
                let mut s = self.state.lock();
                let n = s.notices.pop_front();
                if n.is_some() {
                    s.notifying = true;
                }
                n
            };
            if let Some(notice) = notice {
                notice.notify();
                self.state.lock().notifying = false;
                continue;
            }
            let disposal = {
                let mut s = self.state.lock();
                if s.sealed
                    && s.publisher_closed
                    && !s.notifying
                    && s.notices.is_empty()
                    && s.facts.iter().flatten().all(|f| !f.holds_original_join)
                {
                    s.disposing = true;
                    Some((std::mem::take(&mut s.slots), s.supplier.take()))
                } else {
                    None
                }
            };
            if let Some(disposal) = disposal {
                #[cfg(test)]
                if let Some(hook) = hooks.before_dispose.take() {
                    hook();
                }
                // 仅这个原线程销毁真正的槽、signal keeper 和未消费 opener。
                drop(disposal);
                {
                    let mut s = self.state.lock();
                    s.disposal_complete = true;
                    s.disposing = false;
                }
                #[cfg(test)]
                if let Some(hook) = hooks.after_dispose.take() {
                    hook();
                }
                return;
            }
            let mut s = self.state.lock();
            self.changed.wait_for(&mut s, Duration::from_millis(10));
        }
    }
}
#[derive(Default)]
pub(crate) struct NotifyHooks {
    #[cfg(test)]
    pub(crate) fail_spawn: bool,
    #[cfg(test)]
    pub(crate) before_publish: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(crate) before_dispose: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(crate) after_dispose: Option<Box<dyn FnOnce() + Send>>,
}
struct NotifyOwner {
    original: Option<JoinHandle<()>>,
    gate: Option<mpsc::SyncSender<()>>,
    creation_reserved: bool,
    creation_complete: bool,
    issued: bool,
    joining: bool,
    no_worker: bool,
}
/// 唯一应用 owner。Core/key/notify worker 都不反持本对象或 operation。
pub(crate) struct HostKeyCustody {
    core: Arc<HostKeyCore>,
    notify: Arc<Mutex<NotifyOwner>>,
    #[cfg(test)]
    snapshot_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl HostKeyCustody {
    /// 无原线程；必须先装进 operation 的 Reserved 槽，才取得通知创建能力。
    pub(crate) fn new(token: Arc<NativeRevocation>, cap: Instant) -> Self {
        Self {
            core: Arc::new(HostKeyCore {
                state: Mutex::new(CoreState {
                    ready_issued: false,
                    slots: Default::default(),
                    facts: [None; 2],
                    notices: VecDeque::new(),
                    supplier: None,
                    publisher_closed: false,
                    sealed: false,
                    notifying: false,
                    disposing: false,
                    disposal_complete: false,
                    unknown: false,
                    cap,
                }),
                changed: Condvar::new(),
                token,
                #[cfg(test)]
                notify_joins: std::sync::atomic::AtomicUsize::new(0),
            }),
            notify: Arc::new(Mutex::new(NotifyOwner {
                original: None,
                gate: None,
                creation_reserved: false,
                creation_complete: false,
                issued: false,
                joining: false,
                no_worker: false,
            })),
            #[cfg(test)]
            snapshot_hook: Mutex::new(None),
        }
    }
    pub(crate) fn notification_builder(&self) -> Result<NotificationBuilder, SignFailure> {
        let mut n = self.notify.lock();
        if n.creation_reserved {
            return Err(SignFailure::Closed);
        }
        n.creation_reserved = true;
        Ok(NotificationBuilder {
            core: self.core.clone(),
            notify: self.notify.clone(),
            settled: false,
        })
    }
    #[cfg(test)]
    pub(crate) fn reserve_with(
        token: Arc<NativeRevocation>,
        cap: Instant,
        hooks: NotifyHooks,
    ) -> Self {
        let owner = Self::new(token, cap);
        let _ = owner.notification_builder().unwrap().start(hooks);
        owner
    }
    pub(crate) fn revoke(&self) {
        self.core.token.revoke();
    }
    pub(crate) fn publisher(&self) -> Result<HostKeyPublisher, SignFailure> {
        let mut n = self.notify.lock();
        if !n.creation_complete
            || n.issued
            || n.no_worker
            || self.core.state.lock().publisher_closed
        {
            return Err(SignFailure::Closed);
        }
        n.issued = true;
        if let Some(gate) = n.gate.take() {
            let _ = gate.try_send(());
        }
        Ok(HostKeyPublisher {
            core: self.core.clone(),
            success: false,
        })
    }
    pub(crate) fn snapshot(&self) -> CustodyObservation {
        #[cfg(test)]
        {
            let hook = self.snapshot_hook.lock().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        // 同一 notify→Core 顺序取得完整事实，不能拼接旧成功与新空 handle。
        let n = self.notify.lock();
        let mut observation = self.core.snapshot();
        observation.outstanding |= n.original.is_some() || n.joining;
        observation
    }
    pub(crate) fn advance(&self) -> CustodyObservation {
        self.core.advance();
        let handle = {
            let mut n = self.notify.lock();
            if n.original.as_ref().is_some_and(|h| h.is_finished()) {
                n.joining = true;
                n.original.take()
            } else {
                None
            }
        };
        if let Some(handle) = handle {
            if handle.join().is_err() {
                self.core.failed();
            }
            #[cfg(test)]
            self.core
                .notify_joins
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            self.notify.lock().joining = false;
        }
        self.snapshot()
    }
    #[cfg(test)]
    pub(crate) fn observer(&self) -> Arc<HostKeyCore> {
        self.core.clone()
    }
    #[cfg(test)]
    pub(crate) fn test_between_snapshot(&self, hook: Box<dyn FnOnce() + Send>) {
        *self.snapshot_hook.lock() = Some(hook);
    }
}
/// 仅同步构造调用栈持有；工作线程只持 Core，不持这个原 handle owner。
pub(crate) struct NotificationBuilder {
    core: Arc<HostKeyCore>,
    notify: Arc<Mutex<NotifyOwner>>,
    settled: bool,
}
impl NotificationBuilder {
    pub(crate) fn start(mut self, _hooks: NotifyHooks) -> Result<(), SignFailure> {
        #[cfg(test)]
        let mut hooks = _hooks;
        #[cfg(not(test))]
        let hooks = _hooks;
        #[cfg(test)]
        let before_publish = hooks.before_publish.take();
        #[cfg(test)]
        let fail = hooks.fail_spawn;
        #[cfg(not(test))]
        let fail = false;
        let (gate, start) = mpsc::sync_channel(1);
        let worker_core = self.core.clone();
        let spawned = if fail {
            Err(std::io::Error::other("TEST_ONLY_NOTIFY_SPAWN_FAILURE"))
        } else {
            std::thread::Builder::new()
                .name("remote-host-keys".into())
                .spawn(move || {
                    let _ = start.recv();
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        worker_core.run(hooks)
                    }))
                    .is_err()
                    {
                        worker_core.failed();
                    }
                })
        };
        let original = match spawned {
            Ok(original) => original,
            Err(_) => {
                self.no_worker();
                self.settled = true;
                return Err(SignFailure::Closed);
            }
        };
        {
            let mut n = self.notify.lock();
            n.original = Some(original);
            n.gate = Some(gate);
        }
        // 原 handle 已交回 canonical custody，随后 unwind 也不得拒收或 detach。
        #[cfg(test)]
        if let Some(hook) = before_publish {
            hook();
        }
        self.notify.lock().creation_complete = true;
        self.settled = true;
        Ok(())
    }
    fn no_worker(&self) {
        let mut n = self.notify.lock();
        self.core.token.revoke();
        let mut s = self.core.state.lock();
        // publisher 从未发出，因此不存在 key/notice/待析构材料。
        debug_assert!(
            !n.issued
                && n.original.is_none()
                && s.supplier.is_none()
                && s.slots.iter().all(Option::is_none)
                && s.notices.is_empty()
        );
        s.publisher_closed = true;
        s.sealed = true;
        s.disposal_complete = true;
        n.no_worker = true;
        n.creation_complete = true;
    }
}
impl Drop for NotificationBuilder {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        if self.notify.lock().original.is_none() {
            self.no_worker();
            return;
        }
        self.core.failed();
        self.core.close_publisher(false);
        self.notify.lock().gate.take();
    }
}
impl Drop for HostKeyCustody {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.core.failed();
        }
        self.core.seal();
        {
            let mut n = self.notify.lock();
            if !n.issued {
                self.core.close_publisher(false);
            }
            n.gate.take();
            if !n.creation_reserved {
                n.no_worker = true;
                self.core.state.lock().disposal_complete = true;
            }
        }
        while self.advance().outstanding {
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }
}
pub(crate) struct HostKeyPublisher {
    core: Arc<HostKeyCore>,
    success: bool,
}
#[derive(Default)]
pub(crate) struct PrepareHooks {
    #[cfg(test)]
    pub(crate) fail_second_spawn: bool,
    #[cfg(test)]
    pub(crate) first_opened: Option<mpsc::Receiver<()>>,
}
impl HostKeyPublisher {
    pub(crate) fn prepare(
        self,
        supplier: FrozenHostSupplier,
    ) -> Result<HostSignerSet, SignFailure> {
        self.prepare_with(supplier, PrepareHooks::default())
    }
    pub(crate) fn prepare_with(
        mut self,
        supplier: FrozenHostSupplier,
        _hooks: PrepareHooks,
    ) -> Result<HostSignerSet, SignFailure> {
        let binding = supplier.binding.clone();
        {
            let mut s = self.core.state.lock();
            s.cap = s.cap.min(binding.profile_cap);
            s.supplier = Some(supplier);
        }
        binding.validate()?;
        for (index, purpose, spki) in [
            (0, SigningPurpose::Endpoint, binding.endpoint_spki),
            (1, SigningPurpose::Relay, binding.relay_spki),
        ] {
            #[cfg(test)]
            if index == 1 && _hooks.fail_second_spawn {
                if let Some(opened) = &_hooks.first_opened {
                    opened
                        .recv_timeout(Duration::from_secs(3))
                        .expect("first fixture opened");
                }
                return Err(SignFailure::Closed);
            }
            let (mailbox, cap) = {
                let s = self.core.state.lock();
                (
                    s.supplier.as_ref().expect("one frozen supplier").openers[index].clone(),
                    s.cap,
                )
            };
            if self.core.token.is_revoked() || Instant::now() >= cap {
                return Err(SignFailure::Closed);
            }
            let (owner, signal, start) = KeyCustodian::prepare_host(
                mailbox,
                purpose,
                spki.to_vec(),
                self.core.token.clone(),
                cap,
            )?;
            self.publish(index, owner, signal, start);
        }
        let (handles, cap) = {
            let s = self.core.state.lock();
            (
                [
                    s.slots[0].as_ref().expect("endpoint").signal.clone(),
                    s.slots[1].as_ref().expect("relay").signal.clone(),
                ],
                s.cap,
            )
        };
        let result = HostSigningStages::prepare(handles, cap, self.core.token.clone(), &binding)?;
        self.success = true;
        Ok(result)
    }
    fn publish(
        &self,
        index: usize,
        owner: KeyCustodian,
        signal: KeyRequestHandle,
        start: KeyStart,
    ) {
        let release = {
            let mut s = self.core.state.lock();
            s.slots[index] = Some(Box::new(Slot {
                owner,
                signal,
                sealed: false,
                #[cfg(test)]
                _last_drop: None,
            }));
            s.facts[index] = Some(KeyObservation {
                retirement: KeyRetirement::Pending,
                holds_original_join: true,
            });
            if self.core.token.is_revoked() || Instant::now() >= s.cap || s.sealed {
                self.core.seal_locked(&mut s);
                false
            } else {
                true
            }
        };
        if release {
            start.release();
        }
        self.core.changed.notify_all();
    }
    #[cfg(test)]
    pub(crate) fn test_publish(
        &self,
        index: usize,
        owner: KeyCustodian,
        signal: KeyRequestHandle,
        start: KeyStart,
    ) {
        self.publish(index, owner, signal, start);
    }
    #[cfg(test)]
    pub(crate) fn test_close(mut self) {
        self.success = true;
    }
    #[cfg(test)]
    pub(crate) fn test_last_drop(&self, index: usize, probe: Box<dyn FnOnce() + Send>) {
        self.core.state.lock().slots[index]
            .as_mut()
            .unwrap()
            ._last_drop = Some(LastDrop(probe));
    }
}
impl Drop for HostKeyPublisher {
    fn drop(&mut self) {
        self.core.close_publisher(self.success);
    }
}
