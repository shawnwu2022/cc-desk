#![cfg(feature = "experimental-emulator")]

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::ClipboardType;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use cc_desk_remote_terminal::{
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_FEED_BYTES, MAX_TERMINAL_ROWS, TerminalCore,
    TerminalCoreError, TerminalEventSink, TerminalFault, TerminalSafeCode,
};

#[test]
fn rejects_invalid_geometry_before_term_construction() {
    assert_eq!(
        TerminalCore::new(0, 80).unwrap_err(),
        TerminalCoreError::ZeroRows
    );
    assert_eq!(
        TerminalCore::new(24, 0).unwrap_err(),
        TerminalCoreError::ZeroColumns
    );
    assert_eq!(
        TerminalCore::new(MAX_TERMINAL_ROWS + 1, 80).unwrap_err(),
        TerminalCoreError::RowsExceeded {
            requested: MAX_TERMINAL_ROWS + 1,
            maximum: MAX_TERMINAL_ROWS,
        }
    );
    assert_eq!(
        TerminalCore::new(24, MAX_TERMINAL_COLUMNS + 1).unwrap_err(),
        TerminalCoreError::ColumnsExceeded {
            requested: MAX_TERMINAL_COLUMNS + 1,
            maximum: MAX_TERMINAL_COLUMNS,
        }
    );
}

#[test]
fn renders_ascii_and_wide_chinese_text() {
    let mut core = TerminalCore::new(4, 12).unwrap();

    core.feed("A中B".as_bytes()).unwrap();

    assert_eq!(core.cell(0, 0).unwrap().c, 'A');
    assert_eq!(core.cell(0, 1).unwrap().c, '中');
    assert!(core.cell(0, 1).unwrap().flags.contains(Flags::WIDE_CHAR));
    assert!(
        core.cell(0, 2)
            .unwrap()
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    );
    assert_eq!(core.cell(0, 3).unwrap().c, 'B');
}

#[test]
fn bounded_sgr_preserves_color_bold_and_reset_without_native_effects() {
    let mut core = TerminalCore::new(4, 12).unwrap();
    core.feed(b"\x1b[1;31;44mR\x1b[0mN").unwrap();
    let red = core.cell(0, 0).unwrap();
    assert_eq!(red.c, 'R');
    assert_eq!(red.fg, Color::Named(NamedColor::Red));
    assert_eq!(red.bg, Color::Named(NamedColor::Blue));
    assert!(red.flags.contains(Flags::BOLD));
    assert_eq!(
        core.cell(0, 1).unwrap().fg,
        Color::Named(NamedColor::Foreground)
    );
    assert!(!core.cell(0, 1).unwrap().flags.contains(Flags::BOLD));
    core.feed(b"\x1b[1m\x1b[mD\x1b[1m\x1b[;mE").unwrap();
    for column in [2, 3] {
        assert_eq!(
            core.cell(0, column).unwrap().fg,
            Color::Named(NamedColor::Foreground)
        );
        assert!(!core.cell(0, column).unwrap().flags.contains(Flags::BOLD));
    }
    assert_eq!(core.rejected_events(), Default::default());
    assert!(!core.remote_control_eligible());
}

#[test]
fn bounded_sgr_survives_split_input_and_has_a_parameter_limit() {
    let mut core = TerminalCore::new(4, 12).unwrap();
    core.feed(b"\x1b[1;").unwrap();
    core.feed(b"31mR").unwrap();
    assert_eq!(core.cell(0, 0).unwrap().fg, Color::Named(NamedColor::Red));
    let sixteen = format!("\x1b[{}mA", ["0"; 16].join(";"));
    core.feed(sixteen.as_bytes()).unwrap();
    let seventeen = format!("prefix\x1b[{}mB", ["0"; 17].join(";"));
    let mut core = TerminalCore::new(4, 12).unwrap();
    assert!(core.feed(seventeen.as_bytes()).is_err());
    assert_eq!(core.row_text(0).as_deref(), Some(""));
    assert!(core.feed(b"after").is_err());
}

