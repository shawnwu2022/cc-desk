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
#[path = "browser_grant.rs"]
mod browser_grant;
#[path = "browser_peer.rs"]
pub(crate) mod browser_peer;
#[link(wasm_import_module = "cc_desk_browser_owner_v1")]
unsafe extern "C" {
    fn public_key(which: u32, output: *mut u8, capacity: u32) -> i32;
    fn deadline() -> f64;
    fn now() -> f64;
    fn schedule();
    fn client_trust(output: *mut u8, capacity: u32) -> i32;
    fn grant_config(output: *mut u8, capacity: u32) -> i32;
    fn grant_accepted(pointer: *const u8, length: u32);
    fn peer_installed(pointer: *const u8, length: u32);
    fn management(pointer: *const u8, length: u32);
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
type Task = Pin<Box<dyn Future<Output = Result<(), i32>>>>;
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
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustInput {
    host_key_epoch: String,
    client_key_epoch: String,
    origin: String,
}
fn trust() -> Result<Option<crate::ClientTrust>, SecurityError> {
    let mut bytes = [0u8; 2304];
    let length = unsafe { client_trust(bytes.as_mut_ptr(), bytes.len() as u32) };
    if length == 0 {
        return Ok(None);
    }
    if length < 1 || length as usize > bytes.len() {
        return Err(SecurityError::Identity);
    }
    let wire: TrustInput =
        serde_json::from_slice(&bytes[..length as usize]).map_err(|_| SecurityError::Identity)?;
    fn epoch(value: &str) -> Result<u64, SecurityError> {
        if value.is_empty()
            || value.len() > 20
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|c| c.is_ascii_digit())
        {
            return Err(SecurityError::Identity);
        }
        value.parse().map_err(|_| SecurityError::Identity)
    }
    let origin = cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&wire.origin)
        .map_err(|_| SecurityError::Identity)?;
    if origin.as_str() != wire.origin {
        return Err(SecurityError::Identity);
    }
    Ok(Some(crate::ClientTrust {
        host_key_epoch: epoch(&wire.host_key_epoch)?,
        client_key_epoch: epoch(&wire.client_key_epoch)?,
        origin,
    }))
}
fn bind_error(error: crate::BindError) -> i32 {
    match error {
        crate::BindError::Unsupported => -6,
        crate::BindError::Deadline => -2,
        crate::BindError::Cancelled => -3,
        _ => -7,
    }
}
/// 生产入口只接收固定枚举/预算；公开身份来自私有已登记元数据 import。
pub extern "C" fn browser_tls_start(local_role: u32, peer_role: u32, budget_ms: u32) -> i32 {
    start(local_role, peer_role, budget_ms, None)
}
pub enum BrowserApplicationFailure {
    Operation(crate::DataError),
    Retirement(crate::DataError),
}
pub type BrowserApplication =
    fn(
        crate::RunningChannel,
    ) -> Pin<Box<dyn Future<Output = Result<(), BrowserApplicationFailure>>>>;
