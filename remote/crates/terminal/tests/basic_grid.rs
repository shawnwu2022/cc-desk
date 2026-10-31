#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::TerminalCore;

#[test]
fn basic_grid_redraw_erases_old_cells_without_native_effects() {
    let mut core = TerminalCore::new(3, 8).unwrap();
    core.feed(b"ABC\r\nDEF").unwrap();
    core.feed(b"\x1b[2J\x1b[Hok\x1b[K").unwrap();
    assert_eq!(core.row_text(0).as_deref(), Some("ok"));
    assert_eq!(core.row_text(1).as_deref(), Some(""));
    let snapshot = core.semantic_snapshot().unwrap();
    assert_eq!((snapshot.cursor().row, snapshot.cursor().column), (0, 2));
    assert_eq!(core.rejected_events(), Default::default());
    assert!(!core.remote_control_eligible());
}

#[test]
fn basic_grid_relative_cursor_moves_use_bounded_emulator_state() {
    let mut core = TerminalCore::new(3, 8).unwrap();
    core.feed(b"\x1b[2;3HX\x1b[1DL\x1b[1AU\x1b[1CR\x1b[1BD")
        .unwrap();
    for (row, column, scalar) in [(1, 2, 'L'), (0, 3, 'U'), (0, 5, 'R'), (1, 6, 'D')] {
        assert_eq!(core.cell(row, column).unwrap().c, scalar);
    }
    core.feed(b"\x1b[65535B\x1b[65535C").unwrap();
    let snapshot = core.semantic_snapshot().unwrap();
    assert_eq!((snapshot.cursor().row, snapshot.cursor().column), (2, 7));
    assert_eq!(snapshot.cells().len(), 24);
    assert_eq!(core.rejected_events(), Default::default());
}

#[test]
fn basic_grid_is_identical_at_every_escape_stream_boundary() {
    let bytes = b"stale\r\nold\x1b[2J\x1b[Hnew\x1b[1D!\x1b[0K";
    let mut whole = TerminalCore::new(3, 8).unwrap();
    whole.feed(bytes).unwrap();
    let expected = whole.semantic_snapshot().unwrap();
    for split in 0..=bytes.len() {
        let mut core = TerminalCore::new(3, 8).unwrap();
        core.feed(&bytes[..split]).unwrap();
        core.feed(&bytes[split..]).unwrap();
        assert_eq!(core.semantic_snapshot().unwrap(), expected);
    }
}

#[test]
fn basic_grid_rejects_unreviewed_variants_before_any_batch_effect() {
    for sequence in [
        "3J", "3K", "2;0J", "2:0J", "?2J", "1;2A", "1:2B", "?1C", "1 D",
    ] {
        let mut core = TerminalCore::new(3, 8).unwrap();
        assert!(
            core.feed(format!("prefix\x1b[{sequence}").as_bytes())
                .is_err()
        );
        assert_eq!(core.row_text(0).as_deref(), Some(""));
        assert!(core.feed(b"later").is_err());
        assert_eq!(core.rejected_events(), Default::default());
    }
}

#[test]
fn basic_grid_refuses_erase_above_before_the_locked_handler_can_publish_stale_rows() {
    let mut core = TerminalCore::new(3, 8).unwrap();
    core.feed(b"ABC\r\nDEF").unwrap();
    let result = core.feed(b"\x1b[1J");
    if result.is_ok() {
        eprintln!(
            "TEST_ONLY_ERASE_ABOVE_RETAINED_PRIOR_ROW={}",
            core.row_text(0).as_deref() == Some("ABC")
        );
    }
    assert!(result.is_err(), "unqualified erase-above must fail closed");
    assert_eq!(core.row_text(0).as_deref(), Some("ABC"));
    assert_eq!(core.row_text(1).as_deref(), Some("DEF"));
    assert!(core.semantic_snapshot().is_err());
}

#[test]
fn basic_grid_supported_erase_modes_preserve_exact_visible_cells() {
    for (mode, expected) in [(0, "abc"), (1, "    ef"), (2, "")] {
        let mut core = TerminalCore::new(3, 8).unwrap();
        core.feed(b"abcdef\x1b[1;4H").unwrap();
        core.feed(format!("\x1b[{mode}K").as_bytes()).unwrap();
        assert_eq!(core.row_text(0).as_deref(), Some(expected));
        assert!(core.semantic_snapshot().is_ok());
        assert_eq!(core.rejected_events(), Default::default());
    }
    for default in ["", "0"] {
        let mut core = TerminalCore::new(3, 8).unwrap();
        core.feed(b"ABC\r\nDEF\x1b[1;2H").unwrap();
        core.feed(format!("\x1b[{default}J").as_bytes()).unwrap();
        assert_eq!(core.row_text(0).as_deref(), Some("A"));
        assert_eq!(core.row_text(1).as_deref(), Some(""));
        assert!(core.semantic_snapshot().is_ok());
        assert_eq!(core.rejected_events(), Default::default());
    }
}
