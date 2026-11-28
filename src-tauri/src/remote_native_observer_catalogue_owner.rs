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
    ConnectError, EnrollmentError, HostShareCommand as ShareCommand,
    HostShareResponse as ShareResponse, ShareSelectionKey,
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
    window: CatalogueWindow,
    lifetime: Arc<NativeRevocation>,
    epoch: AtomicU64,
    reading: tokio::sync::Mutex<()>,
}

enum RouteSource {
    Read(Arc<RetainedHistory>),
    ReadAll(Arc<ManagementPortfolio>),
    Manage(Arc<ManagementPortfolio>),
}

#[derive(Clone, Copy)]
enum RoutePurpose {
    Read(ListMode),
    ReadAll,
    // This selector comes from the desktop's protected management reference
    // table, never from a renderer or phone-provided native selector.
    Manage,
}

const MANAGEMENT_WINDOW: Duration = Duration::from_secs(300);
const MIN_OFFER_BUDGET: Duration = Duration::from_secs(10);
const OFFER_TTL: Duration = Duration::from_secs(60);
const CONTROL_BUDGET: Duration = Duration::from_secs(8);
const CLEAR_RETRY: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExactClear {
    Confirmed,
    Retryable,
    Rejected,
}

async fn wait_for_exact_clear<F, Fut>(
    stop: &mut tokio::sync::watch::Receiver<bool>,
    may_retry: bool,
    retry_after: Duration,
    mut clear: F,
    mut waiting: impl FnMut(),
) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ExactClear>,
{
    loop {
        match clear().await {
            ExactClear::Confirmed => return true,
            ExactClear::Rejected => return false,
            ExactClear::Retryable
                if !may_retry || *stop.borrow() || stop.has_changed().is_err() =>
            {
                return false
            }
            ExactClear::Retryable => {}
        }
        waiting();
        tokio::select! {
            biased;
            _ = stop.changed() => return false,
            _ = tokio::time::sleep(retry_after) => {}
        }
        if *stop.borrow() || stop.has_changed().is_err() {
            return false;
        }
    }
}

#[derive(Clone, Copy)]
enum CatalogueWindow {
    // Explicit new consent only; never constructed from a stored pairing alone.
    WhileDesktopOpen,
    Until(Instant),
}
impl CatalogueWindow {
    fn cap(self, now: Instant, budget: Duration) -> Instant {
        let local = now + budget;
        match self {
            Self::WhileDesktopOpen => local,
            Self::Until(end) => end.min(local),
        }
    }

    fn is_open(self, now: Instant) -> bool {
        match self {
            Self::WhileDesktopOpen => true,
            Self::Until(end) => now < end,
        }
    }
}

