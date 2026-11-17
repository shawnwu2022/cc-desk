#![allow(non_snake_case)]
use super::*;
use crate::version_history::windows::pairing_store::WindowsPairingBackend;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
/// TEST_ONLY: no DPAPI, native key API, network or user credential access.
pub(crate) struct TestSealer;
impl Sealer for TestSealer {
    fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok([b"TEST_ONLY/".as_slice(), bytes].concat())
    }
    fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        bytes
            .strip_prefix(b"TEST_ONLY/")
            .map(|b| b.to_vec())
            .ok_or(StoreError::Invalid)
    }
}
pub(crate) type TestStore = PairingStore<WindowsPairingBackend, TestSealer>;
pub(crate) fn synthetic_file_store(parent: &Path) -> Result<TestStore> {
    PairingStore::new(WindowsPairingBackend::create_new(parent)?, TestSealer)
}
pub(crate) fn reopen_synthetic_file_store(parent: &Path) -> Result<TestStore> {
    PairingStore::new(WindowsPairingBackend::open_existing(parent)?, TestSealer)
}
struct TestAuthorization {
    target: StoreTarget,
    expected: Mutation,
    checks: usize,
    fail_at: usize,
}
impl MutationAuthorization for TestAuthorization {
    fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<()> {
        self.checks += 1;
        if !target.same_target(&self.target)
            || *mutation != self.expected
            || self.checks == self.fail_at
        {
            Err(StoreError::Unauthorized)
        } else {
            Ok(())
        }
    }
}
fn apply<B: Backend>(
    store: &PairingStore<B, TestSealer>,
    mutation: Mutation,
) -> Result<LockedSnapshot> {
    let mut auth = TestAuthorization {
        target: store.target(),
        expected: mutation.clone(),
        checks: 0,
        fail_at: usize::MAX,
    };
    store.apply(mutation, &mut auth)
}
fn directory() -> tempfile::TempDir {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".audit")
        .join("native-pairing-tests");
    std::fs::create_dir_all(&path).unwrap();
    tempfile::Builder::new()
        .prefix("synthetic-")
        .tempdir_in(path)
        .unwrap()
}
fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn key(two: bool) -> Vec<u8> {
    // Public P-256 generator points only; no private key created or opened.
    let point = if two {
        "7cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1"
    } else {
        "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"
    };
    hex(&format!(
        "3059301306072a8648ce3d020106082a8648ce3d03010703420004{point}"
    ))
}
pub(crate) fn record() -> PublicDeviceRecord {
    PublicDeviceRecord {
        epochs: PublicEpochs {
            host_key: 2,
            client_key: 3,
            policy: 4,
            issuer: 5,
        },
        device_id: "00000000-0000-4000-8000-000000000001".into(),
        native: NativePublicIdentity {
            profile_id: [1; 16],
            profile_revision: 1,
            endpoint_key_name: "TEST_ONLY endpoint".into(),
            relay_key_name: "TEST_ONLY relay".into(),
            endpoint_spki: key(false),
            relay_spki: key(true),
        },
        browser: BrowserPublicIdentity {
            origin: "https://desk.invalid".into(),
            rp_id: "desk.invalid".into(),
            credential_id: vec![7; 32],
            webauthn_public_key: key(false),
            browser_public_key: key(true),
            grant_public_key: key(false),
            relay_public_key: key(true),
            requested_user_handle: vec![8; 32],
            backup_eligible: false,
            allow_backed_up: false,
        },
        route: PublicRoute {
            address: "127.0.0.1:443".into(),
            origin: "wss://relay.invalid/".into(),
            relay_mode: "rendezvousV1".into(),
            tunnel: [4; 16],
            relay_id: [5; 16],
        },
        principal: [6; 32],
    }
}
fn initial() -> Mutation {
    Mutation::Initialize {
        store_id: "00000000-0000-4000-8000-000000000002".into(),
        record: record(),
    }
}
fn data_path(parent: &Path) -> PathBuf {
    parent.join("remote-pairings-v1/devices.dpapi")
}
#[test]
fn NativePairing_ReloadLockedTombstone_001() {
    let dir = directory();
    assert!(matches!(
        reopen_synthetic_file_store(dir.path()),
        Err(StoreError::Missing)
    ));
    let store = synthetic_file_store(dir.path()).unwrap();
    assert_eq!(store.load(), Err(StoreError::Missing));
    let saved = apply(&store, initial()).unwrap();
    assert_eq!(saved.expected.revision, 1);
    let repeated = Mutation::Remember {
        expected: saved.expected.clone(),
        record: record(),
    };
    assert_eq!(apply(&store, repeated), Err(StoreError::Conflict));
    drop(store);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(reopened.load().unwrap(), saved);
    let public_text = serde_json::to_string(&record()).unwrap();
    for forbidden in [
        "runId",
        "session",
        "generation",
        "scope",
        "activation",
        "permit",
        "owner",
        "secret",
        "Instant",
    ] {
        assert!(!public_text.contains(forbidden));
    }
    let forgotten = apply(
        &reopened,
        Mutation::Forget {
            expected: saved.expected.clone(),
            device_id: record().device_id,
        },
    )
    .unwrap();
    assert_eq!(forgotten.devices[0].state, RecordState::Revoked);
    assert!(forgotten.devices[0].record.is_none());
    assert_eq!(
        apply(
            &reopened,
            Mutation::Remember {
                expected: saved.expected,
                record: record()
            }
        ),
        Err(StoreError::Conflict)
    );
    drop(reopened);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(reopened.load().unwrap(), forgotten);
    assert_eq!(
        apply(
            &reopened,
            Mutation::Remember {
                expected: forgotten.expected,
                record: record()
            }
        )
        .unwrap()
        .expected
        .revision,
        3
    );
}
#[test]
fn NativePairing_ExactTargetAndFinalCheck_002() {
    let a = directory();
    let b = directory();
    let store = synthetic_file_store(a.path()).unwrap();
    let other = synthetic_file_store(b.path()).unwrap();
    let mut auth = TestAuthorization {
        target: store.target(),
        expected: initial(),
        checks: 0,
        fail_at: usize::MAX,
    };
    assert_eq!(
        other.apply(initial(), &mut auth),
        Err(StoreError::Unauthorized)
    );
    auth.checks = 0;
    auth.fail_at = 2;
    assert_eq!(
        store.apply(initial(), &mut auth),
        Err(StoreError::Unauthorized)
    );
    assert_eq!(auth.checks, 2);
    assert_eq!(store.load(), Err(StoreError::Missing));
    assert!(!data_path(a.path()).exists());
    assert!(std::fs::read_dir(a.path().join("remote-pairings-v1"))
        .unwrap()
        .any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("pending-")));
    // Pending files are never a recovery source.
    drop(store);
    assert_eq!(
        reopen_synthetic_file_store(a.path()).unwrap().load(),
        Err(StoreError::Missing)
    );
}
#[test]
fn NativePairing_StrictEnvelopeAndBounds_003() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    let bytes = std::fs::read(data_path(dir.path())).unwrap();
    let text = String::from_utf8(TestSealer.unseal(&bytes).unwrap()).unwrap();
    for malformed in [
        text.replacen("\"domain\":", "\"unknown\":1,\"domain\":", 1),
        text.replacen("\"schema\":2", "\"schema\":2,\"schema\":2", 1),
        text.replacen(DOMAIN, "wrong-domain", 1),
        text.replacen("\"revision\":1", "\"revision\":2", 1),
        text.replacen("\"active\"", "\"approved\"", 1),
    ] {
        assert_eq!(
            store.decode(&TestSealer.seal(malformed.as_bytes()).unwrap()),
            Err(StoreError::Invalid)
        );
    }
    assert_eq!(
        store.decode(&vec![0; MAX_SEALED + 1]),
        Err(StoreError::Invalid)
    );
    let mut doc = store.decode(&bytes).unwrap();
    doc.devices.push(doc.devices[0].clone());
    assert_eq!(validate_document(&doc), Err(StoreError::Invalid));
    doc.devices.pop();
    doc.devices[0]
        .record
        .as_mut()
        .unwrap()
        .browser
        .allow_backed_up = true;
    assert_eq!(validate_document(&doc), Err(StoreError::Invalid));
    let mut invalid = record();
    invalid.native.endpoint_key_name = "..".into();
    assert_eq!(invalid.validate(), Err(StoreError::Invalid));
    invalid = record();
    invalid.browser.credential_id = vec![1; 1025];
    assert_eq!(invalid.validate(), Err(StoreError::Invalid));
    invalid = record();
    invalid.route.origin = "https://desk.invalid/".into();
    assert_eq!(invalid.validate(), Err(StoreError::Invalid));
    // Real DPAPI is hard-disabled in all test builds, including accidental calls.
    assert_eq!(
        CurrentUserDpapi.seal(b"TEST_ONLY"),
        Err(StoreError::Unauthorized)
    );
}
#[test]
fn NativePairing_CorruptNeverRepairs_004() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    drop(store);
    std::fs::write(data_path(dir.path()), b"broken").unwrap();
    let store = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(store.load(), Err(StoreError::Invalid));
    assert_eq!(apply(&store, initial()), Err(StoreError::Conflict));
    assert_eq!(std::fs::read(data_path(dir.path())).unwrap(), b"broken");
}
#[test]
fn NativePairing_RevisionOverflow_005() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    let mut doc = store
        .decode(&std::fs::read(data_path(dir.path())).unwrap())
        .unwrap();
    doc.revision = u64::MAX;
    let bytes = store.encode(&doc).unwrap();
    drop(store);
    std::fs::write(data_path(dir.path()), bytes).unwrap();
    let store = reopen_synthetic_file_store(dir.path()).unwrap();
    let expected = store.load().unwrap().expected;
    assert_eq!(
        apply(
            &store,
            Mutation::Forget {
                expected,
                device_id: record().device_id
            }
        ),
        Err(StoreError::Invalid)
    );
}
#[test]
fn NativePairing_CrossProcessCas_006() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    let spawn = || {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "remote_native_pairing_store::tests::NativePairing_SubprocessWriter_007",
                "--nocapture",
            ])
            .env("CC_TEST_ONLY_PAIRING_CHILD", dir.path())
            .env("TEMP", dir.path())
            .env("TMP", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn();
    let second = spawn();
    let one = first.wait_with_output().unwrap();
    let two = second.wait_with_output().unwrap();
    for output in [&one, &two] {
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&one.stdout),
        String::from_utf8_lossy(&two.stdout)
    );
    assert_eq!(all.matches("PAIRING_COMMITTED").count(), 1);
    assert_eq!(all.matches("PAIRING_CONFLICT").count(), 1);
    assert_eq!(store.load().unwrap().expected.revision, 2);
}
#[test]
fn NativePairing_SubprocessWriter_007() {
    let Some(path) = std::env::var_os("CC_TEST_ONLY_PAIRING_CHILD") else {
        return;
    };
    let store = reopen_synthetic_file_store(Path::new(&path)).unwrap();
    let expected = ExpectedStore {
        store_id: "00000000-0000-4000-8000-000000000002".into(),
        revision: 1,
    };
    for _ in 0..100 {
        match apply(
            &store,
            Mutation::Forget {
                expected: expected.clone(),
                device_id: record().device_id,
            },
        ) {
            Ok(_) => {
                println!("PAIRING_COMMITTED");
                return;
            }
            Err(StoreError::Conflict) => {
                println!("PAIRING_CONFLICT");
                return;
            }
            Err(StoreError::Busy) => std::thread::sleep(std::time::Duration::from_millis(10)),
            other => panic!("unexpected child result: {other:?}"),
        }
    }
    panic!("synthetic child lock timeout");
}
struct UncertainBackend;
struct UncertainTransaction;
impl Backend for UncertainBackend {
    type Guard = UncertainTransaction;
    fn begin(&self) -> Result<Self::Guard> {
        Ok(UncertainTransaction)
    }
    fn target_label(&self) -> Result<String> {
        Ok("TEST_ONLY uncertain".into())
    }
}
impl Transaction for UncertainTransaction {
    fn read(&mut self) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn commit(
        &mut self,
        _: &[u8],
        _: bool,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        authorize()?;
        Err(StoreError::Unknown)
    }
}
#[test]
fn NativePairing_UnknownPoisonsNoReplay_008() {
    let store = PairingStore::new(UncertainBackend, TestSealer).unwrap();
    let mut auth = TestAuthorization {
        target: store.target(),
        expected: initial(),
        checks: 0,
        fail_at: usize::MAX,
    };
    assert_eq!(store.apply(initial(), &mut auth), Err(StoreError::Unknown));
    assert_eq!(store.apply(initial(), &mut auth), Err(StoreError::Poisoned));
    assert_eq!(store.load(), Err(StoreError::Poisoned));
    assert_eq!(auth.checks, 2);
}
#[test]
fn NativePairing_RealPublicationUnknown_009() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    let saved = apply(&store, initial()).unwrap();
    store.backend.test_fail_after_publish();
    let forget = Mutation::Forget {
        expected: saved.expected,
        device_id: record().device_id,
    };
    assert_eq!(apply(&store, forget.clone()), Err(StoreError::Unknown));
    assert_eq!(apply(&store, forget), Err(StoreError::Poisoned));
    assert_eq!(store.load(), Err(StoreError::Poisoned));
    drop(store);
    // Reopening is reconciliation into Locked suggestions, not retry of the old
    // approval. The actual strict ReplaceFileW effect is visible as a tombstone.
    let store = reopen_synthetic_file_store(dir.path()).unwrap();
    let reconciled = store.load().unwrap();
    assert_eq!(reconciled.expected.revision, 2);
    assert_eq!(reconciled.devices[0].state, RecordState::Revoked);
}
#[test]
fn NativePairing_HardlinkRejected_010() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    drop(store);
    std::fs::hard_link(data_path(dir.path()), dir.path().join("unexpected-link")).unwrap();
    let store = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(store.load(), Err(StoreError::Io));
}
#[test]
fn NativePairing_InitializeRechecksAtSyscall_011() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Revocable {
        target: StoreTarget,
        alive: Arc<AtomicBool>,
    }
    impl MutationAuthorization for Revocable {
        fn check(&mut self, target: &StoreTarget, mutation: &Mutation) -> Result<()> {
            if self.target.same_target(target)
                && *mutation == initial()
                && self.alive.load(Ordering::SeqCst)
            {
                Ok(())
            } else {
                Err(StoreError::Unauthorized)
            }
        }
    }
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    let signal = alive.clone();
    let _probe = crate::version_history::windows::files::probe_before_rename(move || {
        signal.store(false, Ordering::SeqCst);
    });
    let mut auth = Revocable {
        target: store.target(),
        alive: alive.clone(),
    };
    assert_eq!(
        store.apply(initial(), &mut auth),
        Err(StoreError::Unauthorized)
    );
    assert!(
        !alive.load(Ordering::SeqCst),
        "real rename boundary was reached"
    );
    assert!(
        !data_path(dir.path()).exists(),
        "revocation must precede the actual syscall"
    );
    assert_eq!(store.load(), Err(StoreError::Missing));
}

