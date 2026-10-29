use super::grant::*;
#[test]
fn any_native_clock_error_permanently_locks_existing_grants() {
    let now = Arc::new(AtomicU64::new(1000));
    let source = now.clone();
    let mut gate = GrantGate::new(
        context(),
        admin(),
        [4; 32],
        Arc::new(move || {
            if source.load(Ordering::Acquire) == 0 {
                Err(GrantError::Policy)
            } else {
                Ok(source.load(Ordering::Acquire))
            }
        }),
    )
    .unwrap();
    let g = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let a = gate.accept(&signed(&g, gate.issuer_kid())).unwrap();
    now.store(0, Ordering::Release);
    assert!(a.check().is_err());
    now.store(1001, Ordering::Release);
    assert!(a.check().is_err());
}
use super::*;
use cc_desk_remote_record::{
    appbind::{CanonicalOrigin, Context},
    grant::{CanonicalGrant, DataRole, SessionScope},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
pub(super) fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
pub(super) fn admin_point() -> Vec<u8> {
    hex(
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    )
}
pub(super) fn admin() -> P256Spki {
    let mut b = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    b.extend(admin_point());
    P256Spki::new(&b).unwrap()
}
pub(super) fn signed(g: &CanonicalGrant, kid: [u8; 32]) -> Vec<u8> {
    signed_aad(g, kid, AAD)
}
fn signed_aad(g: &CanonicalGrant, kid: [u8; 32], aad: &[u8]) -> Vec<u8> {
    use coset::{CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
    let mut scalar = [0; 32];
    scalar[31] = 1; // Public scalar1 / P256 generator: TEST_ONLY, never enroll.
    let rng = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &scalar,
        &admin_point(),
        &rng,
    )
    .unwrap();
    CoseSign1Builder::new()
        .protected(
            HeaderBuilder::new()
                .algorithm(iana::Algorithm::ES256)
                .content_type(CONTENT_TYPE.into())
                .key_id(kid.to_vec())
                .build(),
        )
        .payload(cc_desk_remote_record::grant::encode(g).unwrap())
        .create_signature(aad, |m| key.sign(&rng, m).unwrap().as_ref().to_vec())
        .build()
        .to_tagged_vec()
        .unwrap()
}
pub(super) fn scope() -> SessionScope {
    SessionScope {
        session: "05".repeat(16),
        run: "PUBLIC_TEST_RUN".into(),
        generation: "7".into(),
    }
}
fn context() -> Context {
    Context {
        host_endpoint_id: "01".repeat(32),
        client_endpoint_id: "02".repeat(32),
        host_key_epoch: "1".into(),
        client_key_epoch: "2".into(),
        activation: "03".repeat(32),
        incarnation: "4".into(),
        policy_epoch: "5".into(),
        issuer_epoch: "6".into(),
        origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
    }
}
fn gate(now: Arc<AtomicU64>) -> GrantGate {
    GrantGate::new(
        context(),
        admin(),
        [4; 32],
        Arc::new(move || Ok(now.load(Ordering::Acquire))),
    )
    .unwrap()
}
#[test]
fn exact_real_signature_grant_is_reusable_without_resetting_deadline() {
    let now = Arc::new(AtomicU64::new(1000));
    let mut gate = gate(now.clone());
    let g = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let b = signed(&g, gate.issuer_kid());
    let accepted = gate.accept(&b).unwrap();
    assert_eq!(accepted.scope(), &scope());
    assert_eq!(accepted.expires_host_ms(), 5000);
    now.store(2000, Ordering::Release);
    assert_eq!(gate.accept(&b).unwrap().expires_host_ms(), 5000);
    now.store(5000, Ordering::Release);
    assert!(accepted.check().is_err());
    assert!(gate.accept(&b).is_err());
    let next = gate
        .challenge(scope(), DataRole::Observer, Duration::from_secs(10))
        .unwrap();
    let next = gate.accept(&signed(&next, gate.issuer_kid())).unwrap();
    assert_eq!(next.role(), DataRole::Observer);
    gate.revoke();
    assert!(next.check().is_err());
}
#[test]
fn signature_scope_principal_epoch_aad_and_cbor_tampering_never_grant() {
    for case in 0..10 {
        let now = Arc::new(AtomicU64::new(1000));
        let mut gate = gate(now);
        let g = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
            .unwrap();
        let mut changed = g.clone();
        match case {
            0 => changed.context.client_endpoint_id = "09".repeat(32),
            1 => changed.scope.run = "OTHER".into(),
            2 => changed.principal_id = "08".repeat(32),
            3 => changed.context.policy_epoch = "6".into(),
            4 => changed.context.issuer_epoch = "7".into(),
            5 => changed.context.activation = "09".repeat(32),
            6 => changed.expires_host_ms = "6000".into(),
            _ => {}
        }
        let mut bytes = signed(&changed, gate.issuer_kid());
        match case {
            7 => {
                let i = bytes.len() - 1;
                bytes[i] ^= 1
            }
            8 => bytes[0] = 0xd1,
            9 => bytes.insert(0, 0),
            _ => {}
        }
        assert!(gate.accept(&bytes).is_err(), "case{case}");
        assert!(gate.accept(&signed(&g, gate.issuer_kid())).is_ok());
    }
}
#[test]
fn single_challenge_response_limit_and_clock_rollback_fail_closed() {
    let now = Arc::new(AtomicU64::new(1000));
    let mut gate = gate(now.clone());
    let g = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
        .unwrap();
    assert!(
        gate.challenge(scope(), DataRole::Controller, Duration::from_secs(1))
            .is_err()
    );
    now.store(31000, Ordering::Release);
    assert!(gate.accept(&signed(&g, gate.issuer_kid())).is_err());
    let g = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(900))
        .unwrap();
    now.store(30999, Ordering::Release);
    assert!(gate.accept(&signed(&g, gate.issuer_kid())).is_err());
    now.store(32000, Ordering::Release);
    assert!(
        gate.challenge(scope(), DataRole::Controller, Duration::from_secs(1))
            .is_err()
    );
}
#[test]
fn wrong_aad_issuer_and_protected_cose_structure_are_rejected() {
    use coset::TaggedCborSerializable;
    for case in 0..7 {
        let mut gate = gate(Arc::new(AtomicU64::new(1000)));
        let g = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
            .unwrap();
        let mut bytes = if case == 0 {
            signed_aad(&g, gate.issuer_kid(), b"WRONG_DOMAIN")
        } else {
            signed(&g, gate.issuer_kid())
        };
        if case > 0 {
            let mut c = coset::CoseSign1::from_tagged_slice(&bytes).unwrap();
            c.protected.original_data = None;
            match case {
                1 => c.protected.header.key_id[0] ^= 1,
                2 => {
                    c.protected.header.content_type =
                        Some(coset::ContentType::Text("text/plain".into()))
                }
                3 => c.unprotected.key_id = b"unprotected".to_vec(),
                4 => c.payload = None,
                5 => {
                    bytes.splice(..1, [0xd8, 0x12]);
                }
                _ => c.signature.pop().map(|_| ()).unwrap(),
            };
            if case != 5 {
                bytes = c.to_tagged_vec().unwrap()
            }
        }
        assert!(gate.accept(&bytes).is_err(), "{case}");
        assert!(gate.accept(&signed(&g, gate.issuer_kid())).is_ok());
    }
    let mut other = admin();
    other.0[40] ^= 1;
    let now = Arc::new(AtomicU64::new(1000));
    let mut gate = GrantGate::new(
        context(),
        other,
        [4; 32],
        Arc::new(move || Ok(now.load(Ordering::Acquire))),
    )
    .unwrap();
    let g = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    assert_eq!(
        gate.accept(&signed(&g, gate.issuer_kid())).unwrap_err(),
        GrantError::Signature
    );
}
