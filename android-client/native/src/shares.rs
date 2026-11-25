//! Signed, bounded live offers from the originally verified desktop.
//! A catalog is a display/selection token only. It cannot start a session.
use crate::enrollment::{
    InvalidEnrollment, RestoredMembership, lower_hex, nonzero_decimal, read_envelope,
    signed_message, verify_signature,
};
use cc_desk_remote_record::{
    appbind::CanonicalOrigin,
    grant::{SessionScope, encode_scope},
};
use ring::digest;
use serde::Deserialize;

const CATALOG_DOMAIN: &[u8] = b"cc-desk.native-pairing.shares.v1\0";
const MAX_OFFERS: usize = 4;
const MAX_CATALOG_TTL_MS: u64 = 60_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OfferDto {
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
    id: [u8; 16],
    scope: SessionScope,
    label: String,
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
        route: dto.relay_origin,
        tunnel: catalog.tunnel,
        endpoint_binding: catalog.endpoint_binding,
        host_spki: *member.host_spki(),
        scope,
        expires_unix_ms: expires,
    })
}
