//! The desktop's authority for a catalogue connection is separate from PTY sharing.
use super::*;
use crate::cli::{
    native_projection::{catalogue_history_source::RetainedHistory, wire::ScopeTarget},
    native_runtime::ManagementPortfolio,
    types::WireU64,
};
use crate::remote_host_approval::native_observer_enrollment::store::DeviceSummary;
use cc_desk_remote_security::{
    catalogue_read_channel::{self, CataloguePair, ReadError},
    catalogue_rendezvous::{CatalogueOffer, CatalogueReady, CatalogueSelection},
    session_catalogue::{DisplayRow, ListMode},
    session_management_channel::{
        verify_offer as verify_management_offer, ManagementOffer, ManagementRoute,
        VerifiedManagementRoute,
    },
    P256Spki, Role,
};
use cc_desk_remote_transport::wss::{
    HostShareCommand as ShareCommand, HostShareResponse as ShareResponse, ShareSelectionKey,
};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use tauri::{Emitter, EventTarget};

#[allow(dead_code)] // Only the desktop owner may hand this to a dedicated read route.
pub(super) struct CatalogueOwner {
    owner: Arc<Owner>,
    desktop_window: Window,
    device: DeviceSummary,
    pair: CataloguePair,
    purpose: RoutePurpose,
    grant_id: uuid::Uuid,
    retained: RouteSource,
    end: Instant,
    epoch: AtomicU64,
    reading: tokio::sync::Mutex<()>,
}

enum RouteSource {
    Read(Arc<RetainedHistory>),
    Manage(Arc<ManagementPortfolio>),
}

#[derive(Clone, Copy)]
enum RoutePurpose {
    Read(ListMode),
    // This selector comes from the desktop's protected management reference
    // table, never from a renderer or phone-provided native selector.
    Manage,
}

const MANAGEMENT_WINDOW: Duration = Duration::from_secs(300);
const MIN_OFFER_BUDGET: Duration = Duration::from_secs(10);

fn management_can_rotate(
    now: Instant,
    end: Instant,
    stopped: bool,
    previous_retired: bool,
) -> bool {
    !stopped && previous_retired && end.saturating_duration_since(now) >= MIN_OFFER_BUDGET
}

fn next_management_epochs(epoch: &AtomicU64) -> Option<[WireU64; 3]> {
    let first = epoch
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(3))
        .ok()?;
    Some([
        WireU64::parse(&(first + 1).to_string()).ok()?,
        WireU64::parse(&(first + 2).to_string()).ok()?,
        WireU64::parse(&(first + 3).to_string()).ok()?,
    ])
}

struct ReadySelection {
    wire: String,
    management: Option<VerifiedManagementRoute>,
}

fn target_matches(device: &DeviceSummary, target: &DeviceTarget) -> bool {
    device.state == DeviceState::Confirmed
        && target.tunnel == hex(&device.tunnel)
        && target.phone_id == hex(&device.phone_id)
        && target.phone_endpoint_spki == hex(&device.phone_endpoint_spki)
        && target.phone_relay_public_key_sec1 == hex(&device.phone_relay_public_key_sec1)
        && target.endpoint_binding == hex(&device.endpoint_binding)
}

