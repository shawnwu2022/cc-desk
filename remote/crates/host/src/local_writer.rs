//! 默认关闭的本地 pipe 写入前置；不提供远程授权、PTY 或启动策略。
use crate::journal::*;
#[path = "local_writer_replies.rs"]
mod replies;
#[cfg(feature = "experimental-canonical-replies")]
use replies::CanonicalStart;
use replies::{HostOutputHandle, OutputRequest, OutputResult, ReplyState};
#[cfg(feature = "experimental-host-owner")]
#[path = "local_writer_owner.rs"]
mod owner;
#[cfg(feature = "experimental-host-owner")]
#[path = "local_writer_view_gate.rs"]
mod view_gate;
#[cfg(feature = "experimental-host-owner")]
type InputView = view_gate::SyncedView;
#[cfg(not(feature = "experimental-host-owner"))]
type InputView = ();
#[cfg(feature = "experimental-host-owner")]
type InputPeerScope = view_gate::PeerScope;
#[cfg(not(feature = "experimental-host-owner"))]
type InputPeerScope = ();
#[cfg(feature = "experimental-canonical-replies")]
#[path = "local_writer_projection.rs"]
mod projection;
#[cfg(feature = "experimental-canonical-replies")]
use projection::{HostViewBudget, PendingProjection, ProjectionHandle, ProjectionRequest};
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
pub const MAX_INPUT: usize = 16384;
pub const QUEUE: usize = 8;
const MAX_CREDITS: usize = QUEUE + 1;
#[derive(Default)]
struct Credits {
    used: AtomicUsize,
}
struct Credit {
    pool: Arc<Credits>,
}
impl Credits {
    fn reserve(self: &Arc<Self>) -> Result<Arc<Credit>, Reject> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|next| *next <= MAX_CREDITS)
            })
            .map_err(|_| Reject::Full)?;
        Ok(Arc::new(Credit { pool: self.clone() }))
    }
}
impl Drop for Credit {
    fn drop(&mut self) {
        let previous = self.pool.used.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    Bounds,
    Target,
    Device,
    Epoch,
    Sealed,
    Full,
    Closed,
    Unsupported,
    Thread,
    Rate,
    Continuity,
    Sync,
    Policy,
    Pending,
}
#[derive(Debug, PartialEq, Eq)]
pub enum WriteResult {
    Receipt(ReceiptEnvelope),
    Rejected(Reject),
    Journal(JournalError),
    Partial,
    Unknown,
}
/// 来自后端的摘要 key，不是设备授权；不实现 Debug/Serialize 或导出接口。
pub use crate::journal_digest::DigestKey;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitError {
    Uncertain,
    Closed,
}
pub struct PendingInput {
    reply: mpsc::Receiver<WriteResult>,
    _credit: Arc<Credit>,
}
impl PendingInput {
    /// 超时/反馈丢失不能证明未写；可继续等同一反馈，不自动提交新操作。
    pub fn wait(&self, budget: Duration) -> Result<WriteResult, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
struct Request {
    key: OperationKey,
    epoch: u64,
    bytes: Box<[u8]>,
    view: Option<InputView>,
    source_scope: Option<Arc<InputPeerScope>>,
    reply: mpsc::SyncSender<WriteResult>,
    // 声明在最后，保证元数据/payload/channel 先释放，再归还资格。
    _credit: Arc<Credit>,
}
enum Command {
    Input(Request),
    Output(OutputRequest),
    #[cfg(feature = "experimental-canonical-replies")]
    Projection(ProjectionRequest),
    #[cfg(feature = "experimental-host-owner")]
    ViewAck(view_gate::ViewAckRequest),
    #[cfg(feature = "experimental-host-owner")]
    Control(view_gate::control::ControlRequest),
    #[cfg(feature = "experimental-host-owner")]
    Rebind(Box<view_gate::rebind::RebindRequest>),
    #[cfg(feature = "experimental-host-owner")]
    ControlStatus(view_gate::rebind::StatusRequest),
}
#[derive(Clone)]
pub struct LocalInputHandle {
    tx: mpsc::SyncSender<Command>,
    target: Arc<Target>,
    device: [u8; 32],
    epoch: u64,
    sealed: Arc<AtomicBool>,
    credits: Arc<Credits>,
    view_scope: Option<Arc<InputPeerScope>>,
}
impl LocalInputHandle {
    pub fn submit(
        &self,
        key: OperationKey,
        epoch: u64,
        bytes: Vec<u8>,
    ) -> Result<PendingInput, Reject> {
        self.submit_observed(key, epoch, bytes, || {})
    }
    fn submit_observed(
        &self,
        key: OperationKey,
        epoch: u64,
        bytes: Vec<u8>,
        after_copy: impl FnOnce(),
    ) -> Result<PendingInput, Reject> {
        self.submit_inner(key, epoch, bytes, None, after_copy)
    }
    fn submit_inner(
        &self,
        mut key: OperationKey,
        epoch: u64,
        bytes: Vec<u8>,
        view: Option<InputView>,
        after_copy: impl FnOnce(),
    ) -> Result<PendingInput, Reject> {
        self.check(&key, epoch, bytes.len())?;
        if self.view_scope.is_some() && view.is_none() {
            return Err(Reject::Sync);
        }
        #[cfg(feature = "experimental-host-owner")]
        if self.view_scope.as_ref().is_some_and(|s| !s.is_active()) {
            return Err(Reject::Sync);
        }
        // 资格覆盖 compact copy、pre-send、队列、当前写及尚未释放的反馈。
        let credit = self.credits.reserve()?;
        // 不保留 caller 大 capacity；在旧 allocation 仍存活时新建有界紧凑副本。
        key.run = key.run.as_str().to_owned();
        let bytes: Box<[u8]> = bytes.as_slice().into();
        let (tx, rx) = mpsc::sync_channel(1);
        let request = Request {
            key,
            epoch,
            bytes,
            view,
            source_scope: self.view_scope.clone(),
            reply: tx,
            _credit: credit.clone(),
        };
        let pending = PendingInput {
            reply: rx,
            _credit: credit,
        };
        after_copy();
        self.tx
            .try_send(Command::Input(request))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(pending)
    }
    fn check(&self, key: &OperationKey, epoch: u64, len: usize) -> Result<(), Reject> {
        validate(
            &self.target,
            &self.device,
            self.epoch,
            key,
            epoch,
            len,
            &self.sealed,
        )
    }
    /// 本片未接 PTY/规范化尺寸状态，固定拒绝；不经任何旁路 IO。
    pub fn resize(
        &self,
        _target: &Target,
        _epoch: u64,
        _cols: u16,
        _rows: u16,
    ) -> Result<(), Reject> {
        Err(Reject::Unsupported)
    }
    /// 尚无经验证 parser capability，不能把终端输出当成可写回复。
    pub fn terminal_reply(
        &self,
        _target: &Target,
        _epoch: u64,
        _bytes: &[u8],
    ) -> Result<(), Reject> {
        Err(Reject::Unsupported)
    }
}
/// 单个 stdin 的唯一 writer。创建者仍须提供已准入的后端 IO；此类型不认证它。
#[must_use = "保留 owner，直到 try_join 确认实际线程退出；seal 不是 IO 退休证明"]
pub struct LocalWriter {
    handle: LocalInputHandle,
    worker: Option<JoinHandle<()>>,
    terminal: Option<Result<(), Reject>>,
    #[cfg(feature = "experimental-canonical-replies")]
    projection: Option<ProjectionHandle>,
}
#[derive(Default)]
struct WriterConfig {
    replies: Option<ReplyState>,
    #[cfg(feature = "experimental-host-owner")]
    view_gate: Option<view_gate::ViewGate>,
    #[cfg(test)]
    after_write: Option<Box<dyn FnOnce() + Send>>,
}
impl WriterConfig {
    fn after_write(&mut self) {
        #[cfg(test)]
        if let Some(f) = self.after_write.take() {
            f();
        }
    }
}
impl LocalWriter {
    pub(crate) fn start(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
    ) -> Result<Self, Reject> {
        Self::start_with(
            journal,
            stdin,
            target,
            device,
            epoch,
            key,
            WriterConfig::default(),
        )
    }
    /// Private backend factory. The reader gets only the output capability;
    /// this worker alone retains the PTY writer and its immutable exact target.
    pub(crate) fn start_terminal(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
    ) -> Result<(Self, HostOutputHandle), Reject> {
        let context = Arc::new(());
        let writer = Self::start_with(
            journal,
            stdin,
            target,
            device,
            epoch,
            key,
            WriterConfig {
                replies: Some(ReplyState::new(context.clone())),
                #[cfg(feature = "experimental-host-owner")]
                view_gate: None,
                #[cfg(test)]
                after_write: None,
            },
        )?;
        let output = HostOutputHandle::new(&writer.handle, context);
        Ok((writer, output))
    }
    #[cfg(feature = "experimental-canonical-replies")]
    fn start_canonical_terminal(
        journal: Journal,
        stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        start: CanonicalStart,
    ) -> Result<(Self, HostOutputHandle), Reject> {
        let budget = start.budget.clone();
        let context = Arc::new(());
        let mut writer = Self::start_with(
            journal,
            stdin,
            target,
            device,
            epoch,
            key,
            WriterConfig {
                replies: Some(ReplyState::canonical(context.clone(), start)),
                #[cfg(feature = "experimental-host-owner")]
                view_gate: None,
                #[cfg(test)]
                after_write: None,
            },
        )?;
        writer.projection = Some(ProjectionHandle::new(
            &writer.handle,
            context.clone(),
            budget,
        ));
        let output = HostOutputHandle::new(&writer.handle, context);
        Ok((writer, output))
    }
    fn start_with(
        mut journal: Journal,
        mut stdin: impl Write + Send + 'static,
        target: Target,
        device: [u8; 32],
        epoch: u64,
        key: DigestKey,
        mut config: WriterConfig,
    ) -> Result<Self, Reject> {
        if target.run.is_empty() || target.run.len() > 128 {
            return Err(Reject::Bounds);
        }
        let target = Target {
            run: target.run.as_str().to_owned(),
            ..target
        };
        let (tx, rx) = mpsc::sync_channel::<Command>(QUEUE);
        let sealed = Arc::new(AtomicBool::new(false));
        let target = Arc::new(target);
        #[cfg(feature = "experimental-host-owner")]
        let view_scope = config.view_gate.as_ref().map(|g| g.scope().clone());
        #[cfg(not(feature = "experimental-host-owner"))]
        let view_scope = None;
        let handle = LocalInputHandle {
            tx,
            target: target.clone(),
            device,
            epoch,
            sealed: sealed.clone(),
            credits: Arc::new(Credits::default()),
            view_scope,
        };
        let worker = std::thread::Builder::new()
            .name("cc-local-writer".into())
            .spawn(move || {
                let mut replies = config.replies.take();
                #[cfg(feature = "experimental-host-owner")]
                let mut view_gate = config.view_gate.take();
                loop {
                    if sealed.load(Ordering::Acquire) {
                        reject_queued(&rx);
                        break;
                    }
                    let command = match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(r) => r,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    let request = match command {
                        #[cfg(feature = "experimental-canonical-replies")]
                        Command::Projection(request) => {
                            let result = match &mut replies {
                                Some(state) => {
                                    state.capture(&request, target.clone(), epoch, &sealed)
                                }
                                None => Err(Reject::Unsupported),
                            };
                            #[cfg(feature = "experimental-host-owner")]
                            let result = if let Some(peer) = &request.peer {
                                result.and_then(|frame| {
                                    view_gate
                                        .as_mut()
                                        .ok_or(Reject::Unsupported)?
                                        .issue(frame, peer)
                                })
                            } else {
                                result
                            };
                            let _ = request.reply.try_send(result);
                            continue;
                        }
                        #[cfg(feature = "experimental-host-owner")]
                        Command::ViewAck(request) => {
                            let result = if sealed.load(Ordering::Acquire) {
                                Err(Reject::Sealed)
                            } else {
                                view_gate
                                    .as_mut()
                                    .ok_or(Reject::Unsupported)
                                    .and_then(|gate| {
                                        let epochs = replies
                                            .as_ref()
                                            .and_then(ReplyState::epochs)
                                            .ok_or(Reject::Sync)?;
                                        gate.acknowledge(&request.scope, &request.ack, epochs)
                                    })
                            };
                            let _ = request.reply.try_send(result);
                            continue;
                        }
                        Command::Input(request) => request,
                        #[cfg(feature = "experimental-host-owner")]
                        Command::ControlStatus(request) => {
                            if sealed.load(Ordering::Acquire) {
                                request.reject(Reject::Sealed);
                            } else if let Some(gate) = &view_gate {
                                request.process(gate);
                            } else {
                                request.reject(Reject::Unsupported);
                            }
                            continue;
                        }
                        #[cfg(feature = "experimental-host-owner")]
                        Command::Rebind(request) => {
                            if sealed.load(Ordering::Acquire) {
                                request.reject(Reject::Sealed);
                            } else if let Some(gate) = &mut view_gate {
                                request.process(gate, &journal);
                            } else {
                                request.reject(Reject::Unsupported);
                            }
                            continue;
                        }
                        #[cfg(feature = "experimental-host-owner")]
                        Command::Control(request) => {
                            if sealed.load(Ordering::Acquire) {
                                request.reject(Reject::Sealed);
                            } else if let Some(gate) = &mut view_gate {
                                if let Some(epochs) = replies.as_ref().and_then(ReplyState::epochs)
                                {
                                    request.process(gate, epochs, &mut journal);
                                } else {
                                    request.reject(Reject::Sync);
                                }
                            } else {
                                request.reject(Reject::Unsupported);
                            }
                            continue;
                        }
                        Command::Output(request) => {
                            let result = match &mut replies {
                                Some(state) => state.process(&request, &mut stdin, &sealed),
                                None => OutputResult::Rejected(Reject::Unsupported),
                            };
                            let _ = request.reply.try_send(result);
                            continue;
                        }
                    };
                    #[cfg(feature = "experimental-host-owner")]
                    let current_device = view_gate
                        .as_ref()
                        .map_or(&device, view_gate::ViewGate::device);
                    #[cfg(not(feature = "experimental-host-owner"))]
                    let current_device = &device;
                    if let Err(reason) = validate(
                        &target,
                        current_device,
                        epoch,
                        &request.key,
                        request.epoch,
                        request.bytes.len(),
                        &sealed,
                    ) {
                        let _ = request.reply.try_send(WriteResult::Rejected(reason));
                        continue;
                    }
                    #[cfg(feature = "experimental-host-owner")]
                    if let Some(gate) = &view_gate
                        && let Err(error) = gate.check_request(&request, replies.as_ref())
                    {
                        let _ = request.reply.try_send(WriteResult::Rejected(error));
                        continue;
                    }
                    let digest = digest(&key, &request.key, epoch, &request.bytes);
                    #[cfg(feature = "experimental-host-owner")]
                    let digest = if let Some(view) = &request.view {
                        match view.digest(&key, &request.key, epoch, &request.bytes) {
                            Ok(d) => d,
                            Err(e) => {
                                let _ = request.reply.try_send(WriteResult::Rejected(e));
                                continue;
                            }
                        }
                    } else {
                        digest
                    };
                    #[cfg(feature = "experimental-host-owner")]
                    if let Some(gate) = &view_gate
                        && let Err(error) = gate.can_admit(&request.key, &digest)
                    {
                        let _ = request.reply.try_send(error.feedback());
                        continue;
                    }
                    let outcome = match journal.prepare(&request.key, &digest) {
                        Ok(PrepareDecision::Recorded(outcome)) => {
                            #[cfg(feature = "experimental-host-owner")]
                            if let Some(gate) = &mut view_gate {
                                gate.note_admitted(&request.key, &digest);
                            }
                            Ok(outcome)
                        }
                        Ok(PrepareDecision::Prepared(intent)) => {
                            #[cfg(feature = "experimental-host-owner")]
                            if let Some(gate) = &mut view_gate {
                                gate.note_admitted(&request.key, &digest);
                            }
                            let effect_outcome = (|| {
                                // 意图已持久化后、真正写入前再次采样封存。当前在途写不能靠 seal 撤销。
                                if sealed.load(Ordering::Acquire) {
                                    return Outcome::Rejected;
                                }
                                let mut effect = || {
                                    let result = write_once(&mut stdin, &request.bytes);
                                    if result == Outcome::HostWritten {
                                        config.after_write();
                                    }
                                    result
                                };
                                #[cfg(feature = "experimental-host-owner")]
                                if let Some(gate) = &view_gate {
                                    return gate.effect_if_current(
                                        &request,
                                        replies.as_ref(),
                                        effect,
                                    );
                                }
                                effect()
                            })();
                            journal.finalize(intent, effect_outcome)
                        }
                        Err(error) => Err(error),
                    };
                    let feedback = match outcome {
                        Ok(Outcome::Partial) => {
                            sealed.store(true, Ordering::Release);
                            WriteResult::Partial
                        }
                        Ok(Outcome::Unknown) => {
                            sealed.store(true, Ordering::Release);
                            WriteResult::Unknown
                        }
                        Ok(_) => match journal.receipt(&request.key) {
                            Ok(ReceiptLookup::Known(r)) => WriteResult::Receipt(r),
                            _ => {
                                sealed.store(true, Ordering::Release);
                                WriteResult::Unknown
                            }
                        },
                        Err(error) => {
                            if matches!(error, JournalError::Storage | JournalError::Schema) {
                                sealed.store(true, Ordering::Release);
                            }
                            WriteResult::Journal(error)
                        }
                    };
                    // 每次反馈独立一槽，接收方消失或不读都不能阻塞唯一 writer。
                    let _ = request.reply.try_send(feedback);
                }
                // stdin、Journal、摘要 key 只由本线程持有；退出才释放。
            })
            .map_err(|_| Reject::Thread)?;
        Ok(Self {
            handle,
            worker: Some(worker),
            terminal: None,
            #[cfg(feature = "experimental-canonical-replies")]
            projection: None,
        })
    }
    pub fn handle(&self) -> LocalInputHandle {
        self.handle.clone()
    }
    #[cfg(feature = "experimental-canonical-replies")]
    pub(crate) fn capture(&self) -> Result<PendingProjection, Reject> {
        self.projection
            .as_ref()
            .ok_or(Reject::Unsupported)?
            .capture()
    }
    /// 仅封存新提交/请求停止；不声称当前 write 已结束或允许换 generation。
    pub fn seal(&self) {
        #[cfg(feature = "experimental-host-owner")]
        if let Some(scope) = &self.handle.view_scope {
            scope.close_writer();
        }
        self.handle.sealed.store(true, Ordering::Release);
    }
    pub fn try_join(&mut self) -> Result<bool, Reject> {
        if let Some(result) = self.terminal {
            return result.map(|_| true);
        }
        if self.worker.as_ref().is_none_or(|w| !w.is_finished()) {
            return Ok(false);
        }
        let result = self
            .worker
            .take()
            .ok_or(Reject::Thread)?
            .join()
            .map_err(|_| Reject::Thread);
        self.terminal = Some(result);
        result.map(|_| true)
    }
}
impl Drop for LocalWriter {
    fn drop(&mut self) {
        self.seal(); /* 不阻塞 Drop，也不把 detach 冒充 join。 */
    }
}
fn reject_queued(rx: &mpsc::Receiver<Command>) {
    for r in rx.try_iter() {
        match r {
            #[cfg(feature = "experimental-host-owner")]
            Command::Control(r) => r.reject(Reject::Sealed),
            #[cfg(feature = "experimental-host-owner")]
            Command::Rebind(r) => r.reject(Reject::Sealed),
            #[cfg(feature = "experimental-host-owner")]
            Command::ControlStatus(r) => r.reject(Reject::Sealed),
            #[cfg(feature = "experimental-host-owner")]
            Command::ViewAck(r) => {
                let _ = r.reply.try_send(Err(Reject::Sealed));
            }
            #[cfg(feature = "experimental-canonical-replies")]
            Command::Projection(r) => {
                let _ = r.reply.try_send(Err(Reject::Sealed));
            }
            Command::Input(r) => {
                let _ = r.reply.try_send(WriteResult::Rejected(Reject::Sealed));
            }
            Command::Output(r) => {
                let _ = r.reply.try_send(OutputResult::Rejected(Reject::Sealed));
            }
        }
    }
}
fn validate(
    target: &Target,
    device: &[u8; 32],
    epoch: u64,
    key: &OperationKey,
    request_epoch: u64,
    len: usize,
    sealed: &AtomicBool,
) -> Result<(), Reject> {
    if key.run.is_empty() || key.run.len() > 128 || len == 0 || len > MAX_INPUT {
        return Err(Reject::Bounds);
    }
    if key.host != target.host
        || key.session != target.session
        || key.run != target.run
        || key.generation != target.generation
        || key.incarnation != target.incarnation
    {
        return Err(Reject::Target);
    }
    if &key.device != device {
        return Err(Reject::Device);
    }
    if request_epoch != epoch {
        return Err(Reject::Epoch);
    }
    if sealed.load(Ordering::Acquire) {
        return Err(Reject::Sealed);
    }
    Ok(())
}
fn digest(key: &DigestKey, operation: &OperationKey, epoch: u64, bytes: &[u8]) -> KeyedDigest {
    key.input(operation, epoch, bytes)
        .expect("validated local input bounds")
}
fn write_once(stdin: &mut impl Write, bytes: &[u8]) -> Outcome {
    let mut written = 0;
    // Bound retry-loop CPU, not the duration of a synchronous native call.
    let mut interrupted_left = 16;
    while written < bytes.len() {
        match stdin.write(&bytes[written..]) {
            Ok(0) => {
                return if written == 0 {
                    Outcome::Unknown
                } else {
                    Outcome::Partial
                };
            }
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted && interrupted_left > 0 => {
                interrupted_left -= 1;
                continue;
            }
            Err(_) => {
                return if written == 0 {
                    Outcome::Unknown
                } else {
                    Outcome::Partial
                };
            }
        }
    }
    if stdin.flush().is_err() {
        Outcome::Unknown
    } else {
        Outcome::HostWritten
    }
}
#[cfg(all(test, windows))]
#[path = "local_writer_reply_tests.rs"]
mod reply_tests;
#[cfg(all(test, windows))]
#[path = "local_writer_tests.rs"]
mod tests;

#[test]
fn digest_public_oracle_001() {
    let key = DigestKey::from_backend_bytes([0x11; 32]);
    let operation = OperationKey {
        host: [0x22; 32],
        session: [0x33; 16],
        run: "PUBLIC_TEST_ONLY_VECTOR".into(),
        generation: 7,
        incarnation: 8,
        device: [0x44; 32],
        operation: [0x55; 16],
        sequence: 9,
    };
    assert_eq!(
        digest(&key, &operation, 10, b"PUBLIC_TEST_ONLY\n").0,
        [
            16, 149, 32, 252, 84, 80, 211, 55, 188, 218, 118, 186, 110, 180, 71, 57, 207, 143, 38,
            32, 7, 200, 176, 90, 0, 205, 79, 220, 13, 127, 203, 155
        ]
    );
}
