//! 三 CLI 离线元数据探测；复用原有 Job owner，不能当作 GUI/模型生命周期验收。
use super::*;

fn pin_input(path: &Path, expected: Option<&str>, limit: u64) -> (File, String) {
    use std::os::windows::fs::MetadataExt;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .unwrap();
    let metadata = file.metadata().unwrap();
    assert!(metadata.is_file() && metadata.file_attributes() & 0x400 == 0);
    assert!(metadata.len() > 0 && metadata.len() <= limit);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if let Some(expected) = expected {
        assert_eq!(digest, expected, "fixed executable bytes changed");
    }
    (file, digest)
}

fn execute(cli: &str, binary: &str, expected: &str) {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_METADATA_LIVE_GO").as_deref(),
        Ok("isolated-metadata")
    );
    system_configuration_absent().unwrap();
    let node = fs::canonicalize(
        std::env::var_os("CC_DESK_NATIVE_METADATA_NODE")
            .expect("explicit Node executable required"),
    )
    .unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("scripts/native-cli/metadata-lifecycle.mjs");
    let (node_lease, node_sha) = pin_input(
        &node,
        Some("9a4eb5f1c29c6a2e93852ead46b999e284a6a5ca8bab4d4e241d587d025a52de"),
        200 * 1024 * 1024,
    );
    let (_script_lease, script_sha) = pin_input(&script, None, 128 * 1024);
    let (_binary_lease, binary_sha) =
        pin_input(Path::new(binary), Some(expected), 512 * 1024 * 1024);
    let sdk = if cli == "claude" {
        Some(
            fs::canonicalize(
                std::env::var_os("CC_DESK_NATIVE_METADATA_SDK")
                    .expect("explicit pinned SDK module required"),
            )
            .unwrap(),
        )
    } else {
        None
    };
    let sdk_lease = sdk.as_ref().map(|path| {
        pin_input(
            path,
            Some("6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45"),
            32 * 1024 * 1024,
        )
    });
    let mut fixture = Fixture::new().unwrap();
    fixture.environment.insert(
        "CC_DESK_NATIVE_METADATA_LIVE_GO".into(),
        "isolated-metadata".into(),
    );
    let fixture = Arc::new(fixture);
    let spec = serde_json::json!({"cli":cli,"binary":binary,"binarySha256":binary_sha,"root":fixture.root,"project":fixture.project,"sdk":sdk});
    let spec_path = fixture.root.join("metadata-spec.json");
    fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    let (_spec_lease, _) = pin_input(&spec_path, None, 32768);
    let mut command = Command::new(node);
    command.arg(script).arg(spec_path);
    let owned = start_owned(
        fixture.clone(),
        format!("metadata-{cli}"),
        command,
        Some(node_lease),
        false,
        true,
        None,
    )
    .unwrap()
    .wait_until_settled()
    .unwrap();
    // 子驱动的成功不代表其后代已退出；必须先得到持有 Job 的外层实际收口。
    let owner = serde_json::json!({"rootReaped":true,"jobEmpty":true,"forced":owned.forced,"exitCode":owned.status.code(),"success":owned.status.success()});
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(fixture.root.join("reports/metadata-owner.json"))
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&owner).unwrap())
        .unwrap();
    let report_path = fixture.root.join("reports/metadata-result.json");
    let raw = fs::read(&report_path).unwrap();
    let mut result: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert!(
        owned.status.success() && !owned.forced,
        "driver failed: {}",
        result
    );
    assert_eq!(result["status"], "METADATA_PASS");
    assert_eq!(result["cli"], cli);
    assert_eq!(result["binarySha256"], binary_sha);
    assert_eq!(result["nativeLifecycleAccepted"], false);
    assert_eq!(result["guiAccepted"], false);
    assert_eq!(result["modelCalled"], false);
    result["sourceCommitBuild"] = serde_json::json!(option_env!("CC_DESK_BUILD_SHA"));
    result["nodeSha256"] = serde_json::json!(node_sha);
    result["driverSha256"] = serde_json::json!(script_sha);
    result["sdkSha256"] = serde_json::json!(sdk_lease.as_ref().map(|(_, sha)| sha));
    result["rootReaped"] = serde_json::json!(true);
    result["jobEmpty"] = serde_json::json!(true);
    result["processGroupAcceptance"] = serde_json::json!("outer-owner-confirmed");
    let final_path = fixture.root.join("reports/metadata-owned-result.json");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(final_path)
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&result).unwrap())
        .unwrap();
    println!("NATIVE_METADATA_LIVE_RESULT {result}");
}

#[test]
#[ignore = "explicit isolated Claude SDK metadata experiment; no GUI or model"]
fn NativeMetadataLive_InstalledClaude_110() {
    execute(
        "claude",
        r"C:\Users\wusha\.local\bin\claude.exe",
        "2e055732c9d03d22700d9046251a8370b673607bfcc9e4d16a1b0e8fb1345cbd",
    );
}
#[test]
#[ignore = "explicit isolated Codex app-server metadata experiment; no GUI or model"]
fn NativeMetadataLive_InstalledCodex_111() {
    execute("codex", CODEX_EXE, CODEX_SHA256);
}
#[test]
#[ignore = "explicit isolated OpenCode HTTP metadata experiment; no GUI or model"]
fn NativeMetadataLive_InstalledOpenCode_112() {
    execute(
        "opencode",
        r"D:\nodejs\node_global\node_modules\@opencode\cli\bin\opencode.exe",
        "98cb0aa854c7c015cf6febafa7afb872c359bfe5a823a526f3b4fd347a9e9677",
    );
}