impl Owner {
    #[allow(dead_code)]
    async fn approve_paired_route(
        self: &Arc<Self>,
        target: DeviceTarget,
        scope: Option<ScopeTarget>,
        purpose: RoutePurpose,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Option<Arc<CatalogueOwner>>, SafeError> {
        self.check()?;
        let project_count = match (&scope, purpose) {
            (Some(ScopeTarget::Profile { project_ids, .. }), RoutePurpose::Read(_))
                if !project_ids.is_empty() =>
            {
                project_ids.len()
            }
            (None, RoutePurpose::Manage) => 0,
            _ => return Err(locked()),
        };

        let (_, relay_pairs) = self.listing().await?;
        let store = self.store.clone();
        let device = tokio::task::spawn_blocking(move || {
            store
                .list_locked()
                .map_err(|_| locked())?
                .into_iter()
                .find(|row| target_matches(row, &target))
                .ok_or_else(locked)
        })
        .await
        .map_err(|_| uncertain())??;
        let relay_pair = RelayPair {
            request_id: device.request_id,
            tunnel: device.tunnel,
            phone_id: device.phone_id,
            phone_relay_public_key_sec1: device.phone_relay_public_key_sec1,
            endpoint_binding: device.endpoint_binding,
        };
        if !relay_pairs.iter().any(|p| party_matches(p, &relay_pair)) {
            return Err(locked());
        }
        let pair = CataloguePair {
            host: P256Spki::new(&self.profile.endpoint_spki())
                .map_err(|_| locked())?
                .endpoint_id(Role::Host),
            client: P256Spki::new(&device.phone_endpoint_spki)
                .map_err(|_| locked())?
                .endpoint_id(Role::Client),
        };
        self.check()?;
        let device_id = hex(&device.phone_id);
        let suffix = &device_id[device_id.len() - 6..];
        let (caption, text) = match purpose {
            RoutePurpose::Read(mode) => {
                let list_name = match mode {
                    ListMode::Sessions => "unarchived sessions",
                    ListMode::Archived => "archived sessions",
                };
                ("Allow read-only session listing", format!(
                    "Allow this phone to list {list_name} in {} selected projects?\n\nDevice: {suffix}\n\nThis does not allow terminal input or session changes.",
                    project_count,
                ))
            }
            RoutePurpose::Manage => ("Allow session management", format!(
                "Allow this paired phone to manage eligible Desk sessions from your visible local projects across Codex, Claude Code, and OpenCode for up to five minutes?\n\nDevice: {suffix}\n\nWithin that window the phone can make multiple requests and see the live view of an eligible running session. You can stop it any time. Terminal input is not allowed."
            )),
        };
        let approval = ApprovalMachine::show(
            app,
            window,
            (),
            text,
            caption,
            self.deadline.min(Instant::now() + Duration::from_secs(30)),
        );
        while approval.waiting() {
            self.check()?;
            if let Some(permit) = approval.take() {
                let Some(approval_end) = permit.consume(|(), end| end) else {
                    return Ok(None);
                };
                let end = if matches!(purpose, RoutePurpose::Manage) {
                    self.deadline.min(Instant::now() + MANAGEMENT_WINDOW)
                } else {
                    approval_end
                };
                let owner = self.clone();
                let desktop_window = window.clone();
                let granted = tokio::task::spawn_blocking(move || {
                    owner.check_catalogue_device(&device, end)?;
                    let retained = match scope {
                        Some(scope) => RouteSource::Read(Arc::new(
                            owner
                                .runtime
                                .retain_owner_catalogue(&owner.caller, &scope)?,
                        )),
                        None => RouteSource::Manage(Arc::new(
                            owner
                                .runtime
                                .retain_owner_management_portfolio(&owner.caller, || {
                                    owner.check_catalogue_device(&device, end)
                                })?,
                        )),
                    };
                    owner.check_catalogue_device(&device, end)?;
                    Ok::<_, SafeError>(Arc::new(CatalogueOwner {
                        owner,
                        desktop_window,
                        device,
                        pair,
                        purpose,
                        grant_id: uuid::Uuid::new_v4(),
                        retained,
                        end,
                        epoch: AtomicU64::new(0),
                        reading: tokio::sync::Mutex::new(()),
                    }))
                })
                .await
                .map_err(|_| uncertain())??;
                return Ok(Some(granted));
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        Ok(None)
    }

    fn check_catalogue_device(
        &self,
        expected: &DeviceSummary,
        end: Instant,
    ) -> Result<(), SafeError> {
        self.check()?;
        if Instant::now() >= end
            || !self
                .store
                .list_locked()
                .map_err(|_| locked())?
                .iter()
                .any(|device| device == expected)
        {
            return Err(locked());
        }
        self.check()
    }
}

impl Owner {
    /// Separate one-shot read authority. It shares the existing exclusive
    /// route slot, never turns an active-terminal grant into catalogue access.
    pub(crate) async fn start_catalogue_read(
        self: &Arc<Self>,
        target: DeviceTarget,
        scope: ScopeTarget,
        mode: ListMode,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        self.start_paired_route(target, Some(scope), RoutePurpose::Read(mode), app, window)
            .await
    }

    /// Backend-only entry once management has supplied an opaque reference.
    /// Existing listing and terminal consent cannot reach this path.
    #[allow(dead_code)]
    pub(crate) async fn start_management_rename(
        self: &Arc<Self>,
        target: DeviceTarget,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        self.start_paired_route(target, None, RoutePurpose::Manage, app, window)
            .await
    }

    async fn start_paired_route(
        self: &Arc<Self>,
        target: DeviceTarget,
        scope: Option<ScopeTarget>,
        purpose: RoutePurpose,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        let mut cache = self
            .state
            .try_lock()
            .map_err(|_| error("REMOTE_PAIRING_BUSY"))?;
        if let Some(scope) = &scope {
            scope.validate()?;
        }
        let Some(owner) = self
            .approve_paired_route(target, scope, purpose, app, window)
            .await?
        else {
            return self.snapshot(&mut cache).await;
        };
        self.stop_share(None).await?;
        let offer = owner.sign_offer().await?;
        let tunnel = owner.device.tunnel;
        let (stop, cancel) = tokio::sync::watch::channel(false);
        let phase = Arc::new(AtomicU8::new(1));
        let retired = Arc::new(AtomicBool::new(false));
        let status = phase.clone();
        let retirement = retired.clone();
        let task = tokio::spawn(async move {
            let result = if matches!(purpose, RoutePurpose::Manage) {
                owner
                    .serve_management_window(offer, cancel, &retirement)
                    .await
            } else {
                owner.publish_and_serve(offer, cancel, &retirement).await
            };
            status.store(if result.is_ok() { 0 } else { 3 }, Ordering::Release);
            result
        });
        *self.sharing.lock().await = Some(super::shares_owner::Task::new(
            tunnel, phase, stop, task, retired,
        ));
        self.snapshot(&mut cache).await
    }
}

#[allow(dead_code)]
pub(super) struct SignedCatalogueOffer {
    grant: uuid::Uuid,
    offer: CatalogueOffer,
    pub(super) digest: [u8; 32],
    pub(super) wire: String,
}

fn unix_millis() -> Result<u64, SafeError> {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| locked())?
            .as_millis(),
    )
    .map_err(|_| locked())
}

#[allow(dead_code)]
impl CatalogueOwner {
    /// A fresh signed invitation is issued only once each previous relay
    /// catalogue and endpoint are confirmed retired. Client writes are never
    /// replayed: each successor starts with a new inventory and references.
    async fn serve_management_window(
        self: &Arc<Self>,
        mut offer: SignedCatalogueOffer,
        mut stop: tokio::sync::watch::Receiver<bool>,
        retired: &AtomicBool,
    ) -> Result<(), SafeError> {
        let stopped = |stop: &tokio::sync::watch::Receiver<bool>| {
            *stop.borrow() || stop.has_changed().is_err()
        };
        loop {
            if stopped(&stop) {
                retired.store(true, Ordering::Release);
                return Ok(());
            }
            let previous_retired = AtomicBool::new(false);
            let result = self
                .publish_and_serve(offer, stop.clone(), &previous_retired)
                .await;
            let closed = previous_retired.load(Ordering::Acquire);
            if !closed {
                return Err(uncertain());
            }
            if !management_can_rotate(Instant::now(), self.end, stopped(&stop), closed) {
                retired.store(true, Ordering::Release);
                return if stopped(&stop) { Ok(()) } else { result };
            }
            // A peer transport loss cannot repeat its operation. Give it a
            // fresh offer only after its exact old route is proven retired.
            if result.is_err() {
                tokio::select! {
                    biased;
                    _ = stop.changed() => {},
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                };
            }
            if !management_can_rotate(Instant::now(), self.end, stopped(&stop), closed) {
                retired.store(true, Ordering::Release);
                return Ok(());
            }
            match self.sign_offer().await {
                Ok(next) => offer = next,
                Err(err) => {
                    retired.store(true, Ordering::Release);
                    return Err(err);
                }
            }
        }
    }

