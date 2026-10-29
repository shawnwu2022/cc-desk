use cc_desk_remote_terminal::{TerminalFault, TerminalParser, TerminalSafeCode};
use vte::{Parser, Perform};

#[derive(Default)]
struct Recorder {
    printed: String,
    osc_payload_lens: Vec<usize>,
    osc52_native_calls: usize,
}

impl Perform for Recorder {
    fn print(&mut self, c: char) {
        self.printed.push(c);
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.osc_payload_lens
            .push(params.iter().map(|param| param.len()).sum());
        if params.first().and_then(|param| parse_decimal(param)) == Some(52) {
            self.osc52_native_calls += 1;
        }
    }
}

fn parse_decimal(bytes: &[u8]) -> Option<u64> {
    bytes.iter().try_fold(0u64, |value, byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value
            .checked_mul(10)?
            .checked_add(u64::from(byte.checked_sub(b'0')?))
    })
}

fn osc(payload_len: usize, terminator: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(2 + payload_len + terminator.len());
    input.extend_from_slice(b"\x1b]");
    input.extend_from_slice(&vec![b'a'; payload_len]);
    input.extend_from_slice(terminator);
    input
}

#[test]
fn terminal_osc_bound_001_accepts_4095_and_4096_payload_bytes() {
    for payload_len in [4095, 4096] {
        let mut parser = TerminalParser::new();
        let mut recorder = Recorder::default();

        assert_eq!(
            parser.feed(&mut recorder, &osc(payload_len, b"\x07")),
            Ok(())
        );
        assert_eq!(recorder.osc_payload_lens, [payload_len]);
        assert!(!parser.is_degraded());
    }
}

#[test]
fn terminal_osc_bound_001_degrades_at_4097_and_never_dispatches_or_resumes() {
    let mut parser = TerminalParser::new();
    let mut recorder = Recorder::default();

    assert_eq!(
        parser.feed(&mut recorder, &osc(4097, b"\x07trailing")),
        Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow))
    );
    assert!(parser.is_degraded());
    assert!(recorder.osc_payload_lens.is_empty());
    assert!(
        recorder.printed.is_empty(),
        "bytes after overflow must not execute in the same feed"
    );

    assert_eq!(
        parser.feed(&mut recorder, b"later"),
        Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow))
    );
    assert!(
        recorder.printed.is_empty(),
        "degraded parser must require explicit local replacement"
    );
}

#[test]
fn terminal_osc_bound_001_raw_parser_discards_through_bel_or_split_st() {
    for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
        let mut parser = Parser::new();
        let mut recorder = Recorder::default();

        parser.advance(&mut recorder, &osc(4097, b""));
        assert!(parser.take_osc_overflowed());
        assert_eq!(parser.osc_storage_len(), 0);

        parser.advance(&mut recorder, b"ignored\x1bXstill ignored");
        if terminator == b"\x1b\\" {
            parser.advance(&mut recorder, b"\x1b");
            parser.advance(&mut recorder, b"\\");
        } else {
            parser.advance(&mut recorder, terminator);
        }
        parser.advance(&mut recorder, b"Z");

        assert!(recorder.osc_payload_lens.is_empty());
        assert_eq!(recorder.printed, "Z");
    }
}

#[test]
fn terminal_osc_bound_001_unterminated_input_stops_growing_after_limit() {
    let mut parser = Parser::new();
    let mut recorder = Recorder::default();
    parser.advance(&mut recorder, b"\x1b]");
    parser.advance(&mut recorder, &vec![b'a'; 4096]);
    assert_eq!(parser.osc_storage_len(), 4096);

    for _ in 0..128 {
        parser.advance(&mut recorder, &vec![b'b'; 8192]);
        assert_eq!(parser.osc_storage_len(), 0);
        assert!(parser.osc_storage_capacity() <= 4096);
    }

    assert!(parser.take_osc_overflowed());
    assert!(recorder.osc_payload_lens.is_empty());
}

#[test]
fn terminal_osc_bound_001_counts_ignored_c0_bytes_in_the_payload_budget() {
    let mut parser = TerminalParser::new();
    let mut recorder = Recorder::default();
    let mut input = osc(0, b"");
    input.extend_from_slice(&vec![0; 4097]);
    input.push(0x07);

    assert_eq!(
        parser.feed(&mut recorder, &input),
        Err(TerminalFault::Degraded(TerminalSafeCode::OscOverflow))
    );
    assert!(recorder.osc_payload_lens.is_empty());
}

#[test]
fn terminal_vte_patch_stops_at_the_overflowing_byte_with_a_consumed_prefix() {
    let mut parser = Parser::new();
    let mut recorder = Recorder::default();
    let input = osc(4097, b"\x07trailing");

    let consumed = parser.advance_until_osc_overflow(&mut recorder, &input);

    assert_eq!(consumed, 2 + 4097);
    assert!(parser.take_osc_overflowed());
    assert!(recorder.osc_payload_lens.is_empty());
    assert!(recorder.printed.is_empty());
}

#[test]
fn terminal_parser_preserves_utf8_and_st_state_across_chunks() {
    let mut parser = TerminalParser::new();
    let mut recorder = Recorder::default();

    for chunk in [
        b"\xf0".as_slice(),
        b"\x9f\xa6".as_slice(),
        b"\x80".as_slice(),
    ] {
        assert_eq!(parser.feed(&mut recorder, chunk), Ok(()));
    }
    for chunk in [
        b"\x1b]title".as_slice(),
        b"\x1b".as_slice(),
        b"\\".as_slice(),
    ] {
        assert_eq!(parser.feed(&mut recorder, chunk), Ok(()));
    }

    assert_eq!(recorder.printed, "\u{1f980}");
    assert_eq!(recorder.osc_payload_lens, [5]);
}

#[test]
fn terminal_parser_blocks_normalized_osc52_without_calling_the_consumer() {
    let mut parser = TerminalParser::new();
    let mut recorder = Recorder::default();

    for input in [
        b"\x1b]52;c;secret\x07".as_slice(),
        b"\x1b]052;c;secret\x07".as_slice(),
        b"\x1b]0052;c;secret\x07".as_slice(),
    ] {
        assert_eq!(parser.feed(&mut recorder, input), Ok(()));
    }
    assert_eq!(parser.feed(&mut recorder, b"\x1b]53;allowed\x07"), Ok(()));
    assert_eq!(parser.feed(&mut recorder, b"\x1b]4<;allowed\x07"), Ok(()));

    assert_eq!(parser.blocked_osc52_count(), 3);
    assert_eq!(recorder.osc52_native_calls, 0);
    assert_eq!(recorder.osc_payload_lens, [9, 9]);
}
