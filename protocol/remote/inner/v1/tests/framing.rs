use cc_desk_remote_record::{encode, Error, Header, Kind};

fn header(length: u32, kind: u16, minor: u16) -> [u8; 8] {
    let mut h = [0; 8];
    h[..4].copy_from_slice(&length.to_be_bytes());
    h[4..6].copy_from_slice(&kind.to_be_bytes());
    h[6..].copy_from_slice(&minor.to_be_bytes());
    h
}

#[test]
fn frozen_byte_oracle_matches_length_type_minor_and_payload() {
    assert_eq!(
        encode(Kind::Control, 0, b"{}"),
        Ok(vec![0, 0, 0, 6, 0, 2, 0, 0, b'{', b'}'])
    );
    assert_eq!(
        Header::decode(header(6, 2, 0), 0),
        Ok(Header {
            kind: Kind::Control,
            minor: 0,
            payload_len: 2
        })
    );
}

#[test]
fn exact_caps_and_empty_payload_are_framing_only() {
    for (kind, code, cap) in [
        (Kind::Control, 2, 16384),
        (Kind::Projection, 3, 32768),
        (Kind::AppBind, 4, 16384),
    ] {
        for len in [0, cap] {
            let minor = if kind == Kind::AppBind { 0 } else { 1 };
            let b = encode(kind, minor, &vec![b'x'; len]).unwrap();
            assert_eq!(b.len(), len + 8);
            assert_eq!(
                Header::decode(b[..8].try_into().unwrap(), minor)
                    .unwrap()
                    .payload_len,
                len
            );
        }
        assert_eq!(encode(kind, 0, &vec![0; cap + 1]), Err(Error::Length));
        assert_eq!(
            Header::decode(header((cap + 5) as u32, code, 0), 0),
            Err(Error::Length)
        );
    }
}

#[test]
fn malformed_lengths_unknown_types_and_unnegotiated_minor_reject() {
    for len in [0, 1, 2, 3, 32773, u32::MAX] {
        assert_eq!(Header::decode(header(len, 2, 0), 0), Err(Error::Length));
    }
    for kind in [0, 1, 5, u16::MAX] {
        assert_eq!(Header::decode(header(4, kind, 0), 0), Err(Error::Kind));
    }
    for (wire, expected) in [(1, 0), (0, 1), (2, 2), (u16::MAX, 0)] {
        assert_eq!(
            Header::decode(header(4, 2, wire), expected),
            Err(Error::Version)
        );
    }
    assert_eq!(encode(Kind::Control, 2, b""), Err(Error::Version));
    assert_eq!(encode(Kind::AppBind, 1, b"{}"), Err(Error::Version));
    assert_eq!(Header::decode(header(6, 4, 1), 1), Err(Error::Version));
}