    async fn publish_and_serve(
        self: &Arc<Self>,
        offer: SignedCatalogueOffer,
        mut stop: tokio::sync::watch::Receiver<bool>,
        retired: &AtomicBool,
    ) -> Result<(), SafeError> {
        let mut attempted_publish = false;
        let endpoint_retired = AtomicBool::new(true);
        let result = async {
            let remaining = offer.offer.expires_unix_ms.saturating_sub(unix_millis()?);
            if *stop.borrow() || remaining < 10_000 {
                return Ok(());
            }
            self.verify_current().await?;
            if *stop.borrow() {
                return Ok(());
            }
            attempted_publish = true;
            let response = self
                .owner
                .share_wire(ShareCommand::Publish {
                    tunnel: self.device.tunnel,
                    ttl_seconds: remaining.div_ceil(1000).min(60) as u16,
                    catalog_envelope: offer.wire.clone(),
                })
                .await?;
            if !matches!(response, ShareResponse::Published {ref tunnel,ref catalog_digest,..}
                if tunnel == &hex(&self.device.tunnel) && catalog_digest == &hex(&offer.digest))
            {
                return Err(uncertain());
            }
            loop {
                if *stop.borrow()
                    || Instant::now() >= self.end
                    || offer.offer.expires_unix_ms.saturating_sub(unix_millis()?) < 10_000
                {
                    return Ok(());
                }
                self.verify_current().await?;
                match self
                    .owner
                    .share_wire(ShareCommand::ReadSelection {
                        tunnel: self.device.tunnel,
                    })
                    .await?
                {
                    ShareResponse::Selection {
                        tunnel,
                        catalog_digest,
                        scope_offer_id,
                        selection_id,
                    } => {
                        let key = ShareSelectionKey {
                            tunnel: decode(&tunnel).map_err(|_| locked())?,
                            catalog_digest: decode(&catalog_digest).map_err(|_| locked())?,
                            scope_offer_id: decode(&scope_offer_id).map_err(|_| locked())?,
                            selection_id: decode(&selection_id).map_err(|_| locked())?,
                        };
                        let ready = self.sign_ready(&offer, &key).await?;
                        return self
                            .serve_selection(key, ready, stop.clone(), &endpoint_retired)
                            .await;
                    }
                    ShareResponse::SelectionEmpty { .. } => {}
                    _ => return Err(uncertain()),
                }
                tokio::select! {
                    _ = stop.changed() => {},
                    _ = tokio::time::sleep(Duration::from_millis(750)) => {},
                }
            }
        }
        .await;
        if !attempted_publish {
            retired.store(true, Ordering::Release);
            return result;
        }
        // Clear this exact digest, including an uncertain publish. It cannot
        // clear another grant; the relay also expires it with a bounded TTL.
        let cleaned = self
            .owner
            .share_wire(ShareCommand::Clear {
                tunnel: self.device.tunnel,
                catalog_digest: offer.digest,
            })
            .await;
        if matches!(cleaned, Ok(ShareResponse::Cleared {ref tunnel,ref catalog_digest})
            if tunnel == &hex(&self.device.tunnel) && catalog_digest == &hex(&offer.digest))
        {
            retired.store(endpoint_retired.load(Ordering::Acquire), Ordering::Release);
            result
        } else {
            Err(uncertain())
        }
    }

