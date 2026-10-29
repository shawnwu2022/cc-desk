//! Private TEST_ONLY supervision/IO probes. No public Host or arbitrary launch API.
use std::{
    io::{BufRead, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{DuplicateHandle, DUPLICATE_HANDLE_OPTIONS, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{
            GetCurrentProcess, TerminateProcess, WaitForSingleObject, PROCESS_DUP_HANDLE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    },
};

fn transferred_handle(hex: &str) -> Result<OwnedHandle, &'static str> {
    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("PRIVATE_HANDLE_ENCODING");
    }
    let value = usize::from_str_radix(hex, 16).map_err(|_| "PRIVATE_HANDLE_VALUE")?;
    if value == 0 || value == usize::MAX {
        return Err("PRIVATE_HANDLE_VALUE");
    }
    // This qualification-only value was installed by a known process's actual
    // DuplicateHandle. No untrusted application/relay DTO can call this helper.
    Ok(unsafe { OwnedHandle::from_raw_handle(value as *mut _) })
}
fn control_handle(end: Instant) -> Result<OwnedHandle, &'static str> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::stdin().lock().take(64).read_line(&mut line);
        let _ = tx.send(match result {
            Ok(_) if line.ends_with('\n') => Ok(line),
            _ => Err("CONTROL_INPUT_TRUNCATED"),
        });
    });
    let line = rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .map_err(|_| "CONTROL_INPUT_DEADLINE")??;
    transferred_handle(line.trim_end_matches(['\r', '\n']))
}
pub(super) fn grandchild(marker: &Path) {
    std::fs::write(marker, b"TEST_ONLY_KNOWN_GRANDCHILD").unwrap();
    std::thread::sleep(Duration::from_secs(5));
    std::process::exit(90);
}

