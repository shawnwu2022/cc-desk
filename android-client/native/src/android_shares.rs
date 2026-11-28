//! Retained Android catalog and precise selection owner.
use crate::{
    ObserverHandle, cancel,
    enrollment::{RestoredMembership, lower_hex, restore_membership},
    keystore::{AndroidDurableStore, open_roles},
    shares::{
        prepare_catalogue_read, verify_catalog, verify_catalogue_route_offer_for, verify_ready,
    },
    start_android_observer,
};
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_security::{
    self as security, ClientGrantState, GrantError, LocalIdentity,
    catalogue_rendezvous::VerifiedCatalogueOffer,
    session_catalogue::{DisplayRow, ListMode},
};
use cc_desk_remote_transport::{
    self as transport,
    wss::{
        EnrollmentError, PhoneShareCommand, PhoneShareResponse, RelayAdmissionIdentity, RelayMode,
        ShareSelectionKey, request_phone_shares,
    },
};
use jni::{JavaVM, objects::GlobalRef};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    net::ToSocketAddrs,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub(crate) struct Inputs {
    pub local: [u8; 16],
    pub origin: CanonicalOrigin,
    pub endpoint: [u8; 91],
    pub relay: [u8; 91],
    pub approval: [u8; 91],
    pub mode: ListMode,
    pub management: bool,
}

