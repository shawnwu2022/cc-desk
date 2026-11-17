#![cfg(windows)]
#![allow(non_snake_case)]
use cc_desk_remote_host::journal::{
    Journal, JournalError, KeyedDigest, OperationKey, Outcome, PrepareDecision, PreparedOperation,
    ReceiptLookup,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

fn key(session: u8) -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [session; 16],
        run: "PUBLIC_TEST_ONLY_J2A".into(),
        generation: 1,
        incarnation: 1,
        device: [3; 32],
        operation: [4; 16],
        sequence: 1,
    }
}
fn prepared(journal: &mut Journal, key: &OperationKey) -> PreparedOperation {
    match journal.prepare(key, &KeyedDigest([5; 32])).unwrap() {
        PrepareDecision::Prepared(p) => p,
        PrepareDecision::Recorded(_) => panic!("expected fresh durable intent"),
    }
}

#[test]
fn J2A_SingleOwner_001() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.sqlite");
    let first = Journal::open(&path).unwrap();
    let other = Journal::open(&path);
    let rejected = matches!(other, Err(JournalError::OwnerBusy));
    drop(other);
    drop(first);
    assert!(rejected, "second cooperating owner was admitted");
    assert!(Journal::open(&path).is_ok());
}

#[test]
fn J2A_SharedJournalProgress_002() {
    let directory = tempfile::tempdir().unwrap();
    let journal = Arc::new(Mutex::new(
        Journal::open(&directory.path().join("journal.sqlite")).unwrap(),
    ));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let a = journal.clone();
    let first = std::thread::spawn(move || {
        let token = { prepared(&mut a.lock().unwrap(), &key(1)) };
        let _ = entered_tx.send(());
        let outcome = if release_rx.recv_timeout(Duration::from_secs(5)).is_ok() {
            Outcome::HostWritten
        } else {
            Outcome::Unknown
        };
        a.lock().unwrap().finalize(token, outcome)
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    let b = journal.clone();
    let second = std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(3);
        let token = loop {
            if Instant::now() >= end {
                let _ = done_tx.send(false);
                return;
            }
            if let Ok(mut owner) = b.try_lock() {
                break prepared(&mut owner, &key(2));
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let result = b.lock().unwrap().finalize(token, Outcome::HostWritten);
        let _ = done_tx.send(result == Ok(Outcome::HostWritten) && Instant::now() < end);
    });
    let progressed = done_rx
        .recv_timeout(Duration::from_secs(4))
        .unwrap_or(false);
    // 断言之前先解开并 join 自有线程；这是注入的阻塞效果，不是内核 pipe 背压。
    let _ = release_tx.send(());
    let first_result = first.join().unwrap();
    second.join().unwrap();
    assert!(entered);
    assert_eq!(first_result, Ok(Outcome::HostWritten));
    assert!(progressed, "B could not complete while A effect was parked");
}

#[test]
fn J2A_TokenRetainsOwnerAndUnknown_003() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("journal.sqlite");
    let mut j = Journal::open(&path).unwrap();
    let token = prepared(&mut j, &key(1));
    assert!(matches!(
        j.prepare(&key(1), &KeyedDigest([5; 32])),
        Ok(PrepareDecision::Recorded(Outcome::Unknown))
    ));
    drop(j);
    assert!(matches!(Journal::open(&path), Err(JournalError::OwnerBusy)));
    let other_dir = tempfile::tempdir().unwrap();
    let mut independent = Journal::open(&other_dir.path().join("other.sqlite")).unwrap();
    assert_eq!(
        independent.run_once(&key(9), &KeyedDigest([5; 32]), || Outcome::HostWritten),
        Ok(Outcome::HostWritten)
    );
    drop(token);
    let mut j = Journal::open(&path).unwrap();
    let row = match j.query(&key(1)).unwrap() {
        ReceiptLookup::Known(row) => row,
        _ => panic!("missing unknown"),
    };
    assert_eq!(row.outcome, Outcome::Unknown);
    assert!(row.receipt_proof.is_none());
    let mut next = key(1);
    next.sequence = 2;
    next.operation = [8; 16];
    assert!(matches!(
        j.prepare(&next, &KeyedDigest([5; 32])),
        Err(JournalError::Sequence)
    ));
    assert!(matches!(
        j.prepare(&key(1), &KeyedDigest([6; 32])),
        Err(JournalError::Conflict)
    ));
    let foreign = prepared(&mut j, &key(2));
    assert_eq!(
        independent.finalize(foreign, Outcome::HostWritten),
        Err(JournalError::Conflict)
    );
    assert!(matches!(
        j.prepare(&key(2), &KeyedDigest([5; 32])),
        Ok(PrepareDecision::Recorded(Outcome::Unknown))
    ));
}

#[test]
fn J2A_FinalProofAfterFinalize_004() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("journal.sqlite");
    let mut j = Journal::open(&path).unwrap();
    let token = prepared(&mut j, &key(1));
    let before = match j.query(&key(1)).unwrap() {
        ReceiptLookup::Known(row) => row,
        _ => panic!("intent missing"),
    };
    assert_eq!(before.outcome, Outcome::Unknown);
    assert!(before.receipt_proof.is_none());
    assert_eq!(
        j.finalize(token, Outcome::HostWritten),
        Ok(Outcome::HostWritten)
    );
    let row = match j.query(&key(1)).unwrap() {
        ReceiptLookup::Known(row) => row,
        _ => panic!("result missing"),
    };
    assert_eq!(row.outcome, Outcome::HostWritten);
    assert!(row.receipt_proof.is_some());
    drop(j);
    let mut j = Journal::open(&path).unwrap();
    assert_eq!(j.query(&key(1)), Ok(ReceiptLookup::Known(row)));
    assert!(matches!(
        j.prepare(&key(1), &KeyedDigest([5; 32])),
        Ok(PrepareDecision::Recorded(Outcome::HostWritten))
    ));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _token = prepared(&mut j, &key(2));
        panic!("PUBLIC_TEST_ONLY dropped effect token");
    }));
    assert!(panicked.is_err());
    assert!(matches!(
        j.prepare(&key(2), &KeyedDigest([5; 32])),
        Ok(PrepareDecision::Recorded(Outcome::Unknown))
    ));
}

