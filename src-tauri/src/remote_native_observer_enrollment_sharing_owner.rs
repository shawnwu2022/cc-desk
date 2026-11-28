use super::*;
use crate::remote_host_approval::native_observer_enrollment::{data, shares, store::DeviceSummary};
use cc_desk_remote_transport::wss::{
    HostShareCommand as ShareCommand, HostShareResponse as ShareResponse, ShareSelectionKey,
};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
pub(super) struct PendingCatalogueClear(parking_lot::Mutex<Option<[u8; 32]>>);

impl PendingCatalogueClear {
    pub(super) fn remember(&self, digest: [u8; 32]) {
        *self.0.lock() = Some(digest);
    }

    async fn confirm<F: std::future::Future<Output = bool>>(
        &self,
        clear: impl FnOnce([u8; 32]) -> F,
    ) -> bool {
        let digest = *self.0.lock();
        let Some(digest) = digest else {
            return false;
        };
        if !clear(digest).await {
            return false;
        }
        let mut pending = self.0.lock();
        if *pending != Some(digest) {
            return false;
        }
        *pending = None;
        true
    }
}

pub(super) struct Task {
    pub(super) tunnel: [u8; 16],
    phase: Arc<AtomicU8>,
    stop: tokio::sync::watch::Sender<bool>,
    join: tokio::task::JoinHandle<Result<(), SafeError>>,
    result: Option<Result<(), SafeError>>,
    retired: Arc<std::sync::atomic::AtomicBool>,
    catalogue_lifetime: Option<Arc<NativeRevocation>>,
    pending_clear: Option<Arc<PendingCatalogueClear>>,
}
impl Task {
    pub(super) fn new(
        tunnel: [u8; 16],
        phase: Arc<AtomicU8>,
        stop: tokio::sync::watch::Sender<bool>,
        join: tokio::task::JoinHandle<Result<(), SafeError>>,
        retired: Arc<std::sync::atomic::AtomicBool>,
        catalogue_lifetime: Arc<NativeRevocation>,
        pending_clear: Arc<PendingCatalogueClear>,
    ) -> Self {
        Self {
            tunnel,
            phase,
            stop,
            join,
            retired,
            result: None,
            catalogue_lifetime: Some(catalogue_lifetime),
            pending_clear: Some(pending_clear),
        }
    }

    pub(super) fn phase(&self) -> u8 {
        self.phase.load(Ordering::Acquire)
    }
    fn signal_stop(&self) {
        if let Some(lifetime) = &self.catalogue_lifetime {
            lifetime.revoke();
        }
        self.stop.send_replace(true);
    }

    async fn stop<F: std::future::Future<Output = bool>>(
        &mut self,
        clear: impl FnOnce([u8; 16], [u8; 32]) -> F,
    ) -> Result<(), SafeError> {
        self.signal_stop();
        if self.result.is_none() {
            let joined = tokio::time::timeout(Duration::from_secs(10), &mut self.join)
                .await
                .map_err(|_| uncertain())?;
            self.result = Some(match joined {
                Ok(result) => result,
                Err(_) => {
                    self.pending_clear = None;
                    self.retired.store(false, Ordering::Release);
                    Err(uncertain())
                }
            });
        }
        if self.retired.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(pending) = &self.pending_clear {
            let tunnel = self.tunnel;
            if pending.confirm(|digest| clear(tunnel, digest)).await {
                self.retired.store(true, Ordering::Release);
                self.phase.store(0, Ordering::Release);
                return Ok(());
            }
        }
        let result = self.result.clone().and_then(Result::err);
        Err(result.unwrap_or_else(uncertain))
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.signal_stop();
    }
}

#[cfg(test)]
mod task_tests {
    use super::*;
    use std::{cell::Cell, future::ready, sync::atomic::AtomicBool};

