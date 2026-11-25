//! Exact owner-approved enrollment operations. The original CNG custody is
//! joined off Tokio; a retirement error is not an authorizing result.
use super::{
    setup::{ApprovedSetup, SetupApproval},
    Error, ObserverApproval, SignedObserverReceipt,
};
use crate::native_crypto::host::{HostKeyCustody, HostKeysReady, HostSignerSet, NotifyHooks};
use cc_desk_remote_transport::wss::{
    request_host_enrollment, EnrollmentError, HostEnrollmentCommand, HostEnrollmentResponse,
};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) enum ControlFailure {
    Locked,
    Relay(EnrollmentError),
    RetirementUnknown,
}
type Result<T> = std::result::Result<T, ControlFailure>;

pub(crate) async fn verify_configured_host(
    approval: &SetupApproval,
    address: SocketAddr,
) -> Result<Option<HostEnrollmentResponse>> {
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
) -> Result<HostEnrollmentResponse> {
    on_worker(
        approved,
        move |executor, custody, mut owners, ready, expected, signing| {
            ready
                .consume(custody, expected)
                .map_err(|_| ControlFailure::Locked)?;
            let end = signing.end().min(Instant::now() + Duration::from_secs(5));
            owners
                .stages
                .begin_registration(signing, end)
                .map_err(|_| ControlFailure::Locked)?;
            let response = executor
                .block_on(request_host_enrollment(
                    expected.origin.as_str(),
                    address,
                    end.saturating_duration_since(Instant::now()),
                    &owners.relay,
                    command,
                ))
                .map_err(ControlFailure::Relay)?;
            if Instant::now() >= end {
                return Err(ControlFailure::Locked);
            }
            Ok(response)
        },
    )
    .await
}
pub(crate) async fn sign_membership(
    approved: ApprovedSetup,
    consent: Arc<ObserverApproval>,
) -> Result<Option<SignedObserverReceipt>> {
    on_worker(
        approved,
        move |executor, custody, _signers, ready, expected, _scope| {
            let endpoint = custody
                .observer_endpoint_handle(ready, expected)
                .map_err(|_| ControlFailure::Locked)?;
            executor
                .block_on(consent.sign(None, &endpoint))
                .map_err(ControlFailure::from)
        },
    )
    .await
}
async fn on_worker<T: Send + 'static>(
    approved: ApprovedSetup,
    operation: impl FnOnce(
            &tokio::runtime::Handle,
            &HostKeyCustody,
            HostSignerSet,
            HostKeysReady,
            &crate::native_crypto::host::FrozenHostBinding,
            crate::remote_host_approval::RegistrationSigningScope,
        ) -> Result<T>
        + Send
        + 'static,
) -> Result<T> {
    let executor = tokio::runtime::Handle::try_current().map_err(|_| ControlFailure::Locked)?;
    tokio::task::spawn_blocking(move || {
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
            let owners = custody
                .publisher()
                .map_err(|_| ControlFailure::Locked)?
                .prepare(approved.keys)
                .map_err(|_| ControlFailure::Locked)?;
            let ready_by = cap.min(Instant::now() + Duration::from_secs(2));
            let ready = loop {
                custody.advance();
                if Instant::now() >= ready_by || token.is_revoked() {
                    return Err(ControlFailure::Locked);
                }
                if let Some(ready) = custody.take_ready().map_err(|_| ControlFailure::Locked)? {
                    break ready;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            operation(
                &executor,
                &custody,
                owners,
                ready,
                &frozen,
                approved.signing,
            )
        })();
        custody.revoke();
        while custody.advance().outstanding {
            std::thread::sleep(Duration::from_millis(1));
        }
        if custody.snapshot().unknown {
            return Err(ControlFailure::RetirementUnknown);
        }
        result
    })
    .await
    .map_err(|_| ControlFailure::RetirementUnknown)?
}
impl From<Error> for ControlFailure {
    fn from(_: Error) -> Self {
        Self::Locked
    }
}
