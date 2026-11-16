#![allow(non_snake_case)]
use cc_desk_remote_protocol::*;
fn vector() -> Vec<u8> {
    let s = include_str!("../vectors/one-frame.hex").trim();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
#[test]
fn Outer_Vector_001() {
    let bytes = vector();
    let frames = decode_outer(&bytes).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0].tunnel,
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );
    assert_eq!(frames[0].payload, [0xaa, 0xbb, 0xcc]);
    assert_eq!(encode_outer(&frames).unwrap(), bytes);
    assert_eq!(frames[0].payload.as_ptr(), bytes[24..].as_ptr());
}
#[test]
fn Outer_Empty_001() {
    assert!(decode_outer(&[1, 1, 0, 0]).unwrap().is_empty());
    assert_eq!(encode_outer(&[]).unwrap(), [1, 1, 0, 0]);
}
#[test]
fn Outer_ZeroPayload_001() {
    let f = Subframe {
        tunnel: [7; 16],
        payload: &[],
    };
    let b = encode_outer(&[f]).unwrap();
    assert_eq!(b.len(), 24);
    assert!(decode_outer(&b).unwrap()[0].payload.is_empty());
}
#[test]
fn Outer_ExactLimit_001() {
    let payload = vec![0x5a; 65512];
    let bytes = encode_outer(&[Subframe {
        tunnel: [1; 16],
        payload: &payload,
    }])
    .unwrap();
    assert_eq!(bytes.len(), 65536);
    assert_eq!(decode_outer(&bytes).unwrap()[0].payload, payload);
    let oversized = vec![0; 65513];
    assert_eq!(
        encode_outer(&[Subframe {
            tunnel: [1; 16],
            payload: &oversized
        }]),
        Err(WireError::Length)
    );
    assert!(matches!(
        decode_outer(&vec![0; 65537]),
        Err(WireError::Length)
    ));
}
#[test]
fn Outer_SubframeCount_001() {
    let f = Subframe {
        tunnel: [1; 16],
        payload: &[],
    };
    let valid = encode_outer(&[f; 32]).unwrap();
    assert_eq!(decode_outer(&valid).unwrap().len(), 32);
    assert_eq!(encode_outer(&[f; 33]), Err(WireError::Count));
    assert!(matches!(
        decode_outer(&[1, 1, 0, 33]),
        Err(WireError::Count)
    ));
    assert!(matches!(
        decode_outer(&[1, 1, 255, 255]),
        Err(WireError::Count)
    ));
}
#[test]
fn Outer_VersionKind_001() {
    assert!(matches!(
        decode_outer(&[2, 1, 0, 0]),
        Err(WireError::Version)
    ));
    assert!(matches!(decode_outer(&[1, 2, 0, 0]), Err(WireError::Kind)));
}
#[test]
fn Outer_Truncation_001() {
    let bytes = vector();
    for end in 0..bytes.len() {
        assert!(matches!(
            decode_outer(&bytes[..end]),
            Err(WireError::Length)
        ));
    }
}
#[test]
fn Outer_LengthMismatch_001() {
    let mut bytes = vector();
    bytes[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(matches!(decode_outer(&bytes), Err(WireError::Length)));
    bytes[20..24].copy_from_slice(&0u32.to_be_bytes());
    assert!(matches!(decode_outer(&bytes), Err(WireError::Length)));
}
#[test]
fn Outer_TrailingAndCountMismatch_001() {
    let mut bytes = vector();
    bytes.push(0);
    assert!(matches!(decode_outer(&bytes), Err(WireError::Length)));
    bytes = vector();
    bytes[3] = 0;
    assert!(matches!(decode_outer(&bytes), Err(WireError::Length)));
    bytes[3] = 2;
    assert!(matches!(decode_outer(&bytes), Err(WireError::Length)));
}
#[test]
fn Outer_MultipleRoutes_001() {
    let encoded = encode_outer(&[
        Subframe {
            tunnel: [1; 16],
            payload: b"first",
        },
        Subframe {
            tunnel: [2; 16],
            payload: b"second",
        },
    ])
    .unwrap();
    let frames = decode_outer(&encoded).unwrap();
    assert_eq!(frames[0].tunnel, [1; 16]);
    assert_eq!(frames[0].payload, b"first");
    assert_eq!(frames[1].tunnel, [2; 16]);
    assert_eq!(frames[1].payload, b"second");
}
