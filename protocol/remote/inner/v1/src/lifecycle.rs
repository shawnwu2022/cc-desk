//! Default-off lifecycle candidate. Parsing supplies no authority.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Create,
    Attach,
    Resize,
    Resume,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub generation: u64,
    pub fence: u64,
    pub mode: u64,
    pub dimension: u64,
    pub sync: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub kind: Kind,
    pub operation: [u8; 16],
    pub sequence: u64,
    pub state: State,
    pub size: Option<(u16, u16)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Code {
    Malformed,
    Unauthorized,
    Stale,
    Conflict,
    Pending,
    Unsupported,
    Backend,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Accepted,
    Unknown,
    Rejected(Code),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub request: Request,
    pub status: Status,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Wire {
    message: Kind,
    profile: u16,
    operation: String,
    sequence: String,
    generation: String,
    fence: String,
    mode_epoch: String,
    dimension_epoch: String,
    sync_epoch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    columns: Option<u16>,
}
fn number(s: &str, zero: bool) -> Result<u64, Code> {
    let n = s.parse::<u64>().map_err(|_| Code::Malformed)?;
    if (!zero && n == 0) || n.to_string() != s {
        return Err(Code::Malformed);
    }
    Ok(n)
}
fn valid(r: &Request) -> bool {
    r.operation != [0; 16]
        && r.sequence != 0
        && r.state.generation != 0
        && r.state.fence != 0
        && r.state.sync != 0
        && match (r.kind, r.size) {
            (Kind::Resize, Some((h, w))) => {
                h > 0 && w > 0 && h <= 128 && w <= 256 && u32::from(h) * u32::from(w) <= 16384
            }
            (Kind::Resize, None) => false,
            (_, None) => true,
            _ => false,
        }
}
impl TryFrom<Wire> for Request {
    type Error = Code;
    fn try_from(w: Wire) -> Result<Self, Code> {
        if w.profile != 1
            || w.operation.len() != 32
            || !w
                .operation
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Code::Malformed);
        }
        let mut operation = [0; 16];
        for (i, pair) in w.operation.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            operation[i] =
                u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| Code::Malformed)?, 16)
                    .map_err(|_| Code::Malformed)?;
        }
        let size = match (w.rows, w.columns) {
            (None, None) => None,
            (Some(h), Some(c)) => Some((h, c)),
            _ => return Err(Code::Malformed),
        };
        let r = Self {
            kind: w.message,
            operation,
            sequence: number(&w.sequence, false)?,
            state: State {
                generation: number(&w.generation, false)?,
                fence: number(&w.fence, false)?,
                mode: number(&w.mode_epoch, true)?,
                dimension: number(&w.dimension_epoch, true)?,
                sync: number(&w.sync_epoch, false)?,
            },
            size,
        };
        if !valid(&r) {
            return Err(Code::Malformed);
        }
        Ok(r)
    }
}
fn wire(r: &Request) -> Result<Wire, Code> {
    if !valid(r) {
        return Err(Code::Malformed);
    }
    Ok(Wire {
        message: r.kind,
        profile: 1,
        operation: r.operation.iter().map(|b| format!("{b:02x}")).collect(),
        sequence: r.sequence.to_string(),
        generation: r.state.generation.to_string(),
        fence: r.state.fence.to_string(),
        mode_epoch: r.state.mode.to_string(),
        dimension_epoch: r.state.dimension.to_string(),
        sync_epoch: r.state.sync.to_string(),
        rows: r.size.map(|s| s.0),
        columns: r.size.map(|s| s.1),
    })
}
fn bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, Code> {
    let b = serde_json::to_vec(value).map_err(|_| Code::Malformed)?;
    if b.len() > crate::input::MAX_PAYLOAD {
        return Err(Code::Malformed);
    }
    Ok(b)
}
pub fn decode(bytes: &[u8]) -> Result<Request, Code> {
    if bytes.len() > crate::input::MAX_PAYLOAD {
        return Err(Code::Malformed);
    }
    let w: Wire = shape(bytes)?;
    w.try_into()
}
pub fn encode(r: &Request) -> Result<Vec<u8>, Code> {
    bounded(&wire(r)?)
}
#[derive(Serialize, Deserialize)]
enum ReplyKind {
    LifecycleReply,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReply {
    message: ReplyKind,
    profile: u16,
    request: Wire,
    status: Status,
}
pub fn encode_reply(r: &Reply) -> Result<Vec<u8>, Code> {
    bounded(&WireReply {
        message: ReplyKind::LifecycleReply,
        profile: 1,
        request: wire(&r.request)?,
        status: r.status,
    })
}
pub fn decode_reply(b: &[u8]) -> Result<Reply, Code> {
    if b.len() > crate::input::MAX_PAYLOAD {
        return Err(Code::Malformed);
    }
    let w: WireReply = shape(b)?;
    if w.profile != 1 {
        return Err(Code::Malformed);
    }
    Ok(Reply {
        request: w.request.try_into()?,
        status: w.status,
    })
}
// Typed deserialization rejects duplicate/unknown fields first. Semantic shape
// comparison then rejects serde's alternate unit-enum maps and explicit null
// optionals. Whitespace/key order remain ordinary valid JSON.
fn shape<T: serde::de::DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, Code> {
    let typed: T = serde_json::from_slice(bytes).map_err(|_| Code::Malformed)?;
    let original: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Code::Malformed)?;
    if serde_json::to_value(&typed).map_err(|_| Code::Malformed)? != original {
        return Err(Code::Malformed);
    }
    Ok(typed)
}