#[test]
fn J2A_CheckedOwnerSidecar_005() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("journal.sqlite");
    let owner = d.path().join(".cc-desk-journal.owner");
    std::fs::write(&owner, b"PUBLIC_TEST_ONLY_NOT_EMPTY").unwrap();
    assert!(matches!(Journal::open(&path), Err(JournalError::Bounds)));
    assert!(!path.exists());
    std::fs::remove_file(&owner).unwrap();
    let target = d.path().join("link-target");
    std::fs::write(&target, b"").unwrap();
    std::fs::hard_link(&target, &owner).unwrap();
    assert!(matches!(Journal::open(&path), Err(JournalError::Bounds)));
    assert!(!path.exists());
    std::fs::remove_file(&owner).unwrap();
    match std::os::windows::fs::symlink_file(&target, &owner) {
        Ok(()) => {
            assert!(Journal::open(&path).is_err());
            assert!(!path.exists());
            std::fs::remove_file(&owner).unwrap();
        }
        Err(error) => eprintln!(
            "[SKIP] J2A_owner_symlink privilege/platform unavailable: {:?}",
            error.kind()
        ),
    }
    let j = Journal::open(&path).unwrap();
    assert!(std::fs::remove_file(&owner).is_err());
    assert!(std::fs::rename(&owner, d.path().join("replaced.owner")).is_err());
    drop(j);
    assert!(Journal::open(&path).is_ok());
}

