use cc_desk_remote_record::control_io::{decode, encode};
#[test]
#[allow(non_snake_case)]
fn ControlLease_StrictWireShapesAndIndependentSequences_005() {
    for value in [
        serde_json::json!({"message":"LeaseState","profile":1,"request":"7"}),
        serde_json::json!({"message":"LeaseStateReply","profile":1,"request":"7","lease":"08".repeat(16),"fence":"4","notAfterHostMs":"16000","nextRenewalSequence":"1"}),
        serde_json::json!({"message":"LeaseRenew","profile":1,"request":"8","lease":"08".repeat(16),"fence":"4","renewalSequence":"1"}),
        serde_json::json!({"message":"LeaseRenewed","profile":1,"request":"8","lease":"08".repeat(16),"fence":"4","renewalSequence":"1","notAfterHostMs":"20000"}),
    ] {
        let bytes = serde_json::to_vec(&value).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&encode(&decoded).unwrap()).unwrap(),
            value
        );
        for bad in ["0", "01", "18446744073709551616"] {
            let mut invalid = value.clone();
            invalid["request"] = bad.into();
            assert!(decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        let mut invalid = value.clone();
        invalid["target"] = "other".into();
        assert!(decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        let duplicate = String::from_utf8(bytes).unwrap().replacen(
            "\"profile\":1",
            "\"profile\":1,\"profile\":1",
            1,
        );
        assert!(decode(duplicate.as_bytes()).is_err());
    }
}
