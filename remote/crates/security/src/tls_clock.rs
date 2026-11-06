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
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn exchange_after(budget: Duration) -> Result<Deadline, crate::GrantError> {
    Ok(tokio::time::Instant::now() + budget)
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
    pub(crate) struct Deadline(pub(crate) f64);
    #[link(wasm_import_module = "cc_desk_browser_owner_v1")]
    unsafe extern "C" {
        fn now() -> f64;
        fn deadline() -> f64;
        fn arm_timer(value: f64);
        fn disarm_timer();
        fn grant_deadline(budget_ms: u32) -> f64;
        fn peer_deadline(budget_ms: u32, remaining_ms: u32, sample_before: f64) -> f64;
        fn peer_expiry() -> f64;
    }
    struct Slot {
        generation: u64,
        end: f64,
        wake: Waker,
    }
    struct Registry {
        slots: [Option<Slot>; 8],
        next: u64,
        failed: bool,
    }
    static WAITERS: Mutex<Registry> = Mutex::new(Registry {
        slots: [const { None }; 8],
        next: 1,
        failed: false,
    });
    fn rearm() {
        let next = {
            let slots = WAITERS.lock().unwrap();
            slots.slots.iter().flatten().map(|s| s.end).reduce(f64::min)
        };
        // 不持注册表锁进入受控JS；只保留最早的一个原timer。
        if let Some(end) = next {
            unsafe { arm_timer(end) };
        } else {
            unsafe { disarm_timer() };
        }
    }
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
        WAITERS.lock().unwrap().failed || !current.is_finite() || current >= value.0
    }
    pub(crate) fn native(_: Deadline) -> Option<tokio::time::Instant> {
        None
    }
    pub(crate) fn exchange_after(budget: Duration) -> Result<Deadline, crate::GrantError> {
        let ms = budget.as_millis();
        if ms == 0 || ms > 30000 {
            return Err(crate::GrantError::Policy);
        }
        let end = unsafe { grant_deadline(ms as u32) };
        let current = unsafe { now() };
        if !current.is_finite() || !end.is_finite() || end <= current || end > current + ms as f64 {
            return Err(crate::GrantError::Expired);
        }
        Ok(Deadline(end))
    }
    pub(crate) fn notify() {
        let wakes: Vec<_> = WAITERS
            .lock()
            .unwrap()
            .slots
            .iter()
            .flatten()
            .map(|s| s.wake.clone())
            .collect();
        for wake in wakes {
            wake.wake();
        }
    }
    pub(crate) fn sample_floor() -> Result<f64, crate::DataError> {
        let value = unsafe { now() };
        if WAITERS.lock().unwrap().failed
            || !value.is_finite()
            || !(0.0..=9007199254740991.0).contains(&value)
        {
            return Err(crate::DataError::Deadline);
        }
        Ok(value.floor())
    }
    pub(crate) fn owner_limit(value: f64) -> Option<f64> {
        let limit = unsafe { peer_expiry() };
        (value.is_finite() && limit.is_finite()).then_some(value.min(limit))
    }
    pub(crate) fn peer_times(
        budget: Duration,
        remaining: u64,
        sample_before: f64,
    ) -> Result<(Deadline, Deadline), crate::DataError> {
        let ms = budget.as_millis();
        if ms == 0 || ms > 10000 || remaining == 0 || remaining > 900000 {
            return Err(crate::DataError::Bounds);
        }
        let end = unsafe { peer_deadline(ms as u32, remaining as u32, sample_before) };
        let expiry = unsafe { peer_expiry() };
        let current = unsafe { now() };
        if !current.is_finite()
            || !sample_before.is_finite()
            || sample_before > current
            || !end.is_finite()
            || !expiry.is_finite()
            || end <= current
            || end > current + ms as f64
            || end > expiry
            || expiry > sample_before + remaining as f64
        {
            return Err(crate::DataError::Deadline);
        }
        Ok((Deadline(end), Deadline(expiry)))
    }
    pub(crate) struct Sleep {
        end: Deadline,
        slot: Option<(usize, u64)>,
    }
    pub(crate) fn sleep(end: Deadline) -> Sleep {
        Sleep { end, slot: None }
    }
    impl Future for Sleep {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if expired(self.end) {
                return Poll::Ready(());
            }
            let mut wakes = Vec::new();
            let failed = {
                let mut registry = WAITERS.lock().unwrap();
                if let Some((index, generation)) = self.slot {
                    match &mut registry.slots[index] {
                        Some(slot) if slot.generation == generation => {
                            slot.wake = cx.waker().clone()
                        }
                        _ => registry.failed = true,
                    }
                } else if let Some(index) = registry.slots.iter().position(Option::is_none) {
                    if let Some(next) = registry.next.checked_add(1) {
                        let generation = registry.next;
                        registry.next = next;
                        registry.slots[index] = Some(Slot {
                            generation,
                            end: self.end.0,
                            wake: cx.waker().clone(),
                        });
                        self.slot = Some((index, generation));
                    } else {
                        registry.failed = true;
                    }
                } else {
                    registry.failed = true;
                }
                if registry.failed {
                    wakes.extend(registry.slots.iter().flatten().map(|s| s.wake.clone()));
                }
                registry.failed
            };
            for wake in wakes {
                wake.wake();
            }
            rearm();
            if failed || expired(self.end) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }
    impl Drop for Sleep {
        fn drop(&mut self) {
            if let Some((index, generation)) = self.slot {
                let mut registry = WAITERS.lock().unwrap();
                if registry.slots[index]
                    .as_ref()
                    .is_some_and(|s| s.generation == generation)
                {
                    registry.slots[index] = None;
                }
            }
            rearm();
        }
    }
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use browser::*;
