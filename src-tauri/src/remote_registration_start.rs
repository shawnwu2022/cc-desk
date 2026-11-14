//! 独立登记 owner；不占用永久 Host activation/source 单槽。
use super::*;
use crate::remote_host::activation::{
    ActivationOperation, ActivationReservation, PreparationResult,
};
use crate::remote_host_approval::NativeRegistrationKeyApproval;
use cc_desk_remote_security::{cancellation, LocalIdentity, PeerPin, Role, Unbound};
use std::{
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    time::Duration,
};
#[path = "remote_registration_configuration.rs"]
pub(crate) mod configuration;
use configuration::FrozenRegistrationStart;
#[cfg(test)]
#[path = "tests/native_registration_start.rs"]
mod tests;

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(tag = "state")]
pub(crate) enum RegistrationReply {
    Locked,
    PendingKeyApproval,
    Connecting,
    Verifying,
    PendingBrowserApproval,
    PendingNativePersistence,
    NativeStoreDeclined,
    NativeStoreFailed,
    NativeStoreUnknown,
    NativeStoredRegistrationIncomplete,
    InstalledLocked {
        receipt: String,
        remembered: bool,
    },
    DeliveryUnconfirmed {
        receipt: String,
        remembered: bool,
    },
    RetirementUnknown {
        receipt: Option<String>,
        remembered: bool,
    },
    Cancelled,
    Failed,
    Unknown,
}
fn settle_failure(reply: &mut RegistrationReply, stopped: bool, known: bool) {
    if matches!(
        reply,
        RegistrationReply::NativeStoreDeclined
            | RegistrationReply::NativeStoreFailed
            | RegistrationReply::NativeStoreUnknown
            | RegistrationReply::NativeStoredRegistrationIncomplete
            | RegistrationReply::DeliveryUnconfirmed { .. }
            | RegistrationReply::InstalledLocked { .. }
            | RegistrationReply::RetirementUnknown { .. }
    ) {
        return;
    }
    *reply = if !known {
        RegistrationReply::Unknown
    } else if stopped {
        RegistrationReply::Cancelled
    } else {
        RegistrationReply::Failed
    };
}
#[derive(Default)]
struct AttemptCommitGate {
    serial: Mutex<()>,
    stopped: AtomicBool,
}
impl AttemptCommitGate {
    fn cancel(&self) {
        let _guard = self.serial.lock();
        self.stopped.store(true, Ordering::Release);
    }
    fn commit<T>(&self, work: impl FnOnce() -> Result<T, SafeError>) -> Result<T, SafeError> {
        let _guard = self.serial.lock();
        if self.stopped.load(Ordering::Acquire) {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        work()
    }
}
struct Attempt {
    identity: OwnerIdentity,
    approval: Arc<NativeRegistrationKeyApproval>,
    operation: Arc<ActivationOperation>,
    commit: AttemptCommitGate,
    channel_cancel: cc_desk_remote_security::CancelHandle,
    reply: Mutex<RegistrationReply>,
    remembered: AtomicBool,
    task: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<bool>>>,
    joined: AtomicU8,
    #[cfg(test)]
    loopback: bool,
}
enum BrowserPrompt {
    Native(Box<(tauri::AppHandle, tauri::Window)>),
    #[cfg(test)]
    Test(bool, Option<TestPersistence>),
}
impl Attempt {
    fn stopped(&self) -> bool {
        self.commit.stopped.load(Ordering::Acquire)
    }
    fn cancel(&self) {
        self.commit.cancel();
        self.approval.cancel();
        self.operation.cancel();
        self.channel_cancel.cancel();
    }
    fn unknown(&self) {
        let mut reply = self.reply.lock();
        let receipt = match &*reply {
            RegistrationReply::InstalledLocked { receipt, .. }
            | RegistrationReply::DeliveryUnconfirmed { receipt, .. } => Some(receipt.clone()),
            RegistrationReply::RetirementUnknown { receipt, .. } => receipt.clone(),
            _ => None,
        };
        *reply = RegistrationReply::RetirementUnknown {
            receipt,
            remembered: self.remembered.load(Ordering::Acquire),
        };
    }
    async fn join(&self) -> bool {
        let mut slot = self.task.lock().await;
        if let Some(task) = slot.as_mut() {
            let known = matches!(task.await, Ok(true));
            *slot = None;
            self.joined
                .store(if known { 1 } else { 2 }, Ordering::Release);
            if !known {
                self.unknown();
            }
        }
        // 即使原任务 panic，也继续观察它已发布的原生 key 线程；Unknown 不能提前丢保管。
        loop {
            let state = self.operation.retire();
            if state.unknown {
                self.joined.store(2, Ordering::Release);
                self.unknown();
            }
            if !state.outstanding {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if self.joined.load(Ordering::Acquire) != 1 {
            self.unknown();
        }
        self.joined.load(Ordering::Acquire) == 1
    }
}
#[derive(Default)]
struct CustodyState {
    closing: bool,
    attempts: Vec<Arc<Attempt>>,
}
#[derive(Default)]
pub(crate) struct RegistrationCustody {
    state: Mutex<CustodyState>,
}
impl RegistrationCustody {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn request(
        self: &Arc<Self>,
        binding: Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: NativeRunAdmission,
        frozen: FrozenRegistrationStart,
        controller: Arc<RemoteHostController>,
        app: tauri::AppHandle,
        window: tauri::Window,
    ) -> Result<RegistrationReply, SafeError> {
        RemoteHostController::authenticate(&binding, &caller, &admission)?;
        self.reap_finished().await;
        RemoteHostController::authenticate(&binding, &caller, &admission)?;
        let identity = OwnerIdentity {
            caller: caller.clone(),
            binding: Arc::downgrade(&binding),
            origin: admission.network_origin()?,
        };
        let mut state = self.state.lock();
        if state.closing
            || state
                .attempts
                .iter()
                .any(|a| a.joined.load(Ordering::Acquire) != 1)
        {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        state.attempts.clear();
        let approval = NativeRegistrationKeyApproval::show(&app, &window, frozen)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let reservation = ActivationReservation::registration();
        let (channel_cancel, token) = cancellation();
        let attempt = Arc::new(Attempt {
            identity,
            approval,
            operation: reservation.operation(),
            commit: AttemptCommitGate::default(),
            channel_cancel,
            reply: Mutex::new(RegistrationReply::PendingKeyApproval),
            remembered: AtomicBool::new(false),
            task: tokio::sync::Mutex::new(None),
            joined: AtomicU8::new(0),
            #[cfg(test)]
            loopback: false,
        });
        // 任务在 publication 锁内创建且保留原 handle；退出不能漏过半发布的 owner。
        let original = attempt.clone();
        let task = tauri::async_runtime::spawn(async move {
            run(
                original,
                binding,
                caller,
                admission,
                reservation,
                controller,
                BrowserPrompt::Native(Box::new((app, window))),
                token,
            )
            .await
        });
        *attempt
            .task
            .try_lock()
            .expect("unpublished registration task") = Some(task);
        state.attempts.push(attempt);
        Ok(RegistrationReply::PendingKeyApproval)
    }
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn test_request(
        self: &Arc<Self>,
        binding: Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        admission: NativeRunAdmission,
        frozen: FrozenRegistrationStart,
        controller: Arc<RemoteHostController>,
        first_yes: Option<bool>,
        browser_yes: bool,
        persistence: Option<TestPersistence>,
    ) -> Result<TestRegistrationHandle, SafeError> {
        RemoteHostController::authenticate(&binding, &caller, &admission)?;
        self.reap_finished().await;
        RemoteHostController::authenticate(&binding, &caller, &admission)?;
        let identity = OwnerIdentity {
            caller: caller.clone(),
            binding: Arc::downgrade(&binding),
            origin: admission.network_origin()?,
        };
        let mut state = self.state.lock();
        if state.closing
            || state
                .attempts
                .iter()
                .any(|a| a.joined.load(Ordering::Acquire) != 1)
        {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        state.attempts.clear();
        let approval = NativeRegistrationKeyApproval::test_pending(frozen);
        if let Some(yes) = first_yes {
            approval.test_complete(yes);
        }
        let reservation = ActivationReservation::registration();
        let (channel_cancel, token) = cancellation();
        let attempt = Arc::new(Attempt {
            identity,
            approval,
            operation: reservation.operation(),
            commit: AttemptCommitGate::default(),
            channel_cancel,
            reply: Mutex::new(RegistrationReply::PendingKeyApproval),
            remembered: AtomicBool::new(false),
            task: tokio::sync::Mutex::new(None),
            joined: AtomicU8::new(0),
            loopback: true,
        });
        let original = attempt.clone();
        *attempt.task.try_lock().unwrap() = Some(tauri::async_runtime::spawn(async move {
            run(
                original,
                binding,
                caller,
                admission,
                reservation,
                controller,
                BrowserPrompt::Test(browser_yes, persistence),
                token,
            )
            .await
        }));
        state.attempts.push(attempt.clone());
        Ok(TestRegistrationHandle(attempt))
    }
    async fn reap_finished(&self) {
        let attempts = self.state.lock().attempts.clone();
        for a in attempts {
            let finished = a
                .task
                .try_lock()
                .ok()
                .is_some_and(|slot| slot.as_ref().is_some_and(|t| t.inner().is_finished()));
            if finished {
                a.join().await;
            }
        }
    }
    pub(crate) async fn current(
        &self,
        binding: &Arc<DocumentBinding<NativeRun>>,
        caller: &CallerIdentity,
        admission: &NativeRunAdmission,
        cancel: bool,
    ) -> Result<RegistrationReply, SafeError> {
        RemoteHostController::authenticate(binding, caller, admission)?;
        let attempt = self.state.lock().attempts.last().cloned();
        let Some(a) = attempt else {
            return Ok(RegistrationReply::Locked);
        };
        a.identity.check(binding, caller, admission)?;
        if cancel {
            a.cancel();
        }
        // 已结束的 handle 仍必须由原 join 消费，不能以业务状态释放容量。
        let finished = a
            .task
            .try_lock()
            .ok()
            .is_some_and(|slot| slot.as_ref().is_some_and(|t| t.inner().is_finished()));
        if finished {
            a.join().await;
        }
        let reply = a.reply.lock().clone();
        Ok(reply)
    }
    pub(crate) fn begin_shutdown(&self) {
        let attempts = {
            let mut state = self.state.lock();
            state.closing = true;
            state.attempts.clone()
        };
        for a in attempts {
            a.cancel();
        }
    }
    pub(crate) async fn join_all(&self) -> bool {
        self.begin_shutdown();
        let attempts = self.state.lock().attempts.clone();
        let mut known = true;
        for a in attempts {
            known &= a.join().await;
        }
        known
    }
}
#[cfg(test)]
pub(crate) struct TestRegistrationHandle(Arc<Attempt>);
#[cfg(test)]
impl TestRegistrationHandle {
    pub(crate) fn finished(&self) -> bool {
        self.0
            .task
            .try_lock()
            .ok()
            .is_some_and(|slot| slot.as_ref().is_some_and(|task| task.inner().is_finished()))
    }
    pub(crate) fn reply(&self) -> RegistrationReply {
        self.0.reply.lock().clone()
    }
    pub(crate) async fn join(&self) -> bool {
        self.0.join().await
    }
    pub(crate) fn retired(&self) -> bool {
        self.0.joined.load(Ordering::Acquire) == 1 && self.0.operation.snapshot().retired()
    }
}
async fn checked<F: std::future::Future<Output = Result<(), SafeError>>>(
    work: F,
) -> Result<(), SafeError> {
    tokio::pin!(work);
    std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work.as_mut().poll(cx))) {
            Ok(p) => p,
            Err(_) => std::task::Poll::Ready(Err(error("REMOTE_HOST_UNKNOWN"))),
        }
    })
    .await
}
#[allow(clippy::too_many_arguments)]
async fn run(
    attempt: Arc<Attempt>,
    binding: Arc<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    admission: NativeRunAdmission,
    reservation: ActivationReservation,
    controller: Arc<RemoteHostController>,
    mut prompt: BrowserPrompt,
    token: cc_desk_remote_security::CancelToken,
) -> bool {
    let mut tunnel = None;
    let mut original_reservation = Some(reservation);
    let work = async {
        let permit = loop {
            attempt.identity.check(&binding, &caller, &admission)?;
            if attempt.stopped() {
                return Err(error("REMOTE_HOST_LOCKED"));
            }
            if let Some(permit) = attempt.approval.take() {
                break permit;
            }
            if !attempt.approval.pending() {
                return Err(error("REMOTE_HOST_LOCKED"));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let frozen = permit.frozen;
        let end = frozen.end;
        let signing_token = permit.signing.token();
        let check = || -> Result<(), SafeError> {
            attempt.identity.check(&binding, &caller, &admission)?;
            if attempt.stopped() || Instant::now() >= end || signing_token.is_revoked() {
                Err(error("REMOTE_HOST_LOCKED"))
            } else {
                Ok(())
            }
        };
        check()?;
        let owner =
            NativeRegistrationOwner::begin(&binding, &caller, &admission, frozen.context()?)?;
        let mut reservation = original_reservation
            .take()
            .ok_or_else(|| error("REMOTE_HOST_LOCKED"))?;
        let publisher = reservation
            .prepare_keys(permit.signing.token(), end)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let supplier = frozen.supplier()?;
        let (prepared, mut ready) = tokio::sync::oneshot::channel();
        reservation
            .spawn_prepared(move |_| {
                let result = publisher.prepare(supplier);
                let ok = result.is_ok();
                let delivered = prepared.send(result).is_ok();
                PreparationResult::Complete(ok && delivered)
            })
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let mut keys = loop {
            check()?;
            attempt.operation.advance_join();
            match ready.try_recv() {
                Ok(result) => break result.map_err(|_| error("REMOTE_HOST_LOCKED"))?,
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                Err(_) => return Err(error("REMOTE_HOST_LOCKED")),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        check()?;
        *attempt.reply.lock() = RegistrationReply::Connecting;
        let setup_end = (Instant::now() + Duration::from_secs(8)).min(end);
        keys.stages
            .begin_registration(permit.signing, setup_end)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        #[cfg(test)]
        let socket = if attempt.loopback {
            cc_desk_remote_transport::wss::test_only_connect_admitted_loopback_with_mode(
                frozen.origin.as_str(),
                frozen.address,
                setup_end.saturating_duration_since(Instant::now()),
                &keys.relay,
                frozen.mode,
                frozen.tunnel,
            )
            .await
        } else {
            cc_desk_remote_transport::wss::connect_admitted_with_mode(
                frozen.origin.as_str(),
                frozen.address,
                setup_end.saturating_duration_since(Instant::now()),
                &keys.relay,
                frozen.mode,
                frozen.tunnel,
            )
            .await
        }
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        #[cfg(not(test))]
        let socket = cc_desk_remote_transport::wss::connect_admitted_with_mode(
            frozen.origin.as_str(),
            frozen.address,
            setup_end.saturating_duration_since(Instant::now()),
            &keys.relay,
            frozen.mode,
            frozen.tunnel,
        )
        .await
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        attempt.identity.check(&binding, &caller, &admission)?;
        if attempt.stopped() || Instant::now() >= setup_end {
            return Err(error("REMOTE_HOST_LOCKED"));
        }
        keys.stages
            .arm_endpoint()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let mut transport_token = token.clone();
        let (stream, owner_tunnel) = cc_desk_remote_transport::start_admitted_registration(
            socket,
            frozen.tunnel,
            Duration::from_secs(30),
            end.into(),
            async move { transport_token.cancelled().await },
        )
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        tunnel = Some(owner_tunnel);
        let local = LocalIdentity::new(Role::Host, frozen.endpoint()?, keys.endpoint)
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let pending = cc_desk_remote_security::registration_channel::registration_accept_until(
            Unbound::new(stream),
            local,
            PeerPin::new(Role::Client, frozen.tls_key.clone()),
            setup_end.into(),
            end.into(),
            token.clone(),
        )
        .await
        .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let channel = pending
            .into_channel()
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        let mut session =
            NativeRegistrationSession::new(owner, channel, &binding, &caller, &admission)?;
        *attempt.reply.lock() = RegistrationReply::Verifying;
        session.verify(&binding, &caller, &admission).await?;
        let mut request = frozen.installation(
            &session.verified_binding(&binding, &caller, &admission)?,
            &admission,
        )?;
        if frozen.remember() {
            let store = match &mut prompt {
                BrowserPrompt::Native(native) => {
                    let (app, window) = native.as_ref();
                    let (store, target) = crate::remote_pairing_product::remember_target()
                        .map_err(|e| {
                            *attempt.reply.lock() = RegistrationReply::NativeStoreFailed;
                            crate::remote_pairing_product::store_error(e)
                        })?;
                    session.request_native_persistence(
                        &binding, &caller, &admission, request.0, target, app, window,
                    )?;
                    PersistenceStore::Native(store)
                }
                #[cfg(test)]
                BrowserPrompt::Test(_, persistence) => {
                    let p = persistence
                        .take()
                        .ok_or_else(|| error("REMOTE_NATIVE_STORE_FAILED"))?;
                    let target = crate::remote_pairing_product::persistence_target(&p.store)
                        .map_err(crate::remote_pairing_product::store_error)?;
                    let a = session.test_request_native_persistence(
                        &binding, &caller, &admission, request.0, target,
                    )?;
                    if let Some(yes) = p.yes {
                        a.test_complete(yes);
                    }
                    PersistenceStore::Test(p.store)
                }
            };
            *attempt.reply.lock() = RegistrationReply::PendingNativePersistence;
            loop {
                check()?;
                match attempt.commit.commit(|| {
                    session.persist_native(&binding, &caller, &admission, |m, a| store.apply(m, a))
                }) {
                    Ok(Some((host, _))) => {
                        attempt.remembered.store(true, Ordering::Release);
                        request.0 = host;
                        break;
                    }
                    Ok(None) if session.native_persistence_pending() => {}
                    Ok(None) => {
                        *attempt.reply.lock() = RegistrationReply::NativeStoreDeclined;
                        return Err(error("REMOTE_NATIVE_STORE_DECLINED"));
                    }
                    Err(e) => {
                        *attempt.reply.lock() = if e.code == "REMOTE_NATIVE_STORE_COMMIT_UNKNOWN" {
                            RegistrationReply::NativeStoreUnknown
                        } else {
                            RegistrationReply::NativeStoreFailed
                        };
                        return Err(e);
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        match prompt {
            BrowserPrompt::Native(native) => {
                let (app, window) = *native;
                session.request_browser_installation(
                    &binding, &caller, &admission, request, &app, &window,
                )?;
            }
            #[cfg(test)]
            BrowserPrompt::Test(yes, _) => {
                let approval = session.test_request_browser_installation(
                    &binding, &caller, &admission, request.0, request.1,
                )?;
                approval.test_complete(yes);
            }
        }
        *attempt.reply.lock() = RegistrationReply::PendingBrowserApproval;
        let mut installed = loop {
            attempt.identity.check(&binding, &caller, &admission)?;
            if Instant::now() >= end {
                return Err(error("REMOTE_HOST_LOCKED"));
            }
            match attempt
                .commit
                .commit(|| session.install_browser(&binding, &caller, &admission, &controller))
            {
                Ok(installed) => break installed,
                Err(_) if !attempt.stopped() && session.browser_approval_pending() => {}
                Err(e) => return Err(e),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let receipt=match installed.outcome(){cc_desk_remote_security::registration_channel::RegistrationOutcome::BrowserInstallationApproved{receipt,..}=>receipt.clone(),_=>return Err(error("REMOTE_HOST_LOCKED"))};
        *attempt.reply.lock() = RegistrationReply::DeliveryUnconfirmed {
            receipt: receipt.clone(),
            remembered: attempt.remembered.load(Ordering::Acquire),
        };
        installed
            .deliver()
            .await
            .map_err(|_| error("REMOTE_HOST_LOCKED"))?;
        *attempt.reply.lock() = RegistrationReply::InstalledLocked {
            receipt,
            remembered: attempt.remembered.load(Ordering::Acquire),
        };
        Ok(())
    };
    let result = {
        let work = checked(work);
        tokio::pin!(work);
        loop {
            tokio::select! {biased;
                result=&mut work=>break result,
                _=tokio::time::sleep(Duration::from_millis(20))=>{
                    if attempt.stopped()||attempt.identity.check(&binding,&caller,&admission).is_err(){break Err(error("REMOTE_HOST_LOCKED"));}
                }
            }
        }
    };
    attempt.channel_cancel.cancel();
    attempt.approval.cancel();
    drop(original_reservation.take());
    let mut known = !matches!(&result,Err(e) if e.code == "REMOTE_HOST_UNKNOWN");
    if !known {
        attempt.unknown();
    }
    if let Some(tunnel) = tunnel {
        tunnel.request_close();
        known &= tunnel.wait_closed().await.is_ok();
        if !known {
            attempt.unknown();
        }
    }
    loop {
        let observed = attempt.operation.retire();
        if observed.unknown {
            known = false;
            attempt.unknown();
        }
        if !observed.outstanding {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if result.is_err() {
        let mut state = attempt.reply.lock();
        settle_failure(&mut state, attempt.stopped(), known);
        if attempt.remembered.load(Ordering::Acquire)
            && matches!(
                *state,
                RegistrationReply::Failed
                    | RegistrationReply::Cancelled
                    | RegistrationReply::Unknown
            )
        {
            *state = RegistrationReply::NativeStoredRegistrationIncomplete;
        }
    }
    if !known {
        attempt.unknown();
    }
    known
}

#[cfg(test)]
pub(crate) struct TestPersistence {
    pub(crate) store: crate::remote_native_pairing_store::tests::TestStore,
    pub(crate) yes: Option<bool>,
}
enum PersistenceStore {
    Native(crate::remote_native_pairing_store::WindowsPairingStore),
    #[cfg(test)]
    Test(crate::remote_native_pairing_store::tests::TestStore),
}
impl PersistenceStore {
    fn apply(
        &self,
        mutation: &crate::remote_native_pairing_store::Mutation,
        authorization: &mut dyn crate::remote_native_pairing_store::MutationAuthorization,
    ) -> Result<
        crate::remote_native_pairing_store::LockedSnapshot,
        crate::remote_native_pairing_store::StoreError,
    > {
        match self {
            Self::Native(store) => store.apply(mutation.clone(), authorization),
            #[cfg(test)]
            Self::Test(store) => store.apply(mutation.clone(), authorization),
        }
    }
}
