#![cfg(feature = "conpty-qualification")]
#[path = "../bundle_manifest.rs"]
mod bundle_manifest;

fn manifest() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../../src-tauri/conpty/manifest.json")).unwrap()
}

#[test]
fn complete_manifest_preserves_the_single_source_file_list() {
    let m = manifest();
    let names = bundle_manifest::entries(&m).unwrap();
    assert_eq!(names.len(), 3);
    assert_eq!(names[0], ("conpty.dll", 109920));
    assert_eq!(names[1], ("OpenConsole.exe", 1066296));
}

#[test]
fn duplicates_missing_files_and_path_components_reject_before_staging() {
    let original = manifest();
    for name in [
        "conpty.dll",
        "CONPTY.DLL",
        "../OpenConsole.exe",
        "unexpected.dll",
    ] {
        let mut m = original.clone();
        m["files"][2]["name"] = name.into();
        assert!(bundle_manifest::entries(&m).is_err());
    }
    let mut m = original.clone();
    m["files"].as_array_mut().unwrap().pop();
    assert!(bundle_manifest::entries(&m).is_err());
    let mut m = original;
    m["files"][0]["bytes"] = 0.into();
    assert!(bundle_manifest::entries(&m).is_err());
}
