//! Original NativeHost custody drives this coordinator independently of API waiters.
use super::*;
use cc_desk_remote_security::{DataError, HostInstallRequest};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use tokio::{task::JoinHandle, time::Instant};
fn finished<T>(task: &mut JoinHandle<T>) -> Option<Result<T, tokio::task::JoinError>> {
    if !task.is_finished() {
        return None;
    }
    let waker = Waker::noop();
    match Pin::new(task).poll(&mut Context::from_waker(waker)) {
        Poll::Ready(result) => Some(result),
        Poll::Pending => None,
    }
}

enum Stage {
    Request(JoinHandle<Result<HostInstallRequest, DataError>>),
    Native {
        attempt: InstallAttempt,
        request: Option<HostInstallRequest>,
        end: Instant,
    },
    Response {
        attempt: InstallAttempt,
        task: JoinHandle<Result<(), DataError>>,
    },
    Done(InstallAttempt),
    Failed,
}
pub(super) struct Coordinator {
    authorize: Option<Box<dyn FnOnce() -> bool + Send>>,
    native_attempt: Option<InstallAttempt>,
    target: Target,
    device: [u8; 32],
    expected: u64,
    lease: [u8; 16],
    channel: Arc<RunningChannel>,
    stage: Stage,
}
impl Coordinator {
    pub(super) fn reclaimable(&self) -> bool {
        matches!(self.stage, Stage::Done(_) | Stage::Failed) && self.authorize.is_none()
    }
    pub(super) fn completed_attempt(&self, id: InstallAttempt) -> bool {
        matches!(self.stage,Stage::Done(done) if done==id)
    }
    pub(super) fn channel(&self) -> Arc<RunningChannel> {
        self.channel.clone()
    }
    pub(super) fn pending(&self) -> bool {
        matches!(
            self.stage,
            Stage::Request(_) | Stage::Native { .. } | Stage::Response { .. }
        )
    }
}
impl NativeHost {
    /// Current native admission selects the same Host-owned resource internally.
    pub fn begin_native_resource_rebind(
        &mut self,
        channel: Arc<RunningChannel>,
        authorize: Box<dyn FnOnce() -> bool + Send>,
    ) -> Result<(), NativeHostError> {
        if self.closing || self.unknown {
            return Err(NativeHostError::Closed);
        }
        self.drive_peer_installs();
        if self.unknown {
            return Err(NativeHostError::Unknown);
        }
        let authority = channel.authority();
        authority.check().map_err(|_| NativeHostError::Authority)?;
        let scope = authority.scope();
        let mut matches = self.owner.active.iter().filter(|writer| {
            writer.target.run == scope.run
                && writer.target.generation.to_string() == scope.generation
                && writer
                    .target
                    .session
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
                    == scope.session
        });
        let target = matches
            .next()
            .ok_or(NativeHostError::Authority)?
            .target
            .clone();
        if matches.next().is_some()
            || self
                .peer_installs
                .iter()
                .any(|job| job.target == target && job.pending())
        {
            return Err(NativeHostError::Rejected);
        }
        let latest = self
            .attempts
            .iter()
            .rfind(|attempt| attempt.authority.scope() == scope)
            .ok_or(NativeHostError::Authority)?;
        let InstallState::Installed {
            fence: expected, ..
        } = latest.state
        else {
            return Err(NativeHostError::Unknown);
        };
        let text = authority.context().client_endpoint_id.as_bytes();
        if text.len() != 64 {
            return Err(NativeHostError::Authority);
        }
        let mut device = [0; 32];
        for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
            device[i] = u8::from_str_radix(
                std::str::from_utf8(pair).map_err(|_| NativeHostError::Authority)?,
                16,
            )
            .map_err(|_| NativeHostError::Authority)?;
        }
        let mut lease = [0; 16];
        use ring::rand::SecureRandom;
        ring::rand::SystemRandom::new()
            .fill(&mut lease)
            .map_err(|_| NativeHostError::Rejected)?;
        self.begin_peer_rebind(target, device, expected, channel, lease)?;
        self.peer_installs
            .last_mut()
            .expect("registered original coordinator")
            .authorize = Some(authorize);
        Ok(())
    }
    pub(super) fn register_native_attempt(
        &mut self,
        target: Target,
        device: [u8; 32],
        lease: [u8; 16],
        channel: Arc<RunningChannel>,
        attempt: InstallAttempt,
    ) {
        let owned = channel.clone();
        let task = tokio::spawn(async move { owned.host_install_request().await });
        self.peer_installs.push(Coordinator {
            authorize: None,
            native_attempt: Some(attempt),
            target,
            device,
            expected: 0,
            lease,
            channel,
            stage: Stage::Request(task),
        });
    }
    /// Locally selected target/expected/lease only. No peer-supplied success/fence.
    /// Caller cancellation cannot drop the request task or original native pending.
    pub fn begin_peer_rebind(
        &mut self,
        target: Target,
        device: [u8; 32],
        expected: u64,
        channel: Arc<RunningChannel>,
        lease: [u8; 16],
    ) -> Result<(), NativeHostError> {
        if self.closing {
            return Err(NativeHostError::Closed);
        }
        self.prune_retired_history();
        if self.unknown {
            return Err(NativeHostError::Unknown);
        }
        if self.peer_installs.len() >= MAX_ATTEMPTS {
            return Err(NativeHostError::Capacity);
        }
        if !channel.peer_install_enabled() || channel.authority().local_role() != Role::Host {
            return Err(NativeHostError::Authority);
        }
        channel
            .authority()
            .check()
            .map_err(|_| NativeHostError::Authority)?;
        let owned = channel.clone();
        let task = tokio::spawn(async move { owned.host_install_request().await });
        self.peer_installs.push(Coordinator {
            authorize: None,
            native_attempt: None,
            target,
            device,
            expected,
            lease,
            channel,
            stage: Stage::Request(task),
        });
        Ok(())
    }
    /// Application service owner calls this even when request waiters have left.
    /// Never waits for unfinished IO or actor work and never holds a database lock.
    pub fn drive_peer_installs(&mut self) {
        let mut jobs = std::mem::take(&mut self.peer_installs);
        for job in &mut jobs {
            let stage = std::mem::replace(&mut job.stage, Stage::Failed);
            job.stage = match stage {
                Stage::Request(mut task) if task.is_finished() => match finished(&mut task) {
                    None => Stage::Request(task),
                    Some(Ok(Ok(request)))
                        if !self.closing && job.channel.authority().check().is_ok() =>
                    {
                        let native = if let Some(attempt) = job.native_attempt {
                            Ok(attempt)
                        } else {
                            self.begin_rebind_checked(
                                &job.target,
                                job.device,
                                job.expected,
                                job.channel.clone(),
                                job.lease,
                                job.authorize.take(),
                            )
                        };
                        match native {
                            Ok(attempt) => Stage::Native {
                                attempt,
                                request: Some(request),
                                end: job
                                    .channel
                                    .peer_install_deadline()
                                    .unwrap()
                                    .min(Instant::now() + Duration::from_secs(1)),
                            },
                            Err(_) => {
                                job.channel.authority().request_close();
                                Stage::Failed
                            }
                        }
                    }
                    result => {
                        if matches!(result, Some(Err(_))) {
                            self.unknown = true;
                        }
                        // The original request settled without handing this capability
                        // to the actor. Keep the result/custody, release unused captures.
                        job.authorize = None;
                        job.channel.authority().request_close();
                        Stage::Failed
                    }
                },
                Stage::Native {
                    attempt,
                    mut request,
                    end,
                } => {
                    let observation = self.observe_install(attempt);
                    if self.closing
                        || Instant::now() >= end
                        || job.channel.authority().check().is_err()
                    {
                        // Existing attempts retains the original Pending and any late fact.
                        job.channel.authority().request_close();
                        Stage::Failed
                    } else {
                        match observation {
                            InstallObservation::Installed { fence } => {
                                let channel = job.channel.clone();
                                let request = request.take().unwrap();
                                let task = tokio::spawn(async move {
                                    channel.complete_host_install(request, fence).await
                                });
                                Stage::Response { attempt, task }
                            }
                            InstallObservation::Pending => Stage::Native {
                                attempt,
                                request,
                                end,
                            },
                            _ => {
                                job.channel.authority().request_close();
                                Stage::Failed
                            }
                        }
                    }
                }
                Stage::Response { attempt, mut task } if task.is_finished() => {
                    match finished(&mut task) {
                        None => Stage::Response { attempt, task },
                        Some(Ok(Ok(()))) if job.channel.installed_fence().is_ok() => {
                            Stage::Done(attempt)
                        }
                        Some(result) => {
                            if result.is_err() {
                                self.unknown = true;
                            }
                            job.channel.authority().request_close();
                            Stage::Failed
                        }
                    }
                }
                other => other,
            };
        }
        self.peer_installs = jobs;
    }
    /// Existing Connection::dispatch_wire remains the sole dispatcher.
    pub fn peer_connection(
        &mut self,
        channel: &RunningChannel,
    ) -> Result<NativeConnection, NativeHostError> {
        let authority = channel.authority();
        authority.check().map_err(|_| NativeHostError::Authority)?;
        let id = self
            .peer_installs
            .iter()
            .find_map(|job| {
                let original = job.channel.authority();
                if !original.same_channel(&authority)
                    || original.grant_id() != authority.grant_id()
                    || original.scope() != authority.scope()
                {
                    return None;
                }
                match job.stage {
                    Stage::Done(id) => Some(id),
                    _ => None,
                }
            })
            .ok_or(NativeHostError::Rejected)?;
        self.connection(id)
    }
    // 只观察原 channel 的终结事实，不使用已撤销的披露授权，也不丢弃原句柄。
    pub(super) fn retire_channel_tasks(
        &mut self,
        authority: &cc_desk_remote_security::BoundDataAuthority,
    ) -> bool {
        let mut pending = false;
        for job in &mut self.peer_installs {
            let original = job.channel.authority();
            if !original.same_channel(authority) || original.grant_id() != authority.grant_id() {
                continue;
            }
            let stage = std::mem::replace(&mut job.stage, Stage::Failed);
            job.stage = match stage {
                Stage::Request(mut task) => match finished(&mut task) {
                    None => {
                        pending = true;
                        Stage::Request(task)
                    }
                    Some(result) => {
                        self.unknown |= result.is_err();
                        job.authorize = None;
                        Stage::Failed
                    }
                },
                Stage::Response { attempt, mut task } => match finished(&mut task) {
                    None => {
                        pending = true;
                        Stage::Response { attempt, task }
                    }
                    Some(result) => {
                        self.unknown |= result.is_err();
                        job.authorize = None;
                        Stage::Failed
                    }
                },
                Stage::Native { .. } => {
                    // 原 bind receiver 仍在 attempts，必须另行收齐其终局。
                    job.authorize = None;
                    Stage::Failed
                }
                other => other,
            };
        }
        pending
    }
    /// Retains/joins every response/request task, including cancelled installations.
    /// Shutdown must first signal authorities, then call this alongside network retirement.
    pub async fn retire_peer_installs(&mut self) {
        for job in &self.peer_installs {
            job.channel.authority().request_close();
        }
        for job in &mut self.peer_installs {
            // Await through a reference: cancellation retains the original handle in Host.
            match &mut job.stage {
                Stage::Request(task) => {
                    self.unknown |= task.await.is_err();
                }
                Stage::Response { task, .. } => {
                    self.unknown |= task.await.is_err();
                }
                _ => (),
            }
            job.authorize = None;
            job.stage = Stage::Failed;
        }
        // Native pending replies remain in attempts until observe_shutdown reaps them.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn legacy_channel_cannot_enter_peer_coordinator_or_seal_writer() {
        use crate::bound_test_support as support;
        use std::sync::atomic::AtomicU64;
        let (c, h, _gate, _cancel, _grant) =
            support::channels(Arc::new(AtomicU64::new(1000))).await;
        let h = Arc::new(h);
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some([4; 32]);
        let mut host = NativeHost::from_owner(owner);
        let target = Target {
            host: support::host(),
            incarnation: 11,
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
        };
        assert_eq!(
            host.begin_peer_rebind(target, support::device(), 4, h.clone(), [9; 16]),
            Err(NativeHostError::Authority)
        );
        assert!(host.peer_installs.is_empty());
        assert!(host.attempts.is_empty());
        assert!(!host.closing);
        c.authority().request_close();
        h.authority().request_close();
        host.retire_peer_installs().await;
        let _ = tokio::join!(c.wait_closed(), h.wait_closed());
    }
}

#[cfg(test)]
#[path = "local_writer_peer_coordinator_tests.rs"]
mod peer_tests;
