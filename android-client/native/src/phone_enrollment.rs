//! One explicit Phone enrollment attempt through the existing WebPKI and Relay
//! admission connector. No control/data authority is inferred from status.
use crate::enrollment::{
    EnrollmentAttempt, HostCommitment, InvalidEnrollment, VerifiedEnrollment, verify_approval,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cc_desk_remote_record::appbind::CanonicalOrigin;
use cc_desk_remote_security::P256Spki;
use cc_desk_remote_transport::wss::{
    EnrollmentError, PhoneEnrollmentCommand, PhoneEnrollmentResponse, RelayAdmissionIdentity,
    RelayRole, request_phone_enrollment,
};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
    signature,
};
use serde::Serialize;
use std::{
    future::Future,
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::watch, time::Instant};
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"cc-desk.native-pairing.phone.v1\0";
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const DURATION: Duration = Duration::from_secs(10 * 60);
const MAX_CALL: Duration = Duration::from_secs(3);
const STATUS_INTERVAL: Duration = Duration::from_secs(1);

pub struct ExplicitPairingRequest {
    origin: CanonicalOrigin,
    address: SocketAddr,
    host: HostCommitment,
    code: Zeroizing<String>,
}

impl ExplicitPairingRequest {
    /// Called only after the pairing UI consumed its versioned scanned or
    /// pasteable invite. The typed Host commitment must not come from Relay.
    pub fn new(
        origin: CanonicalOrigin,
        address: SocketAddr,
        host: HostCommitment,
        raw_code: String,
    ) -> Result<Self, PairingError> {
        let code = Zeroizing::new(raw_code);
        if code.len() != 16
            || !code.as_bytes().iter().all(|b| CODE_ALPHABET.contains(b))
            || address.port() == 0
        {
            return Err(PairingError::Invalid);
        }
        Ok(Self {
            origin,
            address,
            host,
            code,
        })
    }
}

pub struct HardwarePairingIdentity {
    pub relay: RelayAdmissionIdentity,
    pub relay_sec1: [u8; 65],
    pub endpoint_spki: [u8; 91],
    pub approval_spki: [u8; 91],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingError {
    Invalid,
    Cancelled,
    Unavailable,
    DeniedOrClosed,
    Expired,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhoneProof<'a> {
    version: u8,
    relay_origin: &'a CanonicalOrigin,
    code_sha256: String,
    request_id: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    phone_grant_spki: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    payload_b64: String,
    signature_b64: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|n| format!("{n:02x}")).collect()
}
fn parse_id(text: &str) -> Result<[u8; 16], PairingError> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(PairingError::Invalid);
    }
    let mut id = [0; 16];
    for (i, p) in id.iter_mut().enumerate() {
        *p = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| PairingError::Invalid)?;
    }
    if id == [0; 16] {
        Err(PairingError::Invalid)
    } else {
        Ok(id)
    }
}
fn digest32(bytes: &[u8]) -> [u8; 32] {
    let mut v = [0; 32];
    v.copy_from_slice(digest::digest(&digest::SHA256, bytes).as_ref());
    v
}
fn budget(end: Instant) -> Result<Duration, PairingError> {
    let d = end.saturating_duration_since(Instant::now()).min(MAX_CALL);
    if d.is_zero() {
        Err(PairingError::Expired)
    } else {
        Ok(d)
    }
}
async fn cancellable<T>(
    stop: &mut watch::Receiver<bool>,
    task: impl Future<Output = T>,
) -> Result<T, PairingError> {
    if *stop.borrow_and_update() || stop.has_changed().is_err() {
        return Err(PairingError::Cancelled);
    }
    tokio::select! { biased; _ = stop.changed() => Err(PairingError::Cancelled), value = task => Ok(value) }
}

fn verified_ids(tunnel: &str, invite: &str) -> Result<([u8; 16], [u8; 16]), PairingError> {
    Ok((parse_id(tunnel)?, parse_id(invite)?))
}

fn wall_ms() -> Result<u64, PairingError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(PairingError::Unavailable)
}

