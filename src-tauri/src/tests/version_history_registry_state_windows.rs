//! Complete isolated app-registration capture, never the real product keys.
use crate::version_history::windows::{
    files::Directory, registration_state::HeldRegistrationState,
};
use windows::Win32::System::Registry::*;
use windows_core::PCWSTR;
struct Fixture {
    path: Vec<u16>,
    root: HKEY,
}
impl Fixture {
    fn new() -> Self {
        let name = format!(
            "Software\\CCDeskRegistryStateTests\\{}",
            uuid::Uuid::new_v4()
        );
        let path: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let mut root = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut root,
                None,
            )
            .ok()
            .unwrap();
        }
        Self { path, root }
    }
    fn key(&self, path: &str) -> HKEY {
        let p: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        unsafe {
            RegCreateKeyExW(
                self.root,
                PCWSTR(p.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                Some(&mut disposition),
            )
            .ok()
            .unwrap();
        }
        if disposition == REG_CREATED_NEW_KEY {
            use windows::Win32::{
                Foundation::{LocalFree, HLOCAL},
                Security::{
                    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
                },
            };
            // An elevated test token may default ownership to Administrators.
            // Create actual user-owned fixture data; do not fake token elevation.
            let user = CurrentUser::capture().unwrap();
            let owner: Vec<_> = format!("O:{}", user.sid_text())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(owner.as_ptr()),
                    1,
                    &mut descriptor,
                    None,
                )
                .unwrap();
                RegSetKeySecurity(key, OWNER_SECURITY_INFORMATION, descriptor)
                    .ok()
                    .unwrap();
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            }
        }
        key
    }
    fn protect(&self, path: &str) {
        use windows::Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR,
            },
        };
        let user = CurrentUser::capture().unwrap();
        let text: Vec<_> = format!("D:P(A;CI;KA;;;{})", user.sid_text())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let key = self.key(path);
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                1,
                &mut descriptor,
                None,
            )
            .unwrap();
            RegSetKeySecurity(
                key,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
            .ok()
            .unwrap();
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            RegCloseKey(key).ok().unwrap();
        }
    }
    fn value(&self, path: &str, name: &str, kind: REG_VALUE_TYPE, bytes: &[u8]) {
        let key = self.key(path);
        let n: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            RegSetValueExW(key, PCWSTR(n.as_ptr()), None, kind, Some(bytes))
                .ok()
                .unwrap();
            RegCloseKey(key).ok().unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.root);
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr()));
        }
    }
}
const PRODUCT: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk";
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
// 检查未知subkey和原始类型字节被完整保留，Run只纳入CC Desk，其他值不是恢复目标。
#[test]
fn HistoryRegistryState_CompleteCapture_001() {
    let temp = tempfile::tempdir().unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Unknown", REG_BINARY, &[0, 255, 3, 0]);
    f.value(
        &format!("{PRODUCT}\\unknown"),
        "",
        REG_DWORD_BIG_ENDIAN,
        &[1, 2, 3, 4],
    );
    f.value(RUN, "CC Desk", REG_EXPAND_SZ, &[0, 0]);
    f.value(RUN, "Other product", REG_BINARY, b"must not import");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    held.recheck().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&held.encode().unwrap()).unwrap();
    let text = serde_json::to_string(&value).unwrap();
    assert!(text.contains("Unknown"));
    assert!(text.contains("unknown"));
    assert!(!text.contains("Other product"));
    assert!(value["trees"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tree| tree["nodes"].as_array().unwrap().is_empty()));
    f.value(
        &format!("{PRODUCT}\\unknown"),
        "Later",
        REG_BINARY,
        b"changed",
    );
    assert!(held.recheck().is_err());
}
// 检查原树完整但祖先改名并在固定namespace替换后，旧观察不可继续授权。
#[test]
fn HistoryRegistryState_NamespaceChange_002() {
    let temp = tempfile::tempdir().unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"same");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let old: Vec<_> = "Software\\Microsoft"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let new: Vec<_> = "MicrosoftSaved".encode_utf16().chain(Some(0)).collect();
    unsafe {
        RegRenameKey(f.root, PCWSTR(old.as_ptr()), PCWSTR(new.as_ptr()))
            .ok()
            .unwrap();
    }
    f.value(PRODUCT, "Value", REG_BINARY, b"same");
    assert!(held.recheck().is_err());
}
// 检查记录前即拒绝数据超限和任意REG_LINK，不能丢弃未知数据后宣称完整。
#[test]
fn HistoryRegistryState_RefuseUnsupported_003() {
    let temp = tempfile::tempdir().unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Huge", REG_BINARY, &vec![0; 1024 * 1024 + 1]);
    assert!(HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap()
    )
    .is_err());
    let g = Fixture::new();
    g.value(
        "Software\\Classes",
        "SymbolicLinkValue",
        REG_LINK,
        &r"\REGISTRY\USER\another_Classes"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    assert!(HeldRegistrationState::fixture_capture(
        g.root,
        Directory::open_absolute(temp.path()).unwrap()
    )
    .is_err());
}