enum Phase {
    Loading,
    Offers(Vec<([u8; 16], String, &'static str)>, u64),
    Waiting,
    Viewing(ObserverHandle),
    Catalogue(Vec<DisplayRow>),
    ManagementInventory(Vec<security::session_management_channel::ManagementRow>),
    ManagementProjection(String),
    Management(security::session_management_channel::ManagementOutcome),
    Failed,
    Closed,
}
struct Task {
    id: i64,
    local: [u8; 16],
    stop: watch::Sender<bool>,
    choice: watch::Sender<Option<[u8; 16]>>,
    management_choice:
        watch::Sender<Option<security::session_management_channel::ManagementRequest>>,
    phase: Arc<Mutex<Phase>>,
    worker: JoinHandle<bool>,
}
type PhaseRef = Arc<Mutex<Phase>>;
static TASKS: Mutex<(i64, Vec<Task>, bool)> = Mutex::new((0, Vec::new(), false));

fn now() -> Option<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}
pub(crate) fn begin(vm: JavaVM, facade: GlobalRef, input: Inputs) -> Option<i64> {
    let mut r = TASKS.lock().ok()?;
    if r.2 {
        return None;
    }
    if let Some(i) = r.1.iter().position(|t| t.local == input.local) {
        if !r.1[i].worker.is_finished() {
            return None;
        }
        let prior = r.1.remove(i);
        if !matches!(prior.worker.join(), Ok(true)) {
            r.2 = true;
            return None;
        }
    }
    if r.1.len() >= 4 {
        let idx = r.1.iter().position(|t| t.worker.is_finished())?;
        if !matches!(r.1.remove(idx).worker.join(), Ok(true)) {
            r.2 = true;
            return None;
        }
    }
    let id = r.0.checked_add(1)?;
    let local = input.local;
    let (stop, rx) = watch::channel(false);
    let (choice, chosen) = watch::channel(None);
    let (management_choice, selection) = watch::channel(None);
    let phase = Arc::new(Mutex::new(Phase::Loading));
    let original = phase.clone();
    let worker = thread::Builder::new()
        .name("ccdesk-share".into())
        .spawn(move || run(vm, facade, input, original, rx, chosen, selection))
        .ok()?;
    r.0 = id;
    r.1.push(Task {
        id,
        local,
        stop,
        choice,
        management_choice,
        phase,
        worker,
    });
    Some(id)
}
fn find(
    id: i64,
) -> Option<(
    PhaseRef,
    watch::Sender<bool>,
    watch::Sender<Option<[u8; 16]>>,
)> {
    let r = TASKS.lock().ok()?;
    let t = r.1.iter().find(|t| t.id == id)?;
    Some((t.phase.clone(), t.stop.clone(), t.choice.clone()))
}
pub(crate) fn state(id: i64) -> i32 {
    let Some((p, _, _)) = find(id) else { return 0 };
    p.lock()
        .map(|p| match &*p {
            Phase::Closed => 0,
            Phase::Loading => 1,
            Phase::Offers(..) => 2,
            Phase::Failed => 3,
            Phase::Waiting => 4,
            Phase::Viewing(_) => 5,
            Phase::Catalogue(_) => 6,
            Phase::Management(_) => 7,
            Phase::ManagementInventory(_) => 8,
            Phase::ManagementProjection(_) => 9,
        })
        .unwrap_or(0)
}
pub(crate) fn offers(id: i64) -> Option<String> {
    let (p, _, _) = find(id)?;
    let p = p.lock().ok()?;
    let Phase::Offers(items, expires) = &*p else {
        return None;
    };
    if now()? >= *expires {
        return None;
    }
    serde_json::to_string(&items.iter().map(|(offer,label,role)|serde_json::json!({"id":offer.iter().map(|n|format!("{n:02x}")).collect::<String>(),"label":label,"role":role})).collect::<Vec<_>>()).ok()
}
pub(crate) fn catalogue_rows(id: i64) -> Option<String> {
    let (p, _, _) = find(id)?;
    let p = p.lock().ok()?;
    match &*p {
        Phase::Catalogue(rows) => serde_json::to_string(rows).ok(),
        _ => None,
    }
}

pub(crate) fn management_rows(id: i64) -> Option<String> {
    let (p, _, _) = find(id)?;
    let p = p.lock().ok()?;
    let Phase::ManagementInventory(rows) = &*p else {
        return None;
    };
    serde_json::to_string(rows).ok()
}

pub(crate) fn submit_management(
    id: i64,
    request: security::session_management_channel::ManagementRequest,
) -> Option<()> {
    let tasks = TASKS.lock().ok()?;
    let task = tasks.1.iter().find(|task| task.id == id)?;
    let mut phase = task.phase.lock().ok()?;
    let Phase::ManagementInventory(rows) = &*phase else {
        return None;
    };
    if *task.stop.borrow()
        || !rows.iter().any(|row| {
            row.reference == request.reference
                && (!row.archived
                    || matches!(
                        &request.action,
                        security::session_management::ManagementAction::Restore
                    ))
        })
    {
        return None;
    }
    task.management_choice.send(Some(request)).ok()?;
    *phase = Phase::Waiting;
    Some(())
}

pub(crate) fn management_projection(id: i64) -> Option<String> {
    let (phase, stop, _) = find(id)?;
    let phase = phase.lock().ok()?;
    if *stop.borrow() {
        return None;
    }
    match &*phase {
        Phase::ManagementProjection(frame) => Some(frame.clone()),
        _ => None,
    }
}

pub(crate) fn management_result(id: i64) -> Option<i32> {
    let (p, _, _) = find(id)?;
    let p = p.lock().ok()?;
    let Phase::Management(outcome) = &*p else {
        return None;
    };
    Some(*outcome as i32)
}

pub(crate) fn choose(id: i64, raw: &str) -> Option<()> {
    let offer = lower_hex::<16>(raw).ok()?;
    let (p, s, tx) = find(id)?;
    let mut p = p.lock().ok()?;
    let Phase::Offers(items, expires) = &*p else {
        return None;
    };
    if *s.borrow() || now()? >= *expires || !items.iter().any(|(x, _, _)| *x == offer) {
        return None;
    }
    tx.send(Some(offer)).ok()?;
    *p = Phase::Waiting;
    Some(())
}
pub(crate) fn viewer(id: i64) -> Option<i64> {
    let (p, _, _) = find(id)?;
    let p = p.lock().ok()?;
    if let Phase::Viewing(v) = *p {
        Some(v.token())
    } else {
        None
    }
}
pub(crate) fn stop(id: i64) {
    if let Some((p, tx, _)) = find(id) {
        if let Ok(mut p) = p.lock() {
            tx.send_replace(true);
            if let Phase::Viewing(v) = *p {
                cancel(v.token());
            }
            *p = Phase::Closed;
        }
    }
}
fn publish(p: &Mutex<Phase>, stop: &watch::Receiver<bool>, next: Phase) -> Option<()> {
    let mut p = p.lock().ok()?;
    if *stop.borrow() {
        return None;
    }
    *p = next;
    Some(())
}
async fn bounded<T>(stop: &mut watch::Receiver<bool>, f: impl Future<Output = T>) -> Option<T> {
    if *stop.borrow_and_update() || stop.has_changed().is_err() {
        return None;
    }
    tokio::select! {biased; _=stop.changed()=>None,v=f=>Some(v)}
}

/// Only the native enrollment owner can supply these Keystore-backed roles.
#[allow(clippy::too_many_arguments)]
async fn invoke_published_management(
    member: &RestoredMembership,
    key: &RelayAdmissionIdentity,
    point: &[u8; 65],
    addr: std::net::SocketAddr,
    signed_offer: &str,
    digest: [u8; 32],
    relay_expiry: u64,
    identity: LocalIdentity,
    mode: ListMode,
    phase: &Mutex<Phase>,
    selected: &mut watch::Receiver<Option<security::session_management_channel::ManagementRequest>>,
    stop: &mut watch::Receiver<bool>,
) -> Option<security::session_management_channel::ManagementOutcome> {
    use management::{ChannelError, ManagementOutcome};
    use security::{P256Spki, PeerPin, Role, session_management_channel as management};

    let host = P256Spki::new(member.host_spki()).ok()?;
    let phone = member.catalogue_client_id().ok()?;
    if identity.role() != Role::Client || identity.endpoint_id() != phone {
        return None;
    }
    let expiry = now()?;
    if relay_expiry <= expiry {
        return None;
    }
    let offer = management::verify_offer(
        &host,
        member.tunnel(),
        member.endpoint_binding(),
        phone,
        digest,
        signed_offer,
        expiry,
    )
    .ok()?;
    let mut nonce = [0; 16];
    SystemRandom::new().fill(&mut nonce).ok()?;
    if nonce == [0; 16] {
        return None;
    }
    let selection = ShareSelectionKey {
        tunnel: member.tunnel(),
        catalog_digest: digest,
        scope_offer_id: offer.offer().offer,
        selection_id: nonce,
    };
    let origin = member.origin().as_str();
    let call = |cmd| request_phone_shares(origin, addr, Duration::from_secs(3), key, point, cmd);
    match bounded(stop, call(PhoneShareCommand::Choose { selection })).await? {
        Ok(PhoneShareResponse::Selection { .. }) | Err(EnrollmentError::OutcomeUnknown) => {}
        _ => return None,
    }
    let setup_end = Instant::now() + Duration::from_secs(10);
    let route = loop {
        if Instant::now() >= setup_end || now()? >= relay_expiry {
            return None;
        }
        match bounded(stop, call(PhoneShareCommand::GetSelection { selection })).await? {
            Ok(PhoneShareResponse::Ready { ready_envelope, .. }) => {
                break offer
                    .verify_selection(nonce, &ready_envelope, now()?)
                    .ok()?;
            }
            Ok(
                PhoneShareResponse::Selection { .. } | PhoneShareResponse::SelectionEmpty { .. },
            )
            | Err(EnrollmentError::OutcomeUnknown | EnrollmentError::Connection(_)) => {}
            _ => return None,
        }
        bounded(stop, tokio::time::sleep(Duration::from_millis(400))).await?;
    };
    let budget = setup_end
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(relay_expiry.checked_sub(now()?)?));
    let socket = bounded(
        stop,
        transport::wss::connect_admitted_with_mode(
            origin,
            addr,
            budget,
            key,
            RelayMode::RendezvousV1,
            member.tunnel(),
        ),
    )
    .await?
    .ok()?;
    let (stream, mux) =
        transport::start_admitted(socket, member.tunnel(), Duration::from_secs(5)).ok()?;
    let (cancel, token) = security::cancellation();
    let (done, finished) = tokio::sync::oneshot::channel();
    let display_stop = stop.clone();
    let call = async {
        let stream_stop = display_stop.clone();
        let result = management::invoke_from_inventory_with_projection(
            security::Unbound::new(stream),
            identity,
            PeerPin::new(Role::Host, host),
            route,
            token,
            mode,
            |rows| async move {
                publish(phase, &display_stop, Phase::ManagementInventory(rows))?;
                selected.changed().await.ok()?;
                selected.borrow_and_update().clone()
            },
            |frame| publish(phase, &stream_stop, Phase::ManagementProjection(frame)).is_some(),
        )
        .await;
        let _ = done.send(());
        match result {
            Ok(outcome) => Some(outcome),
            Err(ChannelError::OutcomeUnknown) => Some(ManagementOutcome::Unknown),
            Err(ChannelError::Unavailable | ChannelError::Denied) => None,
        }
    };
    let observe = async {
        if *stop.borrow() || stop.has_changed().is_err() {
            cancel.cancel();
            return;
        }
        tokio::select! { biased; _=stop.changed()=> cancel.cancel(), _=finished=>{} }
    };
    let (outcome, ()) = tokio::join!(call, observe);
    mux.request_close();
    if mux.wait_closed().await.is_err() && outcome == Some(ManagementOutcome::Applied) {
        return Some(ManagementOutcome::Unknown);
    }
    outcome
}

