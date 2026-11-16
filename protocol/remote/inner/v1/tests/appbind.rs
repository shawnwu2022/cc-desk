use cc_desk_remote_record::appbind::*;
fn oracle() -> serde_json::Value {
    serde_json::from_slice(include_bytes!("../vectors/appbind-independent.json")).unwrap()
}
#[test]
fn minor_one_data_extension_negotiates_without_downgrading_old_binding() {
    let v = oracle();
    let Message::HostOffer(mut h) =
        decode(v["canonicalParts"][0].as_str().unwrap().as_bytes()).unwrap()
    else {
        panic!()
    };
    let Message::ClientOffer(mut c) =
        decode(v["canonicalParts"][1].as_str().unwrap().as_bytes()).unwrap()
    else {
        panic!()
    };
    h.minors = vec![0, 1];
    c.minors = vec![0, 1];
    assert_eq!(negotiate(&h, &c).unwrap().minor, 1);
    c.minors = vec![0];
    assert_eq!(negotiate(&h, &c).unwrap().minor, 0);
    c.minors = vec![2];
    assert!(negotiate(&h, &c).is_err());
}
#[test]
fn appbind_independent_exact_bytes_and_transcript() {
    let v = oracle();
    let h = v["canonicalParts"][0].as_str().unwrap().as_bytes();
    let c = v["canonicalParts"][1].as_str().unwrap().as_bytes();
    let host = match decode(h).unwrap() {
        Message::HostOffer(h) => h,
        _ => panic!("HostOffer"),
    };
    let client = match decode(c).unwrap() {
        Message::ClientOffer(c) => c,
        _ => panic!("ClientOffer"),
    };
    assert_eq!(encode(&Message::HostOffer(host.clone())).unwrap(), h);
    assert_eq!(encode(&Message::ClientOffer(client.clone())).unwrap(), c);
    let selection = negotiate(&host, &client).unwrap();
    let sb = encode_selection(&selection).unwrap();
    assert_eq!(sb, v["canonicalParts"][2].as_str().unwrap().as_bytes());
    assert_eq!([h.len(), c.len(), sb.len()], [606, 722, 88]);
    let expected = v["transcriptHex"].as_str().unwrap();
    let bytes: Vec<u8> = (0..expected.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&expected[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(transcript(&host, &client, &selection).unwrap(), bytes);
}
#[test]
fn appbind_rejects_noncanonical_duplicate_unknown_and_depth() {
    let v = oracle();
    let h = v["canonicalParts"][0].as_str().unwrap();
    for bad in [
        format!(" {h}"),
        h.replacen("\"profile\":1", "\"profile\":1,\"profile\":1", 1),
        h.replacen("\"profile\":1", "\"unexpected\":1,\"profile\":1", 1),
        h.replace("\"hostKeyEpoch\":\"5\"", "\"hostKeyEpoch\":\"05\""),
    ] {
        assert!(decode(bad.as_bytes()).is_err());
    }
    assert_eq!(
        decode(format!("{}0{}", "[".repeat(17), "]".repeat(17)).as_bytes()).err(),
        Some(CodecError::Depth)
    );
    assert_eq!(decode(&vec![b' '; 16385]).err(), Some(CodecError::Length));
}
#[test]
fn appbind_url_uses_restricted_canonical_wrapper() {
    assert_eq!(
        CanonicalOrigin::trusted("wss://RELAY.invalid:443/relay")
            .unwrap()
            .as_str(),
        "wss://relay.invalid/relay"
    );
    for s in [
        "ws://relay.invalid/",
        "wss://u@relay.invalid/",
        "wss://relay.invalid/?q=1",
        "wss://relay.invalid/#f",
        "wss://relay.invalid/%41",
    ] {
        assert!(CanonicalOrigin::trusted(s).is_err());
    }
}
#[test]
fn appbind_raw_url_syntax_cannot_be_erased_by_normalization() {
    for s in [
        "wss://@relay.invalid/",
        "wss://:@relay.invalid/",
        "wss://relay.invalid/a/%2e%2e/b",
        "wss://relay.invalid\\admin",
        "wss://relay.invalid/\tadmin",
        "wss://relay.invalid/\nadmin",
    ] {
        assert!(
            CanonicalOrigin::trusted(s).is_err(),
            "restricted raw URL accepted"
        );
    }
}
#[test]
fn appbind_validates_public_dto_entries_and_profile_capabilities() {
    let v = oracle();
    let host = match decode(v["canonicalParts"][0].as_str().unwrap().as_bytes()).unwrap() {
        Message::HostOffer(h) => h,
        _ => unreachable!(),
    };
    let client = match decode(v["canonicalParts"][1].as_str().unwrap().as_bytes()).unwrap() {
        Message::ClientOffer(c) => c,
        _ => unreachable!(),
    };
    let mut bad = host.clone();
    bad.context.host_key_epoch = "18446744073709551616".into();
    assert!(encode(&Message::HostOffer(bad.clone())).is_err());
    assert!(negotiate(&bad, &client).is_err());
    for caps in [
        vec![],
        vec!["canonical-json".into(), "canonical-json".into()],
        vec!["unknown".into()],
    ] {
        let mut bad = host.clone();
        bad.required_capabilities = caps;
        assert!(negotiate(&bad, &client).is_err());
    }
    for value in [format!("{}\u{e9}", "a".repeat(62)), "A".repeat(64)] {
        assert_eq!(value.len(), 64);
        let mac: Mac = serde_json::from_value(serde_json::Value::String(value)).unwrap();
        let result = std::panic::catch_unwind(|| mac.bytes());
        assert!(matches!(result, Ok(Err(_))));
    }
}
