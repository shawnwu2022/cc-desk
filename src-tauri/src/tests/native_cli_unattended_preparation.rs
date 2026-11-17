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
    use crate::remote_host_approval::unattended::final_approval::FinalApproval;
    let preparation = UnattendedPreparation::new(
        store,
        synthetic_confirmed_candidate(store),
        admission,
        RequestedRole::Control,
        cutoff(),
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
