//! Explicit in-memory enrollment policy. This API performs no registration or key generation.
use crate::{GrantError, P256Spki};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use cc_desk_remote_record::grant::CanonicalGrant;
use ring::{digest, signature};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct BrowserProfile {
    origin: String,
    rp_id: String,
    credential: Vec<u8>,
    key: P256Spki,
    backup_eligible: bool,
    allow_backed_up: bool,
}
impl std::fmt::Debug for BrowserProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrowserProfile(<explicit-enrollment-policy>)")
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Approval {
    pub version: u16,
    pub cose: String,
    pub credential_id: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    pub signature: String,
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
    Err(serde::de::Error::custom("cross-origin profile unsupported"))
}
fn decode(value: &str, maximum: usize) -> Result<Vec<u8>, GrantError> {
    if value.len() > maximum.div_ceil(3) * 4 {
        return Err(GrantError::Bounds);
    }
    let bytes = B64.decode(value).map_err(|_| GrantError::Encoding)?;
    if bytes.len() > maximum || B64.encode(&bytes) != value {
        return Err(GrantError::Encoding);
    }
    Ok(bytes)
}
impl BrowserProfile {
    /// Values must come from separately approved enrollment, never a wire request.
    pub fn enrolled(
        origin: &str,
        rp_id: &str,
        credential: Vec<u8>,
        key: P256Spki,
        backup_eligible: bool,
        allow_backed_up: bool,
    ) -> Result<Self, GrantError> {
        let url = url::Url::parse(origin).map_err(|_| GrantError::Policy)?;
        let host = url.host_str().ok_or(GrantError::Policy)?;
        if url.scheme() != "https"
            || url.origin().ascii_serialization() != origin
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || rp_id.is_empty()
            || rp_id.len() > 253
            || !(host == rp_id || host.ends_with(&format!(".{rp_id}")))
            || credential.is_empty()
            || credential.len() > 1024
            || (!backup_eligible && allow_backed_up)
        {
            return Err(GrantError::Policy);
        }
        Ok(Self {
            origin: origin.into(),
            rp_id: rp_id.into(),
            credential,
            key,
            backup_eligible,
            allow_backed_up,
        })
    }
    pub(crate) fn challenge(
        &self,
        payload: &CanonicalGrant,
        kid: [u8; 32],
        attempt: ([u8; 32], [u8; 32]),
    ) -> Result<[u8; 32], GrantError> {
        let tbs = crate::grant::signing_candidate(payload, kid)?.tbs_data(crate::grant::AAD);
        let mut hash = digest::Context::new(&digest::SHA256);
        hash.update(b"CC-DESK-BROWSER-APPROVAL-v1\0");
        for part in [
            self.origin.as_bytes(),
            self.rp_id.as_bytes(),
            &self.credential,
            &self.key.0,
            &[self.backup_eligible as u8, self.allow_backed_up as u8],
            &attempt.0,
            &attempt.1,
            &tbs,
        ] {
            hash.update(&(part.len() as u32).to_be_bytes());
            hash.update(part);
        }
        Ok(hash.finish().as_ref().try_into().unwrap())
    }
    pub(crate) fn verify(
        &self,
        bytes: &[u8],
        payload: &CanonicalGrant,
        issuer: &P256Spki,
        kid: [u8; 32],
        attempt: ([u8; 32], [u8; 32]),
    ) -> Result<(), GrantError> {
        if bytes.len() > 16384 {
            return Err(GrantError::Bounds);
        }
        let proof: Approval = serde_json::from_slice(bytes).map_err(|_| GrantError::Encoding)?;
        if proof.version != 1
            || serde_json::to_vec(&proof).map_err(|_| GrantError::Encoding)? != bytes
        {
            return Err(GrantError::Encoding);
        }
        let cose = decode(&proof.cose, 8192)?;
        crate::grant::signature(&cose, payload, issuer, kid)?;
        if decode(&proof.credential_id, 1024)? != self.credential {
            return Err(GrantError::Policy);
        }
        let auth = decode(&proof.authenticator_data, 37)?;
        let client = decode(&proof.client_data_json, 2048)?;
        let sig = decode(&proof.signature, 80)?;
        let c: ClientData = serde_json::from_slice(&client).map_err(|_| GrantError::Encoding)?;
        if c.kind != "webauthn.get"
            || c.origin != self.origin
            || c.cross_origin
            || c.top_origin.is_some()
            || c.challenge != B64.encode(self.challenge(payload, kid, attempt)?)
            || auth.len() != 37
            || auth[..32] != *digest::digest(&digest::SHA256, self.rp_id.as_bytes()).as_ref()
        {
            return Err(GrantError::Context);
        }
        let flags = auth[32];
        // Minimal profile supports no authenticator extensions, attested data or reserved flags.
        if flags & !0x1d != 0
            || flags & 5 != 5
            || (flags & 8 != 0) != self.backup_eligible
            || (flags & 16 != 0 && (!self.backup_eligible || !self.allow_backed_up))
        {
            return Err(GrantError::Policy);
        }
        let mut message = auth;
        message.extend_from_slice(digest::digest(&digest::SHA256, &client).as_ref());
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, &self.key.0[26..])
            .verify(&message, &sig)
            .map_err(|_| GrantError::Signature)
    }
}
