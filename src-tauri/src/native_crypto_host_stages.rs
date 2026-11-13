//! 单个原网络任务的两个一次性签名阶段；没有 key/线程保管或重新批准能力。
use super::{FrozenHostBinding, HostSignerSet};
use crate::native_crypto::{owner::*, signers::CngTlsKey, SigningPurpose};
use crate::remote_host_network::NetworkSigningScope;
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_transport::wss::{
    RelayAdmissionIdentity, RelayRole, RelaySigner, SignError, SigningRequest,
};
use parking_lot::Mutex;
use rustls::sign::{Signer, SigningKey};
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Relay,
    Endpoint,
}
impl Stage {
    fn index(self) -> usize {
        if self == Self::Endpoint {
            0
        } else {
            1
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Dormant,
    RelayArmed,
    RelayClaimed,
    RelayDelivered,
    EndpointArmed,
    EndpointClaimed,
    EndpointDelivered,
}
#[cfg(test)]
type MintHook = Box<dyn FnOnce(&NativeSigningAttempt) + Send>;
#[cfg(test)]
type TicketHook = Box<dyn FnOnce(&mut SignTicket) + Send>;
#[cfg(test)]
type TestRelayFuture = Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send>>;
struct Facts {
    phase: Phase,
    setup_end: Option<Instant>,
    #[cfg(test)]
    attempts: [Option<NativeSigningAttempt>; 2],
    #[cfg(test)]
    mints: [usize; 2],
    #[cfg(test)]
    on_mint: Option<MintHook>,
    #[cfg(test)]
    on_ticket: Option<TicketHook>,
}
struct Shared {
    facts: Mutex<Facts>,
    fence: Arc<SigningFence>,
    handles: [KeyRequestHandle; 2],
    token: Arc<NativeRevocation>,
    cap: Instant,
    origin: CanonicalOrigin,
    relay_id: [u8; 16],
}
/// 唯一 arm 权，随原 fresh material 移入原网络任务，不能 Clone。
pub(crate) struct HostSigningStages {
    shared: Arc<Shared>,
}
impl HostSigningStages {
    pub(super) fn prepare(
        handles: [KeyRequestHandle; 2],
        cap: Instant,
        token: Arc<NativeRevocation>,
        binding: &FrozenHostBinding,
    ) -> Result<HostSignerSet, SignFailure> {
        if handles[0].binding().0 != SigningPurpose::Endpoint
            || handles[1].binding().0 != SigningPurpose::Relay
            || token.is_revoked()
            || Instant::now() >= cap
        {
            token.revoke();
            return Err(SignFailure::Closed);
        }
        let shared = Arc::new(Shared {
            facts: Mutex::new(Facts {
                phase: Phase::Dormant,
                setup_end: None,
                #[cfg(test)]
                attempts: [None, None],
                #[cfg(test)]
                mints: [0, 0],
                #[cfg(test)]
                on_mint: None,
                #[cfg(test)]
                on_ticket: None,
            }),
            fence: SigningFence::new_revocable(cap, token.clone()),
            handles,
            token,
            cap,
            origin: binding.origin.clone(),
            relay_id: binding.relay_id,
        });
        Ok(HostSignerSet {
            stages: Self {
                shared: shared.clone(),
            },
            endpoint: Arc::new(HostTlsKey(shared.clone())),
            relay: RelayAdmissionIdentity::new(
                binding.relay_id,
                RelayRole::Host,
                Arc::new(HostRelaySigner(shared)),
            ),
        })
    }
    pub(crate) fn matches_approval(
        &self,
        approval: &crate::remote_host_network::NativeApprovalStop,
    ) -> bool {
        approval.matches_native(&self.shared.token, self.shared.cap)
    }
    pub(crate) fn begin_relay(&mut self, scope: NetworkSigningScope) -> Result<(), SignFailure> {
        if !scope.matches(&self.shared.token, self.shared.cap) {
            self.cancel();
            return Err(SignFailure::Closed);
        }
        let mut facts = self.shared.facts.lock();
        if facts.phase != Phase::Dormant || !self.shared.live(Some(scope.end())) {
            return Err(SignFailure::Closed);
        }
        facts.setup_end = Some(scope.end());
        facts.phase = Phase::RelayArmed;
        Ok(())
    }
    pub(crate) fn arm_endpoint(&mut self) -> Result<(), SignFailure> {
        let mut facts = self.shared.facts.lock();
        // 原 connect_admitted 已返回 Accepted；已交付签名的五秒窗不再是网络寿命。
        if facts.phase != Phase::RelayDelivered || !self.shared.live(facts.setup_end) {
            return Err(SignFailure::Closed);
        }
        facts.phase = Phase::EndpointArmed;
        Ok(())
    }
    pub(crate) fn cancel(&self) {
        self.shared.token.revoke();
    }
    #[cfg(test)]
    pub(crate) fn test_begin_relay(&mut self, end: Instant) -> Result<(), SignFailure> {
        self.begin_relay(NetworkSigningScope::test_new(
            self.shared.token.clone(),
            self.shared.cap,
            end,
        ))
    }
    /// 仅隔离旧 custody/factory 故障，不证明生产 Relay/Accepted 顺序；不 mint 或开 key。
    #[cfg(test)]
    pub(crate) fn test_endpoint_only(&mut self, end: Instant) -> Result<(), SignFailure> {
        let mut facts = self.shared.facts.lock();
        if facts.phase != Phase::Dormant || !self.shared.live(Some(end)) {
            return Err(SignFailure::Closed);
        }
        facts.setup_end = Some(end.min(self.shared.cap));
        facts.phase = Phase::EndpointArmed;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_mints(&self) -> [usize; 2] {
        self.shared.facts.lock().mints
    }
    #[cfg(test)]
    pub(crate) fn test_on_mint(&self, hook: MintHook) {
        self.shared.facts.lock().on_mint = Some(hook);
    }
    #[cfg(test)]
    pub(crate) fn test_on_ticket(&self, hook: TicketHook) {
        self.shared.facts.lock().on_ticket = Some(hook);
    }
    /// 仅验证底层原 id 的 stale cancel；不代表网络阶段许可。
    #[cfg(test)]
    pub(crate) fn test_raw_relay_attempt(&self) -> Result<SignTicket, SignFailure> {
        let attempt = self.shared.fence.advance(
            &self.shared.handles,
            (Instant::now() + Duration::from_secs(5)).min(self.shared.cap),
        )?;
        self.shared.handles[1].begin_attempt(
            b"PUBLIC_STALE_ID".to_vec(),
            SignatureEncoding::Fixed,
            attempt,
        )
    }
    /// 仅制造原 key Busy 故障，不是网络阶段授权入口。
    #[cfg(test)]
    pub(crate) fn test_queue_relay(&self) -> Result<SignTicket, SignFailure> {
        self.shared.handles[1].begin(b"PUBLIC_BUSY_STAGE".to_vec(), SignatureEncoding::Fixed)
    }
    #[cfg(test)]
    pub(crate) fn test_relay_future(
        &self,
        message: Vec<u8>,
    ) -> Result<TestRelayFuture, SignFailure> {
        Ok(Box::pin(RelayFuture::new(&self.shared, message)?))
    }
    #[cfg(test)]
    pub(crate) fn test_attempt(&self, purpose: SigningPurpose) -> Option<NativeSigningAttempt> {
        let index = match purpose {
            SigningPurpose::Endpoint => 0,
            SigningPurpose::Relay => 1,
            SigningPurpose::Issuer => return None,
        };
        self.shared.facts.lock().attempts[index].clone()
    }
}
impl Drop for HostSigningStages {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Shared {
    fn live(&self, end: Option<Instant>) -> bool {
        if Instant::now() >= self.cap || end.is_some_and(|end| Instant::now() >= end) {
            self.token.revoke();
        }
        !self.token.is_revoked()
    }
    fn endpoint_probe(&self) -> bool {
        let facts = self.facts.lock();
        !matches!(
            facts.phase,
            Phase::EndpointClaimed | Phase::EndpointDelivered
        ) && self.live(facts.setup_end)
    }
    fn claim(self: &Arc<Self>, stage: Stage) -> Result<Claim, SignFailure> {
        let mut facts = self.facts.lock();
        let expected = match stage {
            Stage::Relay => Phase::RelayArmed,
            Stage::Endpoint => Phase::EndpointArmed,
        };
        if facts.phase != expected || !self.live(facts.setup_end) {
            return Err(SignFailure::Closed);
        }
        facts.phase = match stage {
            Stage::Relay => Phase::RelayClaimed,
            Stage::Endpoint => Phase::EndpointClaimed,
        };
        Ok(Claim {
            shared: self.clone(),
            stage,
            active: true,
        })
    }
}
struct Claim {
    shared: Arc<Shared>,
    stage: Stage,
    active: bool,
}
impl Claim {
    fn mint(&self) -> Result<NativeSigningAttempt, SignFailure> {
        let end = self
            .shared
            .facts
            .lock()
            .setup_end
            .ok_or(SignFailure::Closed)?;
        if !self.shared.live(Some(end)) {
            return Err(SignFailure::Closed);
        }
        let end = end
            .min(self.shared.cap)
            .min(Instant::now() + Duration::from_secs(5));
        // 阶段锁已释放；不持它调用 fence、adapter 或任何外部 hook。
        let attempt = self.shared.fence.advance(&self.shared.handles, end)?;
        #[cfg(test)]
        {
            let hook = {
                let mut facts = self.shared.facts.lock();
                facts.attempts[self.stage.index()] = Some(attempt.clone());
                facts.mints[self.stage.index()] += 1;
                facts.on_mint.take()
            };
            if let Some(hook) = hook {
                hook(&attempt);
            }
        }
        Ok(attempt)
    }
    fn deliver(&mut self, attempt: &NativeSigningAttempt) -> Result<(), SignFailure> {
        if !attempt.is_current() {
            return Err(SignFailure::Closed);
        }
        let mut facts = self.shared.facts.lock();
        let expected = match self.stage {
            Stage::Relay => Phase::RelayClaimed,
            Stage::Endpoint => Phase::EndpointClaimed,
        };
        if facts.phase != expected || !self.shared.live(facts.setup_end) {
            return Err(SignFailure::Closed);
        }
        facts.phase = match self.stage {
            Stage::Relay => Phase::RelayDelivered,
            Stage::Endpoint => Phase::EndpointDelivered,
        };
        self.active = false;
        Ok(())
    }
    fn fail(&mut self) {
        if self.active {
            self.shared.token.revoke();
            self.active = false;
        }
    }
}
impl Drop for Claim {
    fn drop(&mut self) {
        self.fail();
    }
}
struct PollUnwind {
    token: Arc<NativeRevocation>,
    armed: bool,
}
impl Drop for PollUnwind {
    fn drop(&mut self) {
        if self.armed {
            self.token.revoke();
        }
    }
}
struct HostTlsKey(Arc<Shared>);
struct HostTlsSigner(Arc<Shared>);
impl fmt::Debug for HostTlsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostTlsKey(<one-stage>)")
    }
}
impl SigningKey for HostTlsKey {
    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
    fn public_key(&self) -> Option<rustls::pki_types::SubjectPublicKeyInfoDer<'_>> {
        Some(self.0.handles[0].binding().1.into())
    }
    fn choose_scheme(&self, offered: &[rustls::SignatureScheme]) -> Option<Box<dyn Signer>> {
        (self.0.endpoint_probe()
            && offered.contains(&rustls::SignatureScheme::ECDSA_NISTP256_SHA256))
        .then(|| Box::new(HostTlsSigner(self.0.clone())) as Box<dyn Signer>)
    }
}
impl fmt::Debug for HostTlsSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostTlsSigner(<one-stage>)")
    }
}
impl Signer for HostTlsSigner {
    fn scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ECDSA_NISTP256_SHA256
    }
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        let error = |_| rustls::Error::General("HOST_STAGE_UNAVAILABLE".into());
        let mut claim = self.0.claim(Stage::Endpoint).map_err(error)?;
        let attempt = claim.mint().map_err(error)?;
        let key =
            CngTlsKey::for_attempt(self.0.handles[0].clone(), attempt.clone()).map_err(error)?;
        let signature = Signer::sign(&key, message)?;
        claim.deliver(&attempt).map_err(error)?;
        Ok(signature)
    }
}
struct HostRelaySigner(Arc<Shared>);
impl RelaySigner for HostRelaySigner {
    fn sign<'a>(
        &'a self,
        request: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        if request.origin() != self.0.origin.as_str()
            || request.key_id() != self.0.relay_id
            || request.role() != RelayRole::Host
        {
            return Box::pin(std::future::ready(Err(SignError)));
        }
        // sign 调用即消费；未 poll 的 Future Drop 也不能归还许可。
        match RelayFuture::new(&self.0, request.transcript().to_vec()) {
            Ok(future) => Box::pin(future),
            Err(_) => Box::pin(std::future::ready(Err(SignError))),
        }
    }
}
struct RelayFuture {
    claim: Option<Claim>,
    ticket: Option<SignTicket>,
    attempt: Option<NativeSigningAttempt>,
    message: Option<Vec<u8>>,
    done: bool,
}
impl RelayFuture {
    fn new(shared: &Arc<Shared>, message: Vec<u8>) -> Result<Self, SignFailure> {
        let claim = shared.claim(Stage::Relay)?;
        Ok(Self {
            claim: Some(claim),
            ticket: None,
            attempt: None,
            message: Some(message),
            done: false,
        })
    }
    fn finish(&mut self, result: Result<[u8; 64], SignError>) -> Poll<Result<[u8; 64], SignError>> {
        if result.is_err() {
            if let Some(claim) = &mut self.claim {
                claim.fail();
            }
        }
        self.done = true;
        // 纯撤销已发生，才能析构可能通知外部 waker 的原票据。
        drop(self.ticket.take());
        drop(self.claim.take());
        Poll::Ready(result)
    }
    fn poll_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<[u8; 64], SignError>> {
        if self.attempt.is_none() {
            match self.claim.as_ref().expect("one claim").mint() {
                Ok(attempt) => self.attempt = Some(attempt),
                Err(_) => return self.finish(Err(SignError)),
            }
        }
        if self.ticket.is_none() {
            let claim = self.claim.as_ref().expect("one claim");
            match claim.shared.handles[1].begin_attempt(
                self.message.take().expect("one message"),
                SignatureEncoding::Fixed,
                self.attempt.as_ref().unwrap().clone(),
            ) {
                Ok(ticket) => {
                    self.ticket = Some(ticket);
                    #[cfg(test)]
                    {
                        let hook = claim.shared.facts.lock().on_ticket.take();
                        if let Some(hook) = hook {
                            hook(self.ticket.as_mut().unwrap());
                        }
                    }
                }
                Err(_) => return self.finish(Err(SignError)),
            }
        }
        match Pin::new(self.ticket.as_mut().unwrap()).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                let result = result
                    .map_err(|_| SignError)
                    .and_then(|bytes| bytes.try_into().map_err(|_| SignError))
                    .and_then(|signature| {
                        self.claim
                            .as_mut()
                            .unwrap()
                            .deliver(self.attempt.as_ref().unwrap())
                            .map_err(|_| SignError)?;
                        Ok(signature)
                    });
                self.finish(result)
            }
        }
    }
}
impl Future for RelayFuture {
    type Output = Result<[u8; 64], SignError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.done {
            return Poll::Ready(Err(SignError));
        }
        // 原 SignTicket 存在本手写 Future 字段里，不在 opaque async 栈先析构。
        let mut unwind = PollUnwind {
            token: self.claim.as_ref().unwrap().shared.token.clone(),
            armed: true,
        };
        let result = self.poll_inner(cx);
        unwind.armed = false;
        result
    }
}
impl Drop for RelayFuture {
    fn drop(&mut self) {
        if !self.done {
            if let Some(claim) = &mut self.claim {
                claim.fail();
            }
        }
        // 字段析构在该 Drop 返回后发生；queued ticket 的 wake 只能晚于纯撤销。
    }
}
