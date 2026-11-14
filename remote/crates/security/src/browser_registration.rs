//! 两阶段注册证据验证；不认证人员、不批准登记、不构造授权或持久记录。
use crate::{CancelToken, GrantError, P256Spki};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use coset::cbor::value::Value;
use ring::{digest, rand::SecureRandom, signature};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::Instant;

/// 必须由独立可信调用者选择；owner_binding 只绑定上下文，本身不是身份或批准。
pub struct BrowserRegistrationContext {
    pub origin: String,
    pub rp_id: String,
    pub user_handle: Vec<u8>,
    pub owner_binding: [u8; 32],
    pub tls_key: P256Spki,
    pub grant_key: P256Spki,
    pub relay_key: P256Spki,
    pub allow_backup_eligible: bool,
    pub allow_backed_up: bool,
}
pub struct BrowserRegistrationAttempt {
    context: BrowserRegistrationContext,
    challenge: [u8; 32],
    cancel: CancelToken,
    deadline: Instant,
}
pub struct BrowserRegistrationPossession {
    attempt: BrowserRegistrationAttempt,
    credential: Vec<u8>,
    key: P256Spki,
    backup_eligible: bool,
    counter: u32,
    challenge: [u8; 32],
}
/// 只读、短期的待批准证据；没有转换为 BrowserProfile、GrantGate 或 supplier 的接口。
pub struct UnapprovedBrowserRegistration {
    verified: BrowserRegistrationPossession,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateResponse {
    version: u16,
    credential_id: String,
    client_data_json: String,
    attestation_object: String,
    tls_signature: String,
    grant_signature: String,
    relay_signature: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssertionResponse {
    version: u16,
    credential_id: String,
    authenticator_data: String,
    client_data_json: String,
    signature: String,
    user_handle: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(default)]
    cross_origin: bool,
    #[serde(default, deserialize_with = "reject_top_origin")]
    top_origin: Option<String>,
}
fn reject_top_origin<'de, D: serde::Deserializer<'de>>(_: D) -> Result<Option<String>, D::Error> {
    Err(serde::de::Error::custom("top origin unsupported"))
}
fn decode(value: &str, max: usize) -> Result<Vec<u8>, GrantError> {
    if value.len() > max.div_ceil(3) * 4 {
        return Err(GrantError::Bounds);
    }
    let bytes = B64.decode(value).map_err(|_| GrantError::Encoding)?;
    if bytes.is_empty() || bytes.len() > max || B64.encode(&bytes) != value {
        return Err(GrantError::Encoding);
    }
    Ok(bytes)
}
fn hashed(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut h = digest::Context::new(&digest::SHA256);
    h.update(domain);
    for p in parts {
        h.update(&(p.len() as u32).to_be_bytes());
        h.update(p)
    }
    h.finish().as_ref().try_into().unwrap()
}
fn client(bytes: &[u8], origin: &str, challenge: [u8; 32], kind: &str) -> Result<(), GrantError> {
    let c: ClientData = serde_json::from_slice(bytes).map_err(|_| GrantError::Encoding)?;
    if c.kind != kind
        || c.origin != origin
        || c.challenge != B64.encode(challenge)
        || c.cross_origin
        || c.top_origin.is_some()
    {
        return Err(GrantError::Context);
    }
    Ok(())
}
fn parse_cbor(bytes: &[u8]) -> Result<Value, GrantError> {
    let mut remaining = bytes;
    let v = coset::cbor::de::from_reader_with_recursion_limit(&mut remaining, 8)
        .map_err(|_| GrantError::Encoding)?;
    if !remaining.is_empty() {
        return Err(GrantError::Encoding);
    }
    Ok(v)
}
fn map(v: Value, len: usize) -> Result<Vec<(Value, Value)>, GrantError> {
    match v {
        Value::Map(m) if m.len() == len => Ok(m),
        _ => Err(GrantError::Encoding),
    }
}
fn field<'a>(m: &'a [(Value, Value)], key: &Value) -> Result<&'a Value, GrantError> {
    let mut found = m.iter().filter(|(k, _)| k == key);
    let v = &found.next().ok_or(GrantError::Encoding)?.1;
    if found.next().is_some() {
        return Err(GrantError::Encoding);
    }
    Ok(v)
}
fn bytes(v: &Value) -> Result<&[u8], GrantError> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err(GrantError::Encoding),
    }
}
fn flags(
    flags: u8,
    attested: bool,
    context: &BrowserRegistrationContext,
) -> Result<bool, GrantError> {
    let be = flags & 8 != 0;
    let bs = flags & 16 != 0;
    if flags & !0x5d != 0
        || flags & 5 != 5
        || (flags & 64 != 0) != attested
        || (be && !context.allow_backup_eligible)
        || (bs && (!be || !context.allow_backed_up))
    {
        return Err(GrantError::Policy);
    }
    Ok(be)
}
impl BrowserRegistrationAttempt {
    pub fn new(
        context: BrowserRegistrationContext,
        cancel: CancelToken,
        budget: Duration,
    ) -> Result<Self, GrantError> {
        let url = url::Url::parse(&context.origin).map_err(|_| GrantError::Policy)?;
        let host = url.host_str().ok_or(GrantError::Policy)?;
        if context.origin.len() > 512
            || url.scheme() != "https"
            || url.origin().ascii_serialization() != context.origin
            || context.rp_id.is_empty()
            || context.rp_id.len() > 253
            || !(host == context.rp_id || host.ends_with(&format!(".{}", context.rp_id)))
            || context.user_handle.is_empty()
            || context.user_handle.len() > 64
            || context.owner_binding == [0; 32]
            || context.tls_key.0 == context.grant_key.0
            || context.tls_key.0 == context.relay_key.0
            || context.grant_key.0 == context.relay_key.0
            || (!context.allow_backup_eligible && context.allow_backed_up)
            || budget.is_zero()
            || budget > Duration::from_secs(120)
        {
            return Err(GrantError::Policy);
        }
        let mut nonce = [0; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| GrantError::Closed)?;
        let challenge = hashed(
            b"CC-DESK-REGISTRATION-v1\0",
            &[
                &nonce,
                &context.owner_binding,
                context.origin.as_bytes(),
                context.rp_id.as_bytes(),
                &context.user_handle,
                &context.tls_key.0,
                &context.grant_key.0,
                &context.relay_key.0,
                &[
                    context.allow_backup_eligible as u8,
                    context.allow_backed_up as u8,
                ],
            ],
        );
        let value = Self {
            context,
            challenge,
            cancel,
            deadline: Instant::now()
                .checked_add(budget)
                .ok_or(GrantError::Clock)?,
        };
        value.live()?;
        Ok(value)
    }
    fn live(&self) -> Result<(), GrantError> {
        if self.cancel.is_cancelled() {
            Err(GrantError::Revoked)
        } else if Instant::now() >= self.deadline {
            Err(GrantError::Expired)
        } else {
            Ok(())
        }
    }
    pub fn request(&self) -> Result<Vec<u8>, GrantError> {
        self.live()?;
        let c = &self.context;
        serde_json::to_vec(&serde_json::json!({"version":1,"origin":c.origin,"rpId":c.rp_id,"userHandle":B64.encode(&c.user_handle),"challenge":B64.encode(self.challenge),"tlsSpki":B64.encode(&c.tls_key.0),"grantSpki":B64.encode(&c.grant_key.0),"relaySpki":B64.encode(&c.relay_key.0)})).map_err(|_|GrantError::Encoding)
    }
    pub fn verify_create(
        self,
        response: &[u8],
    ) -> Result<BrowserRegistrationPossession, GrantError> {
        self.live()?;
        if response.len() > 16384 {
            return Err(GrantError::Bounds);
        }
        let v: CreateResponse =
            serde_json::from_slice(response).map_err(|_| GrantError::Encoding)?;
        if v.version != 1 {
            return Err(GrantError::Policy);
        }
        let credential = decode(&v.credential_id, 1024)?;
        let client_data = decode(&v.client_data_json, 2048)?;
        let attestation = decode(&v.attestation_object, 4096)?;
        client(
            &client_data,
            &self.context.origin,
            self.challenge,
            "webauthn.create",
        )?;
        let transcript = hashed(
            b"CC-DESK-REGISTRATION-CREATE-v1\0",
            &[&self.challenge, &credential, &client_data, &attestation],
        );
        for (purpose, key, sig) in [
            ("tls", &self.context.tls_key, &v.tls_signature),
            ("grant", &self.context.grant_key, &v.grant_signature),
            ("relay", &self.context.relay_key, &v.relay_signature),
        ] {
            let mut message =
                format!("CC-DESK-REGISTRATION-{purpose}-POSSESSION-v1\0").into_bytes();
            message.extend(transcript);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &key.0[26..])
                .verify(&message, &decode(sig, 64)?)
                .map_err(|_| GrantError::Signature)?;
        }
        let m = map(parse_cbor(&attestation)?, 3)?;
        if field(&m, &Value::Text("fmt".into()))? != &Value::Text("none".into())
            || field(&m, &Value::Text("attStmt".into()))? != &Value::Map(vec![])
        {
            return Err(GrantError::Policy);
        }
        let auth = bytes(field(&m, &Value::Text("authData".into()))?)?;
        if auth.len() < 55
            || auth[..32]
                != *digest::digest(&digest::SHA256, self.context.rp_id.as_bytes()).as_ref()
        {
            return Err(GrantError::Context);
        }
        let backup_eligible = flags(auth[32], true, &self.context)?;
        let counter = u32::from_be_bytes(auth[33..37].try_into().unwrap());
        let n = u16::from_be_bytes(auth[53..55].try_into().unwrap()) as usize;
        if n != credential.len() || 55 + n >= auth.len() || auth[55..55 + n] != credential {
            return Err(GrantError::Context);
        }
        let cose = map(parse_cbor(&auth[55 + n..])?, 5)?;
        for (k, v) in [(1, 2), (3, -7), (-1, 1)] {
            if field(&cose, &Value::Integer(k.into()))? != &Value::Integer(v.into()) {
                return Err(GrantError::Policy);
            }
        }
        let x = bytes(field(&cose, &Value::Integer((-2).into()))?)?;
        let y = bytes(field(&cose, &Value::Integer((-3).into()))?)?;
        if x.len() != 32 || y.len() != 32 {
            return Err(GrantError::Bounds);
        }
        let mut spki = self.context.tls_key.0[..27].to_vec();
        spki.extend(x);
        spki.extend(y);
        let key = P256Spki::new(&spki).map_err(|_| GrantError::Encoding)?;
        if [
            &self.context.tls_key,
            &self.context.grant_key,
            &self.context.relay_key,
        ]
        .iter()
        .any(|k| k.0 == key.0)
        {
            return Err(GrantError::Policy);
        }
        let challenge = hashed(
            b"CC-DESK-REGISTRATION-CONFIRM-v1\0",
            &[&self.challenge, response, &credential, &key.0],
        );
        self.live()?;
        Ok(BrowserRegistrationPossession {
            attempt: self,
            credential,
            key,
            backup_eligible,
            counter,
            challenge,
        })
    }
}
impl BrowserRegistrationPossession {
    pub fn request(&self) -> Result<Vec<u8>, GrantError> {
        self.attempt.live()?;
        serde_json::to_vec(&serde_json::json!({"version":1,"origin":self.attempt.context.origin,"rpId":self.attempt.context.rp_id,"credentialId":B64.encode(&self.credential),"challenge":B64.encode(self.challenge)})).map_err(|_|GrantError::Encoding)
    }
    pub fn verify_assertion(
        self,
        response: &[u8],
    ) -> Result<UnapprovedBrowserRegistration, GrantError> {
        self.attempt.live()?;
        if response.len() > 8192 {
            return Err(GrantError::Bounds);
        }
        let v: AssertionResponse =
            serde_json::from_slice(response).map_err(|_| GrantError::Encoding)?;
        if v.version != 1 || decode(&v.credential_id, 1024)? != self.credential {
            return Err(GrantError::Context);
        }
        if let Some(handle) = v.user_handle
            && decode(&handle, 64)? != self.attempt.context.user_handle
        {
            return Err(GrantError::Context);
        }
        let auth = decode(&v.authenticator_data, 37)?;
        let client_data = decode(&v.client_data_json, 2048)?;
        let sig = decode(&v.signature, 80)?;
        client(
            &client_data,
            &self.attempt.context.origin,
            self.challenge,
            "webauthn.get",
        )?;
        if auth.len() != 37
            || auth[..32]
                != *digest::digest(&digest::SHA256, self.attempt.context.rp_id.as_bytes()).as_ref()
            || flags(auth[32], false, &self.attempt.context)? != self.backup_eligible
        {
            return Err(GrantError::Context);
        }
        let counter = u32::from_be_bytes(auth[33..37].try_into().unwrap());
        if (counter != 0 || self.counter != 0) && counter <= self.counter {
            return Err(GrantError::Replay);
        }
        let mut message = auth;
        message.extend(digest::digest(&digest::SHA256, &client_data).as_ref());
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, &self.key.0[26..])
            .verify(&message, &sig)
            .map_err(|_| GrantError::Signature)?;
        self.attempt.live()?;
        Ok(UnapprovedBrowserRegistration { verified: self })
    }
}
impl UnapprovedBrowserRegistration {
    /// 只核对原类型化证据，不将候选或展示 JSON 升格为批准。
    pub fn matches_host_candidate(
        &self,
        owner_binding: [u8; 32],
        phone: &crate::PeerPin,
        issuer: &P256Spki,
        browser: &crate::BrowserProfile,
    ) -> Result<bool, GrantError> {
        let p = &self.verified;
        p.attempt.live()?;
        let c = &p.attempt.context;
        let b = browser.display_metadata();
        Ok(c.owner_binding == owner_binding
            && phone.endpoint_id() == c.tls_key.endpoint_id(crate::Role::Client)
            && issuer.0 == c.grant_key.0
            && b.origin == c.origin
            && b.rp_id == c.rp_id
            && b.credential_fingerprint.as_slice()
                == digest::digest(&digest::SHA256, &p.credential).as_ref()
            && b.key_fingerprint.as_slice() == digest::digest(&digest::SHA256, &p.key.0).as_ref()
            && b.backup_eligible == p.backup_eligible
            && b.allow_backed_up == c.allow_backed_up)
    }
    pub fn description(&self) -> Result<Vec<u8>, GrantError> {
        let p = &self.verified;
        p.attempt.live()?;
        let c = &p.attempt.context;
        serde_json::to_vec(&serde_json::json!({"version":1,"approval":"required","ownerBinding":B64.encode(c.owner_binding),"origin":c.origin,"rpId":c.rp_id,"requestedUserHandle":B64.encode(&c.user_handle),"tlsSpki":B64.encode(&c.tls_key.0),"grantSpki":B64.encode(&c.grant_key.0),"relaySpki":B64.encode(&c.relay_key.0),"credentialId":B64.encode(&p.credential),"credentialSpki":B64.encode(&p.key.0),"backupEligible":p.backup_eligible,"allowBackedUp":c.allow_backed_up})).map_err(|_|GrantError::Encoding)
    }
}
