//! Same-writer durable publication candidate; no native or network route.
use super::*;
use crate::lifecycle_resize_completion::{Fact, Outcome};
use cc_desk_remote_security::BoundDataAuthority;
use std::sync::atomic::AtomicU8;
const RESERVED: u8 = 0;
const EXECUTING: u8 = 1;
const EFFECT: u8 = 2;
const READY: u8 = 3;
const PUBLISHED: u8 = 4;
const CANCELLED: u8 = 5;
pub(in crate::local_writer) struct Prepare {
    pub request: Operation,
    pub proof: BoundDataAuthority,
    pub backend: [u8; 32],
    pub digest: [u8; 32],
    pub end: Instant,
}
pub(in crate::local_writer) struct Reservation {
    #[cfg(test)]
    before_claim: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    before_native_admit: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    cancel_reentry: std::sync::Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    phase: AtomicU8,
    end: Instant,
    sealed: Arc<AtomicBool>,
    scope: Arc<view_gate::PeerScope>,
    proof: BoundDataAuthority,
    identity: Arc<()>,
    expected: Fact,
    result: std::sync::Mutex<Option<Fact>>,
    input: LocalInputHandle,
    _credit: Arc<Credit>,
    index: usize,
}
impl Reservation {
    /// Force the actual native claim across its deadline while holding the
    /// publication guard. The exact-reservation probe detects recursive cancel
    /// and uses its lock-free form so a RED cannot leave an unjoinable thread.
    #[cfg(test)]
    pub(in crate::local_writer) fn test_cross_native_guard_deadline(&self) -> Arc<AtomicBool> {
        let reentered = Arc::new(AtomicBool::new(false));
        let owner = Arc::new(std::sync::Mutex::new(None));
        let entered = owner.clone();
        let end = self.end;
        *self.before_native_admit.lock().unwrap() = Some(Box::new(move || {
            *entered.lock().unwrap() = Some(std::thread::current().id());
            std::thread::sleep(
                end.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
        }));
        let observed = reentered.clone();
        *self.cancel_reentry.lock().unwrap() = Some(Arc::new(move || {
            let same = owner
                .lock()
                .unwrap()
                .is_some_and(|id| id == std::thread::current().id());
            if same {
                observed.store(true, Ordering::Release);
            }
            same
        }));
        reentered
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn test_before_claim(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_claim.lock().unwrap() = Some(Box::new(hook));
    }
    pub(in crate::local_writer) fn cancel(&self) {
        #[cfg(test)]
        if self
            .cancel_reentry
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|probe| probe())
        {
            self.cancel_locked();
            return;
        }
        if self
            .scope
            .with_publication(|| {
                self.cancel_locked();
                Ok(())
            })
            .is_err()
        {
            // Closed/poisoned gates can never admit another publication.
            self.cancel_locked();
        }
    }
    fn cancel_locked(&self) {
        let _ = self
            .phase
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |p| {
                (p != PUBLISHED && p != CANCELLED).then_some(CANCELLED)
            });
        if self.phase.load(Ordering::Acquire) == CANCELLED {
            self.sealed.store(true, Ordering::Release);
        }
    }
    pub(in crate::local_writer) fn expired(&self) -> bool {
        if self.phase.load(Ordering::Acquire) == PUBLISHED {
            return false;
        }
        if Instant::now() >= self.end {
            self.cancel();
        }
        self.phase.load(Ordering::Acquire) == CANCELLED
    }
    pub(in crate::local_writer) fn ready(&self) -> Result<(), Reject> {
        if !self.scope.is_active() {
            return Err(Reject::Closed);
        }
        if self.expired() {
            return Err(Reject::Sealed);
        }
        if self.phase.load(Ordering::Acquire) == PUBLISHED {
            Ok(())
        } else {
            Err(Reject::Pending)
        }
    }
    fn alive(&self) -> Result<(), Reject> {
        if self.expired() || self.sealed.load(Ordering::Acquire) {
            Err(Reject::Sealed)
        } else {
            Ok(())
        }
    }
    fn publish(
        &self,
        witness: crate::lifecycle_resize_durable::PublicationWitness,
    ) -> Result<(), Reject> {
        witness
            .publish(
                self.result
                    .lock()
                    .map_err(|_| Reject::Thread)?
                    .as_ref()
                    .ok_or(Reject::Pending)?,
                &self.identity,
                &self.proof,
                || {
                    self.scope
                        .with_publication(|| {
                            self.proof.check().map_err(|_| Reject::Policy)?;
                            // This guarded decision is the publication linearization point.
                            if self.sealed.load(Ordering::Acquire) || Instant::now() >= self.end {
                                self.cancel_locked();
                                return Err(Reject::Sealed);
                            }
                            self.phase
                                .compare_exchange(
                                    READY,
                                    PUBLISHED,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .map_err(|_| Reject::Pending)?;
                            Ok(())
                        })
                        .map_err(|_| cc_desk_remote_record::lifecycle::Code::Stale)
                },
            )
            .map_err(|_| Reject::Policy)
    }
}
#[derive(Clone)]
pub(in crate::local_writer) struct Prepared {
    pub fact: Fact,
    cap: Arc<Reservation>,
}
impl Prepared {
    pub(in crate::local_writer) fn cancel(&self) {
        self.cap.cancel();
    }
}
enum Action {
    Prepare {
        input: Prepare,
        scope: Arc<view_gate::PeerScope>,
        local: LocalInputHandle,
        credit: Arc<Credit>,
        reply: mpsc::SyncSender<Result<Prepared, Reject>>,
    },
    Execute {
        prepared: Prepared,
        sender: crate::lifecycle_resize_durable::Sender,
        reply: mpsc::SyncSender<Result<Fact, Reject>>,
    },
    Ready {
        prepared: Prepared,
        fact: Fact,
        reply: mpsc::SyncSender<Result<(), Reject>>,
    },
    Publish {
        prepared: Prepared,
        witness: crate::lifecycle_resize_durable::PublicationWitness,
        reply: mpsc::SyncSender<Result<(), Reject>>,
    },
}
pub(in crate::local_writer) struct Command(Action);
/// Retained by the original canonical actor while its sole IO worker owns the
/// physical effect. The original sender/result is never replaced by a timeout.
pub(in crate::local_writer) struct NativeExecution {
    prepared: Prepared,
    sender: crate::lifecycle_resize_durable::Sender,
    reply: mpsc::SyncSender<Result<Fact, Reject>>,
}
impl NativeExecution {
    pub(in crate::local_writer) fn work(&self) -> owner::native_host::producer::NativeWork {
        let p = self.prepared.clone();
        let sender = self.sender.clone();
        Box::new(move |writer| {
            let fact = p.fact.clone();
            let cap = p.cap.clone();
            let same_size = fact.before_size == fact.after_size;
            let authorize = Box::new(move || {
                sender
                    .claim(&fact, &cap.identity, &cap.proof, || {
                        cap.scope
                            .with_publication(|| {
                                #[cfg(test)]
                                if let Some(hook) = cap.before_native_admit.lock().unwrap().take() {
                                    hook();
                                }
                                // This scope's publication guard is already held.
                                // alive/expired/cancel would reacquire that SAME
                                // non-reentrant gate when the deadline crosses.
                                if cap.sealed.load(Ordering::Acquire)
                                    || Instant::now() >= cap.end
                                    || cap.phase.load(Ordering::Acquire) != EXECUTING
                                {
                                    cap.cancel_locked();
                                    return Err(Reject::Sealed);
                                }
                                cap.proof.check().map_err(|_| Reject::Policy)
                            })
                            .map_err(|_| cc_desk_remote_record::lifecycle::Code::Stale)
                    })
                    .is_ok()
            });
            if same_size {
                if !authorize() {
                    return Err(Reject::Policy);
                }
            } else {
                let actual = writer.resize(p.fact.after_size.0, p.fact.after_size.1, authorize)?;
                if actual != p.fact.after_size {
                    return Err(Reject::Continuity);
                }
            }
            Ok(owner::native_host::NativeFrameResult {
                complete: true,
                confirmed_bytes: 0,
            })
        })
    }
    pub(in crate::local_writer) fn complete(
        self,
        result: Result<owner::native_host::NativeFrameResult, Reject>,
        state: Option<&mut State>,
        replies: Option<&mut ReplyState>,
        sealed: &AtomicBool,
    ) {
        let p = &self.prepared;
        let result = (|| {
            let state = state.ok_or(Reject::Unsupported)?;
            let replies = replies.ok_or(Reject::Unsupported)?;
            let mut fact = p.fact.clone();
            match result {
                Ok(r) if r.complete => {
                    replies.resize_commit(fact.after_size.0, fact.after_size.1)?
                }
                Err(Reject::Policy) => {
                    fact.outcome = Outcome::NotApplied;
                    fact.after = fact.request.state;
                    fact.after_size = fact.before_size;
                    fact.after_terminal = fact.before_terminal;
                }
                _ => return Err(Reject::Continuity),
            }
            let (actual, epochs) = replies.resize_baseline()?;
            if actual != fact.after_size || epochs != fact.after_terminal {
                return Err(Reject::Continuity);
            }
            fact.encode_stored().map_err(|_| Reject::Bounds)?;
            self.sender
                .complete(fact.clone())
                .map_err(|_| Reject::Continuity)?;
            *p.cap.result.lock().map_err(|_| Reject::Thread)? = Some(fact.clone());
            state.records[p.cap.index].2 = if fact.outcome == Outcome::Applied {
                Ok(Applied {
                    request: fact.request.clone(),
                    after: fact.after,
                    size: Size {
                        rows: actual.0,
                        columns: actual.1,
                    },
                })
            } else {
                Err(Error::NotApplied)
            };
            p.cap.alive()?;
            p.cap
                .phase
                .compare_exchange(EXECUTING, EFFECT, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| Reject::Sealed)?;
            Ok(fact)
        })();
        if result.is_err() {
            p.cancel();
            sealed.store(true, Ordering::Release);
        }
        if self.reply.try_send(result).is_err() {
            p.cancel();
        }
    }
}
pub(in crate::local_writer) struct Pending<T> {
    reply: mpsc::Receiver<Result<T, Reject>>,
    end: Instant,
    sealed: Arc<AtomicBool>,
    cap: Option<Arc<Reservation>>,
    observed: bool,
    _credit: Arc<Credit>,
}
impl<T> Pending<T> {
    pub(in crate::local_writer) fn wait(mut self) -> Result<Result<T, Reject>, WaitError> {
        let result = self
            .reply
            .recv_timeout(self.end.saturating_duration_since(Instant::now()));
        if result.is_err() || Instant::now() >= self.end {
            return Err(WaitError::Uncertain);
        }
        self.observed = true;
        result.map_err(|_| WaitError::Uncertain)
    }
}
impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        if !self.observed {
            if let Some(cap) = &self.cap {
                cap.cancel();
            } else {
                self.sealed.store(true, Ordering::Release);
            }
        }
    }
}
pub(in crate::local_writer) fn prepare(
    input: &LocalInputHandle,
    scope: Arc<view_gate::PeerScope>,
    request: Prepare,
) -> Result<Pending<Prepared>, Reject> {
    let end = request.end;
    let credit = input.credits.reserve()?;
    let (tx, rx) = mpsc::sync_channel(1);
    input
        .tx
        .try_send(super::super::Command::DurableResize(Box::new(Command(
            Action::Prepare {
                input: request,
                scope,
                local: input.clone(),
                credit: credit.clone(),
                reply: tx,
            },
        ))))
        .map_err(|_| Reject::Full)?;
    Ok(Pending {
        reply: rx,
        end,
        sealed: input.sealed.clone(),
        cap: None,
        observed: false,
        _credit: credit,
    })
}
fn enqueue<T>(
    p: &Prepared,
    make: impl FnOnce(mpsc::SyncSender<Result<T, Reject>>) -> Action,
) -> Result<Pending<T>, Reject> {
    p.cap.alive()?;
    let (tx, rx) = mpsc::sync_channel(1);
    p.cap
        .input
        .tx
        .try_send(super::super::Command::DurableResize(Box::new(Command(
            make(tx),
        ))))
        .map_err(|_| Reject::Full)?;
    Ok(Pending {
        reply: rx,
        end: p.cap.end,
        sealed: p.cap.sealed.clone(),
        cap: Some(p.cap.clone()),
        observed: false,
        _credit: p.cap._credit.clone(),
    })
}
pub(in crate::local_writer) fn execute(
    p: &Prepared,
    sender: crate::lifecycle_resize_durable::Sender,
) -> Result<Pending<Fact>, Reject> {
    enqueue(p, |reply| Action::Execute {
        prepared: p.clone(),
        sender,
        reply,
    })
}
pub(in crate::local_writer) fn ready(p: &Prepared, fact: Fact) -> Result<Pending<()>, Reject> {
    enqueue(p, |reply| Action::Ready {
        prepared: p.clone(),
        fact,
        reply,
    })
}
pub(in crate::local_writer) fn publish(
    p: &Prepared,
    witness: crate::lifecycle_resize_durable::PublicationWitness,
) -> Result<Pending<()>, Reject> {
    enqueue(p, |reply| Action::Publish {
        prepared: p.clone(),
        witness,
        reply,
    })
}
impl Command {
    pub(in crate::local_writer) fn reject(self, error: Reject) {
        match self.0 {
            Action::Prepare { reply, .. } => {
                let _ = reply.try_send(Err(error));
            }
            Action::Execute {
                prepared, reply, ..
            } => {
                prepared.cancel();
                let _ = reply.try_send(Err(error));
            }
            Action::Ready {
                prepared, reply, ..
            } => {
                prepared.cancel();
                let _ = reply.try_send(Err(error));
            }
            Action::Publish {
                prepared, reply, ..
            } => {
                prepared.cancel();
                let _ = reply.try_send(Err(error));
            }
        }
    }
    pub(in crate::local_writer) fn process(
        self,
        state: Option<&mut State>,
        replies: &mut Option<ReplyState>,
        gate: &mut Option<view_gate::ViewGate>,
        sealed: &Arc<AtomicBool>,
        native: Option<&mut owner::native_host::producer::NativeIoState>,
    ) {
        let (Some(state), Some(replies), Some(gate)) = (state, replies.as_mut(), gate.as_mut())
        else {
            self.reject(Reject::Unsupported);
            return;
        };
        match self.0 {
            Action::Prepare {
                input,
                scope,
                local,
                credit,
                reply,
            } => {
                let result = (|| {
                    if !state.durable_only {
                        return Err(Reject::Unsupported);
                    }
                    gate.resize_ready()?;
                    gate.resize_proof(&scope, &input.proof)?;
                    if sealed.load(Ordering::Acquire)
                        || input.end <= Instant::now()
                        || input.end.saturating_duration_since(Instant::now())
                            > Duration::from_secs(1)
                    {
                        return Err(Reject::Sealed);
                    }
                    let (size, epochs) = replies.resize_baseline()?;
                    let current = gate.resize_state(&scope, epochs)?;
                    if input.request.state != current
                        || input.request.sequence <= state.floor
                        || state
                            .records
                            .iter()
                            .any(|(r, _, _)| r.operation == input.request.operation)
                    {
                        return Err(Reject::Epoch);
                    }
                    if state.records.len() == 64 {
                        return Err(Reject::Full);
                    }
                    let requested = input.request.size.ok_or(Reject::Bounds)?;
                    let (_, after_terminal) = replies.resize_preflight(requested.0, requested.1)?;
                    let fact = Fact {
                        request: input.request,
                        resource: crate::lifecycle_router::ResourceIdentity::from_proof(
                            &input.proof,
                        ),
                        backend: input.backend,
                        digest: input.digest,
                        before_size: size,
                        after_size: requested,
                        before_terminal: epochs,
                        after_terminal,
                        after: Epochs {
                            dimension: after_terminal.dimension_epoch,
                            ..current
                        },
                        outcome: Outcome::Applied,
                    };
                    fact.encode_stored().map_err(|_| Reject::Bounds)?;
                    let index = state.records.len();
                    state.floor = fact.request.sequence;
                    state.records.push((
                        fact.request.clone(),
                        scope.clone(),
                        Err(Error::Uncertain),
                    ));
                    let cap = Arc::new(Reservation {
                        #[cfg(test)]
                        before_claim: std::sync::Mutex::new(None),
                        #[cfg(test)]
                        before_native_admit: std::sync::Mutex::new(None),
                        #[cfg(test)]
                        cancel_reentry: std::sync::Mutex::new(None),
                        phase: AtomicU8::new(RESERVED),
                        end: input.end,
                        sealed: sealed.clone(),
                        scope,
                        proof: input.proof,
                        identity: Arc::new(()),
                        expected: fact.clone(),
                        result: std::sync::Mutex::new(None),
                        input: local,
                        _credit: credit,
                        index,
                    });
                    gate.invalidate_for_resize();
                    gate.durable_resize = Some(cap.clone());
                    Ok(Prepared { fact, cap })
                })();
                let cap = result.as_ref().ok().map(|p| p.cap.clone());
                if reply.try_send(result).is_err()
                    && let Some(cap) = cap
                {
                    cap.cancel();
                }
            }
            Action::Execute {
                prepared: p,
                sender,
                reply,
            } => {
                if let Some(native) = native {
                    let result = validate(&p, gate, replies).and_then(|()| {
                        p.cap
                            .phase
                            .compare_exchange(
                                RESERVED,
                                EXECUTING,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .map_err(|_| Reject::Pending)?;
                        gate.resize_proof(&p.cap.scope, &p.cap.proof)
                    });
                    if let Err(error) = result {
                        p.cancel();
                        let _ = reply.try_send(Err(error));
                    } else {
                        native.durable_resize(NativeExecution {
                            prepared: p,
                            sender,
                            reply,
                        });
                    }
                    return;
                }
                let result = (|| {
                    validate(&p, gate, replies)?;
                    p.cap
                        .phase
                        .compare_exchange(RESERVED, EXECUTING, Ordering::AcqRel, Ordering::Acquire)
                        .map_err(|_| Reject::Pending)?;
                    gate.resize_proof(&p.cap.scope, &p.cap.proof)?;
                    p.cap.alive()?;
                    #[cfg(test)]
                    if let Some(hook) = p.cap.before_claim.lock().unwrap().take() {
                        hook();
                    }
                    sender
                        .claim(&p.fact, &p.cap.identity, &p.cap.proof, || {
                            p.cap
                                .scope
                                .with_publication(|| {
                                    if p.cap.sealed.load(Ordering::Acquire)
                                        || Instant::now() >= p.cap.end
                                        || p.cap.phase.load(Ordering::Acquire) != EXECUTING
                                    {
                                        return Err(Reject::Sealed);
                                    }
                                    p.cap.proof.check().map_err(|_| Reject::Policy)
                                })
                                .map_err(|_| cc_desk_remote_record::lifecycle::Code::Stale)
                        })
                        .map_err(|_| Reject::Continuity)?;
                    let mut fact = p.fact.clone();
                    let size = Size {
                        rows: fact.after_size.0,
                        columns: fact.after_size.1,
                    };
                    let effect = if fact.before_size == fact.after_size {
                        Ok(size)
                    } else {
                        state
                            .backend
                            .as_mut()
                            .ok_or(Reject::Unsupported)?
                            .resize(size)
                    };
                    match effect {
                        Ok(actual) if actual == size => {
                            replies.resize_commit(size.rows, size.columns)?;
                        }
                        Err(Fault::KnownNotApplied) => {
                            fact.outcome = Outcome::NotApplied;
                            fact.after = fact.request.state;
                            fact.after_size = fact.before_size;
                            fact.after_terminal = fact.before_terminal;
                        }
                        _ => return Err(Reject::Continuity),
                    }
                    let (actual, epochs) = replies.resize_baseline()?;
                    if actual != fact.after_size || epochs != fact.after_terminal {
                        return Err(Reject::Continuity);
                    }
                    fact.encode_stored().map_err(|_| Reject::Bounds)?;
                    sender
                        .complete(fact.clone())
                        .map_err(|_| Reject::Continuity)?;
                    *p.cap.result.lock().map_err(|_| Reject::Thread)? = Some(fact.clone());
                    state.records[p.cap.index].2 = if fact.outcome == Outcome::Applied {
                        Ok(Applied {
                            request: fact.request.clone(),
                            after: fact.after,
                            size: Size {
                                rows: actual.0,
                                columns: actual.1,
                            },
                        })
                    } else {
                        Err(Error::NotApplied)
                    };
                    p.cap.alive()?;
                    p.cap
                        .phase
                        .compare_exchange(EXECUTING, EFFECT, Ordering::AcqRel, Ordering::Acquire)
                        .map_err(|_| Reject::Sealed)?;
                    Ok(fact)
                })();
                if result.is_err() {
                    p.cancel();
                }
                if reply.try_send(result).is_err() {
                    p.cancel();
                }
            }
            Action::Ready {
                prepared: p,
                fact,
                reply,
            } => {
                let result = (|| {
                    p.cap.alive()?;
                    gate.resize_proof(&p.cap.scope, &p.cap.proof)?;
                    if gate
                        .durable_resize
                        .as_ref()
                        .is_none_or(|r| !Arc::ptr_eq(r, &p.cap))
                        || p.cap.result.lock().map_err(|_| Reject::Thread)?.as_ref() != Some(&fact)
                    {
                        return Err(Reject::Epoch);
                    }
                    let (size, epochs) = replies.resize_baseline()?;
                    if size != fact.after_size
                        || epochs != fact.after_terminal
                        || gate.resize_state(&p.cap.scope, epochs)? != fact.after
                    {
                        return Err(Reject::Epoch);
                    }
                    p.cap.alive()?;
                    p.cap
                        .phase
                        .compare_exchange(EFFECT, READY, Ordering::AcqRel, Ordering::Acquire)
                        .map_err(|_| Reject::Pending)?;
                    Ok(())
                })();
                if result.is_err() {
                    p.cancel();
                }
                if reply.try_send(result).is_err() {
                    p.cancel();
                }
            }
            Action::Publish {
                prepared: p,
                witness,
                reply,
            } => {
                let result = (|| {
                    gate.resize_proof(&p.cap.scope, &p.cap.proof)?;
                    if gate
                        .durable_resize
                        .as_ref()
                        .is_none_or(|r| !Arc::ptr_eq(r, &p.cap))
                    {
                        return Err(Reject::Epoch);
                    }
                    p.cap.publish(witness)
                })();
                if result.is_err() {
                    p.cancel();
                }
                let _ = reply.try_send(result);
            }
        }
    }
}
fn validate(p: &Prepared, gate: &view_gate::ViewGate, replies: &ReplyState) -> Result<(), Reject> {
    p.cap.alive()?;
    if gate
        .durable_resize
        .as_ref()
        .is_none_or(|r| !Arc::ptr_eq(r, &p.cap))
        || p.fact != p.cap.expected
    {
        return Err(Reject::Epoch);
    }
    gate.resize_proof(&p.cap.scope, &p.cap.proof)?;
    let (size, epochs) = replies.resize_baseline()?;
    if size != p.fact.before_size
        || epochs != p.fact.before_terminal
        || gate.resize_state(&p.cap.scope, epochs)? != p.fact.request.state
    {
        return Err(Reject::Epoch);
    }
    Ok(())
}
impl State {
    pub(in crate::local_writer) fn hold_output(
        &mut self,
        request: OutputRequest,
        gate: &Option<view_gate::ViewGate>,
    ) -> Option<OutputRequest> {
        if gate
            .as_ref()
            .and_then(|g| g.durable_resize.as_ref())
            .is_some_and(|r| r.phase.load(Ordering::Acquire) != PUBLISHED)
        {
            if self.held.len() == QUEUE {
                if let Some(r) = gate.as_ref().and_then(|g| g.durable_resize.as_ref()) {
                    r.cancel();
                }
                let _ = request.reply.try_send(OutputResult::Rejected(Reject::Full));
            } else {
                self.held.push_back(request);
            }
            None
        } else {
            Some(request)
        }
    }
    pub(in crate::local_writer) fn next_output(
        &mut self,
        gate: &Option<view_gate::ViewGate>,
    ) -> Option<OutputRequest> {
        if gate
            .as_ref()
            .and_then(|g| g.durable_resize.as_ref())
            .is_some_and(|r| r.phase.load(Ordering::Acquire) == PUBLISHED)
        {
            self.held.pop_front()
        } else {
            None
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        for output in self.held.drain(..) {
            let _ = output
                .reply
                .try_send(OutputResult::Rejected(Reject::Sealed));
        }
    }
}

/// Owns the controlled peer; never exports a production route or persistent authority.
pub(in crate::local_writer) struct Coordinator {
    peer: view_gate::PeerViewHandle,
    owner: crate::lifecycle_resize_durable::Owner,
    prepared: Option<Prepared>,
    sender: Option<crate::lifecycle_resize_durable::Sender>,
    recorded: Option<Fact>,
}
impl Coordinator {
    #[cfg(test)]
    pub(in crate::local_writer) fn fake(
        peer: view_gate::PeerViewHandle,
        owner: crate::lifecycle_resize_durable::Owner,
    ) -> Self {
        Self {
            peer,
            owner,
            prepared: None,
            sender: None,
            recorded: None,
        }
    }
    pub(in crate::local_writer) fn begin(
        &mut self,
        request: Operation,
        budget: Duration,
    ) -> Result<Option<Fact>, Reject> {
        if self
            .prepared
            .as_ref()
            .is_some_and(|p| p.cap.phase.load(Ordering::Acquire) != PUBLISHED)
        {
            return Err(Reject::Pending);
        }
        if budget.is_zero() || budget > Duration::from_secs(1) {
            return Err(Reject::Bounds);
        }
        let end = Instant::now() + budget;
        if let Some(f) = self.owner.history(&request).map_err(|_| Reject::Policy)? {
            return Ok(Some(f));
        }
        let (proof, backend, digest) = self
            .owner
            .actor_binding(&request)
            .map_err(|_| Reject::Policy)?;
        let prepared = self
            .peer
            .prepare_durable_resize(Prepare {
                request,
                proof,
                backend,
                digest,
                end,
            })?
            .wait()
            .map_err(|_| Reject::Continuity)??;
        // Hold the same guard before any durable write so all failure paths close it.
        self.prepared = Some(prepared.clone());
        self.recorded = None;
        match self
            .owner
            .begin_prepared(prepared.fact.clone(), end, prepared.cap.identity.clone())
        {
            Ok(Some(sender)) => {
                self.sender = Some(sender);
                Ok(None)
            }
            _ => {
                self.cancel();
                Err(Reject::Continuity)
            }
        }
    }
    pub(in crate::local_writer) fn execute(&mut self) -> Result<Fact, Reject> {
        let p = self.prepared.as_ref().ok_or(Reject::Pending)?.clone();
        let sender = self.sender.take().ok_or(Reject::Pending)?;
        let result = execute(&p, sender).and_then(|pending| {
            pending
                .wait()
                .map_err(|_| Reject::Continuity)
                .and_then(|r| r)
        });
        if result.is_err() {
            self.cancel();
        }
        result
    }
    pub(in crate::local_writer) fn record(&mut self) -> Result<Fact, Reject> {
        match self.owner.record_actor() {
            Ok(Some(f)) => {
                self.recorded = Some(f.clone());
                Ok(f)
            }
            _ => {
                self.cancel();
                Err(Reject::Continuity)
            }
        }
    }
    pub(in crate::local_writer) fn publish(&mut self) -> Result<(), Reject> {
        let p = self.prepared.as_ref().ok_or(Reject::Pending)?.clone();
        let f = self.recorded.as_ref().ok_or(Reject::Pending)?.clone();
        let result = (|| {
            ready(&p, f.clone())?
                .wait()
                .map_err(|_| Reject::Continuity)??;
            let witness = self
                .owner
                .publication_witness(&f)
                .map_err(|_| Reject::Policy)?;
            publish(&p, witness)?
                .wait()
                .map_err(|_| Reject::Continuity)??;
            // No further storage write is needed. If the reply was not observed,
            // cancel reconciles the slot's published fact without replaying it.
            self.owner.confirm_actor_publication();
            Ok(())
        })();
        if result.is_err() {
            self.cancel();
        }
        result
    }
    pub(in crate::local_writer) fn cancel(&mut self) {
        if let Some(p) = &self.prepared {
            p.cancel();
        }
        self.owner.cancel();
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn peer(&self) -> &view_gate::PeerViewHandle {
        &self.peer
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn owner(&mut self) -> &mut crate::lifecycle_resize_durable::Owner {
        &mut self.owner
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn reservation(&self) -> Arc<Reservation> {
        self.prepared.as_ref().unwrap().cap.clone()
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn test_sender(&self) -> crate::lifecycle_resize_durable::Sender {
        self.sender.as_ref().unwrap().clone()
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn test_replace_sender(
        &mut self,
        sender: crate::lifecycle_resize_durable::Sender,
    ) {
        self.sender = Some(sender);
    }
    #[cfg(test)]
    pub(in crate::local_writer) fn test_prepared(&self) -> Prepared {
        self.prepared.as_ref().unwrap().clone()
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        if let Some(p) = &self.prepared {
            p.cancel();
        }
    }
}
