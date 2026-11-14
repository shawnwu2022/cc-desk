//! Bounded opaque transport. This module grants no endpoint or PTY authority.
mod deadline;
#[cfg(test)]
mod browser_relay_fixture_tests;
#[cfg(test)]
mod owned_tests;
#[cfg(test)]
mod registration_tests;
pub mod records;
pub mod wss;
pub fn websocket_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(131072)
        .max_message_size(Some(65536))
        .max_frame_size(Some(65536))
}
pub const TUNNEL_BUFFER: usize = 65536;
pub const MAX_IO_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);
/// Only this factory can create a route endpoint; callers cannot supply larger buffers.
pub struct RouteEndpoint(tokio::io::DuplexStream);
pub fn bounded_tunnel() -> (tokio::io::DuplexStream, RouteEndpoint) {
    let (app, route) = tokio::io::duplex(TUNNEL_BUFFER);
    (app, RouteEndpoint(route))
}
/// One original mux driver. Cancelling a wait leaves its JoinHandle in this slot.
/// A successful wait attests task termination, not a normal protocol exit.
pub struct OwnedTunnel {
    stop: tokio::sync::watch::Sender<bool>,
    join: tokio::sync::Mutex<Option<tokio::task::JoinHandle<Result<(), TransportError>>>>,
    joined: std::sync::Arc<std::sync::atomic::AtomicU8>,
}
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
pub struct TestOnlyTunnelRetirement(std::sync::Arc<std::sync::atomic::AtomicU8>);
#[cfg(feature = "test-only-loopback")]
impl TestOnlyTunnelRetirement {
    pub fn known_joined(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire) == 1
    }
}
impl OwnedTunnel {
    #[cfg(feature = "test-only-loopback")]
    #[doc(hidden)]
    pub fn test_only_retirement(&self) -> TestOnlyTunnelRetirement {
        TestOnlyTunnelRetirement(self.joined.clone())
    }
    pub fn request_close(&self) {
        self.stop.send_replace(true);
    }
    pub async fn wait_closed(&self) -> Result<(), TransportError> {
        use std::sync::atomic::Ordering;
        let mut slot = self.join.lock().await;
        if let Some(task) = slot.as_mut() {
            let known = task.await.is_ok();
            *slot = None;
            self.joined
                .store(if known { 1 } else { 2 }, Ordering::Release);
        }
        if self.joined.load(Ordering::Acquire) == 1 {
            Ok(())
        } else {
            Err(TransportError::Io)
        }
    }
}
impl Drop for OwnedTunnel {
    fn drop(&mut self) {
        self.request_close();
    }
}
/// Production entry accepts only the original admitted WebSocket.
pub fn start_admitted(
    ws: wss::AdmittedSocket,
    tunnel: [u8; 16],
    budget: std::time::Duration,
) -> Result<(tokio::io::DuplexStream, OwnedTunnel), TransportError> {
    start_owned(ws, tunnel, budget)
}
/// Registration-only outer keepalive. The original total deadline is never renewed.
pub fn start_admitted_registration(
    ws: wss::AdmittedSocket,
    tunnel: [u8; 16],
    budget: std::time::Duration,
    original_end: tokio::time::Instant,
    original_cancel: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(tokio::io::DuplexStream, OwnedTunnel), TransportError> {
    start_owned_registration(ws, tunnel, budget, original_end, original_cancel)
}
fn start_owned_registration<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    tunnel: [u8; 16],
    budget: std::time::Duration,
    original_end: tokio::time::Instant,
    original_cancel: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(tokio::io::DuplexStream, OwnedTunnel), TransportError>
where S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {
    let remaining = original_end.checked_duration_since(tokio::time::Instant::now())
        .filter(|remaining| !remaining.is_zero() && *remaining <= std::time::Duration::from_secs(120))
        .ok_or(TransportError::Limit)?;
    if budget < std::time::Duration::from_millis(30) || remaining.is_zero() {
        return Err(TransportError::Limit);
    }
    start_owned_policy(ws,tunnel,budget,Some(RegistrationPolicy{end:original_end,interval:budget/3,tunnel}),Some(Box::pin(original_cancel)),
        #[cfg(feature="test-only-loopback")]
        None,
    )
}
#[derive(Clone,Copy)]
struct RegistrationPolicy { end:tokio::time::Instant,interval:std::time::Duration,tunnel:[u8;16] }
type OriginalCancellation = std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send>>;
async fn original_cancelled(cancel:&mut Option<OriginalCancellation>) {
    match cancel { Some(future)=>future.await, None=>std::future::pending().await }
}
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestOnlyMuxCause { Cancelled, IoBudget, WebSocketClose, WebSocketEof, WebSocketReset, SocketIo, SocketWrite, RouteWorker, Delivery, Wire }
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct TestOnlyMuxEnd {
    pub cause: TestOnlyMuxCause,
    pub selected_at: tokio::time::Instant,
    pub result: &'static str,
}
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
pub type TestOnlyObservedTunnel = (tokio::io::DuplexStream, OwnedTunnel, tokio::sync::oneshot::Receiver<TestOnlyMuxEnd>);
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
pub fn test_only_start_admitted_observed(ws: wss::AdmittedSocket, tunnel: [u8; 16], budget: std::time::Duration) -> Result<TestOnlyObservedTunnel, TransportError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let (stream, owner) = start_owned_inner(ws, tunnel, budget, Some(sender))?;
    Ok((stream, owner, receiver))
}
/// In-memory outer WebSocket fixture; never a relay admission or production path.
#[cfg(feature = "test-only-loopback")]
#[doc(hidden)]
pub async fn test_only_memory_mux_pair(
    tunnel: [u8; 16],
    budget: std::time::Duration,
) -> Result<
    (
        (tokio::io::DuplexStream, OwnedTunnel),
        (tokio::io::DuplexStream, OwnedTunnel),
    ),
    TransportError,
