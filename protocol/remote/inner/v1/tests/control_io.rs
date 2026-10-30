use cc_desk_remote_record::control_io::{decode, encode, Disposition, Message, Outcome, Status};

fn receipt(outcome: Outcome, proof: Option<String>) -> Message {
    Message::InputReceipt {
        profile: 1,
        operation: "02".repeat(16),
        sequence: "1".into(),
        outcome,
        proof,
    }
}
#[test]
fn window_and_receipt_roundtrip_preserve_exact_public_vectors() {
    let window = Message::WindowGranted {
        profile: 1,
        request: "7".into(),
        window: "03".repeat(32),
        not_after_host_ms: "6000".into(),
    };
    let bytes = encode(&window).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            r#"{{"message":"WindowGranted","profile":1,"request":"7","window":"{}","notAfterHostMs":"6000"}}"#,
            "03".repeat(32)
        )
    );
    assert_eq!(decode(&bytes).unwrap(), window);
    for value in [
        receipt(Outcome::HostWritten, Some("04".repeat(32))),
        receipt(Outcome::Rejected, Some("05".repeat(32))),
        receipt(Outcome::Partial, None),
        receipt(Outcome::Unknown, None),
    ] {
        assert_eq!(decode(&encode(&value).unwrap()).unwrap(), value);
        assert!(!format!("{value:?}").contains("040404"));
    }
}
#[test]
fn receipt_capability_never_attaches_to_unknown_or_missing_fact() {
    for value in [
        receipt(Outcome::HostWritten, None),
        receipt(Outcome::Rejected, None),
        receipt(Outcome::Partial, Some("03".repeat(32))),
        receipt(Outcome::Unknown, Some("03".repeat(32))),
    ] {
        assert!(encode(&value).is_err());
    }
    let missing = Message::ReceiptStatus {
        profile: 1,
        request: "8".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        status: Status::Missing,
        outcome: None,
        proof: None,
    };
    assert_eq!(decode(&encode(&missing).unwrap()).unwrap(), missing);
    let wrong = Message::ReceiptStatus {
        profile: 1,
        request: "8".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        status: Status::Missing,
        outcome: Some(Outcome::Unknown),
        proof: None,
    };
    assert!(encode(&wrong).is_err());
    let unknown_ack = Message::ReceiptAck {
        profile: 1,
        request: "9".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        outcome: Outcome::Unknown,
        proof: "03".repeat(32),
    };
    assert!(encode(&unknown_ack).is_err());
    let settled = Message::ReceiptSettled {
        profile: 1,
        request: "9".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        outcome: Outcome::HostWritten,
        disposition: Disposition::Advanced,
    };
    assert_eq!(decode(&encode(&settled).unwrap()).unwrap(), settled);
}
#[test]
fn wire_rejects_duplicate_unknown_noncanonical_and_overbudget_fields() {
    for bytes in [
        br#"{"message":"ReceiptQuery","profile":1,"request":"1","request":"2","operation":"02020202020202020202020202020202","sequence":"1"}"#.as_slice(),
        br#"{"message":"ReceiptQuery","profile":1,"request":"01","operation":"02020202020202020202020202020202","sequence":"1"}"#,
        br#"{"message":"InputNotAccepted","profile":1,"operation":"00000000000000000000000000000000","sequence":"1"}"#,
        br#"{"message":"InputNotAccepted","profile":1,"operation":"02020202020202020202020202020202","sequence":"1","extra":0}"#,
        br#"{"message":"InputNotAccepted","profile":0,"operation":"02020202020202020202020202020202","sequence":"1"}"#,
    ] { assert!(decode(bytes).is_err()); }
    assert!(decode(&vec![b' '; 16271]).is_err());
    assert!(decode(&[0xff]).is_err());
    assert!(decode(format!("{}0{}", "[".repeat(17), "]".repeat(17)).as_bytes()).is_err());
}
#[test]
fn enum_maps_and_omitted_nullable_fields_do_not_change_wire_shape() {
    let valid = String::from_utf8(encode(&receipt(Outcome::Unknown, None)).unwrap()).unwrap();
    assert!(decode(
        valid
            .replace("\"Unknown\"", "{\"Unknown\":null}")
            .as_bytes()
    )
    .is_err());
    assert!(decode(valid.replace(",\"proof\":null", "").as_bytes()).is_err());
    let missing = Message::ReceiptStatus {
        profile: 1,
        request: "8".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        status: Status::Missing,
        outcome: None,
        proof: None,
    };
    let valid = String::from_utf8(encode(&missing).unwrap()).unwrap();
    assert!(decode(
        valid
            .replace("\"Missing\"", "{\"Missing\":null}")
            .as_bytes()
    )
    .is_err());
    let settled = Message::ReceiptSettled {
        profile: 1,
        request: "9".into(),
        operation: "02".repeat(16),
        sequence: "1".into(),
        outcome: Outcome::HostWritten,
        disposition: Disposition::Advanced,
    };
    let valid = String::from_utf8(encode(&settled).unwrap()).unwrap();
    assert!(decode(
        valid
            .replace("\"Advanced\"", "{\"Advanced\":null}")
            .as_bytes()
    )
    .is_err());
}
#[test]
fn public_rust_encoder_vectors_for_phone_interoperability() {
    let mut values = vec![
        Message::WindowGranted {
            profile: 1,
            request: "7".into(),
            window: "03".repeat(32),
            not_after_host_ms: "6000".into(),
        },
        receipt(Outcome::HostWritten, Some("04".repeat(32))),
        receipt(Outcome::Rejected, Some("05".repeat(32))),
        receipt(Outcome::Partial, None),
        receipt(Outcome::Unknown, None),
        Message::InputNotAccepted {
            profile: 1,
            operation: "02".repeat(16),
            sequence: "1".into(),
        },
    ];
    for (status, outcome, proof) in [
        (
            Status::Known,
            Some(Outcome::HostWritten),
            Some("04".repeat(32)),
        ),
        (Status::Known, Some(Outcome::Unknown), None),
        (Status::Missing, None, None),
        (Status::TooOld, None, None),
    ] {
        values.push(Message::ReceiptStatus {
            profile: 1,
            request: "8".into(),
            operation: "02".repeat(16),
            sequence: "1".into(),
            status,
            outcome,
            proof,
        });
    }
    for disposition in [
        Disposition::Advanced,
        Disposition::AlreadySettled,
        Disposition::Rejected,
    ] {
        values.push(Message::ReceiptSettled {
            profile: 1,
            request: "9".into(),
            operation: "02".repeat(16),
            sequence: "1".into(),
            outcome: Outcome::HostWritten,
            disposition,
        });
    }
    values.push(Message::ProjectionAcknowledged {
        profile: 1,
        stamp: cc_desk_remote_record::projection::Stamp {
            target: cc_desk_remote_record::projection::Target {
                host: "01".repeat(32),
                session: "02".repeat(16),
                run: "TEST_ONLY_CONTROL".into(),
                generation: "3".into(),
                incarnation: "11".into(),
            },
            binding: "03".repeat(32),
            barrier: "04".repeat(16),
            revision: "1".into(),
            dimension_epoch: "1".into(),
            mode_epoch: "1".into(),
            structure_epoch: "1".into(),
            sync_epoch: "1".into(),
        },
    });
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
