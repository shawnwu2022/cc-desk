#![allow(non_snake_case)]
use crate::{P256Spki, browser_registration::*, cancellation};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use coset::cbor::value::Value as Cbor;
use ring::{digest, signature};
use serde_json::{Value, json};
use std::time::Duration;

const POINTS: [&str; 4] = [
    "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    "047cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1",
    "045ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd6c8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032",
    "04e2534a3532d08fbba02dde659ee62bd0031fe2db785596ef509302446b030852e0f1575a4c633cc719dfee5fda862d764efc96c3f30ee0055c42c23f184ed8c6",
];
fn point(n: usize) -> Vec<u8> {
    crate::grant_tests::hex(POINTS[n - 1])
}
fn key(n: usize) -> P256Spki {
    let mut b = crate::grant_tests::hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    b.extend(point(n));
    P256Spki::new(&b).unwrap()
}
fn context() -> BrowserRegistrationContext {
    BrowserRegistrationContext {
        origin: "https://desk.invalid".into(),
        rp_id: "desk.invalid".into(),
        user_handle: vec![7; 32],
        owner_binding: [8; 32],
        tls_key: key(1),
        grant_key: key(2),
        relay_key: key(3),
        allow_backup_eligible: false,
        allow_backed_up: false,
    }
}
fn sign(n: usize, bytes: &[u8], asn1: bool) -> Vec<u8> {
    // 固定公开 TEST_ONLY scalar，不用于真实登记。
    let mut scalar = [0; 32];
    scalar[31] = n as u8;
    let rng = ring::rand::SystemRandom::new();
    let algorithm = if asn1 {
        &signature::ECDSA_P256_SHA256_ASN1_SIGNING
    } else {
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING
    };
    signature::EcdsaKeyPair::from_private_key_and_public_key(algorithm, &scalar, &point(n), &rng)
        .unwrap()
        .sign(&rng, bytes)
        .unwrap()
        .as_ref()
        .to_vec()
}
fn cbor(v: Cbor) -> Vec<u8> {
    let mut out = vec![];
    coset::cbor::ser::into_writer(&v, &mut out).unwrap();
    out
}
fn create_proof(request: &[u8], case: usize) -> Vec<u8> {
    let r: Value = serde_json::from_slice(request).unwrap();
    let challenge = B64.decode(r["challenge"].as_str().unwrap()).unwrap();
    let mut client = json!({"type":"webauthn.create","origin":"https://desk.invalid","challenge":r["challenge"],"crossOrigin":false});
    match case {
        1 => client["challenge"] = B64.encode([0; 32]).into(),
        2 => client["origin"] = "https://evil.invalid".into(),
        3 => client["crossOrigin"] = true.into(),
        4 => client["topOrigin"] = "https://desk.invalid".into(),
        24 => client["topOrigin"] = Value::Null,
        _ => {}
    }
    let p = point(4);
    let mut cose = vec![
        (Cbor::Integer(1.into()), Cbor::Integer(2.into())),
        (Cbor::Integer(3.into()), Cbor::Integer((-7).into())),
        (Cbor::Integer((-1).into()), Cbor::Integer(1.into())),
        (Cbor::Integer((-2).into()), Cbor::Bytes(p[1..33].into())),
        (Cbor::Integer((-3).into()), Cbor::Bytes(p[33..].into())),
    ];
    if case == 10 {
        cose.push(cose[0].clone());
    }
    if case == 29 {
        cose[4] = cose[0].clone();
    }
    if case == 11 {
        cose[1].1 = Cbor::Integer((-257).into());
    }
    let mut auth = digest::digest(&digest::SHA256, b"desk.invalid")
        .as_ref()
        .to_vec();
    auth.push(match case {
        5 => 0x41,
        6 => 0xc5,
        7 => 0x4d,
        8 => 0x55,
        22 => 0x44,
        23 => 0x47,
        27 => 0x5d,
        _ => 0x45,
    });
    auth.extend([0; 4]);
    auth.extend([0; 16]);
    auth.extend([0, 32]);
    auth.extend([7; 32]);
    auth.extend(cbor(Cbor::Map(cose)));
    if case == 9 {
        auth[0] ^= 1;
    }
    if case == 12 {
        auth.push(0);
    }
    let mut att = vec![
        (
            Cbor::Text("fmt".into()),
            Cbor::Text(if case == 13 { "packed" } else { "none" }.into()),
        ),
        (Cbor::Text("authData".into()), Cbor::Bytes(auth)),
        (Cbor::Text("attStmt".into()), Cbor::Map(vec![])),
    ];
    if case == 14 {
        att.push(att[0].clone());
    }
    if case == 30 {
        att[2] = att[0].clone();
    }
    if case == 31 {
        let mut nested = Cbor::Map(vec![]);
        for _ in 0..10 {
            nested = Cbor::Array(vec![nested]);
        }
        att[2].1 = nested;
    }
    if case == 15 {
        att[2].1 = Cbor::Map(vec![(Cbor::Integer(1.into()), Cbor::Integer(1.into()))]);
    }
    let mut client = serde_json::to_vec(&client).unwrap();
    if case == 25 {
        client.splice(1..1, b"\"type\":\"webauthn.create\",".iter().copied());
    }
    let mut att = cbor(Cbor::Map(att));
    if case == 20 {
        att.pop();
    }
    if case == 21 {
        att.push(0);
    }
    let mut h = digest::Context::new(&digest::SHA256);
    h.update(b"CC-DESK-REGISTRATION-CREATE-v1\0");
    for p in [&challenge[..], &[7; 32], &client, &att] {
        h.update(&(p.len() as u32).to_be_bytes());
        h.update(p)
    }
    let transcript = h.finish();
    let proof = |purpose: &str, n| {
        let mut message = format!("CC-DESK-REGISTRATION-{purpose}-POSSESSION-v1\0").into_bytes();
        message.extend(transcript.as_ref());
        B64.encode(sign(n, &message, false))
    };
    let mut v = json!({"version":1,"credentialId":B64.encode([7;32]),"clientDataJson":B64.encode(client),"attestationObject":B64.encode(att),"tlsSignature":proof("tls",1),"grantSignature":proof("grant",2),"relaySignature":proof("relay",3)});
    if case == 16 {
        v["credentialId"] = B64.encode([9; 32]).into();
    }
    if case == 17 {
        v["tlsSignature"] = v["grantSignature"].clone();
    }
    if case == 18 {
        v["relaySignature"] = B64.encode([0; 64]).into();
    }
    if case == 28 {
        v["tlsSignature"] = proof("grant", 1).into();
    }
    if case == 19 {
        v["unexpected"] = true.into();
    }
    let mut out = serde_json::to_vec(&v).unwrap();
    if case == 26 {
        out.splice(1..1, b"\"version\":1,".iter().copied());
    }
    out
}
fn assertion(request: &[u8], case: usize) -> Vec<u8> {
    let r: Value = serde_json::from_slice(request).unwrap();
    let mut client = json!({"type":"webauthn.get","origin":"https://desk.invalid","challenge":r["challenge"],"crossOrigin":false});
    if case == 1 {
        client["challenge"] = B64.encode([0; 32]).into();
    }
    if case == 2 {
        client["type"] = "webauthn.create".into();
    }
    if case == 7 {
        client["origin"] = "https://evil.invalid".into();
    }
    if case == 8 {
        client["crossOrigin"] = true.into();
    }
    if case == 9 {
        client["topOrigin"] = Value::Null;
    }
    let client = serde_json::to_vec(&client).unwrap();
    let mut auth = digest::digest(&digest::SHA256, b"desk.invalid")
        .as_ref()
        .to_vec();
    auth.push(match case {
        3 => 1,
        10 => 13,
        11 => 133,
        12 => 69,
        27 => 29,
        _ => 5,
    });
    auth.extend([0; 4]);
    if case == 13 {
        auth[0] ^= 1;
    }
    let mut message = auth.clone();
    message.extend(digest::digest(&digest::SHA256, &client).as_ref());
    let mut v = json!({"version":1,"credentialId":B64.encode([7;32]),"authenticatorData":B64.encode(auth),"clientDataJson":B64.encode(client),"signature":B64.encode(sign(if case==4{3}else{4},&message,true)),"userHandle":null});
    if case == 5 {
        v["userHandle"] = B64.encode([9; 32]).into();
    }
    if case == 6 {
        v["credentialId"] = B64.encode([9; 32]).into();
    }
    serde_json::to_vec(&v).unwrap()
}