use crate::version_history::{
    journal::{
        CapacityPlan, EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase,
        JournalStore, ManifestRole, Observation, ObservedResult, RootKind,
    },
    windows::{
        files::{ComponentName, PrivateDirectory},
        lease::LeaseFiles,
        registration_state::{probe_registration_fault, RegistrationFault, RegistrationJournal},
        security::CurrentUser,
    },
};
use std::{ffi::OsStr, sync::Arc};
fn binding() -> JournalBinding {
    JournalBinding {
        transaction_id: uuid::Uuid::new_v4().to_string(),
        source_context: uuid::Uuid::new_v4().to_string(),
        target_context: uuid::Uuid::new_v4().to_string(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    }
}
fn applied(store: &mut JournalStore, generation: &mut u64, kind: EffectKind, digest: &str) {
    let id = uuid::Uuid::new_v4().to_string();
    *generation = store
        .append(
            *generation,
            JournalEvent::Intent {
                effect: EffectSpec {
                    effect_id: id.clone(),
                    kind,
                    before: digest.into(),
                    expected_postconditions: digest.into(),
                },
            },
        )
        .unwrap();
    let intent = *generation;
    let receipt = store
        .retain_effect_receipt(&id, Observation::Applied, digest)
        .unwrap();
    *generation = store
        .append(
            *generation,
            JournalEvent::Observed {
                effect_id: id,
                intent_generation: intent,
                result: ObservedResult {
                    observation: Observation::Applied,
                    receipt: Some(receipt),
                },
            },
        )
        .unwrap();
}
fn restoring(store: &mut JournalStore, binding: &JournalBinding, generation: &mut u64) {
    let digest = store.retain_manifest(b"{\"fixture\":true}").unwrap();
    for role in [
        ManifestRole::SourceContext,
        ManifestRole::RetainedTargetContext,
    ] {
        *generation = store
            .append(
                *generation,
                JournalEvent::Manifest {
                    role,
                    digest: digest.clone(),
                },
            )
            .unwrap();
    }
    *generation = store
        .append(
            *generation,
            JournalEvent::Phase {
                phase: JournalPhase::RecoveryRequired,
            },
        )
        .unwrap();
    applied(store, generation, EffectKind::FenceHistoricalImage, &digest);
    for root in [RootKind::Desk, RootKind::WebView] {
        applied(
            store,
            generation,
            EffectKind::PreserveRoot {
                context: binding.target_context.clone(),
                root,
            },
            &digest,
        );
    }
    *generation = store
        .append(
            *generation,
            JournalEvent::Phase {
                phase: JournalPhase::Restoring,
            },
        )
        .unwrap();
}
fn private(path: &std::path::Path, user: &CurrentUser) -> Arc<PrivateDirectory> {
    Arc::new(
        PrivateDirectory::create_new(
            Directory::open_absolute(path).unwrap(),
            ComponentName::new(OsStr::new("private")).unwrap(),
            user,
        )
        .unwrap(),
    )
}
// 检查实际Windows journal保留完整原状态和冲突，再逐entry恢复；共享Run其他值完整保留。
#[test]
fn HistoryRegistryState_RetainRestore_004() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Unknown", REG_BINARY, b"source");
    f.protect(PRODUCT);
    f.value(
        &format!("{PRODUCT}\\child"),
        "Number",
        REG_DWORD_BIG_ENDIAN,
        &[1, 2, 3, 4],
    );
    let source_run: Vec<_> = format!("\"{}\"", temp.path().join("cc-desk.exe").display())
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    f.value(RUN, "CC Desk", REG_SZ, &source_run);
    f.value(RUN, "Other product", REG_BINARY, b"other");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let source = held.encode().unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    f.value(PRODUCT, "Unknown", REG_BINARY, b"later");
    f.value(PRODUCT, "Later", REG_BINARY, b"keep evidence");
    let later_run: Vec<_> = format!("\"{}\"", temp.path().join("CC-DESK.EXE").display())
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    f.value(RUN, "CC Desk", REG_SZ, &later_run);
    f.value(RUN, "Other product", REG_BINARY, b"other changed");
    restoring(&mut store, &binding, &mut generation);
    let mut journal = RegistrationJournal::new(
        &mut store,
        root.clone(),
        &exclusive,
        binding.clone(),
        generation,
    )
    .unwrap();
    let receipt = retained.restore(&mut journal).unwrap();
    receipt.verify().unwrap();
    let preserved = receipt.preserved_current().to_owned();
    generation = journal.generation();
    drop(journal);
    assert!(String::from_utf8(store.read_manifest(&preserved).unwrap())
        .unwrap()
        .contains("Later"));
    let restored = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let original: serde_json::Value = serde_json::from_slice(&source).unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&restored.encode().unwrap()).unwrap();
    assert_eq!(original, actual);
    let key = f.key(RUN);
    let name: Vec<_> = "Other product".encode_utf16().chain(Some(0)).collect();
    let mut bytes = [0u8; 64];
    let mut count = 64;
    unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(bytes.as_mut_ptr()),
            Some(&mut count),
        )
        .ok()
        .unwrap();
        RegCloseKey(key).ok().unwrap();
    }
    assert_eq!(&bytes[..count as usize], b"other changed");
    drop(restored);
    drop(receipt);
    f.value(PRODUCT, "Unknown", REG_BINARY, b"latest external value");
    let before_retry = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
    assert!(retained.restore(&mut journal).is_err());
    let after_retry = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(before_retry, after_retry);
}
// 检查真实写入后丢失回执保留Unknown和原始冲突artifact，不能重试或伪造NotApplied。
#[test]
fn HistoryRegistryState_UnknownNoReplay_005() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"source");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    f.value(PRODUCT, "Value", REG_BINARY, b"later");
    restoring(&mut store, &binding, &mut generation);
    let _fault = probe_registration_fault(RegistrationFault::AfterMutation);
    let mut journal = RegistrationJournal::new(
        &mut store,
        root.clone(),
        &exclusive,
        binding.clone(),
        generation,
    )
    .unwrap();
    assert!(retained.restore(&mut journal).is_err());
    generation = journal.generation();
    drop(journal);
    let state = store.inspect(&binding).unwrap().last_valid.unwrap();
    let pending = state.pending_effect().unwrap();
    assert_eq!(
        state.effect_observation(&pending.effect_id),
        Some(Observation::Unknown)
    );
    assert!(!store.read_manifest(retained.digest()).unwrap().is_empty());
    drop(state);
    drop(store);
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store.bind_existing(&binding).unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
    let reopened=crate::version_history::windows::registration_state::RetainedRegistrationState::fixture_reopen(
        &mut journal,Directory::open_absolute(temp.path()).unwrap(),f.root).unwrap();
    assert!(reopened.restore(&mut journal).is_err());
}

