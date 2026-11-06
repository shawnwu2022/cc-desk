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
    validate_official_release(
        value,
        "1.2.3",
        "windows-x86_64",
        URL,
        &json!({}),
        UpdateChannel::Stable,
    )
}
#[test]
fn official_release_passes_existing_contract() {
    let proof = admission(&release()).unwrap();
    assert_eq!(proof.release_id, 123);
    assert_eq!(proof.source_sha, SHA);
    assert_eq!(proof.package.size, 10);
}

#[test]
fn ReleaseNotes_DisclosureWrapperPreservesAdmission_002() {
    let mut value = release();
    let original = value["body"].as_str().unwrap();
    value["body"] = json!(format!("Fixed\n- Better titles.\n\nImportant\n- Keep backups.\n\n<details>\n<summary>Technical validation details</summary>\n\n{original}\n</details>\n"));
    assert!(admission(&value).is_ok());
    value["body"] = json!("Fixed\n- Better titles.\n");
    assert!(admission(&value).is_err());
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
        assert!(validate_official_release(
            &release(),
            "1.2.3",
            "windows-x86_64",
            URL,
            &raw,
            UpdateChannel::Stable
        )
        .is_err());
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
            .stage,
        "verify"
    );
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
#[test]
fn ReleaseNotes_BoundedLocalesRetainImportantText_001() {
    let notes = crate::updater_policy::localized_release_notes(&serde_json::json!({
        "notes_i18n": {"en":" Important: back up shared data. ", "zh":"重要：备份共享数据。", "private":"must not pass"}
    })).unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes["en"], "Important: back up shared data.");
    assert_eq!(notes["zh"], "重要：备份共享数据。");
    assert!(crate::updater_policy::localized_release_notes(
        &serde_json::json!({"notes":"Legacy notes"})
    )
    .is_none());
    assert!(crate::updater_policy::localized_release_notes(
        &serde_json::json!({"notes_i18n":{"zh":"无英文"}})
    )
    .is_none());
    assert!(crate::updater_policy::localized_release_notes(
        &serde_json::json!({"notes_i18n":{"en":"x".repeat(4001)}})
    )
    .is_none());
    let english = crate::updater_policy::localized_release_notes(
        &serde_json::json!({"notes_i18n":{"en":"Keep backups", "zh":"x".repeat(4001)}}),
    )
    .unwrap();
    assert_eq!(english.len(), 1);
    assert_eq!(english["en"], "Keep backups");
}

fn versioned_release(version: &str) -> Value {
    let mut value = release();
    let category = release_channel(version).unwrap();
    value["tag_name"] = json!(format!("v{version}"));
    value["prerelease"] = json!(category != ReleaseChannel::Stable);
    value["html_url"] = json!(format!(
        "https://github.com/{REPOSITORY}/releases/tag/v{version}"
    ));
    for (index, suffix) in [(0, ""), (1, ".sig")] {
        let name = format!("CC.Desk_{version}_x64-setup.exe{suffix}");
        value["assets"][index]["name"] = json!(name);
        value["assets"][index]["browser_download_url"] = json!(format!(
            "https://github.com/{REPOSITORY}/releases/download/v{version}/{name}"
        ));
    }
    value
}

