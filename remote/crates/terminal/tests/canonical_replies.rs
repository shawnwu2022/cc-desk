#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::{CanonicalEvent, TerminalCore, TerminalFault};

fn feed(core: &mut TerminalCore, bytes: &[u8], replies: &mut Vec<Vec<u8>>) {
    core.feed_with_queries(bytes, |event| -> Result<(), TerminalFault> {
        if let CanonicalEvent::Query(query) = event {
            replies.push(query.encode()?.as_bytes().to_vec());
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn cursor_replies_sample_each_canonical_prefix_instead_of_the_batch_tail() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    let mut replies = Vec::new();
    feed(&mut core, b"abc\x1b[6n\x1b[2;5HZ\x1b[6n", &mut replies);
    assert_eq!(replies, [b"\x1b[1;4R".to_vec(), b"\x1b[2;6R".to_vec()]);
    assert_eq!(core.row_text(1).as_deref(), Some("    Z"));
    assert!(!core.remote_control_eligible());
}

#[test]
fn canonical_replies_survive_every_stream_split_and_use_mature_static_status() {
    let wire = b"abc\x1b[6n\x1b[5n\x1b[c";
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        let mut replies = Vec::new();
        feed(&mut core, &wire[..split], &mut replies);
        feed(&mut core, &wire[split..], &mut replies);
        assert_eq!(
            replies,
            [
                b"\x1b[1;4R".to_vec(),
                b"\x1b[0n".to_vec(),
                b"\x1b[?6c".to_vec()
            ]
        );
    }
}

#[test]
fn a_bad_tail_or_query_flood_is_rejected_before_any_reply_or_grid_effect() {
    for wire in [b"abc\x1b[6n\x1b]52;c;x\x07".to_vec(), b"\x1b[6n".repeat(33)] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        assert!(
            core.feed_with_queries(&wire, |_| -> Result<(), ()> {
                panic!("invalid batch must not reach commit callback")
            })
            .is_err()
        );
        assert_eq!(core.row_text(0).as_deref(), Some(""));
        assert!(core.is_degraded());
    }
}

#[test]
fn denied_commit_freezes_the_canonical_continuation_without_a_native_effect() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    let result = core.feed_with_queries(b"abc\x1b[6n", |event| match event {
        CanonicalEvent::Begin { queries } => {
            assert_eq!(queries, 1);
            Ok(())
        }
        CanonicalEvent::Query(_) => Err("PUBLIC_TEST_ONLY_DENIED"),
    });
    assert!(result.is_err());
    assert!(core.is_degraded());
    assert_eq!(core.rejected_events().clipboard, 0);
    assert_eq!(core.rejected_events().native_effects, 0);
    assert!(core.feed(b"later").is_err());
}

#[test]
fn ordinary_profile_keeps_stateful_queries_disabled() {
    let mut core = TerminalCore::new(3, 10).unwrap();
    assert!(core.feed(b"\x1b[6n").is_err());
}

#[test]
fn a_panicking_commit_callback_cannot_reopen_a_misaligned_parser_continuation() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = core.feed_with_queries(b"abc\x1b[6nXYZ", |event| -> Result<(), ()> {
            match event {
                CanonicalEvent::Begin { .. } => Ok(()),
                CanonicalEvent::Query(_) => panic!("PUBLIC_TEST_ONLY_COMMIT_PANIC"),
            }
        });
    }));
    assert!(result.is_err());
    assert!(core.is_degraded());
    assert_eq!(core.row_text(0).as_deref(), Some("abc"));
    assert!(core.feed(b"later").is_err());
}

#[test]
fn maximum_geometry_cursor_reply_fits_the_exact_ten_byte_limit() {
    let mut core = TerminalCore::new_with_queries(100, 240).unwrap();
    let mut replies = Vec::new();
    feed(&mut core, b"\x1b[100;240H\x1b[6n", &mut replies);
    assert_eq!(replies, [b"\x1b[100;240R".to_vec()]);
    assert_eq!(replies[0].len(), 10);
}

#[test]
fn exact_conpty_startup_controls_have_only_the_declared_compatibility_effects() {
    use alacritty_terminal::term::TermMode;
    let wire = b"\x1b[1t\x1b[c\x1b[?1004h\x1b[?9001h";
    for split in 0..=wire.len() {
        let mut core = TerminalCore::new_with_queries(24, 80).unwrap();
        let mut replies = Vec::new();
        feed(&mut core, &wire[..split], &mut replies);
        feed(&mut core, &wire[split..], &mut replies);
        assert_eq!(replies, [b"\x1b[?6c".to_vec()]);
        assert!(core.term().mode().contains(TermMode::FOCUS_IN_OUT));
        assert_eq!(core.rejected_events().native_effects, 0);
        feed(&mut core, b"\x1b[?1004l\x1b[?9001l", &mut replies);
        assert!(!core.term().mode().contains(TermMode::FOCUS_IN_OUT));
        assert!(!core.remote_control_eligible());
    }
}

#[test]
fn conpty_compatibility_controls_do_not_admit_other_window_or_mode_operations() {
    for wire in [
        b"\x1b[2t".as_slice(),
        b"\x1b[1;2t",
        b"\x1b[?9002h",
        b"\x1b[?1004;9001h",
        b"\x1b[?9001:1h",
    ] {
        let mut core = TerminalCore::new_with_queries(24, 80).unwrap();
        assert!(
            core.feed_with_queries(wire, |_| -> Result<(), ()> {
                panic!("bad batch before callbacks")
            })
            .is_err()
        );
        assert_eq!(core.rejected_events().native_effects, 0);
    }
    let mut core = TerminalCore::new(24, 80).unwrap();
    assert!(core.feed(b"\x1b[?9001h").is_err());
}
