fn main() {
    for key in [
        "LIBSQLITE3_SYS_USE_PKG_CONFIG",
        "SQLITE3_LIB_DIR",
        "SQLITE3_INCLUDE_DIR",
        "LIBSQLITE3_FLAGS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        if std::env::var_os(key).is_some_and(|value| !value.is_empty() && value != "0") {
            panic!("external SQLite build override is not admitted");
        }
    }
}
