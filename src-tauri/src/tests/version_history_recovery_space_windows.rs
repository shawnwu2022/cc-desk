//! Disposable real NTFS objects only; these tests do not admit source effects.
use crate::version_history::{
    journal::{CapacityPlan, JournalBinding, JournalStore},
    windows::{
        pre_context_abort::publish_source_transition,
        recovery_space::{
            probe_reserve_fault, AbortReserve, ObservedAbortReserve, ReserveFault,
            ReserveObservationState, ABORT_RESERVE_FILENAME, MINIMUM_ABORT_BYTES,
        },
        startup::{InstallationControl, TransactionDataRoot},
    },
};
use std::{io::Read, mem::size_of, os::windows::io::AsRawHandle, path::PathBuf, sync::Arc};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{FileStandardInfo, GetFileInformationByHandleEx, FILE_STANDARD_INFO},
};

// 检查128MiB真实分配在关闭后保留，释放保留所有权记录且拒绝第二次执行。
#[test]
fn HistoryReserve_AllocationRelease_001() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000801".into(),
        source_context: "00000000-0000-4000-8000-000000000802".into(),
        target_context: "00000000-0000-4000-8000-000000000803".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let data = Arc::new(
        TransactionDataRoot::create(installation.clone(), &control, &binding.transaction_id)
            .unwrap(),
    );
    let mut journal = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    publish_source_transition(&installation, &control, &mut journal, &binding).unwrap();
    let mut reserve = AbortReserve::create(
        data.clone(),
        installation.clone(),
        &control,
        &mut journal,
        &binding,
        0,
    )
    .unwrap();
    reserve.verify_for(&data, &installation, &binding).unwrap();
    let path = PathBuf::from(data.root().directory().path().unwrap()).join(ABORT_RESERVE_FILENAME);
    assert!(
        std::fs::OpenOptions::new().write(true).open(&path).is_err(),
        "a held reserve must exclude external writers"
    );
    assert!(
        std::fs::remove_file(&path).is_err(),
        "a held reserve must exclude deletion"
    );
    let reference = reserve.record().clone();
    let observed =
        ObservedAbortReserve::open(data.clone(), installation.clone(), &control, &reference)
            .unwrap();
    let before = observed.inspect().unwrap();
    assert!(
        before.allocated_bytes >= MINIMUM_ABORT_BYTES + 4096,
        "EOF alone must not satisfy the 128 MiB allocation requirement"
    );
    assert_eq!(before.state, ReserveObservationState::Held);
    drop(observed);
    let receipt = reserve.release_once(&control).unwrap();
    assert!(
        receipt.released_bytes() >= MINIMUM_ABORT_BYTES,
        "release must positively deallocate the minimum reserve"
    );
    assert!(
        reserve.release_once(&control).is_err(),
        "the release must be one attempt only"
    );
    assert!(
        reserve.verify_for(&data, &installation, &binding).is_err(),
        "a released reserve cannot admit source effects"
    );
    drop(reserve);
    let observed =
        ObservedAbortReserve::open(data.clone(), installation.clone(), &control, &reference)
            .unwrap();
    let after = observed.inspect().unwrap();
    assert_eq!(
        after.logical_bytes, 4096,
        "the ownership record must remain in its original file"
    );
    assert_eq!(after.state, ReserveObservationState::ReleaseAttempted);
    assert_eq!(after.allocated_bytes, receipt.remaining_allocation());
    drop(observed);
    assert!(
        AbortReserve::create(data, installation, &control, &mut journal, &binding, 0).is_err(),
        "the retained slot must never be reused after release"
    );
}

