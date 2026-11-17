//! 隔离历史设备与真实 Node/PTY run；不弹本机对话框或连接网络。
use super::*;
use crate::remote_host_approval::unattended::{
    RequestedRole, UnattendedApproval, UnattendedPreparation,
};
use crate::remote_native_pairing_store::tests::{
    synthetic_confirmed_candidate, synthetic_file_store, synthetic_forget,
};
fn cutoff() -> u64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64)
        + 60000
}
fn store_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("unattended-preparation-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit"),
        )
        .unwrap()
}
#[test]
fn UnattendedPreparation_RealRunSingleUseAndCancel_001() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = UnattendedApproval::test_pending(preparation);
    assert!(approval.consume(&store, &admission).unwrap().is_none());
    approval.test_answer(true, false);
    let fact = approval.consume(&store, &admission).unwrap().unwrap();
    assert!(approval.consume(&store, &admission).unwrap().is_none());
    fact.check(&store, &admission).unwrap();
    approval.cancel();
    assert!(fact.check(&store, &admission).is_err());
    drop(fact);
    drop(approval);
    drop(admission);
    assert!(cleanup.close());
}
#[test]
fn UnattendedPreparation_DeniedLateAndPrecancel_002() {
    for mode in 0..3 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let dir = store_root();
        let store = synthetic_file_store(dir.path()).unwrap();
        let preparation = UnattendedPreparation::new(
            &store,
            synthetic_confirmed_candidate(&store),
            &admission,
            RequestedRole::View,
            cutoff(),
        )
        .unwrap();
        let approval = UnattendedApproval::test_pending(preparation);
        if mode == 2 {
            approval.cancel();
        }
        approval.test_answer(mode != 0, mode == 1);
        assert!(approval.consume(&store, &admission).unwrap().is_none());
        drop(approval);
        drop(admission);
        assert!(cleanup.close());
    }
}
#[test]
fn UnattendedPreparation_ForeignRunAndOriginalDocument_003() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let (foreign, mut foreign_cleanup) = setup();
    let other = admit(&foreign);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = UnattendedApproval::test_pending(preparation);
    approval.test_answer(true, false);
    assert!(approval.consume(&store, &other).is_err());
    drop(approval);
    let candidate = store
        .select_installed(
            &store.load().unwrap().expected,
            &crate::remote_native_pairing_store::tests::record().device_id,
        )
        .unwrap();
    let preparation =
        UnattendedPreparation::new(&store, candidate, &admission, RequestedRole::View, cutoff())
            .unwrap();
    let approval = UnattendedApproval::test_pending(preparation);
    approval.test_answer(true, false);
    f.service.registry().activate_window("main").unwrap();
    assert!(approval.consume(&store, &admission).is_err());
    drop(approval);
    drop(admission);
    drop(other);
    assert!(cleanup.close());
    assert!(foreign_cleanup.close());
}
#[test]
fn UnattendedPreparation_ForgetDuringApproval_004() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = UnattendedApproval::test_pending(preparation);
    approval.test_answer(true, false);
    synthetic_forget(&store);
    assert!(approval.consume(&store, &admission).is_err());
    drop(approval);
    drop(admission);
    assert!(cleanup.close());
}

use crate::remote_native_pairing_store::{Backend, PairingStore, Transaction};
use crate::version_history::windows::pairing_store::WindowsPairingBackend;
type ReadHook = Arc<parking_lot::Mutex<Option<Box<dyn FnOnce() + Send>>>>;
struct BlockingBackend {
    inner: WindowsPairingBackend,
    hook: ReadHook,
}
struct BlockingTransaction {
    inner: <WindowsPairingBackend as Backend>::Guard,
    hook: ReadHook,
}
impl Backend for BlockingBackend {
    type Guard = BlockingTransaction;
    fn begin(&self) -> crate::remote_native_pairing_store::Result<Self::Guard> {
        Ok(BlockingTransaction {
            inner: self.inner.begin()?,
            hook: self.hook.clone(),
        })
    }
    fn target_label(&self) -> crate::remote_native_pairing_store::Result<String> {
        self.inner.target_label()
    }
}
impl Transaction for BlockingTransaction {
    fn read(&mut self) -> crate::remote_native_pairing_store::Result<Option<Vec<u8>>> {
        let result = self.inner.read();
        let hook = self.hook.lock().take();
        if let Some(hook) = hook {
            hook();
        }
        result
    }
    fn commit(
        &mut self,
        bytes: &[u8],
        replacing: bool,
        authorize: &mut dyn FnMut() -> crate::remote_native_pairing_store::Result<()>,
    ) -> crate::remote_native_pairing_store::Result<()> {
        self.inner.commit(bytes, replacing, authorize)
    }
}
fn during_store_read(mode: u8) {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let hook: ReadHook = Arc::default();
    let store = Arc::new(
        PairingStore::new(
            BlockingBackend {
                inner: WindowsPairingBackend::create_new(dir.path()).unwrap(),
                hook: hook.clone(),
            },
            crate::remote_native_pairing_store::tests::TestSealer,
        )
        .unwrap(),
    );
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = UnattendedApproval::test_pending(preparation);
    approval.test_answer(true, false);
    let mut fact = approval.consume(&store, &admission).unwrap().unwrap();
    let short_end = Instant::now() + Duration::from_millis(200);
    if mode == 1 {
        fact.test_limit_end(short_end);
    }
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    *hook.lock() = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let worker_store = store.clone();
    let worker = std::thread::spawn(move || fact.check(&worker_store, &admission).is_err());
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    match mode {
        0 => approval.cancel(),
        1 => std::thread::sleep(
            short_end.saturating_duration_since(Instant::now()) + Duration::from_millis(10),
        ),
        _ => {
            cleanup.stop = Some(
                f.supervisor
                    .begin_shutdown_owned(vec![cleanup.pin.take().unwrap()])
                    .unwrap(),
            )
        }
    }
    resume_tx.send(()).unwrap();
    let denied = worker.join().unwrap();
    drop(approval);
    assert!(cleanup.close());
    assert!(
        denied,
        "state changed during store read must reject the observation"
    );
}
#[test]
fn UnattendedPreparation_ReadTailCancellation_005() {
    during_store_read(0);
}
#[test]
fn UnattendedPreparation_ReadTailExpiry_006() {
    during_store_read(1);
}
#[test]
fn UnattendedPreparation_ReadTailRunStop_007() {
    during_store_read(2);
}

#[test]
fn UnattendedPreparation_CancelDoesNotWaitForStore_009() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let hook: ReadHook = Arc::default();
    let store = Arc::new(
        PairingStore::new(
            BlockingBackend {
                inner: WindowsPairingBackend::create_new(dir.path()).unwrap(),
                hook: hook.clone(),
            },
            crate::remote_native_pairing_store::tests::TestSealer,
        )
        .unwrap(),
    );
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = Arc::new(UnattendedApproval::test_pending(preparation));
    approval.test_answer(true, false);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    *hook.lock() = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let worker_store = store.clone();
    let worker_approval = approval.clone();
    let worker =
        std::thread::spawn(move || worker_approval.consume(&worker_store, &admission).is_err());
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let cancel_approval = approval.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let cancel = std::thread::spawn(move || {
        cancel_approval.cancel();
        done_tx.send(()).unwrap();
    });
    let cancelled_before_io_release = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    resume_tx.send(()).unwrap();
    let denied = worker.join().unwrap();
    cancel.join().unwrap();
    drop(approval);
    assert!(cleanup.close());
    assert!(
        cancelled_before_io_release,
        "cancel waited for blocked storage I/O"
    );
    assert!(denied, "cancelled consume published a usable approval");
}
#[test]
fn UnattendedPreparation_CutoffBounds_010() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    drop(synthetic_confirmed_candidate(&store));
    for end in [0, u64::MAX, cutoff() + 30 * 24 * 60 * 60 * 1000] {
        let candidate = store
            .select_installed(
                &store.load().unwrap().expected,
                &crate::remote_native_pairing_store::tests::record().device_id,
            )
            .unwrap();
        assert!(UnattendedPreparation::new(
            &store,
            candidate,
            &admission,
            RequestedRole::View,
            end
        )
        .is_err());
    }
    drop(admission);
    assert!(cleanup.close());
}

