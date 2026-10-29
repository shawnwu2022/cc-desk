use super::*;
#[test]
fn lifecycle_recovery_corrupt_result_cannot_release_next_admission() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("attach.sqlite");
    let mut s = Store::open(&p).unwrap();
    let t = intent(&mut s);
    s.transition(&t, Phase::InFlight).unwrap();
    s.transition(&t, Phase::Accepted).unwrap();
    s.record_attach(&t, [6; 32], &event(), &key()).unwrap();
    s.db.execute("UPDATE results SET seal=zeroblob(32)", [])
        .unwrap();
    drop(t);
    drop(s);
    let s = Store::open(&p).unwrap();
    assert!(s.validate_results([1; 32], Some([6; 32]), &key()).is_err());
}
use crate::{journal_digest::DigestKey, lifecycle_completion::Applied};
use cc_desk_remote_record::lifecycle::{Kind, Request, State};
fn event() -> Applied {
    let request = Request {
        kind: Kind::Attach,
        operation: [2u8; 16],
        sequence: 1,
        state: State {
            generation: 3,
            fence: 4,
            mode: 0,
            dimension: 0,
            sync: 1,
        },
        size: None,
    };
    let mut after = request.state;
    after.fence += 1;
    after.sync += 1;
    Applied { request, after }
}
fn key() -> DigestKey {
    DigestKey::from_backend_bytes([0x5a; 32])
}
fn intent(s: &mut Store) -> Ticket {
    match s.prepare([1u8; 32], [2u8; 16], 1, [3u8; 32]).unwrap() {
        Prepare::New(t) => t,
        _ => panic!("new"),
    }
}
#[test]
fn lifecycle_recovery_legacy_is_read_only_and_never_admits_or_invents() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("legacy.sqlite");
    let db = Connection::open(&p).unwrap();
    for sql in &SQL[..4] {
        db.execute_batch(sql).unwrap()
    }
    db.pragma_update(None, "application_id", APP).unwrap();
    db.pragma_update(None, "user_version", LEGACY_VERSION)
        .unwrap();
    db.execute(
        "INSERT INTO binding VALUES(1,?1)",
        params![key().lifecycle_key_id()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO floors VALUES(?1,?2)",
        params![[1u8; 32], 1u64.to_be_bytes()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO ops VALUES(?1,?2,?3,?4,3,NULL)",
        params![[1u8; 32], [2u8; 16], 1u64.to_be_bytes(), [3u8; 32]],
    )
    .unwrap();
    drop(db);
    let before = std::fs::read(&p).unwrap();
    let mut s = Store::open(&p).unwrap();
    assert!(s.legacy);
    s.bind_key(key().lifecycle_key_id()).unwrap();
    assert!(matches!(
        s.prepare([1u8; 32], [2u8; 16], 1, [3u8; 32]).unwrap(),
        Prepare::Recorded(Phase::Uncertain)
    ));
    assert!(matches!(
        s.prepare([1u8; 32], [4; 16], 2, [3u8; 32]),
        Err(Error::Sequence)
    ));
    assert_eq!(
        s.restore_attach([1u8; 32], [2u8; 16], 1, [3u8; 32], [6; 32], &key())
            .unwrap(),
        None
    );
    drop(s);
    assert_eq!(std::fs::read(&p).unwrap(), before);
}
#[test]
fn lifecycle_recovery_atomic_result_commit_cuts_and_corruption() {
    for cut in [
        None,
        Some(Cut::BeforeTransition),
        Some(Cut::AfterTransition),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("attach.sqlite");
        let mut s = Store::open(&p).unwrap();
        let t = intent(&mut s);
        s.transition(&t, Phase::InFlight).unwrap();
        s.transition(&t, Phase::Accepted).unwrap();
        s.fault = cut;
        let result = s.record_attach(&t, [6; 32], &event(), &key());
        assert_eq!(result.is_ok(), cut.is_none());
        drop(t);
        drop(s);
        let mut s = Store::open(&p).unwrap();
        let got = s
            .restore_attach([1u8; 32], [2u8; 16], 1, [3u8; 32], [6; 32], &key())
            .unwrap();
        if cut == Some(Cut::BeforeTransition) {
            assert_eq!(got, None);
            assert!(matches!(
                s.prepare([1u8; 32], [2u8; 16], 1, [3u8; 32]).unwrap(),
                Prepare::Recorded(Phase::Uncertain)
            ))
        } else {
            assert_eq!(got, Some(event()));
            assert!(
                s.restore_attach([1u8; 32], [2u8; 16], 1, [3u8; 32], [7; 32], &key())
                    .is_err()
            );
            s.db.execute("UPDATE results SET payload=zeroblob(length(payload))", [])
                .unwrap();
            assert!(
                s.restore_attach([1u8; 32], [2u8; 16], 1, [3u8; 32], [6; 32], &key())
                    .is_err()
            );
        }
    }
}
#[test]
fn lifecycle_recovery_unknown_schema_is_rejected_without_change() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("attach.sqlite");
    drop(Store::open(&p).unwrap());
    let db = Connection::open(&p).unwrap();
    db.pragma_update(None, "user_version", 0x4c4affff).unwrap();
    drop(db);
    let before = std::fs::read(&p).unwrap();
    assert!(matches!(Store::open(&p), Err(Error::Schema)));
    assert_eq!(std::fs::read(&p).unwrap(), before);
}
