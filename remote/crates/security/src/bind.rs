use crate::{CancelToken, Pending, Role};
use cc_desk_remote_record::{
    Header, Kind,
    appbind::{
        self, CanonicalOrigin, ClientConfirm, ClientOffer, CodecError, Context, HostConfirm,
        HostOffer, HostSelection, Mac, Message, RequestedRole,
    },
};
use ring::{hmac, rand::SecureRandom};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::oneshot,
    task::JoinHandle,
    time::Instant,
};
#[derive(Clone)]
pub struct HostSnapshot {
    pub host_key_epoch: u64,
    pub client_key_epoch: u64,
    pub activation: [u8; 32],
    pub incarnation: u64,
    pub policy_epoch: u64,
    pub issuer_epoch: u64,
    pub origin: CanonicalOrigin,
}
#[derive(Clone)]
pub struct ClientTrust {
    pub host_key_epoch: u64,
    pub client_key_epoch: u64,
    pub origin: CanonicalOrigin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    Codec,
    Unsupported,
    Context,
    Mac,
    Io,
    Deadline,
    Cancelled,
    Order,
    Nonce,
    Owner,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerExit {
    LocalClose,
    Cancelled,
    PeerClosed,
    Unsupported,
    Io,
    Aborted,
}
/// Local software binding only. No plaintext, session, grant or online-status API.
#[derive(Clone, PartialEq, Eq)]
pub struct BoundContext(Context);
impl std::fmt::Debug for BoundContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundContext(<verified>)")
    }
}
impl BoundContext {
    pub(crate) fn raw(&self) -> &Context {
        &self.0
    }
    pub fn policy_epoch(&self) -> &str {
        &self.0.policy_epoch
    }
    pub fn issuer_epoch(&self) -> &str {
        &self.0.issuer_epoch
    }
    pub fn host_endpoint_id(&self) -> &str {
        &self.0.host_endpoint_id
    }
    pub fn client_endpoint_id(&self) -> &str {
        &self.0.client_endpoint_id
    }
    pub fn origin(&self) -> &str {
        self.0.origin.as_str()
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct BoundSelection(HostSelection);
impl std::fmt::Debug for BoundSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundSelection(<verified-management-only>)")
    }
}
impl BoundSelection {
    pub fn profile(&self) -> u16 {
        self.0.profile
    }
    pub fn minor(&self) -> u16 {
        self.0.minor
    }
}
pub struct BoundManagement {
    context: BoundContext,
    selection: BoundSelection,
    task: Option<JoinHandle<OwnerExit>>,
    stop: Option<oneshot::Sender<()>>,
    exit: Option<OwnerExit>,
}
impl BoundManagement {
    pub fn context(&self) -> &BoundContext {
        &self.context
    }
    pub fn selection(&self) -> &BoundSelection {
        &self.selection
    }
    /// Waits for the actual owned task to exit. Dropping this future retains ownership.
    pub async fn wait_closed(&mut self) -> OwnerExit {
        if let Some(exit) = self.exit {
            return exit;
        }
        let exit = match self.task.as_mut() {
            Some(task) => task.await.unwrap_or(OwnerExit::Aborted),
            None => OwnerExit::Aborted,
        };
        self.task.take();
        self.exit = Some(exit);
        exit
    }
    pub async fn close(mut self) -> OwnerExit {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.wait_closed().await
    }
}
impl Drop for BoundManagement {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn nonce() -> Result<String, BindError> {
    let mut bytes = [0; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| BindError::Nonce)?;
    Ok(hex(&bytes))
}
fn projected<S>(pending: &Pending<S>) -> Result<(String, String), BindError> {
    let (local, lid, peer, pid) = pending._context;
    match (local, peer) {
        (Role::Host, Role::Client) => Ok((hex(&lid), hex(&pid))),
        (Role::Client, Role::Host) => Ok((hex(&pid), hex(&lid))),
        _ => Err(BindError::Context),
    }
}
fn codec(error: CodecError) -> BindError {
    if error == CodecError::Unsupported {
        BindError::Unsupported
    } else {
        BindError::Codec
    }
}
async fn send_with<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
    hooks: &Hooks,
    message: Message,
) -> Result<(), BindError> {
    let payload = hooks.encoded(message)?;
    let record =
        cc_desk_remote_record::encode(Kind::AppBind, 0, &payload).map_err(|_| BindError::Codec)?;
    io.write_all(&record).await.map_err(|_| BindError::Io)?;
    io.flush().await.map_err(|_| BindError::Io)
}
async fn receive<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
) -> Result<Message, BindError> {
    let mut bytes = [0; 8];
    io.read_exact(&mut bytes).await.map_err(|_| BindError::Io)?;
    let header = Header::decode(bytes, 0).map_err(|_| BindError::Codec)?;
    if header.kind != Kind::AppBind {
        return Err(BindError::Order);
    }
    let mut body = vec![0; header.payload_len];
    io.read_exact(&mut body).await.map_err(|_| BindError::Io)?;
    appbind::decode(&body).map_err(codec)
}
fn hash(h: &HostOffer, c: &ClientOffer, s: &HostSelection) -> Result<[u8; 32], BindError> {
    let bytes = appbind::transcript(h, c, s).map_err(codec)?;
    Ok(ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .try_into()
        .unwrap())
}
fn exporter<S>(
    tls: &tokio_rustls::TlsStream<S>,
    transcript: &[u8; 32],
) -> Result<hmac::Key, BindError> {
    let key = match tls {
        tokio_rustls::TlsStream::Client(s) => s.get_ref().1.export_keying_material(
            [0; 32],
            b"EXPORTER-CC-DESK-APPBIND-v1",
            Some(transcript),
        ),
        tokio_rustls::TlsStream::Server(s) => s.get_ref().1.export_keying_material(
            [0; 32],
            b"EXPORTER-CC-DESK-APPBIND-v1",
            Some(transcript),
        ),
    }
    .map_err(|_| BindError::Mac)?;
    Ok(hmac::Key::new(hmac::HMAC_SHA256, &key))
}
fn mac_input(host: bool, t: &[u8; 32]) -> Vec<u8> {
    let label: &[u8] = if host {
        b"CC-DESK-HOST-CONFIRM-v1\0"
    } else {
        b"CC-DESK-CLIENT-CONFIRM-v1\0"
    };
    let mut bytes = label.to_vec();
    bytes.extend_from_slice(t);
    bytes
}
fn sign(key: &hmac::Key, host: bool, t: &[u8; 32]) -> Mac {
    Mac::from_bytes(
        hmac::sign(key, &mac_input(host, t))
            .as_ref()
            .try_into()
            .unwrap(),
    )
}
fn verify(key: &hmac::Key, host: bool, t: &[u8; 32], mac: &Mac) -> Result<(), BindError> {
    hmac::verify(key, &mac_input(host, t), &mac.bytes().map_err(codec)?).map_err(|_| BindError::Mac)
}
fn eligible(deadline: Instant, token: &CancelToken) -> Result<(), BindError> {
    if Instant::now() >= deadline {
        Err(BindError::Deadline)
    } else if token.is_cancelled() {
        Err(BindError::Cancelled)
    } else {
        Ok(())
    }
}
async fn within<T, F: std::future::Future<Output = Result<T, BindError>>>(
    deadline: Instant,
    token: &mut CancelToken,
    work: F,
) -> Result<T, BindError> {
    eligible(deadline, token)?;
    let value = tokio::select! {biased;_=token.cancelled()=>Err(BindError::Cancelled),_=tokio::time::sleep_until(deadline)=>Err(BindError::Deadline),r=work=>r}?;
    eligible(deadline, token)?;
    Ok(value)
}
fn owner<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    mut io: tokio_rustls::TlsStream<S>,
    mut token: CancelToken,
    deadline: Instant,
    context: Context,
    selection: HostSelection,
) -> Result<BoundManagement, BindError> {
    let published = context.clone();
    let published_selection = selection.clone();
    eligible(deadline, &token)?;
    let (stop, mut stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut byte = [0; 1];
        tokio::select! {biased;
         _=token.cancelled()=>OwnerExit::Cancelled,
         _=&mut stopped=>OwnerExit::LocalClose,
         r=io.read(&mut byte)=>match r{Ok(0)=>OwnerExit::PeerClosed,Ok(_)=>OwnerExit::Unsupported,Err(_)=>OwnerExit::Io},
        }
    });
    Ok(BoundManagement {
        context: BoundContext(published),
        selection: BoundSelection(published_selection),
        task: Some(task),
        stop: Some(stop),
        exit: None,
    })
}
pub async fn bind_host<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
) -> Result<BoundManagement, BindError> {
    bind_host_with(pending, snapshot, Hooks::default()).await
}
async fn bind_host_with<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
    hooks: Hooks,
) -> Result<BoundManagement, BindError> {
    let e = establish_host(pending, snapshot, hooks, vec![0], 0).await?;
    owner(e.io, e.token, e.deadline, e.context, e.selection)
}
struct Established<S> {
    io: tokio_rustls::TlsStream<S>,
    token: CancelToken,
    deadline: Instant,
    context: Context,
    selection: HostSelection,
    binding: [u8; 32],
}
async fn establish_host<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
    hooks: Hooks,
    minors: Vec<u16>,
    grant_management: u8,
) -> Result<Established<S>, BindError> {
    let (host_id, client_id) = projected(&pending)?;
    if pending._context.0 != Role::Host {
        return Err(BindError::Context);
    }
    let deadline = pending._deadline;
    let mut token = pending._cancel;
    let mut io = pending._io;
    let context = Context {
        host_endpoint_id: host_id,
        client_endpoint_id: client_id,
        host_key_epoch: snapshot.host_key_epoch.to_string(),
        client_key_epoch: snapshot.client_key_epoch.to_string(),
        activation: hex(&snapshot.activation),
        incarnation: snapshot.incarnation.to_string(),
        policy_epoch: snapshot.policy_epoch.to_string(),
        issuer_epoch: snapshot.issuer_epoch.to_string(),
        origin: snapshot.origin,
    };
    let h = HostOffer {
        context,
        host_nonce: hooks.nonce()?,
        profile: 1,
        major: 1,
        minors,
        minimum_security: "1".into(),
        required_capabilities: management_capabilities(grant_management),
    };
    let (context, selection, binding) = within(deadline, &mut token, async {
        send_with(&mut io, &hooks, Message::HostOffer(h.clone())).await?;
        let c = match receive(&mut io).await? {
            Message::ClientOffer(c) => c,
            _ => return Err(BindError::Order),
        };
        let selection =
            hooks.selection(management_negotiate(&h, &c, grant_management).map_err(codec)?);
        let t = hash(&h, &c, &selection)?;
        hooks.observe(&h, &c, &selection, &t);
        let key = exporter(&io, &t)?;
        send_with(
            &mut io,
            &hooks,
            Message::HostConfirm(HostConfirm {
                selection: selection.clone(),
                mac: hooks.host_mac(&key, &t),
            }),
        )
        .await?;
        let confirm = match receive(&mut io).await? {
            Message::ClientConfirm(c) => c,
            _ => return Err(BindError::Order),
        };
        verify(&key, false, &t, &confirm.mac)?;
        let binding = data_binding(&io, &t)?;
        Ok((h.context.clone(), selection, binding))
    })
    .await?;
    Ok(Established {
        io,
        token,
        deadline,
        context,
        selection,
        binding,
    })
}
pub async fn bind_client<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
) -> Result<BoundManagement, BindError> {
    bind_client_with(pending, trust, Hooks::default()).await
}
async fn bind_client_with<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
    hooks: Hooks,
) -> Result<BoundManagement, BindError> {
    let e = establish_client(pending, trust, hooks, vec![0], 0).await?;
    owner(e.io, e.token, e.deadline, e.context, e.selection)
}
async fn establish_client<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
    hooks: Hooks,
    minors: Vec<u16>,
    grant_management: u8,
) -> Result<Established<S>, BindError> {
    let (host_id, client_id) = projected(&pending)?;
    if pending._context.0 != Role::Client {
        return Err(BindError::Context);
    }
    let deadline = pending._deadline;
    let mut token = pending._cancel;
    let mut io = pending._io;
    let (context, selection, binding) = within(deadline, &mut token, async {
        let h = match receive(&mut io).await? {
            Message::HostOffer(h) => h,
            _ => return Err(BindError::Order),
        };
        if h.context.host_endpoint_id != host_id
            || h.context.client_endpoint_id != client_id
            || h.context.host_key_epoch != trust.host_key_epoch.to_string()
            || h.context.client_key_epoch != trust.client_key_epoch.to_string()
            || h.context.origin != trust.origin
        {
            return Err(BindError::Context);
        }
        let c = ClientOffer {
            context: h.context.clone(),
            host_nonce: h.host_nonce.clone(),
            client_nonce: hooks.nonce()?,
            profile: 1,
            major: 1,
            minors,
            minimum_security: "1".into(),
            required_capabilities: management_capabilities(grant_management),
            requested_role: RequestedRole::ManagementOnly,
        };
        let selection = management_negotiate(&h, &c, grant_management).map_err(codec)?;
        send_with(&mut io, &hooks, Message::ClientOffer(c.clone())).await?;
        let confirm = match receive(&mut io).await? {
            Message::HostConfirm(h) => h,
            _ => return Err(BindError::Order),
        };
        if confirm.selection != selection {
            return Err(BindError::Context);
        }
        let t = hash(&h, &c, &selection)?;
        hooks.observe(&h, &c, &selection, &t);
        let key = exporter(&io, &t)?;
        verify(&key, true, &t, &confirm.mac)?;
        send_with(
            &mut io,
            &hooks,
            Message::ClientConfirm(ClientConfirm {
                mac: sign(&key, false, &t),
            }),
        )
        .await?;
        let binding = data_binding(&io, &t)?;
        Ok((h.context, selection, binding))
    })
    .await?;
    Ok(Established {
        io,
        token,
        deadline,
        context,
        selection,
        binding,
    })
}

