use cc_desk_remote_record::appbind::{CanonicalOrigin, Context};
use cc_desk_remote_record::{grant::*, Kind};
fn payload() -> CanonicalGrant {
    CanonicalGrant {
        version: 1,
        context: Context {
            host_endpoint_id: "01".repeat(32),
            client_endpoint_id: "02".repeat(32),
            host_key_epoch: "1".into(),
            client_key_epoch: "2".into(),
            activation: "03".repeat(32),
            incarnation: "4".into(),
            policy_epoch: "5".into(),
            issuer_epoch: "6".into(),
            origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
        },
        principal_id: "04".repeat(32),
        scope: SessionScope {
            session: "05".repeat(16),
            run: "PUBLIC_TEST_RUN".into(),
            generation: "7".into(),
        },
        role: DataRole::Controller,
        challenge: "06".repeat(32),
        grant_id: "07".repeat(32),
        created_host_ms: "1000".into(),
        expires_host_ms: "5000".into(),
    }
}
#[test]
fn grant_payload_is_closed_canonical_and_bounded() {
    let p = payload();
    let bytes = encode(&p).unwrap();
    assert_eq!(decode(&bytes).unwrap(), p);
    for bad in [
        format!(" {}", String::from_utf8(bytes.clone()).unwrap()),
        String::from_utf8(bytes.clone())
            .unwrap()
            .replacen("{", "{\"version\":1,", 1),
        String::from_utf8(bytes.clone())
            .unwrap()
            .replacen("{", "{\"unknown\":1,", 1),
    ] {
        assert!(decode(bad.as_bytes()).is_err());
    }
    for field in 0..5 {
        let mut bad = p.clone();
        match field {
            0 => bad.scope.generation = "01".into(),
            1 => bad.expires_host_ms = "901001".into(),
            2 => bad.expires_host_ms = "1000".into(),
            3 => bad.scope.run = "x".repeat(129),
            _ => bad.principal_id = "0".repeat(64),
        };
        assert!(encode(&bad).is_err());
    }
    assert!(decode(&vec![b'['; 8193]).is_err());
}
#[test]
fn data_frame_exact_identity_sequence_and_complete_budget_are_checked() {
    let header = DataHeader {
        binding: [1; 32],
        grant: [2; 32],
        scope: [3; 32],
        fence: 4,
        sequence: 1,
        kind: Kind::Control,
    };
    let bytes = encode_data(&header, b"public").unwrap();
    let (decoded, body) = decode_data(&bytes).unwrap();
    assert_eq!(decoded, header);
    assert_eq!(body, b"public");
    assert!(encode_data(&header, &vec![0; 16384 - DATA_PREFIX]).is_ok());
    assert!(encode_data(&header, &vec![0; 16385 - DATA_PREFIX]).is_err());
    for position in [0, 32, 64] {
        let mut bad = bytes.clone();
        bad[position..position + 32].fill(0);
        assert!(decode_data(&bad).is_err());
    }
    let mut bad = header.clone();
    bad.sequence = 0;
    assert!(encode_data(&bad, b"a").is_err());
    assert!(decode_data(&bytes[..DATA_PREFIX - 1]).is_err());
    assert!(cc_desk_remote_record::encode(Kind::Data, 0, &bytes).is_err());
    assert!(cc_desk_remote_record::encode(Kind::Data, 1, &bytes).is_ok());
}
