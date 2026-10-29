#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::{BufferKind, CellRole, SemanticColor, TerminalCore};

#[test]
fn semantic_snapshot_preserves_host_width_styles_and_cursor() {
    let mut core = TerminalCore::new(3, 10).unwrap();
    core.feed("中\x1b[1;31;44mA".as_bytes()).unwrap();
    let frame = core.semantic_snapshot().unwrap();
    assert_eq!(
        (frame.rows(), frame.columns(), frame.cells().len()),
        (3, 10, 30)
    );
    assert_eq!(frame.buffer(), BufferKind::Normal);
    let lead = frame.cell(0, 0).unwrap();
    assert_eq!(lead.scalar, '中');
    assert_eq!(lead.role, CellRole::WideLead);
    assert_eq!(frame.cell(0, 1).unwrap().role, CellRole::WideSpacer);
    let styled = frame.cell(0, 2).unwrap();
    assert_eq!(styled.scalar, 'A');
    assert!(styled.style.bold);
    assert_eq!(styled.foreground, SemanticColor::Palette(1));
    assert_eq!(styled.background, SemanticColor::Palette(4));
    assert_eq!((frame.cursor().row, frame.cursor().column), (0, 3));
    assert!(!core.remote_control_eligible());
}

#[test]
fn semantic_snapshot_switches_visible_buffer_without_losing_host_normal_state() {
    let mut core = TerminalCore::new(3, 10).unwrap();
    core.feed(b"base").unwrap();
    // Alternate-buffer entry is not a cursor-home instruction.
    core.feed(b"\x1b[?1049h\x1b[Halt").unwrap();
    let alternate = core.semantic_snapshot().unwrap();
    assert_eq!(alternate.buffer(), BufferKind::Alternate);
    assert_eq!(alternate.cell(0, 0).unwrap().scalar, 'a');
    core.feed(b"\x1b[?1049l").unwrap();
    let normal = core.semantic_snapshot().unwrap();
    assert_eq!(normal.buffer(), BufferKind::Normal);
    assert_eq!(normal.cell(0, 0).unwrap().scalar, 'b');
    assert_eq!(normal.cell(0, 3).unwrap().scalar, 'e');
}

#[test]
fn semantic_snapshot_is_identical_across_utf8_and_escape_boundaries() {
    let wire = "中A\x1b[2;3H\x1b[32m文".as_bytes();
    let mut oracle = TerminalCore::new(3, 10).unwrap();
    oracle.feed(wire).unwrap();
    let expected = oracle.semantic_snapshot().unwrap();
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new(3, 10).unwrap();
        core.feed(&wire[..split]).unwrap();
        core.feed(&wire[split..]).unwrap();
        assert_eq!(core.semantic_snapshot().unwrap(), expected);
    }
}

#[test]
fn semantic_snapshot_refuses_degraded_state_and_bounds_payload_debug() {
    let mut core = TerminalCore::new(100, 240).unwrap();
    core.feed(b"PUBLIC_TEST_ONLY_VIEW").unwrap();
    let view = core.semantic_snapshot().unwrap();
    assert_eq!(view.cells().len(), 24_000);
    assert!(view.cell(100, 0).is_none());
    assert!(view.cell(0, 240).is_none());
    assert!(!format!("{view:?}").contains("PUBLIC_TEST_ONLY_VIEW"));
    assert!(core.feed(b"\x1b]52;c;x\x07").is_err());
    assert!(core.semantic_snapshot().is_err());
}

#[test]
fn semantic_snapshot_preserves_right_edge_wide_wrap_and_styled_spaces() {
    use cc_desk_remote_terminal::Underline;
    let mut core = TerminalCore::new(3, 4).unwrap();
    core.feed("abc中".as_bytes()).unwrap();
    let view = core.semantic_snapshot().unwrap();
    assert_eq!(view.cell(0, 3).unwrap().role, CellRole::LeadingWideSpacer);
    assert!(view.cell(0, 3).unwrap().wrap_end);
    assert_eq!(view.cell(1, 0).unwrap().scalar, '中');
    assert_eq!(view.cell(1, 0).unwrap().role, CellRole::WideLead);
    assert_eq!(view.cell(1, 1).unwrap().role, CellRole::WideSpacer);
    core.feed(b"\x1b[3;1H\x1b[1;4;7;32;44m ").unwrap();
    let view = core.semantic_snapshot().unwrap();
    let blank = view.cell(2, 0).unwrap();
    assert_eq!(blank.scalar, ' ');
    assert!(blank.style.bold && blank.style.inverse);
    assert_eq!(blank.style.underline, Underline::Single);
    assert_eq!(blank.background, SemanticColor::Palette(4));
    assert!(format!("{blank:?}").contains("<redacted>"));
}

#[test]
fn one_column_geometry_cannot_admit_a_wide_character_panic() {
    if let Ok(mut core) = TerminalCore::new(3, 1) {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.feed("界".as_bytes())));
        assert!(
            result.is_ok(),
            "admitted one-column geometry reached an upstream panic"
        );
        panic!("one-column geometry violates the locked emulator minimum");
    }
}

#[test]
fn one_column_resize_refuses_before_changing_the_existing_grid() {
    let mut core = TerminalCore::new(3, 2).unwrap();
    core.feed("界".as_bytes()).unwrap();
    let before = core.semantic_snapshot().unwrap();
    assert!(core.resize(3, 1).is_err());
    assert_eq!(core.semantic_snapshot().unwrap(), before);
}
