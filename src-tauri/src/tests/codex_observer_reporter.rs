#![allow(non_snake_case)]
use super::read_metadata;
use crate::observer_registry::MAX_OBSERVER_PAYLOAD;
use serde_json::json;
use std::io::Cursor;

fn payload() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "hook_event_name": "SessionStart",
        "session_id": "11111111-2222-4333-8444-555555555555",
        "prompt": "synthetic-private",
        "cwd": "synthetic-private"
    }))
    .unwrap()
}

#[test]
fn CodexReporter_PlainAndOneLeadingBom_001() {
    let plain = payload();
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend(&plain);
    for input in [plain, bom] {
        assert_eq!(
            read_metadata(input.as_slice()).unwrap(),
            json!({
                "hook_event_name": "SessionStart",
                "session_id": "11111111-2222-4333-8444-555555555555"
            })
        );
    }
}

#[test]
fn CodexReporter_OnlyOneExactLeadingBom_002() {
    for prefix in [
        vec![0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf],
        vec![0xef, 0xbb],
        vec![0xef, 0xbb, 0xbe],
        vec![0xff, 0xfe],
        vec![b' ', 0xef, 0xbb, 0xbf],
    ] {
        let mut input = prefix;
        input.extend(payload());
        assert!(read_metadata(input.as_slice()).is_err());
    }
    assert!(read_metadata([0xef, 0xbb, 0xbf].as_slice()).is_err());
}

#[test]
fn CodexReporter_RawLimitPrecedesBomRemoval_003() {
    let mut full = payload();
    full.resize(MAX_OBSERVER_PAYLOAD, b' ');
    assert!(read_metadata(full.as_slice()).is_ok());
    let mut within = vec![0xef, 0xbb, 0xbf];
    within.extend(payload());
    within.resize(MAX_OBSERVER_PAYLOAD, b' ');
    assert!(read_metadata(within.as_slice()).is_ok());
    let mut over = vec![0xef, 0xbb, 0xbf];
    over.extend(full);
    let mut reader = Cursor::new(over);
    assert!(read_metadata(&mut reader).is_err());
    assert_eq!(reader.position(), (MAX_OBSERVER_PAYLOAD + 1) as u64);
    let mut over = payload();
    over.resize(MAX_OBSERVER_PAYLOAD + 1, b' ');
    assert!(read_metadata(over.as_slice()).is_err());
}

#[test]
fn CodexReporter_InvalidJsonAndMetadataStayRejected_004() {
    for input in [b"{".as_slice(), b"{}", b"null", b"[]", b"\xff"] {
        assert!(read_metadata(input).is_err());
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(input);
        assert!(read_metadata(bom.as_slice()).is_err());
    }
    for invalid in [
        json!({"hook_event_name":"SessionStart","session_id":"not-a-uuid"}),
        json!({"hook_event_name":"Unknown","session_id":"11111111-2222-4333-8444-555555555555"}),
        json!({"hook_event_name":"Stop","session_id":"11111111-2222-4333-8444-555555555555","agent_id":"child"}),
    ] {
        let bytes = serde_json::to_vec(&invalid).unwrap();
        assert!(read_metadata(bytes.as_slice()).is_err());
    }
}
