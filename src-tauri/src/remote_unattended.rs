//! 默认关闭、仅进程内的单 run 授权模型，尚无 UI、IPC 或网络调用者。
//! 授权和回收证明刻意不提供生产构造入口。
//! 本模块不提供持久授权、自动重连循环或输入队列。
#![allow(dead_code)] // 分阶段模型；生产接线须另行审查。

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_DELEGATION_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const ATTEMPT_MS: u64 = 60_000;
const NETWORK_ATTEMPT_MS: u64 = 900_000;
#[derive(Clone, Copy)]
enum AttemptWindow {
    Local,
    Network,
}
impl AttemptWindow {
    fn millis(self) -> u64 {
        match self {
            Self::Local => ATTEMPT_MS,
            Self::Network => NETWORK_ATTEMPT_MS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Binding {
    installation: [u8; 32],
    device: [u8; 32],
    key_fingerprints: [[u8; 32]; 4],
    full_record_digest: [u8; 32],
    enrollment_revision: u64,
    issuer: [u8; 32],
    principal: [u8; 32],
    host_epoch: u64,
    client_epoch: u64,
    policy_epoch: u64,
    issuer_epoch: u64,
    run: [u8; 32],
    origin: String,
    rp: String,
    relay_endpoint: String,
    route: [u8; 16],
    role: Role,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
enum Role {
    View,
    Control,
}
impl Binding {
    fn bounded(&self) -> bool {
        self.enrollment_revision != 0
            && [
                self.host_epoch,
                self.client_epoch,
                self.policy_epoch,
                self.issuer_epoch,
            ]
            .iter()
            .all(|v| *v != 0)
            && [&self.origin, &self.rp, &self.relay_endpoint]
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 2048)
    }
}

// 未来构造入口必须消费已核实的原生登记、run admission 和原生确认，
// 不接受 JS DTO 作为批准。所有字段刻意保持私有。
struct NativeConsent {
    binding: Binding,
    revision: u64,
    cutoff_ms: u64,
}
#[derive(Clone, Copy)]
struct Sample {
    wall_ms: u64,
    monotonic: Instant,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Disabled,
    Enabled,
    Revoked,
    Expired,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Starting,
    Connected,
    Retiring,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Denied {
    Locked,
    Bounds,
    Stale,
    Busy,
    Unknown,
}
struct Live {
    sequence: u64,
    phase: Phase,
    end: Instant,
    wall_end: u64,
}
struct State {
    gate: Gate,
    binding: Option<Binding>,
    revision: u64,
    epoch: u64,
    sequence: u64,
    cutoff_ms: u64,
    end: Option<Instant>,
    wall_floor: u64,
    mono_floor: Option<Instant>,
    live: Option<Live>,
}
struct Owner {
    identity: Arc<()>,
    state: Mutex<State>,
}
// 不可 Clone 或序列化；不携带密钥、断言或输入负载。
struct Attempt {
    owner: Arc<()>,
    revision: u64,
    epoch: u64,
    sequence: u64,
}
// 未来托管适配器只能在该次 socket、密钥及材料所有者、原任务
// 全部具有明确回收证明后生成此见证。
struct RetirementProof {
    owner: Arc<()>,
    sequence: u64,
}
impl Owner {
    fn disabled() -> Self {
        Self {
            identity: Arc::new(()),
            state: Mutex::new(State {
                gate: Gate::Disabled,
                binding: None,
                revision: 0,
                epoch: 0,
                sequence: 0,
                cutoff_ms: 0,
                end: None,
                wall_floor: 0,
                mono_floor: None,
                live: None,
            }),
        }
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>, Denied> {
        match self.state.lock() {
            Ok(s) => Ok(s),
            Err(poison) => {
                let mut s = poison.into_inner();
                s.gate = Gate::Unknown;
                if let Some(live) = &mut s.live {
                    live.phase = Phase::Unknown;
                }
                Err(Denied::Unknown)
            }
        }
    }
    fn enable(&self, consent: NativeConsent, now: Sample) -> Result<(), Denied> {
        let mut s = self.lock()?;
        if s.gate != Gate::Disabled {
            return Err(Denied::Locked);
        }
        let duration = consent
            .cutoff_ms
            .checked_sub(now.wall_ms)
            .filter(|d| *d > 0 && *d <= MAX_DELEGATION_MS)
            .ok_or(Denied::Bounds)?;
        let end = now
            .monotonic
            .checked_add(Duration::from_millis(duration))
            .ok_or(Denied::Bounds)?;
        if consent.revision == 0 || !consent.binding.bounded() {
            return Err(Denied::Bounds);
        }
        s.binding = Some(consent.binding);
        s.revision = consent.revision;
        s.epoch = 1;
        s.cutoff_ms = consent.cutoff_ms;
        s.end = Some(end);
        s.wall_floor = now.wall_ms;
        s.mono_floor = Some(now.monotonic);
        s.gate = Gate::Enabled;
        Ok(())
    }
    fn close(s: &mut State, gate: Gate) {
        s.gate = gate;
        // epoch 溢出也保持终止，不允许回绕恢复授权。
        s.epoch = s.epoch.saturating_add(1);
        if let Some(live) = &mut s.live {
            if live.phase != Phase::Unknown {
                live.phase = Phase::Retiring;
            }
        }
    }
    fn check(s: &mut State, binding: &Binding, now: Sample) -> Result<(), Denied> {
        if s.gate != Gate::Enabled {
            return Err(Denied::Locked);
        }
        if s.binding.as_ref() != Some(binding)
            || now.wall_ms < s.wall_floor
            || s.mono_floor.is_some_and(|floor| now.monotonic < floor)
        {
            Self::close(s, Gate::Revoked);
            return Err(Denied::Locked);
        }
        s.wall_floor = now.wall_ms;
        s.mono_floor = Some(now.monotonic);
        if now.wall_ms >= s.cutoff_ms || s.end.is_none_or(|end| now.monotonic >= end) {
            Self::close(s, Gate::Expired);
            return Err(Denied::Locked);
        }
        Ok(())
    }
    fn begin(&self, binding: &Binding, now: Sample) -> Result<Attempt, Denied> {
        let mut s = self.lock()?;
        self.begin_locked(&mut s, binding, now)
    }
    fn begin_locked(
        &self,
        s: &mut State,
        binding: &Binding,
        now: Sample,
    ) -> Result<Attempt, Denied> {
        self.begin_window_locked(s, binding, now, AttemptWindow::Local)
    }
    fn begin_window_locked(
        &self,
        s: &mut State,
        binding: &Binding,
        now: Sample,
        window: AttemptWindow,
    ) -> Result<Attempt, Denied> {
        Self::check(s, binding, now)?;
        if s.live.is_some() {
            return Err(Denied::Busy);
        }
        let Some(sequence) = s.sequence.checked_add(1) else {
            Self::close(s, Gate::Revoked);
            return Err(Denied::Bounds);
        };
        let remaining = (s.cutoff_ms - now.wall_ms).min(
            u64::try_from(
                s.end
                    .ok_or(Denied::Locked)?
                    .saturating_duration_since(now.monotonic)
                    .as_millis(),
            )
            .map_err(|_| Denied::Bounds)?,
        );
        let duration = window.millis().min(remaining);
        if duration == 0 {
            return Err(Denied::Locked);
        }
        let wall_end = now.wall_ms.checked_add(duration).ok_or(Denied::Bounds)?;
        let end = now
            .monotonic
            .checked_add(Duration::from_millis(duration))
            .ok_or(Denied::Bounds)?
            .min(s.end.ok_or(Denied::Locked)?);
        s.sequence = sequence;
        s.live = Some(Live {
            sequence,
            phase: Phase::Starting,
            end,
            wall_end,
        });
        Ok(Attempt {
            owner: self.identity.clone(),
            revision: s.revision,
            epoch: s.epoch,
            sequence,
        })
    }
    fn same_attempt(&self, s: &State, ticket: &Attempt) -> bool {
        Arc::ptr_eq(&self.identity, &ticket.owner)
            && s.live
                .as_ref()
                .is_some_and(|a| a.sequence == ticket.sequence)
    }
    // 仅观察准入状态，不授予脱锁执行副作用的许可。
    // 后续材料供应和网络接线必须在自身副作用边界重新检查。
    fn check_attempt(
        &self,
        ticket: &Attempt,
        binding: &Binding,
        now: Sample,
        publish: bool,
    ) -> Result<(), Denied> {
        let mut s = self.lock()?;
        self.check_attempt_locked(&mut s, ticket, binding, now, publish)
    }
    fn check_attempt_locked(
        &self,
        s: &mut State,
        ticket: &Attempt,
        binding: &Binding,
        now: Sample,
        publish: bool,
    ) -> Result<(), Denied> {
        Self::check(s, binding, now)?;
        if !self.same_attempt(s, ticket) || ticket.epoch != s.epoch || ticket.revision != s.revision
        {
            return Err(Denied::Stale);
        }
        let live = s.live.as_mut().ok_or(Denied::Stale)?;
        if now.monotonic >= live.end || now.wall_ms >= live.wall_end {
            live.phase = Phase::Retiring;
        }
        match live.phase {
            Phase::Starting => {
                if publish {
                    live.phase = Phase::Connected;
                }
                Ok(())
            }
            Phase::Connected if !publish => Ok(()),
            _ => Err(Denied::Locked),
        }
    }
    fn disconnect(&self, ticket: &Attempt) -> Result<(), Denied> {
        let mut s = self.lock()?;
        if !self.same_attempt(&s, ticket) {
            return Err(Denied::Stale);
        }
        let live = s.live.as_mut().ok_or(Denied::Stale)?;
        if live.phase != Phase::Unknown {
            live.phase = Phase::Retiring;
        }
        Ok(())
    }
    fn retirement_unknown(&self, ticket: &Attempt) -> Result<(), Denied> {
        let mut s = self.lock()?;
        if !self.same_attempt(&s, ticket) {
            return Err(Denied::Stale);
        }
        Self::close(&mut s, Gate::Unknown);
        s.live.as_mut().ok_or(Denied::Stale)?.phase = Phase::Unknown;
        Ok(())
    }
    fn finish_retirement(&self, proof: RetirementProof) -> Result<(), Denied> {
        let mut s = self.lock()?;
        self.finish_retirement_locked(&mut s, proof)
    }
    fn finish_retirement_locked(
        &self,
        s: &mut State,
        proof: RetirementProof,
    ) -> Result<(), Denied> {
        if !Arc::ptr_eq(&self.identity, &proof.owner) {
            return Err(Denied::Stale);
        }
        let live = s.live.as_ref().ok_or(Denied::Stale)?;
        if live.sequence != proof.sequence {
            return Err(Denied::Stale);
        }
        if live.phase != Phase::Retiring {
            return Err(Denied::Locked);
        }
        s.live = None;
        Ok(())
    }
    fn revoke(&self) -> Result<(), Denied> {
        let mut s = self.lock()?;
        Self::close(&mut s, Gate::Revoked);
        Ok(())
    }
}

#[cfg(test)]
#[path = "remote_unattended_tests.rs"]
mod tests;

#[cfg(feature = "remote-host")]
#[path = "remote_unattended/capability.rs"]
pub(crate) mod capability;
