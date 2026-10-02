use crate::version_history::manager_entry::{classify, DesktopEntryRequest};
use std::ffi::{OsStr, OsString};

fn arguments(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

// distinct manager只能接受规范UUID，不能落回普通App或接受路径、PID、任意附加参数。
#[test]
fn HistoryManagerEntry_ExactGrammar_001() {
    let id = "11111111-1111-4111-8111-111111111111";
    let parsed = classify(
        OsStr::new("cc-desk-version-manager.exe"),
        &arguments(&["manager.exe", "--version-manager", id]),
    )
    .unwrap();
    let DesktopEntryRequest::Manager(request) = parsed else {
        panic!("manager route required")
    };
    assert_eq!(request.transaction_id(), id);
    for values in [
        arguments(&["manager.exe"]),
        arguments(&["manager.exe", "--check-conpty", "report.json"]),
        arguments(&[
            "manager.exe",
            "--version-manager",
            "C:\\private\\transaction",
        ]),
        arguments(&["manager.exe", "--version-manager", "1234"]),
        arguments(&["manager.exe", "--version-manager", id, "--source-pid", "12"]),
        arguments(&[
            "manager.exe",
            "--version-manager",
            "11111111111141118111111111111111",
        ]),
    ] {
        assert!(classify(OsStr::new("cc-desk-version-manager.exe"), &values).is_err());
    }
}

// 普通可执行文件不能只凭参数进入manager；其它既有目录参数不改变普通启动模式。
#[test]
fn HistoryManagerEntry_OriginalImageCannotSelectManager_002() {
    let id = "11111111-1111-4111-8111-111111111111";
    assert!(classify(
        OsStr::new("cc-desk.exe"),
        &arguments(&["cc-desk.exe", "--version-manager", id])
    )
    .is_err());
    assert!(classify(
        OsStr::new("cc-desk.exe"),
        &arguments(&["cc-desk.exe", "--version-manager=anything"])
    )
    .is_err());
    assert!(matches!(
        classify(
            OsStr::new("cc-desk.exe"),
            &arguments(&["cc-desk.exe", "C:\\Project"])
        )
        .unwrap(),
        DesktopEntryRequest::Ordinary
    ));
}
