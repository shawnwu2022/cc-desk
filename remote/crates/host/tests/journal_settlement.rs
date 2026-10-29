#![cfg(windows)]

use cc_desk_remote_host::journal::*;

fn key(sequence: u64) -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_RUN".into(),
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [sequence as u8; 16],
        sequence,
    }
}

fn crash_key() -> OperationKey {
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

fn final_receipt(journal: &mut Journal, sequence: u64, outcome: Outcome) -> ReceiptEnvelope {
    let key = key(sequence);
    assert_eq!(
        journal.run_once(&key, &KeyedDigest([sequence as u8; 32]), || outcome),
        Ok(outcome)
    );
    match journal.receipt(&key).unwrap() {
        ReceiptLookup::Known(receipt) => receipt,
        other => panic!("expected known receipt, got {other:?}"),
    }
}

fn ack(proofs: Vec<ReceiptProof>) -> SettlementAck {
    let through = proofs.last().map(|proof| proof.key.sequence).unwrap_or(1);
    SettlementAck {
        target: key(1).target(),
        device: [5; 32],
        through,
        proofs,
    }
}

#[test]
fn receipt_tokens_are_redacted_and_only_final_exact_rows_return_proof() {
    let d = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(&d.path().join("journal.sqlite")).unwrap();
    let final_receipt = final_receipt(&mut journal, 1, Outcome::HostWritten);
    assert_eq!(final_receipt.key, key(1));
    assert_eq!(final_receipt.outcome, Outcome::HostWritten);
    let token = final_receipt.receipt_proof.as_ref().unwrap();
    assert_eq!(token.encoded_len(), 32);
    let debug = format!("{token:?}");
    assert_eq!(debug, "ReceiptToken([REDACTED])");
    assert!(!debug.contains("010101"));

    let partial_key = key(2);
    journal
        .run_once(&partial_key, &KeyedDigest([2; 32]), || Outcome::Partial)
        .unwrap();
    match journal.receipt(&partial_key).unwrap() {
        ReceiptLookup::Known(receipt) => {
            assert_eq!(receipt.outcome, Outcome::Partial);
            assert!(receipt.receipt_proof.is_none());
        }
        other => panic!("expected known partial receipt, got {other:?}"),
    }
}

#[test]
fn receipt_lookup_rejects_schema_changed_after_open_without_returning_proof() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    let mut journal = Journal::open(&p).unwrap();
    final_receipt(&mut journal, 1, Outcome::HostWritten);
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute_batch("CREATE TABLE TEST_ONLY_EXTRA(value INTEGER) STRICT;")
        .unwrap();
    assert_eq!(journal.receipt(&key(1)), Err(JournalError::Schema));
    assert_eq!(journal.query(&key(1)), Err(JournalError::Schema));
}

#[test]
fn active_sequence_distinguishes_wrong_operation_from_missing_durable_row() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    let mut journal = Journal::open(&p).unwrap();
    final_receipt(&mut journal, 1, Outcome::HostWritten);
    let mut wrong_operation = key(1);
    wrong_operation.operation = [99; 16];
    assert_eq!(journal.query(&wrong_operation), Ok(ReceiptLookup::Missing));

    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute("DELETE FROM operations", []).unwrap();
    assert_eq!(journal.query(&key(1)), Err(JournalError::Storage));
}

