fn main() {
    // 共享源码含远程门；仅声明合法 cfg，不启用远程功能。
    println!("cargo:rustc-check-cfg=cfg(feature, values(\"remote-host\"))");
}
