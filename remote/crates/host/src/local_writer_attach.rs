//! Existing-writer Attach only. No listener, launch, enrollment or PTY custody.
use super::*;
use crate::lifecycle_completion::Sender;
use crate::lifecycle_router::{Context, Host, ResourceIdentity};
use authorized::{AuthorizedPeer, AuthorizedPendingRebind};
use cc_desk_remote_record::lifecycle::{Code, Request as LifecycleRequest, State, Status};
use std::time::Instant;
#[path = "local_writer_attach_connection.rs"]
mod connection;
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}
struct Pending {
    native: Option<AuthorizedPendingRebind>,
    projection: Option<PendingProjection>,
    request: LifecycleRequest,
    sender: Sender,
    end: Instant,
}
pub(super) struct AttachBackend {
    owner: Arc<Mutex<WriterHostOwner>>,
    target: Target,
    device: [u8; 32],
    lease: [u8; 16],
    identity: ResourceIdentity,
    id: [u8; 32],
    baseline: State,
    budget: Duration,
    pending: Option<Pending>,
    peer: Option<AuthorizedPeer>,
    failed: bool,
    verified: Option<crate::lifecycle_completion::Applied>,
    authority: Option<cc_desk_remote_security::BoundDataAuthority>,
    ready: bool,
    connection_taken: bool,
    #[cfg(test)]
    expire_before_publish: bool,
}
impl AttachBackend {
    pub(super) fn connection(&mut self) -> Result<connection::Connection<'_>, Reject> {
        if !self.ready || self.failed || self.connection_taken {
            return Err(Reject::Pending);
        }
        let c = connection::Connection::new(
            self.peer.as_ref().ok_or(Reject::Closed)?,
            self.authority.as_ref().ok_or(Reject::Policy)?.clone(),
            self.verified.as_ref().ok_or(Reject::Pending)?.after.fence,
        )?;
        self.connection_taken = true;
        Ok(c)
    }
    pub(super) fn new(
        owner: Arc<Mutex<WriterHostOwner>>,
        target: Target,
        device: [u8; 32],
        lease: [u8; 16],
        baseline: State,
        id: [u8; 32],
        budget: Duration,
    ) -> Result<Self, Code> {
        let guard = owner.try_lock().map_err(|_| Code::Pending)?;
        guard.find(&target).map_err(|_| Code::Unauthorized)?;
        if target.host != guard.host
            || target.incarnation != guard.incarnation
            || baseline.generation != target.generation
            || baseline.fence == 0
            || baseline.sync == 0
            || lease == [0; 16]
            || id == [0; 32]
            || budget.is_zero()
            || budget > Duration::from_secs(1)
        {
            return Err(Code::Unauthorized);
        }
        let identity = ResourceIdentity {
            host: hex(&target.host),
            session: hex(&target.session),
            run: target.run.clone(),
            generation: target.generation.to_string(),
            incarnation: target.incarnation.to_string(),
            principal: guard.principal.ok_or(Code::Unauthorized)?,
        };
        drop(guard);
        Ok(Self {
            owner,
            target,
            device,
            lease,
            identity,
            id,
            baseline,
            budget,
            pending: None,
            peer: None,
            failed: false,
            verified: None,
            authority: None,
            ready: false,
            connection_taken: false,
            #[cfg(test)]
            expire_before_publish: false,
        })
    }
}
impl Host for AttachBackend {
    fn publish_attach(&mut self, event: &crate::lifecycle_completion::Applied) -> Result<(), Code> {
        if self.failed || self.peer.is_none() || self.verified.as_ref() != Some(event) {
            return Err(Code::Conflict);
        }
        self.ready = true;
        Ok(())
    }
    fn permits_attach_recovery(&self) -> bool {
        false
    } // Native ownership recovery is not admitted.
    fn resource_identity(&self) -> Option<&ResourceIdentity> {
        Some(&self.identity)
    }
    fn completion_backend_id(&self) -> Option<[u8; 32]> {
        Some(self.id)
    }
    fn attach_with_completion(
        &mut self,
        c: &Context<'_>,
        r: &LifecycleRequest,
        sender: Sender,
    ) -> Status {
        if self.failed || self.pending.is_some() || self.peer.is_some() {
            return Status::Rejected(Code::Pending);
        }
        if c.proof.check().is_err()
            || ResourceIdentity::from_proof(c.proof) != self.identity
            || r.state != self.baseline
            || r.kind != cc_desk_remote_record::lifecycle::Kind::Attach
        {
            return Status::Rejected(Code::Unauthorized);
        }
        let end = Instant::now() + self.budget;
        let Ok(mut owner) = self.owner.try_lock() else {
            return Status::Rejected(Code::Pending);
        };
        let native = match owner.rebind_authorized(
            &self.target,
            self.device,
            r.state.fence,
            c.proof.clone(),
            self.lease,
        ) {
            Ok(p) => p,
            Err(_) => return Status::Rejected(Code::Backend),
        };
        drop(owner);
        self.authority = Some(c.proof.clone());
        self.pending = Some(Pending {
            native: Some(native),
            projection: None,
            request: r.clone(),
            sender,
            end,
        });
        Status::Accepted
    }
    fn poll_attach(&mut self) -> Result<(), Code> {
        if self.failed {
            return Err(Code::Backend);
        }
        let Some(job) = &mut self.pending else {
            return Ok(());
        };
        if Instant::now() >= job.end {
            self.failed = true;
            return Err(Code::Backend);
        }
        if let Some(native) = &job.native {
            match native.wait(Duration::ZERO) {
                Err(WaitError::Uncertain) => return Ok(()),
                Err(WaitError::Closed) | Ok(Err(_)) => {
                    self.failed = true;
                    return Err(Code::Backend);
                }
                Ok(Ok(peer)) => {
                    self.peer = Some(peer);
                    job.native = None;
                    job.projection = match self.peer.as_ref().unwrap().capture() {
                        Ok(p) => Some(p),
                        Err(_) => {
                            self.failed = true;
                            return Err(Code::Backend);
                        }
                    };
                }
            }
        }
        let frame = match job
            .projection
            .as_ref()
            .ok_or(Code::Backend)?
            .wait(Duration::ZERO)
        {
            Err(WaitError::Uncertain) => return Ok(()),
            Ok(Ok(f)) => f,
            _ => {
                self.failed = true;
                return Err(Code::Backend);
            }
        };
        let stamp = frame.issued_stamp().map_err(|_| Code::Backend)?;
        let after = State {
            generation: stamp.target.generation.parse().map_err(|_| Code::Backend)?,
            fence: job.request.state.fence.checked_add(1).ok_or(Code::Stale)?,
            mode: stamp.mode_epoch.parse().map_err(|_| Code::Backend)?,
            dimension: stamp.dimension_epoch.parse().map_err(|_| Code::Backend)?,
            sync: stamp.sync_epoch.parse().map_err(|_| Code::Backend)?,
        };
        // Observed native epochs, never delivered or ACKed. Fence is the exact
        // expected+1 guaranteed by rebind_authorized and writer replacement.
        #[cfg(test)]
        if self.expire_before_publish {
            job.end = Instant::now();
        }
        if Instant::now() >= job.end {
            self.failed = true;
            return Err(Code::Backend);
        }
        let result = job.sender.complete(&job.request, &self.identity, after);
        if result.is_ok() {
            self.verified = Some(crate::lifecycle_completion::Applied {
                request: job.request.clone(),
                after,
            });
        }
        self.pending = None;
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
#[cfg(all(test, windows))]
#[path = "local_writer_attach_tests.rs"]
mod tests;
