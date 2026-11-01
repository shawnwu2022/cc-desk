use super::*;
use crate::browser_approval::Approval;
use crate::grant_tests::{admin, scope, signed};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use cc_desk_remote_record::grant::CanonicalGrant;
use cc_desk_remote_record::grant::DataRole;
use std::time::Duration;

pub(super) fn profile() -> BrowserProfile {
    BrowserProfile::enrolled(
        "https://desk.invalid",
        "desk.invalid",
        vec![7; 32],
        admin(),
        false,
        false,
    )
    .unwrap()
}
fn proof(profile: &BrowserProfile, grant: &CanonicalGrant, kid: [u8; 32], case: usize) -> Vec<u8> {
    proof_challenge(
        grant,
        kid,
        profile.challenge(grant, kid, ([8; 32], [9; 32])).unwrap(),
        case,
    )
}
pub(super) fn proof_challenge(
    grant: &CanonicalGrant,
    kid: [u8; 32],
    mut challenge: [u8; 32],
    case: usize,
) -> Vec<u8> {
    if case == 1 {
        challenge[0] ^= 1;
    }
    let mut client = serde_json::json!({"type":"webauthn.get", "challenge":B64.encode(challenge), "origin":"https://desk.invalid", "crossOrigin":false});
    if case == 2 {
        client["origin"] = "https://evil.invalid".into();
    }
    if case == 3 {
        client["type"] = "webauthn.create".into();
    }
    if case == 4 {
        client["crossOrigin"] = true.into();
    }
    if case == 5 {
        client["topOrigin"] = "https://desk.invalid".into();
    }
    let client = serde_json::to_vec(&client).unwrap();
    let mut auth = ring::digest::digest(&ring::digest::SHA256, b"desk.invalid")
        .as_ref()
        .to_vec();
    let flags = match case {
        6 => 1,
        7 => 4,
        8 => 13,
        9 => 21,
        10 => 69,
        11 => 133,
        _ => 5,
    };
    auth.push(flags);
    auth.extend_from_slice(&[0; 4]); // Counter zero is valid; pending challenge prevents replay.
    if case == 12 {
        auth[0] ^= 1;
    }
    let mut material = auth.clone();
    material.extend_from_slice(ring::digest::digest(&ring::digest::SHA256, &client).as_ref());
    // PUBLIC TEST_ONLY scalar1. Never register this fixture credential.
    let mut scalar = [0; 32];
    scalar[31] = 1;
    let rng = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        &scalar,
        &crate::grant_tests::admin_point(),
        &rng,
    )
    .unwrap();
    let mut signature = key.sign(&rng, &material).unwrap().as_ref().to_vec();
    if case == 13 {
        signature[10] ^= 1;
    }
    let mut proof = Approval {
        version: 1,
        cose: B64.encode(signed(grant, kid)),
        credential_id: B64.encode([7; 32]),
        authenticator_data: B64.encode(auth),
        client_data_json: B64.encode(client),
        signature: B64.encode(signature),
    };
    if case == 14 {
        proof.version = 2;
    }
    if case == 15 {
        proof.credential_id = B64.encode([6; 32]);
    }
    if case == 16 {
        proof.credential_id.push('=');
    }
    serde_json::to_vec(&proof).unwrap()
}

#[test]
fn browser_approval_no_native_downgrade_001() {
    let mut gate = crate::grant_tests::browser_test_gate();
    let profile = BrowserProfile::enrolled(
        "https://desk.invalid",
        "desk.invalid",
        vec![7; 32],
        admin(),
        false,
        false,
    )
    .unwrap();
    gate.require_browser(profile).unwrap();
    gate.browser_attempt([8; 32], [9; 32]).unwrap();
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    assert!(gate.accept(&signed(&grant, gate.issuer_kid())).is_err());
    assert_eq!(
        gate.browser_attempt([10; 32], [9; 32]),
        Err(GrantError::Busy)
    );
}

