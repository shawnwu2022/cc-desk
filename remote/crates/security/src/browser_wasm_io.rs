use std::{
    io,
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
const CAP: usize = 16 * 1024;
#[link(wasm_import_module = "cc_desk_browser_owner_v1")]
unsafe extern "C" {
    fn read(pointer: *mut u8, capacity: u32) -> i32;
    fn write(pointer: *const u8, length: u32) -> i32;
    fn flush() -> i32;
    fn shutdown() -> i32;
    fn close_io();
}
struct Signals {
    read: Mutex<Option<Waker>>,
    write: Mutex<Option<Waker>>,
    end: Mutex<Option<Waker>>,
    ended: AtomicBool,
    closed: AtomicBool,
}
static SIGNALS: OnceLock<Arc<Signals>> = OnceLock::new();
pub(crate) struct BrowserIo {
    signals: Arc<Signals>,
}
impl BrowserIo {
    pub(crate) fn new() -> Result<Self, io::Error> {
        let signals = Arc::new(Signals {
            read: Mutex::new(None),
            write: Mutex::new(None),
            end: Mutex::new(None),
            ended: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        });
        SIGNALS
            .set(signals.clone())
            .map_err(|_| io::Error::other("browser IO reused"))?;
        Ok(Self { signals })
    }
}
pub(crate) async fn ended() {
    std::future::poll_fn(|cx| {
        let signals = SIGNALS.get().expect("owned browser IO");
        *signals.end.lock().unwrap() = Some(cx.waker().clone());
        if signals.ended.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}
pub(crate) fn notify(kind: u32) {
    if let Some(s) = SIGNALS.get() {
        if s.closed.load(Ordering::SeqCst) {
            return;
        }
        let wake = if kind == 1 {
            s.read.lock().unwrap().take()
        } else if kind == 2 {
            s.write.lock().unwrap().take()
        } else {
            s.ended.store(true, Ordering::SeqCst);
            s.end.lock().unwrap().take()
        };
        // 唤醒会进入JS；必须先释放所有锁。
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl AsyncRead for BrowserIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        *self.signals.read.lock().unwrap() = Some(cx.waker().clone());
        let mut bytes = [0u8; CAP];
        let capacity = out.remaining().min(CAP);
        let result = unsafe { read(bytes.as_mut_ptr(), capacity as u32) };
        match result {
            -1 => Poll::Pending,
            0 => Poll::Ready(Ok(())),
            n if n > 0 && n as usize <= capacity => {
                out.put_slice(&bytes[..n as usize]);
                Poll::Ready(Ok(()))
            }
            _ => Poll::Ready(Err(io::Error::other("browser read failed"))),
        }
    }
}
impl AsyncWrite for BrowserIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        *self.signals.write.lock().unwrap() = Some(cx.waker().clone());
        let capacity = bytes.len().min(CAP);
        match unsafe { write(bytes.as_ptr(), capacity as u32) } {
            -1 => Poll::Pending,
            n if n > 0 && n as usize <= capacity => Poll::Ready(Ok(n as usize)),
            _ => Poll::Ready(Err(io::Error::other("browser write failed"))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        *self.signals.write.lock().unwrap() = Some(cx.waker().clone());
        match unsafe { flush() } {
            1 => Poll::Ready(Ok(())),
            -1 => Poll::Pending,
            _ => Poll::Ready(Err(io::Error::other("browser flush failed"))),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        *self.signals.write.lock().unwrap() = Some(cx.waker().clone());
        match unsafe { shutdown() } {
            1 => Poll::Ready(Ok(())),
            -1 => Poll::Pending,
            _ => Poll::Ready(Err(io::Error::other("browser shutdown failed"))),
        }
    }
}
impl Drop for BrowserIo {
    fn drop(&mut self) {
        self.signals.closed.store(true, Ordering::SeqCst);
        self.signals.read.lock().unwrap().take();
        self.signals.write.lock().unwrap().take();
        self.signals.end.lock().unwrap().take();
        unsafe { close_io() };
    }
}
