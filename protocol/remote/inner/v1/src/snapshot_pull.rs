//! Explicit pull wire syntax only; callers must require authenticated capability.
use serde::{Deserialize, Serialize};
pub const MAX_PAYLOAD: usize = 32768 - 114;
#[derive(Debug)]
pub struct Error;
#[derive(Serialize, Deserialize)]
#[serde(tag = "message", deny_unknown_fields)]
pub enum Message {
    SnapshotBegin {
        profile: u16,
        request: String,
    },
    SnapshotNext {
        profile: u16,
        request: String,
        transfer: String,
        batch: String,
        through: String,
    },
}
fn number(s: &str, nonzero: bool) -> bool {
    s.parse::<u64>()
        .is_ok_and(|n| (!nonzero || n != 0) && n.to_string() == s)
}
impl Message {
    fn valid(&self) -> bool {
        let (profile, request, rest) = match self {
            Self::SnapshotBegin { profile, request } => (*profile, request, None),
            Self::SnapshotNext {
                profile,
                request,
                transfer,
                batch,
                through,
            } => (*profile, request, Some((transfer, batch, through))),
        };
        profile == 1
            && number(request, true)
            && rest.is_none_or(|(t, b, o)| {
                t.len() == 64
                    && t.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                    && number(b, false)
                    && b.parse::<u32>().is_ok()
                    && number(o, true)
                    && o.parse::<u32>().is_ok()
            })
    }
}
pub fn decode(bytes: &[u8]) -> Result<Message, Error> {
    if bytes.len() > MAX_PAYLOAD {
        return Err(Error);
    }
    let m: Message = serde_json::from_slice(bytes).map_err(|_| Error)?;
    if !m.valid() {
        return Err(Error);
    }
    Ok(m)
}
pub fn encode(m: &Message) -> Result<Vec<u8>, Error> {
    if !m.valid() {
        return Err(Error);
    }
    let bytes = serde_json::to_vec(m).map_err(|_| Error)?;
    if bytes.len() > MAX_PAYLOAD {
        return Err(Error);
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    #[test]
    fn strict_begin_rejects_duplicate_unknown_and_noncanonical() {
        for wire in [
            r#"{"message":"SnapshotBegin","profile":1,"request":"1","request":"2"}"#,
            r#"{"message":"SnapshotBegin","profile":1,"request":"01"}"#,
            r#"{"message":"SnapshotBegin","profile":1,"request":"0"}"#,
            r#"{"message":"SnapshotBegin","profile":1,"request":"1","extra":1}"#,
        ] {
            assert!(super::decode(wire.as_bytes()).is_err());
        }
        let wire = br#"{"message":"SnapshotBegin","profile":1,"request":"1"}"#;
        assert!(super::decode(wire).is_ok());
        assert!(crate::command::decode(wire).is_err());
    }
}

pub const BATCH_HEADER: usize = 57;
pub const MAX_PART: usize = MAX_PAYLOAD - BATCH_HEADER;
/// Borrowed wire payload; decoding never copies the projection bytes.
pub struct Batch<'a> {
    pub request: u64,
    pub transfer: [u8; 32],
    pub batch: u32,
    pub through: u32,
    pub last: bool,
    pub part: &'a [u8],
}
fn decode_batch_frame(bytes: &[u8]) -> Result<Batch<'_>, Error> {
    if bytes.len() < BATCH_HEADER || bytes.len() > MAX_PAYLOAD || &bytes[..4] != b"SPB1" {
        return Err(Error);
    }
    let request = u64::from_be_bytes(bytes[4..12].try_into().map_err(|_| Error)?);
    let transfer = bytes[12..44].try_into().map_err(|_| Error)?;
    let batch = u32::from_be_bytes(bytes[44..48].try_into().map_err(|_| Error)?);
    let through = u32::from_be_bytes(bytes[48..52].try_into().map_err(|_| Error)?);
    let last = match bytes[52] {
        0 => false,
        1 => true,
        _ => return Err(Error),
    };
    let len = u32::from_be_bytes(bytes[53..57].try_into().map_err(|_| Error)?) as usize;
    if request == 0 || through == 0 || len == 0 || len != bytes.len() - BATCH_HEADER {
        return Err(Error);
    }
    Ok(Batch {
        request,
        transfer,
        batch,
        through,
        last,
        part: &bytes[BATCH_HEADER..],
    })
}
pub fn encode_batch(b: &Batch<'_>) -> Result<Vec<u8>, Error> {
    if b.request == 0 || b.through == 0 || b.part.is_empty() || b.part.len() > MAX_PART {
        return Err(Error);
    }
    let mut out = Vec::with_capacity(BATCH_HEADER.checked_add(b.part.len()).ok_or(Error)?);
    out.extend_from_slice(b"SPB1");
    out.extend_from_slice(&b.request.to_be_bytes());
    out.extend_from_slice(&b.transfer);
    out.extend_from_slice(&b.batch.to_be_bytes());
    out.extend_from_slice(&b.through.to_be_bytes());
    out.push(u8::from(b.last));
    out.extend_from_slice(&(b.part.len() as u32).to_be_bytes());
    out.extend_from_slice(b.part);
    Ok(out)
}
#[cfg(test)]
mod batch_tests {
    use super::*;
    #[test]
    fn exact_length_and_borrowed_payload() {
        let part = vec![b'x'; MAX_PART];
        let b = Batch {
            request: 1,
            transfer: [7; 32],
            batch: 0,
            through: 128,
            last: false,
            part: &part,
        };
        let mut bytes = encode_batch(&b).unwrap();
        assert_eq!(bytes.len(), MAX_PAYLOAD);
        let parsed = decode_batch_frame(&bytes).unwrap();
        assert_eq!(parsed.part.as_ptr(), bytes[BATCH_HEADER..].as_ptr());
        assert_eq!(parsed.part, part);
        bytes.push(0);
        assert!(decode_batch_frame(&bytes).is_err());
        bytes.pop();
        bytes[52] = 2;
        assert!(decode_batch_frame(&bytes).is_err());
        bytes[52] = 0;
        bytes.pop();
        assert!(decode_batch_frame(&bytes).is_err());
        let too_big = vec![0; MAX_PART + 1];
        assert!(encode_batch(&Batch {
            part: &too_big,
            ..b
        })
        .is_err());
    }
}

