//! Pure comparison/build-binding tests, never a native roundtrip PASS.
use crate::version_history::acceptance::validate_target_bytes;
use serde_json::json;

fn bound() -> serde_json::Value {
    json!({"schema":1,"enabled":true,"baseHead":"9c981a5093a80b947817af8eebe4855293690185",
        "buildId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","runId":"12345678-1234-4234-8234-123456789abc",
        "scenario":"success","targetSid":"S-1-5-21-1-2-3-1001","profileDirectory":"C:\\Users\\Disposable",
        "installDirectory":"C:\\Users\\Disposable\\AppData\\Local\\CC Desk","evidenceDirectory":"C:\\RoundtripEvidence\\run"})
}
#[test]
fn HistoryRoundtripBinding_DeniesMissingDisabledAndMalformed() {
    assert!(validate_target_bytes("").is_err());
    assert!(validate_target_bytes("{}").is_err());
    let input = bound();
    assert!(validate_target_bytes(&input.to_string()).is_ok());
    for (field, value) in [
        ("enabled", json!(false)),
        ("schema", json!(2)),
        ("baseHead", json!("wrong")),
        ("buildId", json!("9c981a5093a80b947817af8eebe4855293690185")),
        ("buildId", json!("local")),
        ("runId", json!("")),
        ("scenario", json!("other")),
        ("targetSid", json!("S-1-5-18")),
        ("profileDirectory", json!("relative")),
        ("installDirectory", json!("C:\\safe\\..\\real")),
        ("evidenceDirectory", json!("C:\\safe\\.")),
        ("evidenceDirectory", json!("\\\\server\\share")),
    ] {
        let mut invalid = input.clone();
        invalid[field] = value;
        assert!(
            validate_target_bytes(&invalid.to_string()).is_err(),
            "accepted {field}"
        );
    }
    let mut extra = input.clone();
    extra["version"] = json!("0.17.6");
    assert!(validate_target_bytes(&extra.to_string()).is_err());
    let mut failure = input;
    failure["scenario"] = json!("before-installer-resume");
    assert!(validate_target_bytes(&failure.to_string()).is_ok());
    assert!(validate_target_bytes(include_str!(
        "../../../tests/fixtures/version-history-roundtrip/target.json"
    ))
    .is_err());
}

#[test]
fn HistoryRoundtripBinding_OnlyMeasured0177() {
    use crate::version_history::acceptance::require_payload;
    let digest = "e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac";
    assert!(require_payload("0.17.7", digest, 4_966_193).is_ok());
    assert!(require_payload("0.17.6", digest, 4_966_193).is_err());
    assert!(require_payload("0.18.0", digest, 4_966_193).is_err());
    assert!(require_payload("0.17.7", &"0".repeat(64), 4_966_193).is_err());
    assert!(require_payload("0.17.7", digest, 4_966_192).is_err());
}

#[cfg(windows)]
#[test]
fn HistoryRoundtripReport_BundleLogicalEqualityCoversEveryEntry() {
    use crate::version_history::windows::context::InstalledBundleManifest;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/version-history-payload/v0.17.7-measured.json"
    ))
    .unwrap();
    let input = json!({"schema":1,"original_image_name":"Old Desk.exe","fenced_image_location":"held-source",
        "tree":{"schema":1,"location_identity":"a".repeat(64),"entries":fixture["source"]}});
    let digest = |value: &serde_json::Value| {
        serde_json::from_value::<InstalledBundleManifest>(value.clone())
            .unwrap()
            .logical_digest()
            .unwrap()
    };
    let expected = digest(&input);
    // Copies intentionally have different file IDs. They do not alter the
    // existing logical digest that the actual final observer reports.
    let mut copied = input.clone();
    copied["tree"]["entries"][1]["metadata"]["object_identity"] = json!("f".repeat(64));
    assert_eq!(digest(&copied), expected);
    for index in 0..input["tree"]["entries"].as_array().unwrap().len() {
        let mut missing = input.clone();
        missing["tree"]["entries"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        assert_ne!(digest(&missing), expected);
    }
    let mut extra = input.clone();
    extra["tree"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(fixture["source"][1].clone());
    assert_ne!(digest(&extra), expected);
    for (path, value) in [
        ("/tree/entries/1/sha256", json!("0".repeat(64))),
        ("/tree/entries/1/metadata/size", json!(1)),
        (
            "/tree/entries/1/metadata/permissions/Windows/attributes",
            json!(2),
        ),
        (
            "/tree/entries/1/metadata/permissions/Windows/descriptor",
            json!([9, 8, 7]),
        ),
    ] {
        let mut changed = input.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert_ne!(digest(&changed), expected, "unchanged digest for {path}");
    }
}

#[cfg(windows)]
#[test]
fn HistoryRoundtripReport_BothShortcutContentPermissionAndAbsenceContracts() {
    use crate::version_history::windows::shortcuts::ShortcutState;
    let present = json!({"Present":{"identity":{"volume":1,"id":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1]},
        "attributes":32,"bytes":[1,2,3],"sha256":"a".repeat(64),"descriptor":[1,0,4,128,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}});
    for _slot in ["Desktop", "StartMenu"] {
        let original: ShortcutState = serde_json::from_value(present.clone()).unwrap();
        assert!(original.matches_restored_content_and_permissions(&original));
        assert!(!ShortcutState::Absent.matches_restored_content_and_permissions(&original));
        assert!(!original.matches_restored_content_and_permissions(&ShortcutState::Absent));
        for (field, value) in [
            ("bytes", json!([9])),
            ("sha256", json!("b".repeat(64))),
            ("attributes", json!(128)),
            ("descriptor", json!([1, 0, 4, 128, 9])),
        ] {
            let mut changed = present.clone();
            changed["Present"][field] = value;
            let changed: ShortcutState = serde_json::from_value(changed).unwrap();
            assert!(
                !changed.matches_restored_content_and_permissions(&original),
                "accepted {field}"
            );
        }
    }
}

#[test]
fn HistoryRoundtripReport_TerminalMarkerCannotBeInferredFromStatus() {
    use crate::version_history::{journal::JournalBinding, maintenance::ActiveContextMarker};
    let binding: JournalBinding = serde_json::from_value(json!({
        "transaction_id":"12345678-1234-4234-8234-123456789abc",
        "source_context":"22345678-1234-4234-8234-123456789abc",
        "target_context":"32345678-1234-4234-8234-123456789abc",
        "user_installation":"a".repeat(64),"source_bundle":"b".repeat(64),
        "target_package":"c".repeat(64),"target_payload":"d".repeat(64),"roots":"e".repeat(64)
    }))
    .unwrap();
    let transition = ActiveContextMarker::transition(binding.clone(), 7, "f".repeat(64)).unwrap();
    assert!(!transition.is_terminal());
    let mut report: serde_json::Value =
        serde_json::from_slice(&transition.encode().unwrap()).unwrap();
    report["state"] = json!("Restored");
    let restored = ActiveContextMarker::decode(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert!(restored.is_terminal());
    assert_eq!(restored.binding(), &binding);
    report["binding"]["transaction_id"] = json!("42345678-1234-4234-8234-123456789abc");
    let foreign = ActiveContextMarker::decode(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_ne!(foreign.binding(), &binding);
    report["binding"]["source_context"] = report["binding"]["target_context"].clone();
    assert!(ActiveContextMarker::decode(&serde_json::to_vec(&report).unwrap()).is_err());
}
