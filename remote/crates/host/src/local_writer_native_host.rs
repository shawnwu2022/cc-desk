//! Application custody of the original writer and pending install; not a listener.
use super::*;
use attach::connection::{Connection, custody};
use authorized::{AuthorizedPeer, AuthorizedPendingRebind};
use cc_desk_remote_security::{BoundDataAuthority, Role, RunningChannel};

const MAX_ATTEMPTS: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeHostError {
    Closed,
    Capacity,
    Authority,
    Rejected,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallAttempt(u64);
/// Installation fact only; never a snapshot ACK, input grant or Data capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallObservation {
    Pending,
    Installed { fence: u64 },
    Rejected,
    Unknown,
}
/// Only the owned writer thread and install replies; not PTY/network retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterRetirement {
    Pending,
    Joined,
    Unknown,
}
enum InstallState {
    Pending(AuthorizedPendingRebind),
    Installed {
        peer: Arc<AuthorizedPeer>,
        fence: u64,
    },
    Rejected,
    Unknown,
}
struct Attempt {
    id: InstallAttempt,
    channel: Arc<RunningChannel>,
    authority: BoundDataAuthority,
    next_fence: u64,
    connection_taken: bool,
    state: InstallState,
}
pub struct NativeHost {
    owner: WriterHostOwner,
    anchor: Option<Arc<RunningChannel>>,
    attempts: Vec<Attempt>,
    pending: Arc<custody::Owner>,
    closing: bool,
    unknown: bool,
    peer_installs: Vec<peer_coordinator::Coordinator>,
}
impl NativeHost {
    /// Native live authority only. No endpoint enrollment, raw peer or IO factory.
    pub fn from_channel(channel: Arc<RunningChannel>) -> Result<Self, NativeHostError> {
        let authority = channel.authority();
        authority.check().map_err(|_| NativeHostError::Authority)?;
        if authority.local_role() != Role::Host
            || authority.data_role() != cc_desk_remote_record::grant::DataRole::Controller
        {
            return Err(NativeHostError::Authority);
        }
        let c = authority.context();
        let bytes = c.host_endpoint_id.as_bytes();
        if bytes.len() != 64 {
            return Err(NativeHostError::Authority);
        }
        let mut host = [0u8; 32];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let nibble = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            host[index] = nibble(pair[0])
                .zip(nibble(pair[1]))
                .map(|(a, b)| a * 16 + b)
                .ok_or(NativeHostError::Authority)?;
        }
        let incarnation = c
            .incarnation
            .parse()
            .map_err(|_| NativeHostError::Authority)?;
        let owner = WriterHostOwner::new_authorized(host, incarnation, authority.principal_id())
            .map_err(|_| NativeHostError::Rejected)?;
        let mut host = Self::from_owner(owner);
        host.anchor = Some(channel);
        Ok(host)
    }
    fn from_owner(owner: WriterHostOwner) -> Self {
        Self {
            owner,
            anchor: None,
            attempts: Vec::new(),
            pending: Arc::new(custody::Owner::new()),
            closing: false,
            unknown: false,
            peer_installs: Vec::new(),
        }
    }
    /// Native provider lifetime binding; not an input permission or wire API.
    pub fn owns_run_scope(&self, run: &str, generation: u32) -> bool {
        self.anchor.as_ref().is_some_and(|channel| {
            let authority = channel.authority();
            authority.check().is_ok()
                && authority.scope().run == run
                && authority.scope().generation == generation.to_string()
        })
    }
    /// Retain the original pending before returning an observation token. Dropping
    /// the API waiter/token cannot cancel it, replay it or release owner custody.
    pub fn begin_rebind(
        &mut self,
        target: &Target,
        device: [u8; 32],
        expected: u64,
        channel: Arc<RunningChannel>,
        lease: [u8; 16],
    ) -> Result<InstallAttempt, NativeHostError> {
        if self.closing {
            return Err(NativeHostError::Closed);
        }
        if self.attempts.len() >= MAX_ATTEMPTS {
            return Err(NativeHostError::Capacity);
        }
        let next_fence = expected.checked_add(1).ok_or(NativeHostError::Rejected)?;
        let authority = channel.authority();
        let pending = self
            .owner
            .rebind_authorized(target, device, expected, authority.clone(), lease)
            .map_err(|_| NativeHostError::Rejected)?;
        let id = InstallAttempt(self.attempts.len() as u64 + 1);
        self.attempts.push(Attempt {
            id,
            channel,
            authority,
            next_fence,
            connection_taken: false,
            state: InstallState::Pending(pending),
        });
        Ok(id)
    }
    pub fn observe_install(&mut self, id: InstallAttempt) -> InstallObservation {
        let Some(attempt) = self.attempts.iter_mut().find(|attempt| attempt.id == id) else {
            return InstallObservation::Unknown;
        };
        Self::poll(attempt);
        // Facts stay owned even when disclosure is no longer authorized.
        if attempt.authority.check().is_err() {
            return InstallObservation::Unknown;
        }
        match attempt.state {
            InstallState::Pending(_) => InstallObservation::Pending,
            InstallState::Installed { fence, .. } => InstallObservation::Installed { fence },
            InstallState::Rejected => InstallObservation::Rejected,
            InstallState::Unknown => InstallObservation::Unknown,
        }
    }
    fn poll(attempt: &mut Attempt) {
        let InstallState::Pending(pending) = &attempt.state else {
            return;
        };
        attempt.state = match pending.wait(Duration::ZERO) {
            Ok(Ok(peer)) => InstallState::Installed {
                peer: Arc::new(peer),
                fence: attempt.next_fence,
            },
            Ok(Err(_)) => InstallState::Rejected,
            Err(WaitError::Closed) => InstallState::Unknown,
            Err(WaitError::Uncertain) => return,
        };
    }
    /// Existing installed peer only. The token cannot choose authority or fence.
    /// No native IO factory, bootstrap, Data grant installation or listener.
    pub fn connection(&mut self, id: InstallAttempt) -> Result<NativeConnection, NativeHostError> {
        if self.closing {
            return Err(NativeHostError::Closed);
        }
        let attempt = self
            .attempts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(NativeHostError::Unknown)?;
        Self::poll(attempt);
        if attempt.connection_taken {
            return Err(NativeHostError::Rejected);
        }
        attempt
            .authority
            .check()
            .map_err(|_| NativeHostError::Authority)?;
        let InstallState::Installed { peer, fence } = &attempt.state else {
            return Err(NativeHostError::Rejected);
        };
        let inner = Connection::new_owned(
            peer.clone(),
            attempt.authority.clone(),
            *fence,
            self.pending.clone(),
        )
        .map_err(|_| NativeHostError::Rejected)?;
        attempt.connection_taken = true;
        Ok(NativeConnection {
            inner,
            channel: attempt.channel.clone(),
        })
    }
    /// Phone loss only invalidates its exact original channel. It does not stop a
    /// user's task or seal the writer. Network join belongs to its network owner.
    pub fn disconnect(&self, id: InstallAttempt) -> Result<(), NativeHostError> {
        let attempt = self
            .attempts
            .iter()
            .find(|attempt| attempt.id == id)
            .ok_or(NativeHostError::Unknown)?;
        attempt.authority.request_close();
        Ok(())
    }
    pub fn begin_shutdown(&mut self) {
        self.pending.seal();
        self.closing = true;
        if let Some(anchor) = &self.anchor {
            anchor.authority().request_close();
        }
        for attempt in &self.attempts {
            attempt.authority.request_close();
        }
        for job in &self.peer_installs {
            job.channel().authority().request_close();
        }
        // Signal before inspecting pending replies: a pending operation may need
        // this very stop to become terminal. Never wait under a database lock.
        for session in &self.owner.active {
            session.writer.seal();
        }
    }
    pub fn network_retirement(&self) -> Option<NetworkRetirement> {
        if !self.closing {
            return None;
        }
        let channels = self
            .anchor
            .iter()
            .cloned()
            .chain(self.attempts.iter().map(|a| a.channel.clone()))
            .chain(self.peer_installs.iter().map(|job| job.channel()))
            .collect();
        Some(NetworkRetirement { channels })
    }
    pub fn observe_shutdown(&mut self) -> WriterRetirement {
        if !self.closing {
            return WriterRetirement::Pending;
        }
        self.drive_peer_installs();
        let mut pending = self.peer_installs.iter().any(|job| job.pending());
        match self.pending.observe() {
            custody::Observation::Pending => pending = true,
            custody::Observation::Unknown => self.unknown = true,
            custody::Observation::Drained => (),
        }
        for attempt in &mut self.attempts {
            Self::poll(attempt);
            pending |= matches!(attempt.state, InstallState::Pending(_));
            self.unknown |= matches!(attempt.state, InstallState::Unknown);
        }
        let targets: Vec<_> = self.owner.active.iter().map(|s| s.target.clone()).collect();
        for target in targets {
            match self.owner.try_reap(&target) {
                Ok(true) => (),
                Ok(false) => pending = true,
                Err(_) => self.unknown = true,
            }
        }
        if pending {
            WriterRetirement::Pending
        } else if self.unknown {
            WriterRetirement::Unknown
        } else {
            WriterRetirement::Joined
        }
    }
}

