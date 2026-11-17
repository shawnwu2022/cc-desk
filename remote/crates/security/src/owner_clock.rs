//! 原Native时间语义不变；浏览器只从受控owner的保守连续时钟取值。
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use tokio::time::{Instant, sleep_until, timeout_at};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod browser {
    use std::{future::Future, ops::Add, time::Duration};
    #[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
    pub struct Instant(pub(crate) f64);
    impl Instant {
        pub fn now() -> Self {
            Self(crate::tls_clock::sample_floor().unwrap_or(f64::INFINITY))
        }
        pub fn checked_add(self, duration: Duration) -> Option<Self> {
            let value = self.0 + duration.as_secs_f64() * 1000.0;
            if !self.0.is_finite() || !value.is_finite() || value > 9007199254740991.0 {
                return None;
            }
            crate::tls_clock::owner_limit(value).map(Self)
        }
        pub fn from_owner_deadline(value: f64) -> Option<Self> {
            crate::tls_clock::owner_limit(value)
                .filter(|end| *end == value && value > Self::now().0)
                .map(Self)
        }
        pub fn saturating_duration_since(self, earlier: Self) -> Duration {
            if !self.0.is_finite() || !earlier.0.is_finite() {
                return Duration::ZERO;
            }
            Duration::from_secs_f64(((self.0 - earlier.0).max(0.0)) / 1000.0)
        }
        pub fn min(self, other: Self) -> Self {
            if self.0 <= other.0 { self } else { other }
        }
    }
    impl Add<Duration> for Instant {
        type Output = Self;
        fn add(self, duration: Duration) -> Self {
            self.checked_add(duration).unwrap_or(Self(f64::INFINITY))
        }
    }
    pub async fn sleep_until(end: Instant) {
        crate::tls_clock::sleep(crate::tls_clock::Deadline(end.0)).await;
    }
    pub async fn timeout_at<F: Future>(end: Instant, future: F) -> Result<F::Output, ()> {
        tokio::select! { biased; _=sleep_until(end)=>Err(()), value=future=>Ok(value) }
    }
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use browser::*;
