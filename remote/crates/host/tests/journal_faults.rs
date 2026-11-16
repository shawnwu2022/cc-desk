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
fn durable_result_is_returned_without_repeating_effect_after_reopen() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    let mut j = Journal::open(&p).unwrap();
    let mut effects = 0;
    assert_eq!(
        j.run_once(&key(), &KeyedDigest([5; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Ok(Outcome::HostWritten)
    );
    drop(j);
    let mut j = Journal::open(&p).unwrap();
    assert_eq!(
        j.run_once(&key(), &KeyedDigest([5; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Ok(Outcome::HostWritten)
    );
    assert_eq!(effects, 1);
}
#[test]
fn changed_digest_cannot_reuse_a_recorded_operation() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&d.path().join("j.sqlite")).unwrap();
    j.run_once(&key(), &KeyedDigest([5; 32]), || Outcome::Rejected)
        .unwrap();
    let mut effects = 0;
    assert_eq!(
        j.run_once(&key(), &KeyedDigest([6; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Err(JournalError::Conflict)
    );
    assert_eq!(effects, 0);
}
#[test]
fn unknown_outcome_remains_unknown_instead_of_automatically_retrying() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("j.sqlite");
    let mut j = Journal::open(&p).unwrap();
    j.run_once(&key(), &KeyedDigest([5; 32]), || Outcome::Unknown)
        .unwrap();
    drop(j);
    let mut j = Journal::open(&p).unwrap();
    let mut effects = 0;
    assert_eq!(
        j.run_once(&key(), &KeyedDigest([5; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Ok(Outcome::Unknown)
    );
    assert_eq!(effects, 0);
}
#[test]
fn real_process_exit_cuts_never_repeat_external_effect() {
    for (cut, want, effects) in [
        ("before-effect", Outcome::Unknown, 0),
        ("after-effect", Outcome::Unknown, 1),
        ("after-result", Outcome::HostWritten, 1),
    ] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("journal.sqlite");
        let marker = d.path().join("effect");
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_journal-crash-fixture"));
        c.args(["TEST_ONLY_CRASH"])
            .arg(&p)
            .arg(&marker)
            .arg(cut)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000);
        }
        let mut child = c.spawn().unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if std::time::Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("owned crash fixture timeout");
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(status.code(), Some(61));
        assert_eq!(usize::from(marker.exists()), effects);
        let mut j = Journal::open(&p).unwrap();
        let mut repeated = 0;
        assert!(matches!(
            j.query(&key()),
            Ok(ReceiptLookup::Known(ReceiptEnvelope { outcome, .. })) if outcome == want
        ));
        assert_eq!(
            j.run_once(&key(), &KeyedDigest([5; 32]), || {
                repeated += 1;
                Outcome::HostWritten
            }),
            Ok(want)
        );
        assert_eq!(repeated, 0);
    }
}
#[test]
fn failure_to_persist_final_result_is_explicitly_unknown() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("j.sqlite");
    let mut j = Journal::open(&p).unwrap();
    let mut effects = 0;
    let r=j.run_once(&key(),&KeyedDigest([5;32]),||{
        effects+=1;
        let db=rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("CREATE TRIGGER fail_result BEFORE UPDATE ON operations BEGIN SELECT RAISE(ABORT,'TEST_ONLY_STORAGE_FAILURE'); END;").unwrap();
        Outcome::HostWritten
    });
    assert_eq!(r, Ok(Outcome::Unknown));
    assert_eq!(effects, 1);
    assert_eq!(j.query(&key()), Err(JournalError::Schema));
}
#[test]
fn uncertain_scope_blocks_new_sequence_and_other_host_is_independent() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&d.path().join("j.sqlite")).unwrap();
    j.run_once(&key(), &KeyedDigest([5; 32]), || Outcome::Partial)
        .unwrap();
    let mut second = key();
    second.operation = [9; 16];
    second.sequence = 2;
    let mut effects = 0;
    assert_eq!(
        j.run_once(&second, &KeyedDigest([6; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Err(JournalError::Sequence)
    );
    assert_eq!(effects, 0);
    let mut other = key();
    other.host = [8; 32];
    assert_eq!(j.query(&other), Ok(ReceiptLookup::Missing));
    assert_eq!(
        j.run_once(&other, &KeyedDigest([5; 32]), || Outcome::Rejected),
        Ok(Outcome::Rejected)
    );
}
#[test]
fn foreign_database_is_rejected_before_schema_or_mode_changes() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("foreign.sqlite");
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute_batch(
        "CREATE TABLE private_data(value INTEGER);INSERT INTO private_data VALUES(42);",
    )
    .unwrap();
    assert!(matches!(Journal::open(&p), Err(JournalError::Schema)));
    let value: u32 = db
        .query_row("SELECT value FROM private_data", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, 42);
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    let added: u32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('floors','operations')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(added, 0);
}
#[test]
fn final_outcome_without_durable_opaque_proof_is_not_reported_as_success() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("j.sqlite");
    let mut j = Journal::open(&p).unwrap();
    j.run_once(&key(), &KeyedDigest([5; 32]), || Outcome::HostWritten)
        .unwrap();
    let db = rusqlite::Connection::open(&p).unwrap();
    let len: u32 = db
        .query_row("SELECT length(proof) FROM operations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(len, 32);
    db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE operations SET proof=NULL;")
        .unwrap();
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
#[test]
fn after_update_trigger_cannot_forge_final_write_witness() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("j.sqlite");
    let mut j = Journal::open(&p).unwrap();
    let result=j.run_once(&key(),&KeyedDigest([5;32]),||{
        let db=rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("CREATE TRIGGER rewrite_result AFTER UPDATE OF state ON operations BEGIN UPDATE operations SET state=4,proof=NULL WHERE scope=NEW.scope AND op=NEW.op; END;").unwrap();
        Outcome::HostWritten
    });
    assert_eq!(result, Ok(Outcome::Unknown));
    assert_eq!(j.query(&key()), Err(JournalError::Schema));
}
#[test]
fn claimed_identity_with_wrong_schema_is_still_read_only_rejected() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("fake.sqlite");
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute_batch("PRAGMA application_id=0x43434452;PRAGMA user_version=1;CREATE TABLE floors(scope TEXT,last TEXT);CREATE TABLE operations(scope TEXT,op TEXT);CREATE INDEX unresolved ON operations(op);").unwrap();
    assert!(matches!(Journal::open(&p), Err(JournalError::Schema)));
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
}