#[test]
fn NativePairing_ProductProjectionStrictSelection_020() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    let saved = apply(&store, initial()).unwrap();
    drop(store);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    let loaded = reopened.load().unwrap();
    let request=serde_json::json!({"storeId":saved.expected.store_id,"revision":"1","deviceId":record().device_id}).to_string();
    let selected = crate::remote_pairing_product::selected(&loaded, &request).unwrap();
    assert_eq!(selected, record());
    let projection =
        serde_json::to_value(crate::remote_pairing_product::project(Some(loaded.clone()))).unwrap();
    assert_eq!(projection["state"], "Locked");
    assert_eq!(projection["devices"][0]["installation"], "pending");
    assert!(matches!(
        reopened.select_installed(&loaded.expected, &record().device_id),
        Err(InstalledCandidateError::Pending)
    ));
    assert_eq!(projection["devices"].as_array().unwrap().len(), 1);
    for invalid in [
        request.replace("\"revision\":\"1\"", "\"revision\":\"01\""),
        request.replacen('{', "{\"scope\":{},", 1),
        request.replacen('{', "{\"revision\":\"1\",", 1),
    ] {
        assert!(crate::remote_pairing_product::selected(&loaded, &invalid).is_err());
    }
    let forgotten = apply(
        &reopened,
        Mutation::Forget {
            expected: loaded.expected,
            device_id: record().device_id,
        },
    )
    .unwrap();
    assert!(crate::remote_pairing_product::selected(&forgotten, &request).is_err());
    assert_eq!(
        serde_json::to_value(crate::remote_pairing_product::project(Some(forgotten))).unwrap()
            ["devices"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
#[test]
fn NativePairing_LegacySchemaNeverMigrates_021() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    let path = data_path(dir.path());
    drop(store);
    let original = std::fs::read(&path).unwrap();
    let legacy = String::from_utf8(original)
        .unwrap()
        .replace("\"schema\":2", "\"schema\":1");
    std::fs::write(&path, &legacy).unwrap();
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    assert_eq!(reopened.load(), Err(StoreError::Invalid));
    assert_eq!(std::fs::read(&path).unwrap(), legacy.as_bytes());
}
#[test]
fn NativePairing_MissingLeaseNeverRecreated_022() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    drop(store);
    let lease = dir.path().join("remote-pairings-v1/control.lock");
    let backup = dir.path().join("remote-pairings-v1/control.moved");
    std::fs::rename(&lease, &backup).unwrap();
    assert!(reopen_synthetic_file_store(dir.path()).is_err());
    assert!(!lease.exists());
    assert!(backup.exists());
}