> {
    let (a, b) = tokio::io::duplex(TUNNEL_BUFFER);
    let a = tokio_tungstenite::WebSocketStream::from_raw_socket(
        a,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        Some(websocket_config()),
    )
    .await;
    let b = tokio_tungstenite::WebSocketStream::from_raw_socket(
        b,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        Some(websocket_config()),
    )
    .await;
    Ok((
        start_owned(a, tunnel, budget)?,
        start_owned(b, tunnel, budget)?,
    ))
}
fn start_owned<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    tunnel: [u8; 16],
    budget: std::time::Duration,
) -> Result<(tokio::io::DuplexStream, OwnedTunnel), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    start_owned_inner(ws, tunnel, budget, #[cfg(feature = "test-only-loopback")] None)
}
fn start_owned_inner<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    tunnel: [u8; 16],
    budget: std::time::Duration,
    #[cfg(feature = "test-only-loopback")] observer: Option<tokio::sync::oneshot::Sender<TestOnlyMuxEnd>>,
) -> Result<(tokio::io::DuplexStream, OwnedTunnel), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    start_owned_policy(ws,tunnel,budget,None,None,
        #[cfg(feature="test-only-loopback")]
        observer,
    )
}
fn start_owned_policy<S>(
    ws:tokio_tungstenite::WebSocketStream<S>, tunnel:[u8;16],budget:std::time::Duration,
    registration:Option<RegistrationPolicy>,original_cancel:Option<OriginalCancellation>,
    #[cfg(feature="test-only-loopback")] observer:Option<tokio::sync::oneshot::Sender<TestOnlyMuxEnd>>,
) -> Result<(tokio::io::DuplexStream,OwnedTunnel),TransportError>
where S:tokio::io::AsyncRead+tokio::io::AsyncWrite+Unpin+Send+'static {
    if tunnel == [0; 16] || budget.is_zero() || budget > MAX_IO_BUDGET {
        return Err(TransportError::Limit);
    }
    let (stream, route) = bounded_tunnel();
    let (stop, rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(drive_policy(
        ws,
        std::collections::HashMap::from([(tunnel, route)]),
        rx,
        budget,
        registration,
        original_cancel,
        #[cfg(feature = "test-only-loopback")]
        observer,
    ));
    Ok((
        stream,
        OwnedTunnel {
            stop,
            join: tokio::sync::Mutex::new(Some(task)),
            joined: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
        },
    ))
}
#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    Limit,
    Wire,
    UnknownTunnel,
    Backpressure,
    Io,
    Deadline,
    Closed,
    Cancelled,
}
pub async fn drive<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    routes: std::collections::HashMap<[u8; 16], RouteEndpoint>,
    cancel: tokio::sync::watch::Receiver<bool>,
    budget: std::time::Duration,
) -> Result<(), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    drive_inner(ws, routes, cancel, budget, #[cfg(feature = "test-only-loopback")] None).await
}
async fn drive_inner<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    routes: std::collections::HashMap<[u8; 16], RouteEndpoint>,
    cancel: tokio::sync::watch::Receiver<bool>,
    budget: std::time::Duration,
    #[cfg(feature = "test-only-loopback")] observer: Option<tokio::sync::oneshot::Sender<TestOnlyMuxEnd>>,
) -> Result<(), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    drive_policy(ws,routes,cancel,budget,None,None,
        #[cfg(feature="test-only-loopback")]
        observer,
    ).await
}
async fn drive_policy<S>(
    mut ws:tokio_tungstenite::WebSocketStream<S>,routes:std::collections::HashMap<[u8;16],RouteEndpoint>,
    mut cancel:tokio::sync::watch::Receiver<bool>,budget:std::time::Duration,
    registration:Option<RegistrationPolicy>,mut original_cancel:Option<OriginalCancellation>,
    #[cfg(feature="test-only-loopback")] observer:Option<tokio::sync::oneshot::Sender<TestOnlyMuxEnd>>,
) -> Result<(),TransportError>
where S:tokio::io::AsyncRead+tokio::io::AsyncWrite+Unpin+Send+'static {
    use cc_desk_remote_protocol::{Subframe, encode_outer};
    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::mpsc,
        task::JoinSet,
        time::{Instant, timeout, timeout_at},
    };
    use tokio_tungstenite::tungstenite::Message;
    if routes.len() > 16 || budget.is_zero() || budget > MAX_IO_BUDGET {
        return Err(TransportError::Limit);
    }
    let c = ws.get_config();
    if c.max_message_size.is_none_or(|n| n > 65536)
        || c.max_frame_size.is_none_or(|n| n > 65536)
        || c.read_buffer_size > 4096
        || c.max_write_buffer_size > 131072
        || c.accept_unmasked_frames
    {
        return Err(TransportError::Limit);
    }
    if *cancel.borrow() {
        return Err(TransportError::Cancelled);
    }
    let mut workers = JoinSet::new();
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(16);
    let mut incoming = std::collections::HashMap::new();
    for (id, stream) in routes {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(4);
        incoming.insert(id, tx);
        let out = out_tx.clone();
        workers.spawn(async move {
            let (mut read, mut write) = tokio::io::split(stream.0);
            let read_loop = async {
                let mut buf = [0; 16384];
                loop {
                    let n = read.read(&mut buf).await.map_err(|_| TransportError::Io)?;
                    if n == 0 {
                        return Err::<(), _>(TransportError::Closed);
                    }
                    let msg = encode_outer(&[Subframe {
                        tunnel: id,
                        payload: &buf[..n],
                    }])
                    .map_err(|_| TransportError::Wire)?;
                    out.send(msg).await.map_err(|_| TransportError::Closed)?;
                }
            };
            let write_loop = async {
                while let Some(bytes) = rx.recv().await {
                    timeout(budget, write.write_all(&bytes))
                        .await
                        .map_err(|_| TransportError::Deadline)?
                        .map_err(|_| TransportError::Io)?;
                }
                Err::<(), _>(TransportError::Closed)
            };
            tokio::select! { r=read_loop=>r,r=write_loop=>r }
        });
    }
    drop(out_tx);
    let mut read_deadline = Instant::now()
        .checked_add(budget)
        .ok_or(TransportError::Limit)?;
    let cap=|end:Instant|registration.map_or(end,|r|end.min(r.end));
    read_deadline=cap(read_deadline);
    let mut heartbeat=registration.map_or(read_deadline,|r|cap(Instant::now()+r.interval));
    #[cfg(feature = "test-only-loopback")]
    let observed_end;
    macro_rules! finish {
        ($result:expr, $cause:ident) => {{
            let terminal_result = $result;
            #[cfg(feature = "test-only-loopback")]
            { observed_end = TestOnlyMuxEnd { cause: TestOnlyMuxCause::$cause, selected_at: Instant::now(), result: match &terminal_result { Ok(()) => "Ok", Err(TransportError::Limit) => "Limit", Err(TransportError::Wire) => "Wire", Err(TransportError::UnknownTunnel) => "UnknownTunnel", Err(TransportError::Backpressure) => "Backpressure", Err(TransportError::Io) => "Io", Err(TransportError::Deadline) => "Deadline", Err(TransportError::Closed) => "Closed", Err(TransportError::Cancelled) => "Cancelled" } }; }
            break terminal_result;
        }};
    }
    let result = loop {
        if registration.is_some() {
            if *cancel.borrow() || cancel.has_changed().is_err(){finish!(Err(TransportError::Cancelled),Cancelled);}
            tokio::select!{biased;_=original_cancelled(&mut original_cancel)=>finish!(Err(TransportError::Cancelled),Cancelled),_=std::future::ready(())=>{}}
        }
        if Instant::now() >= read_deadline {
            finish!(Err(TransportError::Deadline), IoBudget);
        }
        if let Some(policy)=registration.filter(|_|Instant::now()>=heartbeat) {
            let bytes=encode_outer(&[Subframe{tunnel:policy.tunnel,payload:&[]}]).map_err(|_|TransportError::Wire)?;
            let sent=tokio::select!{biased;
                _=cancel.changed()=>Err(TransportError::Cancelled),
                _=original_cancelled(&mut original_cancel)=>Err(TransportError::Cancelled),
                r=timeout_at(read_deadline,ws.send(Message::Binary(bytes.into())))=>r.map_err(|_|TransportError::Deadline).and_then(|r|r.map_err(|_|TransportError::Io)),
            };
            if let Err(e)=sent{finish!(Err(e),SocketWrite);}
            if Instant::now()>=read_deadline{finish!(Err(TransportError::Deadline),IoBudget);}
            heartbeat=cap(Instant::now()+policy.interval);
        }
        // Service at most one ready outbound envelope before the next inbound
        // event. A permanently-ready inbound stream cannot starve outbound.
        if let Ok(bytes) = out_rx.try_recv() {
            let sent = tokio::select! {
                biased;
                _=cancel.changed()=>Err(TransportError::Cancelled),
                _=original_cancelled(&mut original_cancel)=>Err(TransportError::Cancelled),
                r=timeout_at(read_deadline,ws.send(Message::Binary(bytes.into())))=>r.map_err(|_|TransportError::Deadline).and_then(|r|r.map_err(|_|TransportError::Io)),
            };
            if let Err(e) = sent {
                finish!(Err(e), SocketWrite);
            }
            if registration.is_some()&&Instant::now()>=read_deadline{finish!(Err(TransportError::Deadline),IoBudget);}
        }
        tokio::select! {
            biased;
            _=cancel.changed()=>finish!(Err(TransportError::Cancelled), Cancelled),
            _=original_cancelled(&mut original_cancel)=>finish!(Err(TransportError::Cancelled), Cancelled),
            _=tokio::time::sleep_until(heartbeat),if registration.is_some()=>{},
            ended=workers.join_next(), if !workers.is_empty()=>{
                finish!(ended.unwrap().unwrap_or(Err(TransportError::Io)), RouteWorker);
            }
            next=timeout_at(read_deadline,ws.next())=>{
                if Instant::now()>=read_deadline {finish!(Err(TransportError::Deadline), IoBudget);}
                match next {
                    Err(_)=>finish!(Err(TransportError::Deadline), IoBudget),
                    Ok(Some(Ok(Message::Binary(bytes))))=>{
                        if let Err(e)=deliver(&bytes,&incoming){finish!(Err(e), Delivery);}
                        read_deadline=match Instant::now().checked_add(budget){Some(d)=>cap(d),None=>finish!(Err(TransportError::Limit), IoBudget)};
                    }
                    Ok(Some(Ok(Message::Ping(_))))=>{
                        let sent=tokio::select! {
                            biased;
                            _=cancel.changed()=>Err(TransportError::Cancelled),
                            _=original_cancelled(&mut original_cancel)=>Err(TransportError::Cancelled),
                            r=timeout_at(read_deadline,ws.flush())=>r.map_err(|_|TransportError::Deadline).and_then(|r|r.map_err(|_|TransportError::Io)),
                        };
                        if let Err(e)=sent {finish!(Err(e), SocketWrite);}
                        if registration.is_some()&&Instant::now()>=read_deadline{finish!(Err(TransportError::Deadline),IoBudget);}
                    }
                    Ok(Some(Ok(Message::Pong(_))))=>{}
                    Ok(Some(Ok(Message::Close(_))))=>finish!(Err(TransportError::Closed), WebSocketClose),
                    Ok(None)=>finish!(Err(TransportError::Closed), WebSocketEof),
                    Ok(Some(Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed
                        |tokio_tungstenite::tungstenite::Error::AlreadyClosed
                        |tokio_tungstenite::tungstenite::Error::Protocol(tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake))))=>finish!(Err(TransportError::Closed), WebSocketReset),
                    Ok(Some(Err(tokio_tungstenite::tungstenite::Error::Io(_))))=>finish!(Err(TransportError::Io), SocketIo),
                    _=>finish!(Err(TransportError::Wire), Wire),
                }
            }
            outgoing=out_rx.recv(), if !out_rx.is_closed()=>{
                if let Some(bytes)=outgoing {
                    let sent=tokio::select! {
                        biased;
                        _=cancel.changed()=>Err(TransportError::Cancelled),
                        _=original_cancelled(&mut original_cancel)=>Err(TransportError::Cancelled),
                        r=timeout_at(read_deadline,ws.send(Message::Binary(bytes.into())))=>r.map_err(|_|TransportError::Deadline).and_then(|r|r.map_err(|_|TransportError::Io)),
                    };
                    if let Err(e)=sent {finish!(Err(e), SocketWrite);}
                    if registration.is_some()&&Instant::now()>=read_deadline{finish!(Err(TransportError::Deadline),IoBudget);}
                }
            }
        }
        // Buffered network messages may complete without a scheduler suspension.
        // Let bounded route producers run even on a current-thread runtime.
        tokio::task::yield_now().await;
    };
    workers.abort_all();
    while workers.join_next().await.is_some() {}
    #[cfg(feature = "test-only-loopback")]
    if let Some(observer) = observer { let _ = observer.send(observed_end); }
    result
}
type Routes = std::collections::HashMap<[u8; 16], tokio::sync::mpsc::Sender<Vec<u8>>>;
fn deliver(bytes: &[u8], incoming: &Routes) -> Result<(), TransportError> {
    let frames = cc_desk_remote_protocol::decode_outer(bytes).map_err(|_| TransportError::Wire)?;
    if frames.iter().any(|f| !incoming.contains_key(&f.tunnel)) {
        return Err(TransportError::UnknownTunnel);
    }
    let mut ids = std::collections::HashSet::new();
    let mut reservations = Vec::with_capacity(frames.len());
    for frame in &frames {
        if !ids.insert(frame.tunnel) {
            return Err(TransportError::Wire);
        }
        reservations.push(
            incoming[&frame.tunnel]
                .clone()
                .try_reserve_owned()
                .map_err(|_| TransportError::Backpressure)?,
        );
    }
    for (permit, frame) in reservations.into_iter().zip(frames) {
        permit.send(frame.payload.to_vec());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use cc_desk_remote_protocol::{Subframe, encode_outer};
    #[tokio::test]
    async fn abrupt_owned_websocket_peer_exit_is_closed_not_malformed_wire() {
        let (client, peer) = tokio::io::duplex(65536);
        let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
            client,
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            Some(websocket_config()),
        )
        .await;
        let (_app, route) = bounded_tunnel();
        let (_stop, token) = tokio::sync::watch::channel(false);
        drop(peer);
        assert_eq!(
            drive(
                ws,
                std::collections::HashMap::from([([9; 16], route)]),
                token,
                std::time::Duration::from_secs(1)
            )
            .await,
            Err(TransportError::Closed)
        );
    }
    #[test]
    fn full_later_queue_cannot_partially_deliver_earlier_frame() {
        let (a, mut ar) = tokio::sync::mpsc::channel(4);
        let (b, _br) = tokio::sync::mpsc::channel(4);
        for _ in 0..4 {
            b.try_send(vec![0]).unwrap();
        }
        let routes = Routes::from([([1; 16], a), ([2; 16], b)]);
        let msg = encode_outer(&[
            Subframe {
                tunnel: [1; 16],
                payload: b"a",
            },
            Subframe {
                tunnel: [2; 16],
                payload: b"b",
            },
        ])
        .unwrap();
        assert_eq!(deliver(&msg, &routes), Err(TransportError::Backpressure));
        assert!(matches!(
            ar.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
    #[test]
    fn duplicate_route_is_rejected_before_any_delivery() {
        let (a, mut ar) = tokio::sync::mpsc::channel(4);
        let routes = Routes::from([([1; 16], a)]);
        let msg = encode_outer(&[
            Subframe {
                tunnel: [1; 16],
                payload: b"a",
            },
            Subframe {
                tunnel: [1; 16],
                payload: b"b",
            },
        ])
        .unwrap();
        assert_eq!(deliver(&msg, &routes), Err(TransportError::Wire));
        assert!(matches!(
            ar.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
}
