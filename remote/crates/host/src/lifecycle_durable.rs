//! Default-off lifecycle queue adapter. No real lifecycle backend is registered.
use crate::{
    journal::JournalError,
    journal_digest::DigestKey,
    lifecycle_journal::{Phase, Prepare, Store, Ticket},
    lifecycle_router::{Context, Host},
};
use cc_desk_remote_record::lifecycle::*;
pub(crate) struct Durable<B> {
    store: Store,
    key: DigestKey,
    backend: B,
    resource: Option<[u8; 32]>,
    pending: Option<Ticket>,
    instance: std::sync::Arc<()>,
    completion: Option<(crate::lifecycle_completion::Receiver, [u8; 32])>,
    completion_backend: Option<[u8; 32]>,
    #[cfg(test)]
    before_backend: Option<Box<dyn FnOnce() + Send>>,
}
fn code(c: Code) -> u8 {
    match c {
        Code::Malformed => 0,
        Code::Unauthorized => 1,
        Code::Stale => 2,
        Code::Conflict => 3,
        Code::Pending => 4,
        Code::Unsupported => 5,
        Code::Backend => 6,
    }
}
fn decode(c: u8) -> Code {
    match c {
        0 => Code::Malformed,
        1 => Code::Unauthorized,
        2 => Code::Stale,
        3 => Code::Conflict,
        4 => Code::Pending,
        5 => Code::Unsupported,
        _ => Code::Backend,
    }
}
fn resource(c: &Context<'_>, key: &DigestKey) -> Result<[u8; 32], Code> {
    let mut bytes = Vec::new();
    for s in [
        &c.proof.context().host_endpoint_id,
        &c.proof.scope().session,
        &c.proof.scope().run,
        &c.proof.scope().generation,
        &c.proof.context().incarnation,
    ] {
        bytes.extend_from_slice(&(s.len() as u32).to_be_bytes());
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&c.proof.principal_id());
    key.lifecycle_scope(&bytes).map_err(|_| Code::Malformed)
}
impl<B: Host> Durable<B> {
    #[cfg(test)]
    pub(crate) fn inspect_backend(&self) -> &B {
        &self.backend
    }
    #[cfg(test)]
    pub(crate) fn inspect_backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    pub(crate) fn new(store: Store, key: DigestKey, backend: B) -> Self {
        Self {
            store,
            key,
            backend,
            resource: None,
            pending: None,
            instance: std::sync::Arc::new(()),
            completion: None,
            completion_backend: None,
            #[cfg(test)]
            before_backend: None,
        }
    }
    fn apply(&mut self, c: &Context<'_>, r: &Request, kind: Kind) -> Status {
        if c.proof.check().is_err() {
            return Status::Rejected(Code::Unauthorized);
        }
        if r.kind != kind || r.state != c.state {
            return Status::Rejected(Code::Stale);
        }
        if kind == Kind::Attach
            && self
                .backend
                .completion_backend_id()
                .is_none_or(|id| id == [0; 32])
        {
            return Status::Rejected(Code::Unsupported);
        }
        if self.backend.resource_identity()
            != Some(&crate::lifecycle_router::ResourceIdentity::from_proof(
                c.proof,
            ))
        {
            return Status::Rejected(Code::Unauthorized);
        }
        if self.store.bind_key(self.key.lifecycle_key_id()).is_err() {
            return Status::Unknown;
        }
        let scope = match resource(c, &self.key) {
            Ok(s) => s,
            Err(e) => return Status::Rejected(e),
        };
        if self.resource.is_some_and(|old| old != scope) {
            return Status::Rejected(Code::Unauthorized);
        }
        self.resource = Some(scope);
        if self
            .store
            .validate_results(scope, self.backend.completion_backend_id(), &self.key)
            .is_err()
        {
            return Status::Unknown;
        }
        let mut bytes = scope.to_vec();
        bytes.extend_from_slice(c.proof.context().client_endpoint_id.as_bytes());
        match encode(r) {
            Ok(b) => bytes.extend(b),
            Err(e) => return Status::Rejected(e),
        }
        let digest = match self.key.lifecycle_request(&bytes) {
            Ok(d) => d,
            Err(_) => return Status::Rejected(Code::Malformed),
        };
        let ticket = match self.store.prepare(scope, r.operation, r.sequence, digest) {
            Ok(Prepare::New(t)) => t,
            Ok(Prepare::Recorded(p)) => {
                return match p {
                    Phase::Accepted | Phase::Completed => Status::Accepted,
                    Phase::Rejected(c) => Status::Rejected(decode(c)),
                    _ => Status::Unknown,
                };
            }
            Err(JournalError::Conflict) => return Status::Rejected(Code::Conflict),
            Err(JournalError::Sequence) => return Status::Rejected(Code::Pending),
            Err(_) => return Status::Unknown,
        };
        if self.store.transition(&ticket, Phase::InFlight).is_err() {
            return Status::Unknown;
        }
        self.pending = Some(ticket);
        self.completion = None;
        #[cfg(test)]
        if let Some(f) = self.before_backend.take() {
            f();
        }
        if c.proof.check().is_err() {
            if self
                .store
                .transition(
                    self.pending.as_ref().unwrap(),
                    Phase::Rejected(code(Code::Unauthorized)),
                )
                .is_err()
            {
                return Status::Unknown;
            }
            self.pending.take();
            return Status::Rejected(Code::Unauthorized);
        }
        // No SQL transaction is held during the trusted nonblocking queue call.
        let status = match kind {
            Kind::Create => self.backend.create(c, r),
            Kind::Attach => {
                let Ok((sender, receiver)) = crate::lifecycle_completion::channel(
                    r.clone(),
                    crate::lifecycle_router::ResourceIdentity::from_proof(c.proof),
                    digest,
                    self.instance.clone(),
                ) else {
                    return Status::Unknown;
                };
                self.completion = Some((receiver, digest));
                self.completion_backend = self.backend.completion_backend_id();
                self.backend.attach_with_completion(c, r, sender)
            }
            Kind::Resize => self.backend.resize(c, r),
            Kind::Resume => self.backend.resume(c, r),
            Kind::Close => self.backend.close(c, r),
        };
        let phase = match status {
            Status::Accepted => Phase::Accepted,
            Status::Unknown => Phase::Uncertain,
            Status::Rejected(c) => Phase::Rejected(code(c)),
        };
        if self
            .store
            .transition(self.pending.as_ref().unwrap(), phase)
            .is_err()
        {
            return Status::Unknown;
        }
        if matches!(status, Status::Rejected(_)) {
            self.pending.take();
        }
        status
    }
    #[cfg(test)]
    fn complete_fixture(&mut self) -> Result<(), JournalError> {
        self.store.transition(
            self.pending.as_ref().ok_or(JournalError::Conflict)?,
            Phase::Completed,
        )?;
        self.pending.take();
        Ok(())
    }
    pub(crate) fn complete_attach(
        &mut self,
    ) -> Result<Option<crate::lifecycle_completion::Applied>, Code> {
        if self.backend.poll_attach().is_err() {
            if let Some(ticket) = &self.pending {
                let _ = self.store.transition(ticket, Phase::Uncertain);
            }
            return Err(Code::Backend);
        }
        let Some((receiver, digest)) = &self.completion else {
            return Ok(None);
        };
        receiver.check_identity(self.backend.resource_identity())?;
        let Some(event) = receiver.evidence(*digest, &self.instance)? else {
            return Ok(None);
        };
        // Authenticated historical evidence may be recorded after revocation;
        // disclosing it is a separate live-authority check in the router.
        let backend = self.completion_backend.ok_or(Code::Conflict)?;
        if self.backend.completion_backend_id() != Some(backend) {
            return Err(Code::Conflict);
        }
        self.store
            .record_attach(
                self.pending.as_ref().ok_or(Code::Conflict)?,
                backend,
                &event,
                &self.key,
            )
            .map_err(|_| Code::Backend)?;
        Ok(Some(event))
    }
    #[cfg(test)]
    pub(crate) fn fail_completion_for_test(&mut self) {
        self.store.fault = Some(crate::lifecycle_journal::Cut::BeforeTransition);
    }
}
impl<B: Host> Host for Durable<B> {
    fn restored_attach(
        &mut self,
        c: &Context<'_>,
        r: &Request,
    ) -> Result<Option<crate::lifecycle_completion::Applied>, Code> {
        if !self.backend.permits_attach_recovery() {
            return Ok(None);
        }
        c.proof.check().map_err(|_| Code::Unauthorized)?;
        if self.backend.resource_identity()
            != Some(&crate::lifecycle_router::ResourceIdentity::from_proof(
                c.proof,
            ))
        {
            return Err(Code::Unauthorized);
        }
        let Some(backend) = self.backend.completion_backend_id() else {
            return Ok(None);
        };
        self.store
            .bind_key(self.key.lifecycle_key_id())
            .map_err(|_| Code::Backend)?;
        let scope = resource(c, &self.key)?;
        self.store
            .validate_results(scope, Some(backend), &self.key)
            .map_err(|_| Code::Conflict)?;
        let mut bytes = scope.to_vec();
        bytes.extend_from_slice(c.proof.context().client_endpoint_id.as_bytes());
        bytes.extend(encode(r)?);
        let digest = self
            .key
            .lifecycle_request(&bytes)
            .map_err(|_| Code::Malformed)?;
        self.store
            .restore_attach(scope, r.operation, r.sequence, digest, backend, &self.key)
            .map_err(|_| Code::Conflict)
    }
    fn create(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.apply(c, r, Kind::Create)
    }
    fn attach(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.apply(c, r, Kind::Attach)
    }
    fn resize(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.apply(c, r, Kind::Resize)
    }
    fn resume(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.apply(c, r, Kind::Resume)
    }
    fn close(&mut self, c: &Context<'_>, r: &Request) -> Status {
        self.apply(c, r, Kind::Close)
    }
}
#[cfg(all(test, windows))]
#[path = "lifecycle_durable_tests.rs"]
mod tests;