#[test]
fn J2A_DirectoryOwnerPrecedesDifferentDatabaseCreation_006() {
    let d = tempfile::tempdir().unwrap();
    let first = Journal::open(&d.path().join("first.sqlite")).unwrap();
    let second_path = d.path().join("different.sqlite");
    let other = Journal::open(&second_path);
    let rejected = matches!(other, Err(JournalError::OwnerBusy));
    let untouched = !second_path.exists();
    drop(other);
    drop(first);
    assert!(
        rejected,
        "same physical metadata directory admitted another owner"
    );
    assert!(
        untouched,
        "loser created its database before owner rejection"
    );
}

#[test]
fn J2A_ReservedOwnerIsNeverDatabase_007() {
    let d = tempfile::tempdir().unwrap();
    let owner = d.path().join(".cc-desk-journal.owner");
    std::fs::write(&owner, b"").unwrap();
    let result = Journal::open(&owner);
    assert!(
        matches!(result, Err(JournalError::Bounds)),
        "reserved target must reject before owner locking/SQLite classification"
    );
    assert!(std::fs::read(&owner).unwrap().is_empty());
    for name in [
        ".CC-DESK-JOURNAL.OWNER",
        ".cc-desk-journal.owner.",
        ".cc-desk-journal.owner:PUBLIC_TEST_ONLY",
    ] {
        assert!(matches!(
            Journal::open(&d.path().join(name)),
            Err(JournalError::Bounds)
        ));
        assert!(std::fs::read(&owner).unwrap().is_empty());
    }
    // 使用真正 Journal 创建合格 DB；所有 connection/owner 已关闭后再布置反例。
    let qualified = d.path().join("qualified.sqlite");
    drop(Journal::open(&qualified).unwrap());
    let image = std::fs::read(&qualified).unwrap();
    assert!(image.len() >= 100 && &image[..16] == b"SQLite format 3\0");
    std::fs::remove_file(&owner).unwrap();
    std::fs::rename(&qualified, &owner).unwrap();
    assert_eq!(std::fs::read(&owner).unwrap(), image);
    assert!(matches!(Journal::open(&owner), Err(JournalError::Bounds)));
    assert_eq!(std::fs::read(&owner).unwrap(), image);
    eprintln!("TEST_ONLY_J2A qualified_db_reserved_target_rejected_complete_bytes_unchanged=true");
}

#[test]
fn J2A_OwnPathShortAliases_008() {
    use std::{
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
    };
    fn alias(path: &Path) -> Option<PathBuf> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 32768];
        let count = unsafe {
            windows::Win32::Storage::FileSystem::GetShortPathNameW(
                windows::core::PCWSTR(wide.as_ptr()),
                Some(&mut buffer),
            )
        } as usize;
        if count == 0 || count >= buffer.len() {
            return None;
        }
        let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]));
        if short.file_name() == path.file_name() {
            None
        } else {
            Some(short)
        }
    }
    let d = tempfile::tempdir().unwrap();
    let owner = d.path().join(".cc-desk-journal.owner");
    std::fs::write(&owner, b"").unwrap();
    match alias(&owner) {
        Some(short) => {
            assert!(matches!(Journal::open(&short), Err(JournalError::Bounds)));
            assert!(std::fs::read(&owner).unwrap().is_empty());
            eprintln!("TEST_ONLY_J2A reserved_owner_fileid_alias_rejected=true");
        }
        None => {
            eprintln!("[SKIP] J2A_reserved_owner_short_alias unavailable on owned fixture volume")
        }
    }
    let path = d
        .path()
        .join("PUBLIC_TEST_ONLY_LONG_DATABASE_FILENAME.sqlite");
    let journal = Journal::open(&path).unwrap();
    match alias(&path) {
        Some(short) => {
            assert!(matches!(
                Journal::open(&short),
                Err(JournalError::OwnerBusy)
            ));
            eprintln!("TEST_ONLY_J2A db_short_alias_same_directory_owner=true");
        }
        None => eprintln!("[SKIP] J2A_database_short_alias unavailable on owned fixture volume"),
    };
    drop(journal);
}
