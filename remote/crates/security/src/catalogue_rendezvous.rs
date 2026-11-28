//! Distinct signed authority for the read-only catalogue rendezvous.
//! A relay selection or a previous terminal offer is not sufficient.
use crate::{P256Spki, session_catalogue::ListMode};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

const DOMAIN: &[u8] = b"cc-desk.catalogue-ready.read-only.v1\0";
const PAYLOAD_LEN: usize = 16 + 32 + 32 + 16 + 16 + 8;
const SIGNATURE_LEN: usize = 64;
const MAX_TTL_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogueSelection {
    pub tunnel: [u8; 16],
    pub endpoint_binding: [u8; 32],
    pub catalog_digest: [u8; 32],
    pub offer: [u8; 16],
    pub selection: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogueReady {
    pub selection: CatalogueSelection,
    pub expires_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCatalogueReady;

impl CatalogueReady {
    fn payload(&self) -> [u8; PAYLOAD_LEN] {
        let s = self.selection;
        let mut bytes = [0; PAYLOAD_LEN];
        bytes[..16].copy_from_slice(&s.tunnel);
        bytes[16..48].copy_from_slice(&s.endpoint_binding);
        bytes[48..80].copy_from_slice(&s.catalog_digest);
        bytes[80..96].copy_from_slice(&s.offer);
        bytes[96..112].copy_from_slice(&s.selection);
        bytes[112..].copy_from_slice(&self.expires_unix_ms.to_be_bytes());
        bytes
    }

    /// Only used after local host consent and the exact current phone selection.
    pub fn signing_message(&self) -> Vec<u8> {
        [DOMAIN, &self.payload()].concat()
    }

    pub fn envelope(&self, signature: [u8; SIGNATURE_LEN]) -> String {
        let packet = [&self.payload()[..], &signature[..]].concat();
        URL_SAFE_NO_PAD.encode(packet)
    }
}

const OFFER_DOMAIN: &[u8] = b"cc-desk.catalogue-offer.read-only.v1\0";
const ARCHIVE_OFFER_DOMAIN: &[u8] = b"cc-desk.catalogue-offer.archive-read-only.v1\0";
const OFFER_LEN: usize = 16 + 32 + 16 + 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogueOffer {
    pub tunnel: [u8; 16],
    pub endpoint_binding: [u8; 32],
    pub offer: [u8; 16],
    pub expires_unix_ms: u64,
}

impl CatalogueOffer {
    fn payload(&self) -> [u8; OFFER_LEN] {
        let mut bytes = [0; OFFER_LEN];
        bytes[..16].copy_from_slice(&self.tunnel);
        bytes[16..48].copy_from_slice(&self.endpoint_binding);
        bytes[48..64].copy_from_slice(&self.offer);
        bytes[64..].copy_from_slice(&self.expires_unix_ms.to_be_bytes());
        bytes
    }

    pub fn signing_message(&self) -> Vec<u8> {
        self.signing_message_for(ListMode::Sessions)
    }

    pub fn signing_message_for(&self, mode: ListMode) -> Vec<u8> {
        let domain = match mode {
            ListMode::Sessions => OFFER_DOMAIN,
            ListMode::Archived => ARCHIVE_OFFER_DOMAIN,
        };
        [domain, &self.payload()].concat()
    }

    pub fn envelope(&self, signature: [u8; SIGNATURE_LEN]) -> String {
        URL_SAFE_NO_PAD.encode([&self.payload()[..], &signature[..]].concat())
    }
}

pub struct VerifiedCatalogueOffer {
    offer: CatalogueOffer,
    digest: [u8; 32],
    host: P256Spki,
    mode: ListMode,
}

impl VerifiedCatalogueOffer {
    pub fn mode(&self) -> ListMode {
        self.mode
    }

    pub fn selection(&self, nonce: [u8; 16]) -> CatalogueSelection {
        CatalogueSelection {
            tunnel: self.offer.tunnel,
            endpoint_binding: self.offer.endpoint_binding,
            catalog_digest: self.digest,
            offer: self.offer.offer,
            selection: nonce,
        }
    }

    pub fn verify_selection(
        &self,
        nonce: [u8; 16],
        wire: &str,
        now: u64,
    ) -> Result<VerifiedCatalogueReady, InvalidCatalogueReady> {
        let mut proof = verify_ready(
            &self.host,
            self.selection(nonce),
            wire,
            now,
            self.offer.expires_unix_ms,
        )?;
        proof.mode = self.mode;
        Ok(proof)
    }
}

pub fn verify_offer(
    host: &P256Spki,
    tunnel: [u8; 16],
    endpoint_binding: [u8; 32],
    advertised_digest: [u8; 32],
    signed: &str,
    now: u64,
) -> Result<VerifiedCatalogueOffer, InvalidCatalogueReady> {
    verify_offer_for(
        host,
        tunnel,
        endpoint_binding,
        advertised_digest,
        signed,
        now,
        ListMode::Sessions,
    )
}

pub fn verify_offer_for(
    host: &P256Spki,
    tunnel: [u8; 16],
    endpoint_binding: [u8; 32],
    advertised_digest: [u8; 32],
    signed: &str,
    now: u64,
    mode: ListMode,
) -> Result<VerifiedCatalogueOffer, InvalidCatalogueReady> {
    if tunnel == [0; 16] || signed.len() != ((OFFER_LEN + SIGNATURE_LEN) * 4).div_ceil(3) {
        return Err(InvalidCatalogueReady);
    }
    if ring::digest::digest(&ring::digest::SHA256, signed.as_bytes()).as_ref() != advertised_digest
    {
        return Err(InvalidCatalogueReady);
    }
    let packet = URL_SAFE_NO_PAD
        .decode(signed)
        .map_err(|_| InvalidCatalogueReady)?;
    if packet.len() != OFFER_LEN + SIGNATURE_LEN {
        return Err(InvalidCatalogueReady);
    }
    let offer = CatalogueOffer {
        tunnel,
        endpoint_binding,
        offer: packet[48..64].try_into().unwrap(),
        expires_unix_ms: u64::from_be_bytes(packet[64..OFFER_LEN].try_into().unwrap()),
    };
    if packet[..OFFER_LEN] != offer.payload()
        || offer.offer == [0; 16]
        || offer.expires_unix_ms <= now
        || offer.expires_unix_ms - now > MAX_TTL_MS
    {
        return Err(InvalidCatalogueReady);
    }
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &host.0[26..])
        .verify(&offer.signing_message_for(mode), &packet[OFFER_LEN..])
        .map_err(|_| InvalidCatalogueReady)?;
    Ok(VerifiedCatalogueOffer {
        offer,
        digest: advertised_digest,
        host: host.clone(),
        mode,
    })
}

pub struct VerifiedCatalogueReady {
    ready: CatalogueReady,
    host_id: [u8; 32],
    mode: ListMode,
}

impl VerifiedCatalogueReady {
    pub fn selection(&self) -> CatalogueSelection {
        self.ready.selection
    }
    pub(crate) fn mode(&self) -> ListMode {
        self.mode
    }
    pub(crate) fn host_id(&self) -> [u8; 32] {
        self.host_id
    }
    pub fn expires_unix_ms(&self) -> u64 {
        self.ready.expires_unix_ms
    }
}

/// The expected fields are from the phone's current paired membership and locally
/// generated selection, never from the untrusted relay reply. host is the phone's
/// previously verified Host key. This proof authorizes no command or session input.
pub fn verify_ready(
    host: &P256Spki,
    expected: CatalogueSelection,
    signed: &str,
    now_unix_ms: u64,
    catalog_expiry_ms: u64,
) -> Result<VerifiedCatalogueReady, InvalidCatalogueReady> {
    let len = ((PAYLOAD_LEN + SIGNATURE_LEN) * 4).div_ceil(3);
    if signed.len() != len
        || expected.tunnel == [0; 16]
        || expected.offer == [0; 16]
        || expected.selection == [0; 16]
    {
        return Err(InvalidCatalogueReady);
    }
    let packet = URL_SAFE_NO_PAD
        .decode(signed)
        .map_err(|_| InvalidCatalogueReady)?;
    if packet.len() != PAYLOAD_LEN + SIGNATURE_LEN {
        return Err(InvalidCatalogueReady);
    }
    let expires = u64::from_be_bytes(packet[112..PAYLOAD_LEN].try_into().unwrap());
    if expires <= now_unix_ms || expires - now_unix_ms > MAX_TTL_MS || expires > catalog_expiry_ms {
        return Err(InvalidCatalogueReady);
    }
    let ready = CatalogueReady {
        selection: expected,
        expires_unix_ms: expires,
    };
    if packet[..PAYLOAD_LEN] != ready.payload() {
        return Err(InvalidCatalogueReady);
    }
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &host.0[26..])
        .verify(&ready.signing_message(), &packet[PAYLOAD_LEN..])
        .map_err(|_| InvalidCatalogueReady)?;
    Ok(VerifiedCatalogueReady {
        ready,
        host_id: host.endpoint_id(crate::Role::Host),
        mode: ListMode::Sessions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
    };

    #[test]
    fn exact_signed_ready_rejects_another_device_route_or_expiry() {
        let random = SystemRandom::new();
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &random).unwrap();
        let host =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let head = b"\x30\x59\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07\x03\x42\x00";
        let host_spki = P256Spki::new(&[&head[..], host.public_key().as_ref()].concat()).unwrap();
        let pick = CatalogueSelection {
            tunnel: [1; 16],
            endpoint_binding: [2; 32],
            catalog_digest: [3; 32],
            offer: [4; 16],
            selection: [5; 16],
        };
        let ready = CatalogueReady {
            selection: pick,
            expires_unix_ms: 1000,
        };
        let signature = host.sign(&random, &ready.signing_message()).unwrap();
        let wire = ready.envelope(signature.as_ref().try_into().unwrap());
        let proof = verify_ready(&host_spki, pick, &wire, 1, 1000).unwrap();
        assert_eq!(proof.selection(), pick);
        for expected in [
            CatalogueSelection {
                selection: [6; 16],
                ..pick
            },
            CatalogueSelection {
                endpoint_binding: [6; 32],
                ..pick
            },
            CatalogueSelection {
                tunnel: [6; 16],
                ..pick
            },
            CatalogueSelection {
                offer: [6; 16],
                ..pick
            },
            CatalogueSelection {
                catalog_digest: [6; 32],
                ..pick
            },
        ] {
            assert!(verify_ready(&host_spki, expected, &wire, 1, 1000).is_err());
        }
        assert!(verify_ready(&host_spki, pick, &wire, 1000, 1000).is_err());
        assert!(verify_ready(&host_spki, pick, &wire, 1, 999).is_err());
        let old_purpose = host
            .sign(
                &random,
                &[
                    b"cc-desk.native-pairing.share-ready.v1\0",
                    &ready.payload()[..],
                ]
                .concat(),
            )
            .unwrap();
        let wire = ready.envelope(old_purpose.as_ref().try_into().unwrap());
        assert!(verify_ready(&host_spki, pick, &wire, 1, 1000).is_err());

        let offer = CatalogueOffer {
            tunnel: pick.tunnel,
            endpoint_binding: pick.endpoint_binding,
            offer: pick.offer,
            expires_unix_ms: 1000,
        };
        let signature = host.sign(&random, &offer.signing_message()).unwrap();
        let wire = offer.envelope(signature.as_ref().try_into().unwrap());
        let hash: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, wire.as_bytes())
            .as_ref()
            .try_into()
            .unwrap();
        assert!(verify_offer(&host_spki, [8; 16], pick.endpoint_binding, hash, &wire, 1).is_err());
        assert!(
            verify_offer(
                &host_spki,
                pick.tunnel,
                pick.endpoint_binding,
                [8; 32],
                &wire,
                1
            )
            .is_err()
        );
        let verified = verify_offer(
            &host_spki,
            pick.tunnel,
            pick.endpoint_binding,
            hash,
            &wire,
            1,
        )
        .unwrap();
        assert_eq!(verified.mode(), ListMode::Sessions);
        assert!(
            verify_offer_for(
                &host_spki,
                pick.tunnel,
                pick.endpoint_binding,
                hash,
                &wire,
                1,
                ListMode::Archived
            )
            .is_err()
        );
        let sig = host
            .sign(&random, &offer.signing_message_for(ListMode::Archived))
            .unwrap();
        let archived = offer.envelope(sig.as_ref().try_into().unwrap());
        let archived_hash = ring::digest::digest(&ring::digest::SHA256, archived.as_bytes())
            .as_ref()
            .try_into()
            .unwrap();
        assert!(
            verify_offer(
                &host_spki,
                pick.tunnel,
                pick.endpoint_binding,
                archived_hash,
                &archived,
                1
            )
            .is_err()
        );
        let archived = verify_offer_for(
            &host_spki,
            pick.tunnel,
            pick.endpoint_binding,
            archived_hash,
            &archived,
            1,
            ListMode::Archived,
        )
        .unwrap();
        assert_eq!(archived.mode(), ListMode::Archived);
        assert_ne!(
            archived.selection(pick.selection),
            verified.selection(pick.selection)
        );
        let selected = CatalogueReady {
            selection: verified.selection(pick.selection),
            expires_unix_ms: 900,
        };
        let signature = host.sign(&random, &selected.signing_message()).unwrap();
        let wire = selected.envelope(signature.as_ref().try_into().unwrap());
        assert!(verified.verify_selection([8; 16], &wire, 1).is_err());
        assert!(archived.verify_selection(pick.selection, &wire, 1).is_err());
        assert_eq!(
            verified
                .verify_selection(pick.selection, &wire, 1)
                .unwrap()
                .selection(),
            selected.selection
        );
    }
}