// 检查已有占位文件不被覆盖，错误控制根和错误日志不能创建预留文件。
#[test]
fn HistoryReserve_PreserveCollision_002() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000811".into(),
        source_context: "00000000-0000-4000-8000-000000000812".into(),
        target_context: "00000000-0000-4000-8000-000000000813".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let data = Arc::new(
        TransactionDataRoot::create(installation.clone(), &control, &binding.transaction_id)
            .unwrap(),
    );
    let mut journal = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    publish_source_transition(&installation, &control, &mut journal, &binding).unwrap();
    let foreign_temporary = tempfile::tempdir().unwrap();
    let foreign = InstallationControl::fixture(foreign_temporary.path()).unwrap();
    let foreign_control = foreign.acquire_control().unwrap();
    assert!(
        AbortReserve::create(
            data.clone(),
            foreign.clone(),
            &foreign_control,
            &mut journal,
            &binding,
            0
        )
        .is_err(),
        "data and control must belong to the same actual installation"
    );
    let mut foreign_journal =
        JournalStore::create_windows_transaction(foreign.root().clone(), &binding.transaction_id)
            .unwrap();
    foreign_journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    assert!(
        AbortReserve::create(
            data.clone(),
            installation.clone(),
            &control,
            &mut foreign_journal,
            &binding,
            0
        )
        .is_err(),
        "a same-binding journal from another control root must fail"
    );
    let path = PathBuf::from(data.root().directory().path().unwrap()).join(ABORT_RESERVE_FILENAME);
    assert!(
        !path.exists(),
        "failed root admission must precede creation"
    );
    std::fs::write(&path, b"foreign file: preserve exactly").unwrap();
    let failure =
        match AbortReserve::create(data, installation, &control, &mut journal, &binding, 0) {
            Ok(_) => panic!("an existing reserve slot must be rejected"),
            Err(failure) => failure,
        };
    assert!(
        failure.into_partial().is_none(),
        "collision must not become cleanup authority"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"foreign file: preserve exactly"
    );
}

// 检查真实64MiB分配后注入磁盘满，保留类型化证据并只释放该次创建的尾部。
#[test]
fn HistoryReserve_PartialFailure_003() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000821".into(),
        source_context: "00000000-0000-4000-8000-000000000822".into(),
        target_context: "00000000-0000-4000-8000-000000000823".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let data = Arc::new(
        TransactionDataRoot::create(installation.clone(), &control, &binding.transaction_id)
            .unwrap(),
    );
    let mut journal = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    publish_source_transition(&installation, &control, &mut journal, &binding).unwrap();
    let _fault = probe_reserve_fault(ReserveFault::PartialAllocation);
    let failure = match AbortReserve::create(
        data.clone(),
        installation.clone(),
        &control,
        &mut journal,
        &binding,
        0,
    ) {
        Ok(_) => panic!("injected disk-full must prevent source admission"),
        Err(failure) => failure,
    };
    let mut partial = failure
        .into_partial()
        .expect("positively created partial file must remain owned");
    assert!(
        partial.inspect().unwrap().allocated_bytes >= MINIMUM_ABORT_BYTES / 2,
        "the failure fixture must contain a real allocation"
    );
    let receipt = partial.release_owned_partial_once(&control).unwrap();
    assert!(
        receipt.released_bytes() >= MINIMUM_ABORT_BYTES / 2,
        "owned partial capacity must be recoverable without deleting its record"
    );
    assert!(
        partial.release_owned_partial_once(&control).is_err(),
        "partial cleanup is also one attempt only"
    );
    assert_eq!(partial.inspect().unwrap().logical_bytes, 4096);
}

