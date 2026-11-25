//! Verify the Android enrollment response against the original local
//! attempt and independently transported desktop identity commitment.
//! Cryptographic verification does not write storage or authorize startup.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cc_desk_remote_client::bootstrap::NativeProfile;
use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{DataRole, SessionScope, encode_scope},
};
use cc_desk_remote_security::{
    ClientGrantState, ClientGrantTrust, ClientTrust, LocalIdentity, NativeGrantSigner, P256Spki,
    PeerPin, Role,
};
use cc_desk_remote_transport::wss::{RelayAdmissionIdentity, RelayMode, RelayRole, RelaySigner};
use ring::{digest, signature};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc, time::Duration};

const HOST_DOMAIN: &[u8] = b"cc-desk.native-pairing.host.v1\0";
const PHONE_DOMAIN: &[u8] = b"cc-desk.native-pairing.phone.v1\0";
const MAX_ENVELOPE_BYTES: usize = 4096;
const MAX_APPROVAL_LIFETIME_MS: u64 = 5 * 60 * 1000;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidEnrollment;

/// This commitment comes from scanning the physical desktop screen or
/// entering its full manual pairing string, not from the Relay response.
pub enum HostCommitment {
    QrEndpointId([u8; 32]),
    Manual([u8; 20]),
}

impl HostCommitment {
    fn matches(&self, id: &[u8; 32]) -> bool {
        match self {
            Self::QrEndpointId(expected) => id == expected,
            Self::Manual(expected) => expected.iter().enumerate().all(|(i, c)| {
                let bit = i * 5;
                let pair = u16::from_be_bytes([id[bit / 8], id[bit / 8 + 1]]);
                let index = ((pair >> (11 - (bit % 8))) & 31) as usize;
                CROCKFORD[index] == *c
            }),
        }
    }
}

pub struct EnrollmentAttempt {
    pub host: HostCommitment,
    pub origin: CanonicalOrigin,
    pub code_sha256: [u8; 32],
    pub invite_id: [u8; 16],
    pub request_id: [u8; 16],
    pub phone_id: [u8; 16],
    pub phone_relay_key: [u8; 65],
    pub phone_endpoint_spki: [u8; 91],
    pub phone_grant_spki: [u8; 91],
}

/// Verified public enrollment material for transactional persistence by the
/// actual trusted platform owner. This is not a live session grant, trusted
/// Keystore signer, rollback-protection mechanism, or bootstrap constructor.
pub struct VerifiedEnrollment {
    host_endpoint: P256Spki,
    phone_grant: P256Spki,
    origin: CanonicalOrigin,
    scope: SessionScope,
    metadata: VerifiedMetadata,
}

pub struct VerifiedMetadata {
    pub invite_id: [u8; 16],
    pub request_id: [u8; 16],
    pub phone_id: [u8; 16],
    pub tunnel: [u8; 16],
    pub endpoint_binding: [u8; 32],
    pub principal: [u8; 32],
    pub host_key_epoch: u64,
    pub client_key_epoch: u64,
    pub policy_epoch: u64,
    pub issuer_epoch: u64,
    pub expires_unix_ms: u64,
}

