use crate::cli::types::{LaunchRequest, NativeSessionRef, WireBytes, WireU64};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedValue {
    name: String,
    value: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireGoldens {
    valid_u64: Vec<String>,
    invalid_u64: Vec<String>,
    valid_launch_requests: Vec<NamedValue>,
    native_session_refs: Vec<Value>,
}

fn goldens() -> WireGoldens {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .join("tests/fixtures/native-cli/wire-goldens.json");
    let bytes = std::fs::read(path).expect("wire goldens");
    serde_json::from_slice(&bytes).expect("valid wire goldens")
}

#[test]
fn D05_Wire_U64CanonicalBoundaries_001() {
    let fixture = goldens();
    for value in fixture.valid_u64 {
        let parsed = WireU64::parse(&value).expect("valid u64 string");
        assert_eq!(parsed.to_string(), value);
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(value));
    }
    for value in fixture.invalid_u64 {
        let error = WireU64::parse(&value).expect_err("invalid u64 string");
        assert_eq!(error.code, "INVALID_REQUEST");
        assert_eq!(error.field.as_deref(), Some("u64"));
        assert_eq!(error.to_string(), "INVALID_REQUEST:u64");
    }
}

#[test]
fn D05_Wire_LaunchGoldensRoundTrip_002() {
    for golden in goldens().valid_launch_requests {
        let request: LaunchRequest = serde_json::from_value(golden.value.clone())
            .unwrap_or_else(|error| panic!("{} must deserialize: {error}", golden.name));
        request
            .validate()
            .unwrap_or_else(|error| panic!("{} must validate: {error}", golden.name));
        assert_eq!(
            serde_json::to_value(request).expect("serialize launch request"),
            golden.value,
            "{}",
            golden.name
        );
    }
}

#[test]
fn D05_Wire_RejectsUnknownCliAndAction_003() {
    let mut cli = goldens().valid_launch_requests[0].value.clone();
    cli["cli"] = json!("Claude");
    assert!(serde_json::from_value::<LaunchRequest>(cli).is_err());

    let mut action = goldens().valid_launch_requests[0].value.clone();
    action["action"] = json!({"kind": "continue-last"});
    assert!(serde_json::from_value::<LaunchRequest>(action).is_err());
}

#[test]
fn D05_Wire_RejectsInvalidGenerationDimensionsAndNul_004() {
    for generation in [json!(-1), json!(1.5), json!(4_294_967_296_u64)] {
        let mut value = goldens().valid_launch_requests[0].value.clone();
        value["generation"] = generation;
        assert!(serde_json::from_value::<LaunchRequest>(value).is_err());
    }

    for (field, number) in [
        ("cols", json!(0)),
        ("cols", json!(65_536)),
        ("rows", json!(1.5)),
    ] {
        let mut value = goldens().valid_launch_requests[0].value.clone();
        value[field] = number;
        let request = serde_json::from_value::<LaunchRequest>(value);
        if let Ok(request) = request {
            let error = request.validate().expect_err("invalid dimension");
            assert_eq!(error.code, "INVALID_REQUEST");
            assert_eq!(error.field.as_deref(), Some(field));
        }
    }

    let mut value = goldens().valid_launch_requests[0].value.clone();
    value["launchCwd"] = json!("/repo/fixture-secret\u{0}must-not-appear");
    let request: LaunchRequest = serde_json::from_value(value).expect("wire shape");
    let error = request.validate().expect_err("NUL must be rejected");
    assert_eq!(error.code, "INVALID_REQUEST");
    assert_eq!(error.field.as_deref(), Some("launchCwd"));
    assert!(!error.to_string().contains("fixture-secret"));
}

#[test]
fn D05_Wire_RawRejectsExtraArgs_005() {
    let mut value = goldens().valid_launch_requests[3].value.clone();
    value["extraArgs"] = json!(["--model", "fixture"]);
    let request: LaunchRequest = serde_json::from_value(value).expect("wire shape");
    let error = request.validate().expect_err("raw extraArgs must fail");
    assert_eq!(error.code, "INVALID_REQUEST");
    assert_eq!(error.field.as_deref(), Some("extraArgs"));
}

#[test]
fn D05_Identity_CliAndRootProduceDifferentKeys_006() {
    let references: Vec<NativeSessionRef> = goldens()
        .native_session_refs
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("native session ref"))
        .collect();
    let keys: HashSet<String> = references
        .iter()
        .map(NativeSessionRef::stable_key)
        .collect();
    assert_eq!(keys.len(), references.len());
    assert_eq!(
        references[0].stable_key(),
        serde_json::to_string(&[
            references[0].host_id.as_str(),
            references[0].cli.as_str(),
            references[0].source_root_key.as_str(),
            references[0].native_session_id.as_str(),
        ])
        .unwrap()
    );
}

#[test]
fn D05_Wire_BytesAreStrictOctets_007() {
    let bytes: WireBytes = serde_json::from_value(json!([0, 1, 127, 128, 255])).unwrap();
    assert_eq!(bytes.as_slice(), &[0, 1, 127, 128, 255]);
    assert_eq!(
        serde_json::to_value(bytes).unwrap(),
        json!([0, 1, 127, 128, 255])
    );

    for invalid in [json!([-1]), json!([256]), json!([1.5]), json!(["1"])] {
        assert!(serde_json::from_value::<WireBytes>(invalid).is_err());
    }
}