#[test]
#[allow(non_snake_case)]
fn UpdateChannel_AdmissionMatrix_001() {
    for (version, admitted) in [
        ("1.2.3", [true, true, true]),
        ("1.2.3-beta.2", [false, true, true]),
        ("1.2.3-rc.3", [false, true, true]),
        ("1.2.3-preview.4", [false, false, true]),
    ] {
        let release = versioned_release(version);
        let url = release["assets"][0]["browser_download_url"]
            .as_str()
            .unwrap();
        for (selected, expected) in [
            UpdateChannel::Stable,
            UpdateChannel::Test,
            UpdateChannel::Preview,
        ]
        .into_iter()
        .zip(admitted)
        {
            let result = validate_official_release(
                &release,
                version,
                "windows-x86_64",
                url,
                &json!({}),
                selected,
            );
            assert_eq!(result.is_ok(), expected, "{version} / {selected:?}");
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn UpdateChannel_UnknownVersionsAndInvalidSelection_002() {
    assert_eq!(
        UpdateChannel::from_config(None).unwrap(),
        UpdateChannel::Stable
    );
    for value in ["candidate", "test-only", "nightly", "Stable", ""] {
        assert!(UpdateChannel::from_config(Some(value)).is_err());
        assert!(serde_json::from_value::<UpdateChannel>(json!(value)).is_err());
    }
    for version in [
        "1.2.3-alpha.1",
        "1.2.3-rc.x",
        "1.2.3-RC.1",
        "1.2.3-rc.1.more",
        "1.2.3+candidate",
        "01.2.3",
        "1.2.3-preview.evil",
    ] {
        assert!(release_channel(version).is_none(), "{version}");
    }
    for (version, label) in [
        ("1.2.3-beta", "beta"),
        ("1.2.3-rc", "rc"),
        ("1.2.3-preview", "preview"),
    ] {
        assert_eq!(release_channel(version).unwrap().as_str(), label);
    }
}

#[test]
#[allow(non_snake_case)]
fn UpdateChannel_ExclusionIsIndependentOfUserSelection_003() {
    let release = versioned_release("1.2.3-preview.1");
    let url = release["assets"][0]["browser_download_url"]
        .as_str()
        .unwrap();
    for raw in [
        json!({"channel":"candidate"}),
        json!({"channel":"test-only"}),
        json!({"publishable":false}),
        json!({"updaterPublication":false}),
    ] {
        assert!(validate_official_release(
            &release,
            "1.2.3-preview.1",
            "windows-x86_64",
            url,
            &raw,
            UpdateChannel::Preview
        )
        .is_err());
    }
    for (field, invalid) in [
        ("draft", json!(true)),
        ("prerelease", json!(false)),
        ("published_at", json!("")),
        ("target_commitish", json!("main")),
        ("body", json!("Source: fake")),
    ] {
        let mut altered = release.clone();
        altered[field] = invalid;
        assert!(
            validate_official_release(
                &altered,
                "1.2.3-preview.1",
                "windows-x86_64",
                url,
                &json!({}),
                UpdateChannel::Preview
            )
            .is_err(),
            "{field}"
        );
    }
}

#[test]
#[allow(non_snake_case)]
fn UpdateChannel_SelectHighestSemverNeverDowngrade_004() {
    let releases = [
        versioned_release("1.4.0-preview.1"),
        versioned_release("1.2.3"),
        versioned_release("1.3.0-rc.2"),
        versioned_release("1.3.0-beta.9"),
    ];
    for (selected, expected) in [
        (UpdateChannel::Stable, "v1.2.3"),
        (UpdateChannel::Test, "v1.3.0-rc.2"),
        (UpdateChannel::Preview, "v1.4.0-preview.1"),
    ] {
        assert_eq!(
            select_release(&releases, "1.2.2", selected)
                .unwrap()
                .unwrap()["tag_name"],
            expected
        );
    }
    assert!(
        select_release(&releases, "1.3.0-rc.2", UpdateChannel::Stable)
            .unwrap()
            .is_none()
    );
    let mut releases = releases.to_vec();
    let mut duplicate = versioned_release("1.4.0-preview.1");
    duplicate["id"] = json!(456);
    releases.push(duplicate);
    assert!(select_release(&releases, "1.2.2", UpdateChannel::Preview).is_err());
}

#[test]
#[allow(non_snake_case)]
fn UpdateChannel_PreviewProofStillBindsSourceAssetsAndSignatures_005() {
    let release = versioned_release("1.2.3-preview.1");
    let url = release["assets"][0]["browser_download_url"]
        .as_str()
        .unwrap();
    for (field, value) in [
        ("digest", Value::Null),
        ("browser_download_url", json!("https://example.com/package")),
        ("state", json!("new")),
        ("id", json!(0)),
        ("size", json!(MAX_PACKAGE_BYTES + 1)),
    ] {
        let mut altered = release.clone();
        altered["assets"][0][field] = value;
        assert!(
            validate_official_release(
                &altered,
                "1.2.3-preview.1",
                "windows-x86_64",
                url,
                &json!({}),
                UpdateChannel::Preview
            )
            .is_err(),
            "{field}"
        );
    }
    let proof = validate_official_release(
        &release,
        "1.2.3-preview.1",
        "windows-x86_64",
        url,
        &json!({}),
        UpdateChannel::Preview,
    )
    .unwrap();
    assert!(validate_tag_commit(&json!({"ref":"refs/tags/v1.2.3-preview.1", "object":{"type":"commit","sha":"e".repeat(40)}}), &proof).is_err());
    assert!(validate_source_config(
        &json!({"version":"1.2.3-preview.1", "plugins":{"updater":{"pubkey":"evil"}}}),
        &proof,
        KEY
    )
    .is_err());
}

#[test]
#[allow(non_snake_case)]
fn UpdateCancellation_CommitIsOneShotAndCancellationIsFinal_006() {
    let cancelled = InstallCancellation::default();
    assert_eq!(cancelled.request(), CancelStatus::Requested);
    assert_eq!(cancelled.request(), CancelStatus::Requested);
    assert_eq!(cancelled.check().unwrap_err().code, "UPDATER_CANCELLED");
    assert!(cancelled.commit().is_err());
    let committed = InstallCancellation::default();
    committed.commit().unwrap();
    assert_eq!(committed.request(), CancelStatus::TooLate);
    assert!(committed.commit().is_err());
}

#[test]
#[allow(non_snake_case)]
fn UpdateCancellation_RacingCancelAndInstallHaveOneWinner_007() {
    for _ in 0..128 {
        let control = std::sync::Arc::new(InstallCancellation::default());
        let cancel = control.clone();
        let waiter = std::thread::spawn(move || cancel.request());
        let committed = control.commit().is_ok();
        let cancelled = waiter.join().unwrap();
        assert_eq!(committed, cancelled == CancelStatus::TooLate);
        if committed {
            assert!(control.check().is_ok());
        } else {
            assert!(control.check().is_err());
        }
    }
}

#[tokio::test]
#[allow(non_snake_case)]
async fn UpdateCancellation_AlreadyRequestedAndLiveWaitBothWake_008() {
    let control = std::sync::Arc::new(InstallCancellation::default());
    let cancel = control.clone();
    let request = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel.request()
    });
    control.cancelled().await;
    assert_eq!(request.await.unwrap(), CancelStatus::Requested);
    control.cancelled().await;
    assert!(control.commit().is_err());
}