impl VerifiedEnrollment {
    pub fn host_endpoint(&self) -> &P256Spki {
        &self.host_endpoint
    }
    pub fn origin(&self) -> &CanonicalOrigin {
        &self.origin
    }
    pub fn scope(&self) -> &SessionScope {
        &self.scope
    }
    pub fn metadata(&self) -> &VerifiedMetadata {
        &self.metadata
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    payload_b64: String,
    signature_b64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Phone {
    version: u8,
    relay_origin: CanonicalOrigin,
    code_sha256: String,
    request_id: String,
    phone_id: String,
    phone_relay_public_key_sec1: String,
    phone_endpoint_spki: String,
    phone_grant_spki: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Host {
    version: u8,
    role: String,
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
    session: String,
    run: String,
    generation: String,
    host_key_epoch: String,
    client_key_epoch: String,
    policy_epoch: String,
    issuer_epoch: String,
    expires_unix_ms: String,
}

fn nonzero<const N: usize>(value: [u8; N]) -> Result<[u8; N], InvalidEnrollment> {
    if value != [0; N] {
        Ok(value)
    } else {
        Err(InvalidEnrollment)
    }
}

fn lower_hex<const N: usize>(value: &str) -> Result<[u8; N], InvalidEnrollment> {
    let digit = |b: u8| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(InvalidEnrollment),
    };
    if value.len() != 2 * N {
        return Err(InvalidEnrollment);
    }
    let mut out = [0; N];
    for (i, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        out[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    nonzero(out)
}

fn decode64(raw: &str) -> Result<Vec<u8>, InvalidEnrollment> {
    if raw.is_empty() || raw.len() > MAX_ENVELOPE_BYTES {
        return Err(InvalidEnrollment);
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| InvalidEnrollment)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != raw {
        return Err(InvalidEnrollment);
    }
    Ok(bytes)
}

fn read_envelope(outer: &str) -> Result<(Vec<u8>, [u8; 64]), InvalidEnrollment> {
    let raw = decode64(outer)?;
    let envelope: Envelope = serde_json::from_slice(&raw).map_err(|_| InvalidEnrollment)?;
    let signature = decode64(&envelope.signature_b64)?
        .try_into()
        .map_err(|_| InvalidEnrollment)?;
    Ok((decode64(&envelope.payload_b64)?, signature))
}

fn signed_message(domain: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(domain.len() + payload.len());
    msg.extend_from_slice(domain);
    msg.extend_from_slice(payload);
    msg
}

fn verify_signature(
    public: &[u8; 91],
    msg: &[u8],
    sig: &[u8; 64],
) -> Result<(), InvalidEnrollment> {
    // Check the entire exact DER SPKI before referencing the SEC1 point.
    P256Spki::new(public).map_err(|_| InvalidEnrollment)?;
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &public[26..])
        .verify(msg, sig)
        .map_err(|_| InvalidEnrollment)
}

fn nonzero_decimal(raw: &str) -> Result<u64, InvalidEnrollment> {
    let number = raw.parse::<u64>().map_err(|_| InvalidEnrollment)?;
    if number == 0 || number.to_string() != raw {
        return Err(InvalidEnrollment);
    }
    Ok(number)
}

/// `phone_outer` is the original locally submitted signed payload;
/// `host_outer` is the returned opaque approval. The attempt contains the
/// current local Keystore public keys and pin and is never built from these blobs.
pub fn verify_approval(
    attempt: EnrollmentAttempt,
    phone_outer: &str,
    host_outer: &str,
    now_unix_ms: u64,
) -> Result<VerifiedEnrollment, InvalidEnrollment> {
    let grant_key = P256Spki::new(&attempt.phone_grant_spki).map_err(|_| InvalidEnrollment)?;
    let endpoint = P256Spki::new(&attempt.phone_endpoint_spki).map_err(|_| InvalidEnrollment)?;
    if attempt.phone_relay_key[0] != 4
        || attempt.phone_grant_spki == attempt.phone_endpoint_spki
        || attempt.phone_relay_key == attempt.phone_grant_spki[26..]
        || attempt.phone_relay_key == attempt.phone_endpoint_spki[26..]
    {
        return Err(InvalidEnrollment);
    }

    let (phone_bytes, phone_sig) = read_envelope(phone_outer)?;
    let phone: Phone = serde_json::from_slice(&phone_bytes).map_err(|_| InvalidEnrollment)?;
    if phone.version != 1
        || phone.relay_origin != attempt.origin
        || lower_hex::<32>(&phone.code_sha256)? != attempt.code_sha256
        || lower_hex::<16>(&phone.request_id)? != attempt.request_id
        || lower_hex::<16>(&phone.phone_id)? != attempt.phone_id
        || lower_hex::<65>(&phone.phone_relay_public_key_sec1)? != attempt.phone_relay_key
        || lower_hex::<91>(&phone.phone_endpoint_spki)? != attempt.phone_endpoint_spki
        || lower_hex::<91>(&phone.phone_grant_spki)? != attempt.phone_grant_spki
    {
        return Err(InvalidEnrollment);
    }
    let proof = signed_message(PHONE_DOMAIN, &phone_bytes);
    verify_signature(&attempt.phone_endpoint_spki, &proof, &phone_sig)?;
    let endpoint_binding: [u8; 32] = digest::digest(&digest::SHA256, &proof)
        .as_ref()
        .try_into()
        .map_err(|_| InvalidEnrollment)?;

    let (host_bytes, host_sig) = read_envelope(host_outer)?;
    let response: Host = serde_json::from_slice(&host_bytes).map_err(|_| InvalidEnrollment)?;
    let host_spki = lower_hex::<91>(&response.host_endpoint_spki)?;
    let host_endpoint = P256Spki::new(&host_spki).map_err(|_| InvalidEnrollment)?;
    if !attempt.host.matches(&host_endpoint.endpoint_id(Role::Host))
        || host_spki == attempt.phone_endpoint_spki
        || grant_key.endpoint_id(Role::Host) == host_endpoint.endpoint_id(Role::Host)
        || attempt.phone_relay_key == host_spki[26..]
    {
        return Err(InvalidEnrollment);
    }
    verify_signature(
        &host_spki,
        &signed_message(HOST_DOMAIN, &host_bytes),
        &host_sig,
    )?;
    let principal = lower_hex::<32>(&response.principal)?;
    let expires_unix_ms = nonzero_decimal(&response.expires_unix_ms)?;
    if response.version != 1
        || response.role != "Observer"
        || response.relay_origin != attempt.origin
        || lower_hex::<16>(&response.invite_id)? != attempt.invite_id
        || lower_hex::<16>(&response.request_id)? != attempt.request_id
        || lower_hex::<16>(&response.phone_id)? != attempt.phone_id
        || lower_hex::<65>(&response.phone_relay_public_key_sec1)? != attempt.phone_relay_key
        || lower_hex::<91>(&response.phone_endpoint_spki)? != attempt.phone_endpoint_spki
        || lower_hex::<91>(&response.phone_grant_spki)? != attempt.phone_grant_spki
        || lower_hex::<91>(&response.issuer_spki)? != attempt.phone_grant_spki
        || lower_hex::<32>(&response.endpoint_binding)? != endpoint_binding
        || principal != endpoint.endpoint_id(Role::Client)
        || expires_unix_ms <= now_unix_ms
        || expires_unix_ms - now_unix_ms > MAX_APPROVAL_LIFETIME_MS
    {
        return Err(InvalidEnrollment);
    }
    let scope = SessionScope {
        session: response.session,
        run: response.run,
        generation: response.generation,
    };
    encode_scope(&scope).map_err(|_| InvalidEnrollment)?;
    Ok(VerifiedEnrollment {
        host_endpoint,
        phone_grant: grant_key,
        origin: response.relay_origin,
        scope,
        metadata: VerifiedMetadata {
            invite_id: attempt.invite_id,
            request_id: attempt.request_id,
            phone_id: attempt.phone_id,
            tunnel: lower_hex(&response.tunnel)?,
            endpoint_binding,
            principal,
            host_key_epoch: nonzero_decimal(&response.host_key_epoch)?,
            client_key_epoch: nonzero_decimal(&response.client_key_epoch)?,
            policy_epoch: nonzero_decimal(&response.policy_epoch)?,
            issuer_epoch: nonzero_decimal(&response.issuer_epoch)?,
            expires_unix_ms,
        },
    })
}

/// Keystore-backed signers opened by the native enrollment owner. The endpoint
/// is checked immediately; the existing relay and grant challenge exchanges
/// independently verify the two other keys before the channel can be installed.
pub struct NativeKeystoreBindings {
    pub endpoint: LocalIdentity,
    pub relay: Arc<dyn RelaySigner>,
    pub approval: Box<dyn NativeGrantSigner>,
}

impl VerifiedEnrollment {
    /// Consume a still-current, already verified receipt; no input or wildcard
    /// mode is possible. Caller must first complete its durable transaction.
    /// Address is resolved by the native owner from this receipt's same WSS URL.
    pub fn into_profile(
        self,
        address: SocketAddr,
        keys: NativeKeystoreBindings,
        state: ClientGrantState,
        now_unix_ms: u64,
    ) -> Result<NativeProfile, InvalidEnrollment> {
        if now_unix_ms >= self.metadata.expires_unix_ms
            || keys.endpoint.role() != Role::Client
            || keys.endpoint.endpoint_id() != self.metadata.principal
            || address.port() == 0
        {
            return Err(InvalidEnrollment);
        }
        Ok(NativeProfile {
            address,
            relay: RelayAdmissionIdentity::new(
                self.metadata.phone_id,
                RelayRole::Phone,
                keys.relay,
            ),
            relay_mode: RelayMode::RendezvousV1,
            tunnel: self.metadata.tunnel,
            local: keys.endpoint,
            host_pin: PeerPin::new(Role::Host, self.host_endpoint),
            binding: ClientTrust {
                host_key_epoch: self.metadata.host_key_epoch,
                client_key_epoch: self.metadata.client_key_epoch,
                origin: self.origin,
            },
            grant: ClientGrantTrust {
                issuer: self.phone_grant,
                principal: self.metadata.principal,
                scope: self.scope,
                role: DataRole::Observer,
            },
            signer: keys.approval,
            state,
            setup_budget: Duration::from_secs(10),
            io_budget: Duration::from_secs(1),
        })
    }
}
