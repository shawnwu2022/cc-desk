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

    tauri_build::build()
}