fn data_binding<S>(
    io: &tokio_rustls::TlsStream<S>,
    transcript: &[u8; 32],
) -> Result<[u8; 32], BindError> {
    let label = b"EXPORTER-CC-DESK-DATA-BIND-v1";
    match io {
        tokio_rustls::TlsStream::Client(s) => {
            s.get_ref()
                .1
                .export_keying_material([0; 32], label, Some(transcript))
        }
        tokio_rustls::TlsStream::Server(s) => {
            s.get_ref()
                .1
                .export_keying_material([0; 32], label, Some(transcript))
        }
    }
    .map_err(|_| BindError::Mac)
}
pub async fn bind_host_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_host(pending, snapshot, Hooks::default(), vec![0, 1], 0).await?;
    eligible(e.deadline, &e.token)?;
    if e.selection.minor != 1 {
        return Err(BindError::Unsupported);
    }
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Host,
    ))
}
pub async fn bind_client_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_client(pending, trust, Hooks::default(), vec![0, 1], 0).await?;
    eligible(e.deadline, &e.token)?;
    if e.selection.minor != 1 {
        return Err(BindError::Unsupported);
    }
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Client,
    ))
}

// No test-support feature or public bypass: all non-default hook fields exist only
// in the unit-test build. Production nonce generation and framing remain fixed.
#[cfg(test)]
type WireMutation = std::sync::Arc<dyn Fn(Vec<u8>) -> Vec<u8> + Send + Sync>;
#[derive(Default)]
struct Hooks {
    #[cfg(test)]
    fixed_nonce: Option<[u8; 32]>,
    #[cfg(test)]
    alter: Option<std::sync::Arc<dyn Fn(Message) -> Message + Send + Sync>>,
    #[cfg(test)]
    selected: Option<std::sync::Arc<dyn Fn(HostSelection) -> HostSelection + Send + Sync>>,
    #[cfg(test)]
    wire: Option<WireMutation>,
    #[cfg(test)]
    observed: Option<std::sync::Arc<std::sync::Mutex<Vec<Observation>>>>,
    #[cfg(test)]
    client_direction_host_mac: bool,
}
#[cfg(test)]
#[derive(PartialEq, Eq)]
struct Observation {
    host: Vec<u8>,
    client: Vec<u8>,
    selection: Vec<u8>,
    hash: [u8; 32],
}
impl Hooks {
    fn nonce(&self) -> Result<String, BindError> {
        #[cfg(test)]
        if let Some(n) = self.fixed_nonce {
            return Ok(hex(&n));
        }
        nonce()
    }
    fn encoded(&self, message: Message) -> Result<Vec<u8>, BindError> {
        #[cfg(test)]
        let message = if let Some(alter) = &self.alter {
            alter(message)
        } else {
            message
        };
        let bytes = appbind::encode(&message).map_err(codec)?;
        #[cfg(test)]
        let bytes = if let Some(wire) = &self.wire {
            wire(bytes)
        } else {
            bytes
        };
        Ok(bytes)
    }
    fn selection(&self, value: HostSelection) -> HostSelection {
        #[cfg(test)]
        if let Some(select) = &self.selected {
            return select(value);
        }
        value
    }
    fn host_mac(&self, key: &hmac::Key, t: &[u8; 32]) -> Mac {
        #[cfg(test)]
        if self.client_direction_host_mac {
            return sign(key, false, t);
        }
        sign(key, true, t)
    }
    fn observe(&self, _h: &HostOffer, _c: &ClientOffer, _s: &HostSelection, _t: &[u8; 32]) {
        #[cfg(test)]
        if let Some(observed) = &self.observed {
            observed.lock().unwrap().push(Observation {
                host: appbind::encode(&Message::HostOffer(_h.clone())).unwrap(),
                client: appbind::encode(&Message::ClientOffer(_c.clone())).unwrap(),
                selection: appbind::encode_selection(_s).unwrap(),
                hash: *_t,
            });
        }
    }
}
#[cfg(test)]
#[path = "bind_tests.rs"]
mod tests;