#[test]
fn unreviewed_sgr_forms_stay_rejected_before_batch_application() {
    for input in [
        b"prefix\x1b[38;5;1m".as_slice(),
        b"prefix\x1b[48;2;1;2;3m",
        b"prefix\x1b[4:2m",
        b"prefix\x1b[?31m",
        b"prefix\x1b[>31m",
    ] {
        let mut core = TerminalCore::new(4, 12).unwrap();
        assert!(core.feed(input).is_err());
        assert_eq!(core.row_text(0).as_deref(), Some(""));
    }
    let mut core = TerminalCore::new(4, 12).unwrap();
    core.feed(b"\x1b[4:").unwrap();
    assert!(core.feed(b"2m").is_err());
    assert_eq!(core.row_text(0).as_deref(), Some(""));
}

#[test]
fn carriage_return_and_linefeed_use_alacritty_cursor_semantics() {
    let mut core = TerminalCore::new(4, 12).unwrap();

    core.feed(b"first\r\nsecond").unwrap();

    assert_eq!(core.row_text(0).as_deref(), Some("first"));
    assert_eq!(core.row_text(1).as_deref(), Some("second"));
}

#[test]
fn alternate_buffer_is_isolated_and_primary_is_restored() {
    let mut core = TerminalCore::new(4, 12).unwrap();
    core.feed(b"primary").unwrap();

    core.feed(b"\x1b[?1049h\x1b[Halternate").unwrap();
    assert_eq!(core.cell(0, 0).unwrap().c, 'a');

    core.feed(b"\x1b[?1049l").unwrap();
    assert_eq!(core.row_text(0).as_deref(), Some("primary"));
}

#[test]
fn resize_validates_first_and_preserves_basic_content() {
    let mut core = TerminalCore::new(4, 12).unwrap();
    core.feed(b"abc").unwrap();

    assert_eq!(core.resize(0, 20).unwrap_err(), TerminalCoreError::ZeroRows);
    assert_eq!(core.term().screen_lines(), 4);
    assert_eq!(core.term().columns(), 12);

    core.resize(5, 20).unwrap();
    assert_eq!(core.term().screen_lines(), 5);
    assert_eq!(core.term().columns(), 20);
    assert_eq!(core.row_text(0).as_deref(), Some("abc"));
    assert_eq!(core.term().grid().history_size(), 0);
}

#[test]
fn terminal_replies_are_dropped_without_retaining_payloads() {
    let sink = TerminalEventSink::default();

    sink.send_event(Event::PtyWrite("sensitive reply".into()));
    sink.send_event(Event::ClipboardStore(
        ClipboardType::Clipboard,
        "sensitive clipboard".into(),
    ));
    sink.send_event(Event::Title("sensitive title".into()));

    let rejected = sink.rejected_events();
    assert_eq!(rejected.pty_writes, 1);
    assert_eq!(rejected.clipboard, 1);
    assert_eq!(rejected.native_effects, 1);
}

#[test]
fn osc52_is_disabled_in_both_validation_and_emulator_profiles() {
    let mut core = TerminalCore::new(4, 12).unwrap();

    assert_eq!(
        core.feed(b"\x1b]52;c;SGVsbG8=\x07"),
        Err(TerminalFault::Degraded(
            TerminalSafeCode::UnsupportedSequence
        ))
    );

    assert_eq!(core.blocked_osc52_count(), 1);
    assert_eq!(core.rejected_events().clipboard, 0);
}

#[test]
fn unreviewed_osc_dcs_and_csi_never_reach_alacritty() {
    for input in [
        &b"prefix\x1b]2;title\x07"[..],
        &b"prefix\x1bPqpayload\x1b\\"[..],
        &b"prefix\x1b[c"[..],
    ] {
        let mut core = TerminalCore::new(4, 12).unwrap();
        assert_eq!(
            core.feed(input),
            Err(TerminalFault::Degraded(
                TerminalSafeCode::UnsupportedSequence
            ))
        );
        assert_eq!(core.row_text(0).as_deref(), Some(""));
        assert!(core.is_degraded());
    }
}

#[test]
fn unterminated_string_control_degrades_at_the_introducing_batch() {
    for input in [
        &b"\x1b]2;partial title"[..],
        &b"\x1bPqpartial dcs"[..],
        &b"\x1bXpartial sos"[..],
        &b"\x1b^partial pm"[..],
        &b"\x1b_partial apc"[..],
    ] {
        let mut core = TerminalCore::new(4, 12).unwrap();
        assert_eq!(
            core.feed(input),
            Err(TerminalFault::Degraded(
                TerminalSafeCode::UnsupportedSequence
            ))
        );
        assert_eq!(core.rejected_events().native_effects, 0);
        assert_eq!(core.row_text(0).as_deref(), Some(""));
    }
}