#[test]
fn exact_contiguous_proofs_advance_settlement_and_retire_details_without_replay() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    let mut journal = Journal::open(&p).unwrap();
    let first = final_receipt(&mut journal, 1, Outcome::HostWritten);
    let second = final_receipt(&mut journal, 2, Outcome::Rejected);
    let committed_batch = ack(vec![
        first.into_proof().unwrap(),
        second.into_proof().unwrap(),
    ]);
    let decision = journal.acknowledge(&[5; 32], &committed_batch);
    assert_eq!(decision, Ok(AckDecision::Advanced(2)));
    assert_eq!(
        journal.acknowledge(&[5; 32], &committed_batch),
        Ok(AckDecision::AlreadySettled(2))
    );
    assert_eq!(journal.query(&key(1)), Ok(ReceiptLookup::TooOld));
    assert_eq!(journal.query(&key(2)), Ok(ReceiptLookup::TooOld));
    let mut effects = 0;
    assert_eq!(
        journal.run_once(&key(1), &KeyedDigest([1; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Err(JournalError::Sequence)
    );
    assert_eq!(effects, 0);

    drop(journal);
    let db = rusqlite::Connection::open(&p).unwrap();
    let (admitted, settled): (Vec<u8>, Vec<u8>) = db
        .query_row("SELECT admitted,settled FROM floors", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(u64::from_be_bytes(admitted.try_into().unwrap()), 2);
    assert_eq!(u64::from_be_bytes(settled.try_into().unwrap()), 2);
    let rows: u32 = db
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn too_old_is_a_settled_prefix_classification_and_does_not_attest_operation_identity() {
    let d = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(&d.path().join("journal.sqlite")).unwrap();
    let first = final_receipt(&mut journal, 1, Outcome::HostWritten);
    assert_eq!(
        journal.acknowledge(&[5; 32], &ack(vec![first.into_proof().unwrap()])),
        Ok(AckDecision::Advanced(1))
    );
    let mut never_existed = key(1);
    never_existed.operation = [99; 16];
    assert_eq!(journal.query(&never_existed), Ok(ReceiptLookup::TooOld));

    let mut reused_operation = key(2);
    reused_operation.operation = key(1).operation;
    journal
        .run_once(&reused_operation, &KeyedDigest([2; 32]), || {
            Outcome::HostWritten
        })
        .unwrap();
    assert_eq!(journal.query(&key(1)), Ok(ReceiptLookup::TooOld));
    assert!(matches!(
        journal.query(&reused_operation),
        Ok(ReceiptLookup::Known(ReceiptEnvelope {
            outcome: Outcome::HostWritten,
            ..
        }))
    ));
}

#[test]
fn older_ack_batch_rejects_after_later_advance_without_reopening_retired_state() {
    let d = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(&d.path().join("journal.sqlite")).unwrap();
    let first_batch = ack(vec![
        final_receipt(&mut journal, 1, Outcome::HostWritten)
            .into_proof()
            .unwrap(),
    ]);
    assert_eq!(
        journal.acknowledge(&[5; 32], &first_batch),
        Ok(AckDecision::Advanced(1))
    );
    let second_batch = SettlementAck {
        target: key(2).target(),
        device: [5; 32],
        through: 2,
        proofs: vec![
            final_receipt(&mut journal, 2, Outcome::Rejected)
                .into_proof()
                .unwrap(),
        ],
    };
    assert_eq!(
        journal.acknowledge(&[5; 32], &second_batch),
        Ok(AckDecision::Advanced(2))
    );
    assert_eq!(
        journal.acknowledge(&[5; 32], &first_batch),
        Ok(AckDecision::Rejected(
            AckRejectReason::MissingOrMismatchedProof
        ))
    );
    assert_eq!(journal.query(&key(1)), Ok(ReceiptLookup::TooOld));
    assert_eq!(journal.query(&key(2)), Ok(ReceiptLookup::TooOld));
    assert_eq!(
        journal.acknowledge(&[5; 32], &second_batch),
        Ok(AckDecision::AlreadySettled(2))
    );
    let mut effects = 0;
    assert_eq!(
        journal.run_once(&key(1), &KeyedDigest([1; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Err(JournalError::Sequence)
    );
    assert_eq!(effects, 0);
}

#[test]
fn ack_rejects_peer_target_key_outcome_token_order_and_bare_through_atomically() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    let mut journal = Journal::open(&p).unwrap();
    let first = final_receipt(&mut journal, 1, Outcome::HostWritten)
        .into_proof()
        .unwrap();
    let second = final_receipt(&mut journal, 2, Outcome::Rejected)
        .into_proof()
        .unwrap();

    let mut cases = Vec::new();
    cases.push(([9; 32], ack(vec![first.clone(), second.clone()])));
    let mut self_reported = ack(vec![first.clone(), second.clone()]);
    self_reported.device = [9; 32];
    cases.push(([5; 32], self_reported));
    let mut wrong_target = ack(vec![first.clone(), second.clone()]);
    wrong_target.target.generation += 1;
    cases.push(([5; 32], wrong_target));
    let mut wrong_key = first.clone();
    wrong_key.key.operation = [99; 16];
    cases.push(([5; 32], ack(vec![wrong_key, second.clone()])));
    let mut wrong_outcome = first.clone();
    wrong_outcome.outcome = Outcome::Rejected;
    cases.push(([5; 32], ack(vec![wrong_outcome, second.clone()])));
    let mut wrong_token = second.clone();
    wrong_token.key = key(1);
    wrong_token.outcome = Outcome::HostWritten;
    cases.push(([5; 32], ack(vec![wrong_token, second.clone()])));
    cases.push(([5; 32], ack(vec![second.clone(), first.clone()])));
    cases.push(([5; 32], ack(vec![first.clone(), first.clone()])));
    cases.push((
        [5; 32],
        SettlementAck {
            target: key(1).target(),
            device: [5; 32],
            through: 2,
            proofs: Vec::new(),
        },
    ));

    for (expected_device, invalid) in cases {
        assert!(matches!(
            journal.acknowledge(&expected_device, &invalid),
            Ok(AckDecision::Rejected(_))
        ));
        assert!(matches!(
            journal.receipt(&key(1)),
            Ok(ReceiptLookup::Known(_))
        ));
    }
    drop(journal);
    let db = rusqlite::Connection::open(&p).unwrap();
    let settled: Vec<u8> = db
        .query_row("SELECT settled FROM floors", [], |row| row.get(0))
        .unwrap();
    assert_eq!(u64::from_be_bytes(settled.try_into().unwrap()), 0);
    let rows: u32 = db
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 2);
}

#[test]
fn unknown_hole_too_many_proofs_and_old_ack_never_advance_a_new_window() {
    let d = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(&d.path().join("journal.sqlite")).unwrap();
    let first = final_receipt(&mut journal, 1, Outcome::HostWritten)
        .into_proof()
        .unwrap();
    journal
        .run_once(&key(2), &KeyedDigest([2; 32]), || Outcome::Unknown)
        .unwrap();
    let with_hole = SettlementAck {
        target: key(1).target(),
        device: [5; 32],
        through: 2,
        proofs: vec![first.clone()],
    };
    assert!(matches!(
        journal.acknowledge(&[5; 32], &with_hole),
        Ok(AckDecision::Rejected(_))
    ));

    let valid = ack(vec![first.clone()]);
    assert_eq!(
        journal.acknowledge(&[5; 32], &valid),
        Ok(AckDecision::Advanced(1))
    );
    assert_eq!(
        journal.acknowledge(&[5; 32], &valid),
        Ok(AckDecision::AlreadySettled(1))
    );
    let donor = tempfile::tempdir().unwrap();
    let mut donor_journal = Journal::open(&donor.path().join("donor.sqlite")).unwrap();
    let wrong_old_token = final_receipt(&mut donor_journal, 1, Outcome::HostWritten)
        .into_proof()
        .unwrap();
    assert!(matches!(
        journal.acknowledge(&[5; 32], &ack(vec![wrong_old_token])),
        Ok(AckDecision::Rejected(
            AckRejectReason::MissingOrMismatchedProof
        ))
    ));

    let mut too_many = Vec::new();
    for sequence in 1..=33 {
        let mut proof = first.clone();
        proof.key.sequence = sequence;
        proof.key.operation = [sequence as u8; 16];
        too_many.push(proof);
    }
    let invalid = SettlementAck {
        target: key(1).target(),
        device: [5; 32],
        through: 33,
        proofs: too_many,
    };
    assert!(matches!(
        journal.acknowledge(&[5; 32], &invalid),
        Ok(AckDecision::Rejected(_))
    ));
}

#[test]
fn schema_v1_is_rejected_read_only_without_silent_migration() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("legacy.sqlite");
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute_batch(
        "PRAGMA application_id=0x43434452;PRAGMA user_version=1;\
         CREATE TABLE floors(scope BLOB NOT NULL PRIMARY KEY,last BLOB NOT NULL CHECK(length(last)=8)) STRICT;\
         CREATE TABLE operations(scope BLOB NOT NULL,op BLOB NOT NULL CHECK(length(op)=16),seq BLOB NOT NULL CHECK(length(seq)=8),digest BLOB NOT NULL CHECK(length(digest)=32),state INTEGER NOT NULL CHECK(state IN (0,1,2,3,4)),proof BLOB,PRIMARY KEY(scope,op),CHECK((state IN (1,2) AND proof IS NOT NULL AND length(proof)=32) OR (state IN (0,3,4) AND proof IS NULL))) STRICT;\
         CREATE UNIQUE INDEX unresolved ON operations(scope) WHERE state IN (0,3,4);",
    )
    .unwrap();
    assert!(matches!(Journal::open(&p), Err(JournalError::Schema)));
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
}

#[test]
fn durable_scope_cap_does_not_evict_unknown_floor_evidence() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("journal.sqlite");
    drop(Journal::open(&p).unwrap());
    let mut db = rusqlite::Connection::open(&p).unwrap();
    let tx = db.transaction().unwrap();
    for n in 0_u32..4096 {
        tx.execute(
            "INSERT INTO floors(scope,admitted,settled,settled_op,settled_state,settled_proof,settled_ack_hash) VALUES(?1,?2,?3,NULL,NULL,NULL,NULL)",
            rusqlite::params![n.to_be_bytes().as_slice(), 1_u64.to_be_bytes().as_slice(), 0_u64.to_be_bytes().as_slice()],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    let mut journal = Journal::open(&p).unwrap();
    let mut effects = 0;
    assert_eq!(
        journal.run_once(&key(1), &KeyedDigest([1; 32]), || {
            effects += 1;
            Outcome::HostWritten
        }),
        Err(JournalError::Capacity)
    );
    assert_eq!(effects, 0);
    drop(journal);
    let db = rusqlite::Connection::open(&p).unwrap();
    let count: u32 = db
        .query_row("SELECT count(*) FROM floors", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 4096);
}

#[test]
fn process_exit_ack_cuts_preserve_no_replay_and_atomic_retirement() {
    for (cut, committed) in [
        ("settlement-before", false),
        ("settlement-uncommitted", false),
        ("settlement-committed-lost", true),
    ] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("journal.sqlite");
        let marker = d.path().join("effect");
        let mut setup = Journal::open(&p).unwrap();
        setup
            .run_once(&crash_key(), &KeyedDigest([5; 32]), || {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&marker)
                    .unwrap();
                file.write_all(b"x").unwrap();
                file.sync_all().unwrap();
                Outcome::HostWritten
            })
            .unwrap();
        let retained_proof = match setup.receipt(&crash_key()).unwrap() {
            ReceiptLookup::Known(receipt) => receipt.into_proof().unwrap(),
            other => panic!("missing setup proof: {other:?}"),
        };
        drop(setup);
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_journal-crash-fixture"));
        command
            .args(["TEST_ONLY_CRASH"])
            .arg(&p)
            .arg(&marker)
            .arg(cut)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let status = command.status().unwrap();
        assert_eq!(status.code(), Some(61), "cut {cut}");
        assert_eq!(std::fs::read(&marker).unwrap(), b"x", "cut {cut}");

        let mut journal = Journal::open(&p).unwrap();
        if committed {
            assert_eq!(journal.receipt(&crash_key()), Ok(ReceiptLookup::TooOld));
            let old_ack = SettlementAck {
                target: crash_key().target(),
                device: [3; 32],
                through: 1,
                proofs: vec![retained_proof],
            };
            assert_eq!(
                journal.acknowledge(&[3; 32], &old_ack),
                Ok(AckDecision::AlreadySettled(1))
            );
        } else {
            let receipt = match journal.receipt(&crash_key()).unwrap() {
                ReceiptLookup::Known(receipt) => receipt,
                other => panic!("cut {cut} lost unacknowledged detail: {other:?}"),
            };
            let unsettled_ack = SettlementAck {
                target: crash_key().target(),
                device: [3; 32],
                through: 1,
                proofs: vec![receipt.into_proof().unwrap()],
            };
            assert_eq!(
                journal.acknowledge(&[3; 32], &unsettled_ack),
                Ok(AckDecision::Advanced(1))
            );
        }
        let mut replayed = 0;
        assert_eq!(
            journal.run_once(&crash_key(), &KeyedDigest([5; 32]), || {
                replayed += 1;
                Outcome::HostWritten
            }),
            Err(JournalError::Sequence)
        );
        assert_eq!(replayed, 0, "cut {cut}");
    }
}