/// Validate the original projection and the outer batch's coverage assertions.
fn projection_part(b: &Batch<'_>) -> Result<crate::projection::SnapshotPart, Error> {
    let message = crate::projection::decode(b.part, 1).map_err(|_| Error)?;
    let crate::projection::Content::SnapshotPart(part) = message.content else {
        return Err(Error);
    };
    let through = usize::from(part.start)
        .checked_add(part.cells.len())
        .ok_or(Error)?;
    let total = usize::from(part.state.rows)
        .checked_mul(usize::from(part.state.columns))
        .ok_or(Error)?;
    if part.cells.is_empty()
        || through > total
        || through != b.through as usize
        || b.last != (through == total)
    {
        return Err(Error);
    }
    Ok(part)
}

#[cfg(test)]
mod semantic_tests {
    use super::*;
    #[test]
    fn coverage_and_terminal_flag_match_original_projection() {
        let part = include_bytes!("../test-vectors/projection-snapshot.json");
        let m = crate::projection::decode(part, 1).unwrap();
        let crate::projection::Content::SnapshotPart(p) = m.content else {
            panic!()
        };
        let through = u32::from(p.start) + p.cells.len() as u32;
        let last = through == u32::from(p.state.rows) * u32::from(p.state.columns);
        let mut b = Batch {
            request: 1,
            transfer: [1; 32],
            batch: 0,
            through,
            last,
            part,
        };
        assert!(projection_part(&b).is_ok());
        b.last = !last;
        assert!(projection_part(&b).is_err());
        b.last = last;
        b.through += 1;
        assert!(projection_part(&b).is_err());
    }
    #[test]
    fn malformed_header_never_accepts_or_panics() {
        let wire = encode_batch(&Batch {
            request: u64::MAX,
            transfer: [1; 32],
            batch: u32::MAX,
            through: u32::MAX,
            last: true,
            part: b"x",
        })
        .unwrap();
        for len in 0..wire.len() {
            assert!(decode_batch_frame(&wire[..len]).is_err());
        }
        for index in [0, 1, 2, 3] {
            let mut b = wire.clone();
            b[index] ^= 1;
            assert!(decode_batch_frame(&b).is_err());
        }
        let mut b = wire.clone();
        b[53..57].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_batch_frame(&b).is_err());
        assert!(decode_batch_frame(&wire).is_ok());
    }
}

/// Checked batch shape; callers still validate current request/transfer/stamp/offset.
pub fn decode_validated_batch(
    bytes: &[u8],
) -> Result<(Batch<'_>, crate::projection::SnapshotPart), Error> {
    let batch = decode_batch_frame(bytes)?;
    let part = projection_part(&batch)?;
    Ok((batch, part))
}

#[cfg(test)]
mod next_tests {
    #[test]
    fn next_counters_have_wire_u32_range() {
        for (n, ok) in [("4294967295", true), ("4294967296", false), ("01", false)] {
            let wire = format!(
                r#"{{"message":"SnapshotNext","profile":1,"request":"1","transfer":"{}","batch":"{}","through":"{}"}}"#,
                "ab".repeat(32),
                n,
                n
            );
            assert_eq!(super::decode(wire.as_bytes()).is_ok(), ok);
        }
    }
}
