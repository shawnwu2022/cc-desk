use crate::cli::{
    document::{DocumentAuthority, NativeContext},
    run_registry::RunRegistry,
};
use std::sync::Arc;
use tauri::{ResourceTable, Url};

const TRANSACTION: &str = "11111111-1111-4111-8111-111111111111";
fn url() -> Url {
    "http://tauri.localhost/".parse().unwrap()
}

// 无维护pin时，普通重载仍撤销原document，与既有行为一致。
#[test]
fn HistorySourceDocument_OrdinaryNavigation_001() {
    let authority = DocumentAuthority::<()>::new(Arc::new(RunRegistry::new(8)), url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = Arc::new(authority.attach(&mut table).unwrap());
    authority.started(&url());
    authority.finished(&url());
    assert!(!binding.blocks_handoff_exit());
    assert!(!authority.navigation(&url()));
    assert!(!binding.observation_alive());
}

// 拒绝导航不撤销已冻结页面；真实替换事件仍撤销，阻止借同名页面接管。
#[test]
fn HistorySourceDocument_PinAndReplacement_002() {
    let registry = Arc::new(RunRegistry::<()>::new(8));
    let authority = DocumentAuthority::new(registry.clone(), url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = Arc::new(authority.attach(&mut table).unwrap());
    authority.started(&url());
    authority.finished(&url());
    let pin = binding
        .pin_handoff(&authority.test_caller(), TRANSACTION)
        .unwrap();
    assert!(binding.blocks_handoff_exit());
    assert!(!authority.navigation(&url()));
    pin.verify().unwrap();
    let current_url = url();
    binding
        .admit(
            &table,
            &NativeContext {
                window_label: "main",
                webview_label: "main",
                url: &current_url,
            },
            &authority.test_headers(),
        )
        .unwrap();
    authority.started(&url());
    assert!(pin.verify().is_err());
    assert!(binding.blocks_handoff_exit());
    assert!(pin.release_review().is_err());
    // No publication occurred; abandoning this old capture releases only its pin.
    assert!(!binding.blocks_handoff_exit());
    let next = DocumentAuthority::new(registry, url()).unwrap();
    let mut next_table = ResourceTable::default();
    let newer = Arc::new(next.attach(&mut next_table).unwrap());
    next.started(&url());
    next.finished(&url());
    assert!(!newer.blocks_handoff_exit());
    assert!(newer
        .pin_handoff(&authority.test_caller(), TRANSACTION)
        .is_err());
}

// 取消仅解除同一原文档pin，不创建新owner或默认放行旧任务。
#[test]
fn HistorySourceDocument_ExactReviewRelease_003() {
    let authority = DocumentAuthority::<()>::new(Arc::new(RunRegistry::new(8)), url()).unwrap();
    let mut table = ResourceTable::default();
    let binding = Arc::new(authority.attach(&mut table).unwrap());
    authority.started(&url());
    authority.finished(&url());
    let pin = binding
        .pin_handoff(&authority.test_caller(), TRANSACTION)
        .unwrap();
    assert!(binding
        .pin_handoff(&authority.test_caller(), TRANSACTION)
        .is_err());
    pin.release_review().unwrap();
    assert!(!binding.blocks_handoff_exit());
    assert!(binding.observation_alive());
}
