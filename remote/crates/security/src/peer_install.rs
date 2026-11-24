//! Single-use bootstrap on the original IO owner. Wire DTOs never mint proof.
use crate::{BoundDataAuthority, DataError, Role, RunningChannel};
use cc_desk_remote_record::{Header, Kind, peer_install as wire};
use ring::rand::SecureRandom;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot},
    time::Instant,
};

enum Admission {
    Legacy,
    Pending,
    Ready(u64),
    Terminal,
}
#[derive(Clone, Copy)]
enum InstallEnd {
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        allow(dead_code, reason = "原Native公开类型在浏览器入口仍拒绝")
    )]
    Native(Instant),
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    Browser(crate::tls_clock::Deadline),
}
pub(crate) struct Shared {
    admission: Mutex<Admission>,
    end: Option<InstallEnd>,
    client: tokio::sync::Mutex<Option<oneshot::Receiver<ClientInstalledPeer>>>,
    request: tokio::sync::Mutex<Option<oneshot::Receiver<HostInstallRequest>>>,
    complete: mpsc::Sender<Completion>,
}
pub(crate) struct Bootstrap {
    client: oneshot::Sender<ClientInstalledPeer>,
    request: oneshot::Sender<HostInstallRequest>,
    complete: mpsc::Receiver<Completion>,
}
struct Completion {
    request: HostInstallRequest,
    fence: u64,
    reply: oneshot::Sender<Result<(), DataError>>,
}
impl Shared {
    fn owner_deadline(&self) -> Option<crate::owner_clock::Instant> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            self.native_deadline()
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            match self.end {
                Some(InstallEnd::Browser(end)) => Some(crate::owner_clock::Instant(end.0)),
                _ => None,
            }
        }
    }
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        allow(dead_code, reason = "原Native工厂不在浏览器启动")
    )]
    pub(crate) fn new(end: Option<Instant>) -> (Arc<Self>, Bootstrap) {
        Self::create(end.map(InstallEnd::Native))
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) fn new_browser(end: crate::tls_clock::Deadline) -> (Arc<Self>, Bootstrap) {
        Self::create(Some(InstallEnd::Browser(end)))
    }
    fn native_deadline(&self) -> Option<Instant> {
        match self.end {
            Some(InstallEnd::Native(end)) => Some(end),
            _ => None,
        }
    }
    fn create(end: Option<InstallEnd>) -> (Arc<Self>, Bootstrap) {
        let (ct, cr) = oneshot::channel();
        let (rt, rr) = oneshot::channel();
        let (tx, rx) = mpsc::channel(1);
        (
            Arc::new(Self {
                admission: Mutex::new(if end.is_some() {
                    Admission::Pending
                } else {
                    Admission::Legacy
                }),
                end,
                client: tokio::sync::Mutex::new(Some(cr)),
                request: tokio::sync::Mutex::new(Some(rr)),
                complete: tx,
            }),
            Bootstrap {
                client: ct,
                request: rt,
                complete: rx,
            },
        )
    }
    pub(crate) fn enabled(&self) -> bool {
        self.end.is_some()
    }
    // 只读缓存，不消费原安装请求、回复或 client proof。
    pub(crate) fn cached_live_fence(&self) -> Result<Option<u64>, DataError> {
        match *self.admission.lock().map_err(|_| DataError::Closed)? {
            Admission::Pending => Ok(None),
            Admission::Ready(f) => Ok(Some(f)),
            Admission::Terminal => Err(DataError::Closed),
            Admission::Legacy => Err(DataError::Context),
        }
    }
    pub(crate) fn fence(&self) -> Result<u64, DataError> {
        match *self.admission.lock().map_err(|_| DataError::Closed)? {
            Admission::Ready(f) => Ok(f),
            Admission::Terminal => Err(DataError::Closed),
            _ => Err(DataError::Context),
        }
    }
    pub(crate) fn try_matches_installed_fence(&self, fence: u64) -> bool {
        self.admission.try_lock().is_ok_and(
            |state| matches!(*state, Admission::Ready(installed) if installed == fence),
        )
    }
    pub(crate) fn check_data(&self, fence: u64) -> Result<(), DataError> {
        match *self.admission.lock().map_err(|_| DataError::Closed)? {
            Admission::Legacy => Ok(()),
            Admission::Ready(f) if f == fence => Ok(()),
            Admission::Ready(_) => Err(DataError::Fence),
            Admission::Pending => Err(DataError::Context),
            Admission::Terminal => Err(DataError::Closed),
        }
    }
    pub(crate) fn admission_stamp(&self, fence: u64) -> Result<Option<u64>, DataError> {
        self.check_data(fence)?;
        Ok(self.enabled().then_some(fence))
    }
    pub(crate) fn check_queued(&self, fence: u64, stamp: Option<u64>) -> Result<(), DataError> {
        self.check_data(fence)?;
        if self.enabled() && stamp != Some(fence) {
            return Err(DataError::Context);
        }
        Ok(())
    }
    fn commit(&self, authority: &BoundDataAuthority, fence: u64) -> Result<(), DataError> {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        if let Some(InstallEnd::Browser(end)) = self.end {
            // 固定clock import与grant检查均在admission锁外；同一非shared WASM owner发布。
            authority.check()?;
            if crate::tls_clock::expired(end) {
                return Err(DataError::Deadline);
            }
            if fence == 0 {
                return Err(DataError::Fence);
            }
            let mut state = self.admission.lock().map_err(|_| DataError::Closed)?;
            if !matches!(*state, Admission::Pending) {
                return Err(DataError::Closed);
            }
            *state = Admission::Ready(fence);
            return Ok(());
        }
        let mut state = self.admission.lock().map_err(|_| DataError::Closed)?;
        authority.check()?;
        if Instant::now() >= self.native_deadline().ok_or(DataError::Context)? {
            return Err(DataError::Deadline);
        }
        if fence == 0 {
            return Err(DataError::Fence);
        }
        if !matches!(*state, Admission::Pending) {
            return Err(DataError::Closed);
        }
        // Immutable complete installed metadata and marker publish under the SAME lock.
        *state = Admission::Ready(fence);
        Ok(())
    }
    pub(crate) fn terminal(&self) {
        if let Ok(mut state) = self.admission.lock() {
            *state = Admission::Terminal;
        }
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) fn completed_client(&self) -> Result<ClientInstalledPeer, DataError> {
        let mut rx = self
            .client
            .try_lock()
            .map_err(|_| DataError::Full)?
            .take()
            .ok_or(DataError::Full)?;
        rx.try_recv().map_err(|_| DataError::Closed)
    }
}