pub(super) fn fixture(marker: &Path, pressure: bool) {
    let lifetime = if pressure { 12 } else { 5 };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(lifetime));
        std::process::exit(90);
    });
    std::fs::write(marker, b"TEST_ONLY_EXECUTED").unwrap();
    println!("TEST_ONLY_LIFECYCLE_READY");
    std::io::stdout().flush().unwrap();
    // Only one bounded private command is accepted. The fixture's absolute
    // 5s/12s watchdog also bounds a missing/partial synchronous terminal read.
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .take(64)
        .read_line(&mut line)
        .unwrap();
    let mut known_child = None;
    if line.ends_with('\n') && line.len() <= 63 {
        let command = line.trim_end_matches(['\r', '\n']);
        if let Some(hex) = command.strip_prefix("grandchild:") {
            let parent = transferred_handle(hex).unwrap();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--known-grandchild")
                .arg(marker.with_extension("grandchild"))
                .env_clear()
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let mut in_parent = HANDLE::default();
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    HANDLE(child.as_raw_handle()),
                    HANDLE(parent.as_raw_handle()),
                    &mut in_parent,
                    (PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).0,
                    false,
                    DUPLICATE_HANDLE_OPTIONS(0),
                )
            }
            .unwrap();
            known_child = Some(child);
            println!("TEST_ONLY_GRANDCHILD:{:x}", in_parent.0 as usize);
            std::io::stdout().flush().unwrap();
        } else if command == "pressure" && pressure {
            let heartbeat = marker.with_extension("heartbeat");
            let progress = marker.with_extension("progress");
            let complete = marker.with_extension("complete");
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let beat = std::thread::spawn(move || {
                let mut n = 0u64;
                while !flag.load(Ordering::Acquire) {
                    n += 1;
                    std::fs::write(&heartbeat, n.to_string()).unwrap();
                    std::thread::sleep(Duration::from_millis(15));
                }
            });
            std::fs::write(&progress, b"0").unwrap();
            let mut chunk = [b'X'; 16 * 1024];
            let mut stdout = std::io::stdout().lock();
            for n in 1u64..=4096 {
                // Public, sequence-varying text with line boundaries. Avoid
                // relying on the console host to re-emit an unchanged screen.
                chunk.fill(b'A' + (n % 26) as u8);
                for line in chunk.chunks_mut(80) {
                    if let Some(last) = line.last_mut() {
                        *last = b'\n';
                    }
                }
                let sequence = format!("TEST_ONLY_CHUNK:{n:04}\r\n");
                chunk[..sequence.len()].copy_from_slice(sequence.as_bytes());
                stdout.write_all(&chunk).unwrap();
                stdout.flush().unwrap();
                std::fs::write(&progress, (n * chunk.len() as u64).to_string()).unwrap();
            }
            std::fs::write(complete, b"TEST_ONLY_COMPLETE").unwrap();
            stop.store(true, Ordering::Release);
            beat.join().unwrap();
        }
    }
    // EOF is not success for the abrupt-exit probe. The finite watchdog is a
    // fallback only; positive tests require observed exit before this deadline.
    loop {
        std::hint::black_box(&known_child);
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

struct DrainState {
    pause_at: AtomicU64,
    paused: AtomicBool,
    stop: AtomicBool,
    total: AtomicU64,
    prefix: Mutex<Vec<u8>>,
}
struct ResumeOnDrop(Arc<DrainState>);
impl Drop for ResumeOnDrop {
    fn drop(&mut self) {
        self.0.pause_at.store(u64::MAX, Ordering::Release);
        self.0.stop.store(true, Ordering::Release);
    }
}
fn event(text: &str) -> Result<(), &'static str> {
    println!("{text}");
    std::io::stdout().flush().map_err(|_| "CONTROL_OUTPUT")
}
fn until(end: Instant, mut check: impl FnMut() -> bool) -> Result<(), &'static str> {
    while Instant::now() < end {
        if check() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("PHASE_DEADLINE")
}
fn counter(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(33).read_to_string(&mut text).ok()?;
    if text.len() > 20 {
        return None;
    }
    text.parse().ok()
}
fn stable_stall(
    marker: &Path,
    state: &DrainState,
    raw: HANDLE,
    end: Instant,
) -> Result<(u64, u64), &'static str> {
    let progress = marker.with_extension("progress");
    let heartbeat = marker.with_extension("heartbeat");
    let mut last = None;
    let mut since = Instant::now();
    let mut initial_beat = 0;
    while Instant::now() < end {
        if marker.with_extension("complete").exists() {
            return Err("OUTPUT_BUDGET_COMPLETED_NOT_STALLED");
        }
        if unsafe { WaitForSingleObject(raw, 0) } != WAIT_TIMEOUT {
            return Err("FIXTURE_EXITED_NOT_STALLED");
        }
        if !state.paused.load(Ordering::Acquire) {
            return Err("DRAIN_NOT_PAUSED");
        }
        if let (Some(written), Some(beat)) = (counter(&progress), counter(&heartbeat)) {
            if last != Some(written) {
                last = Some(written);
                since = Instant::now();
                initial_beat = beat;
            }
            if since.elapsed() >= Duration::from_millis(300) && beat >= initial_beat + 3 {
                return Ok((written, beat));
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("NO_STABLE_OUTPUT_STALL")
}

pub(super) fn run(mode: &str, marker: &Path) -> Result<(), &'static str> {
    // This private pipe value was installed by DuplicateHandle from the process
    // which actually created us. No PID opening or external transport is involved.
    let supervisor = control_handle(Instant::now() + Duration::from_secs(3))?;
    super::bundled_runtime::initialize().map_err(|_| "PINNED_LOADER_REJECTED")?;
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize::default())
        .map_err(|_| "PTY_OPEN")?;
    let mut reader = pair.master.try_clone_reader().map_err(|_| "PTY_READER")?;
    let writer = Arc::new(Mutex::new(
        pair.master.take_writer().map_err(|_| "PTY_WRITER")?,
    ));
    let state = Arc::new(DrainState {
        pause_at: AtomicU64::new(u64::MAX),
        paused: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        total: AtomicU64::new(0),
        prefix: Mutex::new(Vec::new()),
    });
    let drain_state = state.clone();
    let reply_writer = writer.clone();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let drain = std::thread::spawn(move || {
        let mut reply_writer = Some(reply_writer);
        let mut replied_dsr = false;
        let mut buf = [0; 4096];
        let result = loop {
            while drain_state.total.load(Ordering::Acquire)
                >= drain_state.pause_at.load(Ordering::Acquire)
                && !drain_state.stop.load(Ordering::Acquire)
            {
                drain_state.paused.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(1));
            }
            drain_state.paused.store(false, Ordering::Release);
            if drain_state.stop.load(Ordering::Acquire) {
                break Err("DRAIN_STOPPED_WITHOUT_EOF");
            }
            match reader.read(&mut buf) {
                Ok(0) => break Ok(()),
                Err(_) => break Err("DRAIN_IO_NOT_EOF"),
                Ok(n) => {
                    drain_state.total.fetch_add(n as u64, Ordering::AcqRel);
                    let mut bytes = drain_state.prefix.lock().unwrap();
                    let keep = (8192 - bytes.len()).min(n);
                    bytes.extend_from_slice(&buf[..keep]);
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
            }
        };
        let _ = done_tx.send(result);
    });
    let _resume = ResumeOnDrop(state.clone());
    let job = super::windows_job::Job::new().map_err(|_| "JOB_SETUP")?;
    let mut cmd =
        portable_pty::CommandBuilder::new(std::env::current_exe().map_err(|_| "SELF_PATH")?);
    cmd.arg(if mode == "lifecycle-pressure" {
        "--pressure-fixture"
    } else {
        "--lifecycle-fixture"
    });
    cmd.arg(marker);
    cmd.env_clear();
    let mut child = pair
        .slave
        .spawn_command_in_job(cmd, job.borrowed())
        .map_err(|_| "JOB_SPAWN")?;
    let raw = HANDLE(child.as_raw_handle().ok_or("NO_OWNED_PROCESS_HANDLE")?);
    let mut belongs = windows::core::BOOL::default();
    unsafe {
        IsProcessInJob(
            raw,
            Some(HANDLE(job.borrowed().as_raw_handle())),
            &mut belongs,
        )
    }
    .map_err(|_| "JOB_QUERY")?;
    if !belongs.as_bool() {
        return Err("WRONG_JOB");
    }
    let mut in_supervisor = HANDLE::default();
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            raw,
            HANDLE(supervisor.as_raw_handle()),
            &mut in_supervisor,
            (PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    }
    .map_err(|_| "WITNESS_DUPLICATE")?;
    event(&format!("WITNESS:{:x}", in_supervisor.0 as usize))?;
    until(Instant::now() + Duration::from_secs(3), || {
        state
            .prefix
            .lock()
            .unwrap()
            .windows(25)
            .any(|x| x == b"TEST_ONLY_LIFECYCLE_READY")
    })?;
    if mode == "lifecycle-crash" || mode == "lifecycle-tree" {
        if mode == "lifecycle-tree" {
            let mut in_fixture = HANDLE::default();
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    HANDLE(supervisor.as_raw_handle()),
                    raw,
                    &mut in_fixture,
                    PROCESS_DUP_HANDLE.0,
                    false,
                    DUPLICATE_HANDLE_OPTIONS(0),
                )
            }
            .map_err(|_| "FIXTURE_PARENT_CAP")?;
            writer
                .lock()
                .unwrap()
                .write_all(format!("grandchild:{:x}\r\n", in_fixture.0 as usize).as_bytes())
                .map_err(|_| "GRANDCHILD_INPUT")?;
            let mut hex = None;
            until(Instant::now() + Duration::from_secs(2), || {
                let bytes = state.prefix.lock().unwrap();
                let label = b"TEST_ONLY_GRANDCHILD:";
                if let Some(start) = bytes.windows(label.len()).position(|x| x == label) {
                    let tail = &bytes[start + label.len()..];
                    let len = tail.iter().take_while(|b| b.is_ascii_hexdigit()).count();
                    if len > 0 && len <= 16 && tail.len() > len {
                        hex = std::str::from_utf8(&tail[..len]).ok().map(str::to_owned);
                    }
                }
                hex.is_some()
            })?;
            event(&format!("GRANDCHILD:{}", hex.ok_or("GRANDCHILD_ENCODING")?))?;
            let query = control_handle(Instant::now() + Duration::from_secs(2))?;
            let query_raw = HANDLE(query.as_raw_handle());
            let mut in_job = windows::core::BOOL::default();
            unsafe {
                IsProcessInJob(
                    query_raw,
                    Some(HANDLE(job.borrowed().as_raw_handle())),
                    &mut in_job,
                )
            }
            .map_err(|_| "GRANDCHILD_JOB_QUERY")?;
            if !in_job.as_bool()
                || unsafe { WaitForSingleObject(query_raw, 0) } != WAIT_TIMEOUT
                || unsafe { WaitForSingleObject(raw, 0) } != WAIT_TIMEOUT
            {
                return Err("GRANDCHILD_NOT_LIVE_IN_EXACT_JOB");
            }
            event("READY:TREE_JOB_MEMBERSHIP_VERIFIED")?;
        } else {
            event("READY:CRASH")?;
        }
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    state.pause_at.store(64 * 1024, Ordering::Release);
    writer
        .lock()
        .unwrap()
        .write_all(b"pressure\r\n")
        .map_err(|_| "PRESSURE_INPUT")?;
    until(Instant::now() + Duration::from_secs(2), || {
        state.paused.load(Ordering::Acquire)
    })?;
    let first = stable_stall(marker, &state, raw, Instant::now() + Duration::from_secs(2))?;
    event(&format!(
        "PRESSURE:STALLED1:read={}:written={}:heartbeat={}",
        state.total.load(Ordering::Acquire),
        first.0,
        first.1
    ))?;
    let read_before = state.total.load(Ordering::Acquire);
    state.pause_at.store(u64::MAX, Ordering::Release);
    until(Instant::now() + Duration::from_secs(2), || {
        counter(&marker.with_extension("progress")).is_some_and(|n| n >= first.0 + 64 * 1024)
            && state.total.load(Ordering::Acquire) >= read_before + 64 * 1024
    })?;
    event(&format!(
        "PRESSURE:RESUMED:read={}:written={}",
        state.total.load(Ordering::Acquire),
        counter(&marker.with_extension("progress")).ok_or("PROGRESS_UNREADABLE")?
    ))?;
    state.pause_at.store(
        state.total.load(Ordering::Acquire) + 64 * 1024,
        Ordering::Release,
    );
    until(Instant::now() + Duration::from_secs(2), || {
        state.paused.load(Ordering::Acquire)
    })?;
    let second = stable_stall(marker, &state, raw, Instant::now() + Duration::from_secs(2))?;
    event(&format!(
        "PRESSURE:STALLED2:read={}:written={}:heartbeat={}",
        state.total.load(Ordering::Acquire),
        second.0,
        second.1
    ))?;
    unsafe { TerminateProcess(raw, 0) }.map_err(|_| "PRESSURE_OWNED_STOP")?;
    if unsafe { WaitForSingleObject(raw, 1000) } != WAIT_OBJECT_0 {
        return Err("PRESSURE_ROOT_EXIT_UNKNOWN");
    }
    // No external Job duplicate keeps last-handle semantics alive.
    drop(child.try_wait().map_err(|_| "ROOT_WAIT")?);
    drop(child);
    drop(job);
    state.pause_at.store(u64::MAX, Ordering::Release);
    drop(writer);
    drop(pair);
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "DRAIN_EOF_UNKNOWN")??;
    drain.join().map_err(|_| "DRAIN_JOIN")?;
    event("PASS:PRESSURE_STOP_ROOT_EXIT_DRAIN_EOF")
}
