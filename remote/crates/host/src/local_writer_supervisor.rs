//! TEST_ONLY fixed-case re-exec supervision, reused from reviewed D containment pattern.
use std::{
    io::{Read, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
    },
    core::{BOOL, PCWSTR},
};
struct Supervisor {
    child: std::process::Child,
    outer: Option<OwnedHandle>,
    cleanup_end: Option<Instant>,
    root_confirmed: bool,
}
impl Supervisor {
    fn contain(&mut self) {
        let job = HANDLE(self.outer.as_ref().unwrap().as_raw_handle());
        unsafe { AssignProcessToJobObject(job, HANDLE(self.child.as_raw_handle())) }.unwrap();
        let mut member = BOOL(0);
        unsafe { IsProcessInJob(HANDLE(self.child.as_raw_handle()), Some(job), &mut member) }
            .unwrap();
        assert!(member.as_bool());
    }
    fn cleanup(&mut self) -> bool {
        let now = Instant::now();
        let end = *self
            .cleanup_end
            .get_or_insert_with(|| now.checked_add(Duration::from_secs(2)).unwrap_or(now));
        // Closing only our unnamed outer Job covers the pre-inner-assignment window.
        drop(self.outer.take());
        if self.root_confirmed {
            return true;
        }
        let _ = self.child.kill();
        while Instant::now() < end {
            if matches!(self.child.try_wait(), Ok(Some(_))) && Instant::now() < end {
                self.root_confirmed = true;
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn outer_job() -> OwnedHandle {
    let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.unwrap();
    let own = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    }
    .unwrap();
    own
}
fn capture(
    mut pipe: impl Read + Send + 'static,
    tag: usize,
    done: mpsc::SyncSender<(usize, std::io::Result<Vec<u8>>)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .by_ref()
            .take(16385)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = done.send((tag, result));
    })
}

pub(super) fn run(case: &str) -> bool {
    const CASES: [&str; 10] = [
        "fresh_owned_pipe_launch_phase_probe",
        "actual_owned_pipe_is_committed_through_durable_single_writer",
        "actual_owner_factory_pipe_deduplicates_receipt_and_retires_writer",
        "actual_guarded_owner_pipe_rejects_stale_view_and_disconnected_input",
        "actual_pipe_guard_bounds_and_queue_are_fail_closed",
        "actual_writer_panic_retains_terminal_join_error_and_unknown_intent",
        "actual_pipe_effect_then_storage_failure_reopens_only_as_unknown",
        "actual_owned_pty_uses_the_same_durable_single_writer_without_reply_bypass",
        "actual_owned_pty_uses_canonical_writer_without_reply_bypass",
        "cli_tests::installed_codex_help_uses_empty_home_and_private_pty_reply_gate",
    ];
    assert!(CASES.contains(&case));
    if let Some(sentinel) = std::env::var_os("CC_DESK_TEST_ONLY_WRITER_WORKER") {
        assert_eq!(sentinel, case);
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(18));
            std::process::exit(91);
        });
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        assert_eq!(go, *b"G");
        eprintln!("TEST_ONLY_E_PHASE=contained_go_received");
        return true;
    }
    let temp = tempfile::tempdir().unwrap();
    let outer = outer_job();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg(format!("local_writer::tests::{case}"))
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env_clear()
        .env("TEMP", temp.path())
        .env("TMP", temp.path())
        .env("CC_DESK_TEST_ONLY_WRITER_WORKER", case)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x00000008);
    if case == "cli_tests::installed_codex_help_uses_empty_home_and_private_pty_reply_gate" {
        command.arg("--ignored");
        for name in [
            "SystemRoot",
            "CC_DESK_TEST_ONLY_CODEX_EXE",
            "CC_DESK_TEST_ONLY_CODEX_SHA256",
        ] {
            command.env(
                name,
                std::env::var_os(name).expect("explicit reviewed CLI fixture metadata required"),
            );
        }
    }
    let mut owner = Supervisor {
        child: command.spawn().unwrap(),
        outer: Some(outer),
        cleanup_end: None,
        root_confirmed: false,
    };
    owner.contain();
    let (tx, rx) = mpsc::sync_channel(2);
    let a = capture(owner.child.stdout.take().unwrap(), 0, tx.clone());
    let b = capture(owner.child.stderr.take().unwrap(), 1, tx);
    let end = Instant::now() + Duration::from_secs(12);
    owner.child.stdin.take().unwrap().write_all(b"G").unwrap();
    let status = loop {
        if Instant::now() >= end {
            break None;
        }
        if let Some(status) = owner.child.try_wait().unwrap() {
            if Instant::now() < end {
                owner.root_confirmed = true;
                break Some(status);
            }
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let cleanup = owner.cleanup();
    let capture_end = owner.cleanup_end.unwrap();
    let mut outputs = [None, None];
    for _ in 0..2 {
        if Instant::now() >= capture_end {
            break;
        }
        if let Ok((i, bytes)) =
            rx.recv_timeout(capture_end.saturating_duration_since(Instant::now()))
        {
            if Instant::now() >= capture_end {
                break;
            }
            if let Ok(bytes) = &bytes {
                assert!(bytes.len() <= 16384);
            }
            outputs[i] = Some(bytes);
        }
    }
    for bytes in outputs.iter().flatten().flatten() {
        eprintln!("{}", String::from_utf8_lossy(bytes));
    }
    let mut joined = true;
    for thread in [a, b] {
        while !thread.is_finished() && Instant::now() < capture_end {
            std::thread::sleep(Duration::from_millis(1));
        }
        if thread.is_finished() && Instant::now() < capture_end {
            thread.join().unwrap();
        } else {
            joined = false;
        }
    }
    assert!(
        status.is_some(),
        "E worker deadline: exact root cleanup={cleanup}; descendants not independently observed"
    );
    assert!(
        cleanup
            && joined
            && outputs
                .iter()
                .all(|result| result.as_ref().is_some_and(Result::is_ok)),
        "E capture/cleanup Unknown"
    );
    assert_eq!(status.unwrap().code(), Some(0));
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    struct DataThenError(bool);
    impl Read for DataThenError {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if !self.0 {
                self.0 = true;
                bytes[..3].copy_from_slice(b"pub");
                Ok(3)
            } else {
                Err(std::io::Error::other("TEST_ONLY_CAPTURE_READ_ERROR"))
            }
        }
    }
    #[test]
    fn capture_partial_read_error_is_never_confirmed_eof() {
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = capture(DataThenError(false), 0, tx);
        let (tag, result) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert_eq!(tag, 0);
        assert!(
            result.is_err(),
            "partial read error was falsely published as confirmed EOF"
        );
    }
    #[test]
    fn capture_normal_eof_preserves_exact_public_bytes() {
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = capture(std::io::Cursor::new(b"PUBLIC_TEST_ONLY".to_vec()), 1, tx);
        let (tag, result) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert_eq!(tag, 1);
        assert_eq!(result.unwrap(), b"PUBLIC_TEST_ONLY");
    }
}