#[test]
fn UnattendedPreparation_CancelWhileOriginalRunLocked_011() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let locked_admission = admit(&f);
    let dir = store_root();
    let store = Arc::new(synthetic_file_store(dir.path()).unwrap());
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let approval = Arc::new(UnattendedApproval::test_pending(preparation));
    approval.test_answer(true, false);
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locker = std::thread::spawn(move || {
        crate::run_supervisor::delegated::test_hold_admission_resource(&locked_admission, || {
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    held_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let worker_approval = approval.clone();
    let worker_store = store.clone();
    let worker = std::thread::spawn(move || {
        worker_approval
            .consume(&worker_store, &admission)
            .map(|value| value.is_none())
            .unwrap_or(true)
    });
    let cancel_approval = approval.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let cancel = std::thread::spawn(move || {
        cancel_approval.cancel();
        done_tx.send(()).unwrap();
    });
    let cancelled_before_release = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    release_tx.send(()).unwrap();
    locker.join().unwrap();
    cancel.join().unwrap();
    let denied = worker.join().unwrap();
    drop(approval);
    assert!(cleanup.close());
    assert!(
        cancelled_before_release,
        "cancel depended on the original run resource lock"
    );
    assert!(denied);
}

#[test]
fn UnattendedFinal_IndependentYesAndCancelGate_012() {
    use crate::remote_host_approval::unattended::final_approval::FinalApproval;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::Control,
        cutoff(),
    )
    .unwrap();
    let first = UnattendedApproval::test_pending(preparation);
    first.test_answer(true, false);
    let prepared = first.consume(&store, &admission).unwrap().unwrap();
    let final_approval = FinalApproval::test_pending(prepared, &store, &admission).unwrap();
    // A3 的 Yes 不会替最终阶段回答。
    let before_yes = final_approval
        .consume(&store, &admission)
        .unwrap()
        .is_none();
    final_approval.test_answer(true);
    let final_fact = final_approval.consume(&store, &admission).unwrap().unwrap();
    let gate = final_fact.control();
    let initially_closed = !gate.active();
    final_fact.check_start(&store, &admission).unwrap();
    final_approval.cancel();
    let rejected = final_fact.publish().is_err() && !gate.active() && gate.closed();
    drop(final_fact);
    drop(final_approval);
    drop(first);
    drop(admission);
    let retired = cleanup.close();
    assert!(before_yes && initially_closed && rejected && retired);
}

#[test]
fn UnattendedFinal_DocumentChangeBeforePublicationRejects_013() {
    use crate::remote_host_approval::unattended::final_approval::FinalApproval;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let dir = store_root();
    let store = synthetic_file_store(dir.path()).unwrap();
    let preparation = UnattendedPreparation::new(
        &store,
        synthetic_confirmed_candidate(&store),
        &admission,
        RequestedRole::View,
        cutoff(),
    )
    .unwrap();
    let first = UnattendedApproval::test_pending(preparation);
    first.test_answer(true, false);
    let prepared = first.consume(&store, &admission).unwrap().unwrap();
    let final_approval = FinalApproval::test_pending(prepared, &store, &admission).unwrap();
    final_approval.test_answer(true);
    let final_fact = final_approval.consume(&store, &admission).unwrap().unwrap();
    f.service.registry().activate_window("main").unwrap();
    let rejected = final_fact.check_start(&store, &admission).is_err();
    final_approval.cancel();
    let closed = !final_fact.control().active();
    drop(final_fact);
    drop(final_approval);
    drop(first);
    drop(admission);
    let retired = cleanup.close();
    assert!(rejected && closed && retired);
}

struct SingleRunSealer;
impl crate::remote_unattended_store::Sealer for SingleRunSealer {
    fn seal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
        Ok([b"TEST_ONLY_SINGLE_RUN/".as_slice(), bytes].concat())
    }
    fn unseal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
        bytes
            .strip_prefix(b"TEST_ONLY_SINGLE_RUN/")
            .map(Vec::from)
            .ok_or(crate::remote_unattended_store::StoreError::Invalid)
    }
}
fn final_consent(
    store: &crate::remote_native_pairing_store::tests::TestStore,
    admission: &crate::run_supervisor::NativeRunAdmission,
) -> (
    Arc<crate::remote_host_approval::unattended::final_approval::FinalApproval>,
    crate::remote_host_approval::unattended::final_approval::FinalConsent,
) {
    final_consent_until(store, admission, cutoff())
}
fn final_consent_until(
    store: &crate::remote_native_pairing_store::tests::TestStore,
    admission: &crate::run_supervisor::NativeRunAdmission,
    deadline: u64,
) -> (
    Arc<crate::remote_host_approval::unattended::final_approval::FinalApproval>,
    crate::remote_host_approval::unattended::final_approval::FinalConsent,
) {
    use crate::remote_host_approval::unattended::final_approval::FinalApproval;
    let preparation = UnattendedPreparation::new(
        store,
        synthetic_confirmed_candidate(store),
        admission,
        RequestedRole::Control,
        deadline,
    )
    .unwrap();
    let first = UnattendedApproval::test_pending(preparation);
    first.test_answer(true, false);
    let prepared = first.consume(store, admission).unwrap().unwrap();
    let final_approval = Arc::new(FinalApproval::test_pending(prepared, store, admission).unwrap());
    final_approval.test_answer(true);
    let consent = final_approval.consume(store, admission).unwrap().unwrap();
    (final_approval, consent)
}
fn service(
    root: &std::path::Path,
) -> crate::remote_unattended_store::service::SingleRunService<SingleRunSealer> {
    let normalized = std::path::PathBuf::from(root.to_string_lossy().replace('/', "\\"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &normalized,
        )
        .unwrap();
    crate::remote_unattended_store::service::SingleRunService::new(backend, SingleRunSealer)
        .unwrap()
}
fn retire_service<S: crate::remote_unattended_store::Sealer>(
    owner: &mut crate::remote_unattended_store::service::SingleRunService<S>,
) {
    let end = Instant::now() + Duration::from_secs(5);
    while let Some((keys, effects)) = owner.advance_retirement() {
        if !keys.outstanding {
            assert!(keys.disposal_complete && !keys.unknown);
            assert_eq!(effects, CloseObservation::EffectsClosed);
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn UnattendedSingleRun_RealFinalEnableCaptureAndDocumentTransfer_005() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let store = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&store, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    let started = owner.start(
        consent,
        keys.custody,
        keys.ready,
        keys.signers,
        admission,
        &store,
    );
    let was_active = owner.is_published();
    f.service.registry().activate_window("main").unwrap();
    let resized = owner.resize(&store, &f.supervisor, 31, 91);
    let written = owner.write_frame(&store, &f.supervisor, b"native");
    let actual = if written.as_ref().is_ok_and(|r| r.complete) {
        Some(report(&f))
    } else {
        None
    };
    let closed = owner.stop();
    retire_service(&mut owner);
    let revoked = owner.test_durable_revoked();
    drop(owner);
    drop(approval);
    let retired = cleanup.close();
    assert!(
        started.is_ok() && was_active && resized.is_ok(),
        "{started:?} / {resized:?}"
    );
    assert!(written.is_ok_and(|r| r.complete && r.confirmed_bytes == 6));
    assert_eq!(
        actual.unwrap()["input"],
        json!([110, 97, 116, 105, 118, 101])
    );
    assert!(closed.is_ok() && revoked && retired);
}
#[test]
fn UnattendedSingleRun_EnableThenCancelledOrCaptureFailsCompensates_006() {
    for mode in 0..3 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let store = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&store, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let mut owner = service(durable_dir.path());
        let cancel = approval.clone();
        match mode {
            0 => owner.test_after_enable(move || cancel.cancel()),
            1 => owner.test_before_publish(move || cancel.cancel()),
            _ => {
                let registry = f.service.registry().clone();
                owner.test_before_capture(move || {
                    registry.activate_window("main").unwrap();
                });
            }
        }
        let result = owner.start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &store,
        );
        let inactive = !owner.is_published();
        let revoked = owner.test_durable_revoked();
        retire_service(&mut owner);
        drop(owner);
        drop(approval);
        let retired = cleanup.close();
        assert!(
            result.is_err() && inactive && revoked && retired,
            "mode {mode}: {result:?}, revoked {revoked}"
        );
    }
}

#[test]
fn UnattendedSingleRun_DropKeepsCustodyUntilOriginalKeysJoin_007() {
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let store = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&store, &admission);
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let keys = crate::native_crypto::single_run_test_keys_with_hook(
        consent.take_supplier().unwrap(),
        Box::new(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }),
    );
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &store,
        )
        .unwrap();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
    let dropping = std::thread::spawn(move || drop(owner));
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let during = backend.claim_owner_custody();
    let busy = matches!(
        during,
        Err(crate::remote_unattended_store::StoreError::Busy)
    );
    drop(during);
    release_tx.send(()).unwrap();
    let joined = dropping.join().is_ok();
    let after = backend.claim_owner_custody();
    let released = after.is_ok();
    drop(after);
    drop(backend);
    drop(approval);
    let retired = cleanup.close();
    assert!(busy && joined && released && retired,
        "custody must remain Busy during original worker retirement; busy={busy}, joined={joined}, released={released}");
}

#[test]
fn UnattendedSingleRun_UnknownQuarantineEndsOnlyWithProcess_008() {
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    const FLAG: &str = "CC_DESK_TEST_SINGLE_RUN_QUARANTINE_ROOT";
    if let Some(path) = std::env::var_os(FLAG) {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let store = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&store, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let path = std::path::PathBuf::from(path);
        let mut owner = service(&path);
        owner
            .start(
                consent,
                keys.custody,
                keys.ready,
                keys.signers,
                admission,
                &store,
            )
            .unwrap();
        // 注入原 record_frame_result 的 partial 结果，不声称制造了真实 PTY 部分写入。
        println!("TEST_ONLY_QUARANTINE before partial/drop");
        owner.test_partial_result();
        drop(owner);
        println!("TEST_ONLY_QUARANTINE after drop");
        let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
        assert!(matches!(
            backend.claim_owner_custody(),
            Err(crate::remote_unattended_store::StoreError::Busy)
        ));
        let same = WindowsUnattendedBackend::open_existing(&path).unwrap();
        assert!(matches!(
            crate::remote_unattended_store::service::SingleRunService::new(same, SingleRunSealer),
            Err(crate::remote_unattended_store::StoreError::Busy)
        ));
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if crate::remote_unattended_store::service::advance_quarantine()
                .is_some_and(|o| !o.outstanding && o.disposal_complete && !o.unknown)
            {
                break;
            }
            assert!(
                Instant::now() < end,
                "known keys must join even while effects remain quarantined"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        println!("TEST_ONLY_QUARANTINE original keys joined");
        drop(backend);
        drop(approval);
        assert_eq!(
            crate::remote_unattended_store::service::test_quarantine_state(),
            Some((
                true,
                crate::remote_unattended::capability::CloseObservation::Unknown
            ))
        );
        cleanup.host.test_begin_host_shutdown();
        let pin = cleanup.pin.take().unwrap();
        cleanup.stop = Some(cleanup.supervisor.begin_shutdown_owned(vec![pin]).unwrap());
        let close_end = Instant::now() + Duration::from_secs(10);
        let mut closed = false;
        while Instant::now() < close_end {
            crate::remote_unattended_store::service::advance_quarantine();
            let writer = cleanup.host.test_writer_retirement();
            let native = cleanup.stop.as_mut().unwrap().observe_run(&cleanup.run);
            if writer == WriterRetirement::Joined
                && matches!(
                    native,
                    NativeShutdownObservation::OwnedSessionReleased
                        | NativeShutdownObservation::Unknown {
                            direct_child_reaped: true,
                            output_reader_joined: true,
                            wait_workers_joined: true,
                            original_io_drop_returned: true,
                            stop_job_joined: true,
                            ..
                        }
                )
            {
                closed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let writer = cleanup.host.test_writer_retirement();
        let native = cleanup.stop.as_mut().map(|s| s.observe_run(&cleanup.run));
        println!("TEST_ONLY_QUARANTINE cleanup={closed} writer={writer:?} native={native:?}");
        assert!(closed);
        assert_eq!(
            crate::remote_unattended_store::service::test_quarantine_state(),
            Some((
                false,
                crate::remote_unattended::capability::CloseObservation::Unknown
            ))
        );
        let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
        assert!(matches!(
            backend.claim_owner_custody(),
            Err(crate::remote_unattended_store::StoreError::Busy)
        ));
        drop(backend);
        println!("TEST_ONLY_QUARANTINE after run cleanup");
        println!("TEST_ONLY_SINGLE_RUN_QUARANTINED_NOT_RETIRED");
        // 进程退出释放 OS 锁，不冒称隔离 owner 获得了成功退休收据。
        std::process::exit(0);
    }
    let dir = store_root();
    let path = std::path::PathBuf::from(dir.path().to_string_lossy().replace('/', "\\"));
    use std::os::windows::process::CommandExt;
    let child=std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","tests::native_cli_run_supervisor::remote_composition::delegated::preparation::UnattendedSingleRun_UnknownQuarantineEndsOnlyWithProcess_008","--nocapture","--test-threads=1"])
        .env(FLAG,&path).creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut child = Child(Some(child));
    let end = Instant::now() + Duration::from_secs(20);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        if Instant::now() >= end {
            let mut timed_out = child.0.take().unwrap();
            let _ = timed_out.kill();
            let output = timed_out.wait_with_output().unwrap();
            panic!(
                "owned quarantine child timeout; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("TEST_ONLY_SINGLE_RUN_QUARANTINED_NOT_RETIRED"));
    let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
    let custody = backend.claim_owner_custody().unwrap();
    drop(custody);
    let reopened =
        crate::remote_unattended_store::service::SingleRunService::new(backend, SingleRunSealer)
            .unwrap();
    assert!(reopened.test_loaded_enabled_is_locked());
    drop(reopened);
}

#[test]
fn UnattendedSingleRun_ConcurrentRevokeCannotPublishOrBeOverwritten_009() {
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let store = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&store, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let path = durable_dir.path().to_path_buf();
    let mut owner = service(&path);
    let changed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let changed_hook = changed.clone();
    let hook_path = path.clone();
    owner.test_after_enable(move || {
        let backend = WindowsUnattendedBackend::open_existing(&hook_path).unwrap();
        let revision =
            crate::remote_unattended_store::service::test_external_revoke(backend, SingleRunSealer);
        changed_hook.store(revision, std::sync::atomic::Ordering::Release);
    });
    let result = owner.start(
        consent,
        keys.custody,
        keys.ready,
        keys.signers,
        admission,
        &store,
    );
    let inactive = !owner.is_published();
    retire_service(&mut owner);
    drop(owner);
    drop(approval);
    let retired = cleanup.close();
    let revision = changed.load(std::sync::atomic::Ordering::Acquire);
    let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
    let preserved = crate::remote_unattended_store::service::test_file_revoked_at(
        backend,
        SingleRunSealer,
        revision,
    );
    assert_eq!(
        result,
        Err(crate::remote_unattended_store::StoreError::Conflict)
    );
    assert!(inactive && retired && revision > 0 && preserved);
}

#[test]
fn UnattendedSingleRun_CancelDuringEnableSealHitsOriginalFence_010() {
    use crate::remote_unattended_store::{Sealer, StoreError};
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    struct CancelSeal {
        approval: Arc<crate::remote_host_approval::unattended::final_approval::FinalApproval>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Sealer for CancelSeal {
        fn seal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let envelope: Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                envelope["document"]["status"],
                if call == 0 { "Disabled" } else { "Enabled" }
            );
            if call == 1 {
                self.approval.cancel();
            }
            SingleRunSealer.seal(bytes)
        }
        fn unseal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            SingleRunSealer.unseal(bytes)
        }
    }
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let store = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&store, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backend = WindowsUnattendedBackend::create_new(&path).unwrap();
    let mut owner = crate::remote_unattended_store::service::SingleRunService::new(
        backend,
        CancelSeal {
            approval: approval.clone(),
            calls: calls.clone(),
        },
    )
    .unwrap();
    let result = owner.start(
        consent,
        keys.custody,
        keys.ready,
        keys.signers,
        admission,
        &store,
    );
    let inactive = !owner.is_published();
    retire_service(&mut owner);
    drop(owner);
    drop(approval);
    let retired = cleanup.close();
    let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
    let unchanged =
        crate::remote_unattended_store::service::test_file_disabled_at(backend, SingleRunSealer, 1);
    assert_eq!(result, Err(StoreError::Unauthorized));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 2);
    assert!(inactive && retired && unchanged);
}

#[test]
fn UnattendedClose_ServiceLockCannotDelayClosingGate_001() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let store = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&store, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &store,
        )
        .unwrap();
    let controller = Arc::new(SingleRunController::from_started(owner).unwrap());
    let held = controller.test_lock_service();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let closing = controller.clone();
    let worker = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        closing.request_close();
        done_tx.send(()).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let fast = done_rx.recv_timeout(Duration::from_millis(250)).is_ok();
    let gate_closed_while_held = !held.is_published();
    drop(held);
    worker.join().unwrap();
    let closed = controller.finish_close();
    let mut owner = Arc::try_unwrap(controller)
        .ok()
        .unwrap()
        .test_into_service();
    let rejected = owner.write_frame(&store, &f.supervisor, b"BAD").is_err();
    retire_service(&mut owner);
    let revoked = owner.test_durable_revoked();
    drop(owner);
    drop(approval);
    let retired = cleanup.close();
    assert!(
        fast && gate_closed_while_held && closed.is_ok() && rejected && revoked && retired,
        "close blocked behind service mutex: fast={fast}, gate_closed={gate_closed_while_held}"
    );
}

