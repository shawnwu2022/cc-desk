//! Public scalar1/generator fixture, never enroll or use for real access.
use cc_desk_remote_record::grant::{CanonicalGrant, SessionScope};
use cc_desk_remote_security::P256Spki;
use coset::{CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn point() -> Vec<u8> {
    hex(
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    )
}
pub(super) fn admin() -> P256Spki {
    let mut spki = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend(point());
    P256Spki::new(&spki).unwrap()
}
pub(super) fn scope() -> SessionScope {
    SessionScope {
        session: "02".repeat(16),
        run: "TEST_ONLY_BOUND_PTY".into(),
        generation: "3".into(),
    }
}
pub(super) fn signed(payload: &CanonicalGrant, kid: [u8; 32]) -> Vec<u8> {
    let mut scalar = [0; 32];
    scalar[31] = 1;
    let rng = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &scalar,
        &point(),
        &rng,
    )
    .unwrap();
    CoseSign1Builder::new()
        .protected(
            HeaderBuilder::new()
                .algorithm(iana::Algorithm::ES256)
                .content_type("application/cc-desk-grant+json".into())
                .key_id(kid.to_vec())
                .build(),
        )
        .payload(cc_desk_remote_record::grant::encode(payload).unwrap())
        .create_signature(b"CC-DESK-DATA-GRANT-v1\0", |bytes| {
            key.sign(&rng, bytes).unwrap().as_ref().to_vec()
        })
        .build()
        .to_tagged_vec()
        .unwrap()
}
