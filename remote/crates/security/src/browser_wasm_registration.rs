//! Independent, single-use enrollment owner; never creates an application channel.
use crate::registration_channel::{RegistrationError, registration_connect};
use crate::{
    BrowserWasmSigningKey, CancelHandle, LocalIdentity, P256Spki, PeerPin, Role, SecurityError,
    Unbound, browser_wasm_io::BrowserIo, cancellation,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};
#[link(wasm_import_module = "cc_desk_browser_registration_v1")]
unsafe extern "C" {
    fn deadline() -> f64;
    fn request(kind: u32, pointer: *const u8, length: u32) -> i32;
    fn response(kind: u32, output: *mut u8, capacity: u32) -> i32;
    fn outcome(pointer: *const u8, length: u32);
}
#[link(wasm_import_module = "cc_desk_browser_owner_v1")]
unsafe extern "C" {
    fn public_key(which: u32, output: *mut u8, capacity: u32) -> i32;
    #[link_name = "deadline"]
    fn setup_deadline() -> f64;
    fn schedule();
    fn report(
        code: i32,
        protocol: u32,
        cipher: u32,
        group: u32,
        flags: u32,
        peer: *const u8,
        length: u32,
    );
}
pub(crate) fn total_deadline() -> f64 {
    unsafe { deadline() }
}
static STARTED: AtomicBool = AtomicBool::new(false);
static PRODUCER: Mutex<Option<Waker>> = Mutex::new(None);
type Task = Pin<Box<dyn Future<Output = Result<(), i32>>>>;
thread_local! {
    static TASK:RefCell<Option<Task>>=const{RefCell::new(None)};
    static CANCEL:RefCell<Option<CancelHandle>>=const{RefCell::new(None)};
    static POLLING:Cell<bool>=const{Cell::new(false)};
}
struct WorkerWake;
impl Wake for WorkerWake {
    fn wake(self: Arc<Self>) {
        unsafe { schedule() }
    }
    fn wake_by_ref(self: &Arc<Self>) {
        unsafe { schedule() }
    }
}
fn security(e: SecurityError) -> i32 {
    match e {
        SecurityError::Cancelled => -3,
        SecurityError::Deadline => -2,
        _ => -1,
    }
}
fn registration(e: RegistrationError) -> i32 {
    match e {
        RegistrationError::Cancelled => -3,
        RegistrationError::Deadline => -2,
        _ => -1,
    }
}
fn key(which: u32) -> Result<P256Spki, SecurityError> {
    let mut b = [0; 91];
    if unsafe { public_key(which, b.as_mut_ptr(), 91) } != 91 {
        return Err(SecurityError::Key);
    }
    P256Spki::new(&b)
}
async fn produce(kind: u32, bytes: &[u8]) -> Result<Vec<u8>, i32> {
    if bytes.is_empty()
        || bytes.len() > 4096
        || unsafe { request(kind, bytes.as_ptr(), bytes.len() as u32) } != 1
    {
        return Err(-1);
    }
    let mut output = vec![0; 16384];
    std::future::poll_fn(|cx| {
        *PRODUCER.lock().unwrap() = Some(cx.waker().clone());
        let n = unsafe { response(kind, output.as_mut_ptr(), output.len() as u32) };
        if n == -1 {
            return Poll::Pending;
        }
        PRODUCER.lock().unwrap().take();
        if n <= 0 || n as usize > output.len() {
            Poll::Ready(Err(-1))
        } else {
            Poll::Ready(Ok(output[..n as usize].to_vec()))
        }
    })
    .await
}
pub extern "C" fn browser_registration_start(setup: u32, total: u32) -> i32 {
    if STARTED.swap(true, Ordering::SeqCst)
        || setup == 0
        || setup > 10000
        || total == 0
        || total > 120000
    {
        return -1;
    }
    let prepared = (|| {
        let public = key(0)?;
        let signer = Arc::new(BrowserWasmSigningKey::new(public.clone(), unsafe {
            setup_deadline()
        })?);
        let local = LocalIdentity::new(Role::Client, public, signer)?;
        let pin = PeerPin::new(Role::Host, key(1)?);
        let io = BrowserIo::new().map_err(|_| SecurityError::Tls)?;
        let end = crate::owner_clock::Instant::from_owner_deadline(total_deadline())
            .ok_or(SecurityError::Deadline)?;
        let (cancel, token) = cancellation();
        CANCEL.with(|s| *s.borrow_mut() = Some(cancel));
        let task: Task = Box::pin(async move {
            let work = async {
                let pending = registration_connect(
                    Unbound::new(io),
                    local,
                    pin,
                    Duration::from_millis(setup as u64),
                    Duration::from_millis(total as u64),
                    token.clone(),
                )
                .await
                .map_err(security)?;
                let e = pending.evidence();
                unsafe {
                    report(
                        1,
                        u16::from(e.protocol) as u32,
                        u16::from(e.cipher) as u32,
                        u16::from(e.group) as u32,
                        (e.full_handshake as u32)
                            | ((e.raw_public_key as u32) << 1)
                            | ((e.alpn_matches as u32) << 2),
                        e.peer_endpoint_id.as_ptr(),
                        32,
                    )
                };
                let mut channel = pending.into_channel().map_err(registration)?;
                let create = channel.client_create().await.map_err(registration)?;
                let created = produce(1, &create).await?;
                let confirm = channel
                    .client_created(&created)
                    .await
                    .map_err(registration)?;
                let asserted = produce(2, &confirm).await?;
                let result = channel
                    .client_asserted(&asserted)
                    .await
                    .map_err(registration)?;
                let bytes = serde_json::to_vec(&result).map_err(|_| -1)?;
                if bytes.len() > 16384 {
                    return Err(-1);
                }
                unsafe { outcome(bytes.as_ptr(), bytes.len() as u32) };
                channel.client_ack().await.map_err(registration)
            };
            let mut cancellation = token.clone();
            tokio::select! {biased;_=cancellation.cancelled()=>Err(-3),_=crate::owner_clock::sleep_until(end)=>Err(-2),result=work=>result}
        });
        TASK.with(|s| *s.borrow_mut() = Some(task));
        Ok::<_, SecurityError>(())
    })();
    match prepared {
        Ok(()) => 1,
        Err(e) => {
            unsafe { report(security(e), 0, 0, 0, 0, std::ptr::null(), 0) };
            -1
        }
    }
}
pub extern "C" fn browser_registration_poll() {
    if POLLING.with(|s| s.replace(true)) {
        return;
    }
    if let Some(mut task) = TASK.with(|s| s.borrow_mut().take()) {
        let waker = Waker::from(Arc::new(WorkerWake));
        match task.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Pending => TASK.with(|s| *s.borrow_mut() = Some(task)),
            Poll::Ready(result) => {
                drop(task);
                CANCEL.with(|s| s.borrow_mut().take());
                PRODUCER.lock().unwrap().take();
                unsafe { report(result.err().unwrap_or(-5), 0, 0, 0, 0, std::ptr::null(), 0) }
            }
        }
    }
    POLLING.with(|s| s.set(false));
}
pub extern "C" fn browser_registration_cancel() {
    if let Some(c) = CANCEL.with(|s| s.borrow().clone()) {
        c.cancel()
    }
    unsafe { schedule() }
}
pub extern "C" fn browser_registration_event(kind: u32) {
    match kind {
        6 => {
            let wake = PRODUCER.lock().unwrap().take();
            if let Some(w) = wake {
                w.wake()
            }
        }
        1 | 2 | 4 => crate::browser_wasm_io::notify(kind),
        3 => crate::tls_clock::notify(),
        _ => {}
    }
}
