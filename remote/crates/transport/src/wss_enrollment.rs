//! 复用 WebPKI 与 Relay 准入证明；此通道不会授予任何原生会话权限。
use super::{ConnectError, RelayAdmissionIdentity, RelayRole, admission, connect_protocol};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

pub const ENROLLMENT_SUBPROTOCOL: &str = "cc-desk-relay-enrollment-v1";
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const EVIDENCE_CAP: usize = 4096;
const COMMAND_CAP: usize = 6144;
pub(super) const RESPONSE_CAP: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnrollmentError {
    Connection(ConnectError),
    Invalid,
    Rejected,
    // 写请求后的失联不能安全重试；调用方用 Host list 或 Phone status 对账。
    OutcomeUnknown,
}

pub enum HostEnrollmentCommand {
    Create {
        ttl_seconds: u16,
    },
    List,
    Decide {
        invite_id: [u8; 16],
        request_id: [u8; 16],
        approve: bool,
        native_approval: Option<String>,
    },
    Cancel {
        invite_id: [u8; 16],
    },
    Revoke {
        tunnel: [u8; 16],
    },
}
pub enum PhoneEnrollmentCommand {
    Claim {
        code: String,
        request_id: [u8; 16],
        endpoint_binding: [u8; 32],
        candidate_attestation: String,
    },
    Status {
        request_id: [u8; 16],
    },
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostEnrollmentResponse {
    Created {
        invite_id: String,
        tunnel: String,
        code: String,
        expires_unix_ms: u64,
    },
    List {
        invites: Vec<EnrollmentInvite>,
        pairs: Vec<EnrollmentPair>,
    },
    Approved {
        tunnel: String,
    },
    Closed {
        tunnel: Option<String>,
    },
    Revoked {
        tunnel: String,
    },
    Rejected,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentInvite {
    pub invite_id: String,
    pub tunnel: String,
    pub expires_unix_ms: u64,
    pub candidate: Option<EnrollmentCandidate>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentCandidate {
    pub request_id: String,
    pub phone_id: String,
    pub phone_public_key_sec1: String,
    pub endpoint_binding: String,
    pub candidate_attestation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentPair {
    pub request_id: String,
    pub tunnel: String,
    pub phone_id: String,
    pub phone_public_key_sec1: String,
    pub endpoint_binding: String,
}
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhoneEnrollmentResponse {
    Pending {
        request_id: String,
        tunnel: String,
        invite_id: String,
    },
    Approved {
        tunnel: String,
        invite_id: String,
        native_approval: String,
    },
    Closed,
    Rejected,
}
fn hex(input: &[u8]) -> String {
    let mut out = String::with_capacity(2 * input.len());
    const DIGITS: &[u8] = b"0123456789abcdef";
    for b in input {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 15)] as char);
    }
    out
}
fn fixed_hex(text: &str, n: usize) -> bool {
    text.len() == 2 * n
        && text
            .bytes()
            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
}
fn valid_code(code: &str) -> bool {
    code.len() == 16 && code.bytes().all(|x| CODE_ALPHABET.contains(&x))
}
fn valid_evidence(value: &str) -> bool {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    if value.is_empty()
        || value.len() > EVIDENCE_CAP
        || !value.bytes().all(|x| ALPHABET.contains(&x))
    {
        return false;
    }
    let Some(last) = ALPHABET
        .iter()
        .position(|x| Some(x) == value.as_bytes().last())
    else {
        return false;
    };
    matches!(value.len() % 4, 0)
        || (value.len() % 4 == 2 && last & 15 == 0)
        || (value.len() % 4 == 3 && last & 3 == 0)
}
fn phone_values(id: &str, key: &str, binding: &str) -> bool {
    fixed_hex(id, 16) && fixed_hex(key, 65) && key.starts_with("04") && fixed_hex(binding, 32)
}
impl HostEnrollmentResponse {
    fn valid_for(&self, cmd: &HostEnrollmentCommand) -> bool {
        match (self, cmd) {
            (
                Self::Created {
                    invite_id,
                    tunnel,
                    code,
                    expires_unix_ms,
                },
                HostEnrollmentCommand::Create { .. },
            ) => {
                fixed_hex(invite_id, 16)
                    && fixed_hex(tunnel, 16)
                    && valid_code(code)
                    && *expires_unix_ms != 0
            }
            (Self::List { invites, pairs }, HostEnrollmentCommand::List) => {
                invites.len() <= 1
                    && pairs.len() <= 8
                    && invites.iter().all(|i| {
                        fixed_hex(&i.invite_id, 16)
                            && fixed_hex(&i.tunnel, 16)
                            && i.expires_unix_ms != 0
                            && i.candidate.as_ref().is_none_or(|c| {
                                fixed_hex(&c.request_id, 16)
                                    && phone_values(
                                        &c.phone_id,
                                        &c.phone_public_key_sec1,
                                        &c.endpoint_binding,
                                    )
                                    && valid_evidence(&c.candidate_attestation)
                            })
                    })
                    && pairs.iter().all(|p| {
                        fixed_hex(&p.request_id, 16)
                            && fixed_hex(&p.tunnel, 16)
                            && phone_values(
                                &p.phone_id,
                                &p.phone_public_key_sec1,
                                &p.endpoint_binding,
                            )
                    })
            }
            (Self::Approved { tunnel }, HostEnrollmentCommand::Decide { approve: true, .. }) => {
                fixed_hex(tunnel, 16)
            }
            (
                Self::Closed {
                    tunnel: Some(tunnel),
                },
                HostEnrollmentCommand::Decide { approve: false, .. },
            ) => fixed_hex(tunnel, 16),
            (Self::Closed { tunnel: None }, HostEnrollmentCommand::Cancel { .. }) => true,
            (Self::Revoked { tunnel }, HostEnrollmentCommand::Revoke { tunnel: wanted }) => {
                *tunnel == hex(wanted)
            }
            _ => false,
        }
    }
}
impl PhoneEnrollmentResponse {
    fn valid_for(&self, cmd: &PhoneEnrollmentCommand) -> bool {
        match (self, cmd) {
            (
                Self::Pending {
                    request_id,
                    tunnel,
                    invite_id,
                },
                PhoneEnrollmentCommand::Claim {
                    request_id: expected,
                    ..
                }
                | PhoneEnrollmentCommand::Status {
                    request_id: expected,
                },
            ) => *request_id == hex(expected) && fixed_hex(tunnel, 16) && fixed_hex(invite_id, 16),
            (
                Self::Approved {
                    tunnel,
                    invite_id,
                    native_approval,
                },
                PhoneEnrollmentCommand::Status { .. },
            ) => {
                fixed_hex(tunnel, 16) && fixed_hex(invite_id, 16) && valid_evidence(native_approval)
            }
            (Self::Closed, PhoneEnrollmentCommand::Status { .. }) => true,
            _ => false,
        }
    }
}
pub async fn request_host_enrollment(
    origin: &str,
    address: SocketAddr,
    budget: Duration,
    identity: &RelayAdmissionIdentity,
    command: HostEnrollmentCommand,
) -> Result<HostEnrollmentResponse, EnrollmentError> {
    if identity.binding().1 != RelayRole::Host {
        return Err(EnrollmentError::Invalid);
    }
    let wire = match &command {
        HostEnrollmentCommand::Create { ttl_seconds } if matches!(ttl_seconds, 60 | 300 | 600) => {
            json!({"op":"create","ttl_seconds":ttl_seconds})
        }
        HostEnrollmentCommand::List => json!({"op":"list"}),
        HostEnrollmentCommand::Decide {
            invite_id,
            request_id,
            approve,
            native_approval,
        } => {
            let mut wire = json!({"op":"decide","invite_id":hex(invite_id),"request_id":hex(request_id),"approve":approve});
            match (approve, native_approval) {
                (true, Some(receipt)) if valid_evidence(receipt) => {
                    wire["native_approval"] = json!(receipt);
                }
                (false, None) => (),
                _ => return Err(EnrollmentError::Invalid),
            }
            wire
        }
        HostEnrollmentCommand::Cancel { invite_id } => {
            json!({"op":"cancel","invite_id":hex(invite_id)})
        }
        HostEnrollmentCommand::Revoke { tunnel } => json!({"op":"revoke","tunnel":hex(tunnel)}),
        _ => return Err(EnrollmentError::Invalid),
    };
    let reply = request(origin, address, budget, identity, None, wire).await?;
    match reply {
        HostEnrollmentResponse::Rejected => Err(EnrollmentError::Rejected),
        response if response.valid_for(&command) => Ok(response),
        _ => Err(EnrollmentError::OutcomeUnknown),
    }
}
pub async fn request_phone_enrollment(
    origin: &str,
    address: SocketAddr,
    budget: Duration,
    identity: &RelayAdmissionIdentity,
    public_key_sec1: &[u8; 65],
    command: PhoneEnrollmentCommand,
) -> Result<PhoneEnrollmentResponse, EnrollmentError> {
    if identity.binding().1 != RelayRole::Phone || public_key_sec1[0] != 4 {
        return Err(EnrollmentError::Invalid);
    }
    let wire = match &command {
        PhoneEnrollmentCommand::Claim {
            code,
            request_id,
            endpoint_binding,
            candidate_attestation,
        } if valid_code(code) && valid_evidence(candidate_attestation) => {
            json!({"op":"claim","code":code,"request_id":hex(request_id),"endpoint_binding":hex(endpoint_binding),"candidate_attestation":candidate_attestation})
        }
        PhoneEnrollmentCommand::Status { request_id } => {
            json!({"op":"status","request_id":hex(request_id)})
        }
        _ => return Err(EnrollmentError::Invalid),
    };
    let reply = request(
        origin,
        address,
        budget,
        identity,
        Some(public_key_sec1),
        wire,
    )
    .await?;
    match reply {
        PhoneEnrollmentResponse::Rejected => Err(EnrollmentError::Rejected),
        response if response.valid_for(&command) => Ok(response),
        _ => Err(EnrollmentError::OutcomeUnknown),
    }
}

async fn request<T: for<'de> Deserialize<'de>>(
    origin: &str,
    address: SocketAddr,
    budget: Duration,
    identity: &RelayAdmissionIdentity,
    point: Option<&[u8; 65]>,
    wire: Value,
) -> Result<T, EnrollmentError> {
    let wire = serde_json::to_string(&wire).map_err(|_| EnrollmentError::Invalid)?;
    if wire.len() > COMMAND_CAP {
        return Err(EnrollmentError::Invalid);
    }
    admission::endpoint(origin, address, budget).map_err(EnrollmentError::Connection)?;
    let deadline = Instant::now()
        .checked_add(budget)
        .ok_or(EnrollmentError::Invalid)?;
    let mut attempted = false;
    let result = crate::deadline::bounded(deadline, async {
        let ws = connect_protocol(origin, address, budget, Some(ENROLLMENT_SUBPROTOCOL))
            .await
            .map_err(EnrollmentError::Connection)?;
        let mut ws = admission::admit_with_point(ws, origin, identity, deadline, point)
            .await
            .map_err(EnrollmentError::Connection)?;
        attempted = true;
        ws.send(Message::Text(wire.into()))
            .await
            .map_err(|_| EnrollmentError::OutcomeUnknown)?;
        let Some(Ok(Message::Text(response))) = ws.next().await else {
            return Err(EnrollmentError::OutcomeUnknown);
        };
        if response.len() > RESPONSE_CAP {
            return Err(EnrollmentError::OutcomeUnknown);
        }
        serde_json::from_str(&response).map_err(|_| EnrollmentError::OutcomeUnknown)
    })
    .await;
    result.unwrap_or_else(|_| {
        Err(if attempted {
            EnrollmentError::OutcomeUnknown
        } else {
            EnrollmentError::Connection(ConnectError::Deadline)
        })
    })
}
