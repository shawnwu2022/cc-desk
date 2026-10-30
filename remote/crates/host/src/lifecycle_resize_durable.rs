//! Private fake-only durable result owner. Never registers a lifecycle backend.
use crate::{
    journal_digest::DigestKey,
    lifecycle_journal::{Phase, Prepare, Store, Ticket},
    lifecycle_resize_completion::{Fact, Outcome},
    lifecycle_router::{Context, ResourceIdentity},
};
use cc_desk_remote_record::lifecycle::{self, Code, Request};
use cc_desk_remote_security::BoundDataAuthority;
#[cfg(test)]
use cc_desk_remote_security::Role;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Slot {
    active: bool,
    claimed: bool,
    published: bool,
    end: Instant,
    reservation: Option<Arc<()>>,
    proof: BoundDataAuthority,
    expected: Fact,
    result: Option<Fact>,
}
#[derive(Clone)]
pub(crate) struct Sender(Arc<Mutex<Slot>>);
impl Sender {
    /// Linearization point for one accepted effect. Cancellation after this claim
    /// cannot undo an in-flight effect, but prevents result publication.
    pub(crate) fn claim(
        &self,
        expected: &Fact,
        reservation: &Arc<()>,
        proof: &BoundDataAuthority,
        admit: impl FnOnce() -> Result<(), Code>,
    ) -> Result<(), Code> {
        let mut s = self.0.lock().map_err(|_| Code::Backend)?;
        if !s.active || Instant::now() >= s.end {
            return Err(Code::Stale);
        }
        if s.claimed || s.result.is_some() || &s.expected != expected {
            return Err(Code::Conflict);
        }
        if s.reservation
            .as_ref()
            .is_none_or(|id| !Arc::ptr_eq(id, reservation))
            || !s.proof.same_channel(proof)
        {
            return Err(Code::Conflict);
        }
        admit()?;
        s.claimed = true;
        Ok(())
    }
    pub(crate) fn complete(&self, fact: Fact) -> Result<(), Code> {
        fact.encode_stored()?;
        let mut s = self.0.lock().map_err(|_| Code::Backend)?;
        if !s.active {
            return Err(Code::Stale);
        }
        let mut expected = s.expected.clone();
        if fact.outcome == Outcome::NotApplied {
            expected.outcome = Outcome::NotApplied;
            expected.after = expected.request.state;
            expected.after_size = expected.before_size;
            expected.after_terminal = expected.before_terminal;
        }
        if fact != expected || s.result.as_ref().is_some_and(|old| old != &fact) {
            return Err(Code::Conflict);
        }
        s.result = Some(fact);
        Ok(())
    }
}
/// Only an owner that recorded and cached this exact fact can mint this ability.
/// It is consumed once and never serialised or reconstructed from history.
pub(crate) struct PublicationWitness {
    slot: Arc<Mutex<Slot>>,
    fact: Fact,
    proof: BoundDataAuthority,
}
impl PublicationWitness {
    pub(crate) fn publish(
        self,
        expected: &Fact,
        reservation: &Arc<()>,
        proof: &BoundDataAuthority,
        effect: impl FnOnce() -> Result<(), Code>,
    ) -> Result<(), Code> {
        let mut slot = self.slot.lock().map_err(|_| Code::Backend)?;
        self.proof.check().map_err(|_| Code::Unauthorized)?;
        if !slot.active || slot.published || Instant::now() >= slot.end {
            return Err(Code::Stale);
        }
        if &self.fact != expected || slot.result.as_ref() != Some(expected) {
            return Err(Code::Conflict);
        }
        if slot
            .reservation
            .as_ref()
            .is_none_or(|id| !Arc::ptr_eq(id, reservation))
            || !self.proof.same_channel(proof)
        {
            return Err(Code::Conflict);
        }
        effect()?;
        slot.active = false;
        slot.published = true;
        Ok(())
    }
}
struct Pending {
    ticket: Ticket,
    slot: Arc<Mutex<Slot>>,
    end: Instant,
    actor: bool,
}
pub(crate) struct Owner {
    store: Store,
    key: DigestKey,
    proof: BoundDataAuthority,
    backend: [u8; 32],
    resource: ResourceIdentity,
    current: Fact,
    pending: Option<Pending>,
    cache: Option<Fact>,
    published: bool,
    quarantined: bool,
    #[cfg(test)]
    cut: Option<Cut>,
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cut {
    AfterDurable,
    BeforePublish,
}
impl Owner {
    /// Test fixtures must supply their own explicit Resize permission and current fake state.
    #[cfg(test)]
    pub(crate) fn fake(
        mut store: Store,
        key: DigestKey,
        proof: BoundDataAuthority,
        current: Fact,
        permit: bool,
    ) -> Result<Self, Code> {
        if !permit
            || current.resource != ResourceIdentity::from_proof(&proof)
            || proof.local_role() != Role::Host
            || proof.data_role() != cc_desk_remote_record::grant::DataRole::Controller
        {
            return Err(Code::Unauthorized);
        }
        proof.check().map_err(|_| Code::Unauthorized)?;
        current.encode_stored()?;
        store
            .bind_key(key.lifecycle_key_id())
            .map_err(|_| Code::Backend)?;
        Ok(Self {
            store,
            key,
            resource: current.resource.clone(),
            backend: current.backend,
            proof,
            current,
            pending: None,
            cache: None,
            published: false,
            quarantined: false,
            cut: None,
        })
    }
    fn authority(&self) -> Result<(), Code> {
        self.proof.check().map_err(|_| Code::Unauthorized)
    }
    fn metadata(&self, r: &Request) -> Result<([u8; 32], [u8; 32]), Code> {
        self.authority()?;
        if self.resource != ResourceIdentity::from_proof(&self.proof) {
            return Err(Code::Unauthorized);
        }
        let scope = crate::lifecycle_durable::resource(
            &Context {
                proof: &self.proof,
                state: r.state,
            },
            &self.key,
        )?;
        let mut bytes = scope.to_vec();
        bytes.extend(self.proof.context().client_endpoint_id.as_bytes());
        bytes.extend(lifecycle::encode(r)?);
        Ok((
            scope,
            self.key
                .lifecycle_request(&bytes)
                .map_err(|_| Code::Malformed)?,
        ))
    }
    /// Historical lookup cannot update current state, release pending or publish a capability.
    pub(crate) fn history(&self, r: &Request) -> Result<Option<Fact>, Code> {
        let (scope, digest) = self.metadata(r)?;
        self.store
            .validate_results(scope, Some(self.backend), &self.key)
            .map_err(|_| Code::Backend)?;
        let f = self
            .store
            .restore_resize(scope, r, digest, self.backend, &self.resource, &self.key)
            .map_err(|_| Code::Conflict)?;
        self.authority()?;
        Ok(f)
    }
    pub(crate) fn begin(&mut self, r: Request, budget: Duration) -> Result<Option<Sender>, Code> {
        if budget.is_zero() || budget > Duration::from_secs(1) {
            return Err(Code::Malformed);
        }
        self.begin_inner(r, Instant::now() + budget, None)
    }
    pub(crate) fn actor_binding(
        &self,
        r: &Request,
    ) -> Result<(BoundDataAuthority, [u8; 32], [u8; 32]), Code> {
        let (_, digest) = self.metadata(r)?;
        Ok((self.proof.clone(), self.backend, digest))
    }
    pub(crate) fn begin_prepared(
        &mut self,
        expected: Fact,
        end: Instant,
        reservation: Arc<()>,
    ) -> Result<Option<Sender>, Code> {
        if end <= Instant::now()
            || end.saturating_duration_since(Instant::now()) > Duration::from_secs(1)
        {
            return Err(Code::Pending);
        }
        self.begin_inner(expected.request.clone(), end, Some((expected, reservation)))
    }
    fn begin_inner(
        &mut self,
        r: Request,
        end: Instant,
        expected: Option<(Fact, Arc<()>)>,
    ) -> Result<Option<Sender>, Code> {
        self.authority()?;
        if self.quarantined || self.pending.is_some() {
            return Err(Code::Pending);
        }
        if let Some(f) = self.history(&r)? {
            self.cache = Some(f);
            self.published = false;
            return Ok(None);
        }
        if !self.store.resize_writable() {
            return Err(Code::Unsupported);
        }
        if expected.is_none() && r.state != self.current.after {
            return Err(Code::Stale);
        }
        let (scope, digest) = self.metadata(&r)?;
        let actor = expected.is_some();
        let reservation = expected.as_ref().map(|(_, id)| id.clone());
        let f = if let Some((f, _)) = expected {
            if f.request != r
                || f.resource != self.resource
                || f.backend != self.backend
                || f.digest != digest
                || f.outcome != Outcome::Applied
            {
                return Err(Code::Conflict);
            }
            f
        } else {
            let mut f = self.current.clone();
            f.request = r;
            f.digest = digest;
            f.before_size = self.current.after_size;
            f.before_terminal = self.current.after_terminal;
            f.after_terminal = f.before_terminal;
            f.after = f.request.state;
            f.outcome = Outcome::Applied;
            f.after_size = f.request.size.ok_or(Code::Malformed)?;
            if f.after_size != f.before_size {
                f.after_terminal.dimension_epoch = f
                    .before_terminal
                    .dimension_epoch
                    .checked_add(1)
                    .ok_or(Code::Stale)?;
                f.after_terminal.structure_epoch = f
                    .before_terminal
                    .structure_epoch
                    .checked_add(1)
                    .ok_or(Code::Stale)?;
                f.after.dimension = f.after_terminal.dimension_epoch;
            }
            f
        };
        f.encode_stored()?;
        self.store
            .bind_key(self.key.lifecycle_key_id())
            .map_err(|_| Code::Backend)?;
        let t = match self
            .store
            .prepare(scope, f.request.operation, f.request.sequence, digest)
            .map_err(|_| Code::Conflict)?
        {
            Prepare::New(t) => t,
            Prepare::Recorded(_) => {
                self.quarantined = true;
                return Err(Code::Pending);
            }
        };
        self.store
            .transition(&t, Phase::InFlight)
            .map_err(|_| Code::Backend)?;
        self.store
            .transition(&t, Phase::Accepted)
            .map_err(|_| Code::Backend)?;
        let slot = Arc::new(Mutex::new(Slot {
            active: true,
            claimed: false,
            published: false,
            end,
            reservation,
            proof: self.proof.clone(),
            expected: f,
            result: None,
        }));
        self.pending = Some(Pending {
            ticket: t,
            slot: slot.clone(),
            end,
            actor,
        });
        self.cache = None;
        self.published = false;
        if self.authority().is_err() || Instant::now() >= end {
            self.cancel();
            return Err(Code::Unauthorized);
        }
        Ok(Some(Sender(slot)))
    }
    pub(crate) fn cancel(&mut self) {
        if self.published {
            return;
        }
        // Make the cancel/publish decision under the same slot lock. A successful
        // publication remains a historical fact even if its reply was not observed.
        let already_published = if let Some(p) = &self.pending
            && let Ok(mut s) = p.slot.lock()
        {
            if s.published {
                true
            } else {
                s.active = false;
                false
            }
        } else {
            false
        };
        if already_published {
            self.published = true;
            self.pending = None;
            return;
        }
        self.quarantined = true;
        self.published = false;
        if let Some(p) = &self.pending {
            let _ = self.store.transition(&p.ticket, Phase::Uncertain);
        }
    }
    pub(crate) fn finish(&mut self) -> Result<bool, Code> {
        self.finish_inner(false)
    }
    pub(crate) fn record_actor(&mut self) -> Result<Option<Fact>, Code> {
        if self.finish_inner(true)? {
            Ok(self.cache.clone())
        } else {
            Ok(None)
        }
    }
    pub(crate) fn actor_publication_check(&self, fact: &Fact) -> Result<(), Code> {
        self.authority()?;
        let p = self.pending.as_ref().ok_or(Code::Pending)?;
        if self.quarantined
            || !p.actor
            || Instant::now() >= p.end
            || self.cache.as_ref() != Some(fact)
        {
            return Err(Code::Pending);
        }
        Ok(())
    }
    pub(crate) fn confirm_actor_publication(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.slot.lock().is_ok_and(|s| s.published && !s.active))
        {
            self.published = true;
            self.pending = None;
        }
    }
    pub(crate) fn publication_witness(&self, fact: &Fact) -> Result<PublicationWitness, Code> {
        self.actor_publication_check(fact)?;
        let p = self.pending.as_ref().ok_or(Code::Pending)?;
        let s = p.slot.lock().map_err(|_| Code::Backend)?;
        if !s.active || s.result.as_ref() != Some(fact) {
            return Err(Code::Stale);
        }
        Ok(PublicationWitness {
            slot: p.slot.clone(),
            fact: fact.clone(),
            proof: self.proof.clone(),
        })
    }
    #[cfg(test)]
    pub(crate) fn fail_storage_for_test(&mut self) {
        self.store.fault = Some(crate::lifecycle_journal::Cut::BeforeTransition);
    }
    #[cfg(test)]
    pub(crate) fn cut_after_durable_for_test(&mut self) {
        self.cut = Some(Cut::AfterDurable);
    }
    fn finish_inner(&mut self, actor: bool) -> Result<bool, Code> {
        if self.quarantined {
            return Err(Code::Pending);
        }
        let p = self.pending.as_ref().ok_or(Code::Pending)?;
        if p.actor != actor {
            return Err(Code::Unsupported);
        }
        if Instant::now() >= p.end {
            self.cancel();
            return Err(Code::Pending);
        }
        let fact = p.slot.lock().map_err(|_| Code::Backend)?.result.clone();
        let Some(f) = fact else {
            return Ok(false);
        };
        // The private owner observes evidence within its absolute deadline; no SQL transaction spans a fake call.
        if self
            .store
            .record_resize(&p.ticket, self.backend, &self.resource, &f, &self.key)
            .is_err()
        {
            self.cancel();
            return Err(Code::Backend);
        }
        #[cfg(test)]
        if self.cut == Some(Cut::AfterDurable) {
            self.cancel();
            return Err(Code::Backend);
        }
        self.current = f.clone();
        self.cache = Some(f);
        #[cfg(test)]
        if self.cut == Some(Cut::BeforePublish) {
            self.cancel();
            return Err(Code::Backend);
        }
        if self.authority().is_err() || Instant::now() >= p.end {
            self.cancel();
            return Err(Code::Unauthorized);
        }
        if actor {
            return Ok(true);
        }
        p.slot.lock().map_err(|_| Code::Backend)?.active = false;
        self.published = true;
        self.pending = None;
        Ok(true)
    }
    pub(crate) fn result(&self) -> Result<Option<Fact>, Code> {
        self.authority()?;
        if self.quarantined || !self.published {
            return Err(Code::Pending);
        }
        Ok(self.cache.clone())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(p) = &self.pending
            && let Ok(mut s) = p.slot.lock()
        {
            s.active = false;
        }
    }
}
#[cfg(test)]
#[path = "lifecycle_resize_recovery_tests.rs"]
mod tests;