/// Owned API shell around the EXISTING wire dispatcher. Dropping a request or
/// this shell closes its authority, but original native replies stay in Host.
pub struct NativeConnection {
    inner: Connection<'static>,
    channel: Arc<RunningChannel>,
}
impl NativeConnection {
    pub async fn dispatch_received(
        &mut self,
        record: &cc_desk_remote_security::DataRecord,
        budget: Duration,
    ) -> Result<(), Reject> {
        self.inner
            .dispatch_wire(record, &self.channel, budget)
            .await
    }
}
impl Drop for NativeConnection {
    fn drop(&mut self) {
        self.channel.authority().request_close();
    }
}

/// Owns exactly the retained network owners. Await proves their tasks were joined,
/// not that the protocol ended normally (RunningChannel folds its JoinError).
pub struct NetworkRetirement {
    channels: Vec<Arc<RunningChannel>>,
}
impl NetworkRetirement {
    pub async fn wait(self) {
        for channel in self.channels {
            let _reason = channel.wait_closed().await;
        }
    }
}
#[cfg(test)]
#[path = "local_writer_native_connection_tests.rs"]
mod native_connection_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn native_host_shutdown_requires_actual_writer_join() {
        struct Block(mpsc::SyncSender<()>, mpsc::Receiver<()>);
        impl Write for Block {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.send(()).unwrap();
                self.1
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(std::io::Error::other)?;
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let registry = OwnerRegistry::default();
        let mut owner = WriterHostOwner::from_registry(&registry, [1; 32], 1).unwrap();
        let target = Target {
            host: [1; 32],
            incarnation: 1,
            session: [2; 16],
            run: "TEST_ONLY_NATIVE_HOST".into(),
            generation: 1,
        };
        let dir = tempfile::tempdir().unwrap();
        let (entered_tx, entered) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let mut started = owner
            .start(
                Journal::open(&dir.path().join("native.sqlite")).unwrap(),
                Block(entered_tx, release_rx),
                SessionStart {
                    target,
                    device: [3; 32],
                    writer_epoch: 1,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 3,
                    columns: 10,
                },
            )
            .unwrap();
        let response = started.output.feed_at(0, b"\x1b[c".to_vec()).unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut host = NativeHost::from_owner(owner);
        host.begin_shutdown();
        assert_eq!(host.observe_shutdown(), WriterRetirement::Pending);
        assert!(host.closing);
        release.send(()).unwrap();
        let _ = response.wait(Duration::from_secs(2));
        let end = Instant::now() + Duration::from_secs(2);
        while host.observe_shutdown() == WriterRetirement::Pending {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
    }

