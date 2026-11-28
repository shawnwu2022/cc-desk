use super::{ApprovalLease, ApprovalMachine, DIALOG_ACTIVE};
#[path = "remote_native_observer_enrollment_control.rs"]
pub(crate) mod control;
#[path = "remote_native_observer_enrollment_data.rs"]
pub(crate) mod data;
#[path = "remote_native_observer_enrollment_ipc.rs"]
pub(crate) mod ipc;
#[path = "remote_native_observer_enrollment_owner.rs"]
pub(crate) mod owner;
#[path = "remote_native_observer_enrollment_setup.rs"]
pub(crate) mod setup;
#[path = "remote_native_observer_enrollment_shares.rs"]
pub(crate) mod shares;
#[path = "remote_native_observer_enrollment_store.rs"]
pub(crate) mod store;
use crate::{
    native_crypto::{
        owner::{KeyRequestHandle, SignatureEncoding, SigningFence},
        SigningPurpose,
    },
    run_supervisor::NativeRunAdmission,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{encode_scope, SessionScope},
};
use cc_desk_remote_security::{P256Spki, Role};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PHONE_DOMAIN: &[u8] = b"cc-desk.native-pairing.phone.v1\0";
const HOST_DOMAIN: &[u8] = b"cc-desk.native-pairing.host.v1\0";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const MAX_ENVELOPE: usize = 4096;
const MAX_WSS: usize = 512;
const MAX_INVITE: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Invalid,
    Expired,
    Busy,
    Authority,
    Signing,
}
type Result<T> = std::result::Result<T, Error>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}
fn decode<const N: usize>(value: &str) -> Result<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(Error::Invalid);
    }
    let mut out = [0; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| Error::Invalid)?;
    }
    if out == [0; N] {
        return Err(Error::Invalid);
    }
    Ok(out)
}
fn unix_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Expired)?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Expired)
}
fn relay_code(code: &str) -> bool {
    code.len() == 16 && code.bytes().all(|b| ALPHABET.contains(&b))
}
fn url_shape(origin: &CanonicalOrigin) -> bool {
    origin.as_str().len() <= MAX_WSS
}
fn transcript(domain: &[u8], payload: &[u8]) -> Vec<u8> {
    [domain, payload].concat()
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    payload_b64: String,
    signature_b64: String,
}
fn packed(payload: &[u8], sig: &[u8]) -> Result<String> {
    if sig.len() != 64 {
        return Err(Error::Signing);
    }
    let json = serde_json::to_vec(&Envelope {
        payload_b64: B64.encode(payload),
        signature_b64: B64.encode(sig),
    })
    .map_err(|_| Error::Invalid)?;
    let wire = B64.encode(json);
    if wire.len() > MAX_ENVELOPE {
        return Err(Error::Invalid);
    }
    Ok(wire)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PhoneClaim {
    version: u8,
    relay_origin: CanonicalOrigin,
    code_sha256: String,
    request_id: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    phone_grant_spki: String,
}

/// Bind the opaque claim to the independently authenticated Relay admission.
pub(crate) struct RelayCandidate {
    pub(crate) request_id: [u8; 16],
    pub(crate) phone_id: [u8; 16],
    pub(crate) phone_relay_public_key_sec1: [u8; 65],
    pub(crate) endpoint_binding: [u8; 32],
}
pub(crate) struct RelayInvite {
    pub(crate) id: [u8; 16],
    pub(crate) tunnel: [u8; 16],
    pub(crate) expires_unix_ms: u64,
}

/// This is a backend-only, verified candidate, not a deserializable UI DTO.
pub(crate) struct VerifiedCandidate {
    origin: CanonicalOrigin,
    invite: RelayInvite,
    relay: RelayCandidate,
    phone_endpoint: [u8; 91],
    phone_grant: [u8; 91],
    principal: [u8; 32],
    end: Instant,
}
impl VerifiedCandidate {
    pub(crate) fn verify(
        origin: CanonicalOrigin,
        code: &str,
        invite: RelayInvite,
        relay: RelayCandidate,
        wire: &str,
    ) -> Result<Self> {
        if !relay_code(code)
            || !url_shape(&origin)
            || wire.is_empty()
            || wire.len() > MAX_ENVELOPE
            || invite.id == [0; 16]
            || invite.tunnel == [0; 16]
            || relay.request_id == [0; 16]
            || relay.phone_id == [0; 16]
            || relay.endpoint_binding == [0; 32]
            || relay.phone_relay_public_key_sec1[0] != 4
        {
            return Err(Error::Invalid);
        }
        let budget = Duration::from_millis(
            invite
                .expires_unix_ms
                .checked_sub(unix_ms()?)
                .ok_or(Error::Expired)?,
        );
        if budget.is_zero() || budget > MAX_INVITE {
            return Err(Error::Expired);
        }
        let end = Instant::now().checked_add(budget).ok_or(Error::Expired)?;
        let decoded = B64.decode(wire).map_err(|_| Error::Invalid)?;
        let envelope: Envelope = serde_json::from_slice(&decoded).map_err(|_| Error::Invalid)?;
        let payload = B64
            .decode(envelope.payload_b64)
            .map_err(|_| Error::Invalid)?;
        let signature = B64
            .decode(envelope.signature_b64)
            .map_err(|_| Error::Invalid)?;
        let phone: PhoneClaim = serde_json::from_slice(&payload).map_err(|_| Error::Invalid)?;
        let phone_endpoint = decode::<91>(&phone.phone_endpoint_spki)?;
        let phone_grant = decode::<91>(&phone.phone_grant_spki)?;
        let endpoint = P256Spki::new(&phone_endpoint).map_err(|_| Error::Invalid)?;
        P256Spki::new(&phone_grant).map_err(|_| Error::Invalid)?;
        let message = transcript(PHONE_DOMAIN, &payload);
        if phone.version != 1
            || phone.relay_origin != origin
            || decode::<32>(&phone.code_sha256)?
                != <[u8; 32]>::from(Sha256::digest(code.as_bytes()))
            || decode::<16>(&phone.request_id)? != relay.request_id
            || decode::<16>(&phone.phone_id)? != relay.phone_id
            || decode::<65>(&phone.phone_relay_public_key_sec1)?
                != relay.phone_relay_public_key_sec1
            || <[u8; 32]>::from(Sha256::digest(&message)) != relay.endpoint_binding
            || phone_endpoint == phone_grant
            || phone_endpoint[26..] == relay.phone_relay_public_key_sec1
            || phone_grant[26..] == relay.phone_relay_public_key_sec1
        {
            return Err(Error::Invalid);
        }
        verify_p256(&phone_endpoint, &message, &signature)?;
        Ok(Self {
            origin,
            invite,
            relay,
            phone_endpoint,
            phone_grant,
            principal: endpoint.endpoint_id(Role::Client),
            end,
        })
    }
}
fn verify_p256(spki: &[u8; 91], message: &[u8], signature: &[u8]) -> Result<()> {
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &spki[26..])
        .verify(message, signature)
        .map_err(|_| Error::Invalid)
}

