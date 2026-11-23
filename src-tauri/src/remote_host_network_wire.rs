//! 两种封闭授权入口共用的原传输机制；本模块不产生批准或回收证明。
use super::*;
use cc_desk_remote_record::grant::{DataRole, SessionScope};

/// A single-use handoff from the original wire. Callers cannot substitute a
/// channel, approval or deadline when passing this to the run owner.
pub(super) struct OriginalObserverGrant {
    channel: Arc<RunningChannel>,
    approval: NativeApprovalStop,
    deadline: Instant,
}
impl OriginalObserverGrant {
    pub(super) fn into_parts(self) -> (Arc<RunningChannel>, NativeApprovalStop, Instant) {
        (self.channel, self.approval, self.deadline)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum DelegatedWireMode {
    Manual,
    Live,
    LiveSnapshotPull,
}
pub(super) struct WireInputs {
    mode: DelegatedWireMode,
    delegated_signing: Option<
        crate::remote_host_approval::unattended::final_approval::DelegatedSigningAuthorization,
    >,
    #[cfg(test)]
    activation_hook: Option<(bool, Box<dyn FnOnce() + Send>)>,
    #[cfg(windows)]
    stages: Option<crate::native_crypto::host::HostSigningStages>,
    address: SocketAddr,
    relay: RelayAdmissionIdentity,
    tunnel: [u8; 16],
    relay_mode: transport::wss::RelayMode,
    local: LocalIdentity,
    phone_pin: PeerPin,
    binding: HostSnapshot,
    browser: BrowserProfile,
    issuer: P256Spki,
    principal: [u8; 32],
    policy: NativeGrantPolicy,
    io_budget: Duration,
    #[cfg(test)]
    outer_accepted: Option<Box<TestAcceptedGate>>,
}
impl WireInputs {
    #[cfg(test)]
    pub(super) fn test_outer_observer(
        mut self,
        accepted: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        self.outer_accepted = Some(Box::new((accepted, None)));
        self
    }
    #[cfg(test)]
    pub(super) fn test_activation_hook(
        mut self,
        after: bool,
        hook: Box<dyn FnOnce() + Send>,
    ) -> Self {
        self.activation_hook = Some((after, hook));
        self
    }
    #[cfg(test)]
    pub(super) fn test_live(mut self) -> Self {
        self.mode = DelegatedWireMode::Live;
        self
    }
    #[cfg(test)]
    pub(super) fn test_live_snapshot_pull(mut self) -> Self {
        self.mode = DelegatedWireMode::LiveSnapshotPull;
        self
    }
    #[cfg(test)]
    pub(super) fn from_test(inputs: super::delegated::TestMaterial) -> Self {
        Self {
            mode: DelegatedWireMode::Manual,
            delegated_signing: None,
            #[cfg(test)]
            activation_hook: None,
            #[cfg(windows)]
            stages: None,
            address: inputs.address,
            relay: inputs.relay,
            tunnel: inputs.tunnel,
            relay_mode: transport::wss::RelayMode::Legacy,
            local: inputs.local,
            phone_pin: inputs.phone_pin,
            binding: inputs.binding,
            browser: inputs.browser,
            issuer: inputs.issuer,
            principal: inputs.principal,
            policy: inputs.policy,
            io_budget: Duration::from_secs(2),
            outer_accepted: inputs.accepted,
        }
    }
    pub(super) fn from_delegated(
        authority: crate::remote_host_approval::unattended::final_approval::DelegatedSigningAuthorization,
        signers: crate::native_crypto::host::HostSignerSet,
    ) -> Result<Self, HostNetworkError> {
        let record = authority.record();
        let browser = &record.browser;
        let route = &record.route;
        let origin = cc_desk_remote_record::appbind::CanonicalOrigin::trusted(&route.origin)
            .map_err(|_| HostNetworkError::Authority)?;
        let endpoint =
            P256Spki::new(&record.native.endpoint_spki).map_err(|_| HostNetworkError::Authority)?;
        let local = LocalIdentity::new(security::Role::Host, endpoint, signers.endpoint)
            .map_err(|_| HostNetworkError::Authority)?;
        let activation = authority.host_identity().activation();
        let incarnation = authority.host_identity().incarnation();
        let remaining = authority
            .data_end()
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(HostNetworkError::Closed);
        }
        Ok(Self {
            mode: DelegatedWireMode::LiveSnapshotPull,
            #[cfg(test)]
            activation_hook: None,
            stages: Some(signers.stages),
            address: route
                .address
                .parse()
                .map_err(|_| HostNetworkError::Authority)?,
            relay: signers.relay,
            tunnel: route.tunnel,
            relay_mode: match route.relay_mode.as_str() {
                "legacyV1" => transport::wss::RelayMode::Legacy,
                "rendezvousV1" => transport::wss::RelayMode::RendezvousV1,
                _ => return Err(HostNetworkError::Authority),
            },
            local,
            phone_pin: PeerPin::new(
                security::Role::Client,
                P256Spki::new(&browser.browser_public_key)
                    .map_err(|_| HostNetworkError::Authority)?,
            ),
            binding: HostSnapshot {
                host_key_epoch: record.epochs.host_key,
                client_key_epoch: record.epochs.client_key,
                policy_epoch: record.epochs.policy,
                issuer_epoch: record.epochs.issuer,
                activation,
                incarnation,
                origin,
            },
            browser: BrowserProfile::enrolled(
                &browser.origin,
                &browser.rp_id,
                browser.credential_id.clone(),
                P256Spki::new(&browser.webauthn_public_key)
                    .map_err(|_| HostNetworkError::Authority)?,
                browser.backup_eligible,
                browser.allow_backed_up,
            )
            .map_err(|_| HostNetworkError::Authority)?,
            issuer: P256Spki::new(&browser.grant_public_key)
                .map_err(|_| HostNetworkError::Authority)?,
            principal: record.principal,
            policy: NativeGrantPolicy {
                scope: authority.scope().clone(),
                role: cc_desk_remote_record::grant::DataRole::Controller,
                duration: remaining,
            },
            io_budget: Duration::from_secs(2),
            #[cfg(test)]
            outer_accepted: None,
            delegated_signing: Some(authority),
        })
    }
    pub(super) fn from_approved(inputs: ApprovedHostInputs) -> Self {
        Self {
            mode: DelegatedWireMode::Manual,
            delegated_signing: None,
            #[cfg(test)]
            activation_hook: None,
            #[cfg(windows)]
            stages: inputs.stages,
            address: inputs.address,
            relay: inputs.relay,
            tunnel: inputs.tunnel,
            relay_mode: inputs.relay_mode,
            local: inputs.local,
            phone_pin: inputs.phone_pin,
            binding: inputs.binding,
            browser: inputs.browser,
            issuer: inputs.issuer,
            principal: inputs.principal,
            policy: inputs.policy,
            io_budget: inputs.io_budget,
            #[cfg(test)]
            outer_accepted: inputs.outer_accepted,
        }
    }
}
enum WireAuthority<'a> {
    Original(Arc<dyn GrantClock>),
    Delegated(&'a crate::remote_unattended::capability::network::NetworkBudget),
}
impl WireAuthority<'_> {
    fn clock(&self) -> Arc<dyn GrantClock> {
        match self {
            Self::Original(clock) => clock.clone(),
            Self::Delegated(budget) => budget.clock(),
        }
    }
}
pub(super) struct NetworkWire {
    started: bool,
    live_requested: bool,
    activation: Option<transport::DataKeepaliveActivation>,
    #[cfg(test)]
    activation_hook: Option<(bool, Box<dyn FnOnce() + Send>)>,
    cancel: security::CancelHandle,
    token: Option<security::CancelToken>,
    mux: Option<transport::OwnedTunnel>,
    pub(super) running: Option<Arc<RunningChannel>>,
    observer_source: Option<(Arc<ApprovedHostSource>, SessionScope)>,
    gate: Option<GrantGate>,
    #[cfg(windows)]
    signing_stages: Option<crate::native_crypto::host::HostSigningStages>,
    #[cfg(test)]
    pub(super) mux_termination: Option<tokio::sync::oneshot::Receiver<transport::TestOnlyMuxEnd>>,
}
impl NetworkWire {
    pub(super) fn new() -> Self {
        let (cancel, token) = security::cancellation();
        Self {
            started: false,
            live_requested: false,
            activation: None,
            #[cfg(test)]
            activation_hook: None,
            cancel,
            token: Some(token),
            mux: None,
            running: None,
            observer_source: None,
            gate: None,
            #[cfg(windows)]
            signing_stages: None,
            #[cfg(test)]
            mux_termination: None,
        }
    }
    pub(super) async fn setup(
        &mut self,
        inputs: WireInputs,
        connector: Connector,
        clock: Arc<dyn GrantClock>,
        end: Instant,
        approval_stop: Option<&NativeApprovalStop>,
        original_source: Arc<ApprovedHostSource>,
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        let observer_scope =
            (inputs.policy.role == DataRole::Observer).then(|| inputs.policy.scope.clone());
        let channel = self
            .setup_inner(
                inputs,
                connector,
                WireAuthority::Original(clock),
                end,
                approval_stop,
                None,
            )
            .await?;
        self.observer_source = observer_scope.map(|scope| (original_source, scope));
        Ok(channel)
    }

