//! Windows10 pinned runtime qualification, never a real-CLI or tree barrier claim.
#![cfg(windows)]
use std::{
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
struct OwnedWorker(Child);
impl Drop for OwnedWorker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let end = Instant::now() + Duration::from_secs(2);
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
fn run(mode: &str) {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("TEST_ONLY-marker");
    let worker = env!("CARGO_BIN_EXE_conpty-qualification-worker");
    let mut child = OwnedWorker(
        Command::new(worker)
            .arg(mode)
            .arg(&marker)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    fn capture(
        mut pipe: impl std::io::Read + Send + 'static,
    ) -> mpsc::Receiver<Result<Vec<u8>, &'static str>> {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Ok(data));
                        return;
                    }
                    Ok(n) if data.len() + n <= 64 * 1024 => data.extend_from_slice(&buf[..n]),
                    Ok(_) => {
                        let _ = tx.send(Err("CAPTURE_LIMIT"));
                        return;
                    }
                    Err(_) => {
                        let _ = tx.send(Err("CAPTURE_IO"));
                        return;
                    }
                }
            }
        });
        rx
    }
    let stdout_rx = capture(child.0.stdout.take().unwrap());
    let stderr_rx = capture(child.0.stderr.take().unwrap());
    let end = Instant::now() + Duration::from_secs(12);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < end,
            "owned worker deadline; ConPTY cleanup unknown"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .expect("stdout EOF unknown at worker deadline")
        .unwrap();
    let stderr = stderr_rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .expect("stderr EOF unknown at worker deadline")
        .unwrap();
    let stdout = String::from_utf8(stdout).unwrap();
    let stderr = String::from_utf8(stderr).unwrap();
    println!("{mode}: {status}; stdout={stdout}; stderr={stderr}");
    assert!(status.success(), "finite owned worker rejected: {stderr}");
    assert!(stdout.contains("PASS:"));
}
#[test]
fn pinned_conpty_normal_io_resize_and_owned_root_stop() {
    run("normal");
}
#[test]
fn assignment_failure_is_known_not_started_and_has_no_marker() {
    run("assignment-rejected");
}
#[test]
fn closing_only_owned_job_stops_owned_root_and_drains() {
    run("job-drop");
}