#[test]
fn cancelled_sos_pm_apc_still_degrade_before_alacritty() {
    for introducer in *b"X^_" {
        let mut same_batch = b"prefix\x1b".to_vec();
        same_batch.push(introducer);
        same_batch.extend_from_slice(b"secret\x1b[H");

        let mut core = TerminalCore::new(4, 12).unwrap();
        assert_eq!(
            core.feed(&same_batch),
            Err(TerminalFault::Degraded(
                TerminalSafeCode::UnsupportedSequence
            ))
        );
        assert_eq!(core.row_text(0).as_deref(), Some(""));

        let mut split = TerminalCore::new(4, 12).unwrap();
        split.feed(b"prefix\x1b").unwrap();
        let mut tail = vec![introducer];
        tail.extend_from_slice(b"secret\x1b[H");
        assert_eq!(
            split.feed(&tail),
            Err(TerminalFault::Degraded(
                TerminalSafeCode::UnsupportedSequence
            ))
        );
        assert_eq!(split.row_text(0).as_deref(), Some("prefix"));
    }
}

#[test]
fn combining_scalar_is_rejected_before_cell_allocation() {
    let mut core = TerminalCore::new(4, 12).unwrap();

    assert_eq!(
        core.feed("a\u{0301}".as_bytes()),
        Err(TerminalFault::Degraded(TerminalSafeCode::CombiningScalar))
    );
    assert_eq!(core.row_text(0).as_deref(), Some(""));
}

#[test]
fn experimental_slice_never_enables_remote_control() {
    let core = TerminalCore::new(4, 12).unwrap();

    assert!(!core.remote_control_eligible());
}

#[test]
fn oversized_feed_degrades_before_any_parser_or_term_work() {
    let mut at_limit = TerminalCore::new(4, 12).unwrap();
    let accepted = vec![b'x'; MAX_TERMINAL_FEED_BYTES];
    assert_eq!(at_limit.feed(&accepted), Ok(()));
    assert!(!at_limit.is_degraded());

    let mut core = TerminalCore::new(4, 12).unwrap();
    let bytes = vec![b'x'; MAX_TERMINAL_FEED_BYTES + 1];

    assert_eq!(
        core.feed(&bytes),
        Err(TerminalFault::Degraded(
            TerminalSafeCode::InputBatchExceeded
        ))
    );
    assert_eq!(core.row_text(0).as_deref(), Some(""));
}

#[test]
fn public_grid_accessors_are_bounds_checked() {
    let core = TerminalCore::new(2, 2).unwrap();

    assert!(core.cell(2, 0).is_none());
    assert!(core.cell(0, 2).is_none());
    assert!(core.cell(usize::MAX, usize::MAX).is_none());
    assert!(core.row_text(2).is_none());
    assert!(core.row_text(usize::MAX).is_none());
}

#[test]
fn osc_overflow_degrades_before_the_batch_reaches_term() {
    let mut core = TerminalCore::new(4, 12).unwrap();
    let mut batch = b"not-applied\x1b]2;".to_vec();
    batch.extend(std::iter::repeat_n(b'x', 4097));
    batch.push(0x07);

    assert_eq!(
        core.feed(&batch),
        Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow))
    );
    assert!(core.is_degraded());
    assert_eq!(core.row_text(0).as_deref(), Some(""));
    assert_eq!(
        core.feed(b"still-not-applied"),
        Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow))
    );
}

#[test]
fn maximum_geometry_is_accepted() {
    let core = TerminalCore::new(MAX_TERMINAL_ROWS, MAX_TERMINAL_COLUMNS).unwrap();

    assert_eq!(core.term().screen_lines(), MAX_TERMINAL_ROWS);
    assert_eq!(core.term().columns(), MAX_TERMINAL_COLUMNS);
}

#[test]
fn grid_coordinates_match_public_accessors() {
    let mut core = TerminalCore::new(2, 2).unwrap();
    core.feed(b"x").unwrap();

    assert_eq!(core.term().grid()[Line(0)][Column(0)].c, 'x');
}