    async fn serve_selection(
        self: &Arc<Self>,
        selection: ShareSelectionKey,
        ready: ReadySelection,
        mut stop: tokio::sync::watch::Receiver<bool>,
        endpoint_retired: &AtomicBool,
    ) -> Result<(), SafeError> {
        use crate::remote_host_approval::native_observer_enrollment::control::ControlFailure;
        use cc_desk_remote_security as security;
        use cc_desk_remote_transport as transport;

        let settings = self
            .owner
            .profile
            .operation(&self.owner.lifetime, self.end)
            .map_err(|_| locked())?;
        let address = self.owner.address;
        let end = self.end;
        let exact_owner = self.clone();
        let ReadySelection {
            wire: envelope,
            management,
        } = ready;
        let (cancel, token) = security::cancellation();
        let (armed, wait_armed) = tokio::sync::oneshot::channel();
        let serving = control::on_worker(
            settings,
            move |exec, custody, mut keys, ready, expected, signing| {
                ready
                    .consume(custody, expected)
                    .map_err(|_| ControlFailure::Locked)?;
                let start = Instant::now();
                let connect_end = end.min(start + Duration::from_secs(10));
                let endpoint = security::P256Spki::new(&expected.endpoint_spki)
                    .map_err(|_| ControlFailure::Locked)?;
                let identity =
                    security::LocalIdentity::new(security::Role::Host, endpoint, keys.endpoint)
                        .map_err(|_| ControlFailure::Locked)?;
                keys.stages
                    .begin_registration(signing, connect_end)
                    .map_err(|_| ControlFailure::Locked)?;
                armed.send(()).map_err(|_| ControlFailure::Locked)?;
                exec.block_on(async move {
                let mut observe = token.clone();
                let socket = tokio::select! {
                    biased;
                    _ = observe.cancelled() => return Ok(()),
                    result = transport::wss::connect_admitted_with_mode(expected.origin.as_str(),address,
                        connect_end.saturating_duration_since(Instant::now()), &keys.relay,
                        transport::wss::RelayMode::RendezvousV1, selection.tunnel) =>
                        result.map_err(|_|ControlFailure::Locked)?,
                };
                keys.stages.arm_endpoint().map_err(|_|ControlFailure::Locked)?;
                let (stream, mux) = transport::start_admitted(socket, selection.tunnel, Duration::from_secs(5))
                    .map_err(|_|ControlFailure::Locked)?;
                let served = match management {
                    Some(proof) => exact_owner.serve_rename(security::Unbound::new(stream), identity, proof, token).await.map_err(|_| ()),
                    None => exact_owner.serve_list(security::Unbound::new(stream), identity, token).await.map_err(|_| ()),
                };
                mux.request_close();
                mux.wait_closed().await.map_err(|_|ControlFailure::RetirementUnknown)?;
                if observe.is_cancelled() { Ok(()) } else { served.map_err(|_| ControlFailure::Locked) }
            })
            },
        );
        let (done, mut finished) = tokio::sync::oneshot::channel();
        let completion = async {
            endpoint_retired.store(false, Ordering::Release);
            let result = serving.await;
            if !matches!(&result, Err(ControlFailure::RetirementUnknown)) {
                endpoint_retired.store(true, Ordering::Release);
            }
            let _ = done.send(());
            result.map_err(|_| uncertain())
        };
        let answer = async {
            let outcome = async {
                if *stop.borrow() {
                    return Ok(());
                }
                tokio::select! {
                    biased;
                    _ = stop.changed() => return Ok(()),
                    ready = wait_armed => ready.map_err(|_|uncertain())?,
                }
                self.verify_current().await?;
                if *stop.borrow() {
                    return Ok(());
                }
                let answer = self
                    .owner
                    .share_wire(ShareCommand::Answer {
                        selection,
                        ready_envelope: envelope,
                    })
                    .await?;
                if !matches!(answer, ShareResponse::Answered { .. }) {
                    return Err(uncertain());
                }
                tokio::select! { biased; _=stop.changed()=>{}, _=&mut finished=>{} }
                Ok(())
            }
            .await;
            cancel.cancel();
            outcome
        };
        let (outcome, result) = tokio::join!(answer, completion);
        result?;
        outcome
    }

