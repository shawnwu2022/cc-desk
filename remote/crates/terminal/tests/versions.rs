#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::{TerminalCore, TerminalEpochs};

#[test]
fn every_actual_mode_change_is_counted_even_when_batch_final_mode_is_unchanged() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    assert_eq!(core.epochs(), TerminalEpochs::default());
    core.feed(b"\x1b[?1004h\x1b[?1004l").unwrap();
    assert_eq!(core.epochs().mode_epoch, 2);
    assert_eq!(core.epochs().structure_epoch, 0);
    core.feed(b"\x1b[?1004l\x1b[?9001h\x1b[1t").unwrap();
    assert_eq!(core.epochs().mode_epoch, 2);
    assert!(!core.semantic_snapshot().unwrap().focus_reporting());
}

#[test]
fn alternate_buffer_transitions_are_structural_but_repeated_noops_are_not() {
    let mut core = TerminalCore::new(3, 10).unwrap();
    core.feed(b"base\x1b[?1049h\x1b[?1049h\x1b[?1049l\x1b[?1049l")
        .unwrap();
    assert_eq!(core.epochs().mode_epoch, 2);
    assert_eq!(core.epochs().structure_epoch, 2);
    assert_eq!(
        core.semantic_snapshot().unwrap().cell(0, 0).unwrap().scalar,
        'b'
    );
}

#[test]
fn versions_and_visible_semantics_are_equal_across_every_stream_split() {
    let wire = "a\x1b[?1004h\x1b[?1049h\x1b[H中\x1b[?1049l\x1b[?1004l".as_bytes();
    let mut full = TerminalCore::new_with_queries(3, 10).unwrap();
    full.feed(wire).unwrap();
    let expected = full.semantic_snapshot().unwrap();
    assert_eq!(expected.epochs().mode_epoch, 4);
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        core.feed(&wire[..split]).unwrap();
        core.feed(&wire[split..]).unwrap();
        assert_eq!(core.semantic_snapshot().unwrap(), expected);
    }
}

#[test]
fn rejected_tail_or_mark_flood_does_not_apply_prefix_modes_or_versions() {
    for wire in [
        b"\x1b[?1004h\x1b]52;c;x\x07".to_vec(),
        b"\x1b[?1004h\x1b[?1004l".repeat(129),
    ] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        assert!(core.feed(&wire).is_err());
        assert_eq!(core.epochs(), TerminalEpochs::default());
        assert!(core.is_degraded());
    }
}

#[test]
fn only_successful_dimension_changes_advance_dimension_and_structure_epochs() {
    let mut core = TerminalCore::new(3, 10).unwrap();
    core.resize(4, 12).unwrap();
    assert_eq!(
        core.epochs(),
        TerminalEpochs {
            dimension_epoch: 1,
            mode_epoch: 0,
            structure_epoch: 1
        }
    );
    core.resize(4, 12).unwrap();
    assert!(core.resize(4, 1).is_err());
    assert_eq!(
        core.epochs(),
        TerminalEpochs {
            dimension_epoch: 1,
            mode_epoch: 0,
            structure_epoch: 1
        }
    );
}
