use super::*;
pub(crate) fn fact() -> Fact {
    let before = State {
        generation: 3,
        fence: 4,
        mode: 5,
        dimension: 6,
        sync: 7,
    };
    Fact {
        request: Request {
            kind: Kind::Resize,
            operation: [1; 16],
            sequence: 1,
            state: before,
            size: Some((4, 12)),
        },
        resource: ResourceIdentity {
            host: "11".repeat(32),
            session: "22".repeat(16),
            run: "TEST_ONLY_RSC1".into(),
            generation: "3".into(),
            incarnation: "4".into(),
            principal: [5; 32],
        },
        backend: [6; 32],
        digest: [7; 32],
        before_size: (3, 10),
        after_size: (4, 12),
        before_terminal: TerminalEpochs {
            mode_epoch: 5,
            dimension_epoch: 6,
            structure_epoch: 8,
        },
        after_terminal: TerminalEpochs {
            mode_epoch: 5,
            dimension_epoch: 7,
            structure_epoch: 9,
        },
        after: State {
            dimension: 7,
            ..before
        },
        outcome: Outcome::Applied,
    }
}
#[test]
fn lifecycle_resize_codec_roundtrip_and_disjoint_attach() {
    let f = fact();
    let b = f.encode_stored().unwrap();
    assert_eq!(&b[..4], b"RSC1");
    assert_eq!(Fact::decode_stored(&b).unwrap(), f);
    assert!(crate::lifecycle_completion::Applied::decode_stored(&b).is_err());
}
#[test]
fn lifecycle_resize_codec_geometry_transition_and_truncation() {
    let f = fact();
    let b = f.encode_stored().unwrap();
    for n in 0..b.len() {
        assert!(Fact::decode_stored(&b[..n]).is_err(), "{n}");
    }
    let mut extra = b.clone();
    extra.push(0);
    assert!(Fact::decode_stored(&extra).is_err());
    assert!(Fact::decode_stored(&vec![0; 4097]).is_err());
    for mode in 0..10 {
        let mut bad = f.clone();
        match mode {
            0 => bad.request.kind = Kind::Attach,
            1 => bad.request.size = Some((100, 240)),
            2 => bad.backend = [0; 32],
            3 => bad.resource.generation = "03".into(),
            4 => bad.resource.principal = [0; 32],
            5 => bad.after.fence += 1,
            6 => bad.after.sync += 1,
            7 => bad.after_terminal.structure_epoch += 1,
            8 => bad.before_terminal.mode_epoch += 1,
            _ => bad.outcome = Outcome::NotApplied,
        };
        assert!(bad.encode_stored().is_err(), "{mode}");
    }
    let mut same = f.clone();
    same.request.size = Some(same.before_size);
    same.after_size = same.before_size;
    same.after = same.request.state;
    same.after_terminal = same.before_terminal;
    assert!(same.encode_stored().is_ok());
    same.outcome = Outcome::NotApplied;
    assert!(same.encode_stored().is_ok());
    for structure in [false, true] {
        let mut bad = f.clone();
        if structure {
            bad.before_terminal.structure_epoch = u64::MAX;
        } else {
            bad.before_terminal.dimension_epoch = u64::MAX;
            bad.request.state.dimension = u64::MAX;
        }
        assert!(bad.encode_stored().is_err());
    }
}
#[test]
fn lifecycle_resize_codec_can_describe_existing_core_larger_than_request_budget() {
    let mut f = fact();
    f.before_size = (100, 240);
    assert!(f.encode_stored().is_ok());
}
