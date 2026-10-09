use crate::updater_policy::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY: &str = "fixture-public-key";
const URL: &str =
    "https://github.com/shawnwu2022/cc-desk/releases/download/v1.2.3/CC.Desk_1.2.3_x64-setup.exe";
fn release() -> Value {
    json!({"id": 123, "tag_name": "v1.2.3", "target_commitish": SHA, "draft": false, "prerelease": false,
    "published_at": "2026-10-09T14:00:00Z", "html_url": "https://github.com/shawnwu2022/cc-desk/releases/tag/v1.2.3",
    "body": format!("Validation policy: required-checks-and-disclosed-host-unverified-v1\n\nSource: {SHA}\nCI run: 7, attempt: 1\n"),
    "assets": [
      {"id":1,"name":"CC.Desk_1.2.3_x64-setup.exe","browser_download_url":URL,"size":10,"state":"uploaded","digest":format!("sha256:{}", "b".repeat(64))},
      {"id":2,"name":"CC.Desk_1.2.3_x64-setup.exe.sig","browser_download_url":format!("{URL}.sig"),"size":420,"state":"uploaded","digest":format!("sha256:{}", "c".repeat(64))},
      {"id":3,"name":"preserved-old.bin","size":20,"state":"uploaded"}
    ]})
}
fn admission(value: &Value) -> Result<OfficialRelease, UpdateFailure> {
    validate_official_release(value, "1.2.3", "windows-x86_64", URL, &json!({}))
}
#[test]
fn official_release_passes_existing_contract() {
    let proof = admission(&release()).unwrap();
    assert_eq!(proof.release_id, 123);
    assert_eq!(proof.source_sha, SHA);
    assert_eq!(proof.package.size, 10);
}
#[test]
fn draft_prerelease_and_candidate_metadata_never_admit() {
    for field in ["draft", "prerelease"] {
        let mut r = release();
        r[field] = json!(true);
        assert!(admission(&r).is_err());
    }
    for raw in [
        json!({"channel":"candidate"}),
        json!({"channel":"test-only"}),
        json!({"publishable":false}),
        json!({"updaterPublication":false}),
    ] {
        assert!(
            validate_official_release(&release(), "1.2.3", "windows-x86_64", URL, &raw).is_err()
        );
    }
}
#[test]
fn tag_version_body_source_and_published_state_must_match() {
    for (field, value) in [
        ("tag_name", json!("v1.2.2")),
        ("target_commitish", json!("main")),
        ("published_at", Value::Null),
        ("body", json!("candidate source")),
    ] {
        let mut r = release();
        r[field] = value;
        assert!(admission(&r).is_err());
    }
}
#[test]
fn foreign_urls_duplicates_and_missing_digests_are_refused() {
    let mut r = release();
    r["assets"][0]["browser_download_url"] = json!("https://example.com/update.exe");
    assert!(admission(&r).is_err());
    let mut r = release();
    let duplicate = r["assets"][0].clone();
    r["assets"].as_array_mut().unwrap().push(duplicate);
    assert!(admission(&r).is_err());
    let mut r = release();
    r["assets"][0]["digest"] = Value::Null;
    assert!(admission(&r).is_err());
    let mut r = release();
    r["assets"][0]["size"] = json!(MAX_PACKAGE_BYTES + 1);
    assert!(admission(&r).is_err());
}
#[test]
fn ref_is_exact_not_ancestor_or_recent_green() {
    let proof = admission(&release()).unwrap();
    assert!(validate_tag_commit(
        &json!({"ref":"refs/tags/v1.2.3","object":{"type":"commit","sha":SHA}}),
        &proof
    )
    .is_ok());
    assert!(validate_tag_commit(
        &json!({"ref":"refs/tags/v1.2.3","object":{"type":"commit","sha":"d".repeat(40)}}),
        &proof
    )
    .is_err());
}
#[test]
fn source_version_and_public_key_match_installed_trust_root() {
    let proof = admission(&release()).unwrap();
    assert!(validate_source_config(
        &json!({"version":"1.2.3","plugins":{"updater":{"pubkey":KEY}}}),
        &proof,
        KEY
    )
    .is_ok());
    assert!(validate_source_config(
        &json!({"version":"1.2.4","plugins":{"updater":{"pubkey":KEY}}}),
        &proof,
        KEY
    )
    .is_err());
    assert!(validate_source_config(
        &json!({"version":"1.2.3","plugins":{"updater":{"pubkey":"other"}}}),
        &proof,
        KEY
    )
    .is_err());
}
#[test]
fn explicit_proxy_is_separate_from_cli_config_and_empty_means_inherit() {
    assert!(validated_proxy(None).unwrap().is_none());
    assert!(validated_proxy(Some(" ")).unwrap().is_none());
    assert_eq!(
        validated_proxy(Some("http://localhost:1080"))
            .unwrap()
            .unwrap()
            .scheme(),
        "http"
    );
    for value in [
        "file:///tmp/proxy",
        "not a URL",
        "socks5://localhost:1080",
        "https://host/#secret",
    ] {
        assert!(validated_proxy(Some(value)).is_err());
    }
}
#[test]
fn failures_serialize_only_fixed_code_and_stage() {
    let value = serde_json::to_value(failure("UPDATER_PROXY_INVALID", "proxy")).unwrap();
    assert_eq!(
        value,
        json!({"code":"UPDATER_PROXY_INVALID","stage":"proxy"})
    );
}

#[test]
fn signed_bytes_and_official_digests_are_both_required_before_install() {
    // Public prehashed Minisign vector from minisign-verify 0.2.5 tests.
    let key = STANDARD.encode(
        "untrusted comment: fixture\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n",
    );
    let signature = STANDARD.encode(concat!(
        "untrusted comment: signature from minisign secret key\n",
        "RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/",
        "z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\n",
        "trusted comment: timestamp:1556193335\tfile:test\n",
        "y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg=="
    ));
    let mut asset = admission(&release()).unwrap().package;
    asset.size = 4;
    asset.sha256 = sha256(b"test");
    assert!(verify_package(b"test", &signature, &key, &asset).is_ok());
    assert_eq!(
        verify_package(b"evil", &signature, &key, &asset)
            .unwrap_err()
            .code,
        "UPDATER_PACKAGE_MISMATCH"
    );
    asset.sha256 = sha256(b"evil");
    assert_eq!(
        verify_package(b"evil", &signature, &key, &asset)
            .unwrap_err()
            .code,
        "UPDATER_SIGNATURE_INVALID"
    );
    assert!(verify_package(b"evil", "not a signature", &key, &asset).is_err());
    assert!(verify_package(b"evil", &signature, "not a key", &asset).is_err());
}

#[test]
fn manifest_signature_is_bound_to_the_uploaded_signature_asset() {
    let signature = b"fixture-base64-signature\n";
    let mut asset = admission(&release()).unwrap().signature;
    asset.size = signature.len() as u64;
    asset.sha256 = sha256(signature);
    assert!(verify_signature_asset(signature, &asset, "fixture-base64-signature").is_ok());
    assert!(verify_signature_asset(signature, &asset, "other-signature").is_err());
    assert!(verify_signature_asset(b"other-signature\n", &asset, "other-signature").is_err());
}
