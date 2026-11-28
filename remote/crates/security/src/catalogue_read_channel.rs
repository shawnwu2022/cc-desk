//! A one-request, mutually pinned, read-only catalogue protocol.
//! It neither negotiates session control nor accepts native selectors.

use crate::owner_clock::Instant;
use crate::session_catalogue::{DisplayRow, ListMode, valid_display_row};
use crate::{CancelToken, LocalIdentity, PeerPin, Pending, Role, Unbound};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const ALPN: &[u8] = b"cc-desk-catalogue-read/1";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_ROWS: usize = 8192;
const LIMIT: Duration = Duration::from_secs(10);
const REPLY_READ: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CataloguePair {
    pub host: [u8; 32],
    pub client: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Transport,
    Denied,
    Timeout,
    Cancelled,
    InvalidMessage,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    mode: ListMode,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    mode: ListMode,
    rows: Vec<DisplayRow>,
}

async fn bounded<T>(
    end: Instant,
    cancel: &mut CancelToken,
    op: impl Future<Output = Result<T, ReadError>>,
) -> Result<T, ReadError> {
    if cancel.is_cancelled() {
        return Err(ReadError::Cancelled);
    }
    if Instant::now() >= end {
        return Err(ReadError::Timeout);
    }
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ReadError::Cancelled),
        _ = crate::owner_clock::sleep_until(end) => Err(ReadError::Timeout),
        r = op => r,
    }?;
    if cancel.is_cancelled() {
        return Err(ReadError::Cancelled);
    }
    if Instant::now() >= end {
        return Err(ReadError::Timeout);
    }
    Ok(result)
}

async fn require_current(
    end: Instant,
    cancel: &mut CancelToken,
    current: impl Future<Output = bool>,
) -> Result<(), ReadError> {
    bounded(end, cancel, async {
        if current.await {
            Ok(())
        } else {
            Err(ReadError::Denied)
        }
    })
    .await
}

fn verified_peer<S>(p: &Pending<S>, pair: CataloguePair, expected: Role) -> Result<(), ReadError> {
    let parties = match p._context {
        (Role::Host, host, Role::Client, client) if expected == Role::Host => {
            CataloguePair { host, client }
        }
        (Role::Client, client, Role::Host, host) if expected == Role::Client => {
            CataloguePair { host, client }
        }
        _ => return Err(ReadError::Denied),
    };
    if parties != pair {
        return Err(ReadError::Denied);
    }
    Ok(())
}

async fn send(io: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> Result<(), ReadError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(ReadError::InvalidMessage);
    }
    io.write_u32(bytes.len() as u32)
        .await
        .map_err(|_| ReadError::Transport)?;
    io.write_all(bytes)
        .await
        .map_err(|_| ReadError::Transport)?;
    io.flush().await.map_err(|_| ReadError::Transport)
}

async fn receive(io: &mut (impl AsyncRead + Unpin), max: usize) -> Result<Vec<u8>, ReadError> {
    let len = io.read_u32().await.map_err(|_| ReadError::Transport)? as usize;
    if len == 0 || len > max {
        return Err(ReadError::InvalidMessage);
    }
    let mut bytes = vec![0; len];
    io.read_exact(&mut bytes)
        .await
        .map_err(|_| ReadError::Transport)?;
    Ok(bytes)
}

fn validate_rows(mode: ListMode, rows: &[DisplayRow]) -> Result<(), ReadError> {
    if rows.len() > MAX_ROWS || rows.iter().any(|row| !valid_display_row(mode, row)) {
        return Err(ReadError::InvalidMessage);
    }
    Ok(())
}