    async fn verify_current(self: &Arc<Self>) -> Result<(), SafeError> {
        let owner = self.clone();
        tokio::task::spawn_blocking(move || owner.check_current())
            .await
            .map_err(|_| uncertain())?
    }

    async fn sign_packet(self: &Arc<Self>, message: Vec<u8>) -> Result<[u8; 64], SafeError> {
        use super::control::ControlFailure;
        use crate::native_crypto::{
            owner::{SignatureEncoding, SigningFence},
            SigningPurpose,
        };
        self.verify_current().await?;
        let settings = self
            .owner
            .profile
            .operation(&self.owner.lifetime, self.end)
            .map_err(|_| locked())?;
        let public = self.owner.profile.endpoint_spki();
        let root = self.owner.lifetime.clone();
        let cap = self.end;
        let signature =
            control::on_worker(settings, move |exec, custody, _, ready, expected, _| {
                let key = custody
                    .observer_endpoint_handle(ready, expected)
                    .map_err(|_| ControlFailure::Locked)?;
                if key.binding() != (SigningPurpose::Endpoint, public.as_slice()) {
                    return Err(ControlFailure::Locked);
                }
                let fence = SigningFence::new_revocable(cap, root);
                let receipt = fence
                    .advance(
                        std::slice::from_ref(&key),
                        cap.min(Instant::now() + Duration::from_secs(5)),
                    )
                    .map_err(|_| ControlFailure::Locked)?;
                let operation = key
                    .begin_attempt(message.clone(), SignatureEncoding::Fixed, receipt)
                    .map_err(|_| ControlFailure::Locked)?;
                let sig = exec
                    .block_on(operation)
                    .map_err(|_| ControlFailure::Locked)?;
                super::super::verify_p256(&public, &message, &sig)
                    .map_err(|_| ControlFailure::Locked)?;
                sig.as_slice()
                    .try_into()
                    .map_err(|_| ControlFailure::Locked)
            })
            .await
            .map_err(|_| locked())?;
        self.verify_current().await?;
        Ok(signature)
    }

