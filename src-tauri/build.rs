fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_dir = std::path::Path::new(&manifest_dir);
    let package_json_path = manifest_dir.parent().unwrap().join("package.json");
    if package_json_path.exists() {
        let content = std::fs::read_to_string(&package_json_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        println!(
            "cargo:rustc-env=APP_VERSION={}",
            json["version"].as_str().unwrap()
        );
    }
    let build_sha = std::env::var("CC_DESK_BUILD_SHA")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .unwrap_or_else(|_| "local".to_string());
    println!("cargo:rerun-if-env-changed=CC_DESK_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rustc-env=CC_DESK_BUILD_SHA={build_sha}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        assert_eq!(
            std::env::var("CARGO_CFG_TARGET_ARCH").unwrap(),
            "x86_64",
            "Bundled ConPTY currently supports Windows x64 only"
        );
        let manifest = manifest_dir.join("conpty/manifest.json");
        println!("cargo:rerun-if-changed={}", manifest.display());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let profile = out.ancestors().nth(3).expect("Cargo profile directory");
        // tauri dev, --no-bundle, normal binaries and library test executables.
        for entry in json["files"].as_array().unwrap() {
            let name = entry["name"].as_str().unwrap();
            let source = manifest_dir.join("conpty/runtime").join(name);
            println!("cargo:rerun-if-changed={}", source.display());
            let bytes = std::fs::read(&source).unwrap_or_else(|_| panic!("Missing {name}. Run node scripts/prepare-conpty.mjs from the repository root before cargo on Windows."));
            for dir in [profile.to_path_buf(), profile.join("deps")] {
                std::fs::create_dir_all(&dir).unwrap();
                let destination = dir.join(name);
                if std::fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
                    std::fs::write(destination, &bytes).unwrap();
                }
            }
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Tauri's resource archive does not reach the library unit-test EXE.
        // Embed its unchanged Common Controls v6 declaration at link time for
        // all MSVC executables, including that target. Do not embed it twice.
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("failed to build Tauri resources");
        let manifest = manifest_dir.join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        // The original Tauri default declares no UAC policy. Do not synthesize
        // a new privilege declaration while relocating manifest embedding.
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    } else {
        tauri_build::build();
    }
}