    #[tokio::test]
    async fn repeated_stop_retries_only_the_pending_joined_digest() {
        let tunnel = [2; 16];
        let digest = [7; 32];
        let pending = Arc::new(PendingCatalogueClear::default());
        let record = pending.clone();
        let lifetime = NativeRevocation::new();
        let (stop, mut received) = tokio::sync::watch::channel(false);
        let join = tokio::spawn(async move {
            received.changed().await.unwrap();
            record.remember(digest);
            Err(uncertain())
        });
        let mut task = Task::new(
            tunnel,
            Arc::new(AtomicU8::new(3)),
            stop,
            join,
            Arc::new(AtomicBool::new(false)),
            lifetime.clone(),
            pending,
        );
        let attempts = Cell::new(0);
        assert!(task
            .stop(|t, d| {
                assert!(lifetime.is_revoked());
                assert_eq!((t, d), (tunnel, digest));
                attempts.set(attempts.get() + 1);
                ready(false)
            })
            .await
            .is_err());
        assert!(task
            .stop(|t, d| {
                assert_eq!((t, d), (tunnel, digest));
                attempts.set(attempts.get() + 1);
                ready(true)
            })
            .await
            .is_ok());
        assert_eq!(task.phase(), 0);
        assert!(task
            .stop(|_, _| async { panic!("a confirmed clear must not repeat") })
            .await
            .is_ok());
        assert_eq!(attempts.get(), 2);
    }

    #[tokio::test]
    async fn neither_an_unjoined_endpoint_nor_join_failure_can_attempt_clear() {
        for panicked in [false, true] {
            let pending = Arc::new(PendingCatalogueClear::default());
            if panicked {
                pending.remember([7; 32]);
            }
            let (stop, _) = tokio::sync::watch::channel(false);
            let join = tokio::spawn(async move {
                assert!(!panicked, "simulate a failed task join");
                Err(uncertain())
            });
            let mut task = Task::new(
                [2; 16],
                Arc::new(AtomicU8::new(3)),
                stop,
                join,
                Arc::new(AtomicBool::new(false)),
                NativeRevocation::new(),
                pending,
            );
            for _ in 0..2 {
                assert!(task
                    .stop(|_, _| async { panic!("no proven joined digest") })
                    .await
                    .is_err());
            }
        }
    }
}

