//! 未授权公开候选：严格验证并冻结，不执行 key、文件或网络 I/O。
use crate::{
    cli::{profiles::error, types::SafeError},
    remote_host_approval::{FrozenHostApproval, HostApprovalDescription},
    run_supervisor::NativeRunAdmission,
};
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::{
    BrowserProfile, HostSnapshot, NativeGrantPolicy, P256Spki, PeerPin, Role,
};
use cc_desk_remote_transport::wss::{RelayMode, RelayRole};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
const MAX_CANDIDATE: usize = 16384;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigurationRequest {
    pub run_id: String,
    pub generation: u32,
    pub candidate_json: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyReference {
    key_name: String,
    spki: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserCandidate {
    origin: String,
    rp_id: String,
    credential_id: String,
    spki: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    version: u8,
    address: String,
    origin: String,
    relay_mode: String,
    tunnel: String,
    relay_id: String,
    profile_id: String,
    profile_revision: String,
    endpoint: KeyReference,
    relay: KeyReference,
    phone_spki: String,
    issuer_spki: String,
    principal: String,
    activation: String,
    host_key_epoch: String,
    client_key_epoch: String,
    policy_epoch: String,
    issuer_epoch: String,
    incarnation: String,
    browser: BrowserCandidate,
}
fn invalid() -> SafeError {
    error("REMOTE_HOST_CONFIGURATION")
}
fn decimal(s: &str) -> Result<u64, SafeError> {
    if s.is_empty() || s.len() > 20 || s.starts_with('0') || !s.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(invalid());
    }
    s.parse::<u64>().map_err(|_| invalid())
}
fn hex(s: &str, max: usize) -> Result<Vec<u8>, SafeError> {
    if s.is_empty()
        || s.len() > max * 2
        || !s.len().is_multiple_of(2)
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            u8::from_str_radix(std::str::from_utf8(p).map_err(|_| invalid())?, 16)
                .map_err(|_| invalid())
        })
        .collect()
}
fn fixed<const N: usize>(s: &str) -> Result<[u8; N], SafeError> {
    hex(s, N)?.try_into().map_err(|_| invalid())
}
fn nonzero<const N: usize>(s: &str) -> Result<[u8; N], SafeError> {
    let b = fixed(s)?;
    if b == [0; N] {
        Err(invalid())
    } else {
        Ok(b)
    }
}
fn key_name(s: &str) -> Result<(), SafeError> {
    if s.is_empty()
        || s.len() > 128
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" _-.".contains(&b))
        || s.trim() != s
        || s == "."
        || s == ".."
    {
        return Err(invalid());
    }
    Ok(())
}
fn public(s: &str) -> Result<P256Spki, SafeError> {
    P256Spki::new(&fixed::<91>(s)?).map_err(|_| invalid())
}
pub(crate) fn freeze(
    text: &str,
    admission: &NativeRunAdmission,
) -> Result<FrozenHostApproval, SafeError> {
    // 期限在解析前锚定，确认和打开 key 不会延长本次时间域。
    let anchor = Instant::now();
    let cap = anchor
        .checked_add(Duration::from_secs(60))
        .ok_or_else(invalid)?;
    if text.len() > MAX_CANDIDATE {
        return Err(invalid());
    }
    let c = parse(text)?;
    if c.version != 1 {
        return Err(invalid());
    }
    key_name(&c.endpoint.key_name)?;
    key_name(&c.relay.key_name)?;
    let address: std::net::SocketAddr = c.address.parse().map_err(|_| invalid())?;
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        return Err(invalid());
    }
    if c.origin.len() > 512 {
        return Err(invalid());
    }
    let origin = CanonicalOrigin::trusted(&c.origin).map_err(|_| invalid())?;
    if origin.as_str() != c.origin {
        return Err(invalid());
    }
    let relay_mode = match c.relay_mode.as_str() {
        "legacyV1" => RelayMode::Legacy,
        "rendezvousV1" => RelayMode::RendezvousV1,
        _ => return Err(invalid()),
    };
    let endpoint_spki = fixed::<91>(&c.endpoint.spki)?;
    let relay_spki = fixed::<91>(&c.relay.spki)?;
    let endpoint = public(&c.endpoint.spki)?;
    public(&c.relay.spki)?;
    let relay_id = nonzero::<16>(&c.relay_id)?;
    let scope = admission.scope().map_err(|_| error("FORBIDDEN"))?;
    let browser = BrowserProfile::enrolled(
        &c.browser.origin,
        &c.browser.rp_id,
        hex(&c.browser.credential_id, 1024)?,
        public(&c.browser.spki)?,
        false,
        false,
    )
    .map_err(|_| invalid())?;
    let supplier = crate::native_crypto::host_profile::NativeHostProfile::new(
        crate::native_crypto::host::FrozenHostBinding {
            native_profile: crate::native_crypto::host::HostKeyProfileIdentity {
                id: nonzero(&c.profile_id)?,
                revision: decimal(&c.profile_revision)?,
            },
            endpoint_spki,
            relay_spki,
            origin: origin.clone(),
            relay_id,
            profile_cap: cap,
        },
        &c.endpoint.key_name,
        &c.relay.key_name,
    )
    .map_err(|_| invalid())?
    .freeze();
    let fields = HostApprovalDescription {
        endpoint_id: endpoint.endpoint_id(Role::Host),
        endpoint_role: Role::Host,
        relay_binding: (relay_id, RelayRole::Host),
        address,
        tunnel: nonzero(&c.tunnel)?,
        relay_mode,
        phone_pin: PeerPin::new(Role::Client, public(&c.phone_spki)?),
        binding: HostSnapshot {
            host_key_epoch: decimal(&c.host_key_epoch)?,
            client_key_epoch: decimal(&c.client_key_epoch)?,
            activation: nonzero(&c.activation)?,
            incarnation: decimal(&c.incarnation)?,
            policy_epoch: decimal(&c.policy_epoch)?,
            issuer_epoch: decimal(&c.issuer_epoch)?,
            origin,
        },
        browser,
        issuer: public(&c.issuer_spki)?,
        principal: nonzero(&c.principal)?,
        policy: NativeGrantPolicy {
            scope,
            role: DataRole::Controller,
            duration: Duration::from_secs(60),
        },
        clock: Arc::new(move || {
            u64::try_from(anchor.elapsed().as_millis())
                .map_err(|_| cc_desk_remote_security::GrantError::Revoked)
        }),
        expires_host_ms: 60000,
        setup_budget: Duration::from_secs(8),
        io_budget: Duration::from_secs(2),
    };
    FrozenHostApproval::supplied(fields, admission, cap, supplier).map_err(|_| invalid())
}

