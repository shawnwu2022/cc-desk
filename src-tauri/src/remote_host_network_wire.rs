//! 两种封闭授权入口共用的原传输机制；本模块不产生批准或回收证明。
use super::*;
pub(super) struct WireInputs {
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
    pub(super) fn from_test(inputs: super::delegated::TestMaterial) -> Self {
        Self {
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
    pub(super) fn from_approved(inputs: ApprovedHostInputs) -> Self {
        Self {
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
    cancel: security::CancelHandle,
    token: Option<security::CancelToken>,
    mux: Option<transport::OwnedTunnel>,
    pub(super) running: Option<Arc<RunningChannel>>,
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
            cancel,
            token: Some(token),
            mux: None,
            running: None,
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
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        self.setup_inner(
            inputs,
            connector,
            WireAuthority::Original(clock),
            end,
            approval_stop,
        )
        .await
    }
    pub(super) async fn setup_delegated(
        &mut self,
        inputs: WireInputs,
        connector: Connector,
        budget: &crate::remote_unattended::capability::network::NetworkBudget,
        end: Instant,
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        budget.check().map_err(|_| HostNetworkError::Authority)?;
        self.setup_inner(
            inputs,
            connector,
            WireAuthority::Delegated(budget),
            end,
            None,
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
    ) -> Result<Arc<RunningChannel>, HostNetworkError> {
        let clock = authorization.clock();
        if self.started {
            return Err(HostNetworkError::Closed);
        }
        self.started = true;
        #[cfg(test)]
        let mut outer_accepted = inputs.outer_accepted;
        #[cfg(windows)]
        {
            self.signing_stages = inputs.stages;
        }
        let budget = remaining(end)?;
        #[cfg(windows)]
        if let Some(stages) = &mut self.signing_stages {
            let approval = approval_stop.ok_or(HostNetworkError::Authority)?.clone();
            stages
                .begin_relay(NetworkSigningScope {
                    approval,
                    end: end.into_std(),
                })
                .map_err(|_| HostNetworkError::Authority)?;
        }
        let socket = match connector {
            Connector::WebPki => {
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
            #[cfg(test)]
            Connector::PublicLoopbackFixture => {
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
        .map_err(|_| HostNetworkError::Relay)?;
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
        #[cfg(not(test))]
        let (stream, owner) = transport::start_admitted(socket, inputs.tunnel, inputs.io_budget)
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
        let managed = security::bind_host_peer_channel(pending, inputs.binding)
            .await
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
