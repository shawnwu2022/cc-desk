//! Exact token-bound OS Classes alias policy; no user registration mutations.
use crate::version_history::windows::{
    registry::{
        current_user_classes_alias, link_diagnostic, RegistrationKey, RegistrationSlot,
        RegistryValue, RegistryView,
    },
    security::CurrentUser,
};
use windows::Win32::System::Registry::*;
fn value(target: &str) -> RegistryValue {
    RegistryValue {
        kind: REG_LINK.0,
        bytes: target.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    }
}
// 检查仅当前token SID的Classes目标可为四个固定产品子树提供显式canonical遍历。
#[test]
fn HistoryClassesAlias_ExactPolicy_001() {
    run_policy_probe();
}
#[cfg(test)]
pub(crate) fn run_policy_probe() {
    let user = CurrentUser::capture().unwrap();
    let target = format!(r"\REGISTRY\USER\{}_Classes", user.sid_text());
    let good = value(&target);
    let unelevated = user.require_unelevated().is_ok();
    for path in [
        "Software\\Classes\\Directory\\shell\\cc-desk",
        "Software\\Classes\\Directory\\Background\\shell\\cc-desk",
        "Software\\Classes\\Directory\\shell\\cc-box",
        "Software\\Classes\\Directory\\Background\\shell\\cc-box\\command",
    ] {
        let result = current_user_classes_alias(HKEY_CURRENT_USER, path, 1, &good);
        if unelevated {
            assert!(result.unwrap().is_some());
        } else {
            assert!(result.is_err());
        }
        assert!(
            current_user_classes_alias(HKEY_LOCAL_MACHINE, path, 1, &good)
                .unwrap()
                .is_none()
        );
        assert!(
            current_user_classes_alias(HKEY_CURRENT_USER, path, 0, &good)
                .unwrap()
                .is_none()
        );
    }
    for path in [
        "Software\\Classes",
        "Software\\Classes\\Directory\\shell\\foreign",
        "Software\\Classes\\Directory\\shell\\cc-desk-extra",
        "Software\\Classes\\Directory\\shell\\cc-desk\\..\\foreign",
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk",
    ] {
        assert!(
            current_user_classes_alias(HKEY_CURRENT_USER, path, 1, &good)
                .unwrap()
                .is_none()
        );
    }
    for bad in [
        format!(r"\REGISTRY\USER\{}-foreign_Classes", user.sid_text()),
        format!("{target}\\Other"),
        r"\REGISTRY\MACHINE\SOFTWARE\Classes".into(),
    ] {
        let result = current_user_classes_alias(
            HKEY_CURRENT_USER,
            "Software\\Classes\\Directory\\shell\\cc-desk",
            1,
            &value(&bad),
        );
        if unelevated {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
    }
    let mut wrong_kind = good.clone();
    wrong_kind.kind = REG_SZ.0;
    assert!(current_user_classes_alias(
        HKEY_CURRENT_USER,
        "Software\\Classes\\Directory\\shell\\cc-desk",
        1,
        &wrong_kind
    )
    .unwrap()
    .is_none());
    let message = link_diagnostic(
        HKEY_CURRENT_USER,
        RegistryView::View32,
        "Classes",
        &good.bytes,
    );
    assert!(message.contains("component=classes"));
    assert!(message.contains("target=currentUserClasses"));
    assert!(!message.contains(user.sid_text()));
}
// 检查真实OS的两种view均能只打开固定产品leaf或报告缺失，不跟随任意alias，也不写值。
#[test]
fn HistoryClassesAlias_ActualFixedSlots_002() {
    let user = CurrentUser::capture().unwrap();
    if user.require_unelevated().is_err() {
        let target = value(&format!(r"\REGISTRY\USER\{}_Classes", user.sid_text()));
        assert!(current_user_classes_alias(
            HKEY_CURRENT_USER,
            "Software\\Classes\\Directory\\shell\\cc-desk",
            1,
            &target
        )
        .is_err());
        // Actual fixed-slot positives run in the Medium worker's require_absent
        // and before/after inventory, under its independently verified token.
        return;
    }
    for view in [RegistryView::View32, RegistryView::View64] {
        for slot in [
            RegistrationSlot::Directory,
            RegistrationSlot::Background,
            RegistrationSlot::LegacyDirectory,
            RegistrationSlot::LegacyBackground,
        ] {
            let _ = RegistrationKey::open(slot, view, "").unwrap();
        }
    }
}
// 检查独立volatile fixture中被保留的alias值改变后真实held guard拒绝，不改真实Classes。
#[test]
fn HistoryClassesAlias_HeldDrift_003() {
    use crate::version_history::windows::registry::fixture_alias_guard;
    use windows_core::PCWSTR;
    let text = format!("Software\\CCDeskAliasTests\\{}", uuid::Uuid::new_v4());
    let path: Vec<_> = text.encode_utf16().chain(Some(0)).collect();
    let mut root = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut root,
            None,
        )
        .ok()
        .unwrap();
    }
    let alias: Vec<_> = "Alias".encode_utf16().chain(Some(0)).collect();
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(alias.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut key,
            None,
        )
        .ok()
        .unwrap();
    }
    let name: Vec<_> = "SymbolicLinkValue".encode_utf16().chain(Some(0)).collect();
    let user = CurrentUser::capture().unwrap();
    let original = value(&format!(r"\REGISTRY\USER\{}_Classes", user.sid_text()));
    unsafe {
        RegSetValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            REG_LINK,
            Some(&original.bytes),
        )
        .ok()
        .unwrap();
    }
    let probe = fixture_alias_guard(root, "Alias").unwrap();
    probe.verify().unwrap();
    let foreign = value(r"\REGISTRY\USER\foreign_Classes");
    unsafe {
        RegSetValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            REG_LINK,
            Some(&foreign.bytes),
        )
        .ok()
        .unwrap();
    }
    assert!(probe.verify().is_err());
    drop(probe);
    unsafe {
        RegCloseKey(key).ok().unwrap();
        RegCloseKey(root).ok().unwrap();
        RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()))
            .ok()
            .unwrap();
    }
    for view in [RegistryView::View32, RegistryView::View64] {
        assert!(RegistrationKey::open(RegistrationSlot::Directory, view, "..\\foreign").is_err());
    }
}