fn catalogue_can_rotate(now: Instant, end: Instant, stopped: bool, previous_retired: bool) -> bool {
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
    expires_unix_ms: u64,
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
            (None, RoutePurpose::ReadAll | RoutePurpose::Manage) => 0,
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
                (
                    "Allow read-only session listing",
                    format!(
                        "Allow this phone to list {list_name} in {} selected projects?\n\nDevice: {suffix}\n\nThis does not allow terminal input or session changes.",
                        project_count,
                    ),
                )
            }
            RoutePurpose::ReadAll => (
                "Allow read-only session listing",
                format!(
                    "Allow this phone to list unarchived sessions from visible local projects across supported CLIs?\n\nDevice: {suffix}\n\nSharing stops when CC Desk closes or you stop sharing. This does not allow terminal input, archived sessions, or session changes."
                ),
            ),
            RoutePurpose::Manage => (
                "Allow session management",
                format!(
                    "Allow this paired phone to manage eligible Desk sessions from your visible local projects across Codex, Claude Code, and OpenCode for up to five minutes?\n\nDevice: {suffix}\n\nWithin that window the phone can make multiple requests and see the live view of an eligible running session. You can stop it any time. Terminal input is not allowed."
                ),
            ),
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
                use CatalogueWindow::{Until, WhileDesktopOpen};
                let route_window = match purpose {
                    RoutePurpose::ReadAll => WhileDesktopOpen,
                    RoutePurpose::Manage => {
                        Until(self.deadline.min(Instant::now() + MANAGEMENT_WINDOW))
                    }
                    RoutePurpose::Read(_) => Until(approval_end),
                };
                let owner = self.clone();
                let desktop_window = window.clone();
                let granted = tokio::task::spawn_blocking(move || {
                    owner.check_catalogue_device(&device, route_window)?;
                    let retained = match (scope, purpose) {
                        (Some(scope), RoutePurpose::Read(_)) => RouteSource::Read(Arc::new(
                            owner
                                .runtime
                                .retain_owner_catalogue(&owner.caller, &scope)?,
                        )),
                        (None, RoutePurpose::ReadAll) => RouteSource::ReadAll(Arc::new(
                            owner
                                .runtime
                                .retain_owner_management_portfolio(&owner.caller, || {
                                    owner.check_catalogue_device(&device, route_window)
                                })?,
                        )),
                        (None, RoutePurpose::Manage) => RouteSource::Manage(Arc::new(
                            owner
                                .runtime
                                .retain_owner_management_portfolio(&owner.caller, || {
                                    owner.check_catalogue_device(&device, route_window)
                                })?,
                        )),
                        _ => return Err(locked()),
                    };
                    owner.check_catalogue_device(&device, route_window)?;
                    Ok::<_, SafeError>(Arc::new(CatalogueOwner {
                        owner,
                        desktop_window,
                        device,
                        pair,
                        purpose,
                        grant_id: uuid::Uuid::new_v4(),
                        retained,
                        window: route_window,
                        lifetime: NativeRevocation::new(),
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
        window: CatalogueWindow,
    ) -> Result<(), SafeError> {
        let check = || match window {
            CatalogueWindow::WhileDesktopOpen => self.check_document(),
            CatalogueWindow::Until(_) => self.check(),
        };
        check()?;
        if !window.is_open(Instant::now())
            || !self
                .store
                .list_locked()
                .map_err(|_| locked())?
                .iter()
                .any(|device| device == expected)
        {
            return Err(locked());
        }
        check()
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

    pub(crate) async fn start_ordinary_catalogue_read(
        self: &Arc<Self>,
        target: DeviceTarget,
        app: &AppHandle,
        window: &Window,
    ) -> Result<Snapshot, SafeError> {
        self.start_paired_route(target, None, RoutePurpose::ReadAll, app, window)
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
        self.install_catalogue_task(owner).await?;
        self.snapshot(&mut cache).await
    }

    // Only called while processing the exact newly confirmed pairing under the
    // setup state lock. The local signed receipt carries the native dialog choice.
    pub(super) async fn start_ordinary_after_pairing(
        self: &Arc<Self>,
        consent: &ObserverApproval,
        issued: &SignedObserverReceipt,
        window: &Window,
    ) -> Result<(), SafeError> {
        if !issued.share_ordinary
            || !consent.current(issued)
            || self
                .sharing
                .lock()
                .await
                .as_ref()
                .is_some_and(|task| task.phase() != 0)
        {
            return Err(locked());
        }
        let (_, relay_pairs) = self.listing().await?;
        let store = self.store.clone();
        let request_id = issued.request_id;
        let tunnel = issued.tunnel;
        let binding = issued.endpoint_binding;
        let current = self.clone();
        let (device, portfolio) = tokio::task::spawn_blocking(move || {
            let row = store
                .list_locked()
                .map_err(|_| locked())?
                .into_iter()
                .find(|row| {
                    row.request_id == request_id
                        && row.tunnel == tunnel
                        && row.endpoint_binding == binding
                })
                .ok_or_else(locked)?;
            let relay = RelayPair {
                request_id: row.request_id,
                tunnel: row.tunnel,
                phone_id: row.phone_id,
                phone_relay_public_key_sec1: row.phone_relay_public_key_sec1,
                endpoint_binding: row.endpoint_binding,
            };
            if !relay_pairs
                .iter()
                .any(|candidate| party_matches(candidate, &relay))
            {
                return Err(locked());
            }
            if row.state == DeviceState::Pending {
                store.reconcile_present(&relay).map_err(|_| locked())?;
            }
            let device = DeviceSummary {
                state: DeviceState::Confirmed,
                ..row
            };
            let check = || {
                current.check_catalogue_device(&device, CatalogueWindow::Until(current.deadline))
            };
            check()?;
            let portfolio = current
                .runtime
                .retain_owner_management_portfolio(&current.caller, check)?;
            Ok::<_, SafeError>((device, portfolio))
        })
        .await
        .map_err(|_| uncertain())??;
        if !consent.current(issued) {
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
        self.install_catalogue_task(Arc::new(CatalogueOwner {
            owner: self.clone(),
            desktop_window: window.clone(),
            device,
            pair,
            purpose: RoutePurpose::ReadAll,
            grant_id: uuid::Uuid::new_v4(),
            retained: RouteSource::ReadAll(Arc::new(portfolio)),
            window: CatalogueWindow::WhileDesktopOpen,
            lifetime: NativeRevocation::new(),
            epoch: AtomicU64::new(0),
            reading: tokio::sync::Mutex::new(()),
        }))
        .await
    }

    async fn install_catalogue_task(&self, owner: Arc<CatalogueOwner>) -> Result<(), SafeError> {
        self.check()?;
        self.stop_share(None).await?;
        self.check()?;
        let offer = owner.sign_offer().await?;
        self.check()?;
        let tunnel = owner.device.tunnel;
        let catalogue_lifetime = owner.lifetime.clone();
        let (stop, cancel) = tokio::sync::watch::channel(false);
        let phase = Arc::new(AtomicU8::new(1));
        let retired = Arc::new(AtomicBool::new(false));
        let status = phase.clone();
        let retirement = retired.clone();
        let task = tokio::spawn(async move {
            let result = if matches!(owner.purpose, RoutePurpose::ReadAll | RoutePurpose::Manage) {
                owner
                    .serve_catalogue_window(offer, cancel, &retirement, &status)
                    .await
            } else {
                owner
                    .publish_and_serve(offer, cancel, &retirement, &status)
                    .await
            };
            status.store(if result.is_ok() { 0 } else { 3 }, Ordering::Release);
            result
        });
        *self.sharing.lock().await = Some(super::shares_owner::Task::new(
            tunnel,
            phase,
            stop,
            task,
            retired,
            catalogue_lifetime,
        ));
        Ok(())
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
    async fn serve_catalogue_window(
        self: &Arc<Self>,
        mut offer: SignedCatalogueOffer,
        mut stop: tokio::sync::watch::Receiver<bool>,
        retired: &AtomicBool,
        status: &AtomicU8,
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
                .publish_and_serve(offer, stop.clone(), &previous_retired, status)
                .await;
            let closed = previous_retired.load(Ordering::Acquire);
            if !closed {
                return Err(uncertain());
            }
            if !catalogue_can_rotate(
                Instant::now(),
                self.window.cap(Instant::now(), OFFER_TTL),
                stopped(&stop),
                closed,
            ) {
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
            if !catalogue_can_rotate(
                Instant::now(),
                self.window.cap(Instant::now(), OFFER_TTL),
                stopped(&stop),
                closed,
            ) {
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
        status: &AtomicU8,
    ) -> Result<(), SafeError> {
        let mut attempted_publish = false;
        let endpoint_retired = AtomicBool::new(true);
        let result = async {
            let remaining = offer.offer.expires_unix_ms.saturating_sub(unix_millis()?);
            let offer_end = Instant::now() + Duration::from_millis(remaining).min(OFFER_TTL);
            if *stop.borrow() || remaining < 10_000 {
                return Ok(());
            }
            self.verify_current().await?;
            if *stop.borrow() {
                return Ok(());
            }
            attempted_publish = true;
            let response = self
                .catalogue_wire(
                    ShareCommand::Publish {
                        tunnel: self.device.tunnel,
                        ttl_seconds: remaining.div_ceil(1000).min(60) as u16,
                        catalog_envelope: offer.wire.clone(),
                    },
                    offer_end,
                )
                .await?;
            if !matches!(response, ShareResponse::Published {ref tunnel,ref catalog_digest,..}
                if tunnel == &hex(&self.device.tunnel) && catalog_digest == &hex(&offer.digest))
            {
                return Err(uncertain());
            }
            status.store(1, Ordering::Release);
            loop {
                if *stop.borrow()
                    || !self.window.is_open(Instant::now())
                    || offer.offer.expires_unix_ms.saturating_sub(unix_millis()?) < 10_000
                {
                    return Ok(());
                }
                self.verify_current().await?;
                match self
                    .catalogue_wire(
                        ShareCommand::ReadSelection {
                            tunnel: self.device.tunnel,
                        },
                        offer_end,
                    )
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
        let endpoint_closed = endpoint_retired.load(Ordering::Acquire);
        let may_retry = matches!(self.purpose, RoutePurpose::ReadAll)
            && endpoint_closed
            && !self.lifetime.is_revoked();
        let cleaned = wait_for_exact_clear(
            &mut stop,
            may_retry,
            CLEAR_RETRY,
            || self.clear_published_offer(&offer),
            || status.store(5, Ordering::Release),
        )
        .await;
        if !cleaned || !endpoint_closed {
            return Err(uncertain());
        }
        retired.store(true, Ordering::Release);
        result
    }

    async fn catalogue_wire(
        self: &Arc<Self>,
        command: ShareCommand,
        offer_end: Instant,
    ) -> Result<ShareResponse, SafeError> {
        if !matches!(self.purpose, RoutePurpose::ReadAll) {
            return self.owner.share_wire(command).await;
        }
        let expected = self.device.tunnel;
        match &command {
            ShareCommand::Publish { tunnel, .. } | ShareCommand::ReadSelection { tunnel }
                if *tunnel == expected => {}
            ShareCommand::Answer { selection, .. } if selection.tunnel == expected => {}
            _ => return Err(locked()),
        }
        self.verify_current().await?;
        let setup = self
            .owner
            .profile
            .catalogue_operation(
                &self.owner.lifetime,
                &self.lifetime,
                offer_end.min(Instant::now() + CONTROL_BUDGET),
            )
            .map_err(|_| locked())?;
        let reply = match control::share_request(setup, self.owner.address, command).await {
            Ok(reply) => reply,
            Err(control::ControlFailure::RetirementUnknown) => {
                self.lifetime.revoke();
                return Err(uncertain());
            }
            Err(_) => return Err(uncertain()),
        };
        self.verify_current().await?;
        Ok(reply)
    }

    async fn clear_published_offer(&self, offer: &SignedCatalogueOffer) -> ExactClear {
        use control::ControlFailure;
        // Only the same authenticated tunnel/digest can be retired. A lost
        // reply is not a retirement, even after the relay's nominal TTL.
        if offer.grant != self.grant_id || self.owner.check_document().is_err() {
            return ExactClear::Rejected;
        }
        let Ok(setup) = self
            .owner
            .profile
            .operation(&self.owner.lifetime, Instant::now() + CONTROL_BUDGET)
        else {
            return ExactClear::Rejected;
        };
        let reply = control::share_request(
            setup,
            self.owner.address,
            ShareCommand::Clear {
                tunnel: self.device.tunnel,
                catalog_digest: offer.digest,
            },
        )
        .await;
        if self.owner.check_document().is_err() {
            return ExactClear::Rejected;
        }
        match reply {
            Ok(ShareResponse::Cleared {
                tunnel,
                catalog_digest,
            }) if tunnel == hex(&self.device.tunnel) && catalog_digest == hex(&offer.digest) => {
                ExactClear::Confirmed
            }
            Err(ControlFailure::Relay(
                EnrollmentError::Connection(ConnectError::Deadline | ConnectError::Transport)
                | EnrollmentError::OutcomeUnknown,
            )) => ExactClear::Retryable,
            _ => ExactClear::Rejected,
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

        let offer_remaining =
            Duration::from_millis(ready.expires_unix_ms.saturating_sub(unix_millis()?))
                .min(OFFER_TTL);
        let end = self.window.cap(Instant::now(), offer_remaining);
        let settings = self
            .owner
            .profile
            .catalogue_operation(&self.owner.lifetime, &self.lifetime, end)
            .map_err(|_| locked())?;
        let address = self.owner.address;
        let exact_owner = self.clone();
        let ReadySelection {
            wire: envelope,
            management,
            expires_unix_ms: _,
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
                    .catalogue_wire(
                        ShareCommand::Answer {
                            selection,
                            ready_envelope: envelope,
                        },
                        end,
                    )
                    .await?;
                if !matches!(answer, ShareResponse::Answered { .. }) {
                    return Err(uncertain());
                }
                tokio::select! {
                    biased;
                    _ = stop.changed() => {},
                    _ = tokio::time::sleep_until(end.into()) => {},
                    _ = &mut finished => {},
                }
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
        let cap = self.window.cap(Instant::now(), CONTROL_BUDGET);
        let settings = self
            .owner
            .profile
            .catalogue_operation(&self.owner.lifetime, &self.lifetime, cap)
            .map_err(|_| locked())?;
        let public = self.owner.profile.endpoint_spki();
        let root = NativeRevocation::child([
            self.owner.lifetime.root_witness().ok_or_else(locked)?,
            self.lifetime.root_witness().ok_or_else(locked)?,
        ]);
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
        let now = Instant::now();
        let remaining = self
            .window
            .cap(now, OFFER_TTL)
            .saturating_duration_since(now)
            .as_millis() as u64;
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
            RoutePurpose::ReadAll => offer.envelope(
                self.sign_packet(offer.signing_message_for(ListMode::Sessions))
                    .await?,
            ),
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
                expires_unix_ms: expiry,
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
            expires_unix_ms: ready.expires_unix_ms,
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
        if self.lifetime.is_revoked() {
            return Err(locked());
        }
        self.owner
            .check_catalogue_device(&self.device, self.window)?;
        if let RouteSource::ReadAll(portfolio) | RouteSource::Manage(portfolio) = &self.retained {
            self.owner
                .runtime
                .check_owner_management_portfolio(&self.owner.caller, portfolio)?;
        }
        if self.lifetime.is_revoked() {
            return Err(locked());
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
        if !matches!(self.purpose, RoutePurpose::Read(allowed) if allowed == mode)
            && !matches!(
                (self.purpose, mode),
                (RoutePurpose::ReadAll, ListMode::Sessions)
            )
        {
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
            let runtime = &self.owner.runtime;
            let caller = &self.owner.caller;
            match &self.retained {
                RouteSource::Read(retained) => {
                    runtime.read_owner_catalogue(caller, retained, epoch, mode, || {
                        self.check_current()
                    })
                }
                RouteSource::ReadAll(portfolio) => {
                    runtime.read_owner_ordinary_catalogue(caller, portfolio, epoch, || {
                        self.check_current()
                    })
                }
                RouteSource::Manage(_) => return Err(ReadError::Denied),
            }
            .map_err(|_| ReadError::Denied)
        })
        .await
        .map_err(|_| ReadError::Denied)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clear_recovery_waits_for_exact_confirmation_and_obeys_stop() {
        use std::{cell::Cell, future::ready};
        use tokio::sync::watch;
        let (stop_tx, mut stop) = watch::channel(false);
        let attempts = Cell::new(0);
        let waiting = Cell::new(0);
        assert!(
            wait_for_exact_clear(
                &mut stop,
                true,
                Duration::from_millis(1),
                || {
                    let attempt = attempts.get() + 1;
                    attempts.set(attempt);
                    ready(if attempt == 3 {
                        ExactClear::Confirmed
                    } else {
                        ExactClear::Retryable
                    })
                },
                || waiting.set(waiting.get() + 1)
            )
            .await
        );
        assert_eq!((attempts.get(), waiting.get()), (3, 2));

        attempts.set(0);
        assert!(
            !wait_for_exact_clear(
                &mut stop,
                true,
                Duration::from_secs(300),
                || {
                    attempts.set(attempts.get() + 1);
                    ready(ExactClear::Retryable)
                },
                || {
                    stop_tx.send_replace(true);
                }
            )
            .await
        );
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn clear_recovery_never_retries_a_rejection_or_unjoined_endpoint() {
        use std::{cell::Cell, future::ready};
        for (can_retry, outcome) in [(true, ExactClear::Rejected), (false, ExactClear::Retryable)] {
            let (_stop_tx, mut stop) = tokio::sync::watch::channel(false);
            let attempts = Cell::new(0);
            assert!(
                !wait_for_exact_clear(
                    &mut stop,
                    can_retry,
                    Duration::ZERO,
                    || {
                        attempts.set(attempts.get() + 1);
                        ready(outcome)
                    },
                    || panic!("must not wait or request a new offer")
                )
                .await
            );
            assert_eq!(attempts.get(), 1);
        }
    }

    #[test]
    fn ordinary_sharing_outlives_setup_but_never_its_per_offer_budget() {
        let start = Instant::now();
        let setup_end = start + Duration::from_secs(720);
        let later = setup_end + Duration::from_secs(1);
        let ordinary = CatalogueWindow::WhileDesktopOpen;
        let scoped = CatalogueWindow::Until(setup_end);
        assert!(ordinary.is_open(later));
        assert!(!scoped.is_open(later));
        assert_eq!(ordinary.cap(later, OFFER_TTL), later + OFFER_TTL);
        assert_eq!(ordinary.cap(later, CONTROL_BUDGET), later + CONTROL_BUDGET);
        assert_eq!(scoped.cap(later, OFFER_TTL), setup_end);
        assert!(!catalogue_can_rotate(
            later,
            ordinary.cap(later, OFFER_TTL),
            true,
            true
        ));
        assert!(!catalogue_can_rotate(
            later,
            ordinary.cap(later, OFFER_TTL),
            false,
            false
        ));
    }

    #[test]
    fn catalogue_window_rejects_unsafe_rotation_and_preserves_monotonic_epochs() {
        let clock = Instant::now();
        let end = clock + Duration::from_secs(30);
        assert!(catalogue_can_rotate(clock, end, false, true));
        assert!(!catalogue_can_rotate(clock, end, true, true));
        assert!(!catalogue_can_rotate(clock, end, false, false));
        assert!(!catalogue_can_rotate(
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
