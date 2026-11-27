//! Signed, bounded live offers from the originally verified desktop.
//! A catalog is a display/selection token only. It cannot start a session.
use crate::enrollment::{
    InvalidEnrollment, RestoredMembership, lower_hex, nonzero_decimal, read_envelope,
    signed_message, verify_signature,
};
use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{DataRole, SessionScope, encode_scope},
};
use cc_desk_remote_security::{
    CancelToken, LocalIdentity, P256Spki, PeerPin, Role, Unbound,
    catalogue_read_channel::{CataloguePair, ReadError, fetch_admitted},
    catalogue_rendezvous::{VerifiedCatalogueOffer, VerifiedCatalogueReady},
    session_catalogue::{DisplayRow, ListMode},
};
use ring::digest;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite};

const CATALOG_DOMAIN: &[u8] = b"cc-desk.native-pairing.shares.v1\0";
const MAX_OFFERS: usize = 4;
const MAX_CATALOG_TTL_MS: u64 = 60_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OfferDto {
    #[serde(default = "observer", deserialize_with = "role")]
    role: DataRole,
    offer_id: String,
    session: String,
    run: String,
    generation: String,
    label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogDto {
    version: u8,
    relay_origin: CanonicalOrigin,
    tunnel: String,
    endpoint_binding: String,
    catalog_id: String,
    offers: Vec<OfferDto>,
    expires_unix_ms: String,
}

pub struct VerifiedCatalog {
    route: CanonicalOrigin,
    tunnel: [u8; 16],
    endpoint_binding: [u8; 32],
    id: [u8; 16],
    digest: [u8; 32],
    expires_unix_ms: u64,
    offers: Vec<VerifiedOffer>,
}

pub struct VerifiedOffer {
    role: DataRole,
    id: [u8; 16],
    scope: SessionScope,
    label: String,
}

/// This route can only read a catalogue over its separate ALPN. Its proof
/// is deliberately not convertible into the single-session Viewer authority.
pub fn verify_catalogue_route_offer(
    member: &RestoredMembership,
    signed: &str,
    relay_digest: [u8; 32],
    now_unix_ms: u64,
) -> Result<cc_desk_remote_security::catalogue_rendezvous::VerifiedCatalogueOffer, InvalidEnrollment>
{
    verify_catalogue_route_offer_for(
        member,
        signed,
        relay_digest,
        now_unix_ms,
        ListMode::Sessions,
    )
}

pub fn verify_catalogue_route_offer_for(
    member: &RestoredMembership,
    signed: &str,
    relay_digest: [u8; 32],
    now_unix_ms: u64,
    mode: ListMode,
) -> Result<VerifiedCatalogueOffer, InvalidEnrollment> {
    let host = cc_desk_remote_security::P256Spki::new(member.host_spki())
        .map_err(|_| InvalidEnrollment)?;
    cc_desk_remote_security::catalogue_rendezvous::verify_offer_for(
        &host,
        member.tunnel(),
        member.endpoint_binding(),
        relay_digest,
        signed,
        now_unix_ms,
        mode,
    )
    .map_err(|_| InvalidEnrollment)
}

/// A consumed, exact device selection for one read. This does not grant a
/// session, revive a terminal lease, or admit a new relay connection.
pub struct SelectedCatalogueRead {
    pin: PeerPin,
    pair: CataloguePair,
    ready: VerifiedCatalogueReady,
    mode: ListMode,
}

pub fn prepare_catalogue_read(
    member: &RestoredMembership,
    offer: &VerifiedCatalogueOffer,
    nonce: [u8; 16],
    signed_ready: &str,
    now_unix_ms: u64,
) -> Result<SelectedCatalogueRead, InvalidEnrollment> {
    let ready = offer
        .verify_selection(nonce, signed_ready, now_unix_ms)
        .map_err(|_| InvalidEnrollment)?;
    let selection = ready.selection();
    if selection.tunnel != member.tunnel()
        || selection.endpoint_binding != member.endpoint_binding()
    {
        return Err(InvalidEnrollment);
    }
    let host = P256Spki::new(member.host_spki()).map_err(|_| InvalidEnrollment)?;
    Ok(SelectedCatalogueRead {
        pin: PeerPin::new(Role::Host, host),
        pair: CataloguePair {
            host: member.host_id(),
            client: member.catalogue_client_id()?,
        },
        ready,
        mode: offer.mode(),
    })
}

impl SelectedCatalogueRead {
    /// The native route owner supplies the stream and its admitted tunnel ID.
    /// The local key must be the endpoint key from the restored membership.
    pub async fn fetch<S>(
        self,
        io: Unbound<S>,
        identity: LocalIdentity,
        route_tunnel: [u8; 16],
        now_unix_ms: u64,
        cancel: CancelToken,
        mode: ListMode,
    ) -> Result<Vec<DisplayRow>, ReadError>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        if mode != self.mode
            || identity.role() != Role::Client
            || identity.endpoint_id() != self.pair.client
        {
            return Err(ReadError::Denied);
        }
        fetch_admitted(
            io,
            identity,
            self.pin,
            self.pair,
            route_tunnel,
            self.ready,
            now_unix_ms,
            cancel,
            mode,
        )
        .await
    }
}