// 两阶段真实密码学只产生待批准证据，不产生授权对象。
#[test]
fn Registration_TwoStageEvidence_001() {
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 0);
    let result = p.verify_assertion(&bytes).unwrap();
    let description: Value = serde_json::from_slice(&result.description().unwrap()).unwrap();
    assert_eq!(description["approval"], "required");
    assert_eq!(description["credentialId"], B64.encode([7; 32]));
}

// 拒绝注册字段、算法、CBOR、绑定密钥和备份策略的具体变化。
#[test]
fn Registration_RejectCreateChanges_002() {
    for case in 1..=31 {
        let (_h, t) = cancellation();
        let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
        let bytes = create_proof(&a.request().unwrap(), case);
        assert!(a.verify_create(&bytes).is_err(), "create mutation {case}");
    }
}

// 缺失原凭证持有证明不得产生待批准证据。
#[test]
fn Registration_RejectAssertion_003() {
    for case in 1..=13 {
        let (_h, t) = cancellation();
        let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
        let bytes = create_proof(&a.request().unwrap(), 0);
        let p = a.verify_create(&bytes).unwrap();
        let bytes = assertion(&p.request().unwrap(), case);
        assert!(
            p.verify_assertion(&bytes).is_err(),
            "assertion mutation {case}"
        );
    }
}

