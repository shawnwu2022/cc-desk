use crate::cli::document::{DocumentAuthority, NativeContext};
use crate::cli::run_registry::RunRegistry;
use crate::version_history::commands::decode_history_request;
use crate::version_history::types::ListHistoryRequest;
use serde_json::json;
use std::sync::Arc;
use tauri::http::HeaderMap;
use tauri::ipc::InvokeBody;
use tauri::{ResourceTable, Url};

// 文档鉴权必须先于原始请求格式或大小检查。
#[test]
fn HistoryCommand_AuthBeforeDecode_001() {
    let url = Url::parse("https://tauri.localhost/index.html").unwrap();
    let authority =
        DocumentAuthority::new(Arc::new(RunRegistry::<()>::new(8)), url.clone()).unwrap();
    let mut table = ResourceTable::default();
    let binding = authority.attach(&mut table).unwrap();
    authority.started(&url);
    authority.finished(&url);
    let context = NativeContext {
        window_label: "main",
        webview_label: "main",
        url: &url,
    };
    let body = InvokeBody::Json(json!({"cursor": null}));
    let result = decode_history_request::<ListHistoryRequest>(
        binding.admit(&table, &context, &HeaderMap::new()),
        &body,
    );
    assert_eq!(result.err().unwrap().code, "FORBIDDEN");
    let result = decode_history_request::<ListHistoryRequest>(
        binding.admit(&table, &context, &authority.test_headers()),
        &body,
    );
    assert_eq!(result.err().unwrap().code, "RAW_BODY_REQUIRED");
}

// 已授权请求仍拒绝URL/所有者附加字段和超大正文。
#[test]
fn HistoryCommand_BoundedDecode_002() {
    let url = Url::parse("https://tauri.localhost/index.html").unwrap();
    let authority =
        DocumentAuthority::new(Arc::new(RunRegistry::<()>::new(8)), url.clone()).unwrap();
    let mut table = ResourceTable::default();
    let binding = authority.attach(&mut table).unwrap();
    authority.started(&url);
    authority.finished(&url);
    let context = NativeContext {
        window_label: "main",
        webview_label: "main",
        url: &url,
    };
    for (body, expected) in [
        (
            InvokeBody::Raw(
                serde_json::to_vec(&json!({"cursor": null,"url":"https://private"})).unwrap(),
            ),
            "INVALID_REQUEST",
        ),
        (InvokeBody::Raw(vec![b' '; 1025]), "REQUEST_TOO_LARGE"),
    ] {
        assert_eq!(
            decode_history_request::<ListHistoryRequest>(
                binding.admit(&table, &context, &authority.test_headers()),
                &body
            )
            .err()
            .unwrap()
            .code,
            expected
        );
    }
    let body = InvokeBody::Raw(br#"{"cursor":null}"#.to_vec());
    assert!(decode_history_request::<ListHistoryRequest>(
        binding.admit(&table, &context, &authority.test_headers()),
        &body
    )
    .is_ok());
    authority.revoke();
    assert_eq!(
        decode_history_request::<ListHistoryRequest>(
            binding.admit(&table, &context, &authority.test_headers()),
            &body
        )
        .err()
        .unwrap()
        .code,
        "FORBIDDEN"
    );
}

// 网络读取期间原文档撤销后，真实目录响应不能被发布给该旧文档。
#[test]
fn HistoryCommand_RevokedDuringIo_003() {
    use crate::cli::types::SafeError;
    use crate::version_history::catalog::{CatalogService, CatalogSource, ReleaseMetadata};
    use crate::version_history::commands::{HistoryDocument, HistoryService};
    use crate::version_history::policy::HostPlatform;
    use parking_lot::Mutex;
    struct RevokingSource(Arc<DocumentAuthority<()>>);
    impl CatalogSource for RevokingSource {
        fn list(&self, _: u16) -> Result<Vec<ReleaseMetadata>, SafeError> {
            self.0.revoke();
            Ok(vec![])
        }
        fn release(&self, _: u64) -> Result<ReleaseMetadata, SafeError> {
            unreachable!()
        }
    }
    let url = Url::parse("https://tauri.localhost/index.html").unwrap();
    let authority =
        DocumentAuthority::new(Arc::new(RunRegistry::<()>::new(8)), url.clone()).unwrap();
    let table = Arc::new(Mutex::new(ResourceTable::default()));
    let binding = Arc::new(authority.attach(&mut table.lock()).unwrap());
    authority.started(&url);
    authority.finished(&url);
    let document =
        HistoryDocument::test_bound(binding, table, url, authority.test_headers()).unwrap();
    let catalog = Arc::new(CatalogService::new(
        Arc::new(RevokingSource(authority)),
        HostPlatform::WindowsX64,
    ));
    let service = HistoryService::with_catalog(catalog);
    assert_eq!(
        service
            .list(&document, &ListHistoryRequest { cursor: None })
            .err()
            .unwrap()
            .code,
        "FORBIDDEN"
    );
}

// Windows私有目录派生能力必须支持真实的相对目录/文件写入，并保持父目录私有ACL。
#[cfg(windows)]
#[test]
fn HistoryCommand_PrivateCapability_004() {
    use crate::version_history::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        security::CurrentUser,
    };
    use std::ffi::OsStr;
    use std::io::Write;
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let private = PrivateDirectory::create_new(
        parent,
        ComponentName::new(OsStr::new("preparation")).unwrap(),
        &user,
    )
    .unwrap();
    let capability = private.directory().capability().unwrap();
    capability.create_dir("transaction").unwrap();
    let transaction = capability.open_dir("transaction").unwrap();
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    let mut file = transaction.open_with("package.bin", &options).unwrap();
    file.write_all(b"bounded preparation").unwrap();
    file.sync_all().unwrap();
    assert_eq!(
        transaction.read("package.bin").unwrap(),
        b"bounded preparation"
    );
    private.verify(&user).unwrap();
}