/// Pair must come from the host's current local pairing state. The attached
/// cancellation must retire with that pairing; TLS possession alone is not a grant.
pub async fn serve<S, F, A>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    pair: CataloguePair,
    mut cancel: CancelToken,
    current: impl Fn(CataloguePair) -> A,
    list: impl FnOnce(ListMode) -> F,
) -> Result<(), ReadError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    F: Future<Output = Result<Vec<DisplayRow>, ReadError>>,
    A: Future<Output = bool>,
{
    let end = Instant::now() + LIMIT;
    require_current(end, &mut cancel, current(pair)).await?;
    let mut channel = crate::tls::accept_protocol(io, identity, pin, LIMIT, cancel.clone(), ALPN)
        .await
        .map_err(|_| ReadError::Transport)?;
    verified_peer(&channel, pair, Role::Host)?;
    let query: Query =
        serde_json::from_slice(&bounded(end, &mut cancel, receive(&mut channel._io, 128)).await?)
            .map_err(|_| ReadError::InvalidMessage)?;
    require_current(end, &mut cancel, current(pair)).await?;
    let rows = bounded(end, &mut cancel, list(query.mode)).await?;
    validate_rows(query.mode, &rows)?;
    let bytes = serde_json::to_vec(&Reply {
        mode: query.mode,
        rows,
    })
    .map_err(|_| ReadError::InvalidMessage)?;
    require_current(end, &mut cancel, current(pair)).await?;
    bounded(end, &mut cancel, send(&mut channel._io, &bytes)).await?;
    // A flushed TLS write only reached the local mux. Wait for the phone to
    // validate the entire frame before dropping the stream and its relay route.
    bounded(end, &mut cancel, async {
        match channel._io.read_u8().await {
            Ok(REPLY_READ) => Ok(()),
            Ok(_) => Err(ReadError::InvalidMessage),
            Err(_) => Err(ReadError::Transport),
        }
    })
    .await
}

/// Fetch only after verifying signed readiness for this catalogue selection.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_admitted<S>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    pair: CataloguePair,
    route_tunnel: [u8; 16],
    ready: crate::catalogue_rendezvous::VerifiedCatalogueReady,
    now_unix_ms: u64,
    cancel: CancelToken,
    mode: ListMode,
) -> Result<Vec<DisplayRow>, ReadError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let selection = ready.selection();
    if mode != ready.mode()
        || route_tunnel != selection.tunnel
        || pair.host != ready.host_id()
        || pin.endpoint_id() != pair.host
    {
        return Err(ReadError::Denied);
    }
    let remaining = ready
        .expires_unix_ms()
        .checked_sub(now_unix_ms)
        .filter(|time| *time != 0)
        .ok_or(ReadError::Timeout)?;
    fetch(
        io,
        identity,
        pin,
        pair,
        cancel,
        mode,
        Duration::from_millis(remaining).min(LIMIT),
    )
    .await
}