pub(crate) type ConfigurationAdmission = (
    Arc<crate::cli::document::DocumentBinding<crate::cli::launch_service::NativeRun>>,
    crate::cli::snapshot::CallerIdentity,
    NativeRunAdmission,
    String,
);
#[derive(serde::Serialize)]
#[serde(tag = "state")]
pub(crate) enum ConfigurationReply {
    Configured { receipt: String },
}

fn parse(text: &str) -> Result<Candidate, SafeError> {
    if text.len() > MAX_CANDIDATE {
        return Err(invalid());
    }
    serde_json::from_str(text).map_err(|_| invalid())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscardRequest {
    pub run_id: String,
    pub generation: u32,
    pub receipt: String,
}
pub(crate) fn receipt(value: &str) -> Result<u64, SafeError> {
    decimal(value)
}

pub(crate) type DiscardAdmission = (
    Arc<crate::cli::document::DocumentBinding<crate::cli::launch_service::NativeRun>>,
    crate::cli::snapshot::CallerIdentity,
    NativeRunAdmission,
    u64,
);
#[cfg(test)]
mod tests {
    use super::*;
    fn json() -> String {
        serde_json::json!({"version":1,"address":"127.0.0.1:443","origin":"wss://relay.invalid/","relayMode":"rendezvousV1","tunnel":"01".repeat(16),"relayId":"02".repeat(16),"profileId":"03".repeat(16),"profileRevision":"1","endpoint":{"keyName":"TEST_ONLY_ENDPOINT","spki":"00".repeat(91)},"relay":{"keyName":"TEST_ONLY_RELAY","spki":"00".repeat(91)},"phoneSpki":"00".repeat(91),"issuerSpki":"00".repeat(91),"principal":"01".repeat(32),"activation":"02".repeat(32),"hostKeyEpoch":"1","clientKeyEpoch":"1","policyEpoch":"1","issuerEpoch":"1","incarnation":"1","browser":{"origin":"https://desk.invalid","rpId":"desk.invalid","credentialId":"01","spki":"00".repeat(91)}}).to_string()
    }
    #[test]
    #[allow(non_snake_case)]
    fn HostCandidate_RejectsUnknownAndDuplicateFields_001() {
        let valid = json();
        assert!(parse(&valid).is_ok());
        for field in [
            "secret",
            "privateKey",
            "d",
            "approved",
            "scope",
            "clock",
            "path",
            "handle",
        ] {
            let malicious = valid.replacen('{', &format!("{{\"{field}\":true,"), 1);
            assert!(parse(&malicious).is_err());
        }
        assert!(parse(&valid.replacen("\"version\":1", "\"version\":1,\"version\":1", 1)).is_err());
        assert!(parse(&valid.replacen(
            "\"keyName\":\"TEST_ONLY_ENDPOINT\"",
            "\"keyName\":\"TEST_ONLY_ENDPOINT\",\"keyName\":\"OTHER\"",
            1
        ))
        .is_err());
        assert!(parse(&valid.replacen(
            "\"credentialId\":\"01\"",
            "\"credentialId\":\"01\",\"d\":\"secret\"",
            1
        ))
        .is_err());
    }
    #[test]
    #[allow(non_snake_case)]
    fn HostCandidate_BoundedCanonicalIntegersAndHex_002() {
        for s in ["0", "01", "-1", "1.0", "1e2", "18446744073709551616", " 1"] {
            assert!(decimal(s).is_err());
        }
        assert_eq!(decimal("18446744073709551615").unwrap(), u64::MAX);
        for s in ["", "0", "AA", "0g"] {
            assert!(hex(s, 32).is_err());
        }
        assert!(hex(&"01".repeat(33), 32).is_err());
        assert!(nonzero::<16>(&"00".repeat(16)).is_err());
        assert!(parse(&"x".repeat(MAX_CANDIDATE + 1)).is_err());
    }
    #[test]
    #[allow(non_snake_case)]
    fn HostCandidate_KeyReferencesAreNotPaths_003() {
        assert!(key_name("TEST_ONLY_EXISTING-key.1").is_ok());
        for s in [
            "",
            "../key",
            "C:\\key",
            " key",
            "key\n",
            "\u{202e}key",
            "..",
        ] {
            assert!(key_name(s).is_err());
        }
    }
    #[test]
    #[allow(non_snake_case)]
    fn HostCandidate_OuterEnvelopeRejectsExtraAndUnsafeNumbers_004() {
        for s in [
            r#"{"runId":"r","runId":"s","generation":1,"candidateJson":"{}"}"#,
            r#"{"runId":"r","generation":4294967296,"candidateJson":"{}"}"#,
            r#"{"runId":"r","generation":1.5,"candidateJson":"{}"}"#,
            r#"{"runId":"r","generation":1,"candidateJson":"{}","approved":true}"#,
        ] {
            assert!(serde_json::from_str::<ConfigurationRequest>(s).is_err());
        }
    }
}
