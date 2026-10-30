//! Candidate control DTOs only. Authentication, delivery, durable facts and
//! settlement are checked by the native Host, never established by this codec.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    HostWritten,
    Rejected,
    Partial,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Known,
    Missing,
    TooOld,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    Advanced,
    AlreadySettled,
    Rejected,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "message", deny_unknown_fields, rename_all_fields = "camelCase")]
pub enum Message {
    WindowGranted {
        profile: u16,
        request: String,
        window: String,
        not_after_host_ms: String,
    },
    ProjectionAcknowledged {
        profile: u16,
        stamp: crate::projection::Stamp,
    },
    InputReceipt {
        profile: u16,
        operation: String,
        sequence: String,
        outcome: Outcome,
        #[serde(deserialize_with = "required_nullable")]
        proof: Option<String>,
    },
    InputNotAccepted {
        profile: u16,
        operation: String,
        sequence: String,
    },
    ReceiptQuery {
        profile: u16,
        request: String,
        operation: String,
        sequence: String,
    },
    ReceiptStatus {
        profile: u16,
        request: String,
        operation: String,
        sequence: String,
        status: Status,
        #[serde(deserialize_with = "required_nullable")]
        outcome: Option<Outcome>,
        #[serde(deserialize_with = "required_nullable")]
        proof: Option<String>,
    },
    ReceiptAck {
        profile: u16,
        request: String,
        operation: String,
        sequence: String,
        outcome: Outcome,
        proof: String,
    },
    ReceiptSettled {
        profile: u16,
        request: String,
        operation: String,
        sequence: String,
        outcome: Outcome,
        disposition: Disposition,
    },
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ControlIo(<untrusted-sensitive-metadata>)")
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error;
fn required_nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}
fn counter(s: &str) -> bool {
    s.parse::<u64>().is_ok_and(|n| n != 0 && n.to_string() == s)
}
fn hex(s: &str, bytes: usize) -> bool {
    s.len() == bytes * 2
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && s.bytes().any(|b| b != b'0')
}
fn final_outcome(o: Outcome) -> bool {
    matches!(o, Outcome::HostWritten | Outcome::Rejected)
}
fn receipt(o: Outcome, proof: &Option<String>) -> bool {
    match proof {
        Some(p) => final_outcome(o) && hex(p, 32),
        None => !final_outcome(o),
    }
}
fn identity(operation: &str, sequence: &str) -> bool {
    hex(operation, 16) && counter(sequence)
}
impl Message {
    fn valid(&self) -> bool {
        match self {
            Self::WindowGranted {
                profile,
                request,
                window,
                not_after_host_ms,
            } => *profile == 1 && counter(request) && hex(window, 32) && counter(not_after_host_ms),
            Self::ProjectionAcknowledged { profile, stamp } => {
                *profile == 1
                    && crate::projection::encode_ack(
                        &crate::projection::Ack {
                            profile: 1,
                            stamp: stamp.clone(),
                        },
                        1,
                    )
                    .is_ok()
            }
            Self::InputReceipt {
                profile,
                operation,
                sequence,
                outcome,
                proof,
            } => *profile == 1 && identity(operation, sequence) && receipt(*outcome, proof),
            Self::InputNotAccepted {
                profile,
                operation,
                sequence,
            } => *profile == 1 && identity(operation, sequence),
            Self::ReceiptQuery {
                profile,
                request,
                operation,
                sequence,
            } => *profile == 1 && counter(request) && identity(operation, sequence),
            Self::ReceiptStatus {
                profile,
                request,
                operation,
                sequence,
                status,
                outcome,
                proof,
            } => {
                *profile == 1
                    && counter(request)
                    && identity(operation, sequence)
                    && match (status, outcome) {
                        (Status::Known, Some(o)) => receipt(*o, proof),
                        (Status::Missing | Status::TooOld, None) => proof.is_none(),
                        _ => false,
                    }
            }
            Self::ReceiptAck {
                profile,
                request,
                operation,
                sequence,
                outcome,
                proof,
            } => {
                *profile == 1
                    && counter(request)
                    && identity(operation, sequence)
                    && final_outcome(*outcome)
                    && hex(proof, 32)
            }
            Self::ReceiptSettled {
                profile,
                request,
                operation,
                sequence,
                outcome,
                ..
            } => {
                *profile == 1
                    && counter(request)
                    && identity(operation, sequence)
                    && final_outcome(*outcome)
            }
        }
    }
}
fn bounded_json(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > crate::input::MAX_PAYLOAD {
        return false;
    }
    let (mut depth, mut quoted, mut escaped) = (0u8, false, false);
    for &b in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return false;
                    }
                }
                b'}' | b']' => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    depth == 0 && !quoted
}
pub fn encode(message: &Message) -> Result<Vec<u8>, Error> {
    if !message.valid() {
        return Err(Error);
    }
    let bytes = serde_json::to_vec(message).map_err(|_| Error)?;
    if !bounded_json(&bytes) {
        return Err(Error);
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Message, Error> {
    if !bounded_json(bytes) {
        return Err(Error);
    }
    let value: Message = serde_json::from_slice(bytes).map_err(|_| Error)?;
    if !value.valid() {
        return Err(Error);
    }
    // Serde also accepts externally-tagged map forms for unit enums. The phone
    // schema requires scalar strings and explicit nullable fields, not aliases.
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Error)?;
    if raw != serde_json::to_value(&value).map_err(|_| Error)? {
        return Err(Error);
    }
    Ok(value)
}