// 检查原始缺失不会删共享Run树；原始存在但当前缺失则按保留的字节/权限重建。
#[test]
fn HistoryRegistryState_AbsenceAndMissingTarget_006() {
    for source_present in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let user = CurrentUser::capture().unwrap();
        let root = private(temp.path(), &user);
        let leases = LeaseFiles::open(root.clone(), &user).unwrap();
        let control = leases.acquire_control().unwrap();
        let exclusive = leases.acquire_exclusive(&control).unwrap();
        let binding = binding();
        let mut store = JournalStore::open_windows(root.clone()).unwrap();
        store
            .initialize(
                binding.clone(),
                CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
            )
            .unwrap();
        let f = Fixture::new();
        if source_present {
            f.value(PRODUCT, "Value", REG_BINARY, b"source");
        }
        let held = HeldRegistrationState::fixture_capture(
            f.root,
            Directory::open_absolute(temp.path()).unwrap(),
        )
        .unwrap();
        let mut journal =
            RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0)
                .unwrap();
        let retained = held.retain(&mut journal).unwrap();
        let mut generation = journal.generation();
        drop(journal);
        if source_present {
            let path: Vec<_> = PRODUCT.encode_utf16().chain(Some(0)).collect();
            unsafe {
                RegDeleteTreeW(f.root, PCWSTR(path.as_ptr())).ok().unwrap();
            }
        } else {
            f.value(PRODUCT, "Later", REG_BINARY, b"preserve later");
            let later_run: Vec<_> = format!("\"{}\"", temp.path().join("CC-DESK.EXE").display())
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            f.value(RUN, "CC Desk", REG_SZ, &later_run);
            f.value(RUN, "Other product", REG_BINARY, b"unrelated");
        }
        restoring(&mut store, &binding, &mut generation);
        let mut journal =
            RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
        let receipt = retained.restore(&mut journal).unwrap();
        receipt.verify().unwrap();
        let actual: serde_json::Value = serde_json::from_slice(
            &HeldRegistrationState::fixture_capture(
                f.root,
                Directory::open_absolute(temp.path()).unwrap(),
            )
            .unwrap()
            .encode()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            actual["trees"][0]["nodes"].as_array().unwrap().is_empty(),
            !source_present
        );
        assert!(actual["run"]["value"].is_null());
    }
}
// 检查同binding和相同artifact复制到另一个实际私有根，不能挪用原保留能力进行恢复。
#[test]
fn HistoryRegistryState_ForeignJournalRoot_007() {
    use crate::version_history::windows::registration_state::RetainedRegistrationState;
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"source");
    let directory = Directory::open_absolute(temp.path()).unwrap();
    let held = HeldRegistrationState::fixture_capture(f.root, directory.clone()).unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let reopened =
        RetainedRegistrationState::fixture_reopen(&mut journal, directory.clone(), f.root).unwrap();
    assert_eq!(reopened.digest(), retained.digest());
    drop(journal);
    let bytes = store.read_manifest(retained.digest()).unwrap();
    let foreign = Arc::new(
        PrivateDirectory::create_new(
            directory.clone(),
            ComponentName::new(OsStr::new("foreign")).unwrap(),
            &user,
        )
        .unwrap(),
    );
    let foreign_leases = LeaseFiles::open(foreign.clone(), &user).unwrap();
    let foreign_control = foreign_leases.acquire_control().unwrap();
    let foreign_exclusive = foreign_leases.acquire_exclusive(&foreign_control).unwrap();
    let mut other = JournalStore::open_windows(foreign.clone()).unwrap();
    other
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    assert_eq!(other.retain_manifest(&bytes).unwrap(), retained.digest());
    let generation = other
        .append(
            0,
            JournalEvent::Manifest {
                role: ManifestRole::Registration,
                digest: retained.digest().into(),
            },
        )
        .unwrap();
    let mut other_journal =
        RegistrationJournal::new(&mut other, foreign, &foreign_exclusive, binding, generation)
            .unwrap();
    assert!(
        RetainedRegistrationState::fixture_reopen(&mut other_journal, directory, f.root).is_err()
    );
    assert!(retained.restore(&mut other_journal).is_err());
}
// 检查冲突artifact实际partial-write失败会阻止任何registry修改，先保留再覆盖不能反序。
#[test]
fn HistoryRegistryState_ConflictRetentionFailure_008() {
    use crate::version_history::windows::durability::{
        probe_persistence_fault, PersistenceBoundary, PersistenceOperation,
    };
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"source");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    f.value(PRODUCT, "Value", REG_BINARY, b"unretained latest");
    let before = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    restoring(&mut store, &binding, &mut generation);
    let _fault = probe_persistence_fault(
        PersistenceOperation::Artifact,
        PersistenceBoundary::PartialWrite,
    );
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
    assert!(retained.restore(&mut journal).is_err());
    drop(journal);
    let after = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(before, after);
    assert!(store.read_manifest(retained.digest()).is_ok());
}