/// Opaque, non-Clone and non-deserializable. Only the authenticated IO owner mints it.
pub struct ClientInstalledPeer {
    authority: BoundDataAuthority,
    fence: u64,
}
impl ClientInstalledPeer {
    pub fn consume_for(self, channel: &RunningChannel) -> Result<u64, DataError> {
        self.consume_for_owner(&channel.authority(), &channel.peer)
    }
    pub(crate) fn consume_for_owner(
        self,
        other: &BoundDataAuthority,
        peer: &Shared,
    ) -> Result<u64, DataError> {
        self.authority.check()?;
        other.check()?;
        if !peer.enabled()
            || other.local_role() != Role::Client
            || !self.authority.same_channel(other)
            || self.authority.binding_id() != other.binding_id()
            || self.authority.grant_id() != other.grant_id()
            || self.authority.scope() != other.scope()
            || self.authority.data_role() != other.data_role()
            || self.authority.context() != other.context()
            || !self
                .authority
                .clock_domain()
                .same_origin(&other.clock_domain())
            || {
                other.check()?;
                peer.fence()?
            } != self.fence
        {
            return Err(DataError::Context);
        }
        Ok(self.fence)
    }
}
/// Typed request for trusted native coordination; no public constructor or Clone.
pub struct HostInstallRequest {
    authority: BoundDataAuthority,
    nonce: String,
    binding: String,
    grant: String,
    scope: String,
}
impl HostInstallRequest {
    pub fn authority(&self) -> &BoundDataAuthority {
        &self.authority
    }
    fn matches(&self, a: &BoundDataAuthority) -> bool {
        self.authority.same_channel(a)
            && self.authority.grant_id() == a.grant_id()
            && self.authority.scope() == a.scope()
            && a.local_role() == Role::Host
    }
}
struct CloseOnDrop(Option<BoundDataAuthority>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Some(a) = &self.0 {
            a.request_close();
        }
    }
}
impl RunningChannel {
    pub async fn client_installed(&self) -> Result<ClientInstalledPeer, DataError> {
        if self.authority().local_role() != Role::Client || !self.peer.enabled() {
            return Err(DataError::Role);
        }
        let rx = self
            .peer
            .client
            .try_lock()
            .map_err(|_| DataError::Full)?
            .take()
            .ok_or(DataError::Full)?;
        let mut guard = CloseOnDrop(Some(self.authority()));
        let proof = crate::owner_clock::timeout_at(
            self.peer.owner_deadline().ok_or(DataError::Context)?,
            rx,
        )
        .await
        .map_err(|_| DataError::Deadline)?
        .map_err(|_| DataError::Closed)?;
        self.authority().check()?;
        self.peer.check_data(proof.fence)?;
        guard.0 = None;
        Ok(proof)
    }
    pub async fn host_install_request(&self) -> Result<HostInstallRequest, DataError> {
        if self.authority().local_role() != Role::Host || !self.peer.enabled() {
            return Err(DataError::Role);
        }
        let rx = self
            .peer
            .request
            .try_lock()
            .map_err(|_| DataError::Full)?
            .take()
            .ok_or(DataError::Full)?;
        let mut guard = CloseOnDrop(Some(self.authority()));
        let request =
            tokio::time::timeout_at(self.peer.native_deadline().ok_or(DataError::Context)?, rx)
                .await
                .map_err(|_| DataError::Deadline)?
                .map_err(|_| DataError::Closed)?;
        self.authority().check()?;
        guard.0 = None;
        Ok(request)
    }
    /// Trusted native TRANSPORT completion, not an unforgeable actor receipt.
    /// Product use is restricted to NativeHost's coordinator after its original pending succeeds.
    pub async fn complete_host_install(
        &self,
        request: HostInstallRequest,
        fence: u64,
    ) -> Result<(), DataError> {
        let mut guard = CloseOnDrop(Some(self.authority()));
        self.authority().check()?;
        if !self.peer.enabled() || !request.matches(&self.authority()) {
            return Err(DataError::Context);
        }
        if fence == 0 {
            return Err(DataError::Fence);
        }
        let (reply, rx) = oneshot::channel();
        self.peer
            .complete
            .try_send(Completion {
                request,
                fence,
                reply,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DataError::Full,
                _ => DataError::Closed,
            })?;
        tokio::time::timeout_at(self.peer.native_deadline().ok_or(DataError::Context)?, rx)
            .await
            .map_err(|_| DataError::Deadline)?
            .map_err(|_| DataError::Closed)??;
        self.authority().check()?;
        guard.0 = None;
        Ok(())
    }
    /// Fixed at owner construction. This accessor cannot extend installation time.
    pub fn peer_install_deadline(&self) -> Option<Instant> {
        self.peer.native_deadline()
    }
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}
fn metadata(
    a: &BoundDataAuthority,
    access: &crate::data::ChannelGrant,
) -> (String, String, String) {
    (
        hex(&a.binding_id()),
        hex(&a.grant_id()),
        hex(&access.scope_digest()),
    )
}
async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
    m: wire::Message,
) -> Result<(), DataError> {
    let body = wire::encode(&m).map_err(|_| DataError::Codec)?;
    let bytes =
        cc_desk_remote_record::encode(Kind::Control, 1, &body).map_err(|_| DataError::Codec)?;
    #[cfg(test)]
    let bytes = fixture_mutate(bytes, &m)?;
    // Sequential write_all + flush deliberately pauses incoming parsing on BOTH sides.
    io.write_all(&bytes).await.map_err(|_| DataError::Io)?;
    io.flush().await.map_err(|_| DataError::Io)
}
#[cfg(test)]
static FIXTURE_MUTATION: Mutex<Option<&'static str>> = Mutex::new(None);
#[cfg(test)]
pub(crate) fn fixture_mutation(mode: &'static str) {
    *FIXTURE_MUTATION.lock().unwrap() = Some(mode);
}
#[cfg(test)]
fn fixture_mutate(mut bytes: Vec<u8>, message: &wire::Message) -> Result<Vec<u8>, DataError> {
    if !matches!(message, wire::Message::Installed { .. }) {
        return Ok(bytes);
    }
    let Some(mode) = FIXTURE_MUTATION.lock().unwrap().take() else {
        return Ok(bytes);
    };
    // 仅改原bootstrap已经编码的一个失败字段，或重放原完整帧；不另写成功协议。
    if mode == "replay" {
        let copy = bytes.clone();
        bytes.extend_from_slice(&copy);
        return Ok(bytes);
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&bytes[8..]).map_err(|_| DataError::Codec)?;
    match mode {
        "binding" => {
            let old = value["binding"].as_str().ok_or(DataError::Codec)?;
            let first = if old.starts_with('0') { '1' } else { '0' };
            value["binding"] = format!("{first}{}", &old[1..]).into();
        }
        "fence" => value["fence"] = "0".into(),
        "order" => {
            value["message"] = "Request".into();
            value.as_object_mut().unwrap().remove("fence");
        }
        _ => return Err(DataError::Context),
    }
    let body = serde_json::to_vec(&value).map_err(|_| DataError::Codec)?;
    cc_desk_remote_record::encode(Kind::Control, 1, &body).map_err(|_| DataError::Codec)
}
async fn read<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
) -> Result<wire::Message, DataError> {
    let mut bytes = [0; 8];
    io.read_exact(&mut bytes).await.map_err(|_| DataError::Io)?;
    let h = Header::decode(bytes, 1).map_err(|_| DataError::Codec)?;
    if h.kind != Kind::Control || h.payload_len == 0 || h.payload_len > 16384 {
        return Err(DataError::Codec);
    }
    let mut body = vec![0; h.payload_len];
    io.read_exact(&mut body).await.map_err(|_| DataError::Io)?;
    wire::decode(&body).map_err(|_| DataError::Codec)
}
pub(crate) async fn bootstrap<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
    a: &BoundDataAuthority,
    access: &crate::data::ChannelGrant,
    role: Role,
    shared: Arc<Shared>,
    mut ports: Bootstrap,
) -> Result<(), DataError> {
    a.check()?;
    let (binding, grant, scope) = metadata(a, access);
    match role {
        Role::Client => {
            let mut random = [0; 32];
            ring::rand::SystemRandom::new()
                .fill(&mut random)
                .map_err(|_| DataError::Context)?;
            let nonce = hex(&random);
            send(
                io,
                wire::Message::Request {
                    nonce: nonce.clone(),
                    binding: binding.clone(),
                    grant_id: grant.clone(),
                    scope_digest: scope.clone(),
                },
            )
            .await?;
            a.check()?;
            // Phase is now AwaitInstalled before next read; a response may already be buffered.
            let wire::Message::Installed {
                nonce: n,
                binding: b,
                grant_id: g,
                scope_digest: s,
                fence,
            } = read(io).await?
            else {
                return Err(DataError::Role);
            };
            if (n, b, g, s) != (nonce, binding, grant, scope) {
                return Err(DataError::Context);
            }
            let fence = fence.parse::<u64>().map_err(|_| DataError::Fence)?;
            shared.commit(a, fence)?;
            a.check()?;
            ports
                .client
                .send(ClientInstalledPeer {
                    authority: a.clone(),
                    fence,
                })
                .map_err(|_| DataError::Closed)?;
        }
        Role::Host => {
            let wire::Message::Request {
                nonce,
                binding: b,
                grant_id: g,
                scope_digest: s,
            } = read(io).await?
            else {
                return Err(DataError::Role);
            };
            a.check()?;
            if (b, g, s) != (binding.clone(), grant.clone(), scope.clone()) {
                return Err(DataError::Context);
            }
            let expected_nonce = nonce.clone();
            ports
                .request
                .send(HostInstallRequest {
                    authority: a.clone(),
                    nonce,
                    binding,
                    grant,
                    scope,
                })
                .map_err(|_| DataError::Closed)?;
            // Continue rejecting premature/replayed packets while native installation waits.
            // Only this original task ever reads TLS; the native waiter has a typed slot.
            let mut unexpected_byte = [0; 1];
            let complete = tokio::select! { biased;
                unexpected = io.read(&mut unexpected_byte) => { let _ = unexpected; return Err(DataError::Context); },
                c = ports.complete.recv() => c.ok_or(DataError::Closed)?,
            };
            a.check()?;
            if !complete.request.matches(a) || complete.request.nonce != expected_nonce {
                return Err(DataError::Context);
            }
            let r = complete.request;
            send(
                io,
                wire::Message::Installed {
                    nonce: r.nonce,
                    binding: r.binding,
                    grant_id: r.grant,
                    scope_digest: r.scope,
                    fence: complete.fence.to_string(),
                },
            )
            .await?;
            // No read between flush completion and publication of Ready(fence).
            shared.commit(a, complete.fence)?;
            a.check()?;
            complete.reply.send(Ok(())).map_err(|_| DataError::Closed)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod peer_mailbox_tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test]
    async fn peer_install_completion_mailbox_is_one_slot_and_full_rejects_second() {
        let (mut c, mut h, _cancel, _gate) = crate::data::peer_install_tests::granted().await;
        let d = Instant::now() + Duration::from_secs(2);
        let c = c.start_peer_pending(d).unwrap();
        let h = h.start_peer_pending(d).unwrap();
        let original = h.host_install_request().await.unwrap();
        let duplicate = HostInstallRequest {
            authority: original.authority.clone(),
            nonce: original.nonce.clone(),
            binding: original.binding.clone(),
            grant: original.grant.clone(),
            scope: original.scope.clone(),
        };
        // Test the exact typed bootstrap slot with its owner receiver kept alive
        // but deliberately unpolled, so capacity cannot be scheduler-dependent.
        let (shared, _ports) = Shared::new(Some(d));
        let (reply, _rx) = oneshot::channel();
        assert!(
            shared
                .complete
                .try_send(Completion {
                    request: original,
                    fence: 7,
                    reply
                })
                .is_ok()
        );
        let (reply, _rx) = oneshot::channel();
        assert!(matches!(
            shared.complete.try_send(Completion {
                request: duplicate,
                fence: 7,
                reply
            }),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        assert_eq!(shared.complete.capacity(), 0);
        assert!(shared.fence().is_err());
        c.authority().request_close();
        h.authority().request_close();
        let _ = tokio::join!(c.wait_closed(), h.wait_closed());
    }
}