pub(crate) fn fail_after_publish(store: &TestStore) {
    store.backend.test_fail_after_publish();
}

#[test]
fn NativePairing_OldSchema2BytesRemainUnverified_023() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    let record = record();
    // 精确保留扩展前 schema2 的字段次序；不能让新 codec 代造旧 envelope。
    let doc = format!(
        "{{\"schema\":2,\"domain\":\"{}\",\"storeId\":\"00000000-0000-4000-8000-000000000002\",\"revision\":1,\"devices\":[{{\"deviceId\":\"{}\",\"state\":\"active\",\"record\":{}}}]}}",
        DOMAIN, record.device_id, serde_json::to_string(&record).unwrap()
    );
    let digest: [u8; 32] = Sha256::digest(doc.as_bytes()).into();
    let bytes = TestSealer
        .seal(
            format!(
                "{{\"domain\":\"{}\",\"document\":{},\"digest\":{}}}",
                DOMAIN,
                doc,
                serde_json::to_string(&digest).unwrap()
            )
            .as_bytes(),
        )
        .unwrap();
    let decoded = store.decode(&bytes).unwrap();
    assert!(decoded.devices[0].enrollment.is_none());
    assert_eq!(store.encode(&decoded).unwrap(), bytes);
    // 先由原 backend 创建受保护文件，再注入旧字节；不用普通新文件绕过 ACL 边界。
    apply(&store, initial()).unwrap();
    std::fs::write(data_path(dir.path()), &bytes).unwrap();
    drop(store);
    let reopened = reopen_synthetic_file_store(dir.path()).unwrap();
    let loaded = reopened.load().unwrap();
    assert!(loaded.devices[0].enrollment.is_none());
    assert!(matches!(
        reopened.select_installed(&loaded.expected, &record.device_id),
        Err(InstalledCandidateError::Unverified)
    ));
    assert_eq!(
        serde_json::to_value(crate::remote_pairing_product::project(Some(loaded.clone()))).unwrap()
            ["devices"][0]["installation"],
        "unverified"
    );
    assert_eq!(std::fs::read(data_path(dir.path())).unwrap(), bytes);
}
#[test]
fn NativePairing_EnrollmentStrictBoundsAndTombstones_024() {
    let dir = directory();
    let store = synthetic_file_store(dir.path()).unwrap();
    apply(&store, initial()).unwrap();
    let sealed = std::fs::read(data_path(dir.path())).unwrap();
    let valid = store.decode(&sealed).unwrap();
    for mode in 0..5 {
        let mut doc = valid.clone();
        let entry = &mut doc.devices[0];
        let evidence = entry.enrollment.as_mut().unwrap();
        match mode {
            0 => evidence.version = 2,
            1 => evidence.revision = 0,
            2 => evidence.revision = 2,
            3 => evidence.record_digest = [0; 32],
            _ => {
                entry.state = RecordState::Revoked;
                entry.record = None;
            }
        }
        // 重新计算 envelope digest，确证拒绝来自证据语义而非摘要不符。
        assert_eq!(
            store.decode(&store.encode(&doc).unwrap()),
            Err(StoreError::Invalid)
        );
    }
    let text = String::from_utf8(TestSealer.unseal(&sealed).unwrap()).unwrap();
    for malformed in [
        text.replacen("\"enrollment\":", "\"enrollment\":null,\"enrollment\":", 1),
        text.replacen("\"pending\"", "\"unknown\"", 1),
    ] {
        assert_eq!(
            store.decode(&TestSealer.seal(malformed.as_bytes()).unwrap()),
            Err(StoreError::Invalid)
        );
    }
}