fn management_capabilities(enabled: u8) -> Vec<String> {
    if enabled == 2 {
        vec![
            "canonical-json".into(),
            "grant-management-v1".into(),
            "peer-install-v1".into(),
        ]
    } else if enabled == 1 {
        vec![
            "canonical-json".into(),
            cc_desk_remote_record::grant_management::CAPABILITY.into(),
        ]
    } else {
        vec!["canonical-json".into()]
    }
}
fn management_negotiate(
    h: &HostOffer,
    c: &ClientOffer,
    enabled: u8,
) -> Result<HostSelection, CodecError> {
    if enabled == 2 {
        appbind::negotiate_peer(h, c)
    } else if enabled == 1 {
        appbind::negotiate_grant(h, c)
    } else {
        appbind::negotiate(h, c)
    }
}
pub async fn bind_host_grant_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_host(pending, snapshot, Hooks::default(), vec![1], 1).await?;
    eligible(e.deadline, &e.token)?;
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Host,
    )
    .with_grant_management())
}
pub async fn bind_client_grant_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_client(pending, trust, Hooks::default(), vec![1], 1).await?;
    eligible(e.deadline, &e.token)?;
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Client,
    )
    .with_grant_management())
}

/// Opt-in authenticated peer installation; no fallback through another factory.
pub async fn bind_host_peer_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    snapshot: HostSnapshot,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_host(pending, snapshot, Hooks::default(), vec![1], 2).await?;
    eligible(e.deadline, &e.token)?;
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Host,
    )
    .with_grant_management()
    .with_peer_install())
}
pub async fn bind_client_peer_channel<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    pending: Pending<S>,
    trust: ClientTrust,
) -> Result<crate::ManagedChannel<S>, BindError> {
    let e = establish_client(pending, trust, Hooks::default(), vec![1], 2).await?;
    eligible(e.deadline, &e.token)?;
    Ok(crate::ManagedChannel::new(
        e.io,
        e.token,
        BoundContext(e.context),
        e.binding,
        Role::Client,
    )
    .with_grant_management()
    .with_peer_install())
}

