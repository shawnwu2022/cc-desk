mod bundle_manifest;

fn main() {
    // Only the source-reused, noninteractive qualification loader consumes this.
    println!("cargo:rustc-env=CC_DESK_BUILD_SHA=LOCAL-CONPTY-QUALIFICATION-NOT-RELEASE");
    if std::env::var_os("CARGO_FEATURE_CONPTY_QUALIFICATION").is_none()
        || std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
    {
        return;
    }
    assert_eq!(
        std::env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
        Ok("x86_64")
    );
    let crate_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let bundle = crate_dir.join("../../../src-tauri/conpty");
    let manifest = bundle.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).expect("Cargo profile directory");
    // Stage the same pinned bundle as the desktop build. The runtime loader still
    // validates its manifest hashes, PE architecture and exports before any use.
    for (name, size) in bundle_manifest::entries(&json).expect("invalid pinned bundle manifest") {
        let source = bundle.join("runtime").join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        let bytes = std::fs::read(&source)
            .expect("Missing pinned bundle; run node scripts/prepare-conpty.mjs first");
        assert_eq!(bytes.len() as u64, size);
        for dir in [profile.to_path_buf(), profile.join("deps")] {
            std::fs::create_dir_all(&dir).unwrap();
            let destination = dir.join(name);
            if std::fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
                std::fs::write(destination, &bytes).unwrap();
            }
        }
    }
}
