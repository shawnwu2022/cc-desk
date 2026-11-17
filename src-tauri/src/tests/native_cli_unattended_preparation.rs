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
