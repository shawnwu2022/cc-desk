//! 未授权登记输入只含公开材料和已有 key 引用；解析不打开 key 或网络。
use super::*;
use crate::native_crypto::{
    host::{FrozenHostBinding, FrozenHostSupplier, HostKeyProfileIdentity},
    host_profile::NativeHostProfile,
};
use crate::remote_host_approval::{BrowserInstallationTarget, HostApprovalDescription};
use crate::remote_host_configuration::{decimal, fixed, key_name, nonzero, public};
use cc_desk_remote_record::{appbind::CanonicalOrigin, grant::DataRole};
use cc_desk_remote_security::{
    browser_registration::BrowserRegistrationContext, BrowserProfile, HostSnapshot,
    NativeGrantPolicy, P256Spki, PeerPin, Role,
};
use cc_desk_remote_transport::wss::{RelayMode, RelayRole};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyReference {
    key_name: String,
    spki: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserRequest {
    origin: String,
    rp_id: String,
    user_handle: String,
    tls_spki: String,
    grant_spki: String,
    relay_spki: String,
    relay_id: String,
    database_name: String,
    expected_revision: Option<String>,
    wasm_url: String,
    glue_url: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RequestedAccess {
    #[default]
    Controller,
    ObserverOnce,
}
/// Produced only by explicit ObserverOnce configuration, never by Controller
/// configuration, remembered pairing, or the test connector.
pub(crate) struct FormalObserverEndpoint {
    address: std::net::SocketAddr,
    origin: CanonicalOrigin,
}
impl FormalObserverEndpoint {
    pub(crate) fn matches(&self, address: std::net::SocketAddr, origin: &CanonicalOrigin) -> bool {
        self.address == address && &self.origin == origin
    }
}
enum FrozenAccess {
    Controller { remember: bool },
    ObserverOnce,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    #[serde(default)]
    access_mode: RequestedAccess,
    #[serde(default)]
    remember: bool,
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
    principal: String,
    activation: String,
    host_key_epoch: String,
    client_key_epoch: String,
    policy_epoch: String,
    issuer_epoch: String,
    incarnation: String,
    browser: BrowserRequest,
}
#[cfg(test)]
type FixtureKeys = ([[u8; 32]; 2], Arc<[std::sync::atomic::AtomicUsize; 2]>);
pub(crate) struct FrozenRegistrationStart {
    candidate: Candidate,
    access: FrozenAccess,
    pub(crate) end: Instant,
    anchor: Instant,
    pub(crate) address: std::net::SocketAddr,
    pub(crate) origin: CanonicalOrigin,
    pub(crate) mode: RelayMode,
    pub(crate) tunnel: [u8; 16],
    binding: FrozenHostBinding,
    pub(crate) tls_key: P256Spki,
    #[cfg(test)]
    fixture_keys: Option<FixtureKeys>,
}
fn invalid() -> SafeError {
    error("REMOTE_HOST_CONFIGURATION")
}
impl FrozenRegistrationStart {
    pub(crate) fn remember(&self) -> bool {
        matches!(self.access, FrozenAccess::Controller { remember: true })
    }
    pub(crate) fn product_role_enabled(&self) -> bool {
        match self.access {
            FrozenAccess::Controller { .. } => true,
            FrozenAccess::ObserverOnce => {
                self.formal_observer_endpoint().is_some()
                    || crate::remote_host_network::local_observer_address(self.address)
            }
        }
    }
    pub(crate) fn formal_observer_endpoint(&self) -> Option<FormalObserverEndpoint> {
        (cfg!(feature = "remote-observer")
            && matches!(self.access, FrozenAccess::ObserverOnce)
            && !self.address.ip().is_loopback())
            .then(|| FormalObserverEndpoint {
                address: self.address,
                origin: self.origin.clone(),
            })
    }
    pub(super) fn data_role(&self) -> DataRole {
        match self.access {
            FrozenAccess::Controller { .. } => DataRole::Controller,
            FrozenAccess::ObserverOnce => DataRole::Observer,
        }
    }
    pub(crate) fn parse(text: &str) -> Result<Self, SafeError> {
        let anchor = Instant::now();
        if text.len() > 16384 {
            return Err(invalid());
        }
        let c: Candidate = serde_json::from_str(text).map_err(|_| invalid())?;
        if c.version != 1 {
            return Err(invalid());
        }
        let access = match c.access_mode {
            RequestedAccess::Controller => FrozenAccess::Controller {
                remember: c.remember,
            },
            RequestedAccess::ObserverOnce if !c.remember => FrozenAccess::ObserverOnce,
            RequestedAccess::ObserverOnce => return Err(invalid()),
        };
        key_name(&c.endpoint.key_name)?;
        key_name(&c.relay.key_name)?;
        let address: std::net::SocketAddr = c.address.parse().map_err(|_| invalid())?;
        if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
            return Err(invalid());
        }
        let origin = CanonicalOrigin::trusted(&c.origin).map_err(|_| invalid())?;
        if c.origin.len() > 512 || origin.as_str() != c.origin {
            return Err(invalid());
        }
        if matches!(access, FrozenAccess::ObserverOnce)
            && url::Url::parse(origin.as_str())
                .ok()
                .and_then(|endpoint| endpoint.port_or_known_default())
                != Some(address.port())
        {
            return Err(invalid());
        }
        let mode = match c.relay_mode.as_str() {
            "legacyV1" => RelayMode::Legacy,
            "rendezvousV1" => RelayMode::RendezvousV1,
            _ => return Err(invalid()),
        };
        let end = anchor + Duration::from_secs(120);
        let binding = FrozenHostBinding {
            native_profile: HostKeyProfileIdentity {
                id: nonzero(&c.profile_id)?,
                revision: decimal(&c.profile_revision)?,
            },
            endpoint_spki: fixed(&c.endpoint.spki)?,
            relay_spki: fixed(&c.relay.spki)?,
            origin: origin.clone(),
            relay_id: nonzero(&c.relay_id)?,
            profile_cap: end,
        };
        // 构造只验证引用，实际 silent open 留到原生首次 Yes 之后。
        NativeHostProfile::new(binding.clone(), &c.endpoint.key_name, &c.relay.key_name)
            .map_err(|_| invalid())?;
        let tls_key = public(&c.browser.tls_spki)?;
        public(&c.browser.grant_spki)?;
        public(&c.browser.relay_spki)?;
        nonzero::<16>(&c.browser.relay_id)?;
        nonzero::<32>(&c.principal)?;
        nonzero::<32>(&c.activation)?;
        for v in [
            &c.host_key_epoch,
            &c.client_key_epoch,
            &c.policy_epoch,
            &c.issuer_epoch,
            &c.incarnation,
        ] {
            decimal(v)?;
        }
        let browser_origin = tauri::Url::parse(&c.browser.origin).map_err(|_| invalid())?;
        let browser_host = browser_origin.host_str().ok_or_else(invalid)?;
        if browser_origin.scheme() != "https"
            || browser_origin.origin().ascii_serialization() != c.browser.origin
            || c.browser.origin.len() > 512
            || c.browser.rp_id.is_empty()
            || c.browser.rp_id.len() > 253
            || !(browser_host == c.browser.rp_id
                || browser_host.ends_with(&format!(".{}", c.browser.rp_id)))
        {
            return Err(invalid());
        }
        if c.browser.database_name.is_empty()
            || c.browser.database_name.len() > 64
            || !c
                .browser
                .database_name
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || b"._-".contains(&v))
        {
            return Err(invalid());
        }
        if let Some(revision) = &c.browser.expected_revision {
            decimal(revision)?;
        }
        for artifact in [&c.browser.wasm_url, &c.browser.glue_url] {
            let url = tauri::Url::parse(artifact).map_err(|_| invalid())?;
            if artifact.len() > 2048
                || !artifact.is_ascii()
                || artifact.contains(['%', '\\'])
                || url.as_str() != artifact
                || url.origin().ascii_serialization() != c.browser.origin
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid());
            }
        }
        crate::remote_host_configuration::hex(&c.browser.user_handle, 64)?;
        let tunnel = nonzero(&c.tunnel)?;
        let frozen = Self {
            candidate: c,
            access,
            end,
            anchor,
            address,
            origin,
            mode,
            tunnel,
            binding,
            tls_key,
            #[cfg(test)]
            fixture_keys: None,
        };
        // verifier 本身仍由原 run owner 创建；此处不创造 credential 或 BrowserProfile。
        Ok(frozen)
    }
    pub(crate) fn supplier(&self) -> Result<FrozenHostSupplier, SafeError> {
        let profile = NativeHostProfile::new(
            self.binding.clone(),
            &self.candidate.endpoint.key_name,
            &self.candidate.relay.key_name,
        )
        .map_err(|_| invalid())?;
        #[cfg(test)]
        let profile = if let Some((scalars, opened)) = &self.fixture_keys {
            profile.test_anonymous(*scalars, opened.clone())
        } else {
            profile
        };
        Ok(profile.freeze())
    }
    #[cfg(test)]
    pub(crate) fn test_keys(
        &mut self,
        scalars: [[u8; 32]; 2],
        opened: Arc<[std::sync::atomic::AtomicUsize; 2]>,
    ) {
        self.fixture_keys = Some((scalars, opened));
    }
    pub(crate) fn endpoint(&self) -> Result<P256Spki, SafeError> {
        public(&self.candidate.endpoint.spki)
    }
    pub(crate) fn context(&self) -> Result<BrowserRegistrationContext, SafeError> {
        let b = &self.candidate.browser;
        Ok(BrowserRegistrationContext {
            origin: b.origin.clone(),
            rp_id: b.rp_id.clone(),
            user_handle: crate::remote_host_configuration::hex(&b.user_handle, 64)?,
            owner_binding: [0; 32],
            tls_key: self.tls_key.clone(),
            grant_key: public(&b.grant_spki)?,
            relay_key: public(&b.relay_spki)?,
            allow_backup_eligible: false,
            allow_backed_up: false,
        })
    }
    pub(crate) fn render(&self) -> String {
        let c = &self.candidate;
        let details = format!("Approve one registration connection and use these EXISTING native keys?\nThis authorizes one Relay signature and one endpoint TLS signature. It does not create keys, authenticate a person, approve browser storage, or grant terminal control. Compare the browser public keys and independent Host pin before agreeing. The original 120-second deadline is not renewed.\nRelay: {} / {} / mode {} / tunnel {}\nNative profile: {} revision {}\nMicrosoft Software KSP / current user / silent open-only\nEndpoint key: {}\nEndpoint SPKI: {}\nRelay key: {}\nRelay SPKI: {}\nBrowser origin: {}\nRP: {}\nRequested user association (not authenticated): {}\nBrowser TLS SPKI: {}\nBrowser grant SPKI: {}\nBrowser relay SPKI: {}",self.origin.as_str(),self.address,c.relay_mode,c.tunnel,c.profile_id,c.profile_revision,c.endpoint.key_name,c.endpoint.spki,c.relay.key_name,c.relay.spki,c.browser.origin,c.browser.rp_id,c.browser.user_handle,c.browser.tls_spki,c.browser.grant_spki,c.browser.relay_spki);
        match self.access {
            FrozenAccess::Controller { .. } => details,
            FrozenAccess::ObserverOnce => {
                if self.product_role_enabled() {
                    format!("Requested role: one-time read-only Observer. This is not permission to view a session; that requires a separate desktop approval.\n{details}")
                } else {
                    format!("Requested role: one-time read-only Observer (not enabled).\n{details}")
                }
            }
        }
    }
    pub(crate) fn installation(
        &self,
        evidence:&cc_desk_remote_security::browser_registration::VerifiedBrowserRegistrationBinding<'_>,
        admission: &NativeRunAdmission,
    ) -> Result<(FrozenHostApproval, BrowserInstallationTarget), SafeError> {
        let c = &self.candidate;
        let browser = BrowserProfile::enrolled(
            evidence.origin,
            evidence.rp_id,
            evidence.credential_id.to_vec(),
            evidence.credential_key.clone(),
            evidence.backup_eligible,
            evidence.allow_backed_up,
        )
        .map_err(|_| invalid())?;
        let anchor = self.anchor;
        let fields = HostApprovalDescription {
            endpoint_id: self.endpoint()?.endpoint_id(Role::Host),
            endpoint_role: Role::Host,
            relay_binding: (self.binding.relay_id, RelayRole::Host),
            address: self.address,
            tunnel: self.tunnel,
            relay_mode: self.mode,
            phone_pin: PeerPin::new(Role::Client, self.tls_key.clone()),
            binding: HostSnapshot {
                host_key_epoch: decimal(&c.host_key_epoch)?,
                client_key_epoch: decimal(&c.client_key_epoch)?,
                activation: nonzero(&c.activation)?,
                incarnation: decimal(&c.incarnation)?,
                policy_epoch: decimal(&c.policy_epoch)?,
                issuer_epoch: decimal(&c.issuer_epoch)?,
                origin: self.origin.clone(),
            },
            browser,
            issuer: public(&c.browser.grant_spki)?,
            principal: nonzero(&c.principal)?,
            policy: NativeGrantPolicy {
                scope: admission.scope()?,
                role: self.data_role(),
                duration: Duration::from_secs(30),
            },
            clock: Arc::new(move || {
                u64::try_from(anchor.elapsed().as_millis())
                    .map_err(|_| cc_desk_remote_security::GrantError::Revoked)
            }),
            expires_host_ms: 120000,
            setup_budget: Duration::from_secs(8),
            io_budget: Duration::from_secs(2),
        };
        let host = FrozenHostApproval::supplied(fields, admission, self.end, self.supplier()?)
            .and_then(|host| host.with_formal_observer_endpoint(self.formal_observer_endpoint()))
            .map_err(|_| invalid())?;
        Ok((
            host,
            BrowserInstallationTarget {
                database_name: c.browser.database_name.clone(),
                expected_revision: c.browser.expected_revision.clone(),
                host_spki: self.binding.endpoint_spki.to_vec(),
                relay_id: nonzero(&c.browser.relay_id)?,
                wasm_url: c.browser.wasm_url.clone(),
                glue_url: c.browser.glue_url.clone(),
                peer_budget_ms: 10000,
            },
        ))
    }
}

