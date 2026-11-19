#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::{SemanticColor, TerminalCore, TerminalEpochs};

#[test]
fn canonical_rgb_exact_foreground_background_and_reset() {
    let baseline = TerminalCore::new_with_queries(3, 10)
        .unwrap()
        .semantic_snapshot()
        .unwrap();
    let default_fg = baseline.cell(0, 0).unwrap().foreground;
    let default_bg = baseline.cell(0, 0).unwrap().background;
    for (wire, fg, bg) in [
        (
            b"\x1b[38;2;0;128;255;49mX".as_slice(),
            SemanticColor::Rgb(0, 128, 255),
            default_bg,
        ),
        (
            b"\x1b[39;48;2;255;0;1mX".as_slice(),
            default_fg,
            SemanticColor::Rgb(255, 0, 1),
        ),
        (
            b"\x1b[38;2;1;2;3;48;2;4;5;6mX".as_slice(),
            SemanticColor::Rgb(1, 2, 3),
            SemanticColor::Rgb(4, 5, 6),
        ),
        (
            b"\x1b[38;2;0;0;0;48;5;255mX".as_slice(),
            SemanticColor::Rgb(0, 0, 0),
            SemanticColor::Palette(255),
        ),
    ] {
        for split in 0..=wire.len() {
            let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
            core.feed(&wire[..split]).unwrap();
            core.feed(&wire[split..]).unwrap();
            let view = core.semantic_snapshot().unwrap();
            let cell = view.cell(0, 0).unwrap();
            assert_eq!(cell.foreground, fg);
            assert_eq!(cell.background, bg);
            assert_eq!(cell.scalar, 'X');
            assert_eq!(core.epochs(), TerminalEpochs::default());
            assert_eq!(core.rejected_events().native_effects, 0);
        }
    }
}

#[test]
fn canonical_rgb_rejects_malformed_and_unrelated_extensions() {
    for wire in [
        b"\x1b[38;2;1;2m".as_slice(),
        b"\x1b[48;2;1m",
        b"\x1b[38;2;256;0;0m",
        b"\x1b[48;2;0;256;0m",
        b"\x1b[38;2;0;0;256m",
        b"\x1b[38:2:1:2:3m",
        b"\x1b[38;2;1:2;3;4m",
        b"\x1b[58;2;1;2;3m",
        b"\x1b[38;3;1;2;3m",
        b"\x1b[38;2;1;2;3;999m",
        b"\x1b[38;5;256m",
        b"\x1b[?38;2;1;2;3m",
    ] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        assert!(core.feed(wire).is_err(), "accepted {wire:?}");
        assert_eq!(core.epochs(), TerminalEpochs::default());
    }
}

#[test]
fn canonical_rgb_keeps_sixteen_parameter_limit() {
    for (suffix, accepted) in [
        (";0;0;0;0;0;0;0;0;0;0;0", true),
        (";0;0;0;0;0;0;0;0;0;0;0;0", false),
    ] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        let wire = format!("\x1b[38;2;1;2;3{suffix}m");
        assert_eq!(core.feed(wire.as_bytes()).is_ok(), accepted);
    }
}

#[test]
fn canonical_rgb_does_not_expand_ordinary_profile() {
    for wire in [b"\x1b[38;2;1;2;3m".as_slice(), b"\x1b[48;2;255;0;255m"] {
        let mut core = TerminalCore::new(3, 10).unwrap();
        assert!(core.feed(wire).is_err());
        assert_eq!(core.epochs(), TerminalEpochs::default());
    }
}

#[test]
fn canonical_rgb_queries_keep_prefix_order_across_byte_chunks() {
    use cc_desk_remote_terminal::{CanonicalEvent, TerminalFault};
    let wire = b"X\x1b[6n\x1b[38;2;0;128;255;49mY\x1b[6n";
    for chunk in 1..=wire.len() {
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
        assert_eq!(replies, [b"\x1b[1;2R".to_vec(), b"\x1b[1;3R".to_vec()]);
        assert_eq!(
            core.semantic_snapshot()
                .unwrap()
                .cell(0, 1)
                .unwrap()
                .foreground,
            SemanticColor::Rgb(0, 128, 255)
        );
    }
}

#[test]
fn canonical_rgb_unknown_suffix_rejects_batch_before_mode_mutation() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    assert!(core.feed(b"\x1b[?25l\x1b[38;2;1;2;3;999m").is_err());
    assert_eq!(core.epochs(), TerminalEpochs::default());
}