fn optional_scope_field<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QrInvite<'a> {
    version: u8,
    relay_origin: &'a CanonicalOrigin,
    code: &'a str,
    host_endpoint_id: String,
}

/// QR conveys the full endpoint pin; manual copy retains an independent 100-bit
/// commitment. Neither path treats the Relay's response as a key pin.
pub(crate) fn invitation_codes(
    origin: &CanonicalOrigin,
    code: &str,
    host_spki: &[u8; 91],
) -> Result<(String, String)> {
    if !relay_code(code) || !url_shape(origin) {
        return Err(Error::Invalid);
    }
    let host = P256Spki::new(host_spki).map_err(|_| Error::Invalid)?;
    let id = host.endpoint_id(Role::Host);
    let qr = serde_json::to_string(&QrInvite {
        version: 1,
        relay_origin: origin,
        code,
        host_endpoint_id: hex(&id),
    })
    .map_err(|_| Error::Invalid)?;
    let mut short = String::with_capacity(20);
    for offset in (0..100).step_by(5) {
        let mut digit = 0;
        for bit in offset..offset + 5 {
            digit = (digit << 1) | ((id[bit / 8] >> (7 - bit % 8)) & 1);
        }
        short.push(ALPHABET[usize::from(digit)] as char);
    }
    Ok((qr, format!("CCDESK1-{code}-{short}")))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u8,
    role: cc_desk_remote_record::grant::DataRole,
    relay_origin: CanonicalOrigin,
    invite_id: String,
    request_id: String,
    tunnel: String,
    endpoint_binding: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    phone_grant_spki: String,
    host_endpoint_spki: String,
    issuer_spki: String,
    principal: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_scope_field"
    )]
    session: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_scope_field"
    )]
    run: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_scope_field"
    )]
    generation: Option<String>,
    host_key_epoch: String,
    client_key_epoch: String,
    policy_epoch: String,
    issuer_epoch: String,
    expires_unix_ms: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct HostEpochs {
    pub(crate) host_key: u64,
    pub(crate) client_key: u64,
    pub(crate) policy: u64,
    pub(crate) issuer: u64,
}
struct Frozen {
    candidate: VerifiedCandidate,
    scope: Option<SessionScope>,
    public: [u8; 91],
    epochs: HostEpochs,
    share_ordinary: bool,
}
pub(crate) struct ObserverApproval {
    machine: Arc<ApprovalMachine<Frozen>>,
}
pub(crate) struct SignedObserverReceipt {
    pub(crate) wire: String,
    pub(crate) invite_id: [u8; 16],
    pub(crate) request_id: [u8; 16],
    pub(crate) tunnel: [u8; 16],
    pub(crate) endpoint_binding: [u8; 32],
    pub(crate) share_ordinary: bool,
    initial_scope: Option<SessionScope>,
    owner: Arc<ApprovalLease>,
    end: Instant,
}