impl Owner {
    pub(crate) async fn start_sharing(
        self: &Arc<Self>,
        target: DeviceTarget,
        selections: Vec<shares::Target>,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        let mut cache = self
            .state
            .try_lock()
            .map_err(|_| error("REMOTE_PAIRING_BUSY"))?;
        let (_, pairs) = self.listing().await?;
        let store = self.store.clone();
        let devices = tokio::task::spawn_blocking(move || store.list_locked())
            .await
            .map_err(|_| uncertain())?
            .map_err(|_| locked())?;
        let phone = devices
            .into_iter()
            .find(|row| {
                row.state == DeviceState::Confirmed
                    && target.tunnel == hex(&row.tunnel)
                    && target.phone_id == hex(&row.phone_id)
                    && target.phone_endpoint_spki == hex(&row.phone_endpoint_spki)
                    && target.phone_relay_public_key_sec1 == hex(&row.phone_relay_public_key_sec1)
                    && target.endpoint_binding == hex(&row.endpoint_binding)
            })
            .ok_or_else(locked)?;
        let pair = RelayPair {
            request_id: phone.request_id,
            tunnel: phone.tunnel,
            phone_id: phone.phone_id,
            phone_relay_public_key_sec1: phone.phone_relay_public_key_sec1,
            endpoint_binding: phone.endpoint_binding,
        };
        if !pairs.iter().any(|p| party_matches(p, &pair)) {
            return Err(locked());
        }
        let approval = Arc::new(
            shares::Approval::show(
                app,
                window,
                self.profile.endpoint().clone(),
                pair,
                self.profile.endpoint_spki(),
                selections,
            )
            .map_err(|_| locked())?,
        );
        while approval.pending() && !approval.approved() {
            self.check()?;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if !approval.approved() {
            return self.snapshot(&mut cache).await;
        }
        self.stop_share(None).await?;
        let catalog = control::sign_catalog(self.control_setup()?, approval.clone())
            .await
            .map_err(|_| uncertain())?
            .ok_or_else(locked)?;
        self.checked_publish(&catalog).await?;
        let (stop, cancel) = tokio::sync::watch::channel(false);
        let phase = Arc::new(AtomicU8::new(1));
        let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let retirement = retired.clone();
        let owner = self.clone();
        let status = phase.clone();
        let tunnel = phone.tunnel;
        let join = tokio::spawn(async move {
            let result = owner
                .share_loop(
                    cancel,
                    approval,
                    Arc::new(catalog),
                    phone,
                    status.clone(),
                    retirement,
                )
                .await;
            status.store(if result.is_ok() { 0 } else { 3 }, Ordering::Release);
            result
        });
        *self.sharing.lock().await = Some(Task {
            tunnel,
            phase,
            stop,
            join,
            result: None,
            retired,
            catalogue_lifetime: None,
            pending_clear: None,
        });
        self.snapshot(&mut cache).await
    }
    pub(crate) async fn stop_sharing(&self) -> Result<Snapshot, SafeError> {
        self.check_document()?;
        let mut cache = self
            .state
            .try_lock()
            .map_err(|_| error("REMOTE_PAIRING_BUSY"))?;
        self.stop_share(None).await?;
        // Stopping an existing share does not cancel a separate invitation.
        // Refresh it only after the local share has retired. In an expired or
        // unavailable setup, do not present an unverifiable pending request.
        if cache.invite.is_some() && Instant::now() < self.deadline {
            if let Ok(snapshot) = self.snapshot(&mut cache).await {
                return Ok(snapshot);
            }
        }
        self.local_status().await
    }
    pub(super) async fn stop_share(&self, tunnel: Option<[u8; 16]>) -> Result<(), SafeError> {
        let mut slot = self.sharing.lock().await;
        if let Some(task) = &mut *slot {
            if tunnel.is_none_or(|id| id == task.tunnel) {
                let result = task
                    .stop(|tunnel, digest| async move {
                        self.clear_exact_catalogue(tunnel, digest).await
                            == super::catalogue_owner::ExactClear::Confirmed
                    })
                    .await;
                if result.is_err() {
                    return result;
                }
                *slot = None;
            }
        }
        Ok(())
    }
    pub(super) async fn share_wire(
        &self,
        command: ShareCommand,
    ) -> Result<ShareResponse, SafeError> {
        self.check()?;
        let result = control::share_request(self.control_setup()?, self.address, command).await;
        self.check()?;
        result.map_err(|_| uncertain())
    }
    async fn checked_publish(&self, catalog: &shares::Live) -> Result<(), SafeError> {
        catalog.check().map_err(|_| locked())?;
        match self
            .share_wire(ShareCommand::Publish {
                tunnel: catalog.tunnel(),
                ttl_seconds: 60,
                catalog_envelope: catalog.wire.clone(),
            })
            .await?
        {
            ShareResponse::Published {
                tunnel,
                catalog_digest,
                ..
            } if tunnel == hex(&catalog.tunnel()) && catalog_digest == hex(&catalog.digest) => {
                Ok(())
            }
            _ => Err(uncertain()),
        }
    }
    async fn checked_clear(&self, catalog: &shares::Live) -> Result<(), SafeError> {
        if Instant::now() >= catalog.deadline() + Duration::from_secs(20) {
            return Ok(());
        }
        match self
            .share_wire(ShareCommand::Clear {
                tunnel: catalog.tunnel(),
                catalog_digest: catalog.digest,
            })
            .await?
        {
            ShareResponse::Cleared {
                tunnel,
                catalog_digest,
            } if tunnel == hex(&catalog.tunnel()) && catalog_digest == hex(&catalog.digest) => {
                Ok(())
            }
            _ => Err(uncertain()),
        }
    }
    async fn share_loop(
        &self,
        mut stop: tokio::sync::watch::Receiver<bool>,
        _approval: Arc<shares::Approval>,
        mut catalog: Arc<shares::Live>,
        phone: DeviceSummary,
        phase: Arc<AtomicU8>,
        retired: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), SafeError> {
        let mut active: Option<data::Active> = None;
        let mut selection_started = false;
        let mut selection_controls = false;
        let mut published = true;
        let mut stale_selection = false;
        let result=async {
            loop {
                if *stop.borrow() || self.check().is_err(){break;}
                let rotate = stale_selection || active.as_ref().is_some_and(data::Active::is_finished)
                    || (active.is_none() && Instant::now()>=catalog.deadline());
                if rotate {
                    if let Some(mut old)=active.take(){old.retire().await.map_err(|_|uncertain())?;}
                    self.checked_clear(&catalog).await?;
                    published = false;
                    if Instant::now()+Duration::from_secs(6)>=catalog.authority_end{break;}
                    let renewed=control::resign_catalog(self.control_setup()?,catalog.clone()).await.map_err(|_|uncertain())?;
                    self.checked_publish(&renewed).await?;
                    published = true;
                    catalog=Arc::new(renewed); selection_started=false; selection_controls=false; stale_selection=false;
                    phase.store(1,Ordering::Release);
                }
                if active.is_none() && catalog.check().is_err(){break;}
                if !selection_started {
                    if let ShareResponse::Selection{tunnel,catalog_digest,scope_offer_id,selection_id}=self.share_wire(ShareCommand::ReadSelection{tunnel:catalog.tunnel()}).await? {
                        let selected=ShareSelectionKey{tunnel:decode(&tunnel).map_err(|_|locked())?,catalog_digest:decode(&catalog_digest).map_err(|_|locked())?,
                            scope_offer_id:decode(&scope_offer_id).map_err(|_|locked())?,selection_id:decode(&selection_id).map_err(|_|locked())?};
                        if catalog.deadline().saturating_duration_since(Instant::now())<Duration::from_secs(10){stale_selection=true;continue;}
                        let (source,role)=catalog.target(&selected).map_err(|_|locked())?;
                        selection_controls= role == cc_desk_remote_record::grant::DataRole::Controller;
                        let controller = if selection_controls {
                            Some(data::Controller { owner: self.host_service.native_controller_host(source)?, runtime: self.runtime.clone() })
                        } else {None};
                        let source=source.observer_admission_copy()?;
                        active=Some(data::start(self.profile.clone(),self.address,&self.lifetime,phone.clone(),source,controller,catalog.authority_end,catalog.deadline()).map_err(|_|locked())?);
                        active.as_mut().ok_or_else(locked)?.arm_receipt(catalog.deadline()).await.map_err(|_|uncertain())?;
                        if *stop.borrow(){break;}
                        let ready=control::sign_ready(self.control_setup()?,catalog.clone(),selected).await.map_err(|_|uncertain())?;
                        let answer=self.share_wire(ShareCommand::Answer{selection:selected,ready_envelope:ready}).await?;
                        if !matches!(answer,ShareResponse::Answered{..}){return Err(uncertain());}
                        selection_started=true;
                    }
                }
                if active.as_ref().is_some_and(data::Active::installed){phase.store(if selection_controls {4}else{2},Ordering::Release);}
                tokio::select!{_ = stop.changed()=>{}, _=tokio::time::sleep(Duration::from_millis(1200))=>{}}
            }
            Ok(())
        }.await;
        if let Some(mut current) = active {
            current.retire().await.map_err(|_| uncertain())?;
        }
        retired.store(true, Ordering::Release);
        if published {
            self.checked_clear(&catalog).await?;
        }
        result
    }
}