// 检查保留名下的第三方Run命令只可保存为冲突，不能因为叫CC Desk就被覆盖。
#[test]
fn HistoryRegistryState_ForeignRunConflict_009() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"source");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    let foreign: Vec<_> = format!("\"{}\"", temp.path().join("unrelated.exe").display())
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    f.value(RUN, "CC Desk", REG_SZ, &foreign);
    f.value(PRODUCT, "Value", REG_BINARY, b"later source data");
    let before = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    restoring(&mut store, &binding, &mut generation);
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
    assert!(retained.restore(&mut journal).is_err());
    drop(journal);
    let after = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(before, after);
    let expected: serde_json::Value = serde_json::from_slice(&before).unwrap();
    let retained_conflict = std::fs::read_dir(temp.path().join("private"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("manifest-"))
        .any(|entry| {
            std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|value| value.get(2) == Some(&expected))
        });
    assert!(retained_conflict);
}
// 检查source Run只接受引用实际安装目录的固定quoted exe无参数语法，未知变体先保留再拒绝。
#[test]
fn HistoryRegistryState_RunGrammar_010() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    let path = temp.path().join("cc-desk.exe");
    for (kind, command) in [
        (REG_SZ, path.display().to_string()),
        (REG_SZ, format!("\"{}\" --custom", path.display())),
        (REG_EXPAND_SZ, format!("\"{}\"", path.display())),
        (
            REG_SZ,
            format!("\"{}\"", temp.path().join("other.exe").display()),
        ),
    ] {
        let bytes: Vec<_> = command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        f.value(RUN, "CC Desk", kind, &bytes);
        let held = HeldRegistrationState::fixture_capture(
            f.root,
            Directory::open_absolute(temp.path()).unwrap(),
        )
        .unwrap();
        let mut journal =
            RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0)
                .unwrap();
        assert!(held.retain(&mut journal).is_err());
        drop(journal);
        assert!(store
            .inspect(&binding)
            .unwrap()
            .last_valid
            .unwrap()
            .manifest(ManifestRole::Registration)
            .is_none());
    }
    let bytes: Vec<_> = format!("\"{}\"", path.display())
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    f.value(RUN, "CC Desk", REG_SZ, &bytes);
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal = RegistrationJournal::new(&mut store, root, &exclusive, binding, 0).unwrap();
    assert!(held.retain(&mut journal).is_ok());
}

