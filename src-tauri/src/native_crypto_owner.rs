//! Original per-key worker custody. RemoteClientService owns application retirement.
use super::{CngP256Signer, CryptoError, NativeOwnedSoftwareKey, SigningPurpose, MAX_SIGN_BYTES};
use parking_lot::Mutex;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    task::{Context, Poll, Waker},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Native profile/provider selection is not connected.
pub(crate) enum SignatureEncoding {
    Fixed,
    Der,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestFact {
    Queued,
    Started,
    Ready,
    Delivered,
    CancelledNoEffect,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyRetirement {
    Pending,
    Retired,
    NoKeyCreated,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignFailure {
    Busy,
    Closed,
    Bounds,
    CancelledNoEffect,
    Unknown,
    Native(CryptoError),
}
struct Active {
    id: u64,
    fact: RequestFact,
    waker: Option<Waker>,
}
struct Control {
    closing: bool,
    next: u64,
    active: Option<Active>,
    key_freed: bool,
    unknown: bool,
    late_discarded: bool,
}
struct Shared {
    purpose: SigningPurpose,
    spki: Vec<u8>,
    control: Mutex<Control>,
    send: SyncSender<Command>,
    signs: AtomicUsize,
    facades: AtomicUsize,
}
struct Request {
    attempt: Option<NativeSigningAttempt>,
    id: u64,
    end: Instant,
    message: Vec<u8>,
    encoding: SignatureEncoding,
    reply: oneshot::Sender<Result<Vec<u8>, SignFailure>>,
}
/// One concrete native signing authority, not a channel/grant lifetime.
pub(crate) struct SigningFence {
    generation: Mutex<u64>,
    approved_until: Instant,
}
#[derive(Clone)]
pub(crate) struct NativeSigningAttempt {
    #[cfg(test)]
    hooks: Arc<Mutex<AttemptHooks>>,
    fence: Arc<SigningFence>,
    generation: u64,
    end: Instant,
}
#[cfg(test)]
#[derive(Default)]
struct AttemptHooks {
    admission: Option<Box<dyn FnOnce() + Send>>,
    claim: Option<Box<dyn FnOnce() + Send>>,
    returned: Option<Box<dyn FnOnce() + Send>>,
}
impl NativeSigningAttempt {
    pub(crate) fn is_current(&self) -> bool {
        self.valid_locked(*self.fence.generation.lock())
    }
    fn valid_locked(&self, current: u64) -> bool {
        current == self.generation
            && Instant::now() < self.end
            && Instant::now() < self.fence.approved_until
    }
    pub(crate) fn end(&self) -> Instant {
        self.end
    }
    #[cfg(all(test, feature = "remote-client-ipc"))]
    pub(crate) fn same_policy(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.fence, &other.fence)
    }
    #[cfg(all(test, feature = "remote-client-ipc"))]
    pub(crate) fn approved_until(&self) -> Instant {
        self.fence.approved_until
    }
    #[cfg(all(test, feature = "remote-client-ipc"))]
    pub(crate) fn hook_admission(&self, hook: Box<dyn FnOnce() + Send>) {
        self.hooks.lock().admission = Some(hook);
    }
    #[cfg(all(test, feature = "remote-client-ipc"))]
    pub(crate) fn hook_claim(&self, hook: Box<dyn FnOnce() + Send>) {
        self.hooks.lock().claim = Some(hook);
    }
    #[cfg(all(test, feature = "remote-client-ipc"))]
    pub(crate) fn hook_returned(&self, hook: Box<dyn FnOnce() + Send>) {
        self.hooks.lock().returned = Some(hook);
    }
    #[cfg(test)]
    fn before_admission(&self) {
        let hook = self.hooks.lock().admission.take();
        if let Some(hook) = hook {
            hook();
        }
    }
    #[cfg(test)]
    fn before_claim(&self) {
        let hook = self.hooks.lock().claim.take();
        if let Some(hook) = hook {
            hook();
        }
    }
    #[cfg(test)]
    fn after_native(&self) {
        let hook = self.hooks.lock().returned.take();
        if let Some(hook) = hook {
            hook();
        }
    }
}
impl SigningFence {
    #[cfg(feature = "remote-client-ipc")]
    pub(crate) fn new(approved_until: Instant) -> Arc<Self> {
        Arc::new(Self {
            generation: Mutex::new(0),
            approved_until,
        })
    }
    #[cfg(feature = "remote-client-ipc")]
    pub(crate) fn advance(
        self: &Arc<Self>,
        handles: &[KeyRequestHandle],
        end: Instant,
    ) -> Result<NativeSigningAttempt, SignFailure> {
        let mut generation = self.generation.lock();
        if end > self.approved_until
            || end
                .checked_duration_since(Instant::now())
                .is_none_or(|d| d > Duration::from_secs(5))
        {
            return Err(SignFailure::Bounds);
        }
        for handle in handles {
            let c = handle.shared.control.lock();
            if c.closing || c.unknown {
                return Err(SignFailure::Unknown);
            }
            if c.active.is_some() {
                return Err(SignFailure::Busy);
            }
        }
        let next = generation.checked_add(1).ok_or(SignFailure::Bounds)?;
        *generation = next;
        Ok(NativeSigningAttempt {
            #[cfg(test)]
            hooks: Arc::new(Mutex::new(AttemptHooks::default())),
            fence: self.clone(),
            generation: next,
            end,
        })
    }
}
enum Command {
    Sign(Request),
    Shutdown,
}
impl Shared {
    fn cancel_observer(&self, id: u64) -> RequestFact {
        let (fact, waker, discarded, close) = {
            let mut c = self.control.lock();
            match c.active.as_mut().filter(|a| a.id == id) {
                None => return RequestFact::Delivered,
                Some(a)
                    if matches!(a.fact, RequestFact::Queued | RequestFact::CancelledNoEffect) =>
                {
                    a.fact = RequestFact::CancelledNoEffect;
                    (a.fact, a.waker.take(), None, false)
                }
                Some(a) if a.fact == RequestFact::Ready => {
                    let old = c.active.take();
                    (RequestFact::Unknown, None, old, false)
                }
                Some(_) => (RequestFact::Unknown, None, None, true),
            }
        };
        drop(discarded);
        if let Some(waker) = waker {
            waker.wake();
        }
        if close {
            self.cancel(Some(id))
        } else {
            fact
        }
    }
    fn cancel(&self, id: Option<u64>) -> RequestFact {
        let (fact, waker) = {
            let mut c = self.control.lock();
            if id.is_some_and(|id| c.active.as_ref().is_none_or(|a| a.id != id)) {
                return RequestFact::Delivered;
            }
            c.closing = true;
            let mut fact = RequestFact::Delivered;
            let mut waker = None;
            if let Some(active) = c.active.as_mut() {
                active.fact = match active.fact {
                    RequestFact::Queued | RequestFact::CancelledNoEffect => {
                        RequestFact::CancelledNoEffect
                    }
                    _ => RequestFact::Unknown,
                };
                fact = active.fact;
                waker = active.waker.take();
            }
            (fact, waker)
        };
        // Queue is bounded. A full slot already wakes the worker; that original
        // request will see closing. No lock is held over waits/native calls.
        let _ = self.send.try_send(Command::Shutdown);
        if let Some(waker) = waker {
            waker.wake();
        }
        fact
    }
    fn fact(&self, id: u64) -> RequestFact {
        self.control
            .lock()
            .active
            .as_ref()
            .filter(|a| a.id == id)
            .map_or(RequestFact::Delivered, |a| a.fact)
    }
}

/// Cloneable metadata/request capability, never original native key or join custody.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct KeyRequestHandle {
    shared: Arc<Shared>,
}
impl Clone for KeyRequestHandle {
    fn clone(&self) -> Self {
        self.shared.facades.fetch_add(1, Ordering::AcqRel);
        Self {
            shared: self.shared.clone(),
        }
    }
}
impl Drop for KeyRequestHandle {
    fn drop(&mut self) {
        if self.shared.facades.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.shared.cancel(None);
        }
    }
}
#[cfg_attr(not(test), allow(dead_code))]
impl KeyRequestHandle {
    pub(crate) fn binding(&self) -> (SigningPurpose, &[u8]) {
        (self.shared.purpose, &self.shared.spki)
    }
    #[cfg(feature = "remote-client-ipc")]
    pub(crate) fn shutdown(&self) {
        self.shared.cancel(None);
    }
    #[cfg(test)]
    pub(crate) fn control_available(&self) -> bool {
        self.shared.control.try_lock().is_some()
    }
    pub(crate) fn begin(
        &self,
        message: Vec<u8>,
        encoding: SignatureEncoding,
    ) -> Result<SignTicket, SignFailure> {
        self.begin_with_budget(message, encoding, Duration::from_secs(5))
    }
    pub(crate) fn begin_with_budget(
        &self,
        message: Vec<u8>,
        encoding: SignatureEncoding,
        budget: Duration,
    ) -> Result<SignTicket, SignFailure> {
        if message.len() > MAX_SIGN_BYTES || budget.is_zero() || budget > Duration::from_secs(5) {
            return Err(SignFailure::Bounds);
        }
        // Compute once outside any runtime. Sleep is created only inside poll.
        let end = Instant::now()
            .checked_add(budget)
            .ok_or(SignFailure::Bounds)?;
        self.begin_until(message, encoding, end)
    }
    pub(crate) fn begin_until(
        &self,
        message: Vec<u8>,
        encoding: SignatureEncoding,
        end: Instant,
    ) -> Result<SignTicket, SignFailure> {
        self.begin_inner(message, encoding, end, None)
    }
    pub(crate) fn begin_attempt(
        &self,
        message: Vec<u8>,
        encoding: SignatureEncoding,
        attempt: NativeSigningAttempt,
    ) -> Result<SignTicket, SignFailure> {
        #[cfg(test)]
        attempt.before_admission();
        self.begin_inner(message, encoding, attempt.end, Some(attempt))
    }
    fn begin_inner(
        &self,
        message: Vec<u8>,
        encoding: SignatureEncoding,
        end: Instant,
        attempt: Option<NativeSigningAttempt>,
    ) -> Result<SignTicket, SignFailure> {
        let gate = attempt.as_ref().map(|a| a.fence.generation.lock());
        if attempt
            .as_ref()
            .zip(gate.as_ref())
            .is_some_and(|(a, g)| !a.valid_locked(**g))
        {
            return Err(SignFailure::Closed);
        }
        let remaining = end
            .checked_duration_since(Instant::now())
            .ok_or(SignFailure::Bounds)?;
        if message.len() > MAX_SIGN_BYTES || remaining > Duration::from_secs(5) {
            return Err(SignFailure::Bounds);
        }
        let (reply, receive) = oneshot::channel();
        let mut c = self.shared.control.lock();
        if c.closing {
            return Err(SignFailure::Closed);
        }
        if c.active.is_some() {
            return Err(SignFailure::Busy);
        }
        let id = c.next;
        c.next = c.next.checked_add(1).ok_or(SignFailure::Bounds)?;
        c.active = Some(Active {
            id,
            fact: RequestFact::Queued,
            waker: None,
        });
        if self
            .shared
            .send
            .try_send(Command::Sign(Request {
                attempt: attempt.clone(),
                id,
                end,
                message,
                encoding,
                reply,
            }))
            .is_err()
        {
            c.closing = true;
            c.active.as_mut().unwrap().fact = RequestFact::Unknown;
            return Err(SignFailure::Closed);
        }
        Ok(SignTicket {
            attempt: attempt.clone(),
            shared: self.shared.clone(),
            id,
            end,
            receive,
            timer: None,
            done: false,
            #[cfg(test)]
            delivery_hook: None,
        })
    }
}
/// No native key/JoinHandle is owned by this future. Drop closes eligibility and
/// signals the original worker, but cannot preempt synchronous NCrypt.
pub(crate) struct SignTicket {
    attempt: Option<NativeSigningAttempt>,
    shared: Arc<Shared>,
    id: u64,
    end: Instant,
    receive: oneshot::Receiver<Result<Vec<u8>, SignFailure>>,
    timer: Option<Pin<Box<tokio::time::Sleep>>>,
    done: bool,
    #[cfg(test)]
    delivery_hook: Option<Box<dyn FnOnce() + Send>>,
}
impl SignTicket {
    fn deliver(
        &mut self,
        result: Option<Result<Vec<u8>, SignFailure>>,
    ) -> Result<Vec<u8>, SignFailure> {
        #[cfg(test)]
        if let Some(hook) = self.delivery_hook.take() {
            hook();
        }
        let attempt = self.attempt.clone();
        let gate = attempt.as_ref().map(|a| a.fence.generation.lock());
        let valid = attempt
            .as_ref()
            .zip(gate.as_ref())
            .is_none_or(|(a, g)| a.valid_locked(**g));
        let (value, close, delivered) = {
            let mut c = self.shared.control.lock();
            match c
                .active
                .as_ref()
                .filter(|a| a.id == self.id)
                .map(|a| a.fact)
            {
                Some(RequestFact::Ready) if !c.closing && valid && Instant::now() < self.end => {
                    match result {
                        Some(value) => {
                            let old = c.active.take();
                            (value, false, old)
                        }
                        None => (Err(SignFailure::Unknown), true, None),
                    }
                }
                Some(RequestFact::CancelledNoEffect) => {
                    (Err(SignFailure::CancelledNoEffect), false, None)
                }
                None if self.attempt.is_some()
                    && matches!(result, Some(Err(SignFailure::CancelledNoEffect))) =>
                {
                    (Err(SignFailure::CancelledNoEffect), false, None)
                }
                _ => (Err(SignFailure::Unknown), true, None),
            }
        };
        drop(gate);
        drop(delivered);
        self.clear_notification();
        if close {
            self.cancel();
        }
        self.done = true;
        value
    }
    /// rustls Signer is synchronous. This explicitly blocks its calling thread;
    /// it does NOT make an enclosing async TLS deadline preemptible. The key and
    /// original JoinHandle stay in the separate app custodian on timeout.
    pub(crate) fn wait_blocking(mut self) -> Result<Vec<u8>, SignFailure> {
        loop {
            if let Some(error) = self.cancelled() {
                self.cancel();
                self.done = true;
                return Err(error);
            }
            if Instant::now() >= self.end {
                self.cancel();
                self.done = true;
                return Err(self.cancelled().unwrap_or(SignFailure::Unknown));
            }
            match self.receive.try_recv() {
                Ok(result) => {
                    return self.deliver(Some(result));
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.cancel();
                    self.done = true;
                    return Err(SignFailure::Unknown);
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(1))
                }
            }
        }
    }
    fn clear_notification(&self) {
        let old = {
            let mut c = self.shared.control.lock();
            c.active
                .as_mut()
                .filter(|a| a.id == self.id)
                .and_then(|a| a.waker.take())
        };
        drop(old);
    }
    #[cfg(test)]
    pub(crate) fn expire_at_delivery(&mut self) {
        let end = self.end;
        self.delivery_hook = Some(Box::new(move || {
            if let Some(remaining) = end.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining);
            }
        }));
    }
    pub(crate) fn cancel(&self) -> RequestFact {
        if self.attempt.is_some() {
            self.shared.cancel_observer(self.id)
        } else {
            self.shared.cancel(Some(self.id))
        }
    }
    fn cancelled(&self) -> Option<SignFailure> {
        let fact = self.shared.fact(self.id);
        if fact == RequestFact::Unknown {
            return Some(SignFailure::Unknown);
        }
        if self.attempt.as_ref().is_some_and(|a| !a.is_current()) {
            return Some(
                if matches!(fact, RequestFact::Started | RequestFact::Ready) {
                    SignFailure::Unknown
                } else {
                    SignFailure::Closed
                },
            );
        }
        match fact {
            RequestFact::CancelledNoEffect => Some(SignFailure::CancelledNoEffect),
            RequestFact::Unknown => Some(SignFailure::Unknown),
            _ => None,
        }
    }
}
impl Drop for SignTicket {
    fn drop(&mut self) {
        if !self.done {
            self.cancel();
        }
    }
}
impl Future for SignTicket {
    type Output = Result<Vec<u8>, SignFailure>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(Err(SignFailure::Closed));
        }
        // RawWaker clone/drop callbacks are arbitrary code too.
        let mut incoming = Some(cx.waker().clone());
        let previous = {
            let mut c = this.shared.control.lock();
            if let Some(a) = c.active.as_mut().filter(|a| {
                a.id == this.id
                    && matches!(
                        a.fact,
                        RequestFact::Queued | RequestFact::Started | RequestFact::Ready
                    )
            }) {
                a.waker.replace(incoming.take().unwrap())
            } else {
                None
            }
        };
        drop(previous);
        drop(incoming);
        if let Some(error) = this.cancelled() {
            this.cancel();
            this.clear_notification();
            this.done = true;
            return Poll::Ready(Err(error));
        }
        if Instant::now() >= this.end {
            this.cancel();
            this.done = true;
            return Poll::Ready(Err(this.cancelled().unwrap_or(SignFailure::Unknown)));
        }
        let timer = this.timer.get_or_insert_with(|| {
            Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(
                this.end,
            )))
        });
        if timer.as_mut().poll(cx).is_ready() {
            this.cancel();
            this.done = true;
            return Poll::Ready(Err(this.cancelled().unwrap_or(SignFailure::Unknown)));
        }
        match Pin::new(&mut this.receive).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => Poll::Ready(this.deliver(result.ok())),
        }
    }
}