#[test]
fn UnattendedClose_NewDocumentApprovalOrdersDurableRevokeBeforeTombstone_002() {
    use crate::{
        remote_host_approval::unattended_close::CloseAndForgetApproval,
        remote_native_pairing_store::tests::{
            synthetic_change_record, synthetic_fail_after_publish,
        },
        remote_native_pairing_store::{RecordState, StoreError as PairError},
        remote_unattended_store::{
            service::controller::{CloseForgetError, SingleRunController},
            StoreError,
        },
    };
    for mode in 0..8 {
        let (mut f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let store = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&store, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let mut owner = service(durable_dir.path());
        owner
            .start(
                consent,
                keys.custody,
                keys.ready,
                keys.signers,
                admission,
                &store,
            )
            .unwrap();
        let mut controller = SingleRunController::from_started(owner).unwrap();
        // 新 Host 文档确认关闭已移交的委托，不要求旧 document/run admission 仍有效。
        let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
        let document =
            crate::cli::document::DocumentAuthority::new(f.service.registry().clone(), url.clone())
                .unwrap();
        let mut resources = tauri::ResourceTable::default();
        let doc_binding = Arc::new(document.attach(&mut resources).unwrap());
        document.started(&url);
        document.finished(&url);
        f.caller = document.test_caller();
        let request = controller.prepare_forget(&store).unwrap();
        let deleting = CloseAndForgetApproval::test_pending(&doc_binding, &f.caller, request);
        deleting.test_answer(mode != 4);
        if mode == 1 {
            synthetic_change_record(&store);
        }
        if mode == 2 {
            controller.test_fail_durable_revoke();
        }
        if mode == 3 {
            synthetic_fail_after_publish(&store);
        }
        if mode == 5 {
            deleting.cancel();
        }
        if mode == 6 {
            controller = SingleRunController::from_started(controller.test_into_service()).unwrap();
        }
        let before = store.load().unwrap();
        let foreign_dir = store_root();
        let foreign_store = synthetic_file_store(foreign_dir.path()).unwrap();
        let result =
            controller.close_and_forget(&deleting, if mode == 7 { &foreign_store } else { &store });
        let published_after = controller.test_lock_service().is_published();
        let pair_after = store.load();
        let stop_result = controller.finish_close();
        let mut owner = controller.test_into_service();
        retire_service(&mut owner);
        drop(owner);
        drop(deleting);
        drop(approval);
        let retired = cleanup.close();
        assert!(retired, "mode {mode}");
        match mode {
            0 => {
                assert!(result.is_ok_and(|s| s.is_some()));
                let after = pair_after.unwrap();
                assert_eq!(after.expected.revision, before.expected.revision + 1);
                assert_eq!(after.devices[0].state, RecordState::Revoked);
                assert!(!published_after && stop_result.is_ok());
            }
            1 => {
                assert_eq!(result, Err(CloseForgetError::Pairing(PairError::Conflict)));
                assert_eq!(pair_after.unwrap(), before);
                assert_eq!(before.devices[0].state, RecordState::Active);
                assert!(!published_after && stop_result.is_ok());
            }
            2 => {
                assert_eq!(
                    result,
                    Err(CloseForgetError::DurableRevoke(StoreError::Unknown))
                );
                assert_eq!(pair_after.unwrap(), before);
                assert!(!published_after && stop_result == Err(StoreError::Unknown));
            }
            3 => {
                assert_eq!(result, Err(CloseForgetError::Pairing(PairError::Unknown)));
                assert_eq!(pair_after, Err(PairError::Poisoned));
                assert!(!published_after && stop_result.is_ok());
            }
            6 | 7 => {
                assert_eq!(
                    result,
                    Err(CloseForgetError::Pairing(PairError::Unauthorized))
                );
                assert_eq!(pair_after.unwrap(), before);
                assert!(published_after && stop_result.is_ok());
            }
            _ => {
                assert_eq!(result, Ok(None));
                assert_eq!(pair_after.unwrap(), before);
                assert!(published_after && stop_result.is_ok());
            }
        }
    }
}

#[test]
fn UnattendedClose_CancelOrDocumentChangeWhileWaitingCannotDelete_003() {
    use crate::{
        remote_host_approval::unattended_close::CloseAndForgetApproval,
        remote_native_pairing_store::StoreError as PairError,
        remote_unattended_store::service::controller::{CloseForgetError, SingleRunController},
    };
    for change_document in [false, true] {
        let (mut f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let store = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
        let (approval, mut consent) = final_consent(&store, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let mut owner = service(durable_dir.path());
        owner
            .start(
                consent,
                keys.custody,
                keys.ready,
                keys.signers,
                admission,
                &*store,
            )
            .unwrap();
        let controller = Arc::new(SingleRunController::from_started(owner).unwrap());
        let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
        let document =
            crate::cli::document::DocumentAuthority::new(f.service.registry().clone(), url.clone())
                .unwrap();
        let mut resources = tauri::ResourceTable::default();
        let binding = Arc::new(document.attach(&mut resources).unwrap());
        document.started(&url);
        document.finished(&url);
        f.caller = document.test_caller();
        let deleting = Arc::new(CloseAndForgetApproval::test_pending(
            &binding,
            &f.caller,
            controller.prepare_forget(&*store).unwrap(),
        ));
        deleting.test_answer(true);
        let before = store.load().unwrap();
        let held = controller.test_lock_service();
        let worker_controller = controller.clone();
        let worker_store = store.clone();
        let worker_approval = deleting.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            worker_controller.close_and_forget(&worker_approval, &*worker_store)
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while held.is_published() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
        let closed_before_release = !held.is_published();
        if change_document {
            document.started(&url);
        } else {
            deleting.cancel();
        }
        drop(held);
        let result = worker.join().unwrap();
        let after = store.load();
        let mut owner = Arc::try_unwrap(controller)
            .ok()
            .unwrap()
            .test_into_service();
        let revoked = owner.test_durable_revoked();
        retire_service(&mut owner);
        drop(owner);
        drop(deleting);
        drop(approval);
        let retired = cleanup.close();
        assert!(closed_before_release && revoked && retired);
        assert_eq!(
            result,
            Err(CloseForgetError::Pairing(PairError::Unauthorized))
        );
        assert_eq!(after.unwrap(), before);
    }
}

#[test]
fn UnattendedClose_ExistingStatesNeverReinitializeOrResetJournal_004() {
    use crate::{
        remote_host_approval::unattended::final_approval::FinalApproval,
        remote_unattended_store::{service::SingleRunService, Sealer, StoreError},
        version_history::windows::unattended_store::WindowsUnattendedBackend,
    };
    struct FirstSealer {
        cancel: Option<Arc<FinalApproval>>,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Sealer for FirstSealer {
        fn seal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                if let Some(cancel) = &self.cancel {
                    cancel.cancel();
                }
            }
            SingleRunSealer.seal(bytes)
        }
        fn unseal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            SingleRunSealer.unseal(bytes)
        }
    }
    for mode in 0..3 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&pairing, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let path =
            std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
        let backend = WindowsUnattendedBackend::create_new(&path).unwrap();
        let mut first = SingleRunService::new(
            backend,
            FirstSealer {
                cancel: if mode == 0 {
                    Some(approval.clone())
                } else {
                    None
                },
                calls: std::sync::atomic::AtomicUsize::new(0),
            },
        )
        .unwrap();
        let initial = first.start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        );
        if mode == 1 {
            first.stop().unwrap();
        } else {
            first.request_close();
        }
        retire_service(&mut first);
        drop(first);
        drop(approval);
        let blob_path = path.join("remote-unattended-v1").join("delegation.dpapi");
        let journal_path = path.join("remote-unattended-v1").join("commit.journal");
        let before_blob = std::fs::read(&blob_path).unwrap();
        let before_journal = std::fs::read(&journal_path).unwrap();
        let decoded: Value =
            serde_json::from_slice(&SingleRunSealer.unseal(&before_blob).unwrap()).unwrap();
        assert_eq!(
            decoded["document"]["status"],
            ["Disabled", "Revoked", "Enabled"][mode]
        );
        assert_eq!(initial.is_ok(), mode != 0);
        // 第二次仍是同一设备/原run的新明确确认，不绕到新root或新配对文档。
        let admission = admit(&f);
        let snapshot = pairing.load().unwrap();
        let candidate = pairing
            .select_installed(&snapshot.expected, &snapshot.devices[0].device_id)
            .unwrap();
        let prep = UnattendedPreparation::new(
            &pairing,
            candidate,
            &admission,
            RequestedRole::Control,
            cutoff(),
        )
        .unwrap();
        let preliminary = UnattendedApproval::test_pending(prep);
        preliminary.test_answer(true, false);
        let prepared = preliminary.consume(&pairing, &admission).unwrap().unwrap();
        let approval = FinalApproval::test_pending(prepared, &pairing, &admission).unwrap();
        approval.test_answer(true);
        let mut consent = approval.consume(&pairing, &admission).unwrap().unwrap();
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
        let mut reopened = SingleRunService::new(backend, SingleRunSealer).unwrap();
        let result = reopened.start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        );
        let closed = !reopened.is_published();
        retire_service(&mut reopened);
        drop(reopened);
        drop(approval);
        drop(preliminary);
        let after_blob = std::fs::read(&blob_path).unwrap();
        let after_journal = std::fs::read(&journal_path).unwrap();
        let retired = cleanup.close();
        assert_eq!(result, Err(StoreError::Conflict), "state mode {mode}");
        assert!(closed && retired);
        assert_eq!(before_blob, after_blob);
        assert_eq!(before_journal, after_journal);
    }
}

