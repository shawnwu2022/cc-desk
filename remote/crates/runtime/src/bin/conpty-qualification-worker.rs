//! TEST_ONLY finite worker. No network, account, enrollment or terminal dispatch.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../../src-tauri/src/conpty_runtime.rs"]
mod bundled_runtime;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../windows_job.rs"]
mod windows_job;
#[cfg(windows)]
use cc_desk_native_runtime::RuntimeError;
#[cfg(windows)]
#[path = "conpty_qualification/lifecycle.rs"]
mod lifecycle;

#[cfg(windows)]
fn fixture(marker: &std::path::Path) {
    use std::io::{BufRead, Write};
    use windows::Win32::System::Console::{
        GetConsoleScreenBufferInfo, GetStdHandle, CONSOLE_SCREEN_BUFFER_INFO, STD_OUTPUT_HANDLE,
    };
    std::fs::write(marker, b"TEST_ONLY_EXECUTED").unwrap();
    println!("TEST_ONLY_READY");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if line == "quit" {
            break;
        }
        if line == "size" {
            let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
            unsafe {
                GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE).unwrap(), &mut info)
            }
            .unwrap();
            println!("TEST_ONLY_SIZE:{}x{}", info.dwSize.X, info.dwSize.Y);
        } else {
            println!("TEST_ONLY_ECHO:{line}");
        }
        std::io::stdout().flush().unwrap();
    }
}