// 备份策略必须由原请求显式允许，并在两个阶段保持同一 BE。
#[test]
fn Registration_BackupPolicy_007() {
    let (_h, t) = cancellation();
    let mut c = context();
    c.allow_backup_eligible = true;
    c.allow_backed_up = true;
    let a = BrowserRegistrationAttempt::new(c, t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 27);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 27);
    let e = p.verify_assertion(&bytes).unwrap();
    let v: Value = serde_json::from_slice(&e.description().unwrap()).unwrap();
    assert_eq!(v["backupEligible"], true);
    assert_eq!(v["allowBackedUp"], true);
}

// 空绑定、重复用途密钥、无效预算和错误 origin 均不可发请求。
#[test]
fn Registration_RejectContext_008() {
    for case in 0..8 {
        let (_h, t) = cancellation();
        let mut c = context();
        let mut budget = Duration::from_secs(30);
        match case {
            0 => c.owner_binding = [0; 32],
            1 => c.user_handle.clear(),
            2 => c.rp_id = "evil.invalid".into(),
            3 => c.origin = "http://desk.invalid".into(),
            4 => c.tls_key = key(2),
            5 => budget = Duration::ZERO,
            6 => budget = Duration::from_secs(121),
            7 => c.allow_backed_up = true,
            _ => unreachable!(),
        };
        assert!(
            BrowserRegistrationAttempt::new(c, t, budget).is_err(),
            "context mutation {case}"
        );
    }
}

// 新尝试及另一原 owner 的请求不能复用旧响应。
#[test]
fn Registration_RejectReplay_004() {
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t.clone(), Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let b = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    assert_ne!(a.request().unwrap(), b.request().unwrap());
    assert!(b.verify_create(&bytes).is_err());
}

// 注册后撤销沿用原 token，不在确认阶段重新授权。
#[test]
fn Registration_CancelBetweenStages_005() {
    let (h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 0);
    h.cancel();
    assert!(p.verify_assertion(&bytes).is_err());
}

// 两阶段使用同一单调截止，确认阶段不能续期。
#[tokio::test(start_paused = true)]
async fn Registration_OriginalDeadline_006() {
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 0);
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(p.verify_assertion(&bytes).is_err());
}

// 浏览器组合验收仅经显式 TEST_ONLY 原测试入口交换固定合成资料。
// 已验证 create 状态之间仍不能交换同一凭证的 assertion。
#[test]
fn Registration_CrossStageReplay_009() {
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t.clone(), Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let a = a.verify_create(&bytes).unwrap();
    let b = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&b.request().unwrap(), 0);
    let b = b.verify_create(&bytes).unwrap();
    let proof = assertion(&a.request().unwrap(), 0);
    assert!(b.verify_assertion(&proof).is_err());
}

// 完成密码学验证后撤销仍使待批准描述不可用。
#[test]
fn Registration_EvidenceStillLive_010() {
    let (h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 0);
    let evidence = p.verify_assertion(&bytes).unwrap();
    h.cancel();
    assert!(evidence.description().is_err());
}

