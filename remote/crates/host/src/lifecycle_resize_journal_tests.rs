use super::*;
use crate::{journal_digest::DigestKey, lifecycle_resize_completion::tests::fact};
fn key() -> DigestKey {
    DigestKey::from_backend_bytes([0x5a; 32])
}
fn accepted(s: &mut Store) -> Ticket {
    s.bind_key(key().lifecycle_key_id()).unwrap();
    let f = fact();
    let t = match s
        .prepare([1; 32], f.request.operation, 1, f.digest)
        .unwrap()
    {
        Prepare::New(t) => t,
        _ => panic!("new"),
    };
    s.transition(&t, Phase::InFlight).unwrap();
    s.transition(&t, Phase::Accepted).unwrap();
    t
}
#[test]
fn lifecycle_resize_journal_schema_readonly_compatibility() {
    for version in [LEGACY_VERSION, VERSION, RESIZE_VERSION, 0x4c4a0004] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("schema.sqlite");
        if version == RESIZE_VERSION {
            drop(Store::open_resize_candidate(&p).unwrap());
            assert!(Store::open(&p).is_err());
        } else {
            let s = Store::open(&p).unwrap();
            if version != VERSION {
                if version == LEGACY_VERSION {
                    s.db.execute_batch("DROP TABLE results").unwrap();
                }
                s.db.pragma_update(None, "user_version", version).unwrap();
            }
            drop(s);
        }
        let before = std::fs::read(&p).unwrap();
        let opened = Store::open_resize_candidate(&p);
        if version == 0x4c4a0004 {
            assert!(opened.is_err());
        } else {
            let mut s = opened.unwrap();
            assert!(!s.resize_writable());
            assert!(s.prepare([1; 32], [2; 16], 1, [3; 32]).is_err());
        }
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }
}
#[test]
fn lifecycle_resize_journal_integrity_and_ticket_binding() {
    for change in 0..8 {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("result.sqlite");
        let mut s = Store::open_resize_candidate(&p).unwrap();
        let t = accepted(&mut s);
        let f = fact();
        s.record_resize(&t, f.backend, &f.resource, &f, &key())
            .unwrap();
        s.record_resize(&t, f.backend, &f.resource, &f, &key())
            .unwrap();
        assert_eq!(
            s.restore_resize(
                [1; 32],
                &f.request,
                f.digest,
                f.backend,
                &f.resource,
                &key()
            )
            .unwrap(),
            Some(f.clone())
        );
        let mut wrong = f.clone();
        wrong.after_size = (5, 12);
        assert!(
            s.record_resize(&t, f.backend, &f.resource, &wrong, &key())
                .is_err()
        );
        match change {
            0 => {
                s.db.execute("UPDATE results SET seal=zeroblob(32)", [])
                    .unwrap();
            }
            1 => {
                s.db.execute("UPDATE results SET payload=zeroblob(length(payload))", [])
                    .unwrap();
            }
            2 => {
                s.db.execute("UPDATE results SET backend=zeroblob(32)", [])
                    .unwrap();
            }
            3 => {
                s.db.execute("UPDATE ops SET digest=zeroblob(32)", [])
                    .unwrap();
            }
            4 => {
                s.db.execute("UPDATE ops SET seq=?1", params![2u64.to_be_bytes()])
                    .unwrap();
            }
            5 => {
                s.db.execute("DELETE FROM results", []).unwrap();
            }
            6 => {
                let mut wrong = f.resource.clone();
                wrong.principal = [9; 32];
                assert!(
                    s.restore_resize([1; 32], &f.request, f.digest, f.backend, &wrong, &key())
                        .is_err()
                );
                continue;
            }
            _ => {
                assert!(
                    s.restore_resize(
                        [1; 32],
                        &f.request,
                        f.digest,
                        f.backend,
                        &f.resource,
                        &DigestKey::from_backend_bytes([9; 32])
                    )
                    .is_err()
                );
                continue;
            }
        }
        assert!(
            s.validate_results([1; 32], Some(f.backend), &key())
                .is_err()
        );
    }
}
#[test]
fn lifecycle_resize_journal_unknown_never_accepts_late_result() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("unknown.sqlite");
    let mut s = Store::open_resize_candidate(&p).unwrap();
    let t = accepted(&mut s);
    let f = fact();
    s.transition(&t, Phase::Uncertain).unwrap();
    assert!(
        s.record_resize(&t, f.backend, &f.resource, &f, &key())
            .is_err()
    );
    assert!(s.prepare([1; 32], [2; 16], 2, [3; 32]).is_err());
    drop(t);
    drop(s);
    let mut s = Store::open_resize_candidate(&p).unwrap();
    assert!(matches!(
        s.prepare([1; 32], f.request.operation, 1, f.digest)
            .unwrap(),
        Prepare::Recorded(Phase::Uncertain)
    ));
    assert_eq!(
        s.restore_resize(
            [1; 32],
            &f.request,
            f.digest,
            f.backend,
            &f.resource,
            &key()
        )
        .unwrap(),
        None
    );
}
#[test]
fn lifecycle_resize_journal_correct_mac_cannot_change_format_or_kind() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("format.sqlite");
    let mut s = Store::open_resize_candidate(&p).unwrap();
    let t = accepted(&mut s);
    let f = fact();
    s.record_resize(&t, f.backend, &f.resource, &f, &key())
        .unwrap();
    assert!(
        s.restore_attach([1; 32], f.request.operation, 1, f.digest, f.backend, &key())
            .is_err()
    );
    s.db.pragma_update(None, "user_version", VERSION).unwrap();
    assert!(
        s.validate_results([1; 32], Some(f.backend), &key())
            .is_err()
    );
    assert!(
        s.restore_resize(
            [1; 32],
            &f.request,
            f.digest,
            f.backend,
            &f.resource,
            &key()
        )
        .is_err()
    );
    drop(t);
    drop(s);
    let old = Store::open(&p).unwrap();
    assert!(
        old.validate_results([1; 32], Some(f.backend), &key())
            .is_err()
    );
}
#[test]
fn lifecycle_resize_journal_v3_keeps_attach_payload_disjoint() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("attach.sqlite");
    let mut s = Store::open_resize_candidate(&p).unwrap();
    let t = accepted(&mut s);
    let f = fact();
    let mut request = f.request.clone();
    request.kind = cc_desk_remote_record::lifecycle::Kind::Attach;
    request.size = None;
    let mut after = request.state;
    after.fence += 1;
    after.sync += 1;
    let event = crate::lifecycle_completion::Applied { request, after };
    let expected = event.encode_stored().unwrap();
    assert_eq!(&expected[..4], b"ATC2");
    s.record_attach(&t, f.backend, &event, &key()).unwrap();
    let stored: Vec<u8> =
        s.db.query_row("SELECT payload FROM results", [], |r| r.get(0))
            .unwrap();
    assert_eq!(stored, expected);
    assert!(s.validate_results([1; 32], Some(f.backend), &key()).is_ok());
    assert!(
        s.restore_resize(
            [1; 32],
            &f.request,
            f.digest,
            f.backend,
            &f.resource,
            &key()
        )
        .is_err()
    );
}
