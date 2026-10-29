#[path = "../src/bin/conpty_qualification/resume_observation.rs"]
mod observation;
use observation::{resume_report, wait_for_resume};
use std::time::{Duration, Instant};

#[test]
fn accepted_resume_observation_survives_a_later_empty_progress_file() {
    let dir = tempfile::tempdir().unwrap();
    let progress = dir.path().join("PUBLIC_TEST_ONLY_PROGRESS");
    std::fs::write(&progress, b"131072").unwrap();
    let captured = wait_for_resume(
        Instant::now() + Duration::from_secs(1),
        65536,
        65536,
        65536,
        || {
            let written = std::fs::read_to_string(&progress)
                .ok()?
                .parse::<u64>()
                .ok()?;
            Some((131072, written))
        },
    )
    .unwrap();
    // Reproduce the fixture writer's legitimate truncate/update gap after the
    // successful witness. A report must use that witness, not re-read the file.
    std::fs::write(&progress, b"").unwrap();
    assert_eq!(
        resume_report(captured),
        "PRESSURE:RESUMED:read=131072:written=131072"
    );
}

#[test]
fn resume_observation_never_qualifies_a_ready_but_late_sample() {
    let end = Instant::now() + Duration::from_millis(5);
    assert!(wait_for_resume(end, 0, 0, 65536, || {
        std::thread::sleep(Duration::from_millis(10));
        Some((65536, 65536))
    })
    .is_err());
}

#[test]
fn resume_observation_requires_both_thresholds_and_checked_arithmetic() {
    let mut samples = [(65536, 65535), (65535, 65536), (65536, 65536)].into_iter();
    let captured = wait_for_resume(Instant::now() + Duration::from_secs(1), 0, 0, 65536, || {
        samples.next()
    })
    .unwrap();
    assert_eq!(
        resume_report(captured),
        "PRESSURE:RESUMED:read=65536:written=65536"
    );
    assert!(wait_for_resume(
        Instant::now() + Duration::from_secs(1),
        u64::MAX,
        0,
        1,
        || panic!("overflow before sample")
    )
    .is_err());
    assert!(wait_for_resume(Instant::now(), 0, 0, 1, || panic!("expired before sample")).is_err());
}
