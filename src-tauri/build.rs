fn main() {
    // 从 package.json 读取版本号并注入为编译时环境变量
    // 确保 Rust 后端与前端使用同一个版本来源
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let package_json_path = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .join("package.json");

    if package_json_path.exists() {
        let content = std::fs::read_to_string(&package_json_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        let version = json["version"].as_str().unwrap();
        println!("cargo:rustc-env=APP_VERSION={}", version);
    }

    let build_sha = std::env::var("CC_DESK_BUILD_SHA")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .unwrap_or_else(|_| "local".to_string());
    println!("cargo:rerun-if-env-changed=CC_DESK_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rustc-env=CC_DESK_BUILD_SHA={build_sha}");

    tauri_build::build()
}
