use quote::ToTokens;
use std::{env, fs, path::Path};

// 只按名称选取真实源码项；保留属性、签名与函数体，缺项或重名时立即失败。
fn production_items(root: &Path, source: &str, names: &[&str]) -> String {
    let path = root.join(source);
    println!("cargo:rerun-if-changed={}", path.display());
    let parsed = syn::parse_file(&fs::read_to_string(&path).expect("read production source"))
        .expect("parse production source");
    names
        .iter()
        .map(|name| {
            let selected: Vec<_> = parsed
                .items
                .iter()
                .filter(|item| match item {
                    syn::Item::Fn(value) => value.sig.ident == name,
                    syn::Item::Struct(value) => value.ident == name,
                    syn::Item::Const(value) => value.ident == name,
                    _ => false,
                })
                .collect();
            assert_eq!(selected.len(), 1, "production item {source}::{name}");
            selected[0].to_token_stream().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
    let manifest = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest
        .join("../../../src-tauri/src")
        .canonicalize()
        .unwrap();
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let snapshot = production_items(&root, "cli/snapshot.rs", &["CallerIdentity"]);
    fs::write(
        out.join("snapshot.rs"),
        format!("use crate::types::WireU64;\n{snapshot}"),
    )
    .unwrap();
    let store = production_items(
        &root,
        "store.rs",
        &[
            "write_bytes_atomic",
            "replace_file_atomic",
            "is_lock_contention",
            "acquire_lock",
            "acquire_lock_with_label",
            "SessionInfo",
            "SessionEntry",
            "SessionDirectoryEnumeration",
            "session_entry_from_path",
            "enumerate_session_entries",
            "resolve_session_entries",
            "get_sessions_indexed_at",
            "finish_resolver",
        ],
    );
    fs::write(out.join("store.rs"), format!(
        "use anyhow::{{bail, Context, Result}}; use serde::{{Deserialize, Serialize}}; use crate::session_name_index::{{FileStamp, IndexedResult, SessionNameIndexStore, SessionNameResolver}}; use std::{{collections::BTreeSet, fs::{{self, File}}, io::{{self, Write}}, path::{{Path, PathBuf}}, time::{{Duration, Instant}}}};\n{store}"
    )).unwrap();
    let platform = production_items(
        &root,
        "platform/legacy.rs",
        &["CREATE_NO_WINDOW", "configure_command"],
    );
    fs::write(
        out.join("platform.rs"),
        format!("use std::process::Command;\n{platform}"),
    )
    .unwrap();
    let windows = production_items(
        &root,
        "version_history/windows.rs",
        &["blocked", "win_error", "handle", "own"],
    );
    let files = root.join("version_history/windows/files.rs");
    let security = root.join("version_history/windows/security.rs");
    fs::write(out.join("windows.rs"), format!(
        "use std::{{io, os::windows::io::{{AsRawHandle, FromRawHandle, OwnedHandle}}}}; use windows::Win32::Foundation::HANDLE;\n{windows}\n#[path = {files:?}] pub(crate) mod files;\n#[path = {security:?}] pub(crate) mod security;"
    )).unwrap();
    // include_bytes! 使用调用 crate 的 manifest；复制已生成的同一官方 bundle。
    let helper = root.join("../resources/native-session-sdk/bridge.cjs");
    println!("cargo:rerun-if-changed={}", helper.display());
    let bytes =
        fs::read(&helper).expect("run npm ci && npm run prepare:session-sdk before scope-core");
    let destination = manifest.join("resources/native-session-sdk");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("bridge.cjs"), bytes).unwrap();
}