// 检查释放系统调用后的失败保留不确定标记，重试不能再次截断文件。
#[test]
fn HistoryReserve_UnknownRelease_004() {
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000831".into(),
        source_context: "00000000-0000-4000-8000-000000000832".into(),
        target_context: "00000000-0000-4000-8000-000000000833".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let data = Arc::new(
        TransactionDataRoot::create(installation.clone(), &control, &binding.transaction_id)
            .unwrap(),
    );
    let mut journal = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    publish_source_transition(&installation, &control, &mut journal, &binding).unwrap();
    let mut reserve = AbortReserve::create(
        data.clone(),
        installation.clone(),
        &control,
        &mut journal,
        &binding,
        0,
    )
    .unwrap();
    let reference = reserve.record().clone();
    let _fault = probe_reserve_fault(ReserveFault::AfterReleaseAllocation);
    assert!(
        reserve.release_once(&control).is_err(),
        "failure after the syscall must remain uncertain"
    );
    assert!(
        reserve.release_once(&control).is_err(),
        "uncertain release must not retry"
    );
    drop(reserve);
    let observed = ObservedAbortReserve::open(data, installation, &control, &reference).unwrap();
    assert_eq!(
        observed.inspect().unwrap().state,
        ReserveObservationState::ReleaseAttempted
    );
}

// 检查重启保留真实分配，复制所有权记录或稀疏大EOF不能冒充原预留对象。
#[test]
fn HistoryReserve_RejectForeignEof_005() {
    use windows::Win32::System::{Ioctl::FSCTL_SET_SPARSE, IO::DeviceIoControl};
    let temporary = tempfile::tempdir().unwrap();
    let installation = InstallationControl::fixture(temporary.path()).unwrap();
    let control = installation.acquire_control().unwrap();
    let binding = JournalBinding {
        transaction_id: "00000000-0000-4000-8000-000000000841".into(),
        source_context: "00000000-0000-4000-8000-000000000842".into(),
        target_context: "00000000-0000-4000-8000-000000000843".into(),
        user_installation: "1".repeat(64),
        source_bundle: "2".repeat(64),
        target_package: "3".repeat(64),
        target_payload: "4".repeat(64),
        roots: "5".repeat(64),
    };
    let data = Arc::new(
        TransactionDataRoot::create(installation.clone(), &control, &binding.transaction_id)
            .unwrap(),
    );
    let mut journal = JournalStore::create_windows_transaction(
        installation.root().clone(),
        &binding.transaction_id,
    )
    .unwrap();
    journal
        .initialize(
            binding.clone(),
            CapacityPlan::for_effects(10, 10, 10, 4096).unwrap(),
        )
        .unwrap();
    publish_source_transition(&installation, &control, &mut journal, &binding).unwrap();
    let reserve = AbortReserve::create(
        data.clone(),
        installation.clone(),
        &control,
        &mut journal,
        &binding,
        0,
    )
    .unwrap();
    let reference = reserve.record().clone();
    let path = PathBuf::from(data.root().directory().path().unwrap()).join(ABORT_RESERVE_FILENAME);
    drop(reserve);
    let observed =
        ObservedAbortReserve::open(data.clone(), installation.clone(), &control, &reference)
            .unwrap();
    assert!(
        observed.inspect().unwrap().allocated_bytes >= MINIMUM_ABORT_BYTES + 4096,
        "a closed and reopened reserve must retain its allocation"
    );
    drop(observed);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.set_len(4096).unwrap();
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            FSCTL_SET_SPARSE,
            None,
            0,
            None,
            0,
            Some(&mut returned),
            None,
        )
        .unwrap();
    }
    file.set_len(MINIMUM_ABORT_BYTES + 4096).unwrap();
    file.sync_all().unwrap();
    let mut standard = FILE_STANDARD_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
        .unwrap();
    }
    assert!(
        standard.AllocationSize < standard.EndOfFile,
        "negative fixture must be a real sparse large-EOF file"
    );
    drop(file);
    assert!(
        ObservedAbortReserve::open(data.clone(), installation.clone(), &control, &reference)
            .is_err(),
        "sparse EOF cannot establish allocated reserve"
    );
    let mut header = vec![0; 4096];
    std::fs::File::open(&path)
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    std::fs::rename(&path, path.with_extension("foreign")).unwrap();
    std::fs::write(&path, &header).unwrap();
    assert!(
        ObservedAbortReserve::open(data, installation, &control, &reference).is_err(),
        "copied record bytes cannot transfer original file identity"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        header,
        "rejected observation must leave foreign bytes intact"
    );
}