#[cfg(windows)]
fn qualification(mode: &str, marker: &std::path::Path) -> Result<(), &'static str> {
    use portable_pty::{native_pty_system, CommandBuilder, JobSpawnError, JobSpawnPhase, PtySize};
    use std::{
        io::{Read, Write},
        os::windows::io::AsRawHandle,
        sync::{mpsc, Arc, Mutex},
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        System::{
            JobObjects::IsProcessInJob,
            Threading::{GetCurrentProcess, TerminateProcess},
        },
    };
    bundled_runtime::initialize().map_err(|_| "PINNED_LOADER_REJECTED")?;
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .map_err(|_| "PTY_OPEN")?;
    let mut reader = pair.master.try_clone_reader().map_err(|_| "PTY_READER")?;
    let writer = Arc::new(Mutex::new(
        pair.master.take_writer().map_err(|_| "PTY_WRITER")?,
    ));
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let drained = output.clone();
    let reply_writer = writer.clone();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let drain = std::thread::spawn(move || {
        // Fixed small startup replies, not a terminal emulator or remote input.
        let mut reply_writer = Some(reply_writer);
        let mut replied_dsr = false;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut bytes = drained.lock().unwrap();
                    if bytes.len() + n > 256 * 1024 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(writer) = &reply_writer {
                        if !replied_dsr && bytes.windows(4).any(|x| x == b"\x1b[6n") {
                            let _ = writer.lock().unwrap().write_all(b"\x1b[1;1R");
                            replied_dsr = true;
                        }
                        if bytes.windows(3).any(|x| x == b"\x1b[c")
                            || bytes.windows(4).any(|x| x == b"\x1b[0c")
                        {
                            let _ = writer.lock().unwrap().write_all(b"\x1b[?1;2c");
                            reply_writer = None;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        let _ = done_tx.send(());
    });
    let mut job = Some(windows_job::Job::new().map_err(|_| "JOB_SETUP")?);
    let mut cmd = CommandBuilder::new(std::env::current_exe().map_err(|_| "SELF_PATH")?);
    cmd.arg("--fixture");
    cmd.arg(marker);
    cmd.env_clear();
    let result = if mode == "assignment-rejected" {
        use std::os::windows::io::BorrowedHandle;
        // A valid, borrowed current-process handle is deliberately not a Job.
        let not_job = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess().0) };
        pair.slave.spawn_command_in_job(cmd, not_job)
    } else {
        pair.slave
            .spawn_command_in_job(cmd, job.as_ref().unwrap().borrowed())
    };
    if mode == "assignment-rejected" {
        if !matches!(
            result,
            Err(JobSpawnError::KnownNotStarted {
                phase: JobSpawnPhase::AssignmentReaped
            })
        ) || marker.exists()
        {
            return Err("ASSIGNMENT_FAILURE_EXECUTED_OR_UNKNOWN");
        }
        drop(writer);
        drop(pair);
        drop(job);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "DRAIN_UNKNOWN")?;
        drain.join().map_err(|_| "DRAIN_PANIC")?;
        println!("PASS:ASSIGNMENT_REJECTED_BEFORE_EXECUTION");
        return Ok(());
    }
    let mut child = result.map_err(|_| "JOB_SPAWN_REJECTED")?;
    let mut belongs = windows::core::BOOL::default();
    let raw = child.as_raw_handle().ok_or("NO_OWNED_PROCESS_HANDLE")?;
    unsafe {
        IsProcessInJob(
            HANDLE(raw),
            Some(HANDLE(job.as_ref().unwrap().borrowed().as_raw_handle())),
            &mut belongs,
        )
    }
    .map_err(|_| "JOB_QUERY")?;
    if !belongs.as_bool() {
        return Err("WRONG_JOB");
    }
    let contains = |needle: &[u8]| {
        output
            .lock()
            .unwrap()
            .windows(needle.len())
            .any(|x| x == needle)
    };
    let wait_for = |needle: &[u8]| -> Result<(), &'static str> {
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end {
            if contains(needle) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("OUTPUT_DEADLINE")
    };
    wait_for(b"TEST_ONLY_READY")?;
    if !marker.exists() {
        return Err("MISSING_EXECUTION_MARKER");
    }
    pair.master
        .resize(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|_| "RESIZE")?;
    writer
        .lock()
        .unwrap()
        .write_all(b"size\r\nhello\r\n")
        .map_err(|_| "INPUT")?;
    wait_for(b"TEST_ONLY_SIZE:100x30")?;
    wait_for(b"TEST_ONLY_ECHO:hello")?;
    // The exact retained handle is independent of PTY drain/writer locks.
    if mode == "job-drop" {
        drop(job.take());
    } else {
        unsafe { TerminateProcess(HANDLE(raw), 0) }.map_err(|_| "OWNED_STOP")?;
    }
    let end = Instant::now() + Duration::from_secs(2);
    while child.try_wait().map_err(|_| "ROOT_WAIT")?.is_none() {
        if Instant::now() >= end {
            return Err("ROOT_EXIT_UNKNOWN");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(child);
    drop(job);
    drop(writer);
    drop(pair);
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "DRAIN_UNKNOWN")?;
    drain.join().map_err(|_| "DRAIN_PANIC")?;
    println!("PASS:PINNED_IO_RESIZE_OWNED_ROOT_STOP_DRAIN_OBSERVED");
    Ok(())
}

#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        eprintln!("FIXED_ARGUMENTS_REQUIRED");
        std::process::exit(2);
    }
    if args[1] == "--fixture" {
        fixture(std::path::Path::new(&args[2]));
        return;
    }
    if args[1] == "--lifecycle-fixture" || args[1] == "--pressure-fixture" {
        lifecycle::fixture(
            std::path::Path::new(&args[2]),
            args[1] == "--pressure-fixture",
        );
        return;
    }
    if args[1] == "--known-grandchild" {
        lifecycle::grandchild(std::path::Path::new(&args[2]));
        return;
    }
    let mode = args[1].to_str().unwrap_or("");
    if matches!(
        mode,
        "lifecycle-crash" | "lifecycle-pressure" | "lifecycle-tree"
    ) {
        if let Err(code) = lifecycle::run(mode, std::path::Path::new(&args[2])) {
            eprintln!("{code}");
            std::process::exit(3);
        }
        return;
    }
    if !matches!(mode, "normal" | "job-drop" | "assignment-rejected") {
        std::process::exit(2);
    }
    if let Err(code) = qualification(mode, std::path::Path::new(&args[2])) {
        eprintln!("{code}");
        std::process::exit(3);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("WINDOWS_ONLY");
    std::process::exit(2);
}
