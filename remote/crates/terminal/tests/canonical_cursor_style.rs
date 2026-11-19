#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_terminal::{SemanticCursorShape, TerminalCore, TerminalEpochs};

#[test]
fn canonical_cursor_style_exact_shapes_blink_and_default_restore() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    let default = core.semantic_snapshot().unwrap().cursor();
    for (id, shape, blinking) in [
        (1, SemanticCursorShape::Block, true),
        (2, SemanticCursorShape::Block, false),
        (3, SemanticCursorShape::Underline, true),
        (4, SemanticCursorShape::Underline, false),
        (5, SemanticCursorShape::Beam, true),
        (6, SemanticCursorShape::Beam, false),
    ] {
        core.feed(format!("\x1b[{id} q").as_bytes()).unwrap();
        let cursor = core.semantic_snapshot().unwrap().cursor();
        assert_eq!((cursor.shape, cursor.blinking), (shape, blinking));
        assert_eq!(core.epochs(), TerminalEpochs::default());
        core.feed(b"\x1b[0 q").unwrap();
        assert_eq!(core.semantic_snapshot().unwrap().cursor(), default);
    }
    core.feed(b"\x1b[5 q\x1b[ q").unwrap();
    assert_eq!(core.semantic_snapshot().unwrap().cursor(), default);
}

#[test]
fn canonical_cursor_style_respects_visibility_and_preserves_cells() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    core.feed(b"X\x1b[?25l\x1b[5 q").unwrap();
    assert_eq!(
        core.semantic_snapshot().unwrap().cursor().shape,
        SemanticCursorShape::Hidden
    );
    core.feed(b"\x1b[?25h").unwrap();
    let view = core.semantic_snapshot().unwrap();
    assert_eq!(view.cursor().shape, SemanticCursorShape::Beam);
    assert!(view.cursor().blinking);
    assert_eq!(view.cell(0, 0).unwrap().scalar, 'X');
}

#[test]
fn canonical_cursor_style_split_and_query_prefix_are_preserved() {
    use cc_desk_remote_terminal::{CanonicalEvent, TerminalFault};
    let wire = b"X\x1b[6n\x1b[3 qY\x1b[6n";
    for size in 1..=wire.len() {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        let mut replies = Vec::new();
        for chunk in wire.chunks(size) {
            core.feed_with_queries(chunk, |event| -> Result<(), TerminalFault> {
                if let CanonicalEvent::Query(q) = event {
                    replies.push(q.encode()?.as_bytes().to_vec());
                }
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(replies, [b"\x1b[1;2R".to_vec(), b"\x1b[1;3R".to_vec()]);
        let view = core.semantic_snapshot().unwrap();
        assert_eq!(view.cursor().shape, SemanticCursorShape::Underline);
        assert!(view.cursor().blinking);
        assert_eq!(core.rejected_events().native_effects, 0);
    }
}

#[test]
fn canonical_cursor_style_other_q_and_malformed_forms_remain_rejected() {
    for wire in [
        b"\x1b[0q".as_slice(),
        b"\x1b[0\"q",
        b"\x1b[?0q",
        b"\x1b[>0q",
        b"\x1b[0$q",
        b"\x1b[7 q",
        b"\x1b[65535 q",
        b"\x1b[0;1 q",
        b"\x1b[0:1 q",
        b"\x1b[0  q",
    ] {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        assert!(core.feed(wire).is_err());
        assert_eq!(core.epochs(), TerminalEpochs::default());
    }
}

#[test]
fn canonical_cursor_style_does_not_expand_ordinary_profile() {
    for id in 0..=6 {
        let mut core = TerminalCore::new(3, 10).unwrap();
        assert!(core.feed(format!("\x1b[{id} q").as_bytes()).is_err());
        assert_eq!(core.epochs(), TerminalEpochs::default());
    }
}

#[test]
fn canonical_cursor_style_unknown_suffix_rejects_before_any_mode_change() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    assert!(core.feed(b"\x1b[?25l\x1b[3 q\x1b[0\"q").is_err());
    assert_eq!(core.epochs(), TerminalEpochs::default());
}
