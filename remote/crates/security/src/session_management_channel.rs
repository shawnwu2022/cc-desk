//! A separately signed, mutually pinned, single-operation session management lane.
use crate::owner_clock::{Instant, sleep_until};
use crate::session_catalogue::ListMode;
use crate::session_management::ManagementAction;
use crate::{CancelToken, LocalIdentity, P256Spki, PeerPin, Role, Unbound};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use std::{future::Future, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const ALPN: &[u8] = b"cc-desk-session-management/2";
const DOMAIN: &[u8] = b"cc-desk.management.route.v1\0";
const MAX_ACTION: usize = 2048;
const ACK: u8 = 1;
const NEXT_VIEW: u8 = 2;
const VIEW_FRAME: u8 = 4;
const VIEW_ENDED: u8 = 5;
const MAX_VIEW_BYTES: usize = 100 * 240 * 4 + 99;

// Native caller must recheck exact run, source and peer inside each capture.
pub enum ManagementDispatch {
    Outcome(ManagementOutcome),
    Projection(std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>),
}
impl From<ManagementOutcome> for ManagementDispatch {
    fn from(outcome: ManagementOutcome) -> Self {
        Self::Outcome(outcome)
    }
}

fn valid_view(view: &str) -> bool {
    view.len() <= MAX_VIEW_BYTES
        && !view.chars().any(|c| {
            (c.is_control() && c != '\n')
                || matches!(c as u32, 0x061c | 0x200e..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
        })
}

fn handshake_budget(end: Instant) -> Duration {
    end.saturating_duration_since(Instant::now())
        .min(crate::tls::MAX_HANDSHAKE_BUDGET)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ManagementRoute {
    pub tunnel: [u8; 16],
    pub membership_binding: [u8; 32],
    pub offer: [u8; 16],
    pub offer_digest: [u8; 32],
    pub selection: [u8; 16],
    pub host: [u8; 32],
    pub phone: [u8; 32],
}

impl ManagementRoute {
    /// The desktop signs independently of catalogue and terminal approval.
    pub fn signing_bytes(self, expires_ms: u64) -> Vec<u8> {
        [
            DOMAIN,
            &self.tunnel,
            &self.membership_binding,
            &self.offer,
            &self.offer_digest,
            &self.selection,
            &self.host,
            &self.phone,
            &expires_ms.to_be_bytes(),
        ]
        .concat()
    }

    pub fn envelope(expires_ms: u64, signature: [u8; 64]) -> String {
        URL_SAFE_NO_PAD.encode([&expires_ms.to_be_bytes()[..], &signature[..]].concat())
    }
}

const OFFER_DOMAIN: &[u8] = b"cc-desk.management.offer.v1\0";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ManagementOffer {
    pub tunnel: [u8; 16],
    pub membership_binding: [u8; 32],
    pub offer: [u8; 16],
    pub host: [u8; 32],
    pub phone: [u8; 32],
}

impl ManagementOffer {
    pub fn signing_bytes(self, expires_ms: u64) -> Vec<u8> {
        [
            OFFER_DOMAIN,
            &self.tunnel,
            &self.membership_binding,
            &self.offer,
            &self.host,
            &self.phone,
            &expires_ms.to_be_bytes(),
        ]
        .concat()
    }

    pub fn envelope(self, expires_ms: u64, signature: [u8; 64]) -> String {
        URL_SAFE_NO_PAD.encode(
            [
                &self.offer[..],
                &expires_ms.to_be_bytes()[..],
                &signature[..],
            ]
            .concat(),
        )
    }

    pub fn selection(self, digest: [u8; 32], selection: [u8; 16]) -> ManagementRoute {
        ManagementRoute {
            tunnel: self.tunnel,
            membership_binding: self.membership_binding,
            offer: self.offer,
            offer_digest: digest,
            selection,
            host: self.host,
            phone: self.phone,
        }
    }
}

pub struct VerifiedManagementOffer {
    expected: ManagementOffer,
    host: P256Spki,
    digest: [u8; 32],
    expires_ms: u64,
    end: Instant,
}

impl VerifiedManagementOffer {
    pub fn offer(&self) -> ManagementOffer {
        self.expected
    }

    pub fn verify_selection(
        &self,
        selection: [u8; 16],
        ready: &str,
        now_ms: u64,
    ) -> Result<VerifiedManagementRoute, ChannelError> {
        let route = self.expected.selection(self.digest, selection);
        let (expires_ms, _) = unpack(ready, now_ms)?;
        if expires_ms > self.expires_ms || Instant::now() >= self.end {
            return Err(Denied);
        }
        let mut proof = verify_route(&self.host, route, ready, now_ms)?;
        proof.end = proof.end.min(self.end);
        Ok(proof)
    }
}

pub fn verify_offer(
    host: &P256Spki,
    tunnel: [u8; 16],
    membership_binding: [u8; 32],
    phone: [u8; 32],
    advertised_digest: [u8; 32],
    envelope: &str,
    now_ms: u64,
) -> Result<VerifiedManagementOffer, ChannelError> {
    let start = Instant::now();
    if tunnel == [0; 16]
        || phone == [0; 32]
        || membership_binding == [0; 32]
        || envelope.len() != 118
    {
        return Err(Denied);
    }
    let wire = URL_SAFE_NO_PAD.decode(envelope).map_err(|_| Denied)?;
    let (id, signed) = wire.split_at_checked(16).ok_or(Denied)?;
    let offer = id.try_into().map_err(|_| Denied)?;
    if offer == [0; 16] {
        return Err(Denied);
    }
    let expected = ManagementOffer {
        tunnel,
        membership_binding,
        offer,
        host: host.endpoint_id(Role::Host),
        phone,
    };
    let (expires_ms, signature) = unpack_wire(signed, now_ms)?;
    if ring::digest::digest(&ring::digest::SHA256, envelope.as_bytes()).as_ref()
        != advertised_digest
    {
        return Err(Denied);
    }
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &host.0[26..])
        .verify(&expected.signing_bytes(expires_ms), &signature)
        .map_err(|_| Denied)?;
    Ok(VerifiedManagementOffer {
        expected,
        host: host.clone(),
        digest: advertised_digest,
        expires_ms,
        end: start + Duration::from_millis(expires_ms - now_ms),
    })
}

fn unpack(envelope: &str, now_ms: u64) -> Result<(u64, [u8; 64]), ChannelError> {
    if envelope.len() != 96 {
        return Err(Denied);
    }
    let wire = URL_SAFE_NO_PAD.decode(envelope).map_err(|_| Denied)?;
    unpack_wire(&wire, now_ms)
}

fn unpack_wire(wire: &[u8], now_ms: u64) -> Result<(u64, [u8; 64]), ChannelError> {
    if wire.len() != 72 {
        return Err(Denied);
    }
    let expires_ms = u64::from_be_bytes(wire[..8].try_into().map_err(|_| Denied)?);
    expires_ms
        .checked_sub(now_ms)
        .filter(|ms| (1..=60_000).contains(ms))
        .ok_or(Denied)?;
    Ok((expires_ms, wire[8..].try_into().map_err(|_| Denied)?))
}

pub struct VerifiedManagementRoute {
    route: ManagementRoute,
    end: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelError {
    Denied,
    Unavailable,
    OutcomeUnknown,
}
use ChannelError::{Denied, OutcomeUnknown, Unavailable};

/// Expected fields come from local selection and membership, not relay claims.
fn verify_route(
    host: &P256Spki,
    route: ManagementRoute,
    envelope: &str,
    now_ms: u64,
) -> Result<VerifiedManagementRoute, ChannelError> {
    let start = Instant::now();
    if host.endpoint_id(Role::Host) != route.host
        || route.tunnel == [0; 16]
        || route.offer == [0; 16]
        || route.offer_digest == [0; 32]
        || route.selection == [0; 16]
        || route.membership_binding == [0; 32]
        || route.phone == [0; 32]
    {
        return Err(Denied);
    }
    let (expires_ms, signature) = unpack(envelope, now_ms)?;
    let duration = expires_ms - now_ms;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &host.0[26..])
        .verify(&route.signing_bytes(expires_ms), &signature)
        .map_err(|_| Denied)?;
    Ok(VerifiedManagementRoute {
        route,
        end: start + Duration::from_millis(duration),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ManagementOutcome {
    Applied = 1,
    Denied = 2,
    Unknown = 3,
}

#[derive(Clone)]
pub struct ManagementRequest {
    /// A desktop minted, route-scoped random reference, never a provider ID or path.
    pub reference: [u8; 32],
    pub action: ManagementAction,
}

impl ManagementRequest {
    async fn receive(io: &mut (impl AsyncRead + Unpin)) -> Result<Self, ChannelError> {
        let mut reference = [0; 32];
        io.read_exact(&mut reference).await.map_err(io_failure)?;
        let len = usize::from(io.read_u16().await.map_err(io_failure)?);
        if reference == [0; 32] || !(1..=MAX_ACTION).contains(&len) {
            return Err(Denied);
        }
        let mut bytes = vec![0; len];
        io.read_exact(&mut bytes).await.map_err(io_failure)?;
        let action = ManagementAction::decode(&bytes).map_err(|_| Denied)?;
        Ok(Self { reference, action })
    }

    fn encode(&self) -> Result<Vec<u8>, ChannelError> {
        let action = serde_json::to_vec(&self.action).map_err(|_| Denied)?;
        if self.reference == [0; 32] || action.len() > MAX_ACTION {
            return Err(Denied);
        }
        ManagementAction::decode(&action).map_err(|_| Denied)?;
        let mut bytes = Vec::with_capacity(34 + action.len());
        bytes.extend_from_slice(&self.reference);
        bytes.extend_from_slice(&(action.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&action);
        Ok(bytes)
    }
}

fn io_failure(_: std::io::Error) -> ChannelError {
    Unavailable
}

async fn send(io: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> Result<(), ChannelError> {
    io.write_all(bytes).await.map_err(io_failure)?;
    io.flush().await.map_err(io_failure)
}

async fn take<const N: usize>(io: &mut (impl AsyncRead + Unpin)) -> Result<[u8; N], ChannelError> {
    let mut bytes = [0; N];
    io.read_exact(&mut bytes).await.map_err(io_failure)?;
    Ok(bytes)
}

async fn bounded<T>(
    end: Instant,
    cancel: &mut CancelToken,
    f: impl Future<Output = Result<T, ChannelError>>,
) -> Result<T, ChannelError> {
    if Instant::now() >= end || cancel.is_cancelled() {
        return Err(Unavailable);
    }
    let v = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Unavailable),
        _ = sleep_until(end) => Err(Unavailable),
        v = f => v,
    }?;
    if Instant::now() >= end || cancel.is_cancelled() {
        return Err(Unavailable);
    }
    Ok(v)
}

async fn admitted(f: impl Future<Output = bool>) -> Result<(), ChannelError> {
    if f.await { Ok(()) } else { Err(Denied) }
}

const MAX_INVENTORY_BYTES: usize = 512 * 1024;
const MAX_INVENTORY_ROWS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagementRow {
    pub reference: [u8; 32],
    pub title: String,
    pub lifecycle: crate::session_catalogue::DisplayLifecycle,
    pub archived: bool,
}

fn encode_mode(mode: ListMode) -> u8 {
    match mode {
        ListMode::Sessions => 1,
        ListMode::Archived => 2,
    }
}

fn decode_mode(wire: [u8; 1]) -> Result<ListMode, ChannelError> {
    match wire {
        [1] => Ok(ListMode::Sessions),
        [2] => Ok(ListMode::Archived),
        _ => Err(Denied),
    }
}

fn validate_mode(rows: &[ManagementRow], mode: ListMode) -> Result<(), ChannelError> {
    if rows
        .iter()
        .any(|row| row.archived != (mode == ListMode::Archived))
    {
        return Err(Denied);
    }
    Ok(())
}

fn validate_inventory(rows: &[ManagementRow]) -> Result<(), ChannelError> {
    let mut seen = std::collections::BTreeSet::new();
    if rows.len() > MAX_INVENTORY_ROWS
        || rows.iter().any(|r| {
            r.reference == [0; 32]
                || !seen.insert(r.reference)
                || r.title.len() > 256
                || r.title.trim().is_empty()
                || r.title.chars().any(char::is_control)
                || r.lifecycle == crate::session_catalogue::DisplayLifecycle::History
                || (r.archived && r.lifecycle == crate::session_catalogue::DisplayLifecycle::Open)
        })
    {
        return Err(Denied);
    }
    Ok(())
}

async fn write_inventory(
    io: &mut (impl AsyncWrite + Unpin),
    rows: Vec<ManagementRow>,
) -> Result<(), ChannelError> {
    validate_inventory(&rows)?;
    let bytes = serde_json::to_vec(&rows).map_err(|_| Denied)?;
    if bytes.len() > MAX_INVENTORY_BYTES {
        return Err(Denied);
    }
    io.write_u32(bytes.len() as u32).await.map_err(io_failure)?;
    send(io, &bytes).await
}

async fn read_inventory(
    io: &mut (impl AsyncRead + Unpin),
) -> Result<Vec<ManagementRow>, ChannelError> {
    let n = io.read_u32().await.map_err(io_failure)? as usize;
    if !(1..=MAX_INVENTORY_BYTES).contains(&n) {
        return Err(Denied);
    }
    let mut bytes = vec![0; n];
    io.read_exact(&mut bytes).await.map_err(io_failure)?;
    let rows: Vec<ManagementRow> = serde_json::from_slice(&bytes).map_err(|_| Denied)?;
    validate_inventory(&rows)?;
    Ok(rows)
}

/// The desktop consumes its route selection once before serving it. Dispatch
/// must still enforce current paired membership and the exact one-shot grant
/// at the provider write, including the passed cancellation and deadline.
#[allow(clippy::too_many_arguments)]
pub async fn serve<S, T, A, P, D>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    cancel: CancelToken,
    current: impl Fn(ManagementRoute) -> A,
    prepare: impl FnOnce(ManagementRequest) -> P,
    dispatch: impl FnOnce(T, ManagementAction, [u8; 32], CancelToken, Instant) -> D,
) -> Result<(), ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    A: Future<Output = bool>,
    P: Future<Output = Option<([u8; 32], T)>>,
    D: Future<Output = ManagementOutcome>,
{
    serve_inner(
        io,
        identity,
        pin,
        verified,
        cancel,
        current,
        None::<fn(ListMode) -> std::future::Ready<Result<Vec<ManagementRow>, ChannelError>>>,
        prepare,
        dispatch,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn serve_with_inventory<S, T, A, I, P, D, O>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    cancel: CancelToken,
    current: impl Fn(ManagementRoute) -> A,
    inventory: impl FnOnce(ListMode) -> I,
    prepare: impl FnOnce(ManagementRequest) -> P,
    dispatch: impl FnOnce(T, ManagementAction, [u8; 32], CancelToken, Instant) -> D,
) -> Result<(), ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    A: Future<Output = bool>,
    I: Future<Output = Result<Vec<ManagementRow>, ChannelError>>,
    P: Future<Output = Option<([u8; 32], T)>>,
    D: Future<Output = O>,
    O: Into<ManagementDispatch>,
{
    serve_inner(
        io,
        identity,
        pin,
        verified,
        cancel,
        current,
        Some(inventory),
        prepare,
        dispatch,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_inner<S, T, A, I, P, D, O>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    mut cancel: CancelToken,
    current: impl Fn(ManagementRoute) -> A,
    inventory: Option<impl FnOnce(ListMode) -> I>,
    prepare: impl FnOnce(ManagementRequest) -> P,
    dispatch: impl FnOnce(T, ManagementAction, [u8; 32], CancelToken, Instant) -> D,
) -> Result<(), ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    A: Future<Output = bool>,
    I: Future<Output = Result<Vec<ManagementRow>, ChannelError>>,
    P: Future<Output = Option<([u8; 32], T)>>,
    D: Future<Output = O>,
    O: Into<ManagementDispatch>,
{
    let VerifiedManagementRoute { route, end } = verified;
    bounded(end, &mut cancel, admitted(current(route))).await?;
    let mut tls = crate::tls::accept_protocol(
        io,
        identity,
        pin,
        handshake_budget(end),
        cancel.clone(),
        ALPN,
    )
    .await
    .map_err(|_| Unavailable)?;
    if tls._context != (Role::Host, route.host, Role::Client, route.phone) {
        return Err(Denied);
    }
    let allowed = if let Some(inventory) = inventory {
        bounded(end, &mut cancel, admitted(current(route))).await?;
        let mode = decode_mode(bounded(end, &mut cancel, take::<1>(&mut tls._io)).await?)?;
        let rows = bounded(end, &mut cancel, async { inventory(mode).await }).await?;
        validate_mode(&rows, mode)?;
        bounded(end, &mut cancel, admitted(current(route))).await?;
        let allowed = rows
            .iter()
            .map(|row| row.reference)
            .collect::<std::collections::BTreeSet<_>>();
        bounded(end, &mut cancel, write_inventory(&mut tls._io, rows)).await?;
        Some((allowed, mode))
    } else {
        None
    };
    let request = bounded(end, &mut cancel, ManagementRequest::receive(&mut tls._io)).await?;
    if allowed.is_some_and(|(refs, mode)| {
        !refs.contains(&request.reference)
            || (mode == ListMode::Archived) != matches!(request.action, ManagementAction::Restore)
    }) {
        return Err(Denied);
    }
    bounded(end, &mut cancel, admitted(current(route))).await?;
    let action = request.action.clone();
    let reconnect = matches!(action, ManagementAction::Reconnect);
    let prepared = bounded(end, &mut cancel, async { Ok(prepare(request).await) }).await?;
    bounded(end, &mut cancel, admitted(current(route))).await?;
    let outcome = if let Some((token, opaque)) = prepared {
        let mut challenge = [0; 33];
        challenge[0] = 1;
        challenge[1..].copy_from_slice(&token);
        bounded(end, &mut cancel, send(&mut tls._io, &challenge)).await?;
        let proof = bounded(end, &mut cancel, take::<32>(&mut tls._io)).await?;
        bounded(end, &mut cancel, admitted(current(route))).await?;
        let provider_cancel = cancel.clone();
        bounded(end, &mut cancel, async {
            Ok(dispatch(opaque, action, proof, provider_cancel, end)
                .await
                .into())
        })
        .await
        .map_err(|_| OutcomeUnknown)?
    } else {
        bounded(end, &mut cancel, send(&mut tls._io, &[0])).await?;
        ManagementDispatch::Outcome(ManagementOutcome::Denied)
    };
    match outcome {
        ManagementDispatch::Outcome(outcome) => {
            if reconnect && outcome == ManagementOutcome::Applied {
                return Err(Denied);
            }
            bounded(end, &mut cancel, send(&mut tls._io, &[outcome as u8]))
                .await
                .map_err(|_| OutcomeUnknown)?;
        }
        ManagementDispatch::Projection(source) if reconnect => {
            let mut sent = false;
            loop {
                bounded(end, &mut cancel, admitted(current(route))).await?;
                let capture = source.clone();
                let frame = bounded(end, &mut cancel, async {
                    tokio::task::spawn_blocking(move || capture())
                        .await
                        .map_err(|_| Unavailable)
                })
                .await?;
                bounded(end, &mut cancel, admitted(current(route))).await?;
                let Some(frame) = frame else {
                    let status = if sent {
                        VIEW_ENDED
                    } else {
                        ManagementOutcome::Denied as u8
                    };
                    bounded(end, &mut cancel, send(&mut tls._io, &[status])).await?;
                    break;
                };
                if !valid_view(&frame) {
                    return Err(Denied);
                }
                let mut bytes = Vec::with_capacity(5 + frame.len());
                bytes.push(VIEW_FRAME);
                bytes.extend_from_slice(&(frame.len() as u32).to_be_bytes());
                bytes.extend_from_slice(frame.as_bytes());
                bounded(end, &mut cancel, send(&mut tls._io, &bytes)).await?;
                sent = true;
                match bounded(end, &mut cancel, take::<1>(&mut tls._io)).await? {
                    [ACK] => return Ok(()),
                    [NEXT_VIEW] => {
                        bounded(end, &mut cancel, async {
                            sleep_until(Instant::now() + Duration::from_millis(500)).await;
                            Ok(())
                        })
                        .await?;
                    }
                    _ => return Err(Denied),
                }
            }
        }
        ManagementDispatch::Projection(_) => return Err(Denied),
    }
    let ack = bounded(end, &mut cancel, take::<1>(&mut tls._io))
        .await
        .map_err(|_| OutcomeUnknown)?;
    if ack != [ACK] {
        return Err(OutcomeUnknown);
    }
    Ok(())
}

/// A lost result after the challenge is echoed stays uncertain; never
/// automatically repeat an operation that may have committed.
pub async fn invoke<S>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    cancel: CancelToken,
    request: ManagementRequest,
) -> Result<ManagementOutcome, ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    invoke_inner(
        io,
        identity,
        pin,
        verified,
        cancel,
        Some(request),
        None,
        None::<fn(Vec<ManagementRow>) -> std::future::Ready<Option<ManagementRequest>>>,
        None::<fn(String) -> bool>,
    )
    .await
}

pub async fn invoke_from_inventory<S, I>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    cancel: CancelToken,
    mode: ListMode,
    choose: impl FnOnce(Vec<ManagementRow>) -> I,
) -> Result<ManagementOutcome, ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: Future<Output = Option<ManagementRequest>>,
{
    invoke_inner(
        io,
        identity,
        pin,
        verified,
        cancel,
        None,
        Some(mode),
        Some(choose),
        None::<fn(String) -> bool>,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn invoke_from_inventory_with_projection<S, I>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    cancel: CancelToken,
    mode: ListMode,
    choose: impl FnOnce(Vec<ManagementRow>) -> I,
    project: impl FnMut(String) -> bool,
) -> Result<ManagementOutcome, ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: Future<Output = Option<ManagementRequest>>,
{
    invoke_inner(
        io,
        identity,
        pin,
        verified,
        cancel,
        None,
        Some(mode),
        Some(choose),
        Some(project),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn invoke_inner<S, I>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    verified: VerifiedManagementRoute,
    mut cancel: CancelToken,
    request: Option<ManagementRequest>,
    mode: Option<ListMode>,
    choose: Option<impl FnOnce(Vec<ManagementRow>) -> I>,
    mut project: Option<impl FnMut(String) -> bool>,
) -> Result<ManagementOutcome, ChannelError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    I: Future<Output = Option<ManagementRequest>>,
{
    let VerifiedManagementRoute { route, end } = verified;
    if pin.endpoint_id() != route.host {
        return Err(Denied);
    }
    let mut tls = crate::tls::connect_protocol(
        io,
        identity,
        pin,
        handshake_budget(end),
        cancel.clone(),
        ALPN,
    )
    .await
    .map_err(|_| Unavailable)?;
    if tls._context != (Role::Client, route.phone, Role::Host, route.host) {
        return Err(Denied);
    }
    let request = if let Some(choose) = choose {
        let mode = mode.ok_or(Denied)?;
        bounded(end, &mut cancel, send(&mut tls._io, &[encode_mode(mode)])).await?;
        let rows = bounded(end, &mut cancel, read_inventory(&mut tls._io)).await?;
        validate_mode(&rows, mode)?;
        let allowed = rows
            .iter()
            .map(|row| row.reference)
            .collect::<std::collections::BTreeSet<_>>();
        let picked = bounded(end, &mut cancel, async { choose(rows).await.ok_or(Denied) }).await?;
        if !allowed.contains(&picked.reference)
            || (mode == ListMode::Archived) != matches!(picked.action, ManagementAction::Restore)
        {
            return Err(Denied);
        }
        picked
    } else {
        request.ok_or(Denied)?
    };
    let reconnect = matches!(request.action, ManagementAction::Reconnect);
    let request = request.encode()?;
    bounded(end, &mut cancel, send(&mut tls._io, &request)).await?;
    let challenge = bounded(end, &mut cancel, take::<1>(&mut tls._io)).await?;
    match challenge {
        [0] => {}
        [1] => {
            let token = bounded(end, &mut cancel, take::<32>(&mut tls._io)).await?;
            bounded(end, &mut cancel, send(&mut tls._io, &token))
                .await
                .map_err(|_| OutcomeUnknown)?;
        }
        _ => return Err(Denied),
    }
    let mut showed = false;
    let outcome = loop {
        let state = bounded(end, &mut cancel, take::<1>(&mut tls._io))
            .await
            .map_err(|_| OutcomeUnknown)?;
        match (challenge, state) {
            ([1], [1]) if !reconnect => break ManagementOutcome::Applied,
            (_, [2]) if !showed => break ManagementOutcome::Denied,
            ([1], [3]) if !showed => break ManagementOutcome::Unknown,
            ([1], [VIEW_ENDED]) if reconnect && showed => break ManagementOutcome::Applied,
            ([1], [VIEW_FRAME]) if reconnect => {
                let project = project.as_mut().ok_or(Denied)?;
                let frame = bounded(end, &mut cancel, async {
                    let n = tls._io.read_u32().await.map_err(io_failure)? as usize;
                    if n > MAX_VIEW_BYTES {
                        return Err(Denied);
                    }
                    let mut bytes = vec![0; n];
                    tls._io.read_exact(&mut bytes).await.map_err(io_failure)?;
                    let frame = String::from_utf8(bytes).map_err(|_| Denied)?;
                    if !valid_view(&frame) {
                        return Err(Denied);
                    }
                    Ok(frame)
                })
                .await?;
                showed = true;
                if !project(frame) {
                    break ManagementOutcome::Applied;
                }
                bounded(end, &mut cancel, send(&mut tls._io, &[NEXT_VIEW])).await?;
            }
            _ => return Err(OutcomeUnknown),
        }
    };
    bounded(end, &mut cancel, async {
        send(&mut tls._io, &[ACK]).await?;
        // The relay must forward our receipt before we release this stream.
        match tls._io.read(&mut [0]).await {
            Ok(0) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
            _ => Err(OutcomeUnknown),
        }
    })
    .await
    .map_err(|_| OutcomeUnknown)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_management::{CurrentManagementScope, ManagementGrant};
    use rustls::SignatureScheme;
    use std::future::ready;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture {
        host: LocalIdentity,
        phone: LocalIdentity,
        host_pin: PeerPin,
        phone_pin: PeerPin,
        route: ManagementRoute,
        host_key: P256Spki,
        advertised: String,
        signature: String,
    }

    // rustls fixtures sign ASN.1 for TLS, while management tickets use
    // the same fixed-width P-256 encoding as native desktop signing.
    fn fixed_signature(der: Vec<u8>) -> [u8; 64] {
        assert_eq!(der[0], 0x30);
        assert_eq!(usize::from(der[1]), der.len() - 2);
        let mut rest = &der[2..];
        let mut out = [0; 64];
        for part in out.chunks_exact_mut(32) {
            assert_eq!(rest[0], 2);
            let size = usize::from(rest[1]);
            let (integer, next) = rest[2..].split_at(size);
            let value = integer.strip_prefix(&[0]).unwrap_or(integer);
            part[32 - value.len()..].copy_from_slice(value);
            rest = next;
        }
        assert!(rest.is_empty());
        out
    }

    fn fixture() -> Fixture {
        let (host, hp) = crate::test_identities::identity(false, false);
        let (phone, pp) = crate::test_identities::identity(true, false);
        let host_key = P256Spki::new(&hp).unwrap();
        let phone_key = P256Spki::new(&pp).unwrap();
        let host_pin = PeerPin::new(Role::Host, host_key.clone());
        let phone_pin = PeerPin::new(Role::Client, phone_key.clone());
        let offer = ManagementOffer {
            tunnel: [1; 16],
            offer: [2; 16],
            membership_binding: [4; 32],
            host: host_pin.endpoint_id(),
            phone: phone_pin.endpoint_id(),
        };
        let sign = host
            .key
            .choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap();
        let advertised = offer.envelope(
            22_000,
            fixed_signature(sign.sign(&offer.signing_bytes(22_000)).unwrap()),
        );
        let digest = ring::digest::digest(&ring::digest::SHA256, advertised.as_bytes());
        let route = offer.selection(digest.as_ref().try_into().unwrap(), [3; 16]);
        let bytes = sign.sign(&route.signing_bytes(22_000)).unwrap();
        let signature = ManagementRoute::envelope(22_000, fixed_signature(bytes));
        Fixture {
            host: LocalIdentity::new(Role::Host, host_key.clone(), host.key.clone()).unwrap(),
            phone: LocalIdentity::new(Role::Client, phone_key, phone.key.clone()).unwrap(),
            host_pin,
            phone_pin,
            route,
            host_key,
            advertised,
            signature,
        }
    }

    fn verified(f: &Fixture) -> VerifiedManagementRoute {
        verify_offer(
            &f.host_key,
            f.route.tunnel,
            f.route.membership_binding,
            f.route.phone,
            f.route.offer_digest,
            &f.advertised,
            1,
        )
        .unwrap()
        .verify_selection(f.route.selection, &f.signature, 1)
        .unwrap()
    }

    #[test]
    fn management_route_requires_exact_signed_selection_and_independent_purpose() {
        let f = fixture();
        assert!(verify_route(&f.host_key, f.route, &f.signature, 1).is_ok());
        assert!(verify_route(&f.host_key, f.route, &f.signature, 22_000).is_err());
        for changed in [
            ManagementRoute {
                tunnel: [9; 16],
                ..f.route
            },
            ManagementRoute {
                offer: [9; 16],
                ..f.route
            },
            ManagementRoute {
                offer_digest: [9; 32],
                ..f.route
            },
            ManagementRoute {
                selection: [9; 16],
                ..f.route
            },
            ManagementRoute {
                membership_binding: [9; 32],
                ..f.route
            },
            ManagementRoute {
                phone: [9; 32],
                ..f.route
            },
        ] {
            assert!(verify_route(&f.host_key, changed, &f.signature, 1).is_err());
        }
        let (host, _) = crate::test_identities::identity(false, false);
        let sign = host
            .key
            .choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap();
        let msg = f.route.signing_bytes(10_000);
        let wrong = [
            b"cc-desk.catalogue-ready.read-only.v1\0".as_slice(),
            &msg[DOMAIN.len()..],
        ]
        .concat();
        let wire = ManagementRoute::envelope(10_000, fixed_signature(sign.sign(&wrong).unwrap()));
        assert!(verify_route(&f.host_key, f.route, &wire, 1).is_err());
    }

    #[test]
    fn management_offer_ready_is_bound_to_the_signed_advertisement_and_earlier_expiry() {
        let f = fixture();
        let offer = ManagementOffer {
            tunnel: f.route.tunnel,
            membership_binding: f.route.membership_binding,
            offer: f.route.offer,
            host: f.route.host,
            phone: f.route.phone,
        };
        let (host, _) = crate::test_identities::identity(false, false);
        let signer = host
            .key
            .choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256])
            .unwrap();
        let signed = offer.envelope(
            10_000,
            fixed_signature(signer.sign(&offer.signing_bytes(10_000)).unwrap()),
        );
        let digest: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, signed.as_bytes())
            .as_ref()
            .try_into()
            .unwrap();
        let received = verify_offer(
            &f.host_key,
            offer.tunnel,
            offer.membership_binding,
            offer.phone,
            digest,
            &signed,
            1,
        )
        .unwrap();
        assert!(received.offer() == offer);
        assert!(
            verify_offer(
                &f.host_key,
                offer.tunnel,
                offer.membership_binding,
                offer.phone,
                [9; 32],
                &signed,
                1
            )
            .is_err()
        );
        let route = offer.selection(digest, f.route.selection);
        let ready = |exp| {
            ManagementRoute::envelope(
                exp,
                fixed_signature(signer.sign(&route.signing_bytes(exp)).unwrap()),
            )
        };
        assert!(
            received
                .verify_selection(route.selection, &ready(10_000), 1)
                .is_ok()
        );
        assert!(
            received
                .verify_selection(route.selection, &ready(10_001), 1)
                .is_err()
        );
        assert!(
            received
                .verify_selection([9; 16], &ready(10_000), 1)
                .is_err()
        );
        let wrong_route = offer.selection([9; 32], route.selection);
        let wrong = ManagementRoute::envelope(
            10_000,
            fixed_signature(signer.sign(&wrong_route.signing_bytes(10_000)).unwrap()),
        );
        assert!(
            received
                .verify_selection(route.selection, &wrong, 1)
                .is_err()
        );
    }

    #[tokio::test]
    async fn management_tls_one_operation_and_revocation_before_dispatch() {
        for revoke in [false, true] {
            let f = fixture();
            let host_proof = verified(&f);
            let phone_proof = verified(&f);
            let (_host_handle, host_cancel) = crate::cancellation();
            let (_phone_handle, phone_cancel) = crate::cancellation();
            let (a, b) = tokio::io::duplex(8192);
            let checks = AtomicUsize::new(0);
            let changes = AtomicUsize::new(0);
            let scope = CurrentManagementScope {
                peer: f.route.phone,
                membership: f.route.membership_binding,
                document: [1; 32],
                project: [2; 32],
                source: [3; 32],
                profile_revision: 4,
                session: [5; 32],
                run: None,
            };
            let (server, phone) = tokio::join!(
                serve(
                    Unbound::new(a),
                    f.host,
                    f.phone_pin,
                    host_proof,
                    host_cancel,
                    |route| ready(
                        route == f.route && (!revoke || checks.fetch_add(1, Ordering::SeqCst) < 3)
                    ),
                    |request| {
                        assert_eq!(request.reference, [7; 32]);
                        let grant =
                            ManagementGrant::issue(scope, request.action, "local-receipt").unwrap();
                        ready(Some((grant.token(), grant)))
                    },
                    |mut grant, action, token, cancel, end| {
                        assert!(!cancel.is_cancelled() && Instant::now() < end);
                        assert_eq!(
                            grant.redeem(Some(&scope), &token, &action).unwrap(),
                            "local-receipt"
                        );
                        assert!(grant.redeem(Some(&scope), &token, &action).is_err());
                        changes.fetch_add(1, Ordering::SeqCst);
                        ready(ManagementOutcome::Applied)
                    },
                ),
                invoke(
                    Unbound::new(b),
                    f.phone,
                    f.host_pin,
                    phone_proof,
                    phone_cancel,
                    ManagementRequest {
                        reference: [7; 32],
                        action: ManagementAction::Rename {
                            title: "Scoped".into()
                        }
                    }
                ),
            );
            if revoke {
                assert_eq!(server, Err(Denied));
                assert_eq!(phone, Err(OutcomeUnknown));
                assert_eq!(changes.load(Ordering::SeqCst), 0);
            } else {
                assert_eq!(server, Ok(()));
                assert_eq!(phone, Ok(ManagementOutcome::Applied));
                assert_eq!(changes.load(Ordering::SeqCst), 1);
            }
        }
    }

    #[tokio::test]
    async fn management_inventory_exposes_only_listed_reference_for_one_operation() {
        use crate::session_catalogue::DisplayLifecycle;
        let f = fixture();
        let host_proof = verified(&f);
        let phone_proof = verified(&f);
        let (_ha, hc) = crate::cancellation();
        let (_pa, pc) = crate::cancellation();
        let (a, b) = tokio::io::duplex(8192);
        let row = ManagementRow {
            reference: [8; 32],
            title: "Scoped".into(),
            lifecycle: DisplayLifecycle::Closed,
            archived: false,
        };
        let outcome = tokio::join!(
            serve_with_inventory(
                Unbound::new(a),
                f.host,
                f.phone_pin,
                host_proof,
                hc,
                |_| ready(true),
                |_| ready(Ok(vec![row.clone()])),
                |request| {
                    assert_eq!(request.reference, [8; 32]);
                    ready(Some(([7; 32], request.reference)))
                },
                |selected, _, proof, _, _| {
                    assert_eq!(selected, [8; 32]);
                    assert_eq!(proof, [7; 32]);
                    ready(ManagementOutcome::Applied)
                }
            ),
            invoke_from_inventory(
                Unbound::new(b),
                f.phone,
                f.host_pin,
                phone_proof,
                pc,
                ListMode::Sessions,
                |list| {
                    assert_eq!(list, vec![row.clone()]);
                    ready(Some(ManagementRequest {
                        reference: list[0].reference,
                        action: ManagementAction::Rename {
                            title: "New".into(),
                        },
                    }))
                }
            )
        );
        assert_eq!(outcome, (Ok(()), Ok(ManagementOutcome::Applied)));
        assert!(validate_inventory(&[row.clone(), row]).is_err());
    }

    #[tokio::test]
    async fn management_archive_inventory_needs_separate_mode_and_never_leaks() {
        use crate::session_catalogue::{DisplayLifecycle, ListMode};
        for (mode, leaked) in [
            (ListMode::Sessions, false),
            (ListMode::Archived, false),
            (ListMode::Sessions, true),
            (ListMode::Archived, true),
        ] {
            let f = fixture();
            let host_proof = verified(&f);
            let phone_proof = verified(&f);
            let (_ha, hc) = crate::cancellation();
            let (_pa, pc) = crate::cancellation();
            let (a, b) = tokio::io::duplex(8192);
            let expected = mode == ListMode::Archived;
            let (server, client) = tokio::join!(
                serve_with_inventory(
                    Unbound::new(a),
                    f.host,
                    f.phone_pin,
                    host_proof,
                    hc,
                    |_| ready(true),
                    |requested| {
                        assert_eq!(requested, mode);
                        ready(Ok(vec![ManagementRow {
                            reference: [8; 32],
                            title: "Native owned".into(),
                            lifecycle: DisplayLifecycle::Closed,
                            archived: expected != leaked,
                        }]))
                    },
                    |request| {
                        assert!(!leaked);
                        assert_eq!(request.reference, [8; 32]);
                        ready(Some(([7; 32], ())))
                    },
                    |_, _, _, _, _| ready(ManagementOutcome::Applied),
                ),
                invoke_from_inventory(
                    Unbound::new(b),
                    f.phone,
                    f.host_pin,
                    phone_proof,
                    pc,
                    mode,
                    |rows| {
                        assert!(!leaked);
                        assert_eq!(rows[0].archived, expected);
                        ready(Some(ManagementRequest {
                            reference: rows[0].reference,
                            action: if expected {
                                ManagementAction::Restore
                            } else {
                                ManagementAction::Archive
                            },
                        }))
                    },
                ),
            );
            if leaked {
                assert_eq!(server, Err(Denied));
                assert!(client.is_err());
            } else {
                assert_eq!((server, client), (Ok(()), Ok(ManagementOutcome::Applied)));
            }
        }
    }

    #[tokio::test]
    async fn management_projection_stream_only_reconnect_and_stops_on_revocation() {
        use crate::session_catalogue::DisplayLifecycle;
        for (reconnect, revoke) in [(true, false), (true, true), (false, false)] {
            let f = fixture();
            let host_proof = verified(&f);
            let phone_proof = verified(&f);
            let (_ha, hc) = crate::cancellation();
            let (_pa, pc) = crate::cancellation();
            let (a, b) = tokio::io::duplex(8192);
            let live = std::sync::atomic::AtomicBool::new(true);
            let captures = std::sync::Arc::new(AtomicUsize::new(0));
            let source = captures.clone();
            let mut frames = Vec::new();
            let (server, client) = tokio::join!(
                serve_with_inventory(
                    Unbound::new(a),
                    f.host,
                    f.phone_pin,
                    host_proof,
                    hc,
                    |_| ready(live.load(Ordering::Acquire)),
                    |_| ready(Ok(vec![ManagementRow {
                        reference: [8; 32],
                        title: "Original".into(),
                        lifecycle: DisplayLifecycle::Open,
                        archived: false,
                    }])),
                    |_| ready(Some(([7; 32], ()))),
                    |_, _, _, _, _| ready(ManagementDispatch::Projection(std::sync::Arc::new(
                        move || {
                            Some(format!("frame {}", source.fetch_add(1, Ordering::SeqCst)))
                        }
                    ))),
                ),
                invoke_from_inventory_with_projection(
                    Unbound::new(b),
                    f.phone,
                    f.host_pin,
                    phone_proof,
                    pc,
                    ListMode::Sessions,
                    |list| ready(Some(ManagementRequest {
                        reference: list[0].reference,
                        action: if reconnect {
                            ManagementAction::Reconnect
                        } else {
                            ManagementAction::Rename {
                                title: "name".into(),
                            }
                        },
                    })),
                    |frame| {
                        frames.push(frame);
                        if revoke {
                            live.store(false, Ordering::Release);
                        }
                        frames.len() < 2
                    },
                ),
            );
            if reconnect && !revoke {
                assert_eq!(server, Ok(()));
                assert_eq!(client, Ok(ManagementOutcome::Applied));
                assert_eq!(frames, ["frame 0", "frame 1"]);
            } else if revoke {
                assert_eq!(server, Err(Denied));
                assert!(client.is_err());
                assert_eq!(frames, ["frame 0"]);
                assert_eq!(captures.load(Ordering::SeqCst), 1);
            } else {
                assert!(server.is_err());
                assert!(client.is_err());
                assert!(frames.is_empty());
                assert_eq!(captures.load(Ordering::SeqCst), 0);
            }
        }
    }

    #[tokio::test]
    async fn management_tls_does_not_negotiate_read_only_alpn() {
        let f = fixture();
        let route = verified(&f);
        let (_host_handle, host_cancel) = crate::cancellation();
        let (_phone_handle, phone_cancel) = crate::cancellation();
        let (a, b) = tokio::io::duplex(8192);
        let (server, phone) = tokio::join!(
            serve(
                Unbound::new(a),
                f.host,
                f.phone_pin,
                route,
                host_cancel,
                |_| ready(true),
                |_| -> std::future::Ready<Option<([u8; 32], ())>> {
                    panic!("incorrect ALPN reached prepare")
                },
                |_, _, _, _, _| -> std::future::Ready<ManagementOutcome> {
                    panic!("incorrect ALPN reached dispatch")
                },
            ),
            crate::tls::connect_protocol(
                Unbound::new(b),
                f.phone,
                f.host_pin,
                Duration::from_secs(2),
                phone_cancel,
                b"cc-desk-catalogue-read/1"
            ),
        );
        assert!(server.is_err() && phone.is_err());
    }
}