#[cfg(test)]
mod peer_negotiation_refusal_tests {
    use super::*;
    use crate::{LocalIdentity, P256Spki, PeerPin, Unbound, accept, cancellation, connect};
    fn local(client: bool) -> LocalIdentity {
        let (k, p) = crate::test_identities::identity(client, false);
        LocalIdentity::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
            k.key.clone(),
        )
        .unwrap()
    }
    fn pin(client: bool) -> PeerPin {
        let (_, p) = crate::test_identities::identity(client, false);
        PeerPin::new(
            if client { Role::Client } else { Role::Host },
            P256Spki::new(&p).unwrap(),
        )
    }
    #[tokio::test]
    async fn peer_install_authenticated_offer_stripped_capability_and_minor_zero_refused_both_directions()
     {
        for client_side in [true, false] {
            for minor_zero in [true, false] {
                let (c, h) = tokio::io::duplex(65536);
                let (_cancel, t) = cancellation();
                let budget = std::time::Duration::from_secs(1);
                let (c, h) = tokio::join!(
                    connect(Unbound::new(c), local(true), pin(false), budget, t.clone()),
                    accept(Unbound::new(h), local(false), pin(true), budget, t)
                );
                let origin = CanonicalOrigin::trusted("wss://relay.invalid/").unwrap();
                let trust = ClientTrust {
                    host_key_epoch: 5,
                    client_key_epoch: 7,
                    origin: origin.clone(),
                };
                let snapshot = HostSnapshot {
                    host_key_epoch: 5,
                    client_key_epoch: 7,
                    activation: [0x33; 32],
                    incarnation: 11,
                    policy_epoch: 13,
                    issuer_epoch: 17,
                    origin,
                };
                let hooks = Hooks {
                    alter: Some(std::sync::Arc::new(move |mut m| {
                        match &mut m {
                            Message::ClientOffer(c) => {
                                if minor_zero {
                                    c.minors = vec![0]
                                } else {
                                    c.required_capabilities.pop();
                                }
                            }
                            Message::HostOffer(h) => {
                                if minor_zero {
                                    h.minors = vec![0]
                                } else {
                                    h.required_capabilities.pop();
                                }
                            }
                            _ => (),
                        }
                        m
                    })),
                    ..Hooks::default()
                };
                let (c, h) = if client_side {
                    tokio::join!(
                        establish_client(c.unwrap(), trust, hooks, vec![1], 2),
                        establish_host(h.unwrap(), snapshot, Hooks::default(), vec![1], 2)
                    )
                } else {
                    tokio::join!(
                        establish_client(c.unwrap(), trust, Hooks::default(), vec![1], 2),
                        establish_host(h.unwrap(), snapshot, hooks, vec![1], 2)
                    )
                };
                assert!(
                    c.is_err() || h.is_err(),
                    "tampered authenticated offer accepted"
                );
            }
        }
    }
}