fn exact_scope(admission: Option<&NativeRunAdmission>) -> Result<Option<SessionScope>> {
    admission
        .map(|current| current.scope().map_err(|_| Error::Authority))
        .transpose()
}
impl ObserverApproval {
    pub(crate) fn pending(&self) -> bool {
        self.machine.waiting()
    }
    pub(crate) fn approved(&self) -> bool {
        let state = self.machine.state.lock();
        !state.closed
            && state.description.is_some()
            && state.approved_end.is_some_and(|end| Instant::now() < end)
    }
    pub(crate) fn current(&self, issued: &SignedObserverReceipt) -> bool {
        Instant::now() < issued.end
            && Arc::ptr_eq(&issued.owner, &self.machine.lease)
            && !issued.owner.is_revoked()
    }
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        candidate: VerifiedCandidate,
        admission: Option<&NativeRunAdmission>,
        public: [u8; 91],
        epochs: HostEpochs,
        share_ordinary: bool,
    ) -> Result<Self> {
        let scope = exact_scope(admission)?;
        if share_ordinary && scope.is_some() {
            return Err(Error::Authority);
        }
        if let Some(scope) = &scope {
            encode_scope(scope).map_err(|_| Error::Authority)?;
        }
        P256Spki::new(&public).map_err(|_| Error::Authority)?;
        if [
            epochs.host_key,
            epochs.client_key,
            epochs.policy,
            epochs.issuer,
        ]
        .contains(&0)
            || public == candidate.phone_endpoint
            || public == candidate.phone_grant
            || public[26..] == candidate.relay.phone_relay_public_key_sec1
        {
            return Err(Error::Authority);
        }
        let end = candidate.end;
        if Instant::now() >= end {
            return Err(Error::Expired);
        }
        let scope_text = scope
            .as_ref()
            .map(|scope| {
                format!(
                    "Session: {}\nRun: {}\nGeneration: {}",
                    scope.session, scope.run, scope.generation
                )
            })
            .unwrap_or_else(|| {
                if share_ordinary {
                    "Share the unarchived session list from visible local projects while this desktop sharing window is active. This does not include archived sessions, changes, or terminal input.".into()
                } else {
                    "No session is shared yet.".into()
                }
            });
        let text = format!("Approve this Android device for viewing only?\n\nDevice: {}\nEndpoint: {}\n{}\nRelay: {}", hex(&candidate.relay.phone_id), hex(&candidate.relay.endpoint_binding), scope_text, candidate.origin.as_str());
        DIALOG_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Busy)?;
        Ok(Self {
            machine: ApprovalMachine::show(
                app,
                window,
                Frozen {
                    candidate,
                    scope,
                    public,
                    epochs,
                    share_ordinary,
                },
                text,
                "Approve view-only Android device",
                end,
            ),
        })
    }

    pub(crate) fn cancel(&self) {
        self.machine.cancel();
    }

    /// Consume the native Yes once, on the original owner. Neither the receipt
    /// itself nor membership on the Relay constitutes access to a running session.
    pub(crate) async fn sign(
        &self,
        admission: Option<&NativeRunAdmission>,
        key: &KeyRequestHandle,
    ) -> Result<Option<SignedObserverReceipt>> {
        let Some(permit) = self.machine.take() else {
            return Ok(None);
        };
        let (frozen, end) = permit
            .consume(|frozen, end| (frozen, end))
            .ok_or(Error::Authority)?;
        let candidate = &frozen.candidate;
        if exact_scope(admission)? != frozen.scope
            || key.binding() != (SigningPurpose::Endpoint, frozen.public.as_slice())
        {
            return Err(Error::Authority);
        }
        let auth = &candidate.relay;
        let payload = serde_json::to_vec(&Receipt {
            version: 1,
            role: cc_desk_remote_record::grant::DataRole::Observer,
            relay_origin: candidate.origin.clone(),
            invite_id: hex(&candidate.invite.id),
            request_id: hex(&auth.request_id),
            tunnel: hex(&candidate.invite.tunnel),
            endpoint_binding: hex(&auth.endpoint_binding),
            phone_id: hex(&auth.phone_id),
            phone_relay_public_key_sec1: hex(&auth.phone_relay_public_key_sec1),
            phone_endpoint_spki: hex(&candidate.phone_endpoint),
            phone_grant_spki: hex(&candidate.phone_grant),
            host_endpoint_spki: hex(&frozen.public),
            issuer_spki: hex(&candidate.phone_grant),
            principal: hex(&candidate.principal),
            session: frozen.scope.as_ref().map(|scope| scope.session.clone()),
            run: frozen.scope.as_ref().map(|scope| scope.run.clone()),
            generation: frozen.scope.as_ref().map(|scope| scope.generation.clone()),
            host_key_epoch: frozen.epochs.host_key.to_string(),
            client_key_epoch: frozen.epochs.client_key.to_string(),
            policy_epoch: frozen.epochs.policy.to_string(),
            issuer_epoch: frozen.epochs.issuer.to_string(),
            expires_unix_ms: candidate
                .invite
                .expires_unix_ms
                .min(unix_ms()?.saturating_add(240_000)),
        })
        .map_err(|_| Error::Invalid)?;
        let message = transcript(HOST_DOMAIN, &payload);
        let signing_end = end.min(Instant::now() + Duration::from_secs(5));
        let fence = SigningFence::new_revocable(end, self.machine.lease.native.clone());
        let attempt = fence
            .advance(std::slice::from_ref(key), signing_end)
            .map_err(|_| Error::Signing)?;
        let signature = key
            .begin_attempt(message.clone(), SignatureEncoding::Fixed, attempt)
            .map_err(|_| Error::Signing)?
            .await
            .map_err(|_| Error::Signing)?;
        if Instant::now() >= end
            || self.machine.lease.is_revoked()
            || candidate.invite.expires_unix_ms <= unix_ms()?
            || exact_scope(admission)? != frozen.scope
        {
            return Err(Error::Authority);
        }
        verify_p256(&frozen.public, &message, &signature)?;
        Ok(Some(SignedObserverReceipt {
            wire: packed(&payload, &signature)?,
            invite_id: candidate.invite.id,
            request_id: auth.request_id,
            tunnel: candidate.invite.tunnel,
            endpoint_binding: auth.endpoint_binding,
            share_ordinary: frozen.share_ordinary,
            initial_scope: frozen.scope,
            owner: self.machine.lease.clone(),
            end,
        }))
    }
}
