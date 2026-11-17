//! Untrusted grant/data DTOs only; encoding is not signature or policy authority.
use crate::{
    appbind::{self, CodecError, Context},
    Kind,
};
use serde::{Deserialize, Serialize};
pub const MAX_GRANT_JSON: usize = 8192;
pub const DATA_PREFIX: usize = 114;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataRole {
    Observer,
    Controller,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionScope {
    pub session: String,
    pub run: String,
    pub generation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalGrant {
    pub version: u16,
    pub context: Context,
    pub principal_id: String,
    pub scope: SessionScope,
    pub role: DataRole,
    pub challenge: String,
    pub grant_id: String,
    pub created_host_ms: String,
    pub expires_host_ms: String,
}
fn hex(value: &str, n: usize) -> bool {
    value.len() == n * 2
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && value.bytes().any(|b| b != b'0')
}
pub fn encode_scope(scope: &SessionScope) -> Result<Vec<u8>, CodecError> {
    if !hex(&scope.session, 16)
        || scope.run.is_empty()
        || scope.run.len() > 128
        || scope.run.chars().any(char::is_control)
        || appbind::decimal(&scope.generation)? == 0
    {
        return Err(CodecError::Field);
    }
    appbind::canonical(scope)
}
pub fn encode(grant: &CanonicalGrant) -> Result<Vec<u8>, CodecError> {
    appbind::context(&grant.context)?;
    encode_scope(&grant.scope)?;
    if grant.version != 1
        || !hex(&grant.principal_id, 32)
        || !hex(&grant.challenge, 32)
        || !hex(&grant.grant_id, 32)
    {
        return Err(CodecError::Field);
    }
    let start = appbind::decimal(&grant.created_host_ms)?;
    let end = appbind::decimal(&grant.expires_host_ms)?;
    if end <= start || end > start.checked_add(900000).ok_or(CodecError::Field)? {
        return Err(CodecError::Field);
    }
    let bytes = appbind::canonical(grant)?;
    if bytes.len() > MAX_GRANT_JSON {
        return Err(CodecError::Length);
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<CanonicalGrant, CodecError> {
    if bytes.is_empty() || bytes.len() > MAX_GRANT_JSON {
        return Err(CodecError::Length);
    }
    appbind::depth(bytes)?;
    let value = serde_json::from_slice(bytes).map_err(|_| CodecError::Json)?;
    if encode(&value)? != bytes {
        return Err(CodecError::Canonical);
    }
    Ok(value)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataHeader {
    pub binding: [u8; 32],
    pub grant: [u8; 32],
    pub scope: [u8; 32],
    pub fence: u64,
    pub sequence: u64,
    pub kind: Kind,
}
fn valid_data(h: &DataHeader, len: usize) -> Result<(), CodecError> {
    if !matches!(h.kind, Kind::Control | Kind::Projection)
        || h.binding == [0; 32]
        || h.grant == [0; 32]
        || h.scope == [0; 32]
        || h.fence == 0
        || h.sequence == 0
    {
        return Err(CodecError::Field);
    }
    if len
        .checked_add(DATA_PREFIX)
        .is_none_or(|n| n > h.kind.limit())
    {
        return Err(CodecError::Length);
    }
    Ok(())
}
pub fn encode_data(h: &DataHeader, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    valid_data(h, body.len())?;
    let mut b = Vec::with_capacity(DATA_PREFIX + body.len());
    for id in [&h.binding, &h.grant, &h.scope] {
        b.extend_from_slice(id);
    }
    b.extend_from_slice(&h.fence.to_be_bytes());
    b.extend_from_slice(&h.sequence.to_be_bytes());
    b.extend_from_slice(&(if h.kind == Kind::Control { 2u16 } else { 3u16 }).to_be_bytes());
    b.extend_from_slice(body);
    Ok(b)
}
pub fn decode_data(bytes: &[u8]) -> Result<(DataHeader, &[u8]), CodecError> {
    if bytes.len() < DATA_PREFIX || bytes.len() > 32768 {
        return Err(CodecError::Length);
    }
    let kind = match u16::from_be_bytes(bytes[112..114].try_into().unwrap()) {
        2 => Kind::Control,
        3 => Kind::Projection,
        _ => return Err(CodecError::Field),
    };
    let h = DataHeader {
        binding: bytes[..32].try_into().unwrap(),
        grant: bytes[32..64].try_into().unwrap(),
        scope: bytes[64..96].try_into().unwrap(),
        fence: u64::from_be_bytes(bytes[96..104].try_into().unwrap()),
        sequence: u64::from_be_bytes(bytes[104..112].try_into().unwrap()),
        kind,
    };
    valid_data(&h, bytes.len() - DATA_PREFIX)?;
    Ok((h, &bytes[DATA_PREFIX..]))
}
