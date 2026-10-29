//! Inner framing only; bytes are untrusted and this crate grants no authority.
pub mod appbind;
pub mod command;
pub mod grant;
pub mod input;
pub mod lifecycle;
pub mod projection;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Control,
    Projection,
    AppBind,
    Data,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length,
    Kind,
    Version,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub minor: u16,
    pub payload_len: usize,
}
impl Header {
    /// Validate the complete fixed header before a caller allocates payload storage.
    /// `expected_minor` comes from the enclosing channel state, never this header.
    pub fn decode(bytes: [u8; 8], expected_minor: u16) -> Result<Self, Error> {
        let length = u32::from_be_bytes(bytes[..4].try_into().unwrap());
        if !(4..=32772).contains(&length) {
            return Err(Error::Length);
        }
        let kind = match u16::from_be_bytes(bytes[4..6].try_into().unwrap()) {
            2 => Kind::Control,
            3 => Kind::Projection,
            4 => Kind::AppBind,
            5 => Kind::Data,
            _ => return Err(Error::Kind),
        };
        let minor = u16::from_be_bytes(bytes[6..].try_into().unwrap());
        let required_minor = if kind == Kind::AppBind {
            0
        } else {
            expected_minor
        };
        if expected_minor > 1 || minor != required_minor || (kind == Kind::Data && minor != 1) {
            return Err(Error::Version);
        }
        let payload_len = usize::try_from(length - 4).map_err(|_| Error::Length)?;
        if payload_len > kind.limit() {
            return Err(Error::Length);
        }
        Ok(Self {
            kind,
            minor,
            payload_len,
        })
    }
}
impl Kind {
    pub const fn limit(self) -> usize {
        match self {
            Self::Control | Self::AppBind => 16384,
            Self::Projection | Self::Data => 32768,
        }
    }
    const fn code(self) -> u16 {
        match self {
            Self::Control => 2,
            Self::Projection => 3,
            Self::AppBind => 4,
            Self::Data => 5,
        }
    }
}
/// Encode bounded opaque bytes. This does not validate JSON, AppBind or authority.
pub fn encode(kind: Kind, minor: u16, payload: &[u8]) -> Result<Vec<u8>, Error> {
    if minor > 1 || (kind == Kind::AppBind && minor != 0) || (kind == Kind::Data && minor != 1) {
        return Err(Error::Version);
    }
    if payload.len() > kind.limit() {
        return Err(Error::Length);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend_from_slice(&((payload.len() + 4) as u32).to_be_bytes());
    bytes.extend_from_slice(&kind.code().to_be_bytes());
    bytes.extend_from_slice(&minor.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}
