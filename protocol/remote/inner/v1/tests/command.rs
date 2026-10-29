use cc_desk_remote_record::command::*;
#[test]
fn routed_ack_rejects_unknown_fields() {
    use cc_desk_remote_record::projection::{encode_ack, Ack, Stamp, Target};
    let ack = Ack {
        profile: 1,
        stamp: Stamp {
            target: Target {
                host: "01".repeat(32),
                session: "02".repeat(16),
                run: "TEST_ONLY".into(),
                generation: "1".into(),
                incarnation: "1".into(),
            },
            binding: "03".repeat(32),
            barrier: "04".repeat(16),
            revision: "1".into(),
            dimension_epoch: "1".into(),
            mode_epoch: "1".into(),
            structure_epoch: "1".into(),
            sync_epoch: "1".into(),
        },
    };
    let bytes = encode_ack(&ack, 1).unwrap();
    assert!(decode(&bytes).is_ok());
    let s = String::from_utf8(bytes)
        .unwrap()
        .replace("\"profile\":1", "\"profile\":1,\"extra\":0");
    assert!(decode(s.as_bytes()).is_err());
}
#[test]
fn route_commands_are_strict_bounded_and_never_imply_authority() {
    assert!(matches!(
        decode(br#"{"message":"Snapshot","profile":1,"request":"1"}"#),
        Ok(Command::Snapshot(1))
    ));
    for bad in [
        br#"{"message":"Snapshot","profile":1,"request":"01"}"#.as_slice(),
        br#"{"message":"Snapshot","profile":1,"request":"1","unknown":0}"#,
        br#"{"message":"Snapshot","profile":1,"request":"1","request":"2"}"#,
        br#"{"message":"Create","profile":1,"request":"1"}"#,
        br#"{"message":"Snapshot","profile":0,"request":"1"}"#,
    ] {
        assert!(decode(bad).is_err());
    }
    assert!(decode(&vec![b' '; 16271]).is_err());
    assert!(matches!(
        decode(br#"{"message":"Disconnect","profile":1,"request":"2"}"#),
        Ok(Command::Disconnect(2))
    ));
}
