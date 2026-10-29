#![cfg(all(windows, feature = "conpty-qualification"))]
use std::{
    io::{Read, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Command, Stdio},
    sync::{mpsc, Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::HANDLE,
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    },
};
// One absolute readiness deadline, including participants that fail before arriving.
struct ReadyGate {
    state: Mutex<usize>,
    changed: Condvar,
    participants: usize,
    end: Instant,
}
impl ReadyGate {
    fn new(participants: usize, budget: Duration) -> Self {
        Self {
            state: Mutex::new(0),
            changed: Condvar::new(),
            participants,
            end: Instant::now() + budget,
        }
    }
    fn arrive(&self) -> Result<(), &'static str> {
        let mut count = self.state.lock().unwrap();
        if Instant::now() >= self.end {
            return Err("ready-deadline");
        }
        *count += 1;
        self.changed.notify_all();
        loop {
            if Instant::now() >= self.end {
                return Err("ready-deadline");
            }
            if *count == self.participants {
                return Ok(());
            }
            let result = self
                .changed
                .wait_timeout(count, self.end.saturating_duration_since(Instant::now()))
                .unwrap();
            count = result.0;
        }
    }
}
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
fn confirmed_capture(value: &Option<std::io::Result<Vec<u8>>>) -> bool {
    value
        .as_ref()
        .is_some_and(|result| result.as_ref().is_ok_and(|bytes| bytes.len() <= 16384))
}
fn fresh_case_mode(barrier: Option<Arc<ReadyGate>>, mode: &str) {
    let dir = tempfile::tempdir().unwrap();
    let outer = outer_job();
    let mut command = Command::new(env!("CARGO_BIN_EXE_native-pipe-qualification-worker"));
    command
        .arg("TEST_ONLY_D_WORKER")
        .arg(env!("CARGO_BIN_EXE_native-pipe-fixture"))
        .arg(dir.path())
        .arg(mode)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x00000008);
    let mut owner = Supervisor {
        child: command.spawn().unwrap(),
        outer: Some(outer),
        cleanup_end: None,
        root_confirmed: false,
    };
    owner.contain(); // Worker cannot start native creation before the following GO.
    let (tx, rx) = mpsc::sync_channel(2);
    let a = capture(owner.child.stdout.take().unwrap(), 0, tx.clone());
    let b = capture(owner.child.stderr.take().unwrap(), 1, tx);
    if let Some(barrier) = barrier {
        barrier.arrive().unwrap();
    }
    let end = Instant::now() + Duration::from_secs(6);
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
    let timed_out = status.is_none();
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
        } // Detached only on failure, never counted as joined.
    }
    assert!(!timed_out, "D_WORKER_TIMEOUT exact_root_cleanup={cleanup}; containment termination requested, descendant exit not independently observed");
    assert!(
        cleanup && joined && outputs.iter().all(confirmed_capture),
        "bounded root/capture EOF/join unknown"
    );
    assert_eq!(status.unwrap().code(), Some(0));
}
#[test]
fn native_primary_handle_pipe_preserves_unicode_empty_env_and_three_streams() {
    fresh_case_mode(None, "ECHO");
}
#[test]
fn five_concurrent_native_primary_handles_under_original_500ms_qualification() {
    let barrier = Arc::new(ReadyGate::new(5, Duration::from_secs(2)));
    let threads: Vec<_> = (0..5)
        .map(|_| {
            let b = barrier.clone();
            std::thread::spawn(move || fresh_case_mode(Some(b), "ECHO"))
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn failed_preparation_cannot_park_ready_peer_without_a_deadline() {
    let gate = Arc::new(ReadyGate::new(2, Duration::from_millis(20)));
    let other = gate.clone();
    let peer = std::thread::spawn(move || other.arrive());
    // A second participant fails before arriving. No rescue participant is needed.
    assert_eq!(peer.join().unwrap(), Err("ready-deadline"));
    assert_eq!(gate.arrive(), Err("ready-deadline"));
}

#[test]
fn native_assignment_refusal_is_reaped_before_fixture_entry() {
    fresh_case_mode(None, "ASSIGN_REJECT");
}
#[test]
fn native_stop_confirms_owned_root_and_three_stream_eof() {
    fresh_case_mode(None, "STOP");
}

#[test]
fn unrelated_inheritable_event_is_not_in_the_three_stdio_handle_list() {
    fresh_case_mode(None, "CANARY");
}
#[test]
fn an_unrelated_child_event_is_not_evidence_of_parent_canary_inheritance() {
    fresh_case_mode(None, "CANARY_REUSED");
}
#[test]
fn recipe_rejects_invalid_or_overbudget_metadata_before_any_native_call() {
    use cc_desk_native_runtime::{BackendPipeLaunch, NativePipeError, NativePipePhase};
    use std::{ffi::OsString, os::windows::ffi::OsStrExt, path::PathBuf};
    let program = PathBuf::from(r"C:\TEST_ONLY\fixture.exe");
    let cwd = PathBuf::from(r"C:\TEST_ONLY");
    let make = |args: Vec<OsString>, env: Vec<(OsString, OsString)>| {
        BackendPipeLaunch::from_backend_recipe(program.clone(), cwd.clone(), args, env)
    };
    let rejected = |result: Result<BackendPipeLaunch, NativePipeError>| {
        assert!(matches!(
            result,
            Err(NativePipeError::KnownNotStarted {
                phase: NativePipePhase::Preparation
            })
        ))
    };
    assert!(make(vec!["x".into(); 64], Vec::new()).is_ok());
    rejected(make(vec!["x".into(); 65], Vec::new()));
    assert!(make(vec!["x".repeat(4096).into()], Vec::new()).is_ok());
    rejected(make(vec!["x".repeat(4097).into()], Vec::new()));
    let base = program.as_os_str().encode_wide().count() + cwd.as_os_str().encode_wide().count();
    let mut exact = vec![OsString::from("x".repeat(4096)); 3];
    exact.push("x".repeat(16384 - base - 3 * 4096).into());
    assert!(make(exact.clone(), Vec::new()).is_ok());
    exact[3].push("x");
    rejected(make(exact, Vec::new()));
    let env: Vec<_> = (0..64)
        .map(|n| (OsString::from(format!("KEY_{n}")), OsString::from("value")))
        .collect();
    assert!(make(Vec::new(), env.clone()).is_ok());
    let mut too_many = env;
    too_many.push(("ANOTHER".into(), "value".into()));
    rejected(make(Vec::new(), too_many));
    rejected(make(vec!["x\0y".into()], Vec::new()));
    rejected(make(Vec::new(), vec![("KEY".into(), "x\0y".into())]));
    rejected(make(
        Vec::new(),
        vec![("KEY".into(), "x".into()), ("key".into(), "y".into())],
    ));
    rejected(make(Vec::new(), vec![("BAD=KEY".into(), "x".into())]));
    rejected(BackendPipeLaunch::from_backend_recipe(
        "relative.exe".into(),
        cwd.clone(),
        Vec::new(),
        Vec::new(),
    ));
    rejected(BackendPipeLaunch::from_backend_recipe(
        program,
        "relative-cwd".into(),
        Vec::new(),
        Vec::new(),
    ));
}

struct CaptureReaderError(bool);
impl Read for CaptureReaderError {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if !self.0 {
            self.0 = true;
            let count = bytes.len().min(3);
            bytes[..count].copy_from_slice(&b"pub"[..count]);
            Ok(count)
        } else {
            Err(std::io::Error::other("TEST_ONLY_F_CAPTURE_ERROR"))
        }
    }
}
fn reader_result(reader: impl Read + Send + 'static) -> Option<std::io::Result<Vec<u8>>> {
    let (tx, rx) = mpsc::sync_channel(1);
    let thread = capture(reader, 0, tx);
    let (tag, result) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    thread.join().unwrap();
    assert_eq!(tag, 0);
    Some(result)
}
#[test]
fn capture_reader_partial_error_is_not_eof() {
    let result = reader_result(CaptureReaderError(false));
    assert!(
        !confirmed_capture(&result),
        "read error falsely confirmed EOF"
    );
    assert!(result.unwrap().is_err());
}
#[test]
fn capture_reader_clean_eof_is_accepted() {
    for bytes in [Vec::new(), b"PUBLIC_TEST_ONLY".to_vec(), vec![b'x'; 16384]] {
        let result = reader_result(std::io::Cursor::new(bytes.clone()));
        assert!(confirmed_capture(&result));
        assert_eq!(result.unwrap().unwrap(), bytes);
    }
}
#[test]
fn capture_reader_oversize_remains_rejected() {
    let result = reader_result(std::io::Cursor::new(vec![b'x'; 32768]));
    assert!(!confirmed_capture(&result));
    assert_eq!(result.unwrap().unwrap().len(), 16385);
}