/// 应用factory由Rust组合根提供，JS不能构造channel或opaque proof。
pub fn browser_application_start(
    local_role: u32,
    peer_role: u32,
    budget_ms: u32,
    application: BrowserApplication,
) -> i32 {
    start(local_role, peer_role, budget_ms, Some(application))
}
fn start(
    local_role: u32,
    peer_role: u32,
    budget_ms: u32,
    application: Option<BrowserApplication>,
) -> i32 {
    if STARTED.swap(true, Ordering::SeqCst)
        || budget_ms == 0
        || budget_ms > 10_000
        || local_role > 1
        || peer_role > 1
    {
        return -1;
    }
    let setup = (|| {
        let trust = trust()?;
        let mut config_bytes = [0u8; 8192];
        let length = unsafe { grant_config(config_bytes.as_mut_ptr(), 8192) };
        let grant = if length == 0 {
            None
        } else {
            if length < 1 || length as usize > config_bytes.len() || trust.is_none() {
                return Err(SecurityError::Identity);
            }
            Some(
                browser_grant::Configuration::parse(&config_bytes[..length as usize])
                    .map_err(|_| SecurityError::Identity)?,
            )
        };
        // 手动续租只属于原应用数据 owner，不能由仅安装 peer 的入口启用。
        if grant.as_ref().is_some_and(|g| g.control_lease.is_some()) && application.is_none() {
            return Err(SecurityError::Identity);
        }
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
            .await
            .map_err(error_code)?;
            if token.is_cancelled() {
                return Err(-3);
            }
            if crate::tls_clock::expired(pending._deadline) {
                return Err(-2);
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
            if let Some(mut config) = grant {
                let trust = trust.ok_or(-7)?;
                let channel = if config.snapshot_pull.is_some() {
                    crate::bind::browser_live_snapshot_pull_client(pending, trust).await
                } else if matches!(
                    config.control_lease,
                    Some(crate::browser_grant_config::ControlLeaseMode::Live)
                ) {
                    crate::bind::browser_live_control_client(pending, trust).await
                } else if config.control_lease.is_some() {
                    crate::bind::browser_control_lease_client(pending, trust).await
                } else if config.peer_budget.is_some() {
                    crate::bind::browser_peer_client(pending, trust).await
                } else {
                    crate::bind::browser_grant_client(pending, trust).await
                }
                .map_err(bind_error)?;
                let mut channel = match &config.trust {
                    crate::browser_grant_config::Trust::Exact(trust) => {
                        let mut state = crate::ClientGrantState::new(browser_grant::clock());
                        channel
                            .grant_client(trust, &mut config.signer, &mut state, config.budget)
                            .await
                    }
                    crate::browser_grant_config::Trust::AuthenticatedHostRun(trust) => {
                        channel
                            .grant_browser_resume(
                                trust,
                                &mut config.signer,
                                browser_grant::clock(),
                                config.budget,
                            )
                            .await
                    }
                }
                .map_err(|_| -8)?;
                let receipt = channel.browser_receipt().map_err(|_| -8)?;
                if token.is_cancelled() {
                    return Err(-3);
                }
                unsafe { grant_accepted(receipt.as_ptr(), receipt.len() as u32) };
                if let Some(budget) = config.peer_budget {
                    if let Some(application) = application {
                        let (running, driver) =
                            channel.browser_prepare_data(budget).map_err(|_| -12)?;
                        let work = async move {
                            let (reason, application) = tokio::join!(driver, application(running));
                            application.map_err(|e| match e {
                                BrowserApplicationFailure::Retirement(_) => -13,
                                BrowserApplicationFailure::Operation(_) => -12,
                            })?;
                            match reason {
                                crate::DataError::Closed | crate::DataError::Cancelled => Ok(()),
                                _ => Err(-12),
                            }
                        };
                        tokio::pin!(work);
                        return tokio::select! {biased;
                            _=shutdown_requested()=>{if let Some(cancel)=CANCEL.with(|slot|slot.borrow().clone()){cancel.cancel();}work.await},
                            result=&mut work=>result,
                        };
                    }
                    let installed = channel.browser_peer(budget).await.map_err(|e| match e {
                        crate::DataError::Deadline => -10,
                        crate::DataError::Cancelled => -3,
                        crate::DataError::Grant(_) => -11,
                        _ => -9,
                    })?;
                    let receipt = installed.receipt().map_err(|_| -9)?;
                    if token.is_cancelled() {
                        return Err(-3);
                    }
                    unsafe { peer_installed(receipt.as_ptr(), receipt.len() as u32) };
                    let (mut revoked, mut superseded) = installed.revoked();
                    installed.check().map_err(|_| -9)?;
                    return tokio::select! {biased;_=token.cancelled()=>Err(-3),_=shutdown_requested()=>tokio::select!{biased;_=token.cancelled()=>Err(-3),_=revoked.changed()=>Err(-11),_=superseded.changed()=>Err(-11),_=crate::tls_clock::sleep(installed.expiry)=>Err(-11),result=channel.browser_shutdown()=>result.map_err(|_|-1)},_=revoked.changed()=>Err(-11),_=superseded.changed()=>Err(-11),_=crate::tls_clock::sleep(installed.expiry)=>Err(-11),result=channel.browser_wait()=>match result{crate::OwnerExit::Unsupported=>Err(-6),_=>Err(-1)}};
                }
                return tokio::select! {biased;_=token.cancelled()=>Err(-3),_=shutdown_requested()=>tokio::select!{biased;_=token.cancelled()=>Err(-3),result=channel.browser_shutdown()=>result.map_err(|_|-1)},result=channel.browser_wait()=>match result{crate::OwnerExit::Unsupported=>Err(-6),_=>Err(-1)}};
            }
            if let Some(trust) = trust {
                let mut established = crate::bind::browser_establish_client(pending, trust)
                    .await
                    .map_err(bind_error)?;
                let facts =
                    crate::bind::browser_management_facts(&established).map_err(bind_error)?;
                if token.is_cancelled() {
                    return Err(-3);
                }
                if crate::tls_clock::expired(established.deadline) {
                    return Err(-2);
                }
                unsafe { management(facts.as_ptr(), facts.len() as u32) };
                // 建立截止不是长期TTL；与原management owner共享读取/拒绝后续字节语义。
                return tokio::select! {biased;
                    _=token.cancelled()=>Err(-3),
                    _=shutdown_requested()=>tokio::select!{biased;_=token.cancelled()=>Err(-3),result=established.io.shutdown()=>result.map_err(|_|-1)},
                    result=crate::bind::management_read(&mut established.io)=>match result{crate::OwnerExit::Unsupported=>Err(-6),_=>Err(-1)},
                };
            }
            // TLS-only模式仍保留原Pending语义。
            let result = tokio::select! { biased;
                _=token.cancelled()=>Err(-3),
                _=crate::tls_clock::sleep(pending._deadline)=>Err(-2),
                _=shutdown_requested()=>{
                    tokio::select!{biased;
                        _=token.cancelled()=>Err(-3),
                        _=crate::tls_clock::sleep(pending._deadline)=>Err(-2),
                        result=pending._io.shutdown()=>result.map_err(|_|-1),
                    }
                },
                _=crate::browser_wasm_io::ended()=>Err(-1),
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
                unsafe { report(result.err().unwrap_or(-5), 0, 0, 0, 0, std::ptr::null(), 0) };
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
    if kind == 6 {
        browser_grant::notify();
    } else if kind == 1 || kind == 2 || kind == 4 {
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