#[cfg(test)]
pub(crate) fn test_json() -> String {
    use ring::signature::KeyPair;
    fn spki() -> String {
        let rng = ring::rand::SystemRandom::new();
        let encoded = ring::signature::EcdsaKeyPair::generate_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &rng,
        )
        .unwrap();
        let pair = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            encoded.as_ref(),
            &rng,
        )
        .unwrap();
        let mut spki = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
            0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        spki.extend_from_slice(pair.public_key().as_ref());
        spki.iter().map(|b| format!("{b:02x}")).collect()
    }
    serde_json::json!({"version":1,"address":"127.0.0.1:443","origin":"wss://relay.invalid/","relayMode":"rendezvousV1","tunnel":"01".repeat(16),"relayId":"02".repeat(16),"profileId":"03".repeat(16),"profileRevision":"1","endpoint":{"keyName":"TEST_ONLY_ENDPOINT","spki":spki()},"relay":{"keyName":"TEST_ONLY_RELAY","spki":spki()},"principal":"01".repeat(32),"activation":"02".repeat(32),"hostKeyEpoch":"1","clientKeyEpoch":"1","policyEpoch":"1","issuerEpoch":"1","incarnation":"1","browser":{"origin":"https://desk.invalid","rpId":"desk.invalid","userHandle":"07".repeat(32),"tlsSpki":spki(),"grantSpki":spki(),"relaySpki":spki(),"relayId":"04".repeat(16),"databaseName":"test-pairing","expectedRevision":null,"wasmUrl":"https://desk.invalid/tls.wasm","glueUrl":"https://desk.invalid/tls.js"}}).to_string()
}