/// Reads an independently signed list offer. A failed catalogue exchange
/// never falls through to a terminal session or grants input.
async fn fetch_published_catalogue(
    member: &RestoredMembership,
    key: &RelayAdmissionIdentity,
    point: &[u8; 65],
    addr: std::net::SocketAddr,
    offer: &VerifiedCatalogueOffer,
    relay_expiry: u64,
    identity: LocalIdentity,
    stop: &mut watch::Receiver<bool>,
) -> Option<Vec<DisplayRow>> {
    let mut nonce = [0; 16];
    SystemRandom::new().fill(&mut nonce).ok()?;
    if nonce == [0; 16] || relay_expiry <= now()? {
        return None;
    }
    let exact = offer.selection(nonce);
    let selection = ShareSelectionKey {
        tunnel: exact.tunnel,
        catalog_digest: exact.catalog_digest,
        scope_offer_id: exact.offer,
        selection_id: exact.selection,
    };
    let origin = member.origin().as_str();
    let call = |cmd| request_phone_shares(origin, addr, Duration::from_secs(3), key, point, cmd);
    match bounded(stop, call(PhoneShareCommand::Choose { selection })).await? {
        Ok(PhoneShareResponse::Selection { .. }) | Err(EnrollmentError::OutcomeUnknown) => {}
        _ => return None,
    }
    let setup_end = Instant::now() + Duration::from_secs(10);
    let admitted = loop {
        if Instant::now() >= setup_end || now()? >= relay_expiry {
            return None;
        }
        match bounded(stop, call(PhoneShareCommand::GetSelection { selection })).await? {
            Ok(PhoneShareResponse::Ready { ready_envelope, .. }) => {
                break prepare_catalogue_read(member, offer, nonce, &ready_envelope, now()?)
                    .ok()?;
            }
            Ok(
                PhoneShareResponse::Selection { .. } | PhoneShareResponse::SelectionEmpty { .. },
            )
            | Err(EnrollmentError::OutcomeUnknown | EnrollmentError::Connection(_)) => {}
            _ => return None,
        }
        bounded(stop, tokio::time::sleep(Duration::from_millis(400))).await?;
    };
    let budget = setup_end
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(relay_expiry.checked_sub(now()?)?));
    let socket = bounded(
        stop,
        transport::wss::connect_admitted_with_mode(
            origin,
            addr,
            budget,
            key,
            RelayMode::RendezvousV1,
            member.tunnel(),
        ),
    )
    .await?
    .ok()?;
    let fetch_time = now()?;
    let (stream, mux) =
        transport::start_admitted(socket, member.tunnel(), Duration::from_secs(5)).ok()?;
    let (cancel, token) = security::cancellation();
    let (done, finished) = tokio::sync::oneshot::channel();
    let read = async {
        let rows = admitted
            .fetch(
                security::Unbound::new(stream),
                identity,
                member.tunnel(),
                fetch_time,
                token,
                offer.mode(),
            )
            .await
            .ok();
        let _ = done.send(());
        rows
    };
    let stop_and_join = async {
        if *stop.borrow() || stop.has_changed().is_err() {
            cancel.cancel();
            return;
        }
        tokio::select! {
            biased;
            _=stop.changed()=> cancel.cancel(),
            _=finished=>{},
        }
    };
    let (rows, ()) = tokio::join!(read, stop_and_join);
    mux.request_close();
    mux.wait_closed().await.ok()?;
    if *stop.borrow() || stop.has_changed().is_err() {
        None
    } else {
        rows
    }
}