impl VerifiedCatalog {
    pub fn id(&self) -> [u8; 16] {
        self.id
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn expires_unix_ms(&self) -> u64 {
        self.expires_unix_ms
    }
    pub fn offers(&self) -> &[VerifiedOffer] {
        &self.offers
    }
}
impl VerifiedOffer {
    pub fn role(&self) -> DataRole {
        self.role
    }
    pub fn id(&self) -> [u8; 16] {
        self.id
    }
    pub fn scope(&self) -> &SessionScope {
        &self.scope
    }
    pub fn label(&self) -> &str {
        &self.label
    }
}

fn acceptable_label(label: &str) -> bool {
    !label.trim().is_empty() && label.len()<=48 && !label.chars().any(|c|
        c.is_control() || matches!(c,'/'|'\\'|':'|'\u{061c}'|'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}'))
}

pub fn verify_catalog(
    member: &RestoredMembership,
    outer: &str,
    relay_digest: [u8; 32],
    now_unix_ms: u64,
) -> Result<VerifiedCatalog, InvalidEnrollment> {
    let hash = digest::digest(&digest::SHA256, outer.as_bytes());
    if hash.as_ref() != relay_digest {
        return Err(InvalidEnrollment);
    }
    let (payload, sig) = read_envelope(outer)?;
    verify_signature(
        member.host_spki(),
        &signed_message(CATALOG_DOMAIN, &payload),
        &sig,
    )?;
    let dto: CatalogDto = serde_json::from_slice(&payload).map_err(|_| InvalidEnrollment)?;
    let tunnel = lower_hex(&dto.tunnel)?;
    let endpoint_binding = lower_hex(&dto.endpoint_binding)?;
    let expires_unix_ms = nonzero_decimal(&dto.expires_unix_ms)?;
    if dto.version != 1
        || &dto.relay_origin != member.origin()
        || tunnel != member.tunnel()
        || endpoint_binding != member.endpoint_binding()
        || dto.offers.len() > MAX_OFFERS
        || expires_unix_ms <= now_unix_ms
        || expires_unix_ms - now_unix_ms > MAX_CATALOG_TTL_MS
    {
        return Err(InvalidEnrollment);
    }
    let mut offers: Vec<VerifiedOffer> = Vec::with_capacity(dto.offers.len());
    for dto in dto.offers {
        let id = lower_hex(&dto.offer_id)?;
        let scope = SessionScope {
            session: dto.session,
            run: dto.run,
            generation: dto.generation,
        };
        encode_scope(&scope).map_err(|_| InvalidEnrollment)?;
        if !acceptable_label(&dto.label)
            || offers.iter().any(|old| old.id == id || old.scope == scope)
        {
            return Err(InvalidEnrollment);
        }
        offers.push(VerifiedOffer {
            role: dto.role,
            id,
            scope,
            label: dto.label,
        });
    }
    Ok(VerifiedCatalog {
        route: dto.relay_origin,
        tunnel,
        endpoint_binding,
        id: lower_hex(&dto.catalog_id)?,
        digest: relay_digest,
        expires_unix_ms,
        offers,
    })
}

const READY_DOMAIN: &[u8] = b"cc-desk.native-pairing.share-ready.v1\0";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadyDto {
    #[serde(default = "observer", deserialize_with = "role")]
    role: DataRole,
    version: u8,
    relay_origin: CanonicalOrigin,
    tunnel: String,
    endpoint_binding: String,
    catalog_digest: String,
    offer_id: String,
    selection_id: String,
    session: String,
    run: String,
    generation: String,
    expires_unix_ms: String,
}

pub struct VerifiedShare {
    pub(crate) role: DataRole,
    pub(crate) route: CanonicalOrigin,
    pub(crate) tunnel: [u8; 16],
    pub(crate) endpoint_binding: [u8; 32],
    pub(crate) host_spki: [u8; 91],
    pub(crate) scope: SessionScope,
    pub(crate) expires_unix_ms: u64,
}

/// Verify the original Host's explicit readiness, independent of the Relay
/// pending/acknowledgement. Match exact selection nonce, offer and scope.
pub fn verify_ready(
    member: &RestoredMembership,
    catalog: &VerifiedCatalog,
    offer: [u8; 16],
    selection: [u8; 16],
    outer: &str,
    now_unix_ms: u64,
) -> Result<VerifiedShare, InvalidEnrollment> {
    if selection == [0; 16]
        || catalog.expires_unix_ms <= now_unix_ms
        || member.origin() != &catalog.route
        || member.tunnel() != catalog.tunnel
        || member.endpoint_binding() != catalog.endpoint_binding
    {
        return Err(InvalidEnrollment);
    }
    let expected = catalog
        .offers
        .iter()
        .find(|p| p.id == offer)
        .ok_or(InvalidEnrollment)?;
    let (payload, sig) = read_envelope(outer)?;
    verify_signature(
        member.host_spki(),
        &signed_message(READY_DOMAIN, &payload),
        &sig,
    )?;
    let dto: ReadyDto = serde_json::from_slice(&payload).map_err(|_| InvalidEnrollment)?;
    let expires = nonzero_decimal(&dto.expires_unix_ms)?;
    let scope = SessionScope {
        session: dto.session,
        run: dto.run,
        generation: dto.generation,
    };
    if dto.version != 1
        || dto.role != expected.role
        || dto.relay_origin != catalog.route
        || lower_hex::<16>(&dto.tunnel)? != catalog.tunnel
        || lower_hex::<32>(&dto.endpoint_binding)? != catalog.endpoint_binding
        || lower_hex::<32>(&dto.catalog_digest)? != catalog.digest
        || lower_hex::<16>(&dto.offer_id)? != offer
        || lower_hex::<16>(&dto.selection_id)? != selection
        || scope != expected.scope
        || expires <= now_unix_ms
        || expires > catalog.expires_unix_ms
    {
        return Err(InvalidEnrollment);
    }
    Ok(VerifiedShare {
        role: dto.role,
        route: dto.relay_origin,
        tunnel: catalog.tunnel,
        endpoint_binding: catalog.endpoint_binding,
        host_spki: *member.host_spki(),
        scope,
        expires_unix_ms: expires,
    })
}

fn observer() -> DataRole {
    DataRole::Observer
}
fn role<'de, D: serde::Deserializer<'de>>(d: D) -> Result<DataRole, D::Error> {
    DataRole::deserialize(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use ring::{
        rand::SystemRandom,
        signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
    };
    use serde_json::{Value, json};

    fn envelope(key: &EcdsaKeyPair, domain: &[u8], dto: Value) -> String {
        let payload = serde_json::to_vec(&dto).unwrap();
        let sig = key
            .sign(&SystemRandom::new(), &signed_message(domain, &payload))
            .unwrap();
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"payloadB64":URL_SAFE_NO_PAD.encode(payload),
            "signatureB64":URL_SAFE_NO_PAD.encode(sig.as_ref())}))
            .unwrap(),
        )
    }
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|n| format!("{n:02x}")).collect()
    }
    #[tokio::test]
    async fn selected_catalogue_reads_only_from_the_paired_desktop() {
        use cc_desk_remote_security::{
            LocalIdentity, P256Spki, PeerPin, Role, Unbound, cancellation,
            catalogue_read_channel::{CataloguePair, serve},
            catalogue_rendezvous::{CatalogueOffer, CatalogueReady},
            session_catalogue::{DisplayLifecycle, DisplayRow, ListMode},
        };
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

        let rng = SystemRandom::new();
        let identity = |role| {
            let pkcs =
                EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
            let key =
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs.as_ref(), &rng)
                    .unwrap();
            let signer = rustls::crypto::ring::sign::any_supported_type(&PrivateKeyDer::Pkcs8(
                PrivatePkcs8KeyDer::from(pkcs.as_ref()),
            ))
            .unwrap();
            let der: [u8; 91] = signer.public_key().unwrap().as_ref().try_into().unwrap();
            let pin = P256Spki::new(&der).unwrap();
            (
                key,
                der,
                PeerPin::new(role, pin.clone()),
                LocalIdentity::new(role, pin, signer).unwrap(),
            )
        };
        let (host_key, host_der, host_pin, host_id) = identity(Role::Host);
        let (_, client_der, client_pin, client_id) = identity(Role::Client);
        let member = crate::enrollment::test_pinned_catalogue_membership(host_der, client_der);
        let pair = CataloguePair {
            host: host_pin.endpoint_id(),
            client: client_pin.endpoint_id(),
        };
        let sign = |message: &[u8]| {
            host_key
                .sign(&rng, message)
                .unwrap()
                .as_ref()
                .try_into()
                .unwrap()
        };
        let offer = CatalogueOffer {
            tunnel: member.tunnel(),
            endpoint_binding: member.endpoint_binding(),
            offer: [8; 16],
            expires_unix_ms: 5_000,
        };
        let offer_wire = offer.envelope(sign(&offer.signing_message()));
        let digest = digest::digest(&digest::SHA256, offer_wire.as_bytes())
            .as_ref()
            .try_into()
            .unwrap();
        let offer = verify_catalogue_route_offer(&member, &offer_wire, digest, 1_000).unwrap();
        let nonce = [9; 16];
        let ready = CatalogueReady {
            selection: offer.selection(nonce),
            expires_unix_ms: 4_000,
        };
        let wire = ready.envelope(sign(&ready.signing_message()));

        assert!(prepare_catalogue_read(&member, &offer, [10; 16], &wire, 1_000).is_err());
        assert!(prepare_catalogue_read(&member, &offer, nonce, &wire, 4_000).is_err());
        let (_, _, _, unpaired) = identity(Role::Client);
        let denied = prepare_catalogue_read(&member, &offer, nonce, &wire, 1_000).unwrap();
        let (unused, _peer) = tokio::io::duplex(64);
        let (_cancel, token) = cancellation();
        assert_eq!(
            denied
                .fetch(
                    Unbound::new(unused),
                    unpaired,
                    member.tunnel(),
                    1_000,
                    token,
                    ListMode::Sessions
                )
                .await,
            Err(cc_desk_remote_security::catalogue_read_channel::ReadError::Denied),
        );

        let selected = prepare_catalogue_read(&member, &offer, nonce, &wire, 1_000).unwrap();
        let (a, b) = tokio::io::duplex(8192);
        let (_ha, hc) = cancellation();
        let (_ca, cc) = cancellation();
        let expected = DisplayRow {
            lifecycle: DisplayLifecycle::Closed,
            title: "Closed session".into(),
        };
        let (served, rows) = tokio::join!(
            serve(
                Unbound::new(a),
                host_id,
                client_pin,
                pair,
                hc,
                |candidate| std::future::ready(candidate == pair),
                |mode| {
                    assert_eq!(mode, ListMode::Sessions);
                    std::future::ready(Ok(vec![expected.clone()]))
                }
            ),
            selected.fetch(
                Unbound::new(b),
                client_id,
                member.tunnel(),
                1_000,
                cc,
                ListMode::Sessions
            ),
        );
        assert_eq!(served, Ok(()));
        assert_eq!(rows.unwrap(), vec![expected]);
    }

    #[test]
    fn real_p256_offer_and_ready_are_bound_to_role_exact_scope_selection_and_tunnel() {
        let rng = SystemRandom::new();
        let pkcs = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs.as_ref(), &rng)
            .unwrap();
        let der=[b"\x30\x59\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07\x03\x42\x00".as_slice(),
            key.public_key().as_ref()].concat().try_into().unwrap();
        let member = crate::enrollment::test_pinned_membership(der);
        let offer = [8u8; 16];
        let chosen = [9u8; 16];
        let mut offer_dto = json!({"offerId":hex(&offer),"session":"11".repeat(16),"run":"original","generation":"3","label":"Shared","role":"Controller"});
        let catalog_dto = |offer: Value| {
            json!({"version":1,"relayOrigin":member.origin().as_str(),"tunnel":hex(&member.tunnel()),
            "endpointBinding":hex(&member.endpoint_binding()),"catalogId":"10".repeat(16),"offers":[offer],"expiresUnixMs":"55000"})
        };
        let checked_catalog = |offer: Value| {
            let blob = envelope(&key, CATALOG_DOMAIN, catalog_dto(offer));
            let digest = digest::digest(&digest::SHA256, blob.as_bytes())
                .as_ref()
                .try_into()
                .unwrap();
            verify_catalog(&member, &blob, digest, 1000).unwrap()
        };
        let catalog = checked_catalog(offer_dto.clone());
        assert_eq!(catalog.offers()[0].role(), DataRole::Controller);
        let ready = json!({"version":1,"relayOrigin":member.origin().as_str(),"tunnel":hex(&member.tunnel()),
            "endpointBinding":hex(&member.endpoint_binding()),"catalogDigest":hex(&catalog.digest()),"offerId":hex(&offer),
            "selectionId":hex(&chosen),"session":"11".repeat(16),"run":"original","generation":"3","role":"Controller","expiresUnixMs":"54000"});
        let online = |dto: Value, selection: [u8; 16]| {
            verify_ready(
                &member,
                &catalog,
                offer,
                selection,
                &envelope(&key, READY_DOMAIN, dto),
                1000,
            )
        };
        assert_eq!(
            online(ready.clone(), chosen).unwrap().role,
            DataRole::Controller
        );
        for (k, v) in [
            ("role", json!("Observer")),
            ("generation", json!("4")),
            ("tunnel", json!("ff".repeat(16))),
        ] {
            let mut bad = ready.clone();
            bad[k] = v;
            assert!(online(bad, chosen).is_err());
        }
        assert!(online(ready, [99; 16]).is_err());
        offer_dto.as_object_mut().unwrap().remove("role");
        assert_eq!(
            checked_catalog(offer_dto).offers()[0].role(),
            DataRole::Observer
        );
    }
}
