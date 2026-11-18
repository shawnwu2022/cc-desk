use cc_desk_remote_terminal::{TerminalQueries, TerminalQuery};

#[test]
fn primary_da_is_typed_and_persistent_across_every_chunk_boundary() {
    for wire in [b"\x1b[c".as_slice(), b"\x1b[0c"] {
        for split in 0..=wire.len() {
            let mut parser = TerminalQueries::default();
            let mut queries = parser.feed(&wire[..split]).unwrap();
            queries.extend(parser.feed(&wire[split..]).unwrap());
            assert_eq!(queries, [TerminalQuery::PrimaryDeviceAttributes]);
        }
    }
}

#[test]
fn unsupported_queries_and_string_payloads_do_not_become_primary_da() {
    let mut parser = TerminalQueries::default();
    for wire in [
        b"\x1b[>c".as_slice(),
        b"\x1b[1c",
        b"\x1b[0;0c",
        b"\x1b[6n",
        b"\x1b]0;[c[0c\x07",
        b"\x1bPq[c[0c\x1b\\",
        b"\x1b]52;c;[0c\x07",
    ] {
        assert!(parser.feed(wire).unwrap().is_empty());
    }
    assert_eq!(
        parser.feed(b"\x1b[c").unwrap(),
        [TerminalQuery::PrimaryDeviceAttributes]
    );
}

#[test]
fn excessive_batch_or_query_flood_is_sticky_and_returns_no_partial_queries() {
    for wire in [vec![b'x'; 16385], b"\x1b[c".repeat(33)] {
        let mut parser = TerminalQueries::default();
        assert!(parser.feed(&wire).is_err());
        assert!(parser.feed(b"\x1b[c").is_err());
    }
    let mut parser = TerminalQueries::default();
    assert_eq!(parser.feed(&b"\x1b[c".repeat(32)).unwrap().len(), 32);
}

#[test]
fn osc_overflow_discards_prior_query_and_cannot_reopen_afterwards() {
    let mut parser = TerminalQueries::default();
    let mut wire = b"\x1b[c\x1b]0;".to_vec();
    wire.extend(vec![b'x'; 4097]);
    wire.extend(b"\x07\x1b[c");
    let first = parser.feed(&wire).unwrap_err();
    assert_eq!(parser.feed(b"\x1b[c").unwrap_err(), first);
}