// TEST_ONLY：为真实 run 组合隔离构造历史记录；不证明注册或交付成功。
pub(crate) fn synthetic_confirmed_candidate<B: Backend>(
    store: &PairingStore<B, TestSealer>,
) -> InstalledDeviceCandidate {
    apply(
        store,
        Mutation::Initialize {
            store_id: uuid::Uuid::new_v4().to_string(),
            record: record(),
        },
    )
    .unwrap();
    let mut tx = store.backend.begin().unwrap();
    let mut doc = store.decode(&tx.read().unwrap().unwrap()).unwrap();
    doc.revision += 1;
    let evidence = doc.devices[0].enrollment.as_mut().unwrap();
    evidence.state = EnrollmentState::Confirmed;
    evidence.revision = doc.revision;
    tx.commit(&store.encode(&doc).unwrap(), true, &mut || Ok(()))
        .unwrap();
    drop(tx);
    store
        .select_installed(&store.load().unwrap().expected, &record().device_id)
        .unwrap()
}
pub(crate) fn synthetic_forget(store: &TestStore) {
    apply(
        store,
        Mutation::Forget {
            expected: store.load().unwrap().expected,
            device_id: record().device_id,
        },
    )
    .unwrap();
}

// TEST_ONLY：走原公开记录事务构造确认后发生的设备变化，不产生安装证据。
pub(crate) fn synthetic_change_record(store: &TestStore) {
    let snapshot = store.load().unwrap();
    let mut changed = snapshot.devices[0].record.clone().unwrap();
    changed.epochs.policy += 1;
    let forgotten = apply(store, Mutation::Forget { expected: snapshot.expected, device_id: changed.device_id.clone() }).unwrap();
    apply(store, Mutation::Remember { expected: forgotten.expected, record: changed }).unwrap();
}
pub(crate) fn synthetic_fail_after_publish(store: &TestStore) {
    store.backend.test_fail_after_publish();
}