fn run(
    vm: JavaVM,
    facade: GlobalRef,
    input: Inputs,
    p: PhaseRef,
    mut stop: watch::Receiver<bool>,
    mut pick: watch::Receiver<Option<[u8; 16]>>,
    mut management_pick: watch::Receiver<
        Option<security::session_management_channel::ManagementRequest>,
    >,
) -> bool {
    let mut all_retired = true;
    let result = (|| -> Option<()> {
        let mut env = vm.attach_current_thread().ok()?;
        let data = AndroidDurableStore::new(&mut env, facade.as_obj()).load(input.local)?;
        let member = restore_membership(
            &data,
            input.origin.clone(),
            input.endpoint,
            input.relay,
            input.approval,
            now()?,
        )
        .ok()?;
        if *stop.borrow() {
            return None;
        }
        let url = url::Url::parse(member.origin().as_str()).ok()?;
        let addr = (url.host_str()?, url.port().unwrap_or(443))
            .to_socket_addrs()
            .ok()?
            .next()?;
        if *stop.borrow() {
            return None;
        }
        let endpoint = env.get_java_vm().ok()?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        let roles = open_roles(
            endpoint,
            facade.clone(),
            input.endpoint,
            input.origin,
            member.relay_id(),
        )
        .ok()?;
        let relay = roles.relay_for_enrollment(member.relay_id());
        let point: [u8; 65] = input.relay[26..].try_into().ok()?;
        let get = request_phone_shares(
            member.origin().as_str(),
            addr,
            Duration::from_secs(3),
            &relay,
            &point,
            PhoneShareCommand::GetShares {
                tunnel: member.tunnel(),
            },
        );
        let first = rt.block_on(bounded(&mut stop, get));
        if input.management {
            // Never fall through to read-only or terminal consent on failure.
            let (keys, provider) = roles.into_parts();
            let result = match &first {
                Some(Ok(PhoneShareResponse::Shares {
                    catalog_digest,
                    catalog_envelope,
                    expires_unix_ms,
                    ..
                })) => lower_hex(catalog_digest).ok().and_then(|digest| {
                    publish(&p, &stop, Phase::Waiting)?;
                    rt.block_on(invoke_published_management(
                        &member,
                        &relay,
                        &point,
                        addr,
                        catalog_envelope,
                        digest,
                        *expires_unix_ms,
                        keys.endpoint,
                        input.mode,
                        &p,
                        &mut management_pick,
                        &mut stop,
                    ))
                }),
                _ => None,
            };
            drop(relay);
            drop(keys.relay);
            drop(keys.approval);
            all_retired = provider.retire();
            if !all_retired {
                return None;
            }
            publish(&p, &stop, Phase::Management(result?))?;
            return Some(());
        }
        if let Some(Ok(PhoneShareResponse::Shares {
            catalog_digest,
            catalog_envelope,
            expires_unix_ms,
            ..
        })) = &first
        {
            let digest = lower_hex(catalog_digest).ok();
            if let Some(offer) = digest.and_then(|digest| {
                verify_catalogue_route_offer_for(
                    &member,
                    catalog_envelope,
                    digest,
                    now()?,
                    input.mode,
                )
                .ok()
            }) {
                let (keys, provider) = roles.into_parts();
                let rows = publish(&p, &stop, Phase::Waiting).and_then(|()| {
                    rt.block_on(fetch_published_catalogue(
                        &member,
                        &relay,
                        &point,
                        addr,
                        &offer,
                        *expires_unix_ms,
                        keys.endpoint,
                        &mut stop,
                    ))
                });
                drop(relay);
                drop(keys.relay);
                drop(keys.approval);
                all_retired = provider.retire();
                if !all_retired {
                    return None;
                }
                publish(&p, &stop, Phase::Catalogue(rows?))?;
                return Some(());
            }
        }
        if input.mode == ListMode::Archived {
            let (keys, provider) = roles.into_parts();
            drop(relay);
            drop(keys);
            all_retired = provider.retire();
            return None;
        }
        let selection = rt.block_on(async {
            let origin = member.origin().as_str();
            let call = |cmd| {
                request_phone_shares(origin, addr, Duration::from_secs(3), &relay, &point, cmd)
            };
            let value = first?.ok()?;
            let (catalog_digest, catalog_envelope, relay_expiry) = match value {
                PhoneShareResponse::Shares {
                    catalog_digest,
                    catalog_envelope,
                    expires_unix_ms,
                    ..
                } => (catalog_digest, catalog_envelope, expires_unix_ms),
                PhoneShareResponse::SharesEmpty { .. } => {
                    publish(
                        &p,
                        &stop,
                        Phase::Offers(vec![], now()?.saturating_add(60_000)),
                    )?;
                    bounded(&mut stop, tokio::time::sleep(Duration::from_secs(60))).await?;
                    return None;
                }
                _ => return None,
            };
            let catalog = verify_catalog(
                &member,
                &catalog_envelope,
                lower_hex(&catalog_digest).ok()?,
                now()?,
            )
            .ok()?;
            if relay_expiry <= now()? {
                return None;
            }
            publish(
                &p,
                &stop,
                Phase::Offers(
                    catalog
                        .offers()
                        .iter()
                        .map(|x| {
                            (
                                x.id(),
                                x.label().to_owned(),
                                if x.role() == cc_desk_remote_client::ClientRole::Controller {
                                    "controller"
                                } else {
                                    "observer"
                                },
                            )
                        })
                        .collect(),
                    catalog.expires_unix_ms(),
                ),
            )?;
            let remaining = Duration::from_millis(catalog.expires_unix_ms().checked_sub(now()?)?);
            let offer = bounded(
                &mut stop,
                tokio::time::timeout(remaining, async {
                    if let Some(x) = *pick.borrow_and_update() {
                        return Some(x);
                    }
                    pick.changed().await.ok()?;
                    *pick.borrow()
                }),
            )
            .await?
            .ok()??;
            if now()? >= catalog.expires_unix_ms() {
                return None;
            }
            let mut nonce = [0; 16];
            SystemRandom::new().fill(&mut nonce).ok()?;
            if nonce == [0; 16] {
                return None;
            }
            let key = ShareSelectionKey {
                tunnel: member.tunnel(),
                catalog_digest: catalog.digest(),
                scope_offer_id: offer,
                selection_id: nonce,
            };
            match bounded(
                &mut stop,
                call(PhoneShareCommand::Choose {
                    selection: key.clone(),
                }),
            )
            .await?
            {
                Ok(PhoneShareResponse::Selection { .. }) | Err(EnrollmentError::OutcomeUnknown) => {
                }
                _ => return None,
            }
            loop {
                if now()? >= catalog.expires_unix_ms() {
                    return None;
                }
                bounded(&mut stop, tokio::time::sleep(Duration::from_millis(750))).await?;
                match bounded(
                    &mut stop,
                    call(PhoneShareCommand::GetSelection {
                        selection: key.clone(),
                    }),
                )
                .await?
                {
                    Ok(PhoneShareResponse::Ready { ready_envelope, .. }) => {
                        let current = now()?;
                        let scope =
                            verify_ready(&member, &catalog, offer, nonce, &ready_envelope, current)
                                .ok()?;
                        return member.authorize_share(scope, current).ok();
                    }
                    Ok(
                        PhoneShareResponse::Selection { .. }
                        | PhoneShareResponse::SelectionEmpty { .. },
                    )
                    | Err(EnrollmentError::OutcomeUnknown | EnrollmentError::Connection(_)) => {}
                    _ => return None,
                }
            }
        });
        drop(relay);
        let (keys, provider) = roles.into_parts();
        drop(keys);
        all_retired = provider.retire();
        if !all_retired || *stop.borrow() {
            return None;
        }
        let authority = selection?;
        let origin = Instant::now();
        let clock = Arc::new(move || {
            u64::try_from(origin.elapsed().as_millis()).map_err(|_| GrantError::Clock)
        });
        let viewer = {
            let mut phase = p.lock().ok()?;
            if *stop.borrow() {
                return None;
            }
            let viewer = start_android_observer(
                &env,
                facade.as_obj(),
                authority,
                addr,
                ClientGrantState::new(clock),
            )
            .ok()?;
            *phase = Phase::Viewing(viewer);
            viewer
        };
        rt.block_on(async {
            loop {
                if bounded(&mut stop, tokio::time::sleep(Duration::from_millis(200)))
                    .await
                    .is_none()
                {
                    break;
                }
                if matches!(
                    crate::status(viewer.token()),
                    crate::ObserverStatus::Closed | crate::ObserverStatus::Unavailable
                ) {
                    break;
                }
            }
        });
        cancel(viewer.token());
        None
    })();
    if result.is_none() {
        if let Ok(mut state) = p.lock() {
            *state = if *stop.borrow() {
                Phase::Closed
            } else {
                Phase::Failed
            };
        }
    }
    all_retired
}