/// Consume an explicit original pairing action. Calls CLAIM at most once,
/// then queries only the same CSPRNG request ID. The retained enrollment owner
/// must cancel and join this future. The signer must make its hardware call on
/// the retained provider and authenticate only the supplied domain transcript.
pub async fn redeem<F>(
    request: ExplicitPairingRequest,
    identity: HardwarePairingIdentity,
    mut stop: watch::Receiver<bool>,
    endpoint_sign: impl FnOnce(Vec<u8>) -> F,
) -> Result<VerifiedEnrollment, PairingError>
where
    F: Future<Output = Result<[u8; 64], PairingError>>,
{
    let (phone_id, role) = identity.relay.binding();
    let endpoint = &identity.endpoint_spki;
    let approval = &identity.approval_spki;
    if !matches!(role, RelayRole::Phone)
        || phone_id == [0; 16]
        || identity.relay_sec1[0] != 4
        || P256Spki::new(endpoint).is_err()
        || P256Spki::new(approval).is_err()
        || endpoint == approval
        || identity.relay_sec1 == endpoint[26..]
        || identity.relay_sec1 == approval[26..]
    {
        return Err(PairingError::Invalid);
    }
    let end = Instant::now()
        .checked_add(DURATION)
        .ok_or(PairingError::Unavailable)?;
    let mut request_id = [0; 16];
    SystemRandom::new()
        .fill(&mut request_id)
        .map_err(|_| PairingError::Unavailable)?;
    if request_id == [0; 16] {
        return Err(PairingError::Unavailable);
    }
    let code_sha256 = digest32(request.code.as_bytes());
    let proof = serde_json::to_vec(&PhoneProof {
        version: 1,
        relay_origin: &request.origin,
        code_sha256: hex(&code_sha256),
        request_id: hex(&request_id),
        phone_id: hex(&phone_id),
        phone_relay_public_key_sec1: hex(&identity.relay_sec1),
        phone_endpoint_spki: hex(endpoint),
        phone_grant_spki: hex(approval),
    })
    .map_err(|_| PairingError::Invalid)?;
    let signed = [DOMAIN, &proof].concat();
    let sig = cancellable(
        &mut stop,
        tokio::time::timeout(MAX_CALL, endpoint_sign(signed.clone())),
    )
    .await?
    .map_err(|_| PairingError::Unavailable)??;
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &endpoint[26..])
        .verify(&signed, &sig)
        .map_err(|_| PairingError::Invalid)?;
    let attestation = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&Envelope {
            payload_b64: URL_SAFE_NO_PAD.encode(proof),
            signature_b64: URL_SAFE_NO_PAD.encode(sig),
        })
        .map_err(|_| PairingError::Invalid)?,
    );
    let endpoint_binding = digest32(&signed);
    let origin = request.origin.as_str();
    let address = request.address;
    let first = cancellable(
        &mut stop,
        request_phone_enrollment(
            origin,
            address,
            budget(end)?,
            &identity.relay,
            &identity.relay_sec1,
            PhoneEnrollmentCommand::Claim {
                code: request.code.as_str().to_owned(),
                request_id,
                endpoint_binding,
                candidate_attestation: attestation.clone(),
            },
        ),
    )
    .await?;
    drop(request.code);
    let mut known = None;
    match first {
        Ok(PhoneEnrollmentResponse::Pending {
            request_id: echoed,
            tunnel,
            invite_id,
        }) => {
            if parse_id(&echoed)? != request_id {
                return Err(PairingError::Invalid);
            }
            known = Some(verified_ids(&tunnel, &invite_id)?);
        }
        Err(EnrollmentError::OutcomeUnknown) => {} // reconcile only via STATUS
        Ok(PhoneEnrollmentResponse::Closed) | Err(EnrollmentError::Rejected) => {
            return Err(PairingError::DeniedOrClosed);
        }
        _ => return Err(PairingError::Unavailable),
    }
    loop {
        cancellable(
            &mut stop,
            tokio::time::sleep(STATUS_INTERVAL.min(budget(end)?)),
        )
        .await?;
        let reply = cancellable(
            &mut stop,
            request_phone_enrollment(
                origin,
                address,
                budget(end)?,
                &identity.relay,
                &identity.relay_sec1,
                PhoneEnrollmentCommand::Status { request_id },
            ),
        )
        .await?;
        match reply {
            Ok(PhoneEnrollmentResponse::Pending {
                request_id: echoed,
                tunnel,
                invite_id,
            }) => {
                if parse_id(&echoed)? != request_id {
                    return Err(PairingError::Invalid);
                }
                let ids = verified_ids(&tunnel, &invite_id)?;
                if known.is_some_and(|old| old != ids) {
                    return Err(PairingError::Invalid);
                }
                known = Some(ids);
            }
            Ok(PhoneEnrollmentResponse::Approved {
                tunnel,
                invite_id,
                native_approval,
            }) => {
                let ids = verified_ids(&tunnel, &invite_id)?;
                if known.is_some_and(|old| old != ids) {
                    return Err(PairingError::Invalid);
                }
                if *stop.borrow() || stop.has_changed().is_err() {
                    return Err(PairingError::Cancelled);
                }
                return verify_approval(
                    EnrollmentAttempt {
                        host: request.host,
                        origin: request.origin,
                        code_sha256,
                        invite_id: ids.1,
                        request_id,
                        relay_tunnel: ids.0,
                        phone_id,
                        phone_relay_key: identity.relay_sec1,
                        phone_endpoint_spki: identity.endpoint_spki,
                        phone_grant_spki: identity.approval_spki,
                    },
                    &attestation,
                    &native_approval,
                    wall_ms()?,
                )
                .map_err(|InvalidEnrollment| PairingError::Invalid);
            }
            Ok(PhoneEnrollmentResponse::Closed) | Err(EnrollmentError::Rejected) => {
                return Err(PairingError::DeniedOrClosed);
            }
            Err(EnrollmentError::OutcomeUnknown) | Err(EnrollmentError::Connection(_)) => {}
            _ => return Err(PairingError::Invalid),
        }
    }
}