    pub(super) fn mint_original_observer_handoff(
        &mut self,
        registered: &Arc<ApprovedHostSource>,
        approval: &NativeApprovalStop,
        end: Instant,
    ) -> Result<OriginalObserverGrant, HostNetworkError> {
        let (source, scope) = self
            .observer_source
            .as_ref()
            .ok_or(HostNetworkError::Locked)?;
        if !Arc::ptr_eq(source, registered) || *source.revoked.borrow() {
            return Err(HostNetworkError::Authority);
        }
        #[cfg(windows)]
        let matched = self
            .signing_stages
            .as_ref()
            .is_some_and(|stages| stages.matches_approval(approval));
        #[cfg(not(windows))]
        let matched = false;
        if !matched || approval.is_invalid() {
            return Err(HostNetworkError::Authority);
        }
        let channel = self.running.as_ref().ok_or(HostNetworkError::Locked)?;
        let authority = channel.authority();
        if !channel.peer_install_enabled()
            || authority.local_role() != security::Role::Host
            || authority.data_role() != DataRole::Observer
            || authority.scope() != scope
            || authority.control_lease_renew_enabled()
            || authority.data_keepalive_negotiated()
            || authority.check().is_err()
        {
            return Err(HostNetworkError::Authority);
        }
        let deadline = end.min(Instant::from_std(approval.deadline()));
        remaining(deadline)?;
        let handoff = OriginalObserverGrant {
            channel: channel.clone(),
            approval: approval.clone(),
            deadline,
        };
        self.observer_source = None;
        Ok(handoff)
    }
    pub(super) async fn setup_delegated(
        &mut self,
        inputs: WireInputs,
        connector: Connector,
        budget: &crate::remote_unattended::capability::network::NetworkBudget,
        end: Instant,
        wait: &transport::wss::RendezvousWait,
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        budget.check().map_err(|_| HostNetworkError::Authority)?;
        self.setup_inner(
            inputs,
            connector,
            WireAuthority::Delegated(budget),
            end,
            None,
            Some(wait),
        )
        .await
    }
    async fn setup_inner(
        &mut self,
        inputs: WireInputs,
        connector: Connector,
        authorization: WireAuthority<'_>,
        end: Instant,
        approval_stop: Option<&NativeApprovalStop>,
        wait: Option<&transport::wss::RendezvousWait>,
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        let clock = authorization.clock();
        if self.started {
            return Err(HostNetworkError::Closed);
        }
        self.started = true;
        let live = matches!(
            inputs.mode,
            DelegatedWireMode::Live | DelegatedWireMode::LiveSnapshotPull
        );
        if live && matches!(authorization, WireAuthority::Original(_)) {
            return Err(HostNetworkError::Authority);
        }
        self.live_requested = live;
        #[cfg(test)]
        {
            self.activation_hook = inputs.activation_hook;
        }
        #[cfg(test)]
        let mut outer_accepted = inputs.outer_accepted;
        #[cfg(windows)]
        {
            self.signing_stages = inputs.stages;
        }
        let budget = remaining(end)?;
        #[cfg(windows)]
        if let Some(stages) = &mut self.signing_stages {
            let scope = match &authorization {
                WireAuthority::Original(_) => NetworkSigningScope::original(
                    approval_stop.ok_or(HostNetworkError::Authority)?.clone(),
                    end.into_std(),
                ),
                WireAuthority::Delegated(_) => NetworkSigningScope::delegated(
                    inputs
                        .delegated_signing
                        .ok_or(HostNetworkError::Authority)?,
                    end.into_std(),
                ),
            };
            stages
                .begin_relay(scope)
                .map_err(|_| HostNetworkError::Authority)?;
        }
        let wait = wait.filter(|_| inputs.relay_mode == transport::wss::RelayMode::RendezvousV1);
        let socket = match connector {
            Connector::WebPki => {
                if let Some(wait) = wait {
                    transport::wss::connect_admitted_observed(
                        inputs.binding.origin.as_str(),
                        inputs.address,
                        budget,
                        &inputs.relay,
                        inputs.relay_mode,
                        inputs.tunnel,
                        wait,
                    )
                    .await
                } else {
                    transport::wss::connect_admitted_with_mode(
                        inputs.binding.origin.as_str(),
                        inputs.address,
                        budget,
                        &inputs.relay,
                        inputs.relay_mode,
                        inputs.tunnel,
                    )
                    .await
                }
            }
            #[cfg(test)]
            Connector::PublicLoopbackFixture => {
                if let Some(wait) = wait {
                    transport::wss::test_only_connect_admitted_loopback_observed(
                        inputs.binding.origin.as_str(),
                        inputs.address,
                        budget,
                        &inputs.relay,
                        inputs.relay_mode,
                        inputs.tunnel,
                        wait,
                    )
                    .await
                } else {
                    transport::wss::test_only_connect_admitted_loopback_with_mode(
                        inputs.binding.origin.as_str(),
                        inputs.address,
                        budget,
                        &inputs.relay,
                        inputs.relay_mode,
                        inputs.tunnel,
                    )
                    .await
                }
            }
        }
        .map_err(|error| {
            if wait.is_some() && error == transport::wss::ConnectError::Deadline {
                HostNetworkError::Deadline
            } else {
                HostNetworkError::Relay
            }
        })?;
        #[cfg(test)]
        eprintln!("TEST_ONLY_HOST_OUTER_ACCEPTED");
        #[cfg(test)]
        if let Some(accepted) = outer_accepted.take() {
            let (accepted, proceed) = *accepted;
            let _ = accepted.send(());
            // 仅测试中的原任务调度门；仍受原 setup select/end 保管，不改变 mux 的 IO 预算。
            if let Some(proceed) = proceed {
                proceed.await.map_err(|_| HostNetworkError::Closed)?;
            }
        }
        #[cfg(windows)]
        if let Some(stages) = &mut self.signing_stages {
            stages
                .arm_endpoint()
                .map_err(|_| HostNetworkError::Authority)?;
        }
        let (stream, owner) = if live {
            let WireAuthority::Delegated(budget) = &authorization else {
                return Err(HostNetworkError::Authority);
            };
            let mut cancelled = self.token.as_ref().ok_or(HostNetworkError::Closed)?.clone();
            let (stream, owner, activation) = transport::start_admitted_data_keepalive_dormant(
                socket,
                inputs.tunnel,
                inputs.io_budget,
                end,
                Instant::from_std(budget.deadline()),
                async move { cancelled.cancelled().await },
            )
            .map_err(|_| HostNetworkError::Relay)?;
            self.activation = Some(activation);
            (stream, owner)
        } else {
            #[cfg(not(test))]
            let (stream, owner) =
                transport::start_admitted(socket, inputs.tunnel, inputs.io_budget)
                    .map_err(|_| HostNetworkError::Relay)?;
            #[cfg(test)]
            let (stream, owner) = {
                let (stream, owner, ending) = transport::test_only_start_admitted_observed(
                    socket,
                    inputs.tunnel,
                    inputs.io_budget,
                )
                .map_err(|_| HostNetworkError::Relay)?;
                self.mux_termination = Some(ending);
                (stream, owner)
            };

            (stream, owner)
        };
        self.mux = Some(owner);
        #[cfg(test)]
        eprintln!("TEST_ONLY_HOST_TLS_START");
        let pending = security::accept(
            security::Unbound::new(stream),
            inputs.local,
            inputs.phone_pin,
            remaining(end)?,
            self.token.take().expect("original cancellation token"),
        )
        .await
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("TEST_ONLY_HOST_TLS_ERROR {error:?}");
            #[cfg(not(test))]
            let _ = error;
            HostNetworkError::Security
        })?;
        let managed = match &authorization {
            WireAuthority::Original(_) => {
                security::bind_host_peer_channel(pending, inputs.binding).await
            }
            WireAuthority::Delegated(_) => {
                if inputs.mode == DelegatedWireMode::LiveSnapshotPull {
                    security::bind_host_live_snapshot_pull_channel(pending, inputs.binding).await
                } else if live {
                    security::bind_host_live_control_channel(pending, inputs.binding).await
                } else {
                    security::bind_host_control_lease_channel(pending, inputs.binding).await
                }
            }
        }
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("TEST_ONLY_HOST_BIND_ERROR {error:?}");
            #[cfg(not(test))]
            let _ = error;
            HostNetworkError::Security
        })?;
        let mut authority = GrantGate::for_binding(
            managed.context(),
            inputs.issuer,
            inputs.principal,
            clock.clone(),
        )
        .map_err(|_| HostNetworkError::Grant)?;
        authority
            .require_browser(inputs.browser)
            .map_err(|_| HostNetworkError::Grant)?;
        if let WireAuthority::Delegated(budget) = &authorization {
            budget
                .limit_gate(&mut authority)
                .map_err(|_| HostNetworkError::Authority)?;
        }
        self.gate = Some(authority);
        let mut managed = managed
            .grant_host(
                self.gate.as_mut().expect("original GrantGate"),
                inputs.policy,
                remaining(end)?,
            )
            .await
            .map_err(|_| HostNetworkError::Grant)?;
        let channel = Arc::new(
            managed
                .start_peer_pending(end)
                .map_err(|_| HostNetworkError::Handoff)?,
        );
        self.running = Some(channel.clone());
        Ok(channel)
    }
    // 仅原 wire/channel 在原 setup task 内观察缓存；不读取 PeerPump 的事件或证明。
    pub(super) fn requires_live_activation(&self) -> bool {
        self.live_requested
    }
    pub(super) async fn activate_live_after_install(
        &mut self,
        budget: &crate::remote_unattended::capability::network::NetworkBudget,
        setup_end: Instant,
        stop: &watch::Sender<bool>,
    ) -> Result<(), HostNetworkError> {
        if !self.live_requested {
            return Ok(());
        }
        let result = async {
            let channel = self.running.as_ref().ok_or(HostNetworkError::Closed)?;
            loop {
                remaining(setup_end)?;
                if *stop.borrow() {
                    return Err(HostNetworkError::Closed);
                }
                budget.check().map_err(|_| HostNetworkError::Authority)?;
                if let Some(fact) = channel
                    .live_keepalive_fact(security::Role::Host)
                    .map_err(|_| HostNetworkError::Authority)?
                {
                    let clock = budget.clock();
                    if !fact.uses_clock(&clock) {
                        return Err(HostNetworkError::Authority);
                    }
                    #[cfg(test)]
                    if self
                        .activation_hook
                        .as_ref()
                        .is_some_and(|(after, _)| !*after)
                    {
                        (self.activation_hook.take().unwrap().1)();
                    }
                    if *stop.borrow() {
                        return Err(HostNetworkError::Closed);
                    }
                    let sample_before = Instant::now();
                    let left = fact
                        .remaining_ms_for(channel, security::Role::Host)
                        .map_err(|_| HostNetworkError::Authority)?;
                    let fixed_end = sample_before
                        .checked_add(Duration::from_millis(left))
                        .ok_or(HostNetworkError::Deadline)?
                        .min(Instant::from_std(budget.deadline()));
                    budget.check().map_err(|_| HostNetworkError::Authority)?;
                    fact.validate_for(channel, security::Role::Host)
                        .map_err(|_| HostNetworkError::Authority)?;
                    remaining(setup_end)?;
                    self.activation
                        .take()
                        .ok_or(HostNetworkError::Closed)?
                        .activate(fixed_end)
                        .map_err(|_| HostNetworkError::Authority)?;
                    #[cfg(test)]
                    if let Some((_, hook)) = self.activation_hook.take() {
                        hook();
                    }
                    if *stop.borrow() {
                        return Err(HostNetworkError::Closed);
                    }
                    budget.check().map_err(|_| HostNetworkError::Authority)?;
                    fact.validate_for(channel, security::Role::Host)
                        .map_err(|_| HostNetworkError::Authority)?;
                    if !fact.uses_clock(&budget.clock()) {
                        return Err(HostNetworkError::Authority);
                    }
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        .await;
        if result.is_err() {
            self.cancel.cancel();
            if let Some(mux) = &self.mux {
                mux.request_close();
            }
        }
        result
    }
    pub(super) async fn close_and_join(&mut self) -> bool {
        #[cfg(windows)]
        if let Some(stages) = &self.signing_stages {
            stages.cancel();
        }
        // 先通知全部 owner 再等待；取消外部观察者不能移走原始句柄。
        if let Some(gate) = &mut self.gate {
            gate.revoke();
        }
        self.cancel.cancel();
        if let Some(channel) = &self.running {
            channel.authority().request_close();
        }
        if let Some(owner) = &self.mux {
            owner.request_close();
        }
        let (channel_known, mux_known) = tokio::join!(
            async {
                if let Some(channel) = &self.running {
                    channel.wait_closed_checked().await.is_ok()
                } else {
                    true
                }
            },
            async {
                if let Some(owner) = &self.mux {
                    owner.wait_closed().await.is_ok()
                } else {
                    true
                }
            },
        );
        channel_known && mux_known
    }
}

#[cfg(all(test, windows))]
mod observer_handoff_tests {
    use super::*;
    use crate::{
        native_crypto::{
            epoch_test_supplier,
            host::{FrozenHostBinding, HostKeyCustody, HostKeyProfileIdentity},
            host_profile::NativeHostProfile,
            owner::NativeRevocation,
            EpochTestTrace,
        },
        tests::remote_channels,
    };

    #[tokio::test]
    async fn original_observer_handoff_matches_exact_source_approval_and_grant_once() {
        let scope = SessionScope {
            session: "42".repeat(16),
            run: "TEST_ONLY_ORIGINAL".into(),
            generation: "1".into(),
        };
        let clock: Arc<dyn GrantClock> = Arc::new(|| Ok(1000));
        let (peer, host, _gate, cancel) =
            remote_channels::channels_with_role(scope.clone(), clock.clone(), DataRole::Observer)
                .await;
        let channel = Arc::new(host);
        let (_stop, revoked) = watch::channel(false);
        let origin = Arc::new(ApprovedHostSource {
            clock: clock.clone(),
            revoked: revoked.clone(),
            expires_host_ms: 10_000,
        });
        // Equal fields do not make an independently allocated source the original.
        let other = Arc::new(ApprovedHostSource {
            clock,
            revoked,
            expires_host_ms: 10_000,
        });
        let cap = std::time::Instant::now() + Duration::from_secs(10);
        let token = NativeRevocation::new();
        let key_owner = HostKeyCustody::reserve_with(token.clone(), cap, Default::default());
        let binding = FrozenHostBinding {
            native_profile: HostKeyProfileIdentity {
                id: [0x42; 16],
                revision: 1,
            },
            endpoint_spki: remote_channels::public_test_spki(false),
            relay_spki: remote_channels::public_test_spki(true),
            origin: cc_desk_remote_record::appbind::CanonicalOrigin::trusted(
                "wss://relay.invalid/",
            )
            .unwrap(),
            relay_id: [0x42; 16],
            profile_cap: cap,
        };
        let mock = epoch_test_supplier(
            NativeHostProfile::new(binding, "TEST_ONLY_OBSERVER_EP", "TEST_ONLY_OBSERVER_RELAY")
                .unwrap()
                .freeze(),
            token.clone(),
            1,
            &EpochTestTrace::default(),
        );
        let signing = key_owner.publisher().unwrap().prepare(mock).unwrap();
        let approval = NativeApprovalStop::new(token.clone(), cap);
        let mut wire = NetworkWire::new();
        // The real signed duplex grant substitutes only for outer wire setup.
        wire.running = Some(channel.clone());
        wire.signing_stages = Some(signing.stages);
        wire.observer_source = Some((origin.clone(), scope));
        let end = Instant::now() + Duration::from_millis(500);
        assert!(matches!(
            wire.mint_original_observer_handoff(&other, &approval, end),
            Err(HostNetworkError::Authority)
        ));
        for different in [
            NativeApprovalStop::new(NativeRevocation::new(), cap),
            NativeApprovalStop::new(token.clone(), cap - Duration::from_secs(1)),
            NativeApprovalStop::new(token.clone(), std::time::Instant::now()),
        ] {
            assert!(matches!(
                wire.mint_original_observer_handoff(&origin, &different, end),
                Err(HostNetworkError::Authority)
            ));
        }
        assert!(
            !token.is_revoked(),
            "foreign cap touched the original permit"
        );
        let granted = wire
            .mint_original_observer_handoff(&origin, &approval, end)
            .unwrap();
        let (bound_channel, bound_approval, bound_end) = granted.into_parts();
        assert!(Arc::ptr_eq(&bound_channel, &channel));
        assert!(bound_approval.matches_native(&token, cap));
        assert_eq!(bound_end, end);
        assert!(matches!(
            wire.mint_original_observer_handoff(&origin, &approval, end),
            Err(HostNetworkError::Locked)
        ));

        cancel.cancel();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), wire.close_and_join())
                .await
                .unwrap()
        );
        tokio::time::timeout(Duration::from_secs(2), peer.wait_closed_checked())
            .await
            .unwrap()
            .unwrap();
        key_owner.revoke();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let retired = key_owner.advance();
                assert!(!retired.unknown);
                if !retired.outstanding {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