    pub(super) async fn sign_offer(self: &Arc<Self>) -> Result<SignedCatalogueOffer, SafeError> {
        use sha2::{Digest, Sha256};
        let remaining = self
            .end
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(60_000) as u64;
        if remaining < 1000 {
            return Err(locked());
        }
        let offer = CatalogueOffer {
            tunnel: self.device.tunnel,
            endpoint_binding: self.device.endpoint_binding,
            offer: *uuid::Uuid::new_v4().as_bytes(),
            expires_unix_ms: unix_millis()?.checked_add(remaining).ok_or_else(locked)?,
        };
        let wire = match self.purpose {
            RoutePurpose::Read(mode) => {
                offer.envelope(self.sign_packet(offer.signing_message_for(mode)).await?)
            }
            RoutePurpose::Manage => {
                let management = self.management_offer(&offer);
                management.envelope(
                    offer.expires_unix_ms,
                    self.sign_packet(management.signing_bytes(offer.expires_unix_ms))
                        .await?,
                )
            }
        };
        if offer.expires_unix_ms <= unix_millis()? {
            return Err(locked());
        }
        let digest: [u8; 32] = Sha256::digest(wire.as_bytes()).into();
        Ok(SignedCatalogueOffer {
            grant: self.grant_id,
            offer,
            digest,
            wire,
        })
    }

    fn management_offer(&self, offer: &CatalogueOffer) -> ManagementOffer {
        ManagementOffer {
            tunnel: offer.tunnel,
            membership_binding: self.device.endpoint_binding,
            offer: offer.offer,
            host: self.pair.host,
            phone: self.pair.client,
        }
    }