async fn fetch<S>(
    io: Unbound<S>,
    identity: LocalIdentity,
    pin: PeerPin,
    pair: CataloguePair,
    mut cancel: CancelToken,
    mode: ListMode,
    limit: Duration,
) -> Result<Vec<DisplayRow>, ReadError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let end = Instant::now() + limit;
    let mut channel = crate::tls::connect_protocol(io, identity, pin, limit, cancel.clone(), ALPN)
        .await
        .map_err(|_| ReadError::Transport)?;
    verified_peer(&channel, pair, Role::Client)?;
    let bytes = serde_json::to_vec(&Query { mode }).map_err(|_| ReadError::InvalidMessage)?;
    bounded(end, &mut cancel, send(&mut channel._io, &bytes)).await?;
    let reply: Reply = serde_json::from_slice(
        &bounded(end, &mut cancel, receive(&mut channel._io, MAX_BYTES)).await?,
    )
    .map_err(|_| ReadError::InvalidMessage)?;
    if reply.mode != mode {
        return Err(ReadError::InvalidMessage);
    }
    validate_rows(mode, &reply.rows)?;
    bounded(end, &mut cancel, async {
        channel
            ._io
            .write_u8(REPLY_READ)
            .await
            .map_err(|_| ReadError::Transport)?;
        channel
            ._io
            .flush()
            .await
            .map_err(|_| ReadError::Transport)?;
        // Keep our route alive long enough to forward the receipt. The reply
        // is already authenticated; the peer may drop the mux without a TLS
        // close-notify. Any further application bytes violate this protocol.
        match channel._io.read(&mut [0]).await {
            Ok(0) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
            Ok(_) => Err(ReadError::InvalidMessage),
            Err(_) => Err(ReadError::Transport),
        }
    })
    .await?;
    Ok(reply.rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_catalogue::DisplayLifecycle;

    fn identities() -> (
        LocalIdentity,
        PeerPin,
        LocalIdentity,
        PeerPin,
        CataloguePair,
    ) {
        let (host, host_key) = crate::test_identities::identity(false, false);
        let (client, client_key) = crate::test_identities::identity(true, false);
        let host_public = crate::P256Spki::new(&host_key).unwrap();
        let client_public = crate::P256Spki::new(&client_key).unwrap();
        let host_pin = PeerPin::new(Role::Host, host_public.clone());
        let client_pin = PeerPin::new(Role::Client, client_public.clone());
        let pair = CataloguePair {
            host: host_pin.endpoint_id(),
            client: client_pin.endpoint_id(),
        };
        (
            LocalIdentity::new(Role::Host, host_public, host.key.clone()).unwrap(),
            host_pin,
            LocalIdentity::new(Role::Client, client_public, client.key.clone()).unwrap(),
            client_pin,
            pair,
        )
    }

    #[tokio::test]
    async fn pinned_catalogue_is_read_only_and_bound_to_requested_mode() {
        let (host, host_pin, client, client_pin, pair) = identities();
        let (a, b) = tokio::io::duplex(8192);
        let (_ha, hc) = crate::cancellation();
        let (_ca, cc) = crate::cancellation();
        let expected = DisplayRow {
            lifecycle: DisplayLifecycle::History,
            title: "archive".into(),
        };
        let (result, rows) = tokio::join!(
            serve(
                Unbound::new(a),
                host,
                client_pin,
                pair,
                hc,
                |p| std::future::ready(p == pair),
                |mode| {
                    assert_eq!(mode, ListMode::Archived);
                    std::future::ready(Ok(vec![expected.clone()]))
                }
            ),
            fetch(
                Unbound::new(b),
                client,
                host_pin,
                pair,
                cc,
                ListMode::Archived,
                LIMIT
            ),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(rows.unwrap(), vec![expected]);
        assert!(serde_json::from_str::<Query>(r#"{"mode":"archived","action":"resume"}"#).is_err());
    }

    #[tokio::test]
    async fn catalogue_host_waits_for_the_read_receipt_before_retiring() {
        let (host, host_pin, client, client_pin, pair) = identities();
        let (a, b) = tokio::io::duplex(8192);
        let (_ha, hc) = crate::cancellation();
        let (_ca, cc) = crate::cancellation();
        let owner = tokio::spawn(serve(
            Unbound::new(a),
            host,
            client_pin,
            pair,
            hc,
            move |p| std::future::ready(p == pair),
            |_| std::future::ready(Ok(Vec::new())),
        ));
        let mut channel =
            crate::tls::connect_protocol(Unbound::new(b), client, host_pin, LIMIT, cc, ALPN)
                .await
                .unwrap();
        let request = serde_json::to_vec(&Query {
            mode: ListMode::Sessions,
        })
        .unwrap();
        send(&mut channel._io, &request).await.unwrap();
        let response = receive(&mut channel._io, MAX_BYTES).await.unwrap();
        let reply: Reply = serde_json::from_slice(&response).unwrap();
        assert_eq!(reply.mode, ListMode::Sessions);
        tokio::task::yield_now().await;
        assert!(
            !owner.is_finished(),
            "a local write must not retire the route"
        );
        drop(channel);
        assert_eq!(owner.await.unwrap(), Err(ReadError::Transport));
    }

    #[tokio::test]
    async fn pinned_connection_cannot_supply_a_different_pair() {
        let (host, host_pin, client, client_pin, pair) = identities();
        let (a, b) = tokio::io::duplex(8192);
        let (_ha, hc) = crate::cancellation();
        let (_ca, cc) = crate::cancellation();
        let wrong = CataloguePair {
            host: pair.host,
            client: [7; 32],
        };
        let (result, rows) = tokio::join!(
            serve(
                Unbound::new(a),
                host,
                client_pin,
                wrong,
                hc,
                |_| std::future::ready(true),
                |_| async { panic!("wrong paired identity must be rejected before listing") }
            ),
            fetch(
                Unbound::new(b),
                client,
                host_pin,
                pair,
                cc,
                ListMode::Sessions,
                LIMIT
            ),
        );
        assert_eq!(result, Err(ReadError::Denied));
        assert!(rows.is_err());
    }

    #[tokio::test]
    async fn revoked_pair_during_listing_receives_no_rows() {
        let (host, host_pin, client, client_pin, pair) = identities();
        let (a, b) = tokio::io::duplex(8192);
        let (_ha, hc) = crate::cancellation();
        let (_ca, cc) = crate::cancellation();
        let allowed = std::cell::Cell::new(true);
        let (result, rows) = tokio::join!(
            serve(
                Unbound::new(a),
                host,
                client_pin,
                pair,
                hc,
                |_| std::future::ready(allowed.get()),
                |_| async {
                    allowed.set(false);
                    Ok(vec![DisplayRow {
                        lifecycle: DisplayLifecycle::History,
                        title: "not delivered".into(),
                    }])
                }
            ),
            fetch(
                Unbound::new(b),
                client,
                host_pin,
                pair,
                cc,
                ListMode::Sessions,
                LIMIT
            ),
        );
        assert_eq!(result, Err(ReadError::Denied));
        assert!(rows.is_err());
    }
}
