//! 封闭的原生委托能力；生产没有 consent 或绑定快照构造入口。
#![allow(dead_code, reason = "分阶段原生接线尚未开放授权入口")]
use super::*;
use cc_desk_remote_host::native_host::{NativeFrameResult, NativeIoError, NativeLocalInput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::TryLockError;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub(crate) struct VerifiedBinding(Binding);

pub(crate) struct Delegation {
    owner: Owner,
    closed: Arc<AtomicBool>,
    uncertain: AtomicBool,
    clock: Clock,
}
pub(crate) struct Ticket(Attempt);
#[derive(Clone)]
enum Clock {
    System,
    #[cfg(test)]
    Test(Arc<Mutex<Result<Sample, Denied>>>),
}
impl Clock {
    fn sample(&self) -> Result<Sample, Denied> {
        match self {
            Self::System => Ok(Sample {
                wall_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Denied::Locked)?
                    .as_millis()
                    .try_into()
                    .map_err(|_| Denied::Bounds)?,
                monotonic: Instant::now(),
            }),
            #[cfg(test)]
            Self::Test(value) => value.lock().map(|s| *s).map_err(|_| Denied::Unknown)?,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloseObservation {
    Retiring,
    EffectsClosed,
    Unknown,
}

// fence 仅供已持有 Owner 锁的同步 effect 回调；不能独立发行下一次许可。
struct EffectFence {
    closed: Arc<AtomicBool>,
    clock: Clock,
    wall_floor: u64,
    mono_floor: Instant,
    cutoff: u64,
    end: Instant,
}
impl EffectFence {
    fn check(&self) -> bool {
        let valid = !self.closed.load(Ordering::Acquire)
            && self.clock.sample().is_ok_and(|now| {
                now.wall_ms >= self.wall_floor
                    && now.wall_ms < self.cutoff
                    && now.monotonic >= self.mono_floor
                    && now.monotonic < self.end
            });
        if !valid {
            self.closed.store(true, Ordering::Release);
        }
        valid
    }
}
impl Delegation {
    fn sample(&self) -> Result<Sample, Denied> {
        self.clock.sample().inspect_err(|_| self.request_revoke())
    }
    pub(crate) fn commit(
        &self,
        ticket: &Ticket,
        current: &VerifiedBinding,
        input: NativeLocalInput,
        bytes: &[u8],
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<Result<NativeFrameResult, NativeIoError>, Denied> {
        self.with_effect(ticket, current, move |fence| {
            let result = input.commit(bytes, move || fence.check() && authorize() && fence.check());
            self.record_frame_result(&result);
            result
        })
    }
    pub(crate) fn resize(
        &self,
        ticket: &Ticket,
        current: &VerifiedBinding,
        input: NativeLocalInput,
        size: (u16, u16),
        authorize: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<Result<(), NativeIoError>, Denied> {
        self.with_effect(ticket, current, move |fence| {
            let result = input.resize(size.0, size.1, move || {
                fence.check() && authorize() && fence.check()
            });
            if let Err(error) = &result {
                self.record_error(error);
            }
            result
        })
    }

    fn record_error(&self, error: &NativeIoError) {
        if !matches!(error, NativeIoError::Policy) {
            self.uncertain.store(true, Ordering::Release);
        }
        self.request_revoke();
    }
    fn record_frame_result(&self, result: &Result<NativeFrameResult, NativeIoError>) {
        match result {
            Ok(frame) if frame.complete => {}
            Ok(_) => {
                self.uncertain.store(true, Ordering::Release);
                self.request_revoke();
            }
            Err(error) => self.record_error(error),
        }
    }
    pub(crate) fn request_revoke(&self) {
        self.closed.store(true, Ordering::Release);
    }
    pub(crate) fn observe_close(&self) -> CloseObservation {
        if self.uncertain.load(Ordering::Acquire) {
            return CloseObservation::Unknown;
        }
        if !self.closed.load(Ordering::Acquire) {
            return CloseObservation::Retiring;
        }
        match self.owner.state.try_lock() {
            Ok(mut s) => {
                Owner::close(&mut s, Gate::Revoked);
                CloseObservation::EffectsClosed
            }
            Err(TryLockError::WouldBlock) => CloseObservation::Retiring,
            Err(TryLockError::Poisoned(_)) => CloseObservation::Unknown,
        }
    }
    pub(crate) fn begin(&self, current: &VerifiedBinding) -> Result<Ticket, Denied> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Denied::Locked);
        }
        let mut s = match self.owner.state.try_lock() {
            Ok(s) => s,
            Err(TryLockError::WouldBlock) => return Err(Denied::Busy),
            Err(TryLockError::Poisoned(_)) => {
                self.request_revoke();
                return Err(Denied::Unknown);
            }
        };
        if self.closed.load(Ordering::Acquire) {
            return Err(Denied::Locked);
        }
        let ticket = self
            .owner
            .begin_locked(&mut s, &current.0, self.sample()?)
            .inspect_err(|error| {
                if *error != Denied::Busy {
                    self.request_revoke();
                }
            })?;
        if self.closed.load(Ordering::Acquire) {
            return Err(Denied::Locked);
        }
        Ok(Ticket(ticket))
    }
    fn with_effect<T>(
        &self,
        ticket: &Ticket,
        current: &VerifiedBinding,
        operation: impl FnOnce(EffectFence) -> T,
    ) -> Result<T, Denied> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Denied::Locked);
        }
        // 不等待另一个无界 actor 操作，也不从 actor 回调重入这把锁。
        let mut s = match self.owner.state.try_lock() {
            Ok(s) => s,
            Err(TryLockError::WouldBlock) => return Err(Denied::Busy),
            Err(TryLockError::Poisoned(_)) => {
                self.request_revoke();
                return Err(Denied::Unknown);
            }
        };
        if self.closed.load(Ordering::Acquire) {
            return Err(Denied::Locked);
        }
        if current.0.role != Role::Control {
            self.request_revoke();
            return Err(Denied::Locked);
        }
        if let Err(error) =
            self.owner
                .check_attempt_locked(&mut s, &ticket.0, &current.0, self.sample()?, false)
        {
            self.request_revoke();
            return Err(error);
        }
        let fence = EffectFence {
            closed: self.closed.clone(),
            clock: self.clock.clone(),
            wall_floor: s.wall_floor,
            mono_floor: s.mono_floor.ok_or(Denied::Locked)?,
            cutoff: s.cutoff_ms,
            end: s.live.as_ref().ok_or(Denied::Locked)?.end,
        };
        // 锁跨同步提交、原 PTY effect 和答复；close 请求不需要等待这把锁。
        let result = operation(fence);
        if self.closed.load(Ordering::Acquire) {
            Owner::close(
                &mut s,
                if self.uncertain.load(Ordering::Acquire) {
                    Gate::Unknown
                } else {
                    Gate::Revoked
                },
            );
        }
        Ok(result)
    }
    #[cfg(test)]
    pub(crate) fn test_new(run: [u8; 32], view: bool) -> (Arc<Self>, VerifiedBinding) {
        let binding = Binding {
            installation: [1; 32],
            device: [2; 32],
            key_fingerprints: [[3; 32]; 4],
            enrollment_revision: 1,
            issuer: [4; 32],
            principal: [5; 32],
            host_epoch: 1,
            client_epoch: 1,
            policy_epoch: 1,
            issuer_epoch: 1,
            run,
            origin: "TEST_ONLY".into(),
            rp: "TEST_ONLY".into(),
            relay_endpoint: "TEST_ONLY".into(),
            route: [6; 16],
            role: if view { Role::View } else { Role::Control },
        };
        let now = Sample {
            wall_ms: 1000,
            monotonic: Instant::now(),
        };
        let owner = Owner::disabled();
        owner
            .enable(
                NativeConsent {
                    binding: binding.clone(),
                    revision: 1,
                    cutoff_ms: 31000,
                },
                now,
            )
            .unwrap();
        (
            Arc::new(Self {
                owner,
                closed: Arc::new(AtomicBool::new(false)),
                uncertain: AtomicBool::new(false),
                clock: Clock::Test(Arc::new(Mutex::new(Ok(now)))),
            }),
            VerifiedBinding(binding),
        )
    }
    #[cfg(test)]
    pub(crate) fn test_clock(&self, failure: bool) {
        if let Clock::Test(clock) = &self.clock {
            *clock.lock().unwrap() = if failure {
                Err(Denied::Locked)
            } else {
                Ok(Sample {
                    wall_ms: 999,
                    monotonic: Instant::now(),
                })
            };
        }
    }
    #[cfg(test)]
    pub(crate) fn test_restore_clock(&self) {
        if let Clock::Test(clock) = &self.clock {
            *clock.lock().unwrap() = Ok(Sample {
                wall_ms: 1000,
                monotonic: Instant::now(),
            });
        }
    }
    #[cfg(test)]
    pub(crate) fn test_expire(&self) {
        if let Clock::Test(clock) = &self.clock {
            clock.lock().unwrap().as_mut().unwrap().wall_ms = 31000;
        }
    }
}
#[cfg(test)]
impl VerifiedBinding {
    pub(crate) fn test_wrong_device(&self) -> Self {
        let mut next = self.clone();
        next.0.device[0] ^= 1;
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(non_snake_case)]
    fn DelegatedGate_UncertainFrameNeverBecomesEffectsClosed_001() {
        for partial in [true, false] {
            let (policy, current) = Delegation::test_new([7; 32], false);
            let result = if partial {
                Ok(NativeFrameResult {
                    complete: false,
                    confirmed_bytes: 2,
                })
            } else {
                Err(NativeIoError::Thread)
            };
            policy.record_frame_result(&result);
            if partial {
                assert_eq!(result.unwrap().confirmed_bytes, 2);
            }
            assert_eq!(policy.observe_close(), CloseObservation::Unknown);
            assert!(policy.begin(&current).is_err());
            policy.request_revoke();
            assert_eq!(policy.observe_close(), CloseObservation::Unknown);
        }
        let (policy, _) = Delegation::test_new([7; 32], false);
        policy.record_frame_result(&Err(NativeIoError::Policy));
        assert_eq!(policy.observe_close(), CloseObservation::EffectsClosed);
    }
}

#[path = "network.rs"]
pub(crate) mod network;