    async fn sign_ready(
        self: &Arc<Self>,
        offer: &SignedCatalogueOffer,
        selected: &cc_desk_remote_transport::wss::ShareSelectionKey,
    ) -> Result<ReadySelection, SafeError> {
        if offer.grant != self.grant_id
            || offer.offer.expires_unix_ms <= unix_millis()?
            || selected.tunnel != self.device.tunnel
            || selected.tunnel != offer.offer.tunnel
            || selected.catalog_digest != offer.digest
            || selected.scope_offer_id != offer.offer.offer
            || selected.selection_id == [0; 16]
        {
            return Err(locked());
        }
        if matches!(self.purpose, RoutePurpose::Manage) {
            let expiry = offer.offer.expires_unix_ms;
            let management = self.management_offer(&offer.offer);
            let exact = management.selection(offer.digest, selected.selection_id);
            let wire = ManagementRoute::envelope(
                expiry,
                self.sign_packet(exact.signing_bytes(expiry)).await?,
            );
            let host = P256Spki::new(&self.owner.profile.endpoint_spki()).map_err(|_| locked())?;
            let now = unix_millis()?;
            let proof = verify_management_offer(
                &host,
                management.tunnel,
                management.membership_binding,
                management.phone,
                offer.digest,
                &offer.wire,
                now,
            )
            .and_then(|advert| advert.verify_selection(selected.selection_id, &wire, now))
            .map_err(|_| locked())?;
            return Ok(ReadySelection {
                wire,
                management: Some(proof),
            });
        }
        let ready = CatalogueReady {
            selection: CatalogueSelection {
                tunnel: selected.tunnel,
                endpoint_binding: self.device.endpoint_binding,
                catalog_digest: selected.catalog_digest,
                offer: selected.scope_offer_id,
                selection: selected.selection_id,
            },
            expires_unix_ms: offer.offer.expires_unix_ms,
        };
        let wire = ready.envelope(self.sign_packet(ready.signing_message()).await?);
        if ready.expires_unix_ms <= unix_millis()? {
            return Err(locked());
        }
        Ok(ReadySelection {
            wire,
            management: None,
        })
    }

    async fn serve_rename<S>(
        self: Arc<Self>,
        stream: cc_desk_remote_security::Unbound<S>,
        identity: cc_desk_remote_security::LocalIdentity,
        route: VerifiedManagementRoute,
        cancel: cc_desk_remote_security::CancelToken,
    ) -> Result<(), cc_desk_remote_security::session_management_channel::ChannelError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        use crate::cli::native_runtime::ManagementRenameAdmission;
        use cc_desk_remote_security::session_management_channel::ChannelError::Denied;
        let RouteSource::Manage(portfolio) = &self.retained else {
            return Err(Denied);
        };
        let _one = self.reading.try_lock().map_err(|_| Denied)?;
        let [inventory_epoch, prepare_epoch, commit_epoch] =
            next_management_epochs(&self.epoch).ok_or(Denied)?;
        let phone = P256Spki::new(&self.device.phone_endpoint_spki).map_err(|_| Denied)?;
        let current = self.clone();
        let window = self.desktop_window.clone();
        self.owner
            .runtime
            .serve_management_rename(
                stream,
                identity,
                cc_desk_remote_security::PeerPin::new(Role::Client, phone),
                route,
                cancel,
                ManagementRenameAdmission {
                    caller: self.owner.caller.clone(),
                    portfolio: portfolio.clone(),
                    inventory_epoch,
                    prepare_epoch,
                    commit_epoch,
                    claude_archive_changed: Arc::new(move || {
                        // 不发送会话或路径；桌面通过原有只读命令重新加载。
                        let _ = window.emit_to(
                            EventTarget::webview_window(window.label()),
                            "projects-state-changed",
                            (),
                        );
                    }),
                    current_peer: Arc::new(move || {
                        current.check_current()?;
                        Ok((current.pair.client, current.device.endpoint_binding))
                    }),
                },
            )
            .await
    }

    pub(super) fn pair(&self) -> CataloguePair {
        self.pair
    }

    // Membership is an exact receipt, including the independent endpoint and grant keys.
    // Never authorize a new pairing merely because the display ID stayed the same.
    fn check_current(&self) -> Result<(), SafeError> {
        self.owner.check_catalogue_device(&self.device, self.end)?;
        if let RouteSource::Manage(portfolio) = &self.retained {
            self.owner
                .runtime
                .check_owner_management_portfolio(&self.owner.caller, portfolio)?;
        }
        Ok(())
    }

