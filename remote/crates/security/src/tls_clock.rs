use crate::SecurityError;
use std::time::Duration;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) type Deadline = tokio::time::Instant;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn after(budget: Duration) -> Result<Deadline, SecurityError> {
    tokio::time::Instant::now()
        .checked_add(budget)
        .ok_or(SecurityError::Limit)
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn expired(deadline: Deadline) -> bool {
    tokio::time::Instant::now() >= deadline
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) async fn sleep(deadline: Deadline) {
    tokio::time::sleep_until(deadline).await;
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn native(deadline: Deadline) -> Option<tokio::time::Instant> {
    Some(deadline)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod browser {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::Mutex,
        task::{Context, Poll, Waker},
    };
    #[derive(Clone, Copy)]
    pub(crate) struct Deadline(f64);
    #[link(wasm_import_module = "cc_desk_browser_owner_v1")]
    unsafe extern "C" {
        fn now() -> f64;
        fn deadline() -> f64;
        fn arm_timer(value: f64);
        fn disarm_timer();
    }
    static WAITER: Mutex<Option<Waker>> = Mutex::new(None);
    pub(crate) fn after(budget: Duration) -> Result<Deadline, SecurityError> {
        let current = unsafe { now() };
        let original = unsafe { deadline() };
        if !current.is_finite()
            || !original.is_finite()
            || original <= current
            || original - current > 10_000.0
        {
            return Err(SecurityError::Deadline);
        }
        Ok(Deadline(
            original.min(current + budget.as_secs_f64() * 1000.0),
        ))
    }
    pub(crate) fn expired(value: Deadline) -> bool {
        let current = unsafe { now() };
        !current.is_finite() || current >= value.0
    }
    pub(crate) fn native(_: Deadline) -> Option<tokio::time::Instant> {
        None
    }
    pub(crate) fn notify() {
        let wake = WAITER.lock().unwrap().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(crate) struct Sleep {
        end: Deadline,
    }
    pub(crate) fn sleep(end: Deadline) -> Sleep {
        Sleep { end }
    }
    impl Future for Sleep {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if expired(self.end) {
                return Poll::Ready(());
            }
            *WAITER.lock().unwrap() = Some(cx.waker().clone());
            // 注册真实waker后才启动JS timer，不持锁跨入JS；醒后必须重新采样。
            unsafe { arm_timer(self.end.0) };
            Poll::Pending
        }
    }
    impl Drop for Sleep {
        fn drop(&mut self) {
            WAITER.lock().unwrap().take();
            unsafe { disarm_timer() };
        }
    }
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use browser::*;