    #[tokio::test]
    async fn native_host_retains_original_install_after_waiter_leaves_and_disconnect_does_not_seal()
    {
        use crate::bound_test_support as support;
        use std::sync::atomic::AtomicU64;
        let (c, h, _gate, _cancel, grant) = support::channels(Arc::new(AtomicU64::new(1000))).await;
        let c = Arc::new(c);
        assert!(matches!(
            NativeHost::from_channel(c.clone()),
            Err(NativeHostError::Authority)
        ));
        let (mut c2, mut h2, _cancel2) = support::pair().await;
        c2.install(&grant).unwrap();
        h2.install(&grant).unwrap();
        let c2 = c2.start().unwrap();
        let h2 = Arc::new(h2.start().unwrap());
        let mut owner =
            WriterHostOwner::from_registry(&OwnerRegistry::default(), support::host(), 11).unwrap();
        owner.principal = Some([4; 32]);
        let target = Target {
            host: support::host(),
            session: [2; 16],
            run: support::scope().run,
            generation: 3,
            incarnation: 11,
        };
        let dir = tempfile::tempdir().unwrap();
        struct InstallBlock(mpsc::SyncSender<()>, mpsc::Receiver<()>);
        impl Write for InstallBlock {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.send(()).unwrap();
                self.1
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(std::io::Error::other)?;
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (entered_tx, entered) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let mut started = owner
            .start_authorized(
                Journal::open(&dir.path().join("install.sqlite")).unwrap(),
                InstallBlock(entered_tx, release_rx),
                SessionStart {
                    target: target.clone(),
                    device: support::device(),
                    writer_epoch: 9,
                    key: DigestKey::from_backend_bytes([0x5a; 32]),
                    rows: 3,
                    columns: 10,
                },
                h.authority(),
                [8; 16],
                4,
            )
            .unwrap();
        let output = started.output.feed_at(0, b"\x1b[c".to_vec()).unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut host = NativeHost::from_owner(owner);
        let token = {
            host.begin_rebind(&target, support::device(), 4, h2.clone(), [9; 16])
                .unwrap()
        };
        // The caller has no pending receiver to drop. The original stays in host.
        assert_eq!(host.attempts.len(), 1);
        assert_eq!(host.observe_install(token), InstallObservation::Pending);
        release.send(()).unwrap();
        let _ = output.wait(Duration::from_secs(2));
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            match host.observe_install(token) {
                InstallObservation::Installed { fence: 5 } => break,
                InstallObservation::Pending => assert!(Instant::now() < end),
                other => panic!("unexpected original result: {other:?}"),
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            host.observe_install(token),
            InstallObservation::Installed { fence: 5 }
        );
        assert_eq!(
            host.attempts.len(),
            1,
            "observation must not replay the operation"
        );
        host.disconnect(token).unwrap();
        assert_eq!(host.observe_install(token), InstallObservation::Unknown);
        assert!(
            host.owner.capture(&target).is_ok(),
            "network loss sealed user's writer"
        );
        host.begin_shutdown();
        assert_eq!(
            host.begin_rebind(&target, support::device(), 5, h2.clone(), [9; 16]),
            Err(NativeHostError::Closed)
        );
        while host.observe_shutdown() == WriterRetirement::Pending {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(host.observe_shutdown(), WriterRetirement::Joined);
        host.network_retirement().unwrap().wait().await;
        c.authority().request_close();
        h.authority().request_close();
        c2.authority().request_close();
        h2.authority().request_close();
        let _ = tokio::join!(
            c.wait_closed(),
            h.wait_closed(),
            c2.wait_closed(),
            h2.wait_closed()
        );
    }
}

#[path = "local_writer_peer_coordinator.rs"]
mod peer_coordinator;
