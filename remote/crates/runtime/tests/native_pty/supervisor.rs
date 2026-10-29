use std::{
    io::{Read, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::HANDLE,
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    },
};
pub(super) fn job() -> OwnedHandle {
    let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.unwrap();
    let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
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
    owned
}
struct Owner {
    child: Child,
    outer: Option<OwnedHandle>,
    cleanup_end: Option<Instant>,
    root_confirmed: bool,
}
impl Owner {
    fn cleanup(&mut self) -> bool {
        let end = *self
            .cleanup_end
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
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
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn capture(
    mut pipe: impl Read + Send + 'static,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<std::io::Result<Vec<u8>>>,
) {
    let (tx, rx) = mpsc::sync_channel(1);
    let handle = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .by_ref()
            .take(65537)
            .read_to_end(&mut bytes)
            .and_then(|_| {
                if bytes.len() <= 65536 {
                    Ok(bytes)
                } else {
                    Err(std::io::Error::other("CAPTURE_LIMIT"))
                }
            });
        let _ = tx.send(result);
    });
    (handle, rx)
}
// Adapted from the reviewed OS cmd fixture supervisor; original source is unchanged.
pub(super) fn run(mode: &str, worker: &str) {
    assert_eq!(worker, "fixed_pty_runtime_worker");
    assert!(matches!(mode, "custody" | "loader-reject"));
    let temp = tempfile::tempdir().unwrap();
    let outer = job();
    let os_root = std::env::var_os("SystemRoot").unwrap();
    let current = std::env::current_exe().unwrap();
    let executable = if mode == "loader-reject" {
        let isolated = temp.path().join("own-worker-no-runtime.exe");
        std::fs::copy(&current, &isolated).unwrap();
        isolated
    } else {
        current
    };
    let child = Command::new(executable)
        .args([
            "--exact",
            worker,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .creation_flags(0x00000008)
        .env_clear()
        .env("SystemRoot", os_root)
        .env("PTY_CLI_TEST_MODE", mode)
        .env("PTY_CLI_TEST_DIR", temp.path())
        .env("TEMP", temp.path())
        .env("TMP", temp.path())
        .current_dir(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut owner = Owner {
        child,
        outer: Some(outer),
        cleanup_end: None,
        root_confirmed: false,
    };
    unsafe {
        AssignProcessToJobObject(
            HANDLE(owner.outer.as_ref().unwrap().as_raw_handle()),
            HANDLE(owner.child.as_raw_handle()),
        )
    }
    .unwrap();
    let (out, rx) = capture(owner.child.stdout.take().unwrap());
    let (err, erx) = capture(owner.child.stderr.take().unwrap());
    let end = Instant::now() + Duration::from_secs(20);
    owner.child.stdin.take().unwrap().write_all(&[1]).unwrap();
    let result = loop {
        if let Some(result) = owner.child.try_wait().unwrap() {
            break Some(result);
        }
        if Instant::now() >= end {
            break None;
        }
        std::thread::sleep(Duration::from_millis(3));
    };
    let stdout = rx.recv_timeout(end.saturating_duration_since(Instant::now()));
    let stderr = erx.recv_timeout(end.saturating_duration_since(Instant::now()));
    let cleaned = owner.cleanup();
    let cleanup_end = owner.cleanup_end.expect("cleanup budget captured once");
    while !(out.is_finished() && err.is_finished()) && Instant::now() < cleanup_end {
        std::thread::sleep(Duration::from_millis(1));
    }
    let joined = out.is_finished() && err.is_finished() && Instant::now() < cleanup_end;
    if joined {
        out.join().unwrap();
        err.join().unwrap();
    }
    assert!(cleaned && joined, "owned worker/capture cleanup unknown");
    let stdout = stdout.unwrap().unwrap();
    let stderr = stderr.unwrap().unwrap();
    println!(
        "mode={mode}; stdout={}; stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(result.unwrap().code(), Some(0));
    assert!(Instant::now() < end);
    assert!(String::from_utf8_lossy(&stdout).contains("PASS:NATIVE_PTY_TASK1"));
}
