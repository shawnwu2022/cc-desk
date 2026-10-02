use crate::version_history::manager_document::{
    ManagerDocumentRegistry, ManagerNativeContext, MANAGER_DOCUMENT_HEADER, MANAGER_LABEL,
};
use tauri::{http::HeaderMap, ResourceTable, Url};

const TRANSACTION: &str = "00000000-0000-4000-8000-000000000111";
fn url() -> Url {
    "http://tauri.localhost/version-manager.html"
        .parse()
        .unwrap()
}

// 独立manager不需要NativeRuntime或RunRegistry，且只有完成初始加载的原文档可请求。
#[test]
fn HistoryManagerDocument_ExactAuthority_001() {
    let registry = ManagerDocumentRegistry::default();
    let authority = registry.create(TRANSACTION, url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = authority.attach(&mut table).unwrap();
    let headers = authority.test_headers();
    let expected = url();
    let context = ManagerNativeContext {
        window_label: MANAGER_LABEL,
        webview_label: MANAGER_LABEL,
        url: &expected,
    };
    assert!(binding.admit(&table, &context, &headers).is_err());
    authority.started(&expected);
    authority.finished(&expected);
    assert_eq!(
        binding.admit(&table, &context, &headers).unwrap(),
        TRANSACTION
    );
    assert!(registry.create(TRANSACTION, expected).is_err());
}

// 相同URL重载永久撤销旧document，后续同名window不能恢复旧proof。
#[test]
fn HistoryManagerDocument_Replacement_002() {
    let registry = ManagerDocumentRegistry::default();
    let first = registry.create(TRANSACTION, url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = first.attach(&mut table).unwrap();
    let expected = url();
    first.started(&expected);
    first.finished(&expected);
    assert!(!first.navigation(&expected));
    let replacement = registry.create(TRANSACTION, expected.clone()).unwrap();
    let mut next_table = ResourceTable::default();
    let next = replacement.attach(&mut next_table).unwrap();
    replacement.started(&expected);
    replacement.finished(&expected);
    let context = ManagerNativeContext {
        window_label: MANAGER_LABEL,
        webview_label: MANAGER_LABEL,
        url: &expected,
    };
    assert!(binding
        .admit(&table, &context, &first.test_headers())
        .is_err());
    assert!(next
        .admit(&next_table, &context, &first.test_headers())
        .is_err());
    assert!(next
        .admit(&next_table, &context, &replacement.test_headers())
        .is_ok());
    first.revoke();
    assert!(next
        .admit(&next_table, &context, &replacement.test_headers())
        .is_ok());
}

// 标签、URL、资源表和唯一proof必须全部匹配；普通main的凭据不是manager授权。
#[test]
fn HistoryManagerDocument_RejectForeignSurface_003() {
    let registry = ManagerDocumentRegistry::default();
    let authority = registry.create(TRANSACTION, url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = authority.attach(&mut table).unwrap();
    let expected = url();
    authority.started(&expected);
    authority.finished(&expected);
    let headers = authority.test_headers();
    for label in ["main", "peer", "version-manager-2"] {
        let context = ManagerNativeContext {
            window_label: label,
            webview_label: MANAGER_LABEL,
            url: &expected,
        };
        assert!(binding.admit(&table, &context, &headers).is_err());
    }
    let context = ManagerNativeContext {
        window_label: MANAGER_LABEL,
        webview_label: MANAGER_LABEL,
        url: &expected,
    };
    assert!(binding
        .admit(&ResourceTable::default(), &context, &headers)
        .is_err());
    let mut duplicate = headers.clone();
    duplicate.append(
        MANAGER_DOCUMENT_HEADER,
        headers[MANAGER_DOCUMENT_HEADER].clone(),
    );
    assert!(binding.admit(&table, &context, &duplicate).is_err());
    let mut ordinary = HeaderMap::new();
    ordinary.insert(
        "x-cc-desk-document",
        headers[MANAGER_DOCUMENT_HEADER].clone(),
    );
    assert!(binding.admit(&table, &context, &ordinary).is_err());
}

// manager入口只接受自身固定本地页面；不能把普通App或任意远端URL带进权限域。
#[test]
fn HistoryManagerDocument_FixedEntry_004() {
    let registry = ManagerDocumentRegistry::default();
    for bad in [
        "https://example.invalid/version-manager.html",
        "http://tauri.localhost/index.html",
        "http://tauri.localhost/version-manager.html?path=anything",
        "http://user@tauri.localhost/version-manager.html",
        "http://tauri.localhost:123/version-manager.html",
    ] {
        assert!(registry.create(TRANSACTION, bad.parse().unwrap()).is_err());
    }
    assert!(registry.create("not-a-uuid", url()).is_err());
}

// 初始handoff固定当前文档；导航被真正拒绝时不能撤销仍然存活的同一UI。
#[test]
fn HistoryManagerDocument_HandoffPinsCurrentDocument_005() {
    let registry = ManagerDocumentRegistry::default();
    let authority = registry.create(TRANSACTION, url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = authority.attach(&mut table).unwrap();
    let expected = url();
    authority.started(&expected);
    authority.finished(&expected);
    authority.fixture_pin_handoff().unwrap();
    let context = ManagerNativeContext {
        window_label: MANAGER_LABEL,
        webview_label: MANAGER_LABEL,
        url: &expected,
    };
    assert!(!authority.navigation(&expected));
    assert!(!authority.navigation(&"https://example.invalid/".parse().unwrap()));
    assert!(binding
        .admit(&table, &context, &authority.test_headers())
        .is_ok());
    assert!(binding.blocks_native_close());
    assert!(registry.create(TRANSACTION, expected.clone()).is_err());
    // 真正发生新加载意味着固定契约失效；native适配器必须退出该manager。
    authority.started(&expected);
    assert!(binding
        .admit(&table, &context, &authority.test_headers())
        .is_err());
    assert!(!binding.blocks_native_close());
}