#[test]
fn UnattendedClose_NaturalRunExitStillAllowsOriginalCloseHandle_005() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let legacy = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let controller = SingleRunController::from_started(owner).unwrap();
    let close = controller.close_handle();
    let written = controller
        .test_lock_service()
        .write_frame(&pairing, &f.supervisor, b"native");
    let actual = if written.as_ref().is_ok_and(|r| r.complete) {
        Some(report(&f))
    } else {
        None
    };
    let old_admission_rejected = legacy.scope().is_err();
    close.request_close();
    let stopped = controller.finish_close();
    let mut owner = controller.test_into_service();
    let revoked = owner.test_durable_revoked();
    let no_effect = owner.write_frame(&pairing, &f.supervisor, b"BAD").is_err();
    retire_service(&mut owner);
    drop(owner);
    drop(legacy);
    drop(approval);
    let retired = cleanup.close();
    assert!(written.is_ok_and(|r| r.complete && r.confirmed_bytes == 6));
    assert_eq!(
        actual.unwrap()["input"],
        json!([110, 97, 116, 105, 118, 101])
    );
    assert!(old_admission_rejected && stopped.is_ok() && revoked && no_effect && retired);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedApp_OriginalExitClosesAndJoinsManagedOwner_001() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let f = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let _runtime_stop = RuntimeStop(native.clone());
    let events = f.events.clone();
    native
        .start_managed_run(&f.caller, &f.request, |_| {
            f.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let startup_end = Instant::now() + Duration::from_secs(10);
    while f.child_reports() == 0 || f.events.lock().is_empty() {
        assert!(Instant::now() < startup_end);
        std::thread::sleep(Duration::from_millis(5));
    }
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let controller = SingleRunController::from_started(owner).unwrap();
    let close = controller.close_handle();
    let host = Arc::new(RemoteHostService::default());
    host.unattended.install(controller).unwrap();
    let decision = host.request_exit(47, false);
    let immediate = close.test_closed();
    // RED 也先关门并收齐原资源；不可把断言 panic 留成等待原 cap 的 worker。
    close.request_close();

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        47,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let owner_known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
        .await
        .unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    let callback = rx.try_recv().ok();
    drop(approval);
    let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
    let cleanup_known = facts.original_resources_retired();
    eprintln!("TEST_ONLY_APP_EXIT immediate={immediate} owner_known={owner_known} joined={joined} observation={observation:?} callback={callback:?} facts={facts:?}");
    assert!(matches!(
        decision,
        crate::remote_host::ExitDecision::Wait(47)
    ));
    assert!(
        owner_known && joined && observation.watcher_joined && observation.retired && cleanup_known
    );
    // 原 NativeRuntime 可以保守 Unknown；本片不能把它升级成完整 App 退出成功。
    if callback.is_some() {
        assert_eq!(callback, Some(47));
    }
    assert!(
        immediate,
        "original App exit did not close the managed single-run gate"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedApp_FixedDeadlineSurvivesObserverCancellationWithoutJournalTicks_002() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let now = cutoff() - 60000;
    let preparation = UnattendedPreparation::new(
        &pairing,
        synthetic_confirmed_candidate(&pairing),
        &admission,
        RequestedRole::Control,
        now + 4000,
    )
    .unwrap();
    let first = UnattendedApproval::test_pending(preparation);
    first.test_answer(true, false);
    let prepared = first.consume(&pairing, &admission).unwrap().unwrap();
    let approval =
        crate::remote_host_approval::unattended::final_approval::FinalApproval::test_pending(
            prepared, &pairing, &admission,
        )
        .unwrap();
    approval.test_answer(true);
    let mut consent = approval.consume(&pairing, &admission).unwrap().unwrap();
    let fixed_end = consent.end();
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let controller = SingleRunController::from_started(owner).unwrap();
    let close = controller.close_handle();
    let journal = durable_dir
        .path()
        .join("remote-unattended-v1/commit.journal");
    let before = fs::read(&journal).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let host = RemoteHostService::default();
    host.unattended.install(controller).unwrap();
    let cancelled = tokio::time::timeout(Duration::from_millis(100), host.unattended.wait())
        .await
        .is_err();
    let idle_unchanged = fs::read(&journal).unwrap() == before;
    let still_open = !close.test_closed();
    let result = tokio::time::timeout_at(
        tokio::time::Instant::from_std(fixed_end + Duration::from_secs(1)),
        host.unattended.wait(),
    )
    .await;
    let in_original_budget = matches!(result, Ok(true));
    // 任何断言前均先停止并 join 原 watcher，不能 timeout 后丢 handle。
    close.request_close();
    let joined = tokio::time::timeout(Duration::from_secs(5), host.unattended.wait())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    drop(approval);
    drop(first);
    let cleaned = cleanup.close();
    assert!(cancelled && idle_unchanged && still_open && in_original_budget && joined && cleaned, "cancelled={cancelled} idle={idle_unchanged} open={still_open} deadline={in_original_budget} joined={joined} cleanup={cleaned}");
    assert!(
        observation.watcher_joined && observation.controller_drop_returned && observation.known()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedApp_SpawnFailureCloseRaceAndPanicRetainOriginalCustody_003() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    for mode in 0..3 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&pairing, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let mut owner = service(durable_dir.path());
        owner
            .start(
                consent,
                keys.custody,
                keys.ready,
                keys.signers,
                admission,
                &pairing,
            )
            .unwrap();
        let controller = SingleRunController::from_started(owner).unwrap();
        let close = controller.close_handle();
        let host = Arc::new(RemoteHostService::default());
        if mode == 0 {
            host.unattended.test_fail_spawn();
        }
        if mode == 2 {
            host.unattended.test_panic_worker();
        }
        let (entered, arrived) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        host.unattended.test_before_spawn(move || {
            let _ = entered.send(());
            released.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let target = host.clone();
        let install = std::thread::spawn(move || target.unattended.install(controller));
        arrived.recv_timeout(Duration::from_secs(5)).unwrap();
        let starting = host.unattended.observe();
        host.unattended.request_close();
        let closed_before_publish = close.test_closed();
        release.send(()).unwrap();
        let installed = install.join().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), host.unattended.wait())
            .await
            .unwrap();
        let observed = host.unattended.observe();
        drop(approval);
        let cleaned = cleanup.close();
        assert!(starting.present && starting.outstanding && !starting.watcher_joined);
        assert!(
            closed_before_publish
                && cleaned
                && observed.controller_drop_returned
                && !observed.outstanding,
            "mode={mode} observed={observed:?}"
        );
        match mode {
            0 => assert!(
                installed.is_err()
                    && !result
                    && observed.failed
                    && !observed.watcher_joined
                    && observed.durable == Some(Ok(()))
            ),
            1 => assert!(installed.is_ok() && result && observed.known()),
            2 => assert!(
                installed.is_ok()
                    && !result
                    && observed.failed
                    && observed.watcher_joined
                    && observed.durable.is_none()
            ),
            _ => unreachable!(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedApp_DefaultEmptyDoesNotBlockOriginalExit_004() {
    let root = tempfile::tempdir().unwrap();
    let native = Arc::new(NativeRuntime::new(Arc::new(LaunchService::new(
        WorkspaceRepository::open(root.path().join("workspace.json")).unwrap(),
        Some(Default::default()),
        None,
    ))));
    let host = Arc::new(RemoteHostService::default());
    let empty = host.unattended.observe();
    assert!(!empty.present && !empty.watcher_joined && empty.known());
    assert_eq!(
        host.request_exit(48, false),
        crate::remote_host::ExitDecision::Wait(48)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        48,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let result = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .unwrap()
        .unwrap();
    assert!(host.test_join_exit_worker().await);
    assert_eq!(result, 48);
    assert_eq!(
        host.request_exit(48, false),
        crate::remote_host::ExitDecision::Proceed
    );
}

struct UnattendedAppRuntimeStop(Arc<NativeRuntime>);
impl Drop for UnattendedAppRuntimeStop {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}
fn unattended_app_run() -> (Fixture, Arc<NativeRuntime>, UnattendedAppRuntimeStop) {
    let (f, native, guard, ()) = unattended_app_run_prepared(|_| ());
    (f, native, guard)
}
fn unattended_app_run_prepared<T>(
    prepare: impl FnOnce(&mut Fixture) -> T,
) -> (Fixture, Arc<NativeRuntime>, UnattendedAppRuntimeStop, T) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut f = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let prepared = prepare(&mut f);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    let guard = UnattendedAppRuntimeStop(native.clone());
    let events = f.events.clone();
    native
        .start_managed_run(&f.caller, &f.request, |_| {
            f.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while f.child_reports() == 0 || f.events.lock().is_empty() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    (f, native, guard, prepared)
}
#[test]
fn UnattendedApp_UnknownJoinsWatcherAndPhysicalOwnersButNeverPublishesExit_005() {
    unattended_app_unknown_case(false);
}
fn unattended_app_unknown_case(rejected: bool) {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    const FLAG: &str = "CC_TEST_UNATTENDED_APP_QUARANTINE";
    if let Some(path) = std::env::var_os(FLAG) {
        let path = std::path::PathBuf::from(path);
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (f, native, _runtime_stop) = unattended_app_run();
            let admission = admit(&f);
            let pair_dir = store_root();
            let pairing = synthetic_file_store(pair_dir.path()).unwrap();
            let (approval, mut consent) = final_consent(&pairing, &admission);
            let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
            let mut owner = service(&path);
            owner.start(consent, keys.custody, keys.ready, keys.signers, admission, &pairing).unwrap();
            // 原 policy partial-result 注入；不冒称真的制造了未知 PTY 写入。
            owner.test_partial_result();
            let controller = SingleRunController::from_started(owner).unwrap();
            let host = Arc::new(RemoteHostService::default());
            if rejected { host.unattended.request_close(); }
            let installed = host.unattended.install(controller);
            if rejected { assert_eq!(installed, Err(crate::remote_unattended_store::StoreError::Unknown)); } else { installed.unwrap(); }
            assert_eq!(host.request_exit(49, false), crate::remote_host::ExitDecision::Wait(49));
            let (tx, mut rx) = tokio::sync::oneshot::channel();
            host.start_exit(49, native, Arc::new(crate::remote_client_ipc::RemoteClientService::default()), move |code| { let _ = tx.send(code); });
            let known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait()).await.unwrap();
            let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker()).await.unwrap();
            let observation = host.unattended.observe();
            let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
            let quarantine = crate::remote_unattended_store::service::test_quarantine_state();
            let forbidden = rx.try_recv().is_err() && host.request_exit(49, false) == crate::remote_host::ExitDecision::Prevent;
            let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
            let custody_held = matches!(backend.claim_owner_custody(), Err(crate::remote_unattended_store::StoreError::Busy));
            drop(backend);
            drop(approval);
            println!("TEST_ONLY_APP_UNKNOWN known={known} joined={joined} observation={observation:?} facts={facts:?} quarantine={quarantine:?} forbidden={forbidden} custody={custody_held}");
            assert!(!known && joined && !observation.outstanding && forbidden && custody_held);
            if rejected { assert!(!observation.present && observation.failed && !observation.watcher_joined); }
            else { assert!(observation.watcher_joined && observation.controller_drop_returned && !observation.retired); }
            assert!(facts.original_resources_retired() && facts.original_job_handle_closed && facts.job_query_is_historical);
            assert_eq!(quarantine, Some((false, CloseObservation::Unknown)));
        });
        println!("TEST_ONLY_APP_QUARANTINED_NO_EXIT_RECEIPT");
        // 以上原资源实证完成后，隔离槽/OS 锁只由进程退出释放。
        std::process::exit(0);
    }
    let dir = store_root();
    let path = std::path::PathBuf::from(dir.path().to_string_lossy().replace('/', "\\"));
    use std::os::windows::process::CommandExt;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", if rejected { "tests::native_cli_run_supervisor::remote_composition::delegated::preparation::UnattendedApp_RejectedUnknownStillDrivesOriginalQuarantine_009" } else { "tests::native_cli_run_supervisor::remote_composition::delegated::preparation::UnattendedApp_UnknownJoinsWatcherAndPhysicalOwnersButNeverPublishesExit_005" }, "--nocapture", "--test-threads=1"])
        .env(FLAG, &path).creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut child = Child(Some(child));
    let end = Instant::now() + Duration::from_secs(20);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        if Instant::now() >= end {
            let mut owned = child.0.take().unwrap();
            let _ = owned.kill();
            let output = owned.wait_with_output().unwrap();
            panic!(
                "App quarantine child timeout; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("TEST_ONLY_APP_QUARANTINED_NO_EXIT_RECEIPT"));
    let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
    let custody = backend.claim_owner_custody().unwrap();
    drop(custody);
    let reopened =
        crate::remote_unattended_store::service::SingleRunService::new(backend, SingleRunSealer)
            .unwrap();
    assert!(!reopened.is_published());
    drop(reopened);
    assert!(
        crate::remote_unattended_store::service::test_file_revoked_at(
            WindowsUnattendedBackend::open_existing(&path).unwrap(),
            SingleRunSealer,
            3
        )
    );
}

#[test]
fn UnattendedApp_WrapperCoreClosesBeforeLegacyControllerLock_006() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let controller = SingleRunController::from_started(owner).unwrap();
    let close = controller.close_handle();
    let host = Arc::new(RemoteHostService::default());
    host.unattended.install(controller).unwrap();
    let legacy = Arc::new(crate::remote_host_controller::RemoteHostController::default());
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let held_legacy = legacy.clone();
    let lock = std::thread::spawn(move || {
        held_legacy.test_hold_state(|| {
            held_tx.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    held_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (returned_tx, returned_rx) = std::sync::mpsc::channel();
    let closing_host = host.clone();
    let closing = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        crate::remote_host::begin_app_shutdown(&closing_host, &legacy);
        returned_tx.send(()).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    while !close.test_closed() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(2));
    }
    let closed_while_legacy_held = close.test_closed();
    let wrapper_still_waiting = returned_rx.try_recv().is_err();
    release.send(()).unwrap();
    lock.join().unwrap();
    closing.join().unwrap();
    close.request_close();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let joined = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), host.unattended.wait())
            .await
            .unwrap()
    });
    drop(approval);
    let cleaned = cleanup.close();
    assert!(closed_while_legacy_held && wrapper_still_waiting && joined && cleaned);
}

#[test]
fn UnattendedApp_RejectedStartedControllerDurablyClosesWithoutReplacingSlot_007() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let controller = SingleRunController::from_started(owner).unwrap();
    let close = controller.close_handle();
    let host = RemoteHostService::default();
    host.unattended.request_close();
    let result = host.unattended.install(controller);
    let closed = close.test_closed();
    let observation = host.unattended.observe();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let revoked = crate::remote_unattended_store::service::test_file_revoked_at(
        WindowsUnattendedBackend::open_existing(&path).unwrap(),
        SingleRunSealer,
        3,
    );
    drop(approval);
    let cleaned = cleanup.close();
    assert_eq!(
        result,
        Err(crate::remote_unattended_store::StoreError::Conflict)
    );
    assert!(closed && !observation.present && observation.known() && cleaned);
    assert!(
        revoked,
        "rejected already-started owner left avoidable durable Enabled"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedApp_RejectionCleanupBlocksReadyAndUnknownIsSticky_008() {
    use crate::remote_unattended_store::service::controller::SingleRunController;
    use crate::remote_unattended_store::{Sealer, StoreError};
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    struct RevokeSealer {
        calls: std::sync::atomic::AtomicUsize,
        entered: parking_lot::Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: parking_lot::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl Sealer for RevokeSealer {
        fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 2 {
                self.entered.lock().take().unwrap().send(()).unwrap();
                self.release
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            SingleRunSealer.seal(bytes)
        }
        fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
            SingleRunSealer.unseal(bytes)
        }
    }
    for unknown in [false, true] {
        let (f, native, _guard) = unattended_app_run();
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = synthetic_file_store(pair_dir.path()).unwrap();
        let (approval, mut consent) = final_consent(&pairing, &admission);
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let path =
            std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
        let (entered, arrived) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let sealer = RevokeSealer {
            calls: std::sync::atomic::AtomicUsize::new(0),
            entered: parking_lot::Mutex::new(Some(entered)),
            release: parking_lot::Mutex::new(released),
        };
        let mut owner = crate::remote_unattended_store::service::SingleRunService::new(
            WindowsUnattendedBackend::create_new(&path).unwrap(),
            sealer,
        )
        .unwrap();
        owner
            .start(
                consent,
                keys.custody,
                keys.ready,
                keys.signers,
                admission,
                &pairing,
            )
            .unwrap();
        let controller = SingleRunController::from_started(owner).unwrap();
        if unknown {
            controller.test_fail_durable_revoke();
        }
        let close = controller.close_handle();
        let host = Arc::new(RemoteHostService::default());
        host.unattended.request_close();
        let rejecting = host.clone();
        let worker = std::thread::spawn(move || rejecting.unattended.install(controller));
        arrived.recv_timeout(Duration::from_secs(5)).unwrap();
        let held = host.unattended.observe();
        let closed = close.test_closed();
        assert_eq!(
            host.request_exit(50, false),
            crate::remote_host::ExitDecision::Wait(50)
        );
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        host.start_exit(
            50,
            native,
            Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
            move |code| {
                let _ = tx.send(code);
            },
        );
        let prevented = tokio::time::timeout(Duration::from_millis(50), host.unattended.wait())
            .await
            .is_err()
            && rx.try_recv().is_err();
        release.send(()).unwrap();
        let result = worker.join().unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
            .await
            .unwrap();
        let ended = host.unattended.observe();
        let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
        let callback = rx.try_recv().ok();
        drop(approval);
        assert!(
            held.outstanding
                && !held.known()
                && closed
                && prevented
                && joined
                && !ended.outstanding
        );
        assert!(facts.original_resources_retired() && facts.original_job_handle_closed);
        if unknown {
            assert_eq!(result, Err(StoreError::Unknown));
            assert!(ended.failed && !ended.known() && callback.is_none());
            assert_eq!(
                host.request_exit(50, false),
                crate::remote_host::ExitDecision::Prevent
            );
        } else {
            assert_eq!(result, Err(StoreError::Conflict));
            assert!(ended.known());
            assert_eq!(callback, Some(50));
        }
    }
}

#[test]
fn UnattendedApp_RejectedUnknownStillDrivesOriginalQuarantine_009() {
    unattended_app_unknown_case(true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedReservation_ExitDuringEnabledStartRetainsOriginalJoin_001() {
    use crate::remote_unattended_store::service::pending::{PendingStart, StartKeys};
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let f = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let _runtime_stop = RuntimeStop(native.clone());
    let events = f.events.clone();
    native
        .start_managed_run(&f.caller, &f.request, |_| {
            f.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let startup_end = Instant::now() + Duration::from_secs(10);
    while f.child_reports() == 0 || f.events.lock().is_empty() {
        assert!(Instant::now() < startup_end);
        std::thread::sleep(Duration::from_millis(5));
    }
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &path,
        )
        .unwrap();
    let close = consent.control();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut pending = PendingStart::new(
        backend,
        SingleRunSealer,
        consent,
        admission,
        StartKeys {
            custody: keys.custody,
            ready: keys.ready,
            signers: keys.signers,
        },
        pairing,
    )
    .unwrap();
    pending.test_after_enable(move || {
        let _ = entered_tx.send(());
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
    });

    let host = Arc::new(RemoteHostService::default());
    host.unattended
        .reserve_start(pending, native.clone())
        .unwrap();
    let entered = entered_rx.recv_timeout(Duration::from_secs(10)).is_ok();
    let cancelled_observer =
        tokio::time::timeout(Duration::from_millis(20), host.unattended.wait())
            .await
            .is_err();
    let decision = host.request_exit(47, false);
    let immediate = close.closed();
    let pending_at_exit = host.unattended.observe().outstanding;
    let _ = release_tx.send(());
    // RED 也先关门并收齐原资源；不可把断言 panic 留成等待原 cap 的 worker。
    close.close();

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        47,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let owner_known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
        .await
        .unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    let callback = rx.try_recv().ok();
    drop(approval);
    let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
    let cleanup_known = facts.original_resources_retired();
    eprintln!("TEST_ONLY_APP_EXIT immediate={immediate} owner_known={owner_known} joined={joined} observation={observation:?} callback={callback:?} facts={facts:?}");
    assert!(entered && cancelled_observer && pending_at_exit);
    assert!(matches!(
        decision,
        crate::remote_host::ExitDecision::Wait(47)
    ));
    assert!(
        owner_known && joined && observation.watcher_joined && observation.retired && cleanup_known
    );
    // 原 NativeRuntime 可以保守 Unknown；本片不能把它升级成完整 App 退出成功。
    if callback.is_some() {
        assert_eq!(callback, Some(47));
    }
    assert!(
        immediate,
        "original App exit did not close the managed single-run gate"
    );
}

#[test]
fn UnattendedReservation_UnstartedCleanupKeepsCustodyUntilKeysJoin_002() {
    use crate::remote_unattended_store::service::pending::{PendingStart, StartKeys};
    use crate::remote_unattended_store::StoreError;
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    for mode in 0..4 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
        let (approval, mut consent) = final_consent(&pairing, &admission);
        let control = consent.control();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let keys = crate::native_crypto::single_run_test_keys_with_hook(
            consent.take_supplier().unwrap(),
            Box::new(move || {
                let _ = entered_tx.send(());
                let _ = release_rx.recv_timeout(Duration::from_secs(10));
            }),
        );
        let durable_dir = store_root();
        let path =
            std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
        let backend = WindowsUnattendedBackend::create_new(&path).unwrap();
        let pending = PendingStart::new(
            backend,
            SingleRunSealer,
            consent,
            admission,
            StartKeys {
                custody: keys.custody,
                ready: keys.ready,
                signers: keys.signers,
            },
            pairing,
        )
        .unwrap();
        let host = Arc::new(RemoteHostService::default());
        if mode == 1 {
            host.request_exit(52, false);
        }
        if mode == 3 {
            host.unattended.test_fail_spawn();
        }
        let supervisor = if mode == 2 {
            Arc::new(NativeRunSupervisor::new(f.transports.clone()))
        } else {
            f.supervisor.clone()
        };
        let native = Arc::new(NativeRuntime::with_owned_components_for_test(
            f.service.clone(),
            f.transports.clone(),
            supervisor,
        ));
        let original = host.clone();
        let worker = std::thread::spawn(move || {
            if mode == 0 {
                drop(pending);
                Ok(())
            } else {
                original.unattended.reserve_start(pending, native)
            }
        });
        let entered = entered_rx.recv_timeout(Duration::from_secs(10)).is_ok();
        let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
        let busy = matches!(backend.claim_owner_custody(), Err(StoreError::Busy));
        let blocked = mode == 0 || !host.unattended.observe().known();
        let _ = release_tx.send(());
        let result = worker.join().unwrap();
        let reclaimed = backend.claim_owner_custody();
        let available = reclaimed.is_ok();
        drop(reclaimed);
        let directory = path.join("remote-unattended-v1");
        let no_write = !directory.join("delegation.dpapi").exists()
            && std::fs::read(directory.join("commit.journal"))
                .unwrap()
                .is_empty();
        drop(approval);
        let retired = cleanup.close();
        eprintln!("TEST_ONLY_PENDING mode={mode} result={result:?} busy={busy} blocked={blocked} no_write={no_write}");
        assert!(entered && busy && blocked && available && no_write && retired && control.closed());
        assert_eq!(
            result,
            match mode {
                0 => Ok(()),
                3 => Err(StoreError::Io),
                _ => Err(StoreError::Conflict),
            }
        );
        assert_eq!(host.unattended.start_result(), None);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedReservation_ForeignExitCannotReplaceBoundRuntime_003() {
    use crate::remote_unattended_store::service::pending::{PendingStart, StartKeys};
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let f = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let _runtime_stop = RuntimeStop(native.clone());
    let events = f.events.clone();
    native
        .start_managed_run(&f.caller, &f.request, |_| {
            f.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let startup_end = Instant::now() + Duration::from_secs(10);
    while f.child_reports() == 0 || f.events.lock().is_empty() {
        assert!(Instant::now() < startup_end);
        std::thread::sleep(Duration::from_millis(5));
    }
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &path,
        )
        .unwrap();
    let close = consent.control();
    let pending = PendingStart::new(
        backend,
        SingleRunSealer,
        consent,
        admission,
        StartKeys {
            custody: keys.custody,
            ready: keys.ready,
            signers: keys.signers,
        },
        pairing,
    )
    .unwrap();
    let host = Arc::new(RemoteHostService::default());
    host.unattended
        .reserve_start(pending, native.clone())
        .unwrap();
    let start_end = Instant::now() + Duration::from_secs(10);
    while host.unattended.start_result().is_none() && Instant::now() < start_end {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let started = host.unattended.start_result();
    let decision = host.request_exit(47, false);
    let immediate = close.closed();
    let pending_at_exit = host.unattended.observe().outstanding;
    let foreign = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    let foreign_callback = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_flag = foreign_callback.clone();
    host.start_exit(
        47,
        foreign,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |_| {
            callback_flag.store(true, std::sync::atomic::Ordering::Release);
        },
    );
    let rejected_before_shutdown = !host.test_exit_worker_held() && admit(&f).scope().is_ok();
    // RED 也先关门并收齐原资源；不可把断言 panic 留成等待原 cap 的 worker。
    close.close();

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        47,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let owner_known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
        .await
        .unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    let callback = rx.try_recv().ok();
    drop(approval);
    let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
    let cleanup_known = facts.original_resources_retired();
    eprintln!("TEST_ONLY_APP_EXIT immediate={immediate} owner_known={owner_known} joined={joined} observation={observation:?} callback={callback:?} facts={facts:?}");
    assert_eq!(started, Some(Ok(())));
    assert!(
        pending_at_exit
            && rejected_before_shutdown
            && !foreign_callback.load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(matches!(
        decision,
        crate::remote_host::ExitDecision::Wait(47)
    ));
    assert!(
        owner_known && joined && observation.watcher_joined && observation.retired && cleanup_known
    );
    // 原 NativeRuntime 可以保守 Unknown；本片不能把它升级成完整 App 退出成功。
    if callback.is_some() {
        assert_eq!(callback, Some(47));
    }
    assert!(
        immediate,
        "original App exit did not close the managed single-run gate"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedReservation_ExitDuringEnableSealUsesOriginalFence_004() {
    use crate::remote_unattended_store::service::pending::{PendingStart, StartKeys};
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let f = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let _runtime_stop = RuntimeStop(native.clone());
    let events = f.events.clone();
    native
        .start_managed_run(&f.caller, &f.request, |_| {
            f.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .unwrap();
    let startup_end = Instant::now() + Duration::from_secs(10);
    while f.child_reports() == 0 || f.events.lock().is_empty() {
        assert!(Instant::now() < startup_end);
        std::thread::sleep(Duration::from_millis(5));
    }
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &path,
        )
        .unwrap();
    let close = consent.control();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    struct SealBarrier {
        entered: std::sync::mpsc::Sender<()>,
        release: parking_lot::Mutex<std::sync::mpsc::Receiver<()>>,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl crate::remote_unattended_store::Sealer for SealBarrier {
        fn seal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                let _ = self.entered.send(());
                let _ = self.release.lock().recv_timeout(Duration::from_secs(10));
            }
            crate::remote_unattended_store::Sealer::seal(&SingleRunSealer, bytes)
        }
        fn unseal(&self, bytes: &[u8]) -> crate::remote_unattended_store::Result<Vec<u8>> {
            crate::remote_unattended_store::Sealer::unseal(&SingleRunSealer, bytes)
        }
    }
    let pending = PendingStart::new(
        backend,
        SealBarrier {
            entered: entered_tx,
            release: parking_lot::Mutex::new(release_rx),
            calls: std::sync::atomic::AtomicUsize::new(0),
        },
        consent,
        admission,
        StartKeys {
            custody: keys.custody,
            ready: keys.ready,
            signers: keys.signers,
        },
        pairing,
    )
    .unwrap();
    let host = Arc::new(RemoteHostService::default());
    host.unattended
        .reserve_start(pending, native.clone())
        .unwrap();
    let entered = entered_rx.recv_timeout(Duration::from_secs(10)).is_ok();
    let cancelled_observer =
        tokio::time::timeout(Duration::from_millis(20), host.unattended.wait())
            .await
            .is_err();
    let decision = host.request_exit(47, false);
    let immediate = close.closed();
    let pending_at_exit = host.unattended.observe().outstanding;
    let _ = release_tx.send(());
    // RED 也先关门并收齐原资源；不可把断言 panic 留成等待原 cap 的 worker。
    close.close();

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        47,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let owner_known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
        .await
        .unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    let callback = rx.try_recv().ok();
    drop(approval);
    let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
    let cleanup_known = facts.original_resources_retired();
    eprintln!("TEST_ONLY_APP_EXIT immediate={immediate} owner_known={owner_known} joined={joined} observation={observation:?} callback={callback:?} facts={facts:?}");
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::open_existing(
            &path,
        )
        .unwrap();
    let unchanged =
        crate::remote_unattended_store::service::test_file_disabled_at(backend, SingleRunSealer, 1);
    assert_eq!(
        host.unattended.start_result(),
        Some(Err(
            crate::remote_unattended_store::StoreError::Unauthorized
        ))
    );
    assert!(entered && cancelled_observer && pending_at_exit && unchanged);
    assert!(matches!(
        decision,
        crate::remote_host::ExitDecision::Wait(47)
    ));
    assert!(
        owner_known && joined && observation.watcher_joined && observation.retired && cleanup_known
    );
    // 原 NativeRuntime 可以保守 Unknown；本片不能把它升级成完整 App 退出成功。
    if callback.is_some() {
        assert_eq!(callback, Some(47));
    }
    assert!(
        immediate,
        "original App exit did not close the managed single-run gate"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedReservation_GlobalCheckoutFailureClosesAdmissionAndActiveOwner_005() {
    use crate::remote_unattended_store::service::pending::{PendingStart, StartKeys};
    use crate::version_history::windows::unattended_store::WindowsUnattendedBackend;
    for mode in 0..4 {
        let (f, mut cleanup) = setup();
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
        let (approval, mut consent) = final_consent(&pairing, &admission);
        let control = consent.control();
        let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
        let durable_dir = store_root();
        let path =
            std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
        let backend = WindowsUnattendedBackend::create_new(&path).unwrap();
        let pending = PendingStart::new(
            backend,
            SingleRunSealer,
            consent,
            admission,
            StartKeys {
                custody: keys.custody,
                ready: keys.ready,
                signers: keys.signers,
            },
            pairing,
        )
        .unwrap();
        let native = Arc::new(NativeRuntime::with_owned_components_for_test(
            f.service.clone(),
            f.transports.clone(),
            f.supervisor.clone(),
        ));
        let host = Arc::new(RemoteHostService::default());
        if mode < 2 {
            host.test_unfinished_checkout_unwind(mode == 1);
        }
        let result = host.unattended.reserve_start(pending, native);
        if mode >= 2 {
            let end = Instant::now() + Duration::from_secs(10);
            while host.unattended.start_result().is_none() && Instant::now() < end {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            host.test_unfinished_checkout_unwind(mode == 3);
        }
        let immediate = control.closed();
        let closed_admission = !host.network_admission_open();
        // 失败版本也先收原 owner；不会等硬截止来掩盖缺失的关闭通知。
        control.close();
        let known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
            .await
            .unwrap();
        let directory = path.join("remote-unattended-v1");
        let state_ok = if mode < 2 {
            !directory.join("delegation.dpapi").exists()
                && std::fs::read(directory.join("commit.journal"))
                    .unwrap()
                    .is_empty()
        } else {
            let backend = WindowsUnattendedBackend::open_existing(&path).unwrap();
            crate::remote_unattended_store::service::test_file_revoked_at(
                backend,
                SingleRunSealer,
                3,
            )
        };
        drop(approval);
        let retired = cleanup.close();
        eprintln!("TEST_ONLY_GLOBAL_CLOSE mode={mode} result={result:?} immediate={immediate} state_ok={state_ok}");
        assert!(immediate && closed_admission && known && state_ok && retired);
        assert_eq!(
            result,
            if mode < 2 {
                Err(crate::remote_unattended_store::StoreError::Conflict)
            } else {
                Ok(())
            }
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_OriginalAppWorkerRetiresBeforeSecondOpen_001() {
    unattended_epoch_case(0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_CancelAfterOriginalJoinNeverReopens_002() {
    unattended_epoch_case(1).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_SecondGenerationWrongSpkiNeverReady_003() {
    unattended_epoch_case(2).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_SecondGenerationOpenFailureNeverReady_004() {
    unattended_epoch_case(3).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_UnknownFreeNeverOpensReplacement_005() {
    const FLAG: &str = "CC_TEST_EPOCH_FREE_UNKNOWN";
    if std::env::var_os(FLAG).is_some() {
        unattended_epoch_case(4).await;
        return;
    }
    use std::os::windows::process::CommandExt;
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let dir = store_root();
    let path = std::path::PathBuf::from(dir.path().to_string_lossy().replace('/', "\\"));
    let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "tests::native_cli_run_supervisor::remote_composition::delegated::preparation::UnattendedEpoch_UnknownFreeNeverOpensReplacement_005", "--nocapture", "--test-threads=1"]).env(FLAG,&path).creation_flags(0x08000000).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let mut child = Child(Some(child));
    let end = Instant::now() + Duration::from_secs(20);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(Instant::now() < end, "owned Unknown child timeout");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("TEST_ONLY_EPOCH_UNKNOWN_RETAINED"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::open_existing(
            &path,
        )
        .unwrap();
    assert!(backend.claim_owner_custody().is_ok());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_PairingCasChangeAfterJoinRejectsNext_006() {
    unattended_epoch_case(6).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_ParentDeadlineAfterJoinRejectsNext_007() {
    unattended_epoch_case(7).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_ParentCancelAtSecondLoaderTailNeverPublishes_008() {
    unattended_epoch_case(8).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_ParentExpiryAtSecondLoaderTailNeverPublishes_009() {
    unattended_epoch_case(9).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_ConsumedOneShotMaterialCannotSelectEpochs_010() {
    unattended_epoch_case(10).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedEpoch_ForeignAcceptedAppPermitCannotClaimSource_011() {
    unattended_epoch_case(11).await;
}
async fn unattended_epoch_case(mode: u8) {
    use crate::remote_unattended_store::service::pending::PendingStart;
    let (f, native, _runtime_stop) = unattended_app_run();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let (approval, mut consent) = if mode == 7 || mode == 9 {
        final_consent_until(&pairing, &admission, cutoff() - 48000)
    } else {
        final_consent(&pairing, &admission)
    };
    if mode == 10 {
        drop(consent.take_supplier().unwrap());
    }
    let foreign_rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let foreign_pair_dir = store_root();
    let foreign_pairing = if mode == 11 {
        Some(Arc::new(
            synthetic_file_store(foreign_pair_dir.path()).unwrap(),
        ))
    } else {
        None
    };
    let foreign = foreign_pairing
        .as_ref()
        .map(|store| final_consent(store, &admit(&f)));
    let close = consent.control();
    let parent_end = consent.end();
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let path = if mode == 4 {
        std::path::PathBuf::from(std::env::var_os("CC_TEST_EPOCH_FREE_UNKNOWN").unwrap())
    } else {
        path
    };
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &path,
        )
        .unwrap();
    let mut pending = PendingStart::new_epochs(
        backend,
        SingleRunSealer,
        consent,
        admission,
        pairing.clone(),
    )
    .unwrap();
    let mut extra_approval = None;
    if let Some((approval, mut other)) = foreign {
        extra_approval = Some(approval);
        let result = foreign_rejected.clone();
        let close = close.clone();
        let pairing = foreign_pairing.as_ref().unwrap().clone();
        pending.test_permit_hook(move |permit| {
            result.store(
                other.check_runtime(&pairing).is_ok() && other.claim_epochs(permit).is_err(),
                std::sync::atomic::Ordering::Release,
            );
            close.close();
        });
    }
    let trace = crate::native_crypto::EpochTestTrace {
        fault: match mode {
            2 => crate::native_crypto::EpochTestFault::WrongSpki,
            3 => crate::native_crypto::EpochTestFault::SecondOpen,
            4 => crate::native_crypto::EpochTestFault::FreeUnknown,
            _ => crate::native_crypto::EpochTestFault::None,
        },
        ..Default::default()
    };
    if mode == 1 {
        let control = close.clone();
        *trace.after_join.lock() = Some(Box::new(move || control.close()));
    }
    if mode == 6 {
        let pairing = pairing.clone();
        *trace.after_join.lock() = Some(Box::new(move || synthetic_forget(&pairing)));
    }
    if mode == 7 {
        *trace.after_join.lock() = Some(Box::new(move || {
            while Instant::now() < parent_end {
                std::thread::sleep(Duration::from_millis(2));
            }
        }));
    }
    if mode == 8 {
        let control = close.clone();
        *trace.after_second_verified.lock() = Some(Box::new(move || control.close()));
    }
    if mode == 9 {
        *trace.after_second_verified.lock() = Some(Box::new(move || {
            while Instant::now() < parent_end {
                std::thread::sleep(Duration::from_millis(2));
            }
        }));
    }
    pending.test_epoch_trace(trace.clone());
    assert_eq!(pending.end(), parent_end);
    let host = Arc::new(RemoteHostService::default());
    host.unattended
        .reserve_start(pending, native.clone())
        .unwrap();
    let limit = Instant::now() + Duration::from_secs(15);
    loop {
        if ((mode == 10 || mode == 11) && host.unattended.start_result().is_some())
            || trace.events.lock().contains(&(2, "ready"))
            || close.closed()
            || Instant::now() >= limit
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let events = trace.events.lock().clone();
    let parent_open = !close.closed() && Instant::now() < parent_end;
    let started = host.unattended.start_result();
    let joined_index = events.iter().position(|e| *e == (1, "original_joined"));
    let second_open = events
        .iter()
        .position(|e| e.0 == 2 && e.1.ends_with("_open"));
    let ordered = matches!((joined_index, second_open), (Some(a), Some(b)) if a < b);
    let two_ready = events.contains(&(1, "ready")) && events.contains(&(2, "ready"));
    let opens = events.iter().filter(|e| e.1.ends_with("_open")).count();
    if mode == 4 {
        let observation = host.unattended.observe();
        let keys = trace.observer.lock().as_ref().unwrap().snapshot();
        let backend=crate::version_history::windows::unattended_store::WindowsUnattendedBackend::open_existing(&path).unwrap();
        let busy = matches!(
            backend.claim_owner_custody(),
            Err(crate::remote_unattended_store::StoreError::Busy)
        );
        let retained = keys.unknown
            && keys.outstanding
            && keys.keys.iter().flatten().any(|k| k.holds_original_join);
        let good = close.closed()
            && opens == 2
            && !two_ready
            && retained
            && busy
            && observation.outstanding
            && !observation.watcher_joined;
        println!("TEST_ONLY_EPOCH_UNKNOWN_RETAINED good={good} parent_closed={} events={events:?} keys={keys:?} app={observation:?} busy={busy}; no App/PTY retirement claim",close.closed());
        // 原 Unknown key/通知/服务 worker 均保留；进程结束仅为隔离负测清理，不是退休回执。
        std::process::exit(if good { 0 } else { 42 });
    }
    let decision = host.request_exit(53, false);
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    host.start_exit(
        53,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        move |code| {
            let _ = tx.send(code);
        },
    );
    let known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
        .await
        .unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
        .await
        .unwrap();
    let observation = host.unattended.observe();
    let facts = host.test_original_managed_facts(&f.request_run()).unwrap();
    drop(approval);
    drop(extra_approval);
    eprintln!("TEST_ONLY_EPOCH events={events:?} parent_open={parent_open} started={started:?} known={known} joined={joined} observation={observation:?} facts={facts:?}");
    assert!(matches!(
        decision,
        crate::remote_host::ExitDecision::Wait(53)
    ));
    assert!(
        known == (mode != 4)
            && joined
            && observation.watcher_joined
            && facts.original_resources_retired()
    );
    if let Ok(code) = rx.try_recv() {
        assert_eq!(code, 53);
    }
    if mode == 10 || mode == 11 {
        if mode == 11 {
            assert!(foreign_rejected.load(std::sync::atomic::Ordering::Acquire));
        }
        use crate::remote_unattended_store::Backend as _;
        let backend=crate::version_history::windows::unattended_store::WindowsUnattendedBackend::open_existing(&path).unwrap();
        let root = std::path::PathBuf::from(backend.target_label().unwrap());
        assert_eq!(
            started,
            Some(Err(
                crate::remote_unattended_store::StoreError::Unauthorized
            ))
        );
        assert_eq!(opens, 0);
        assert!(!root.join("delegation.dpapi").exists());
        assert_eq!(
            std::fs::metadata(root.join("commit.journal"))
                .unwrap()
                .len(),
            0
        );
        return;
    }
    assert_eq!(started, Some(Ok(())));
    match mode {
        0 => assert!(ordered && two_ready && opens == 4 && parent_open),
        1 | 6 | 7 => assert!(joined_index.is_some() && !two_ready && opens == 2 && !parent_open),
        2 | 3 => {
            let final_events = trace.events.lock().clone();
            let fault = if mode == 2 {
                "wrong_spki_export"
            } else {
                "open_error"
            };
            let fault_index = final_events.iter().position(|e| *e == (2, fault));
            assert!(matches!((joined_index, fault_index), (Some(a), Some(b)) if a < b));
            assert!(ordered && !two_ready && !parent_open);
            assert!(!final_events.contains(&(2, "second_verified")));
            assert!(!final_events.contains(&(2, "ready")));
            for (generation, event) in &final_events {
                let free = match *event {
                    "endpoint_open" => "endpoint_free",
                    "relay_open" => "relay_free",
                    _ => continue,
                };
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|e| *e == &(*generation, free))
                        .count(),
                    1
                );
            }
        }
        8 | 9 => assert!(ordered && !two_ready && opens == 4 && !parent_open),
        4 => assert!(
            joined_index.is_none()
                && !two_ready
                && opens == 2
                && !parent_open
                && !observation.retired
        ),
        _ => unreachable!(),
    }
}

#[test]
fn UnattendedCost_RejectedEffectStillPaysOriginalObservation_001() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = synthetic_file_store(pair_dir.path()).unwrap();
    let (approval, mut consent) = final_consent(&pairing, &admission);
    let keys = crate::native_crypto::single_run_test_keys(consent.take_supplier().unwrap());
    let durable_dir = store_root();
    let mut owner = service(durable_dir.path());
    owner
        .start(
            consent,
            keys.custody,
            keys.ready,
            keys.signers,
            admission,
            &pairing,
        )
        .unwrap();
    let before = owner.test_journal_len();
    let wall = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    };
    let sampled = wall();
    while wall() <= sampled {
        std::thread::sleep(Duration::from_millis(1));
    }
    let foreign = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
    let rejected = owner.resize(&pairing, &foreign, 31, 91).is_err();
    let after = owner.test_journal_len();
    let closed = owner.stop();
    retire_service(&mut owner);
    let revoked = owner.test_durable_revoked();
    let final_bytes = owner.test_journal_len();
    drop(owner);
    drop(approval);
    let retired = cleanup.close();
    assert!(rejected && closed.is_ok() && revoked && retired);
    assert_eq!(before, 896);
    assert_eq!(after - before, 448);
    assert_eq!(final_bytes - after, 448);
}

// 公开测试向量匿名 CNG + 原 App reservation/material/stages；不访问命名用户密钥。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedShortNetwork_OriginalAppRelayWasmInput_001() {
    if short_network_isolated("UnattendedShortNetwork_OriginalAppRelayWasmInput_001") {
        return;
    }
    unattended_short_network_case(0).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedShortNetwork_PairingChangeRejectsNextInput_003() {
    if short_network_isolated("UnattendedShortNetwork_PairingChangeRejectsNextInput_003") {
        return;
    }
    unattended_short_network_case(1).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedShortNetwork_IdleFixedDeadlineRetiresBeforeAppExit_004() {
    if short_network_isolated("UnattendedShortNetwork_IdleFixedDeadlineRetiresBeforeAppExit_004") {
        return;
    }
    unattended_short_network_case(2).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedProduct_OriginalTwoApprovalsRelayWasmInput_002() {
    if short_network_isolated("UnattendedProduct_OriginalTwoApprovalsRelayWasmInput_002") {
        return;
    }
    unattended_short_network_case(3).await;
}
async fn unattended_short_network_case(mode: u8) {
    let revoke = mode == 1;
    let expires = mode == 2;
    use crate::remote_host_approval::unattended::final_approval::FinalApproval;
    use crate::remote_native_pairing_store::tests::{record, synthetic_confirmed_record_candidate};
    use crate::remote_unattended_store::service::pending::PendingStart;
    let (f, native, _runtime_stop, document) = unattended_app_run_prepared(|f| {
        if mode != 3 && mode != 4 {
            return None;
        }
        let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
        let authority =
            crate::cli::document::DocumentAuthority::new(f.service.registry().clone(), url.clone())
                .unwrap();
        let mut resources = tauri::ResourceTable::default();
        let binding = Arc::new(authority.attach(&mut resources).unwrap());
        authority.started(&url);
        authority.finished(&url);
        f.caller = authority.test_caller();
        Some((authority, resources, binding))
    });
    let mut record = record();
    record.native.relay_spki = record.native.endpoint_spki.clone();
    record.native.endpoint_spki = network_identities::identity(false, false).1;
    let mut relay = NetworkRelayFixture::start_with_browser(
        Some(record.native.relay_spki[26..].to_vec()),
        30000,
        true,
    )
    .await;
    let admission = admit(&f);
    let scope = admission.scope().unwrap();
    record.route.address = relay.address.to_string();
    record.route.origin = relay.origin.clone();
    record.route.relay_mode = "legacyV1".into();
    record.route.tunnel = [3; 16];
    record.route.relay_id = [1; 16];
    record.epochs.host_key = 5;
    record.epochs.client_key = 7;
    record.epochs.policy = 13;
    record.epochs.issuer = 17;
    record.principal = [4; 32];
    record.browser.webauthn_public_key = record.native.relay_spki.clone();
    record.browser.grant_public_key = record.native.relay_spki.clone();
    record.browser.browser_public_key = network_identities::identity(true, false).1;
    record.browser.relay_public_key = record.browser.browser_public_key.clone();
    let host_spki = record
        .native
        .endpoint_spki
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let candidate = synthetic_confirmed_record_candidate(&pairing, record);
    let selection=serde_json::json!({"storeId":candidate.expected().store_id,"revision":candidate.expected().revision.to_string(),"deviceId":candidate.record().device_id}).to_string();
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let host = Arc::new(RemoteHostService::default());
    let decode = |s: &str| -> [u8; 32] {
        (0..32)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    let mut relay_scalar = [0; 32];
    relay_scalar[31] = 1;
    let opened = Arc::new(std::array::from_fn(|_| {
        std::sync::atomic::AtomicUsize::new(0)
    }));
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let scalars = [
        decode("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"),
        relay_scalar,
    ];
    let mut product_keepalive = None;
    let (approval, fixed_end, control) = if mode == 3 || mode == 4 {
        let product = crate::remote_host::short_session::ShortSessionService::default();
        let binding = &document.as_ref().unwrap().2;
        let id = product
            .test_request(
                &host,
                crate::remote_host::short_session::StartContext {
                    runtime: native.clone(),
                    binding: binding.clone(),
                    caller: f.caller.clone(),
                    admission,
                },
                &selection,
                pairing.clone(),
            )
            .unwrap();
        for _ in 0..3 {
            product.current(&host).unwrap();
        }
        assert_eq!(
            opened
                .iter()
                .map(|v| v.load(std::sync::atomic::Ordering::Acquire))
                .sum::<usize>(),
            0
        );
        product.test_answer(false, true);
        product
            .test_advance::<SingleRunSealer>(&host, binding, &f.caller, &id, || {
                panic!("preparation must not persist")
            })
            .unwrap();
        product.test_answer(true, true);
        product.test_network(crate::remote_host::short_session::TestNetwork {
            scalars,
            opened: opened.clone(),
            accepted,
        });
        product.test_advance(&host,binding,&f.caller,&id,||Ok((crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(&path).unwrap(),SingleRunSealer))).unwrap();
        product_keepalive = Some(product);
        (None, None, None)
    } else {
        let preparation = UnattendedPreparation::new(
            &pairing,
            candidate,
            &admission,
            RequestedRole::Control,
            cutoff(),
        )
        .unwrap();
        let first = UnattendedApproval::test_pending(preparation);
        first.test_answer(true, false);
        let prepared = first.consume(&pairing, &admission).unwrap().unwrap();
        let approval =
            Arc::new(FinalApproval::test_pending_network(prepared, &pairing, &admission).unwrap());
        approval.test_answer(true);
        let consent = approval.consume(&pairing, &admission).unwrap().unwrap();
        let end = consent.end();
        let control = consent.control();
        let backend=crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(&path).unwrap();
        let mut pending = PendingStart::new_network(
            backend,
            SingleRunSealer,
            consent,
            admission,
            pairing.clone(),
            &host,
        )
        .unwrap();
        pending.test_anonymous_network(scalars, opened.clone(), accepted);
        host.unattended
            .reserve_start(pending, native.clone())
            .unwrap();
        (Some(approval), Some(end), Some(control))
    };
    let accepted = tokio::time::timeout(Duration::from_secs(8), ready).await;
    let mut output = None;
    if matches!(accepted, Ok(Ok(()))) {
        let marker = durable_dir.path().join("network-change");
        let marker = marker.to_string_lossy().to_string();
        let config = serde_json::json!({"case":"short-input","origin":relay.origin,"scope":scope,"hostSpki":host_spki,"expires":expires,"revocationPath":if revoke {Some(&marker)} else {None}}).to_string();
        let pairing = pairing.clone();
        output = Some(
            tokio::task::spawn_blocking(move || {
                let mut child = std::process::Command::new("node")
                    .current_dir(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .parent()
                            .unwrap(),
                    )
                    .args([
                        "--experimental-strip-types",
                        if mode == 4 { "remote/ui-terminal/src/browserSourcePullRelay.fixture.ts" } else { "remote/ui-terminal/src/browserSnapshotPullRelay.fixture.ts" },
                        &config,
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let end = Instant::now() + Duration::from_secs(15);
                let mut changed = false;
                while child.try_wait().unwrap().is_none() {
                    if revoke
                        && !changed
                        && std::path::Path::new(&format!("{marker}.ready")).exists()
                    {
                        synthetic_forget(&pairing);
                        changed = true;
                        std::fs::write(format!("{marker}.go"), b"continue").unwrap();
                    }
                    if Instant::now() >= end {
                        let _ = child.kill();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                child.wait_with_output().unwrap()
            })
            .await
            .unwrap(),
        );
    }
    let succeeded = output.as_ref().is_some_and(|o| o.status.success());
    let input_exact = if succeeded {
        let remainder: &[u8] = if revoke || expires {
            b"native"
        } else {
            b"ative"
        };
        f.service
            .access(&f.caller, &f.request_run())
            .unwrap()
            .write_frame(remainder)
            .unwrap();
        report(&f)["input"] == json!([110, 97, 116, 105, 118, 101])
    } else {
        false
    };
    let started = host.unattended.start_result();
    let before_app_exit = if expires {
        let known = tokio::time::timeout(Duration::from_secs(2), host.unattended.wait()).await;
        let observed = host.unattended.observe();
        eprintln!("TEST_ONLY_SHORT_EXPIRED_BEFORE_APP known={known:?} observation={observed:?}");
        matches!(known, Ok(true))
            && control.as_ref().unwrap().closed()
            && Instant::now() >= fixed_end.unwrap()
            && observed.watcher_joined
            && observed.retired
    } else {
        true
    };
    host.request_exit(53, false);
    host.start_exit(
        53,
        native,
        Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
        |_| {},
    );
    let known = tokio::time::timeout(Duration::from_secs(10), host.unattended.wait()).await;
    let joined = tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker()).await;
    let facts = host.test_original_managed_facts(&f.request_run());
    let relay_closed = relay.close().await;
    eprintln!("TEST_ONLY_SHORT_NETWORK accepted={accepted:?} started={started:?} known={known:?} joined={joined:?} facts={facts:?} opens={:?}",opened.iter().map(|n|n.load(std::sync::atomic::Ordering::Acquire)).collect::<Vec<_>>());
    if let Some(output) = &output {
        eprintln!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(
        succeeded
            && before_app_exit
            && input_exact
            && matches!(known, Ok(true))
            && matches!(joined, Ok(true))
            && relay_closed
    );
    assert!(facts.unwrap().original_resources_retired());
    drop(approval);
    drop(product_keepalive);
}

#[test]
fn UnattendedShortNetwork_LocalConsentCannotStartSocket_002() {
    use crate::remote_unattended_store::service::pending::PendingStart;
    use crate::remote_unattended_store::{Backend as _, StoreError};
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let pair_dir = store_root();
    let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
    let (approval, consent) = final_consent(&pairing, &admission);
    let durable_dir = store_root();
    let path = std::path::PathBuf::from(durable_dir.path().to_string_lossy().replace('/', "\\"));
    let backend =
        crate::version_history::windows::unattended_store::WindowsUnattendedBackend::create_new(
            &path,
        )
        .unwrap();
    let root = std::path::PathBuf::from(backend.target_label().unwrap());
    let host = Arc::new(RemoteHostService::default());
    let result =
        PendingStart::new_network(backend, SingleRunSealer, consent, admission, pairing, &host);
    let rejected = matches!(result, Err(StoreError::Unauthorized));
    drop(result);
    let empty = host.unattended.start_result().is_none()
        && !root.join("delegation.dpapi").exists()
        && std::fs::metadata(root.join("commit.journal"))
            .unwrap()
            .len()
            == 0;
    drop(host);
    drop(approval);
    let retired = cleanup.close();
    assert!(rejected && empty && retired);
}

// 原 Host owner 是进程一次性准入；每个网络用例独立进程，不重置安全注册表。
fn short_network_isolated(case: &str) -> bool {
    const KEY: &str = "CC_TEST_SHORT_NETWORK_CASE";
    if std::env::var(KEY).ok().as_deref() == Some(case) {
        return false;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.log");
    let log = std::fs::File::create(&path).unwrap();
    let exact = format!(
        "{}::{case}",
        module_path!()
            .strip_prefix("cc_desk::")
            .unwrap_or(module_path!())
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
        .env(KEY, case)
        .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(log))
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "isolated short network timeout: {}",
                std::fs::read_to_string(&path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let evidence = std::fs::read_to_string(&path).unwrap();
    eprintln!("{evidence}");
    assert!(status.success() && evidence.contains("1 passed; 0 failed"));
    true
}

#[test]
fn UnattendedPreparing_CloseKeepsInflightAppOwned_001() {
    use crate::{cli::native_runtime::NativeRuntime, remote_host::RemoteHostService};
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    let host = RemoteHostService::default();
    let request = host
        .unattended
        .reserve_preparation(native, admission)
        .unwrap();
    let mut operation = request.begin().unwrap();
    host.unattended.request_close();
    let closed = request.closed();
    let outstanding = host.unattended.observe().outstanding;
    let refused = request.begin().is_err();
    operation.complete();
    drop(operation);
    let settled = host.unattended.observe().known();
    assert!(cleanup.close());
    assert!(closed && outstanding && refused && settled);
}

#[test]
fn UnattendedPreparing_CloseCompletionCannotPrecedeOriginalCancel_003() {
    let (f, mut cleanup) = setup();
    let admission = admit(&f);
    let native = Arc::new(NativeRuntime::with_owned_components_for_test(
        f.service.clone(),
        f.transports.clone(),
        f.supervisor.clone(),
    ));
    let host = Arc::new(RemoteHostService::default());
    let request = host
        .unattended
        .reserve_preparation(native, admission)
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    request.test_before_cancel(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let original = host.clone();
    let worker = std::thread::spawn(move || original.unattended.request_close());
    let entered = entered_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    let before = host.unattended.observe();
    let closed = request.closed();
    let _ = release_tx.send(());
    worker.join().unwrap();
    let after = host.unattended.observe();
    assert!(cleanup.close());
    assert!(entered && closed && before.outstanding && !before.known() && after.known());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedProduct_CancelAndForeignAdvanceNeverPersist_004() {
    use crate::remote_host::short_session::{ShortSessionService, StartContext};
    for mode in 0..5 {
        let (mut f, native, _guard, (document, mut resources, binding)) =
            unattended_app_run_prepared(|f| {
                let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
                let document = crate::cli::document::DocumentAuthority::new(
                    f.service.registry().clone(),
                    url.clone(),
                )
                .unwrap();
                let mut resources = tauri::ResourceTable::default();
                let binding = Arc::new(document.attach(&mut resources).unwrap());
                document.started(&url);
                document.finished(&url);
                f.caller = document.test_caller();
                (document, resources, binding)
            });
        let admission = admit(&f);
        let pair_dir = store_root();
        let pairing = Arc::new(synthetic_file_store(pair_dir.path()).unwrap());
        let candidate =
            crate::remote_native_pairing_store::tests::synthetic_confirmed_record_candidate(
                &pairing,
                crate::remote_native_pairing_store::tests::record(),
            );
        let selection=serde_json::json!({"storeId":candidate.expected().store_id,"revision":candidate.expected().revision.to_string(),"deviceId":candidate.record().device_id}).to_string();
        let host = Arc::new(RemoteHostService::default());
        let product = ShortSessionService::default();
        let id = product
            .test_request(
                &host,
                StartContext {
                    runtime: native.clone(),
                    binding: binding.clone(),
                    caller: f.caller.clone(),
                    admission,
                },
                &selection,
                pairing,
            )
            .unwrap();
        let never = || -> Result<(_, SingleRunSealer), crate::cli::types::SafeError> {
            panic!("unapproved flow reached persistence")
        };
        for _ in 0..3 {
            let state = serde_json::to_value(
                product
                    .status_for(&host, &binding, &f.caller, &native)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(state["phase"], "preparing");
        }
        if mode == 1 {
            product.test_answer(false, true);
            product
                .test_advance(&host, &binding, &f.caller, &id, never)
                .unwrap();
        }
        if mode <= 1 {
            product
                .close_for(&host, &binding, &f.caller, &native, &id)
                .unwrap();
            product.test_answer(mode == 1, true);
            assert!(product
                .test_advance(&host, &binding, &f.caller, &id, never)
                .is_err());
        } else if mode == 2 {
            product.test_answer(false, true);
            assert!(product
                .test_advance(
                    &host,
                    &binding,
                    &f.caller,
                    "22222222-2222-4222-8222-222222222222",
                    never
                )
                .is_err());
            product
                .close_for(&host, &binding, &f.caller, &native, &id)
                .unwrap();
        } else if mode == 4 {
            f.service.access(&f.caller, &f.request_run()).unwrap()
                .write_frame(b"native").unwrap();
            assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
            assert!(f.service.access(&f.caller, &f.request_run()).is_err());
            product.status_for(&host, &binding, &f.caller, &native).unwrap();
            product.close_for(&host, &binding, &f.caller, &native, &id).unwrap();
        } else {
            product.test_answer(false, true);
            let url: tauri::Url = "http://tauri.localhost/".parse().unwrap();
            let next = crate::cli::document::DocumentAuthority::new(
                f.service.registry().clone(),
                url.clone(),
            )
            .unwrap();
            let next_binding = Arc::new(next.attach(&mut resources).unwrap());
            next.started(&url);
            next.finished(&url);
            f.caller = next.test_caller();
            assert!(product
                .test_advance(&host, &next_binding, &f.caller, &id, never)
                .is_err());
            product
                .status_for(&host, &next_binding, &f.caller, &native)
                .unwrap();
            product
                .close_for(&host, &next_binding, &f.caller, &native, &id)
                .unwrap();
        }
        assert!(host.unattended.start_result().is_none());
        host.request_exit(55, false);
        host.start_exit(
            55,
            native.clone(),
            Arc::new(crate::remote_client_ipc::RemoteClientService::default()),
            |_| {},
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(10), host.unattended.wait())
                .await
                .unwrap()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(10), host.test_join_exit_worker())
                .await
                .unwrap()
        );
        drop(document);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn UnattendedProduct_ProductionBrowserSourcePull_005() {
    if short_network_isolated("UnattendedProduct_ProductionBrowserSourcePull_005") { return; }
    unattended_short_network_case(4).await;
}
