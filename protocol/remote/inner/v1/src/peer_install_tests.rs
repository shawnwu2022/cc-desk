use crate::peer_install::{self as wire, Message};
#[test]
fn peer_install_strict_context_and_fence_codec() {
    let m = Message::Installed {
        nonce: "01".repeat(32),
        binding: "02".repeat(32),
        grant_id: "03".repeat(32),
        scope_digest: "04".repeat(32),
        fence: "9007199254740993".into(),
    };
    let bytes = wire::encode(&m).unwrap();
    assert_eq!(wire::decode(&bytes).unwrap(), m);
    for bytes in [
        br#"{"message":"Installed","fence":"0"}"#.as_slice(),
        br#"{"message":"Request","nonce":"x","extra":1}"#,
        br#"{"message":"Request","message":"Installed"}"#,
    ] {
        assert!(wire::decode(bytes).is_err());
    }
    let valid = String::from_utf8(bytes).unwrap();
    for changed in [
        valid.replace("9007199254740993", "0"),
        valid.replace("9007199254740993", "01"),
        valid.replace("9007199254740993", "18446744073709551616"),
        valid.replacen("0101", "ABAB", 1),
        valid.replacen("{", "{\"extra\":1,", 1),
    ] {
        assert!(wire::decode(changed.as_bytes()).is_err());
    }
    assert!(wire::decode(&vec![b' '; 16385]).is_err());
}
