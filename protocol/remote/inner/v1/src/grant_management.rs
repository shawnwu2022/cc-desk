//! Opt-in management messages. Binary COSE is carried unchanged in the single
//! expected Control record between Challenge/Request and Accepted.
use crate::{appbind, grant::CanonicalGrant};
use serde::{Deserialize, Serialize};
pub const CAPABILITY: &str = "grant-management-v1";
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "message", deny_unknown_fields, rename_all_fields = "camelCase")]
pub enum Message {
    Request {
        nonce: String,
        reuse: bool,
    },
    Challenge {
        nonce: String,
        binding: String,
        grant: CanonicalGrant,
    },
    Accepted {
        nonce: String,
        binding: String,
        grant_id: String,
        digest: String,
    },
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GrantManagement(<sensitive-metadata>)")
    }
}
fn id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && s.bytes().any(|b| b != b'0')
}
fn valid(m: &Message) -> Result<(), appbind::CodecError> {
    let (n, b) = match m {
        Message::Request { nonce, .. } => (nonce, None),
        Message::Challenge {
            nonce,
            binding,
            grant,
        } => {
            crate::grant::encode(grant)?;
            (nonce, Some(binding))
        }
        Message::Accepted {
            nonce,
            binding,
            grant_id,
            digest,
        } => {
            if !id(grant_id) || !id(digest) {
                return Err(appbind::CodecError::Field);
            }
            (nonce, Some(binding))
        }
    };
    if !id(n) || b.is_some_and(|s| !id(s)) {
        return Err(appbind::CodecError::Field);
    }
    Ok(())
}
pub fn encode(m: &Message) -> Result<Vec<u8>, appbind::CodecError> {
    valid(m)?;
    appbind::canonical(m)
}
pub fn decode(bytes: &[u8]) -> Result<Message, appbind::CodecError> {
    if bytes.is_empty() || bytes.len() > 16384 {
        return Err(appbind::CodecError::Length);
    }
    appbind::depth(bytes)?;
    let m: Message = serde_json::from_slice(bytes).map_err(|_| appbind::CodecError::Json)?;
    if encode(&m)? != bytes {
        return Err(appbind::CodecError::Canonical);
    }
    Ok(m)
}
