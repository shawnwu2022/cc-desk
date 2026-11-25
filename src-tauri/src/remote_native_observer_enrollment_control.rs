//! One exact owner-approved Relay control operation. These operations never
//! attach to a terminal, open a data route, or restore saved grant authority.
use super::{
    setup::{ApprovedSetup, SetupApproval},
    Error,
};
use crate::native_crypto::host::{HostKeyCustody, NotifyHooks};
use cc_desk_remote_transport::wss::{
    request_host_enrollment, EnrollmentError, HostEnrollmentCommand, HostEnrollmentResponse,
};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) enum ControlFailure {
    Locked,
    Relay(EnrollmentError),
    RetirementUnknown,
}
/// The caller must use an approved endpoint/address selected by the native
/// profile owner, never a renderer's socket address. This runs on a dedicated
/// blocking job so original CNG join/custody cannot block Tokio.
pub(crate) async fn verify_configured_host(
    approval: &SetupApproval,
    address: SocketAddr,
) -> Result<Option<HostEnrollmentResponse>, ControlFailure> {
    let Some(approved) = approval.take().map_err(|_| ControlFailure::Locked)? else {
        return Ok(None);
    };
    request(approved, address, HostEnrollmentCommand::List)
        .await
        .map(Some)
}
pub(crate) async fn request(
    approved: ApprovedSetup,
    address: SocketAddr,
    command: HostEnrollmentCommand,
) -> Result<HostEnrollmentResponse, ControlFailure> {
    let executor = tokio::runtime::Handle::try_current().map_err(|_| ControlFailure::Locked)?;
    tokio::task::spawn_blocking(move || run(executor, approved, address, command))
        .await
        .map_err(|_| ControlFailure::RetirementUnknown)?
}

fn run(
    executor: tokio::runtime::Handle,
    approved: ApprovedSetup,
    address: SocketAddr,
    command: HostEnrollmentCommand,
) -> Result<HostEnrollmentResponse, ControlFailure> {
    let frozen = approved.keys.binding().clone();
    let cap = approved.signing.end();
    let token = approved.signing.token();
    if !approved.signing.matches(&token, cap) || frozen.profile_cap != cap {
        return Err(ControlFailure::Locked);
    }
    let custody = HostKeyCustody::new(token.clone(), cap);
    let result = (|| {
        custody
            .notification_builder()
            .map_err(|_| ControlFailure::Locked)?
            .start(NotifyHooks::default())
            .map_err(|_| ControlFailure::Locked)?;
        let mut owners = custody
            .publisher()
            .map_err(|_| ControlFailure::Locked)?
            .prepare(approved.keys)
            .map_err(|_| ControlFailure::Locked)?;
        let ready_by = cap.min(Instant::now() + Duration::from_secs(2));
        loop {
            custody.advance();
            if Instant::now() >= ready_by || token.is_revoked() {
                return Err(ControlFailure::Locked);
            }
            if let Some(ready) = custody.take_ready().map_err(|_| ControlFailure::Locked)? {
                ready
                    .consume(&custody, &frozen)
                    .map_err(|_| ControlFailure::Locked)?;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let end = cap.min(Instant::now() + Duration::from_secs(5));
        owners
            .stages
            .begin_registration(approved.signing, end)
            .map_err(|_| ControlFailure::Locked)?;
        let response = executor
            .block_on(request_host_enrollment(
                frozen.origin.as_str(),
                address,
                end.saturating_duration_since(Instant::now()),
                &owners.relay,
                command,
            ))
            .map_err(ControlFailure::Relay)?;
        if Instant::now() >= end || token.is_revoked() {
            return Err(ControlFailure::Locked);
        }
        Ok(response)
    })();
    custody.revoke();
    while custody.advance().outstanding {
        std::thread::sleep(Duration::from_millis(1));
    }
    if custody.snapshot().unknown {
        return Err(ControlFailure::RetirementUnknown);
    }
    result
}

impl From<Error> for ControlFailure {
    fn from(_: Error) -> Self {
        Self::Locked
    }
}
