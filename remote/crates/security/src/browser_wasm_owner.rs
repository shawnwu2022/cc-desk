use crate::{
    BrowserWasmSigningKey, CancelHandle, LocalIdentity, P256Spki, PeerPin, Role, SecurityError,
    Unbound, browser_wasm_io::BrowserIo, cancellation, connect,
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
use tokio::io::AsyncWriteExt;
#[link(wasm_import_module = "cc_desk_browser_owner_v1")]
unsafe extern "C" {
    fn public_key(which: u32, output: *mut u8, capacity: u32) -> i32;
    fn deadline() -> f64;
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
static STARTED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_WAKER: Mutex<Option<Waker>> = Mutex::new(None);
async fn shutdown_requested() {
    std::future::poll_fn(|cx| {
        *SHUTDOWN_WAKER.lock().unwrap() = Some(cx.waker().clone());
        if SHUTDOWN.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}
type Task = Pin<Box<dyn Future<Output = Result<(), SecurityError>>>>;
thread_local! {
    static TASK: RefCell<Option<Task>> = const { RefCell::new(None) };
    static CANCEL: RefCell<Option<CancelHandle>> = const { RefCell::new(None) };
    static POLLING: Cell<bool> = const { Cell::new(false) };
}
struct WorkerWake;
impl Wake for WorkerWake {
    fn wake(self: Arc<Self>) {
        unsafe { schedule() };
    }
    fn wake_by_ref(self: &Arc<Self>) {
        unsafe { schedule() };
    }
}
fn error_code(error: SecurityError) -> i32 {
    match error {
        SecurityError::Deadline => -2,
        SecurityError::Cancelled => -3,
        SecurityError::Role => -4,
        _ => -1,
    }
}
fn key(which: u32) -> Result<P256Spki, SecurityError> {
    let mut bytes = [0u8; 91];
    if unsafe { public_key(which, bytes.as_mut_ptr(), 91) } != 91 {
        return Err(SecurityError::Key);
    }
    P256Spki::new(&bytes)
}
/// 生产入口只接收固定枚举/预算；公开身份来自私有已登记元数据 import。
pub extern "C" fn browser_tls_start(local_role: u32, peer_role: u32, budget_ms: u32) -> i32 {
    if STARTED.swap(true, Ordering::SeqCst)
        || budget_ms == 0
        || budget_ms > 10_000
        || local_role > 1
        || peer_role > 1
    {
        return -1;
    }
    let setup = (|| {
        let public = key(0)?;
        let peer = key(1)?;
        let signing = Arc::new(BrowserWasmSigningKey::new(public.clone(), unsafe {
            deadline()
        })?);
        let local = LocalIdentity::new(
            if local_role == 0 {
                Role::Client
            } else {
                Role::Host
            },
            public,
            signing,
        )?;
        let pin = PeerPin::new(
            if peer_role == 0 {
                Role::Client
            } else {
                Role::Host
            },
            peer,
        );
        let io = BrowserIo::new().map_err(|_| SecurityError::Tls)?;
        let (cancel, mut token) = cancellation();
        CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
        let task: Task = Box::pin(async move {
            let mut pending = connect(
                Unbound::new(io),
                local,
                pin,
                Duration::from_millis(budget_ms as u64),
                token.clone(),
            )
            .await?;
            if token.is_cancelled() {
                return Err(SecurityError::Cancelled);
            }
            if crate::tls_clock::expired(pending._deadline) {
                return Err(SecurityError::Deadline);
            }
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
            // 原Pending仍归本owner持有；暂不接AppBind，不发布应用权限。
            let result = tokio::select! { biased;
                _=token.cancelled()=>Err(SecurityError::Cancelled),
                _=crate::tls_clock::sleep(pending._deadline)=>Err(SecurityError::Deadline),
                _=shutdown_requested()=>{
                    tokio::select!{biased;
                        _=token.cancelled()=>Err(SecurityError::Cancelled),
                        _=crate::tls_clock::sleep(pending._deadline)=>Err(SecurityError::Deadline),
                        result=pending._io.shutdown()=>result.map_err(|_|SecurityError::Tls),
                    }
                },
                _=crate::browser_wasm_io::ended()=>Err(SecurityError::Tls),
            };
            drop(pending);
            result
        });
        TASK.with(|slot| *slot.borrow_mut() = Some(task));
        Ok(())
    })();
    match setup {
        Ok(()) => 1,
        Err(error) => {
            unsafe { report(error_code(error), 0, 0, 0, 0, std::ptr::null(), 0) };
            -1
        }
    }
}
pub extern "C" fn browser_tls_poll() {
    if POLLING.with(|flag| flag.replace(true)) {
        return;
    }
    let task = TASK.with(|slot| slot.borrow_mut().take());
    if let Some(mut task) = task {
        let waker = Waker::from(Arc::new(WorkerWake));
        let mut cx = Context::from_waker(&waker);
        // poll及其JS回调期间不持有RefCell借用或Mutex。
        match task.as_mut().poll(&mut cx) {
            Poll::Pending => TASK.with(|slot| *slot.borrow_mut() = Some(task)),
            Poll::Ready(result) => {
                drop(task);
                CANCEL.with(|slot| slot.borrow_mut().take());
                unsafe {
                    report(
                        result.err().map_or(-5, error_code),
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        0,
                    )
                };
            }
        }
    }
    POLLING.with(|flag| flag.set(false));
}
pub extern "C" fn browser_tls_cancel() {
    let cancel = CANCEL.with(|slot| slot.borrow().clone());
    if let Some(cancel) = cancel {
        cancel.cancel();
    }
    unsafe { schedule() };
}
pub extern "C" fn browser_tls_event(kind: u32) {
    if kind == 1 || kind == 2 || kind == 4 {
        crate::browser_wasm_io::notify(kind);
    } else if kind == 3 {
        crate::tls_clock::notify();
    } else if kind == 5 {
        SHUTDOWN.store(true, Ordering::SeqCst);
        let wake = SHUTDOWN_WAKER.lock().unwrap().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
