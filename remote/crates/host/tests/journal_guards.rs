#![cfg(windows)]
use cc_desk_remote_host::journal::*;
fn key() -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_RUN".into(),
        generation: 1,
        incarnation: 1,
        device: [3; 32],
        operation: [4; 16],
        sequence: 1,
    }
}
#[test]
fn oversized_owned_files_reject_before_sqlite_recovery_or_mode_changes() {
    for (suffix, limit) in [("", 64 * 1024 * 1024u64), ("-wal", 16 * 1024 * 1024u64)] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.sqlite");
        drop(Journal::open(&p).unwrap());
        let mut name = p.as_os_str().to_owned();
        name.push(suffix);
        let target = std::path::PathBuf::from(name);
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&target)
            .unwrap();
        f.set_len(limit + 1).unwrap();
        drop(f);
        assert!(matches!(Journal::open(&p), Err(JournalError::Capacity)));
        assert_eq!(std::fs::metadata(&target).unwrap().len(), limit + 1);
    }
}
#[test]
fn malformed_persisted_row_shape_is_storage_error_in_both_read_paths() {
    for sql in [
        "UPDATE operations SET digest=zeroblob(31)",
        "UPDATE operations SET seq=zeroblob(7)",
        "UPDATE operations SET proof=zeroblob(31)",
        "UPDATE operations SET state=4,proof=zeroblob(32)",
    ] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.sqlite");
        let mut j = Journal::open(&p).unwrap();
        j.run_once(&key(), &KeyedDigest([5; 32]), || Outcome::HostWritten)
            .unwrap();
        let db = rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        db.execute_batch(sql).unwrap();
        assert_eq!(j.query(&key()), Err(JournalError::Storage));
        let mut effects = 0;
        assert_eq!(
            j.run_once(&key(), &KeyedDigest([5; 32]), || {
                effects += 1;
                Outcome::HostWritten
            }),
            Err(JournalError::Storage)
        );
        assert_eq!(effects, 0);
    }
}
#[test]
fn windows_junction_and_hardlink_do_not_redirect_journal() {
    let d = tempfile::tempdir().unwrap();
    let actual = d.path().join("actual");
    let link = d.path().join("link");
    std::fs::create_dir(&actual).unwrap();
    let p = actual.join("j.sqlite");
    drop(Journal::open(&p).unwrap());
    let before = std::fs::read(&p).unwrap();
    let ps = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut c = std::process::Command::new(ps);
    c.args(["-NoProfile","-NonInteractive","-Command","New-Item -ItemType Junction -Path $env:CC_TEST_JUNCTION -Target $env:CC_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("CC_TEST_JUNCTION",&link).env("CC_TEST_TARGET",&actual).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x08000000);
    assert!(
        c.status().unwrap().success(),
        "owned standard-user junction creation unavailable"
    );
    use std::os::windows::fs::MetadataExt;
    assert_ne!(
        std::fs::symlink_metadata(&link).unwrap().file_attributes() & 0x400,
        0
    );
    assert!(matches!(
        Journal::open(&link.join("j.sqlite")),
        Err(JournalError::Bounds)
    ));
    assert!(
        before == std::fs::read(&p).unwrap(),
        "junction target changed"
    );
    // Remove just the owned junction; never recursively traverse its target.
    std::fs::remove_dir(&link).unwrap();
    let alias = actual.join("alias.sqlite");
    std::fs::hard_link(&p, &alias).unwrap();
    assert!(matches!(Journal::open(&alias), Err(JournalError::Bounds)));
    assert!(
        before == std::fs::read(&p).unwrap(),
        "hardlink target changed"
    );
    std::fs::remove_file(alias).unwrap();
}
#[test]
fn owned_file_and_parent_names_remain_pinned_until_journal_drop() {
    let d = tempfile::tempdir().unwrap();
    let parent = d.path().join("private");
    std::fs::create_dir(&parent).unwrap();
    let p = parent.join("j.sqlite");
    let j = Journal::open(&p).unwrap();
    assert!(std::fs::rename(&p, parent.join("other.sqlite")).is_err());
    assert!(std::fs::rename(&parent, d.path().join("renamed")).is_err());
    drop(j);
    std::fs::rename(&p, parent.join("other.sqlite")).unwrap();
}
