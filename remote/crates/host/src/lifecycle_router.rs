//! Default-off nonblocking lifecycle dispatch. No native lifecycle adapter is
//! registered here; Accepted means queue admission, never process completion.
use cc_desk_remote_record::lifecycle::*;
use cc_desk_remote_security::{BoundDataAuthority, DataRecord, Role};
pub(crate) struct Context<'a> {
    pub proof: &'a BoundDataAuthority,
    pub state: State,
}
/// Supplied by the local Host resource owner, never decoded from the request.
pub(crate) struct Expected {
    pub context: cc_desk_remote_record::appbind::Context,
    pub principal: [u8; 32],
    pub scope: cc_desk_remote_record::grant::SessionScope,
    pub state: State,
}
/// Immutable queue identity supplied by its owner, never by a remote request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResourceIdentity {
    pub host: String,
    pub session: String,
    pub run: String,
    pub generation: String,
    pub incarnation: String,
    pub principal: [u8; 32],
}
impl ResourceIdentity {
    pub(crate) fn from_proof(p: &BoundDataAuthority) -> Self {
        Self {
            host: p.context().host_endpoint_id.clone(),
            session: p.scope().session.clone(),
            run: p.scope().run.clone(),
            generation: p.scope().generation.clone(),
            incarnation: p.context().incarnation.clone(),
            principal: p.principal_id(),
        }
    }
}
/// Local policy only. A Controller grant does not itself permit new lifecycle
/// operations. No Deserialize or allow-all constructor exists in product code.
#[derive(Default)]
pub(crate) struct Rights([bool; 5]);
impl Rights {
    fn index(k: Kind) -> usize {
        match k {
            Kind::Create => 0,
            Kind::Attach => 1,
            Kind::Resize => 2,
            Kind::Resume => 3,
            Kind::Close => 4,
        }
    }
    pub fn allow(&mut self, k: Kind) {
        self.0[Self::index(k)] = true;
    }
    fn permits(&self, k: Kind) -> bool {
        self.0[Self::index(k)]
    }
    #[cfg(test)]
    fn test_all() -> Self {
        Self([true; 5])
    }
}
pub(crate) trait Host {
    fn resource_identity(&self) -> Option<&ResourceIdentity> {
        None
    }
    fn create(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        Status::Rejected(Code::Unsupported)
    }
    fn attach(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        Status::Rejected(Code::Unsupported)
    }
    fn resize(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        Status::Rejected(Code::Unsupported)
    }
    fn resume(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        Status::Rejected(Code::Unsupported)
    }
    fn close(&mut self, _c: &Context<'_>, _r: &Request) -> Status {
        Status::Rejected(Code::Unsupported)
    }
}
pub(crate) struct Router<B> {
    proof: BoundDataAuthority,
    state: State,
    rights: Rights,
    backend: B,
    last: u64,
    cached: Option<Reply>,
    pending: bool,
}
impl<B: Host> Router<B> {
    #[cfg(test)]
    pub fn new(
        proof: BoundDataAuthority,
        expected: Expected,
        rights: Rights,
        backend: B,
    ) -> Result<Self, Code> {
        Self::validated(proof, expected, rights, backend)
    }
    fn validated(
        proof: BoundDataAuthority,
        expected: Expected,
        rights: Rights,
        backend: B,
    ) -> Result<Self, Code> {
        proof.check().map_err(|_| Code::Unauthorized)?;
        let state = expected.state;
        if proof.local_role() != Role::Host
            || proof.data_role() != cc_desk_remote_record::grant::DataRole::Controller
            || proof.principal_id() != expected.principal
            || proof.context() != &expected.context
            || proof.scope() != &expected.scope
            || proof.scope().generation != state.generation.to_string()
            || state.generation == 0
            || state.fence == 0
            || state.sync == 0
        {
            return Err(Code::Unauthorized);
        }
        Ok(Self {
            proof,
            state,
            rights,
            backend,
            last: 0,
            cached: None,
            pending: false,
        })
    }
    pub fn dispatch(&mut self, record: &DataRecord) -> Result<Reply, Code> {
        self.proof.check().map_err(|_| Code::Unauthorized)?;
        if !record.source().same_channel(&self.proof) || record.scope() != self.proof.scope() {
            return Err(Code::Unauthorized);
        }
        let r = decode(record.payload())?;
        let reject = |code| {
            Ok(Reply {
                request: r.clone(),
                status: Status::Rejected(code),
            })
        };
        if !self.rights.permits(r.kind) {
            return reject(Code::Unauthorized);
        }
        if record.fence() != self.state.fence {
            return reject(Code::Stale);
        }
        if let Some(old) = &self.cached
            && (old.request.operation == r.operation || old.request.sequence == r.sequence)
        {
            if old.request == r {
                return Ok(old.clone());
            }
            return reject(Code::Conflict);
        }
        if r.state != self.state || r.sequence <= self.last {
            return reject(Code::Stale);
        }
        if self.pending {
            return reject(Code::Pending);
        }
        self.last = r.sequence;
        // Reserve before trusted queue submission. If it unwinds after an effect,
        // exact duplicates observe Unknown and no distinct operation is admitted.
        self.pending = true;
        self.cached = Some(Reply {
            request: r.clone(),
            status: Status::Unknown,
        });
        let c = Context {
            proof: &self.proof,
            state: self.state,
        };
        let status = match r.kind {
            Kind::Create => self.backend.create(&c, &r),
            Kind::Attach => self.backend.attach(&c, &r),
            Kind::Resize => self.backend.resize(&c, &r),
            Kind::Resume => self.backend.resume(&c, &r),
            Kind::Close => self.backend.close(&c, &r),
        };
        self.pending = matches!(status, Status::Accepted | Status::Unknown);
        let reply = Reply { request: r, status };
        self.cached = Some(reply.clone());
        Ok(reply)
    }
}
impl<B: Host> Router<crate::lifecycle_durable::Durable<B>> {
    pub(crate) fn durable(
        proof: BoundDataAuthority,
        expected: Expected,
        rights: Rights,
        store: crate::lifecycle_journal::Store,
        key: crate::journal_digest::DigestKey,
        backend: B,
    ) -> Result<Self, Code> {
        Self::validated(
            proof,
            expected,
            rights,
            crate::lifecycle_durable::Durable::new(store, key, backend),
        )
    }
}
#[cfg(test)]
#[path = "lifecycle_router_tests.rs"]
mod tests;
