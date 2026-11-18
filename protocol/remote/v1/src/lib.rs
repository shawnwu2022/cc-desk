//! Shared outer framing. A routing handle carries no endpoint authorization.
pub const MAX_MESSAGE: usize = 65536;
pub const MAX_SUBFRAMES: usize = 32;
pub type TunnelId = [u8; 16];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    Length,
    Version,
    Kind,
    Count,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Subframe<'a> {
    pub tunnel: TunnelId,
    pub payload: &'a [u8],
}
impl core::fmt::Debug for Subframe<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Subframe")
            .field("payload_len", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Length => "outer length",
            Self::Version => "outer version",
            Self::Kind => "outer kind",
            Self::Count => "outer count",
        })
    }
}
impl std::error::Error for WireError {}

/// Decode one complete binary WSS message. Payloads borrow the bounded input.
/// The caller must enforce the message limit in its WSS parser before allocation.
pub fn decode_outer(bytes: &[u8]) -> Result<Vec<Subframe<'_>>, WireError> {
    if !(4..=MAX_MESSAGE).contains(&bytes.len()) {
        return Err(WireError::Length);
    }
    if bytes[0] != 1 {
        return Err(WireError::Version);
    }
    if bytes[1] != 1 {
        return Err(WireError::Kind);
    }
    let count = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    if count > MAX_SUBFRAMES {
        return Err(WireError::Count);
    }
    // Metadata allocation is bounded by32; declared payload length never allocates.
    let mut frames = Vec::with_capacity(count);
    let mut cursor = 4usize;
    for _ in 0..count {
        let header_end = cursor.checked_add(20).ok_or(WireError::Length)?;
        let header = bytes.get(cursor..header_end).ok_or(WireError::Length)?;
        let mut tunnel = [0; 16];
        tunnel.copy_from_slice(&header[..16]);
        let len = usize::try_from(u32::from_be_bytes([
            header[16], header[17], header[18], header[19],
        ]))
        .map_err(|_| WireError::Length)?;
        let end = header_end.checked_add(len).ok_or(WireError::Length)?;
        let payload = bytes.get(header_end..end).ok_or(WireError::Length)?;
        frames.push(Subframe { tunnel, payload });
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(WireError::Length);
    }
    Ok(frames)
}

/// Preflight the entire envelope before allocating or writing any result bytes.
pub fn encode_outer(frames: &[Subframe<'_>]) -> Result<Vec<u8>, WireError> {
    if frames.len() > MAX_SUBFRAMES {
        return Err(WireError::Count);
    }
    let mut len = 4usize;
    for frame in frames {
        len = len
            .checked_add(20)
            .and_then(|n| n.checked_add(frame.payload.len()))
            .filter(|n| *n <= MAX_MESSAGE)
            .ok_or(WireError::Length)?;
    }
    let mut bytes = Vec::with_capacity(len);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&(frames.len() as u16).to_be_bytes());
    for frame in frames {
        bytes.extend_from_slice(&frame.tunnel);
        bytes.extend_from_slice(&(frame.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(frame.payload);
    }
    Ok(bytes)
}
