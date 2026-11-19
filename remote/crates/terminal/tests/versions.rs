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

#[test]
fn bracketed_paste_is_a_real_epoch_fenced_canonical_mode_not_an_input_rewrite() {
    use alacritty_terminal::term::TermMode;
    let wire = b"\x1b[?2004h";
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new_with_queries(24, 80).unwrap();
        core.feed(&wire[..split]).unwrap();
        core.feed(&wire[split..]).unwrap();
        assert!(core.term().mode().contains(TermMode::BRACKETED_PASTE));
        assert_eq!(core.epochs().mode_epoch, 1);
        assert_eq!(core.epochs().structure_epoch, 0);
        assert!(core.semantic_snapshot().unwrap().bracketed_paste());
        core.feed(wire).unwrap();
        assert_eq!(core.epochs().mode_epoch, 1);
        core.feed(b"\x1b[?2004l").unwrap();
        assert!(!core.term().mode().contains(TermMode::BRACKETED_PASTE));
        assert_eq!(core.epochs().mode_epoch, 2);
        assert_eq!(core.rejected_events().pty_writes, 0);
        assert_eq!(core.rejected_events().native_effects, 0);
        assert!(!core.remote_control_eligible());
    }
}

#[test]
fn bracketed_paste_does_not_admit_unreviewed_modes_or_hidden_effects() {
    for wire in [
        b"\x1b[?2005h".as_slice(),
        b"\x1b[?2004;9002h",
        b"\x1b[?2004:1h",
        b"\x1b[?2004h\x1b]52;c;x\x07",
    ] {
        let mut core = TerminalCore::new_with_queries(24, 80).unwrap();
        assert!(core.feed(wire).is_err());
        assert_eq!(core.epochs().mode_epoch, 0);
        assert_eq!(core.rejected_events().clipboard, 0);
    }
}

#[test]
fn bracketed_paste_mixed_modes_preserve_prefix_queries_and_bytewise_state() {
    use cc_desk_remote_terminal::{CanonicalEvent, TerminalFault};
    let wire =
        b"a\x1b[6n\x1b[?2004h\x1b[?1004h\x1b[?1049h\x1b[6n\x1b[?1049l\x1b[?2004l\x1b[?1004l\x1b[6n";
    for chunk in [1, wire.len()] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        let mut replies = Vec::new();
        for part in wire.chunks(chunk) {
            core.feed_with_queries(part, |event| -> Result<(), TerminalFault> {
                if let CanonicalEvent::Query(query) = event {
                    replies.push(query.encode()?.as_bytes().to_vec());
                }
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(
            replies,
            [
                b"\x1b[1;2R".to_vec(),
                b"\x1b[1;2R".to_vec(),
                b"\x1b[1;2R".to_vec()
            ]
        );
        assert_eq!(core.epochs().mode_epoch, 6);
        assert_eq!(core.epochs().structure_epoch, 2);
        assert!(!core.semantic_snapshot().unwrap().bracketed_paste());
    }
    let mut ordinary = TerminalCore::new(3, 10).unwrap();
    assert!(ordinary.feed(b"\x1b[?2004h").is_err());
}

#[test]
fn observed_codex_profile_modes_and_palette_clear_are_canonical_not_native_effects() {
    use alacritty_terminal::term::TermMode;
    use cc_desk_remote_terminal::{SemanticColor, SemanticCursorShape};
    let wire = b"\x1b[?25l\x1b[?1007l\x1b[?1003;1006h\x1b[38;5;6;49mX\x1b[K";
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        core.feed(&wire[..split]).unwrap();
        core.feed(&wire[split..]).unwrap();
        assert_eq!(core.epochs().mode_epoch, 3);
        assert_eq!(core.epochs().structure_epoch, 0);
        assert!(
            core.term()
                .mode()
                .contains(TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE)
        );
        let view = core.semantic_snapshot().unwrap();
        assert_eq!(view.cursor().shape, SemanticCursorShape::Hidden);
        assert_eq!(
            view.cell(0, 0).unwrap().foreground,
            SemanticColor::Palette(6)
        );
        core.feed(b"\x1b[?1003;1006l\x1b[?1007h\x1b[?25h\x1b[2J")
            .unwrap();
        assert_eq!(core.epochs().mode_epoch, 6);
        assert_eq!(core.row_text(0).as_deref(), Some(""));
        assert_eq!(core.rejected_events().native_effects, 0);
    }
}

#[test]
fn observed_profile_rejects_other_mouse_color_and_erase_extensions_atomically() {
    for wire in [
        b"\x1b[?1002h".as_slice(),
        b"\x1b[?1003;1006;25h",
        b"\x1b[38;5;256m",
        b"\x1b[38;2;1;2m",
        b"\x1b[3J",
        b"\x1b[3K",
        b"\x1b[?25l\x1b]52;c;x\x07",
    ] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        assert!(core.feed(wire).is_err());
        assert_eq!(core.epochs().mode_epoch, 0);
    }
}

#[test]
fn canonical_display_profile_does_not_expand_ordinary_terminal_admission() {
    for wire in [
        b"\x1b[?25l".as_slice(),
        b"\x1b[?1007l",
        b"\x1b[?1003;1006h",
        b"\x1b[J",
        b"\x1b[K",
        b"\x1b[38;5;6m",
    ] {
        let mut core = TerminalCore::new(3, 10).unwrap();
        assert!(core.feed(wire).is_err());
        assert_eq!(core.epochs(), TerminalEpochs::default());
    }
}

#[test]
fn individual_mouse_encoding_repeats_do_not_advance_epochs() {
    use alacritty_terminal::term::TermMode;
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    core.feed(b"\x1b[?1006h\x1b[?1006h").unwrap();
    assert_eq!(core.epochs().mode_epoch, 1);
    assert!(core.term().mode().contains(TermMode::SGR_MOUSE));
    core.feed(b"\x1b[?1006l\x1b[?1006l").unwrap();
    assert_eq!(core.epochs().mode_epoch, 2);
    assert!(!core.term().mode().contains(TermMode::SGR_MOUSE));
    core.feed(b"\x1b[?1003;1006h\x1b[?1003;1006h").unwrap();
    assert_eq!(core.epochs().mode_epoch, 3);
    core.feed(b"\x1b[?1003;1006l\x1b[?1003;1006l").unwrap();
    assert_eq!(core.epochs().mode_epoch, 4);
    assert_eq!(core.epochs().structure_epoch, 0);
}
