use cc_desk_remote_record::input::*;
#[test]
fn input_exact_encoding_bounds_and_replay_identity() {
    let command = Input {
        operation: [7; 16],
        sequence: 1,
        window: [8; 32],
        not_after_host_ms: 6000,
        bytes: b"PUBLIC_TEST_ONLY\r".to_vec(),
    };
    let bytes = encode(&command).unwrap();
    assert_eq!(decode(&bytes).unwrap(), command);
    assert!(!format!("{command:?}").contains("PUBLIC_TEST_ONLY"));
    for n in 0..bytes.len() {
        assert!(decode(&bytes[..n]).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode(&extra).is_err());
    let text = String::from_utf8(bytes.clone()).unwrap();
    for bad in [
        text.replace("\"profile\":1", "\"profile\":2"),
        text.replace("\"sequence\":\"1\"", "\"sequence\":\"01\""),
        text.replace(
            "\"notAfterHostMs\":\"6000\"",
            "\"notAfterHostMs\":\"+6000\"",
        ),
        text.replacen('{', "{\"profile\":1,", 1),
        text.replacen('{', "{\"unknown\":0,", 1),
    ] {
        assert!(decode(bad.as_bytes()).is_err());
    }
    let mut maximum = command.clone();
    maximum.bytes = vec![0; MAX_BYTES];
    assert!(encode(&maximum).unwrap().len() + 114 <= 16384);
    assert_eq!(decode(&encode(&maximum).unwrap()).unwrap(), maximum);
    maximum.bytes.push(0);
    assert!(encode(&maximum).is_err());
    let mut bad = command;
    bad.sequence = 0;
    assert!(encode(&bad).is_err());
}