#[test]
fn browser_approval_signed_assertion_bound_fields_002() {
    for case in 0..17 {
        let mut gate = crate::grant_tests::browser_test_gate();
        let profile = profile();
        gate.require_browser(profile.clone()).unwrap();
        gate.browser_attempt([8; 32], [9; 32]).unwrap();
        let grant = gate
            .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
            .unwrap();
        let bytes = proof(&profile, &grant, gate.issuer_kid(), case);
        if case == 0 {
            assert_eq!(gate.accept(&bytes).unwrap().expires_host_ms(), 5000);
            assert_eq!(gate.accept(&bytes).unwrap().expires_host_ms(), 5000);
            let distinct = proof(&profile, &grant, gate.issuer_kid(), 0);
            assert!(
                gate.accept(&distinct).is_err(),
                "different assertion cannot replay accepted grant"
            );
            gate.browser_attempt([10; 32], [9; 32]).unwrap();
            assert!(
                gate.accept(&bytes).is_err(),
                "new management nonce cannot reuse old approval"
            );
        } else {
            assert!(gate.accept(&bytes).is_err(), "case {case}");
        }
    }
}

#[test]
fn browser_approval_full_canonical_binding_expiry_revoke_003() {
    let mut gate = crate::grant_tests::browser_test_gate();
    let profile = profile();
    gate.require_browser(profile.clone()).unwrap();
    gate.browser_attempt([8; 32], [9; 32]).unwrap();
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let kid = gate.issuer_kid();
    let challenge = profile.challenge(&grant, kid, ([8; 32], [9; 32])).unwrap();
    for case in 0..8 {
        let mut altered = grant.clone();
        match case {
            0 => altered.scope.run.push('x'),
            1 => altered.role = DataRole::Observer,
            2 => altered.expires_host_ms = "5001".into(),
            3 => altered.principal_id = "06".repeat(32),
            4 => altered.context.policy_epoch = "8".into(),
            5 => altered.context.client_endpoint_id = "07".repeat(32),
            6 => altered.challenge = "08".repeat(32),
            _ => altered.grant_id = "09".repeat(32),
        }
        assert_ne!(
            profile
                .challenge(&altered, kid, ([8; 32], [9; 32]))
                .unwrap(),
            challenge
        );
    }
    assert_ne!(
        profile
            .challenge(&grant, [10; 32], ([8; 32], [9; 32]))
            .unwrap(),
        challenge
    );
    assert_ne!(
        profile.challenge(&grant, kid, ([8; 32], [10; 32])).unwrap(),
        challenge
    );
    let bytes = proof(&profile, &grant, kid, 0);
    let accepted = gate.accept(&bytes).unwrap();
    gate.revoke();
    assert!(accepted.check().is_err());
    assert!(gate.accept(&bytes).is_err());
}

#[test]
fn browser_approval_json_ambiguity_bounds_expiry_005() {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    let now = Arc::new(AtomicU64::new(1000));
    let mut gate = crate::grant_tests::gate(now.clone());
    let profile = profile();
    gate.require_browser(profile.clone()).unwrap();
    gate.browser_attempt([8; 32], [9; 32]).unwrap();
    let grant = gate
        .challenge(scope(), DataRole::Controller, Duration::from_secs(4))
        .unwrap();
    let kid = gate.issuer_kid();
    let bytes = proof(&profile, &grant, kid, 0);
    for field in ["type", "challenge", "origin", "crossOrigin", "unknown"] {
        let mut p: Approval = serde_json::from_slice(&bytes).unwrap();
        let mut client = String::from_utf8(B64.decode(&p.client_data_json).unwrap()).unwrap();
        client.pop();
        let value = match field {
            "type" => "\"webauthn.get\"",
            "origin" => "\"https://desk.invalid\"",
            "crossOrigin" => "false",
            _ => "\"x\"",
        };
        client.push_str(&format!(",\"{field}\":{value}}}"));
        p.client_data_json = B64.encode(client);
        assert_eq!(
            profile.verify(
                &serde_json::to_vec(&p).unwrap(),
                &grant,
                &admin(),
                kid,
                ([8; 32], [9; 32])
            ),
            Err(GrantError::Encoding),
            "{field}"
        );
    }
    for extra in [",\"version\":1}", ",\"unknown\":1}"] {
        let mut p = bytes.clone();
        p.pop();
        p.extend_from_slice(extra.as_bytes());
        assert_eq!(
            profile.verify(&p, &grant, &admin(), kid, ([8; 32], [9; 32])),
            Err(GrantError::Encoding)
        );
    }
    assert_eq!(
        profile.verify(
            &vec![b' '; 16385],
            &grant,
            &admin(),
            kid,
            ([8; 32], [9; 32])
        ),
        Err(GrantError::Bounds)
    );
    now.store(5000, Ordering::Release);
    assert!(matches!(gate.accept(&bytes), Err(GrantError::Expired)));
}
