//! Native-only original canonical actor, initialized before the sole reader.
//! Local resource ownership is not a grant, Ready state or remote input proof.
use super::*;
use ring::rand::SecureRandom;
use std::sync::OnceLock;

static NATIVE_VIEW_BUDGET: OnceLock<HostViewBudget> = OnceLock::new();

/// The native application must supply its already-admitted original writer.
/// No JS/wire constructor, raw writer getter, peer identity or grant is exposed.
pub struct NativeRunProducer {
    pub(super) writer: LocalWriter,
    output: Option<HostOutputHandle>,
    complete: Arc<AtomicBool>,
    native_session: [u8; 16],
    peer_close: Arc<std::sync::Mutex<Option<BoundDataAuthority>>>,
    joined: Arc<std::sync::atomic::AtomicU8>,
}
#[derive(Clone)]
pub struct NativeLocalInput {
    input: LocalInputHandle,
    peer_close: Arc<std::sync::Mutex<Option<BoundDataAuthority>>>,
    joined: Arc<std::sync::atomic::AtomicU8>,
    complete: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeWriterRetirement {
    Pending,
    Joined,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCanonicalStatus {
    Complete,
    LocalOnly,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeProtocolResult {
    NativeOwned,
    Written(NativeFrameResult),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeFrameResult {
    pub complete: bool,
    pub confirmed_bytes: u64,
}
/// Rust-native original resource adapter. Its mutation region must revalidate
/// the admitted caller, original run Arc and this per-effect authorization.
/// It is never constructed from IPC/wire metadata.
pub trait NativeOriginalIo: Send {
    fn frame(
        &mut self,
        bytes: &[u8],
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<NativeFrameResult, Reject>;
    fn resize(
        &mut self,
        _rows: u16,
        _columns: u16,
        _authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<(), Reject> {
        Err(Reject::Unsupported)
    }
}
struct OriginalWrite<T>(T);
impl<T: Write + Send> NativeOriginalIo for OriginalWrite<T> {
    fn frame(
        &mut self,
        bytes: &[u8],
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<NativeFrameResult, Reject> {
        if !authorize() {
            return Err(Reject::Policy);
        }
        Ok(write_frame(&mut self.0, bytes))
    }
}
pub(in crate::local_writer) struct NativeLocalRequest {
    bytes: Box<[u8]>,
    authorize: Box<dyn FnOnce() -> bool + Send>,
    protocol: bool,
    protocol_source: Option<u64>,
    reply: mpsc::SyncSender<Result<NativeProtocolResult, Reject>>,
    _credit: Arc<Credit>,
}
pub(in crate::local_writer) struct NativeResizeRequest {
    rows: u16,
    columns: u16,
    local_only: bool,
    authorize: Box<dyn FnOnce() -> bool + Send>,
    reply: mpsc::SyncSender<Result<(), Reject>>,
    _credit: Arc<Credit>,
}
impl NativeResizeRequest {
    pub(in crate::local_writer) fn reject(self, e: Reject) {
        let _ = self.reply.try_send(Err(e));
    }
}
impl NativeLocalRequest {
    pub(in crate::local_writer) fn is_protocol(&self) -> bool {
        self.protocol
    }
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        let _ = self.reply.try_send(Err(error));
    }
}

// Exactly one worker owns the original physical writer. The canonical actor
// keeps each request/credit/intent and only polls its original result. No IO
// timeout transfers custody, and actor retirement joins this worker explicitly.
type NativeWork =
    Box<dyn FnOnce(&mut dyn NativeOriginalIo) -> Result<NativeFrameResult, Reject> + Send>;
pub(in crate::local_writer) type NativeStart = (
    Box<dyn NativeOriginalIo>,
    Arc<AtomicBool>,
    Arc<std::sync::Mutex<Option<BoundDataAuthority>>>,
);
enum NativeCompletion {
    Local {
        reply: mpsc::SyncSender<Result<NativeProtocolResult, Reject>>,
        _credit: Arc<Credit>,
    },
    Remote {
        request: Request,
        intent: PreparedOperation,
    },
    Output {
        request: OutputRequest,
        replies: u8,
    },
    Resize {
        request: NativeResizeRequest,
    },
}
pub(in crate::local_writer) struct NativeIoState {
    tx: Option<mpsc::SyncSender<NativeWork>>,
    rx: mpsc::Receiver<Result<NativeFrameResult, Reject>>,
    worker: Option<JoinHandle<()>>,
    pending: Option<NativeCompletion>,
    replies: std::collections::VecDeque<(NativeWork, NativeCompletion)>,
    complete: Arc<AtomicBool>,
    desktop_after: Option<u64>,
    peer_close: Arc<std::sync::Mutex<Option<BoundDataAuthority>>>,
}
fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> NativeFrameResult {
    let mut offset = 0;
    let mut interrupts = 16;
    while offset < bytes.len() {
        match writer.write(&bytes[offset..]) {
            Ok(n) if n > 0 && n <= bytes.len() - offset => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted && interrupts > 0 => {
                interrupts -= 1
            }
            _ => break,
        }
    }
    NativeFrameResult {
        complete: offset == bytes.len() && writer.flush().is_ok(),
        confirmed_bytes: offset as u64,
    }
}
impl NativeIoState {
    pub(in crate::local_writer) fn start(start: NativeStart) -> Result<Self, Reject> {
        let (mut writer, complete, peer_close) = start;
        let (tx, requests) = mpsc::sync_channel::<NativeWork>(1);
        let (results, rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("cc-native-original-io".into())
            .spawn(move || {
                while let Ok(work) = requests.recv() {
                    let result = work(writer.as_mut());
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| Reject::Thread)?;
        Ok(Self {
            tx: Some(tx),
            rx,
            worker: Some(worker),
            pending: None,
            replies: Default::default(),
            complete,
            desktop_after: None,
            peer_close,
        })
    }
    pub(in crate::local_writer) fn busy(&self) -> bool {
        self.pending.is_some() || !self.replies.is_empty()
    }
    pub(in crate::local_writer) fn qualified(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }
    pub(in crate::local_writer) fn track_authority(&self, gate: Option<&view_gate::ViewGate>) {
        if let Some(authority) = gate.and_then(view_gate::ViewGate::bound_authority) {
            *self.peer_close.lock().unwrap_or_else(|p| p.into_inner()) = Some(authority);
        }
    }
    pub(in crate::local_writer) fn local_only(
        &mut self,
        state: &ReplyState,
        gate: Option<&mut view_gate::ViewGate>,
    ) {
        if self.desktop_after.is_none() {
            self.desktop_after = Some(state.through_offset());
        }
        self.complete.store(false, Ordering::Release);
        if let Some(gate) = gate {
            gate.invalidate_for_resize();
            gate.scope().close();
        }
    }
    fn submit(&mut self, work: NativeWork, completion: NativeCompletion) {
        debug_assert!(self.pending.is_none());
        // The single slot is empty precisely when no original result is pending.
        if self.tx.as_ref().is_some_and(|tx| tx.try_send(work).is_ok()) {
            self.pending = Some(completion);
        } else {
            self.pending = Some(completion);
            self.tx.take();
        }
    }
    pub(in crate::local_writer) fn local(
        &mut self,
        request: NativeLocalRequest,
        sealed: Arc<AtomicBool>,
        gate: Option<&mut view_gate::ViewGate>,
        journal: &Journal,
    ) {
        let native_prefix = request
            .protocol_source
            .is_some_and(|source| self.desktop_after.is_some_and(|cut| source <= cut));
        if request.protocol && (self.complete.load(Ordering::Acquire) || native_prefix) {
            let result = if !sealed.load(Ordering::Acquire) && (request.authorize)() {
                Ok(NativeProtocolResult::NativeOwned)
            } else {
                Err(Reject::Policy)
            };
            let _ = request.reply.try_send(result);
            return;
        }
        if request.protocol && request.protocol_source.is_none() {
            request.reject(Reject::Policy);
            return;
        }
        if !request.protocol && self.busy() {
            request.reject(Reject::Pending);
            return;
        }
        if !request.protocol
            && let Some(gate) = gate
            && let Err(e) = gate.native_local_takeover(journal)
        {
            request.reject(e);
            return;
        }
        let NativeLocalRequest {
            bytes,
            authorize,
            reply,
            _credit,
            ..
        } = request;
        let work: NativeWork = Box::new(move |writer| {
            writer.frame(
                &bytes,
                Box::new(move || !sealed.load(Ordering::Acquire) && authorize()),
            )
        });
        let completion = NativeCompletion::Local { reply, _credit };
        if self.busy() {
            self.replies.push_back((work, completion));
        } else {
            self.submit(work, completion);
        }
    }
    pub(in crate::local_writer) fn remote(
        &mut self,
        mut request: Request,
        intent: PreparedOperation,
        authorize: Box<dyn FnOnce() -> bool + Send>,
        sealed: Arc<AtomicBool>,
    ) {
        let bytes = std::mem::take(&mut request.bytes);
        self.submit(
            Box::new(move |writer| {
                writer.frame(
                    &bytes,
                    Box::new(move || !sealed.load(Ordering::Acquire) && authorize()),
                )
            }),
            NativeCompletion::Remote { request, intent },
        );
    }
    pub(in crate::local_writer) fn resize(
        &mut self,
        mut request: NativeResizeRequest,
        sealed: Arc<AtomicBool>,
        gate: Option<&mut view_gate::ViewGate>,
        replies: Option<&ReplyState>,
        journal: &Journal,
    ) {
        if self.busy() {
            request.reject(Reject::Pending);
            return;
        }
        if self.complete.load(Ordering::Acquire) {
            match replies.ok_or(Reject::Unsupported).and_then(|r| {
                r.resize_preflight(request.rows, request.columns)
                    .map(|_| ())
            }) {
                Ok(()) => (),
                Err(Reject::Bounds) => request.local_only = true,
                Err(e) => {
                    request.reject(e);
                    return;
                }
            }
        } else {
            request.local_only = true;
        }
        if let Some(gate) = gate
            && let Err(e) = gate.native_local_takeover(journal)
        {
            request.reject(e);
            return;
        }
        let authorize = std::mem::replace(&mut request.authorize, Box::new(|| false));
        let (rows, columns) = (request.rows, request.columns);
        self.submit(
            Box::new(move |writer| {
                writer.resize(
                    rows,
                    columns,
                    Box::new(move || !sealed.load(Ordering::Acquire) && authorize()),
                )?;
                // Internal unit completion only; no input receipt is minted.
                Ok(NativeFrameResult {
                    complete: true,
                    confirmed_bytes: 0,
                })
            }),
            NativeCompletion::Resize { request },
        );
    }
    pub(in crate::local_writer) fn output(
        &mut self,
        request: OutputRequest,
        state: &mut ReplyState,
        sealed: &Arc<AtomicBool>,
        gate: Option<&mut view_gate::ViewGate>,
    ) {
        if !self.complete.load(Ordering::Acquire) {
            let _ = request.reply.try_send(OutputResult::LocalOnly);
            return;
        }
        // Parsing/encoding stays on the original actor. This bounded collector
        // does not publish an IO acknowledgement; the original OutputRequest is
        // retained until the one physical worker settles every encoded reply.
        struct Encoded(Vec<u8>);
        impl Write for Encoded {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                if self.0.len().saturating_add(b.len()) > MAX_INPUT {
                    return Err(std::io::ErrorKind::OutOfMemory.into());
                }
                self.0.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut encoded = Encoded(Vec::new());
        if sealed.load(Ordering::Acquire) {
            let _ = request
                .reply
                .try_send(OutputResult::Rejected(Reject::Sealed));
            return;
        }
        // A parser/projection failure is optional native projection loss. It
        // must never overwrite an explicit stop or a true native IO seal.
        let parser_sealed = AtomicBool::new(false);
        let result = state.process(&request, &mut encoded, &parser_sealed);
        match result {
            OutputResult::Processed { replies } if replies > 0 => {
                let seal = sealed.clone();
                let work: NativeWork = Box::new(move |writer| {
                    writer.frame(&encoded.0, Box::new(move || !seal.load(Ordering::Acquire)))
                });
                // Each queued batch retains its existing bounded output credit.
                self.replies
                    .push_back((work, NativeCompletion::Output { request, replies }));
            }
            OutputResult::Processed { replies: 0 } => {
                let _ = request
                    .reply
                    .try_send(OutputResult::Processed { replies: 0 });
            }
            result @ OutputResult::Rejected(
                Reject::Continuity | Reject::Bounds | Reject::Sealed,
            ) => {
                sealed.store(true, Ordering::Release);
                let _ = request.reply.try_send(result);
            }
            _ => {
                // No bytes from this collector have reached physical IO. Old
                // native reply batches stay ordered; this whole source frame
                // belongs to the existing desktop parser after the cut.
                self.local_only(state, gate);
                let _ = request.reply.try_send(OutputResult::LocalOnly);
            }
        }
    }
    pub(in crate::local_writer) fn poll(
        &mut self,
        journal: &mut Journal,
        sealed: &AtomicBool,
        replies: Option<&mut ReplyState>,
    ) {
        if self.pending.is_some() {
            let result = match self.rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(Reject::Thread)),
            };
            if let Some(result) = result {
                let completion = self.pending.take().expect("one original result");
                if result.as_ref().is_ok_and(|r| !r.complete)
                    || matches!(result, Err(Reject::Thread))
                {
                    sealed.store(true, Ordering::Release);
                }
                match completion {
                    NativeCompletion::Local { reply, .. } => {
                        let _ = reply.try_send(result.map(NativeProtocolResult::Written));
                    }
                    NativeCompletion::Output { request, replies } => {
                        let result = match result {
                            Ok(r) if r.complete => OutputResult::Processed { replies },
                            Ok(r) if r.confirmed_bytes > 0 => OutputResult::Partial,
                            Ok(_) => OutputResult::Unknown,
                            Err(e) => OutputResult::Rejected(e),
                        };
                        let _ = request.reply.try_send(result);
                    }
                    NativeCompletion::Remote { request, intent } => {
                        let outcome = match result {
                            Ok(r) if r.complete => Outcome::HostWritten,
                            Ok(r) if r.confirmed_bytes > 0 => Outcome::Partial,
                            Ok(_) | Err(Reject::Thread) => Outcome::Unknown,
                            Err(_) => Outcome::Rejected,
                        };
                        let outcome = journal.finalize(intent, outcome);
                        let feedback =
                            super::super::super::feedback(journal, &request.key, outcome, sealed);
                        let _ = request.reply.try_send(feedback);
                    }
                    NativeCompletion::Resize { request } => {
                        let result = result.map(|_| ()).and_then(|()| {
                            if request.local_only {
                                if self.desktop_after.is_none() {
                                    self.desktop_after = Some(
                                        replies
                                            .as_ref()
                                            .ok_or(Reject::Unsupported)?
                                            .through_offset(),
                                    );
                                }
                                self.complete.store(false, Ordering::Release);
                                Ok(())
                            } else {
                                replies
                                    .ok_or(Reject::Unsupported)?
                                    .resize_commit(request.rows, request.columns)
                            }
                        });
                        if result.is_err() && !matches!(result, Err(Reject::Policy)) {
                            sealed.store(true, Ordering::Release);
                        }
                        let _ = request.reply.try_send(result);
                    }
                }
            }
        }
        if sealed.load(Ordering::Acquire) {
            while let Some((_, completion)) = self.replies.pop_front() {
                match completion {
                    NativeCompletion::Output { request, .. } => {
                        let _ = request
                            .reply
                            .try_send(OutputResult::Rejected(Reject::Sealed));
                    }
                    NativeCompletion::Local { reply, .. } => {
                        let _ = reply.try_send(Err(Reject::Sealed));
                    }
                    _ => unreachable!("only bounded protocol work can wait behind original IO"),
                }
            }
        } else if self.pending.is_none()
            && let Some((work, completion)) = self.replies.pop_front()
        {
            self.submit(work, completion);
        }
    }
    pub(in crate::local_writer) fn shutdown(mut self) -> Result<(), Reject> {
        debug_assert!(!self.busy());
        self.tx.take();
        self.worker
            .take()
            .ok_or(Reject::Thread)?
            .join()
            .map_err(|_| Reject::Thread)
    }
}
impl NativeLocalInput {
    pub fn canonical_status(&self) -> NativeCanonicalStatus {
        if self.input.sealed.load(Ordering::Acquire) {
            NativeCanonicalStatus::Unavailable
        } else if self.complete.load(Ordering::Acquire) {
            NativeCanonicalStatus::Complete
        } else {
            NativeCanonicalStatus::LocalOnly
        }
    }
    pub fn protocol_from_source(
        &self,
        bytes: &[u8],
        through_offset: Option<u64>,
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<NativeProtocolResult, Reject> {
        self.submit(bytes, true, through_offset, Box::new(authorize))
    }
    pub fn protocol(
        &self,
        bytes: &[u8],
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<NativeProtocolResult, Reject> {
        self.protocol_from_source(bytes, None, authorize)
    }
    pub fn retirement(&self) -> NativeWriterRetirement {
        match self.joined.load(Ordering::Acquire) {
            1 => NativeWriterRetirement::Joined,
            2 => NativeWriterRetirement::Unknown,
            _ => NativeWriterRetirement::Pending,
        }
    }
    pub fn owns_protocol(&self) -> bool {
        !self.input.sealed.load(Ordering::Acquire) && self.complete.load(Ordering::Acquire)
    }
    pub fn resize(
        &self,
        rows: u16,
        columns: u16,
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<(), Reject> {
        if rows == 0 || columns == 0 {
            return Err(Reject::Bounds);
        }
        if self.input.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.input
            .tx
            .try_send(Command::NativeResize(NativeResizeRequest {
                rows,
                columns,
                local_only: false,
                authorize: Box::new(authorize),
                reply: tx,
                _credit: credit,
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                _ => Reject::Closed,
            })?;
        rx.recv().map_err(|_| Reject::Closed)?
    }
    /// Whole existing desktop frame; admission is rechecked in the same actor
    /// immediately before effect. No timeout can drop/replay the native result.
    pub fn commit(
        &self,
        bytes: &[u8],
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<NativeFrameResult, Reject> {
        match self.submit(bytes, false, None, Box::new(authorize))? {
            NativeProtocolResult::Written(r) => Ok(r),
            NativeProtocolResult::NativeOwned => Err(Reject::Policy),
        }
    }
    fn submit(
        &self,
        bytes: &[u8],
        protocol: bool,
        protocol_source: Option<u64>,
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<NativeProtocolResult, Reject> {
        if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
            return Err(Reject::Bounds);
        }
        if self.input.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let credit = self.input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.input
            .tx
            .try_send(Command::NativeLocal(NativeLocalRequest {
                bytes: bytes.into(),
                protocol,
                protocol_source,
                authorize,
                reply: tx,
                _credit: credit,
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                _ => Reject::Closed,
            })?;
        rx.recv().map_err(|_| Reject::Closed)?
    }
    pub fn stop(&self) {
        if let Some(a) = self.peer_close.lock().unwrap().as_ref() {
            a.request_close();
        }
        self.input.sealed.store(true, Ordering::Release);
        let _ = self.input.tx.try_send(Command::NativeWake);
    }
}
/// Sole-reader sink. Canonical backpressure must not deadlock native draining.
/// Lost continuity permanently refuses remote eligibility; bytes still reach UI.
pub struct NativeOutputIngress {
    #[cfg(test)]
    after_initial_qualified_load: Option<Box<dyn FnOnce() + Send>>,
    output: HostOutputHandle,
    pending: std::collections::VecDeque<replies::PendingOutput>,
    complete: Arc<AtomicBool>,
    peer_close: Arc<std::sync::Mutex<Option<BoundDataAuthority>>>,
}
impl NativeOutputIngress {
    #[cfg(test)]
    pub(super) fn test_after_initial_load(&mut self, hook: impl FnOnce() + Send + 'static) {
        self.after_initial_qualified_load = Some(Box::new(hook));
    }
    fn fail(&self) {
        self.complete.store(false, Ordering::Release);
        self.output.seal();
        if let Some(a) = self.peer_close.lock().unwrap().as_ref() {
            a.request_close();
        }
    }
    pub fn feed(&mut self, mut offset: u64, mut bytes: &[u8]) {
        if !self.complete.load(Ordering::Acquire) {
            self.pending.clear();
            return;
        }
        #[cfg(test)]
        if let Some(hook) = self.after_initial_qualified_load.take() {
            hook();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !bytes.is_empty() {
            let mut i = 0;
            while i < self.pending.len() {
                match self.pending[i].wait(Duration::ZERO) {
                    Ok(OutputResult::Processed { .. }) => {
                        self.pending.remove(i);
                    }
                    Ok(OutputResult::LocalOnly) if !self.complete.load(Ordering::Acquire) => {
                        // Only the original actor's explicit optional projection
                        // transition can relinquish this output capability.
                        // Never treat arbitrary Policy/continuity/IO failures
                        // as permission to reopen or keep a failed writer.
                        self.pending.clear();
                        return;
                    }
                    Err(WaitError::Uncertain) => i += 1,
                    _ => {
                        self.fail();
                        return;
                    }
                }
            }
            if !self.complete.load(Ordering::Acquire) {
                self.pending.clear();
                return;
            }
            let n = bytes.len().min(MAX_INPUT);
            match self.output.feed_at(offset, bytes[..n].to_vec()) {
                Ok(pending) => {
                    self.pending.push_back(pending);
                    offset += n as u64;
                    bytes = &bytes[n..];
                }
                Err(Reject::Full) if std::time::Instant::now() < deadline => {
                    std::thread::park_timeout(Duration::from_millis(1))
                }
                Err(_) => {
                    self.fail();
                    return;
                }
            }
        }
    }
}
impl NativeRunProducer {
    #[cfg(test)]
    pub(super) fn test_output(
        &mut self,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<replies::PendingOutput, Reject> {
        self.output
            .as_mut()
            .ok_or(Reject::Closed)?
            .feed_at(offset, bytes)
    }
    pub fn start(
        journal: Journal,
        original: impl Write + Send + 'static,
        run: String,
        generation: u32,
        rows: usize,
        columns: usize,
    ) -> Result<Self, Reject> {
        Self::start_resource(
            journal,
            OriginalWrite(original),
            run,
            generation,
            rows,
            columns,
        )
    }
    pub fn start_resource(
        journal: Journal,
        original: impl NativeOriginalIo + 'static,
        run: String,
        generation: u32,
        rows: usize,
        columns: usize,
    ) -> Result<Self, Reject> {
        if generation == 0 {
            return Err(Reject::Target);
        }
        let mut key = [0; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| Reject::Policy)?;
        let budget = NATIVE_VIEW_BUDGET
            .get_or_init(|| HostViewBudget::for_owner(Arc::new(owner::OwnerActivation)));
        // Unbound native namespace only. These sentinel fields can never be
        // emitted as authenticated peer identity or used to authorize Data.
        let target = Target {
            host: [0; 32],
            incarnation: 1,
            session: [0; 16],
            run,
            generation: generation.into(),
        };
        let mut native_session = [0; 16];
        ring::rand::SystemRandom::new()
            .fill(&mut native_session)
            .map_err(|_| Reject::Policy)?;
        let complete = Arc::new(AtomicBool::new(true));
        let peer_close = Arc::new(std::sync::Mutex::new(None));
        let (mut writer, output) = LocalWriter::start_canonical_terminal(
            journal,
            Vec::<u8>::new(),
            target,
            [0; 32],
            1,
            DigestKey::from_backend_bytes(key),
            CanonicalStart::new(rows, columns, budget)?.native(
                Box::new(original),
                complete.clone(),
                peer_close.clone(),
            ),
        )?;
        let joined = Arc::new(std::sync::atomic::AtomicU8::new(0));
        writer.native_joined = Some(joined.clone());
        Ok(Self {
            writer,
            output: Some(output),
            complete,
            native_session,
            peer_close,
            joined,
        })
    }
    pub fn native_scope(&self) -> cc_desk_remote_record::grant::SessionScope {
        cc_desk_remote_record::grant::SessionScope {
            session: self
                .native_session
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            run: self.writer.handle.target.run.clone(),
            generation: self.writer.handle.target.generation.to_string(),
        }
    }
    pub fn local_input(&self) -> NativeLocalInput {
        NativeLocalInput {
            input: self.writer.handle(),
            peer_close: self.peer_close.clone(),
            joined: self.joined.clone(),
            complete: self.complete.clone(),
        }
    }
    pub fn take_output(&mut self) -> Result<NativeOutputIngress, Reject> {
        Ok(NativeOutputIngress {
            #[cfg(test)]
            after_initial_qualified_load: None,
            output: self.output.take().ok_or(Reject::Closed)?,
            pending: Default::default(),
            complete: self.complete.clone(),
            peer_close: self.peer_close.clone(),
        })
    }
    pub fn complete_from_zero(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }
    /// Exact source offset on the sole native reader; no history reconstruction.
    pub fn output(&mut self, offset: u64, bytes: Vec<u8>) -> Result<(), Reject> {
        let result = self
            .output
            .as_mut()
            .ok_or(Reject::Closed)?
            .feed_at(offset, bytes)?
            .wait(Duration::from_secs(1));
        match result {
            Ok(OutputResult::Processed { .. }) => Ok(()),
            _ => {
                self.complete.store(false, Ordering::Release);
                self.writer.seal();
                Err(Reject::Continuity)
            }
        }
    }
    pub fn stop(&self) {
        self.local_input().stop();
    }
    /// Wait on the original actor only; never claims native Job/reader retirement.
    pub fn join(&mut self, budget: Duration) -> Result<(), Reject> {
        let end = std::time::Instant::now() + budget;
        loop {
            if self.writer.try_join()? {
                return Ok(());
            }
            if std::time::Instant::now() >= end {
                return Err(Reject::Pending);
            }
            std::thread::yield_now();
        }
    }
}

pub(in crate::local_writer) struct NativeBindRequest {
    pub(in crate::local_writer) input: LocalInputHandle,
    pub(in crate::local_writer) projection: ProjectionHandle,
    pub(in crate::local_writer) target: Target,
    pub(in crate::local_writer) device: [u8; 32],
    pub(in crate::local_writer) admission: view_gate::control::ControlAdmission,
    pub(in crate::local_writer) authorize: Box<dyn FnOnce() -> bool + Send>,
    pub(in crate::local_writer) complete: Arc<AtomicBool>,
    pub(in crate::local_writer) reply: mpsc::SyncSender<Result<view_gate::PeerViewHandle, Reject>>,
    pub(in crate::local_writer) _credit: Arc<Credit>,
}
impl NativeBindRequest {
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        let _ = self.reply.try_send(Err(error));
    }
}
impl NativeRunProducer {
    pub(super) fn bind(
        &self,
        target: Target,
        device: [u8; 32],
        admission: view_gate::control::ControlAdmission,
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<view_gate::rebind::PendingRebind, Reject> {
        let input = self.writer.handle();
        let credit = input.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        input
            .tx
            .try_send(Command::NativeBind(NativeBindRequest {
                input: input.clone(),
                projection: self
                    .writer
                    .projection
                    .as_ref()
                    .ok_or(Reject::Unsupported)?
                    .clone(),
                target,
                device,
                admission,
                authorize,
                complete: self.complete.clone(),
                reply: tx,
                _credit: credit.clone(),
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                _ => Reject::Closed,
            })?;
        Ok(view_gate::rebind::PendingRebind::from_native(rx, credit))
    }
    pub(super) fn retain_authority(&self, a: BoundDataAuthority) {
        *self.peer_close.lock().unwrap() = Some(a);
    }
}