// 浏览器组合验收仅经显式 TEST_ONLY 原测试入口交换固定合成资料。
#[test]
#[ignore]
fn Registration_BrowserOracle_900() {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut input = stdin.lock().lines();
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(60)).unwrap();
    let emit = |v: &[u8]| {
        println!("CC_DESK_REGISTRATION_ORACLE:{}", String::from_utf8_lossy(v));
        std::io::stdout().flush().unwrap();
    };
    emit(&a.request().unwrap());
    let p = a
        .verify_create(input.next().unwrap().unwrap().as_bytes())
        .unwrap();
    emit(&p.request().unwrap());
    let v = p
        .verify_assertion(input.next().unwrap().unwrap().as_bytes())
        .unwrap();
    emit(&v.description().unwrap());
}

// 原验证对象只匹配同一 owner、TLS/issuer 身份和完整 Browser 策略。
#[test]
fn Registration_MatchesCandidate_011() {
    let (_h, t) = cancellation();
    let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
    let bytes = create_proof(&a.request().unwrap(), 0);
    let p = a.verify_create(&bytes).unwrap();
    let bytes = assertion(&p.request().unwrap(), 0);
    let e = p.verify_assertion(&bytes).unwrap();
    let browser = crate::BrowserProfile::enrolled(
        "https://desk.invalid",
        "desk.invalid",
        vec![7; 32],
        key(4),
        false,
        false,
    )
    .unwrap();
    let phone = crate::PeerPin::new(crate::Role::Client, key(1));
    assert!(
        e.matches_host_candidate([8; 32], &phone, &key(2), &browser)
            .unwrap()
    );
    assert!(
        !e.matches_host_candidate([9; 32], &phone, &key(2), &browser)
            .unwrap()
    );
    assert!(
        !e.matches_host_candidate(
            [8; 32],
            &crate::PeerPin::new(crate::Role::Client, key(2)),
            &key(2),
            &browser
        )
        .unwrap()
    );
    assert!(
        !e.matches_host_candidate([8; 32], &phone, &key(3), &browser)
            .unwrap()
    );
    let changed = crate::BrowserProfile::enrolled(
        "https://desk.invalid",
        "desk.invalid",
        vec![9; 32],
        key(4),
        false,
        false,
    )
    .unwrap();
    assert!(
        !e.matches_host_candidate([8; 32], &phone, &key(2), &changed)
            .unwrap()
    );
    for (origin, rp, k, be, bs) in [
        ("https://sub.desk.invalid", "desk.invalid", 4, false, false),
        ("https://desk.invalid", "invalid", 4, false, false),
        ("https://desk.invalid", "desk.invalid", 3, false, false),
        ("https://desk.invalid", "desk.invalid", 4, true, false),
        ("https://desk.invalid", "desk.invalid", 4, true, true),
    ] {
        let changed =
            crate::BrowserProfile::enrolled(origin, rp, vec![7; 32], key(k), be, bs).unwrap();
        assert!(
            !e.matches_host_candidate([8; 32], &phone, &key(2), &changed)
                .unwrap()
        );
    }
}

#[tokio::test(start_paused = true)]
async fn Registration_MatchRetainsOriginalLifetime_012() {
    for cancelled in [true, false] {
        let (h, t) = cancellation();
        let a = BrowserRegistrationAttempt::new(context(), t, Duration::from_secs(30)).unwrap();
        let created = create_proof(&a.request().unwrap(), 0);
        let p = a.verify_create(&created).unwrap();
        let asserted = assertion(&p.request().unwrap(), 0);
        let e = p.verify_assertion(&asserted).unwrap();
        if cancelled {
            h.cancel();
        } else {
            tokio::time::advance(Duration::from_secs(30)).await;
        }
        let browser = crate::BrowserProfile::enrolled(
            "https://desk.invalid",
            "desk.invalid",
            vec![7; 32],
            key(4),
            false,
            false,
        )
        .unwrap();
        assert!(
            e.matches_host_candidate(
                [8; 32],
                &crate::PeerPin::new(crate::Role::Client, key(1)),
                &key(2),
                &browser
            )
            .is_err()
        );
    }
}