// 检查六个owned tree的共享父ACL改变仍可写时，先保留冲突再拒绝，不能先删除任何原始/后来数据。
#[test]
fn HistoryRegistryState_SharedParentDrift_012() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    let shared_parent = PRODUCT.rsplit_once('\\').unwrap().0;
    f.protect(shared_parent);
    f.value(PRODUCT, "Value", REG_BINARY, b"original");
    let installation = Directory::open_absolute(temp.path()).unwrap();
    let held = HeldRegistrationState::fixture_capture(f.root, installation.clone()).unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    f.value(PRODUCT, "Value", REG_BINARY, b"later protected data");
    let key = f.key(shared_parent);
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };
    let changed: Vec<_> = format!("D:P(A;CI;KA;;;{})(A;CI;KR;;;WD)", user.sid_text())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(changed.as_ptr()),
            1,
            &mut descriptor,
            None,
        )
        .unwrap();
        RegSetKeySecurity(
            key,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
        .ok()
        .unwrap();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        RegCloseKey(key).ok().unwrap();
    }
    let before = HeldRegistrationState::fixture_capture(f.root, installation.clone())
        .unwrap()
        .encode()
        .unwrap();
    restoring(&mut store, &binding, &mut generation);
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding.clone(), generation)
            .unwrap();
    assert!(retained.restore(&mut journal).is_err());
    assert_eq!(journal.generation(), generation);
    drop(journal);
    let state = store.inspect(&binding).unwrap().last_valid.unwrap();
    assert!(state.pending_effect().is_none());
    assert_eq!(
        HeldRegistrationState::fixture_capture(f.root, installation)
            .unwrap()
            .encode()
            .unwrap(),
        before
    );
    let expected: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert!(std::fs::read_dir(temp.path().join("private"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("manifest-"))
        .any(|entry| std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|value| value.get(2) == Some(&expected))));
}

// 检查source封存边界重新观察原始状态，返回后不遗留会阻止NSIS替换的产品key句柄。
#[test]
fn HistoryRegistryState_VerifyOriginal_013() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"original");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal = RegistrationJournal::new(&mut store, root, &exclusive, binding, 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    retained.verify_original(&mut journal).unwrap();
    let path: Vec<_> = PRODUCT.encode_utf16().chain(Some(0)).collect();
    unsafe {
        RegDeleteTreeW(f.root, PCWSTR(path.as_ptr())).ok().unwrap();
    }
    f.value(PRODUCT, "Value", REG_BINARY, b"replacement");
    assert!(retained.verify_original(&mut journal).is_err());
}

