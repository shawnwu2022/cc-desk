#![cfg(windows)]
use cc_desk_remote_host::journal::Journal;

#[test]
fn cooperating_second_owner_is_rejected_before_opening_another_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.sqlite");
    let first = Journal::open(&path).unwrap();
    let duplicate = Journal::open(&path);
    assert!(duplicate.is_err(), "second live Journal owner was admitted");
    let other_path = directory.path().join("other.sqlite");
    assert!(Journal::open(&other_path).is_err());
    assert!(!other_path.exists());
    drop(first);
    assert!(Journal::open(&path).is_ok());
}
