//! Bounded InputChunk syntax, not authority. The enclosing authenticated data
//! record supplies endpoint/session/run/generation/fence; the native window
//! supplies exact mode/sync/lease/grant state. Production protocol freeze OPEN.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
pub const MAX_BYTES: usize = 8192;
/// Complete control payload includes the 114-byte authenticated data envelope.
pub const MAX_PAYLOAD: usize = 16384 - 114;
#[derive(Clone, PartialEq, Eq)]
pub struct Input {
    pub operation: [u8; 16],
    pub sequence: u64,
    pub window: [u8; 32],
    pub not_after_host_ms: u64,
    pub bytes: Vec<u8>,
}
impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Input(<untrusted-terminal-input>)")
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Wire {
    message: Message,
    profile: u16,
    operation: String,
    sequence: String,
    window: String,
    not_after_host_ms: String,
    bytes: String,
}
#[derive(Serialize, Deserialize)]
enum Message {
    InputChunk,
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}
fn unhex<const N: usize>(s: &str) -> Result<[u8; N], Error> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error);
    }
    let mut out = [0; N];
    for (i, pair) in s.as_bytes().chunks_exact(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| Error)?, 16)
            .map_err(|_| Error)?
    }
    Ok(out)
}
fn counter(s: &str) -> Result<u64, Error> {
    let n = s.parse::<u64>().map_err(|_| Error)?;
    if n == 0 || n.to_string() != s {
        return Err(Error);
    }
    Ok(n)
}
fn valid(i: &Input) -> bool {
    i.operation != [0; 16]
        && i.sequence != 0
        && i.window != [0; 32]
        && i.not_after_host_ms != 0
        && !i.bytes.is_empty()
        && i.bytes.len() <= MAX_BYTES
}
pub fn encode(i: &Input) -> Result<Vec<u8>, Error> {
    if !valid(i) {
        return Err(Error);
    }
    let w = Wire {
        message: Message::InputChunk,
        profile: 1,
        operation: hex(&i.operation),
        sequence: i.sequence.to_string(),
        window: hex(&i.window),
        not_after_host_ms: i.not_after_host_ms.to_string(),
        bytes: URL_SAFE_NO_PAD.encode(&i.bytes),
    };
    let b = serde_json::to_vec(&w).map_err(|_| Error)?;
    if b.len() > MAX_PAYLOAD {
        return Err(Error);
    }
    Ok(b)
}
pub fn decode(b: &[u8]) -> Result<Input, Error> {
    if b.len() > MAX_PAYLOAD {
        return Err(Error);
    }
    let w: Wire = serde_json::from_slice(b).map_err(|_| Error)?;
    if w.profile != 1 || w.bytes.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err(Error);
    }
    let raw = URL_SAFE_NO_PAD.decode(&w.bytes).map_err(|_| Error)?;
    if URL_SAFE_NO_PAD.encode(&raw) != w.bytes {
        return Err(Error);
    }
    let i = Input {
        operation: unhex(&w.operation)?,
        sequence: counter(&w.sequence)?,
        window: unhex(&w.window)?,
        not_after_host_ms: counter(&w.not_after_host_ms)?,
        bytes: raw,
    };
    if !valid(&i) {
        return Err(Error);
    }
    Ok(i)
}