// 检查完整回退计划超出实际剩余recovery预算时，在第一条修改intent前拒绝且保留当前数据。
#[test]
fn HistoryRegistryState_CapacityBeforeMutation_011() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 1, 10, 4096).unwrap(),
        )
        .unwrap();
    let f = Fixture::new();
    f.value(PRODUCT, "Value", REG_BINARY, b"source");
    let held = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = held.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    f.value(PRODUCT, "Value", REG_BINARY, b"later must remain");
    let before = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    restoring(&mut store, &binding, &mut generation);
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding.clone(), generation)
            .unwrap();
    assert!(retained.restore(&mut journal).is_err());
    drop(journal);
    let after = HeldRegistrationState::fixture_capture(
        f.root,
        Directory::open_absolute(temp.path()).unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(before, after);
    assert!(store
        .inspect(&binding)
        .unwrap()
        .last_valid
        .unwrap()
        .pending_effect()
        .is_none());
}

// 检查完成核对只读接受恢复后合法保留的共享父键，且拒绝缺失回执及之后的修改。
#[test]
fn HistoryRegistryState_CompletedReadback_014() {
    let temp = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let root = private(temp.path(), &user);
    let leases = LeaseFiles::open(root.clone(), &user).unwrap();
    let control = leases.acquire_control().unwrap();
    let exclusive = leases.acquire_exclusive(&control).unwrap();
    let binding = binding();
    let mut store = JournalStore::open_windows(root.clone()).unwrap();
    store
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(100, 100, 100, 16384).unwrap(),
        )
        .unwrap();
    let fixture = Fixture::new();
    let installation = Directory::open_absolute(temp.path()).unwrap();
    let source =
        HeldRegistrationState::fixture_capture(fixture.root, installation.clone()).unwrap();
    let original = source.encode().unwrap();
    let mut journal =
        RegistrationJournal::new(&mut store, root.clone(), &exclusive, binding.clone(), 0).unwrap();
    let retained = source.retain(&mut journal).unwrap();
    let mut generation = journal.generation();
    drop(journal);
    fixture.value(PRODUCT, "Later", REG_BINARY, b"installer registration");
    restoring(&mut store, &binding, &mut generation);
    let journal_bytes = || std::fs::read(temp.path().join("private/journal.log")).unwrap();
    let record_count = || {
        std::fs::read_dir(temp.path().join("private"))
            .unwrap()
            .count()
    };
    let before = (journal_bytes(), record_count());
    let mut journal =
        RegistrationJournal::new(&mut store, root, &exclusive, binding, generation).unwrap();
    assert!(retained.reopen_completed(&mut journal).is_err());
    assert_eq!(generation, journal.generation());
    assert_eq!(before, (journal_bytes(), record_count()));
    let receipt = retained.restore(&mut journal).unwrap();
    receipt.verify().unwrap();
    drop(receipt);

    let generation = journal.generation();
    let before = (journal_bytes(), record_count());
    let reopened = retained.reopen_completed(&mut journal).unwrap();
    reopened.recheck().unwrap();
    assert_ne!(
        original,
        reopened.encode().unwrap(),
        "shared parents legitimately remain"
    );
    assert!(retained.verify_original(&mut journal).is_err());
    assert_eq!(generation, journal.generation());
    assert_eq!(before, (journal_bytes(), record_count()));

    fixture.value(
        PRODUCT,
        "Unexpected",
        REG_BINARY,
        b"changed after completion",
    );
    assert!(reopened.recheck().is_err());
    drop(reopened);
    let changed = HeldRegistrationState::fixture_capture(fixture.root, installation.clone())
        .unwrap()
        .encode()
        .unwrap();
    assert!(retained.reopen_completed(&mut journal).is_err());
    assert_eq!(generation, journal.generation());
    assert_eq!(before, (journal_bytes(), record_count()));
    assert_eq!(
        changed,
        HeldRegistrationState::fixture_capture(fixture.root, installation)
            .unwrap()
            .encode()
            .unwrap()
    );
}
