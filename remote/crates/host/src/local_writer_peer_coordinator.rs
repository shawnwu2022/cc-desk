//! Original NativeHost custody drives this coordinator independently of API waiters.
use super::*;
use cc_desk_remote_security::{DataError, HostInstallRequest};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Wake, Waker},
};
use tokio::{task::JoinHandle, time::Instant};
struct NoWake;
impl Wake for NoWake {
    fn wake(self: Arc<Self>) {}
}
fn finished<T>(task: &mut JoinHandle<T>) -> Option<Result<T, tokio::task::JoinError>> {
    if !task.is_finished() {
        return None;
    }
    let waker = Waker::from(Arc::new(NoWake));
    match Pin::new(task).poll(&mut Context::from_waker(&waker)) {
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
    native_attempt: Option<InstallAttempt>,
    target: Target,
    device: [u8; 32],
    expected: u64,
    lease: [u8; 16],
    channel: Arc<RunningChannel>,
    stage: Stage,
}
impl Coordinator {
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
                            self.begin_rebind(
                                &job.target,
                                job.device,
                                job.expected,
                                job.channel.clone(),
                                job.lease,
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
                    _ => {
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
