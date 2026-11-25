//! Native Phone data plane. This does not alter the Browser/WebAuthn gate.
use super::{
    control::{self, ControlFailure},
    setup::TrustedHostProfile,
    store::DeviceSummary,
    *,
};
use crate::remote_host_network::NativeApprovalStop;
use cc_desk_remote_record::grant::DataRole;
use cc_desk_remote_security as security;
use cc_desk_remote_transport as transport;
use std::net::SocketAddr;
use tokio::time::Instant as TokioInstant;

pub(crate) struct Active {
    stop: security::CancelHandle,
    armed: tokio::sync::oneshot::Receiver<()>,
    installed: Arc<std::sync::atomic::AtomicBool>,
    task: tokio::task::JoinHandle<control::Result<()>>,
}
impl Active {
    pub(crate) async fn arm_receipt(&mut self, end: Instant) -> Result<()> {
        tokio::time::timeout_at(TokioInstant::from_std(end), &mut self.armed)
            .await
            .map_err(|_| Error::Expired)?
            .map_err(|_| Error::Authority)
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
    pub(crate) fn installed(&self) -> bool {
        !self.task.is_finished() && self.installed.load(Ordering::Acquire)
    }
    pub(crate) async fn retire(&mut self) -> Result<()> {
        self.stop.cancel();
        match tokio::time::timeout(Duration::from_secs(4), &mut self.task).await {
            Ok(Ok(Err(ControlFailure::RetirementUnknown))) | Ok(Err(_)) | Err(_) => {
                Err(Error::Authority)
            }
            Ok(Ok(_)) => Ok(()),
        }
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn lease_current(lease: &NativeApprovalStop, stop: &security::CancelToken) -> bool {
    !lease.is_invalid() && !stop.is_cancelled()
}
fn controller_allowed(
    role: DataRole,
    renewable: bool,
    lease: &NativeApprovalStop,
    stop: &security::CancelToken,
) -> bool {
    role == DataRole::Controller && renewable && lease_current(lease, stop)
}

#[cfg(test)]
#[test]
#[allow(non_snake_case)]
fn NativeController_ExactRoleAndOriginalLifetime_001() {
    use crate::native_crypto::owner::NativeRevocation;
    let owner = NativeRevocation::new();
    let live = NativeApprovalStop::new(owner.clone(), Instant::now() + Duration::from_secs(30));
    let (stop, token) = security::cancellation();
    assert!(controller_allowed(
        DataRole::Controller,
        true,
        &live,
        &token
    ));
    assert!(!controller_allowed(DataRole::Observer, true, &live, &token));
    assert!(!controller_allowed(
        DataRole::Controller,
        false,
        &live,
        &token
    ));
    let expired = NativeApprovalStop::new(NativeRevocation::new(), Instant::now());
    assert!(!controller_allowed(
        DataRole::Controller,
        true,
        &expired,
        &token
    ));
    owner.revoke();
    assert!(!controller_allowed(
        DataRole::Controller,
        true,
        &live,
        &token
    ));
    let stopped = NativeApprovalStop::new(
        NativeRevocation::new(),
        Instant::now() + Duration::from_secs(30),
    );
    stop.cancel();
    assert!(!controller_allowed(
        DataRole::Controller,
        true,
        &stopped,
        &token
    ));
}

pub(crate) struct Controller {
    pub(crate) owner: Arc<crate::remote_host::RemoteHostService>,
    pub(crate) runtime: Arc<crate::cli::native_runtime::NativeRuntime>,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    profile: Arc<TrustedHostProfile>,
    address: SocketAddr,
    lifecycle: &Arc<crate::native_crypto::owner::NativeRevocation>,
    phone: DeviceSummary,
    original: NativeRunAdmission,
    controller: Option<Controller>,
    end: Instant,
    rendezvous_end: Instant,
) -> Result<Active> {
    if phone.state != super::store::State::Confirmed || Instant::now() >= end {
        return Err(Error::Authority);
    }
    let exact = original.scope().map_err(|_| Error::Authority)?;
    let (_, run_current) = original.network_checks().map_err(|_| Error::Authority)?;
    encode_scope(&exact).map_err(|_| Error::Authority)?;
    let settings = profile.operation(lifecycle, end)?;
    let (stop, cancel) = security::cancellation();
    let mut cancelled = cancel.clone();
    let (armed_rx, armed) = {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (rx, tx)
    };
    let installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let install_status = installed.clone();
    let task = tokio::spawn(control::on_worker(
        settings,
        move |exec, custody, mut keys, ready, expected, signing| {
            ready
                .consume(custody, expected)
                .map_err(|_| ControlFailure::Locked)?;
            let now = Instant::now();
            let connect_end = rendezvous_end.min(end).min(now + Duration::from_secs(25));
            let live = NativeApprovalStop::new(signing.token(), end);
            let public = security::P256Spki::new(&expected.endpoint_spki)
                .map_err(|_| ControlFailure::Locked)?;
            let local = security::LocalIdentity::new(security::Role::Host, public, keys.endpoint)
                .map_err(|_| ControlFailure::Locked)?;
            let peer = security::P256Spki::new(&phone.phone_endpoint_spki)
                .map_err(|_| ControlFailure::Locked)?;
            let principal = peer.endpoint_id(security::Role::Client);
            let pin = security::PeerPin::new(security::Role::Client, peer);
            let issuer = security::P256Spki::new(&phone.phone_grant_spki)
                .map_err(|_| ControlFailure::Locked)?;
            let epochs = profile.epochs();
            keys.stages
                .begin_registration(signing, connect_end)
                .map_err(|_| ControlFailure::Locked)?;
            if original.scope().ok().as_ref() != Some(&exact) || live.is_invalid() {
                return Err(ControlFailure::Locked);
            }
            armed.send(()).map_err(|_| ControlFailure::Locked)?;
            exec.block_on(async move {
            let socket=tokio::select!{
                biased;
                _=cancelled.cancelled()=>return Ok(()),
                result=transport::wss::connect_admitted_with_mode(expected.origin.as_str(),address,
                    connect_end.saturating_duration_since(Instant::now()),&keys.relay,transport::wss::RelayMode::RendezvousV1,phone.tunnel)=>result.map_err(|_|ControlFailure::Locked)?,
            };
            keys.stages.arm_endpoint().map_err(|_|ControlFailure::Locked)?;
            let (stream,mux)=transport::start_admitted(socket,phone.tunnel,Duration::from_secs(5)).map_err(|_|ControlFailure::Locked)?;
            let outcome=async {
                let pending=security::accept(security::Unbound::new(stream),local,pin,
                    connect_end.saturating_duration_since(Instant::now()).min(Duration::from_secs(10)),cancel).await.map_err(|_|ControlFailure::Locked)?;
                let binding=security::HostSnapshot{host_key_epoch:epochs.host_key,client_key_epoch:epochs.client_key,
                    activation:*Sha256::digest(uuid::Uuid::new_v4().as_bytes()).as_ref(),incarnation:1,
                    policy_epoch:epochs.policy,issuer_epoch:epochs.issuer,origin:expected.origin.clone()};
                let management = if controller.is_some() {
                    security::bind_host_control_lease_channel(pending,binding).await
                } else {
                    security::bind_host_peer_channel(pending,binding).await
                }.map_err(|_|ControlFailure::Locked)?;
                let clock_live=live.clone();
                let clock_cancel=cancelled.clone();
                let clock=Arc::new(move || -> std::result::Result<u64,security::GrantError> {
                    if !lease_current(&clock_live,&clock_cancel) || !run_current() { return Err(security::GrantError::Revoked); }
                    now.elapsed().as_millis().try_into().map_err(|_|security::GrantError::Clock)
                });
                let mut gate=security::GrantGate::for_binding(management.context(),issuer,principal,clock).map_err(|_|ControlFailure::Locked)?;
                // No BrowserProfile is installed; the native grant issuer and
                // TLS endpoint are the independently approved Phone keys.
                // Scope grant lasts only as long as the originating OS approval; the Relay catalog is not the live lease.
                let duration=end.saturating_duration_since(Instant::now()).min(Duration::from_secs(300));
                let budget=connect_end.saturating_duration_since(Instant::now()).min(Duration::from_secs(10));
                let mut managed=management.grant_host(&mut gate,security::NativeGrantPolicy{
                    scope:exact,role:if controller.is_some() {DataRole::Controller}else{DataRole::Observer},duration,
                },budget).await.map_err(|_|ControlFailure::Locked)?;
                let channel=Arc::new(managed.start_peer_pending(TokioInstant::from_std(end)).map_err(|_|ControlFailure::Locked)?);
                if let Some(controller)=controller {
                    if !controller_allowed(channel.authority().data_role(),channel.authority().control_lease_renew_enabled(),&live,&cancelled)
                        || channel.authority().check().is_err() {
                        channel.authority().request_close(); return Err(ControlFailure::Locked);
                    }
                    controller.owner.adopt_admitted(original,channel.clone(),&controller.runtime).map_err(|_|ControlFailure::Locked)?;
                    while !live.is_invalid() && channel.authority().check().is_ok() {
                        if channel.installed_fence().is_ok() {install_status.store(true,Ordering::Release); break;}
                        tokio::select!{_ = cancelled.cancelled()=>break, _=tokio::time::sleep(Duration::from_millis(25))=>{}}
                    }
                    tokio::select!{_=cancelled.cancelled()=>{},_=NativeApprovalStop::wait(Some(&live))=>{},_=channel.wait_closed_checked()=>{}}
                    install_status.store(false,Ordering::Release);
                    channel.authority().request_close();
                    let (closed,pump)=tokio::join!(channel.wait_closed_checked(),controller.owner.wait_native_controller_peer());
                    return if closed.is_ok() && pump {Ok(())} else {Err(ControlFailure::RetirementUnknown)};
                }
                let mut observer=original.start_bound_observer(channel.clone(),live.clone(),TokioInstant::from_std(end)).map_err(|_|ControlFailure::Locked)?;
                tokio::select!{
                    receipt=observer.wait_install_receipt()=>{if receipt.is_some(){install_status.store(true,Ordering::Release);}},
                    _=cancelled.cancelled()=>{},
                    _=NativeApprovalStop::wait(Some(&live))=>{},
                }
                tokio::select!{
                    _=cancelled.cancelled()=>{},
                    _=NativeApprovalStop::wait(Some(&live))=>{},
                    _=channel.wait_closed_checked()=>{},
                }
                install_status.store(false,Ordering::Release);
                match observer.retire().await {
                    Err(e) if e.code == "REMOTE_OBSERVER_RETIREMENT_UNKNOWN" => Err(ControlFailure::RetirementUnknown),
                    _ => Ok(()),
                }
            }.await;
            mux.request_close();
            tokio::time::timeout(Duration::from_secs(2),mux.wait_closed()).await
                .map_err(|_|ControlFailure::RetirementUnknown)?.map_err(|_|ControlFailure::RetirementUnknown)?;
            outcome
        })
        },
    ));
    Ok(Active {
        stop,
        armed: armed_rx,
        installed,
        task,
    })
}
