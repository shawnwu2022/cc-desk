#![cfg(feature = "experimental-emulator")]
use cc_desk_remote_record::projection::{Content, Stamp, Target, decode};
use cc_desk_remote_terminal::{SnapshotEncoder, TerminalCore};
fn stamp() -> Stamp {
    Stamp {
        target: Target {
            host: "11".repeat(32),
            session: "22".repeat(16),
            run: "fixture".into(),
            generation: "1".into(),
            incarnation: "1".into(),
        },
        binding: "33".repeat(32),
        barrier: "44".repeat(16),
        revision: "7".into(),
        dimension_epoch: "0".into(),
        mode_epoch: "0".into(),
        structure_epoch: "0".into(),
        sync_epoch: "9".into(),
    }
}
#[test]
fn snapshot_encoder_lends_one_bounded_part_at_a_time_with_exact_coverage_and_metadata() {
    use std::io::Write;
    let mut artifact = std::env::var_os("CC_DESK_TEST_ONLY_PROJECTION_JSONL").map(|p| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
            .expect("new TEST_ONLY artifact")
    });
    let mut core = TerminalCore::new_with_queries(100, 240).unwrap();
    core.feed("中<&\x1b[?1004h".as_bytes()).unwrap();
    let view = core.semantic_snapshot().unwrap();
    let mut s = stamp();
    s.mode_epoch = "1".into();
    let mut index = 0;
    let mut bytes = 0;
    let mut parts = 0;
    for part in SnapshotEncoder::new(&view, s.clone(), 1).unwrap() {
        let wire = part.unwrap();
        bytes += wire.len();
        parts += 1;
        assert!(wire.len() <= 32768);
        if let Some(file) = &mut artifact {
            file.write_all(&wire).unwrap();
            file.write_all(b"\n").unwrap();
        }
        let message = decode(&wire, 1).unwrap();
        let Content::SnapshotPart(p) = message.content else {
            panic!("snapshot")
        };
        assert_eq!(usize::from(p.start), index);
        assert!(p.cells.len() <= 128);
        assert!(p.stamp == s);
        index += p.cells.len();
    }
    assert_eq!(index, 24000);
    assert_eq!(parts, 188);
    assert!(bytes <= 4 * 1024 * 1024);
}
#[test]
fn snapshot_encoder_refuses_misstamped_core_epochs_and_invalid_binding_before_emitting() {
    let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
    core.feed(b"\x1b[?1049h").unwrap();
    let view = core.semantic_snapshot().unwrap();
    assert!(SnapshotEncoder::new(&view, stamp(), 0).is_err());
    let mut s = stamp();
    s.mode_epoch = "1".into();
    s.structure_epoch = "1".into();
    s.binding.clear();
    assert!(SnapshotEncoder::new(&view, s, 0).is_err());
}
