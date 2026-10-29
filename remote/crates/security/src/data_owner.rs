//! One private task polls both directions; public waits never own TLS futures.
use crate::{
    CancelToken, GrantError, Role, VerifiedGrant,
    data::{DataError, DataRecord},
};
use cc_desk_remote_record::{
    Header, Kind,
    grant::{self, DataHeader, DataRole, SessionScope},
};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};
pub const DATA_QUEUE_SLOTS: usize = 4;
struct Inbox {
    rx: mpsc::Receiver<DataRecord>,
    head: Option<DataRecord>,
}
struct SendRequest {
    fence: u64,
    body: Vec<u8>,
    end: Instant,
    reply: oneshot::Sender<Result<(), DataError>>,
    _credit: OwnedSemaphorePermit,
}
struct Writing {
    bytes: Vec<u8>,
    offset: usize,
    request: SendRequest,
}
struct Reading {
    header: [u8; 8],
    offset: usize,
    payload: Option<Vec<u8>>,
    credit: Option<OwnedSemaphorePermit>,
    end: Option<Instant>,
}
impl Reading {
    fn new() -> Self {
        Self {
            header: [0; 8],
            offset: 0,
            payload: None,
            credit: None,
            end: None,
        }
    }
}
pub struct RunningChannel {
    cancel: CancelToken,
    access: VerifiedGrant,
    role: Role,
    binding: [u8; 32],
    out: mpsc::Sender<SendRequest>,
    incoming: Mutex<Inbox>,
    send_credits: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    active: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<DataError>>>,
    exit: watch::Receiver<Option<DataError>>,
}
impl RunningChannel {
    pub fn binding_id(&self) -> [u8; 32] {
        self.binding
    }
    pub fn is_closed(&self) -> bool {
        !self.active.load(Ordering::Acquire)
    }
    fn check(
        &self,
        scope: &SessionScope,
        fence: u64,
        budget: Duration,
    ) -> Result<Instant, DataError> {
        if self.cancel.is_cancelled() {
            return Err(DataError::Cancelled);
        }
        if *self.stop.borrow() {
            return Err(DataError::Closed);
        }
        if self.is_closed() {
            return Err(DataError::Closed);
        }
        if scope != self.access.scope() {
            return Err(DataError::Scope);
        }
        if fence == 0 {
            return Err(DataError::Fence);
        }
        if budget.is_zero() || budget > Duration::from_secs(10) {
            return Err(DataError::Bounds);
        }
        let ms = self.access.check().map_err(DataError::Grant)?;
        Instant::now()
            .checked_add(budget.min(Duration::from_millis(ms)))
            .ok_or(DataError::Bounds)
    }
    pub async fn send_projection(
        &self,
        scope: &SessionScope,
        fence: u64,
        bytes: &[u8],
        budget: Duration,
    ) -> Result<(), DataError> {
        if self.role != Role::Host {
            return Err(DataError::Role);
        }
        self.send(scope, fence, bytes, budget).await
    }
    pub async fn send_control(
        &self,
        scope: &SessionScope,
        fence: u64,
        bytes: &[u8],
        budget: Duration,
    ) -> Result<(), DataError> {
        if self.role != Role::Client {
            return Err(DataError::Role);
        }
        self.send(scope, fence, bytes, budget).await
    }
    async fn send(
        &self,
        scope: &SessionScope,
        fence: u64,
        bytes: &[u8],
        budget: Duration,
    ) -> Result<(), DataError> {
        let end = self.check(scope, fence, budget)?;
        let kind = if self.role == Role::Host {
            Kind::Projection
        } else {
            Kind::Control
        };
        if kind == Kind::Control && self.access.role() != DataRole::Controller {
            return Err(DataError::Role);
        }
        if bytes.len() > kind.limit() - grant::DATA_PREFIX {
            return Err(DataError::Bounds);
        }
        let credit = self
            .send_credits
            .clone()
            .try_acquire_owned()
            .map_err(|_| DataError::Full)?;
        let (tx, rx) = oneshot::channel();
        self.out
            .try_send(SendRequest {
                fence,
                body: bytes.to_vec(),
                end,
                reply: tx,
                _credit: credit,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DataError::Full,
                _ => DataError::Closed,
            })?;
        tokio::time::timeout_at(end, rx)
            .await
            .map_err(|_| DataError::Uncertain)?
            .map_err(|_| DataError::Uncertain)?
    }
    pub async fn receive(
        &self,
        scope: &SessionScope,
        fence: u64,
        budget: Duration,
    ) -> Result<DataRecord, DataError> {
        let end = self.check(scope, fence, budget)?;
        let mut exit = self.exit.clone();
        let work = async {
            let mut inbox = self.incoming.lock().await;
            if inbox.head.is_none() {
                inbox.head = Some(inbox.rx.recv().await.ok_or(DataError::Closed)?);
            }
            self.check(scope, fence, budget)?;
            if Instant::now() >= end {
                return Err(DataError::Deadline);
            }
            if self.cancel.is_cancelled() {
                return Err(DataError::Cancelled);
            }
            if *self.stop.borrow() {
                return Err(DataError::Closed);
            }
            if inbox.head.as_ref().unwrap().fence() != fence {
                self.stop.send_replace(true);
                return Err(DataError::Fence);
            }
            Ok(inbox.head.take().unwrap())
        };
        tokio::select! {biased;_=exit.changed()=>Err(exit.borrow().unwrap_or(DataError::Closed)),r=tokio::time::timeout_at(end,work)=>r.map_err(|_|DataError::Deadline)?}
    }
    pub async fn close(&self) -> DataError {
        self.stop.send_replace(true);
        self.wait_closed().await
    }
    pub async fn wait_closed(&self) -> DataError {
        let mut task = self.task.lock().await;
        if let Some(join) = task.as_mut() {
            let result = join.await.unwrap_or(DataError::Closed);
            task.take();
            result
        } else {
            self.exit.borrow().unwrap_or(DataError::Closed)
        }
    }
}
impl Drop for RunningChannel {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
enum Event {
    Progress,
    Incoming(DataRecord),
    Written,
}
struct Engine<S> {
    io: tokio_rustls::TlsStream<S>,
    access: VerifiedGrant,
    role: Role,
    binding: [u8; 32],
    read: Reading,
    writing: Option<Writing>,
    sent: u64,
    received: u64,
    write_turn: bool,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Engine<S> {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<Event, DataError>> {
        let mut progress = false;
        let mut written = false;
        let mut timer_started = false;
        if let Some(w) = &mut self.writing {
            if w.offset < w.bytes.len() {
                match Pin::new(&mut self.io).poll_write(cx, &w.bytes[w.offset..]) {
                    Poll::Ready(Ok(0)) => return Poll::Ready(Err(DataError::Io)),
                    Poll::Ready(Ok(n)) => {
                        w.offset += n;
                        progress = true
                    }
                    Poll::Ready(Err(_)) => return Poll::Ready(Err(DataError::Io)),
                    Poll::Pending => {}
                }
            }
            if w.offset == w.bytes.len() {
                match Pin::new(&mut self.io).poll_flush(cx) {
                    Poll::Ready(Ok(())) => written = true,
                    Poll::Ready(Err(_)) => return Poll::Ready(Err(DataError::Io)),
                    Poll::Pending => {}
                }
            }
        }
        if written && self.write_turn {
            self.write_turn = false;
            return Poll::Ready(Ok(Event::Written));
        }
        if self.read.credit.is_some() {
            let buffer = match self.read.payload.as_mut() {
                None => &mut self.read.header[self.read.offset..],
                Some(p) => &mut p[self.read.offset..],
            };
            let mut b = ReadBuf::new(buffer);
            match Pin::new(&mut self.io).poll_read(cx, &mut b) {
                Poll::Ready(Ok(())) => {
                    let n = b.filled().len();
                    if n == 0 {
                        return Poll::Ready(Err(DataError::Closed));
                    }
                    progress = true;
                    self.read.offset += n;
                    if self.read.end.is_none() {
                        self.read.end = Some(Instant::now() + Duration::from_secs(10));
                        timer_started = true;
                    }
                }
                Poll::Ready(Err(_)) => return Poll::Ready(Err(DataError::Io)),
                Poll::Pending => {}
            }
            if self.read.payload.is_none() && self.read.offset == 8 {
                let h = Header::decode(self.read.header, 1).map_err(|_| DataError::Codec)?;
                if h.kind != Kind::Data {
                    return Poll::Ready(Err(DataError::Codec));
                }
                self.read.payload = Some(vec![0; h.payload_len]);
                self.read.offset = 0;
            }
            if self
                .read
                .payload
                .as_ref()
                .is_some_and(|p| self.read.offset == p.len())
            {
                let mut bytes = self.read.payload.take().unwrap();
                let (h, _) = grant::decode_data(&bytes).map_err(|_| DataError::Codec)?;
                let kind = if self.role == Role::Host {
                    Kind::Control
                } else {
                    Kind::Projection
                };
                let next = self.received.checked_add(1).ok_or(DataError::Replay)?;
                if h.binding != self.binding
                    || h.grant != self.access.id()
                    || h.scope != self.access.scope_digest()
                    || h.sequence != next
                    || h.kind != kind
                {
                    return Poll::Ready(Err(DataError::Context));
                }
                if kind == Kind::Control && self.access.role() != DataRole::Controller {
                    return Poll::Ready(Err(DataError::Role));
                }
                self.received = next;
                let n = bytes.len() - grant::DATA_PREFIX;
                bytes.copy_within(grant::DATA_PREFIX.., 0);
                bytes.truncate(n);
                let credit = self.read.credit.take();
                self.read = Reading::new();
                self.write_turn = true;
                return Poll::Ready(Ok(Event::Incoming(DataRecord {
                    scope: self.access.scope().clone(),
                    fence: h.fence,
                    sequence: h.sequence,
                    payload: bytes,
                    _credit: credit,
                })));
            }
        }
        if timer_started {
            return Poll::Ready(Ok(Event::Progress));
        }
        if written {
            self.write_turn = false;
            return Poll::Ready(Ok(Event::Written));
        }
        if progress {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}
pub(super) fn start<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: tokio_rustls::TlsStream<S>,
    mut cancel: CancelToken,
    access: VerifiedGrant,
    role: Role,
    binding: [u8; 32],
    active: Arc<AtomicBool>,
) -> Result<RunningChannel, DataError> {
    let remaining = access.check().map_err(DataError::Grant)?;
    if cancel.is_cancelled() {
        return Err(DataError::Cancelled);
    }
    let (out, mut requests) = mpsc::channel::<SendRequest>(DATA_QUEUE_SLOTS);
    let (in_tx, in_rx) = mpsc::channel(DATA_QUEUE_SLOTS);
    let credits = Arc::new(Semaphore::new(DATA_QUEUE_SLOTS));
    let read_credits = Arc::new(Semaphore::new(DATA_QUEUE_SLOTS));
    let (stop, mut stopped) = watch::channel(false);
    let (exit_tx, exit) = watch::channel(None);
    let public_access = access.clone();
    let public_cancel = cancel.clone();
    let a = active.clone();
    let task = tokio::spawn(async move {
        let (mut revoked, mut superseded) = access.revoked();
        let mut end = Instant::now() + Duration::from_millis(remaining);
        let mut engine = Engine {
            io,
            access,
            role,
            binding,
            read: Reading::new(),
            writing: None,
            sent: 0,
            received: 0,
            write_turn: false,
        };
        let mut pending = None;
        let result = loop {
            let left = match engine.access.check() {
                Ok(ms) => ms,
                Err(e) => break DataError::Grant(e),
            };
            end = end.min(Instant::now() + Duration::from_millis(left));
            let until = engine
                .writing
                .as_ref()
                .map_or(end, |w| end.min(w.request.end));
            let until = engine.read.end.map_or(until, |e| until.min(e));
            tokio::select! {biased;
                            _=cancel.cancelled()=>break DataError::Cancelled,
                            _=stopped.changed()=>break DataError::Closed,
                            _=revoked.changed()=>break DataError::Grant(GrantError::Revoked),
                            _=superseded.changed()=>break DataError::Grant(GrantError::Revoked),
                            _=tokio::time::sleep_until(until)=>break DataError::Deadline,
                            permit=in_tx.reserve(),if pending.is_some()=>{if cancel.is_cancelled(){break DataError::Cancelled}
            if let Err(e)=engine.access.check(){break DataError::Grant(e)}
            match permit{Ok(p)=>p.send(pending.take().unwrap()),Err(_)=>break DataError::Closed}},
                            credit=read_credits.clone().acquire_owned(),if engine.read.credit.is_none()&&pending.is_none()=>{engine.read.credit=Some(credit.unwrap());},
                            request=requests.recv(),if engine.writing.is_none()=>{
                                let Some(request)=request else{break DataError::Closed};if Instant::now()>=request.end{let _=request.reply.send(Err(DataError::Deadline));continue}
                                let Some(sequence)=engine.sent.checked_add(1)else{break DataError::Replay};
                                let kind=if role==Role::Host{Kind::Projection}else{Kind::Control};
                                let header=DataHeader{binding,grant:engine.access.id(),scope:engine.access.scope_digest(),fence:request.fence,sequence,kind};
                                let bytes=match grant::encode_data(&header,&request.body).and_then(|p|cc_desk_remote_record::encode(Kind::Data,1,&p).map_err(|_|cc_desk_remote_record::appbind::CodecError::Field)){Ok(b)=>b,Err(_)=>break DataError::Codec};
                                engine.sent=sequence;engine.writing=Some(Writing{bytes,offset:0,request});
                            },
                            event=std::future::poll_fn(|cx|engine.poll(cx))=>{if cancel.is_cancelled(){break DataError::Cancelled}
            if *stopped.borrow(){break DataError::Closed}
            if Instant::now()>=until{break DataError::Deadline}
            if let Err(e)=engine.access.check(){break DataError::Grant(e)}
            match event{
                                Err(e)=>break e,
                                Ok(Event::Progress)=>{},
                                Ok(Event::Incoming(record))=>{if pending.is_some(){break DataError::Full}pending=Some(record);},
                                Ok(Event::Written)=>{if let Err(e)=engine.access.check(){break DataError::Grant(e)}let w=engine.writing.take().unwrap();if Instant::now()>=w.request.end{break DataError::Deadline}let _=w.request.reply.send(Ok(()));},
                            }}
                        }
        };
        drop(engine);
        drop(pending);
        drop(requests);
        drop(in_tx);
        a.store(false, Ordering::Release);
        exit_tx.send_replace(Some(result));
        result
    });
    Ok(RunningChannel {
        cancel: public_cancel,
        access: public_access,
        role,
        binding,
        out,
        incoming: Mutex::new(Inbox {
            rx: in_rx,
            head: None,
        }),
        send_credits: credits,
        stop,
        active,
        task: Mutex::new(Some(task)),
        exit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GrantGate,
        data_tests::pair,
        grant_tests::{admin, scope, signed},
    };
    use std::sync::atomic::AtomicU64;
    #[tokio::test]
    async fn expired_receive_wait_preserves_the_original_queued_record() {
        let (mut c, mut h, _cancel) = pair().await;
        let mut gate =
            GrantGate::for_binding(h.context(), admin(), [4; 32], Arc::new(|| Ok(1000))).unwrap();
        let p = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
            .unwrap();
        let grant = gate.accept(&signed(&p, gate.issuer_kid())).unwrap();
        c.install(&grant).unwrap();
        h.install(&grant).unwrap();
        let c = c.start().unwrap();
        let h = h.start().unwrap();
        let target = scope();
        c.send_control(&target, 4, b"not-lost", Duration::from_secs(1))
            .await
            .unwrap();
        let queue = h.incoming.lock().await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while queue.head.is_none() && queue.rx.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tokio::time::pause();
        let mut wait = Box::pin(h.receive(&target, 4, Duration::from_secs(1)));
        assert!(futures_util::poll!(wait.as_mut()).is_pending());
        tokio::time::advance(Duration::from_secs(2)).await;
        drop(queue);
        assert_eq!(wait.await.unwrap_err(), DataError::Deadline);
        assert_eq!(
            h.receive(&target, 4, Duration::from_secs(1))
                .await
                .unwrap()
                .payload(),
            b"not-lost"
        );
        h.close().await;
        c.wait_closed().await;
    }
    #[tokio::test]
    async fn buffered_payload_cannot_escape_after_cancel_or_stop_is_requested() {
        for cancel_it in [false, true] {
            let (mut c, mut h, cancel) = pair().await;
            let clock = Arc::new(AtomicU64::new(1000));
            let mut gate = GrantGate::for_binding(
                h.context(),
                admin(),
                [4; 32],
                Arc::new(move || Ok(clock.load(Ordering::Acquire))),
            )
            .unwrap();
            let p = gate
                .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
                .unwrap();
            let grant = gate.accept(&signed(&p, gate.issuer_kid())).unwrap();
            c.install(&grant).unwrap();
            h.install(&grant).unwrap();
            let c = c.start().unwrap();
            let h = h.start().unwrap();
            c.send_control(&scope(), 4, b"buffered", Duration::from_secs(1))
                .await
                .unwrap();
            let queue = h.incoming.lock().await;
            tokio::time::timeout(Duration::from_secs(1), async {
                while queue.head.is_none() && queue.rx.is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            drop(queue);
            if cancel_it {
                cancel.cancel()
            } else {
                h.stop.send_replace(true);
            }
            assert!(
                h.receive(&scope(), 4, Duration::from_secs(1))
                    .await
                    .is_err()
            );
            h.wait_closed().await;
            c.wait_closed().await;
        }
    }
}