    /// Takes a stream from an already admitted catalogue-specific route.
    /// Its cancellation token must retire with that route.
    pub(super) async fn serve_list<S>(
        self: Arc<Self>,
        io: cc_desk_remote_security::Unbound<S>,
        identity: cc_desk_remote_security::LocalIdentity,
        cancel: cc_desk_remote_security::CancelToken,
    ) -> Result<(), ReadError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let _one = self.reading.try_lock().map_err(|_| ReadError::Denied)?;
        let phone =
            P256Spki::new(&self.device.phone_endpoint_spki).map_err(|_| ReadError::Denied)?;
        let pin = cc_desk_remote_security::PeerPin::new(Role::Client, phone);
        let audit = self.clone();
        let source = self.clone();
        catalogue_read_channel::serve(
            io,
            identity,
            pin,
            self.pair,
            cancel,
            move |pair| {
                let owner = audit.clone();
                async move {
                    if pair != owner.pair {
                        return false;
                    }
                    tokio::task::spawn_blocking(move || owner.check_current().is_ok())
                        .await
                        .unwrap_or(false)
                }
            },
            move |mode| source.list(mode),
        )
        .await
    }

    async fn list(self: Arc<Self>, mode: ListMode) -> Result<Vec<DisplayRow>, ReadError> {
        if !matches!(self.purpose, RoutePurpose::Read(allowed) if allowed == mode) {
            return Err(ReadError::Denied);
        }
        tokio::task::spawn_blocking(move || {
            self.check_current().map_err(|_| ReadError::Denied)?;
            let epoch = self
                .epoch
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
                .map_err(|_| ReadError::Denied)?
                + 1;
            let epoch = WireU64::parse(&epoch.to_string()).map_err(|_| ReadError::Denied)?;
            let RouteSource::Read(retained) = &self.retained else {
                return Err(ReadError::Denied);
            };
            self.owner
                .runtime
                .read_owner_catalogue(&self.owner.caller, retained, epoch, mode, || {
                    self.check_current()
                })
                .map_err(|_| ReadError::Denied)
        })
        .await
        .map_err(|_| ReadError::Denied)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn management_window_rejects_unsafe_rotation_and_preserves_monotonic_epochs() {
        let clock = Instant::now();
        let end = clock + Duration::from_secs(30);
        assert!(management_can_rotate(clock, end, false, true));
        assert!(!management_can_rotate(clock, end, true, true));
        assert!(!management_can_rotate(clock, end, false, false));
        assert!(!management_can_rotate(
            clock + Duration::from_secs(21),
            end,
            false,
            true
        ));
        let epoch = AtomicU64::new(0);
        assert_eq!(
            next_management_epochs(&epoch).unwrap().map(|i| i.get()),
            [1, 2, 3]
        );
        assert_eq!(
            next_management_epochs(&epoch).unwrap().map(|i| i.get()),
            [4, 5, 6]
        );
        let exhausted = AtomicU64::new(u64::MAX - 2);
        assert!(next_management_epochs(&exhausted).is_none());
        assert_eq!(exhausted.load(Ordering::Acquire), u64::MAX - 2);
    }

    #[test]
    fn catalogue_target_requires_confirmed_exact_receipt_fields() {
        let mut device = DeviceSummary {
            request_id: [1; 16],
            phone_relay_public_key_sec1: [2; 65],
            phone_endpoint_spki: [3; 91],
            phone_grant_spki: [4; 91],
            tunnel: [5; 16],
            phone_id: [6; 16],
            endpoint_binding: [7; 32],
            state: DeviceState::Confirmed,
        };
        let mut target = DeviceTarget {
            tunnel: hex(&device.tunnel),
            phone_id: hex(&device.phone_id),
            phone_relay_public_key_sec1: hex(&device.phone_relay_public_key_sec1),
            phone_endpoint_spki: hex(&device.phone_endpoint_spki),
            endpoint_binding: hex(&device.endpoint_binding),
        };
        assert!(target_matches(&device, &target));
        device.state = DeviceState::Pending;
        assert!(!target_matches(&device, &target));
        device.state = DeviceState::Revoked;
        assert!(!target_matches(&device, &target));
        device.state = DeviceState::Confirmed;
        target.phone_endpoint_spki = hex(&[8; 91]);
        assert!(!target_matches(&device, &target));
        target.phone_endpoint_spki = hex(&device.phone_endpoint_spki);
        target.endpoint_binding = hex(&[8; 32]);
        assert!(!target_matches(&device, &target));
    }
}