#[derive(Default)]
pub(crate) struct WorkerHooks {
    #[cfg(test)]
    pub(crate) before_claim: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(crate) after_native: Option<Box<dyn FnOnce() + Send>>,
}
struct WorkerEnd {
    key: Option<KeyEnd>,
    normal: bool,
}
enum KeyEnd {
    NotCreated,
    Freed,
}
pub(crate) struct KeyStart(Option<SyncSender<()>>);
impl KeyStart {
    pub(crate) fn release(mut self) {
        if let Some(gate) = self.0.take() {
            let _ = gate.try_send(());
        }
    }
}
/// Must live independently of async tickets/facades, through explicit observation
/// of Free+original join. Application Quit registration is NOT supplied here.
#[must_use = "Retain the original key custodian through explicit retirement"]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct KeyCustodian {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<WorkerEnd>>,
    retirement: KeyRetirement,
}
#[cfg_attr(not(test), allow(dead_code))]
impl KeyCustodian {
    pub(crate) fn start(
        factory: impl FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send + 'static,
        purpose: SigningPurpose,
        spki: Vec<u8>,
    ) -> Result<(Self, KeyRequestHandle), SignFailure> {
        Self::start_with_hooks(factory, purpose, spki, WorkerHooks::default())
    }
    #[cfg(test)]
    pub(crate) fn start_for_test(
        factory: impl FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send + 'static,
        purpose: SigningPurpose,
        spki: Vec<u8>,
        hooks: WorkerHooks,
    ) -> Result<(Self, KeyRequestHandle), SignFailure> {
        Self::start_with_hooks(factory, purpose, spki, hooks)
    }
    fn start_with_hooks(
        factory: impl FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send + 'static,
        purpose: SigningPurpose,
        spki: Vec<u8>,
        hooks: WorkerHooks,
    ) -> Result<(Self, KeyRequestHandle), SignFailure> {
        let (owner, handle, start) = Self::prepare_with_hooks(factory, purpose, spki, hooks)?;
        start.release();
        Ok((owner, handle))
    }
    #[cfg(feature = "remote-client-ipc")]
    pub(crate) fn prepare(
        factory: impl FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send + 'static,
        purpose: SigningPurpose,
        spki: Vec<u8>,
    ) -> Result<(Self, KeyRequestHandle, KeyStart), SignFailure> {
        Self::prepare_with_hooks(factory, purpose, spki, WorkerHooks::default())
    }
    fn prepare_with_hooks(
        factory: impl FnOnce() -> Result<NativeOwnedSoftwareKey, CryptoError> + Send + 'static,
        purpose: SigningPurpose,
        spki: Vec<u8>,
        hooks: WorkerHooks,
    ) -> Result<(Self, KeyRequestHandle, KeyStart), SignFailure> {
        if spki.len() != 91 {
            return Err(SignFailure::Bounds);
        }
        let (send, receive) = mpsc::sync_channel(1);
        let (release, start) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared {
            purpose,
            spki: spki.clone(),
            control: Mutex::new(Control {
                closing: false,
                next: 1,
                active: None,
                key_freed: false,
                unknown: false,
                late_discarded: false,
            }),
            send,
            signs: AtomicUsize::new(0),
            facades: AtomicUsize::new(1),
        });
        let original = shared.clone();
        let worker = std::thread::Builder::new()
            .name("native-cng-key".into())
            .spawn(move || {
                let admitted = loop {
                    if original.control.lock().closing {
                        break false;
                    }
                    match start.recv_timeout(Duration::from_millis(10)) {
                        Ok(()) => break !original.control.lock().closing,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break false,
                    }
                };
                if !admitted {
                    return WorkerEnd {
                        key: Some(KeyEnd::NotCreated),
                        normal: true,
                    };
                }
                let mut key = match factory() {
                    Ok(key) => key,
                    Err(_) => {
                        let mut c = original.control.lock();
                        c.closing = true;
                        c.unknown = true;
                        return WorkerEnd {
                            key: None,
                            normal: false,
                        };
                    }
                };
                let normal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_requests(&mut key, &original, receive, purpose, &spki, hooks)
                }))
                .is_ok();
                if !normal {
                    original.cancel(None);
                    original.control.lock().unknown = true;
                }
                if key.try_retire().is_err() {
                    original.cancel(None);
                    original.control.lock().unknown = true;
                    let _original_key = key;
                    loop {
                        std::thread::park();
                    }
                }
                original.control.lock().key_freed = true;
                WorkerEnd {
                    key: Some(KeyEnd::Freed),
                    normal,
                }
            })
            .map_err(|_| SignFailure::Closed)?;
        let custodian = Self {
            shared: shared.clone(),
            worker: Some(worker),
            retirement: KeyRetirement::Pending,
        };
        // The original JoinHandle is in custodian BEFORE factory/key creation.
        Ok((
            custodian,
            KeyRequestHandle { shared },
            KeyStart(Some(release)),
        ))
    }
    pub(crate) fn shutdown(&self) {
        self.shared.cancel(None);
    }
    pub(crate) fn observe(&mut self) -> KeyRetirement {
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let result = self.worker.take().unwrap().join();
            self.retirement = match result {
                Ok(WorkerEnd {
                    key: Some(KeyEnd::Freed),
                    normal: true,
                }) => KeyRetirement::Retired,
                Ok(WorkerEnd {
                    key: Some(KeyEnd::NotCreated),
                    normal: true,
                }) => KeyRetirement::NoKeyCreated,
                _ => KeyRetirement::Unknown,
            };
        }
        if self.shared.control.lock().unknown {
            KeyRetirement::Unknown
        } else {
            self.retirement
        }
    }
    pub(crate) fn native_sign_count(&self) -> usize {
        self.shared.signs.load(Ordering::Acquire)
    }
    pub(crate) fn request_fact(&self) -> RequestFact {
        self.shared
            .control
            .lock()
            .active
            .as_ref()
            .map_or(RequestFact::Delivered, |a| a.fact)
    }
    pub(crate) fn late_discarded(&self) -> bool {
        self.shared.control.lock().late_discarded
    }
    pub(crate) fn holds_original_join(&self) -> bool {
        self.worker.is_some()
    }
    pub(crate) fn key_freed(&self) -> bool {
        self.shared.control.lock().key_freed
    }
}
impl Drop for KeyCustodian {
    fn drop(&mut self) {
        self.shutdown();
        // Blocking last-resort custody, never an async facade/future destructor.
        // A blocked/Unknown native owner remains owned, not detached/Retired.
        // Real application integration must retain this outside async/UI tasks.
        if let Some(original) = self.worker.take() {
            let _ = original.join();
        }
    }
}
fn run_requests(
    key: &mut NativeOwnedSoftwareKey,
    shared: &Shared,
    receive: Receiver<Command>,
    purpose: SigningPurpose,
    spki: &[u8],
    mut hooks: WorkerHooks,
) {
    loop {
        if shared.control.lock().closing {
            break;
        }
        let Ok(Command::Sign(request)) = receive.recv() else {
            break;
        };
        #[cfg(test)]
        if let Some(hook) = hooks.before_claim.take() {
            hook();
        }
        #[cfg(not(test))]
        let _ = &mut hooks;
        #[cfg(test)]
        if let Some(attempt) = &request.attempt {
            attempt.before_claim();
        }
        let claimed = {
            let gate = request.attempt.as_ref().map(|a| a.fence.generation.lock());
            let mut c = shared.control.lock();
            if Instant::now() >= request.end
                || request
                    .attempt
                    .as_ref()
                    .zip(gate.as_ref())
                    .is_some_and(|(a, g)| !a.valid_locked(**g))
            {
                if request.attempt.is_none() {
                    c.closing = true;
                }
                if let Some(a) = c
                    .active
                    .as_mut()
                    .filter(|a| a.id == request.id && a.fact == RequestFact::Queued)
                {
                    a.fact = RequestFact::CancelledNoEffect;
                }
            }
            if c.closing {
                false
            } else if let Some(a) = c
                .active
                .as_mut()
                .filter(|a| a.id == request.id && a.fact == RequestFact::Queued)
            {
                a.fact = RequestFact::Started;
                true
            } else {
                false
            }
        };
        if !claimed {
            let discarded = if request.attempt.is_some() {
                let mut c = shared.control.lock();
                if c.active
                    .as_ref()
                    .is_some_and(|a| a.id == request.id && a.fact == RequestFact::CancelledNoEffect)
                {
                    c.active.take()
                } else {
                    None
                }
            } else {
                None
            };
            drop(discarded);
            let _ = request.reply.send(Err(SignFailure::CancelledNoEffect));
            continue;
        }
        let result = match CngP256Signer::admit(key, purpose, spki) {
            Ok(mut signer) => {
                shared.signs.fetch_add(1, Ordering::AcqRel);
                match request.encoding {
                    SignatureEncoding::Fixed => signer.sign_fixed(purpose, &request.message),
                    SignatureEncoding::Der => signer.sign_der(purpose, &request.message),
                }
                .map_err(SignFailure::Native)
            }
            Err(error) => Err(SignFailure::Native(error)),
        };
        #[cfg(test)]
        if let Some(hook) = hooks.after_native.take() {
            hook();
        }
        #[cfg(test)]
        if let Some(attempt) = &request.attempt {
            attempt.after_native();
        }
        let gate = request.attempt.as_ref().map(|a| a.fence.generation.lock());
        let mut c = shared.control.lock();
        if Instant::now() >= request.end
            || result.is_err()
            || request
                .attempt
                .as_ref()
                .zip(gate.as_ref())
                .is_some_and(|(a, g)| !a.valid_locked(**g))
        {
            c.closing = true;
        }
        if c.closing {
            if let Some(a) = c.active.as_mut().filter(|a| a.id == request.id) {
                if a.fact != RequestFact::CancelledNoEffect {
                    a.fact = RequestFact::Unknown;
                }
            }
            c.late_discarded = result.is_ok();
            drop(c);
            drop(gate);
            let _ = request.reply.send(Err(SignFailure::Unknown));
            continue;
        }
        if let Some(a) = c.active.as_mut().filter(|a| a.id == request.id) {
            a.fact = RequestFact::Ready;
        }
        // oneshot::send may synchronously invoke an arbitrary Waker.
        // Commit Ready first, then hand off with no control lock held.
        drop(c);
        drop(gate);
        if request.reply.send(result).is_err() {
            if request.attempt.is_some() {
                shared.cancel_observer(request.id);
                continue;
            }
            let mut c = shared.control.lock();
            if let Some(a) = c.active.as_mut().filter(|a| a.id == request.id) {
                a.fact = RequestFact::Unknown;
                c.closing = true;
            }
        }
    }
}
