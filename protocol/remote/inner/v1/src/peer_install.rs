//! Opt-in pre-data native peer installation. Decoding grants no authority.
use crate::appbind::{self, CodecError};
use serde::{Deserialize, Serialize};
pub const CAPABILITY: &str = "peer-install-v1";
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "message", deny_unknown_fields, rename_all_fields = "camelCase")]
pub enum Message {
    Request {
        nonce: String,
        binding: String,
        grant_id: String,
        scope_digest: String,
    },
    Installed {
        nonce: String,
        binding: String,
        grant_id: String,
        scope_digest: String,
        fence: String,
    },
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PeerInstall(<bound-metadata>)")
    }
}
fn id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && s.bytes().any(|b| b != b'0')
}
fn valid(m: &Message) -> Result<(), CodecError> {
    let (n, b, g, s, f) = match m {
        Message::Request {
            nonce,
            binding,
            grant_id,
            scope_digest,
        } => (nonce, binding, grant_id, scope_digest, None),
        Message::Installed {
            nonce,
            binding,
            grant_id,
            scope_digest,
            fence,
        } => (nonce, binding, grant_id, scope_digest, Some(fence)),
    };
    if ![n, b, g, s].into_iter().all(|v| id(v)) {
        return Err(CodecError::Field);
    }
    if let Some(f) = f {
        if f.parse::<u64>().ok().filter(|v| *v > 0).is_none()
            || f.starts_with('0')
            || !f.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(CodecError::Field);
        }
    }
    Ok(())
}
pub fn encode(m: &Message) -> Result<Vec<u8>, CodecError> {
    valid(m)?;
    let b = appbind::canonical(m)?;
    if b.len() > 16384 {
        return Err(CodecError::Length);
    }
    Ok(b)
}
pub fn decode(bytes: &[u8]) -> Result<Message, CodecError> {
    if bytes.is_empty() || bytes.len() > 16384 {
        return Err(CodecError::Length);
    }
    appbind::depth(bytes)?;
    let m: Message = serde_json::from_slice(bytes).map_err(|_| CodecError::Json)?;
    if encode(&m)? != bytes {
        return Err(CodecError::Canonical);
    }
    Ok(m)
}
