use super::*;
fn key() -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_CURSOR".into(),
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    }
}
#[test]
fn durable_cursor_recovers_exact_unsettled_and_only_advances_after_settlement() {
    for outcome in [
        Outcome::HostWritten,
        Outcome::Rejected,
        Outcome::Partial,
        Outcome::Unknown,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cursor.sqlite");
        let mut j = Journal::open(&path).unwrap();
        let k = key();
        assert_eq!(
            j.input_cursor(&k.target(), &k.device)
                .unwrap()
                .next_sequence,
            Some(1)
        );
        j.run_once(&k, &KeyedDigest([7; 32]), || outcome).unwrap();
        drop(j);
        let mut j = Journal::open(&path).unwrap();
        let c = j.input_cursor(&k.target(), &k.device).unwrap();
        assert_eq!(c.next_sequence, None);
        let pending = c.pending.unwrap();
        assert_eq!(pending.key, k);
        assert_eq!(pending.outcome, outcome);
        if let Some(proof) = pending.into_proof() {
            assert_eq!(
                j.acknowledge(
                    &k.device,
                    &SettlementAck {
                        target: k.target(),
                        device: k.device,
                        through: 1,
                        proofs: vec![proof]
                    }
                )
                .unwrap(),
                AckDecision::Advanced(1)
            );
            drop(j);
            let j = Journal::open(&path).unwrap();
            let c = j.input_cursor(&k.target(), &k.device).unwrap();
            assert_eq!(c.next_sequence, Some(2));
            assert!(c.pending.is_none());
        } else {
            assert!(matches!(outcome, Outcome::Partial | Outcome::Unknown));
        }
    }
}
#[test]
fn durable_cursor_never_invents_progress_from_missing_row_or_wrong_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("cursor.sqlite")).unwrap();
    let k = key();
    j.run_once(&k, &KeyedDigest([7; 32]), || Outcome::Unknown)
        .unwrap();
    assert_eq!(
        j.input_cursor(&k.target(), &[8; 32]).unwrap().next_sequence,
        Some(1)
    );
    let s = scope(&k).unwrap();
    j.connection
        .execute(
            "DELETE FROM operations WHERE scope=?1",
            rusqlite::params![&s],
        )
        .unwrap();
    assert!(j.input_cursor(&k.target(), &k.device).is_err());
}
#[test]
fn durable_cursor_rejects_overflow_and_schema_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("cursor.sqlite")).unwrap();
    let k = key();
    j.run_once(&k, &KeyedDigest([7; 32]), || Outcome::HostWritten)
        .unwrap();
    let ReceiptLookup::Known(e) = j.receipt(&k).unwrap() else {
        panic!("receipt");
    };
    j.acknowledge(
        &k.device,
        &SettlementAck {
            target: k.target(),
            device: k.device,
            through: 1,
            proofs: vec![e.into_proof().unwrap()],
        },
    )
    .unwrap();
    j.connection
        .execute(
            "UPDATE floors SET admitted=?1,settled=?1",
            rusqlite::params![&u64::MAX.to_be_bytes()[..]],
        )
        .unwrap();
    assert!(matches!(
        j.input_cursor(&k.target(), &k.device),
        Err(JournalError::Sequence)
    ));
    j.connection.execute("DROP INDEX unresolved", []).unwrap();
    assert!(matches!(
        j.input_cursor(&k.target(), &k.device),
        Err(JournalError::Schema)
    ));
}
