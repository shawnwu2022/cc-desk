use cc_desk_remote_record::lifecycle::*;
fn message(kind: &str, extra: &str) -> String {
    format!(
        r#"{{"message":"{kind}","profile":1,"operation":"{}","sequence":"1","generation":"3","fence":"4","modeEpoch":"0","dimensionEpoch":"0","syncEpoch":"1"{extra}}}"#,
        "06".repeat(16)
    )
}
#[test]
fn lifecycle_schema_strict_commands_and_epochs() {
    for (name, kind) in [
        ("Create", Kind::Create),
        ("Attach", Kind::Attach),
        ("Resume", Kind::Resume),
        ("Close", Kind::Close),
    ] {
        assert_eq!(decode(message(name, "").as_bytes()).unwrap().kind, kind);
    }
    assert_eq!(
        decode(message("Resize", r#", "rows":24,"columns":80"#).as_bytes())
            .unwrap()
            .size,
        Some((24, 80))
    );
    for bad in [
        message("Unknown", ""),
        message("Resize", ""),
        message("Create", r#", "rows":24,"columns":80"#),
        message("Create", r#", "rows":null,"columns":null"#),
        message("Create", "").replace("\"message\":\"Create\"", "\"message\":{\"Create\":null}"),
        message("Close", r#", "unknown":1"#),
        message("Close", r#", "sequence":"2""#),
        message("Close", "").replace("\"fence\":\"4\"", "\"fence\":\"04\""),
        message("Resize", r#", "rows":0,"columns":80"#),
    ] {
        assert!(decode(bad.as_bytes()).is_err());
    }
    assert!(decode(&vec![b' '; 16271]).is_err());
    let r = decode(message("Close", "").as_bytes()).unwrap();
    assert_eq!(decode(&encode(&r).unwrap()).unwrap(), r);
    for status in [
        Status::Accepted,
        Status::Unknown,
        Status::Rejected(Code::Unauthorized),
        Status::Rejected(Code::Stale),
        Status::Rejected(Code::Conflict),
        Status::Rejected(Code::Pending),
        Status::Rejected(Code::Unsupported),
        Status::Rejected(Code::Backend),
    ] {
        let reply = Reply {
            request: r.clone(),
            status,
        };
        let bytes = encode_reply(&reply).unwrap();
        assert_eq!(decode_reply(&bytes).unwrap(), reply);
        let bad = String::from_utf8(bytes)
            .unwrap()
            .replacen('{', "{\"unknown\":1,", 1);
        assert!(decode_reply(bad.as_bytes()).is_err());
        if status == Status::Accepted {
            let bad = String::from_utf8(encode_reply(&reply).unwrap())
                .unwrap()
                .replace("\"status\":\"Accepted\"", "\"status\":{\"Accepted\":null}");
            assert!(decode_reply(bad.as_bytes()).is_err());
        }
    }
}
