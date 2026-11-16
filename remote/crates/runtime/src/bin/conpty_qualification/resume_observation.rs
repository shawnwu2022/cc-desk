//! One TEST_ONLY pressure probe: reporting consumes the successful sample.
use std::time::{Duration, Instant};
pub(super) struct ResumeObservation {
    read: u64,
    written: u64,
}
pub(super) fn wait_for_resume(
    end: Instant,
    read_before: u64,
    written_before: u64,
    minimum: u64,
    mut sample: impl FnMut() -> Option<(u64, u64)>,
) -> Result<ResumeObservation, &'static str> {
    let read_floor = read_before
        .checked_add(minimum)
        .ok_or("RESUME_COUNTER_OVERFLOW")?;
    let written_floor = written_before
        .checked_add(minimum)
        .ok_or("RESUME_COUNTER_OVERFLOW")?;
    while Instant::now() < end {
        match sample() {
            Some((read, written))
                if read >= read_floor && written >= written_floor && Instant::now() < end =>
            {
                return Ok(ResumeObservation { read, written })
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("PHASE_DEADLINE")
}
pub(super) fn resume_report(observed: ResumeObservation) -> String {
    format!(
        "PRESSURE:RESUMED:read={}:written={}",
        observed.read, observed.written
    )
}
