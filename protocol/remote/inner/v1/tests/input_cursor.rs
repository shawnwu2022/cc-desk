use cc_desk_remote_record::control_io::{decode, encode, Message, Outcome, PendingInput};
#[test]
fn public_rust_encoder_cursor_vectors_for_phone_interoperability() {
    let values = [
        Message::InputCursorState {
            profile: 1,
            request: "1".into(),
            next_sequence: Some("42".into()),
            pending: None,
        },
        Message::InputCursorState {
            profile: 1,
            request: "2".into(),
            next_sequence: None,
            pending: Some(PendingInput {
                operation: "02".repeat(16),
                sequence: "41".into(),
                outcome: Outcome::Unknown,
                proof: None,
            }),
        },
    ];
    let mut bytes = Vec::new();
    for value in values {
        let encoded = encode(&value).unwrap();
        assert_eq!(decode(&encoded).unwrap(), value);
        bytes.extend(encoded);
        bytes.push(b'\n');
    }
    if let Some(path) = std::env::var_os("CC_DESK_TEST_ONLY_CONTROL_JSONL") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(&bytes).unwrap();
    }
}
#[test]
fn input_cursor_is_explicit_strict_and_never_a_window_message() {
    for bytes in [br#"{"message":"InputCursor","profile":1,"request":"1"}"#.as_slice(),br#"{"message":"InputCursorState","profile":1,"request":"1","nextSequence":"2","pending":null}"#,br#"{"message":"InputCursorState","profile":1,"request":"1","nextSequence":null,"pending":{"operation":"01010101010101010101010101010101","sequence":"1","outcome":"Unknown","proof":null}}"#] {
        let m=decode(bytes).expect("new explicit cursor messages"); assert_eq!(encode(&m).unwrap(),bytes);
    }
    for bytes in [br#"{"message":"InputCursor","profile":1,"request":"01"}"#.as_slice(),br#"{"message":"InputCursor","profile":1,"request":"1","device":"x"}"#,br#"{"message":"InputCursorState","profile":1,"request":"1","nextSequence":"2"}"#,br#"{"message":"InputCursorState","profile":1,"request":"1","nextSequence":null,"pending":null}"#,br#"{"message":"InputCursorState","profile":1,"request":"1","nextSequence":"2","pending":{"operation":"01010101010101010101010101010101","sequence":"1","outcome":"Unknown","proof":null}}"#] { assert!(decode(bytes).is_err()); }
}
