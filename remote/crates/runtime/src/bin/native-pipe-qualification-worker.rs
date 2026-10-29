//! TEST_ONLY 外层监督 worker；在任何 Native launch 前已由测试 parent 持有精确 Child。
#[cfg(all(windows, feature = "conpty-qualification"))]
fn main() {
    use cc_desk_native_runtime::*;
    use std::{
        ffi::OsString,
        io::{Read, Write},
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 || args[1] != "TEST_ONLY_D_WORKER" {
        std::process::exit(64);
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(8));
        std::process::exit(91);
    });
    let mut go = [0u8; 1];
    std::io::stdin().read_exact(&mut go).unwrap();
    assert_eq!(go, *b"G");
    eprintln!("D_WORKER_PHASE=go_received");
    let marker = std::path::PathBuf::from(&args[3]).join("public-entry");
    if args[4] == "ASSIGN_REJECT" {
        use std::os::windows::io::AsHandle;
        let not_job =
            std::fs::File::create(std::path::PathBuf::from(&args[3]).join("not-a-job")).unwrap();
        let mut command = portable_pty::CommandBuilder::new(args[2].clone());
        command.env_clear();
        command.cwd(args[3].clone());
        command.arg("TEST_ONLY_SLEEP");
        command.arg(marker.clone());
        let error = match portable_pty::spawn_pipe_in_job(command, not_job.as_handle()) {
            Err(error) => error,
            Ok(_) => panic!("non-Job assignment unexpectedly succeeded"),
        };
        assert!(matches!(
            error,
            NativePipeError::KnownNotStarted {
                phase: NativePipePhase::AssignmentReaped
            }
        ));
        assert!(!marker.exists());
        eprintln!("D_WORKER_PHASE=assignment_reaped_no_entry");
        return;
    }
    if args[4] == "STOP" {
        let recipe = BackendPipeLaunch::from_backend_recipe(
            args[2].clone().into(),
            args[3].clone().into(),
            vec!["TEST_ONLY_SLEEP".into(), marker.clone().into_os_string()],
            Vec::new(),
        )
        .unwrap();
        let mut run = NativeOwnedPipeRun::launch(recipe).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while !marker.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(Instant::now() < end && marker.exists());
        assert!(run.try_wait_root().unwrap().is_none());
        let code = run.stop(end).unwrap().exit_code;
        assert_ne!(code, 90);
        drop(run.take_stdin());
        let mut out = Vec::new();
        run.take_stdout()
            .unwrap()
            .take(4097)
            .read_to_end(&mut out)
            .unwrap();
        assert!(out.is_empty());
        run.take_stderr()
            .unwrap()
            .take(4097)
            .read_to_end(&mut out)
            .unwrap();
        assert!(out.is_empty());
        assert!(Instant::now() < end);
        eprintln!("D_WORKER_PHASE=stop_root_confirmed_streams_eof");
        return;
    }
    assert!(args[4] == "ECHO" || args[4] == "CANARY" || args[4] == "CANARY_REUSED");
    let mut echo_args = vec![
        "TEST_ONLY_ECHO".into(),
        marker.clone().into_os_string(),
        OsString::new(),
        "中文🚀".into(),
        "space quote\" trailing\\".into(),
    ];
    let canary = if args[4] == "CANARY" || args[4] == "CANARY_REUSED" {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        let attributes = windows::Win32::Security::SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<windows::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        let raw = unsafe {
            windows::Win32::System::Threading::CreateEventW(Some(&attributes), true, false, None)
        }
        .unwrap();
        let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        // Qualify the oracle against the exact retained parent event, then reset
        // before launch. A child signaling this object must be observable.
        unsafe {
            windows::Win32::System::Threading::SetEvent(raw).unwrap();
            assert_eq!(
                windows::Win32::System::Threading::WaitForSingleObject(raw, 0),
                windows::Win32::Foundation::WAIT_OBJECT_0
            );
            windows::Win32::System::Threading::ResetEvent(raw).unwrap();
            assert_eq!(
                windows::Win32::System::Threading::WaitForSingleObject(raw, 0),
                windows::Win32::Foundation::WAIT_TIMEOUT
            );
        }
        echo_args.push((handle.as_raw_handle() as usize).to_string().into());
        if args[4] == "CANARY_REUSED" {
            echo_args.push("TEST_ONLY_UNRELATED_EVENT".into());
        }
        Some(handle)
    } else {
        None
    };
    let recipe = BackendPipeLaunch::from_backend_recipe(
        args[2].clone().into(),
        args[3].clone().into(),
        echo_args,
        Vec::new(),
    )
    .unwrap();
    eprintln!("D_WORKER_PHASE=launch_begin");
    let mut run = NativeOwnedPipeRun::launch(recipe).unwrap();
    eprintln!("D_WORKER_PHASE=launch_returned");
    let mut input = run.take_stdin().unwrap();
    assert!(run.take_stdin().is_none());
    eprintln!("D_WORKER_PHASE=write_begin");
    input.write_all(b"PUBLIC_TEST_ONLY_INPUT").unwrap();
    drop(input);
    eprintln!("D_WORKER_PHASE=write_returned");
    let stdout = run.take_stdout().unwrap();
    let stderr = run.take_stderr().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    let tx2 = tx.clone();
    let a = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.take(4097).read_to_end(&mut b).unwrap();
        let _ = tx.send((0, b));
    });
    let b = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.take(4097).read_to_end(&mut b).unwrap();
        let _ = tx2.send((1, b));
    });
    let end = Instant::now() + Duration::from_secs(2);
    let mut output = [Vec::new(), Vec::new()];
    for _ in 0..2 {
        let (i, b) = rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .unwrap();
        output[i] = b;
    }
    a.join().unwrap();
    b.join().unwrap();
    assert_eq!(
        output[0],
        b"PUBLIC_TEST_ONLY_INPUT",
        "fixture_canary_assertion_failed={}",
        String::from_utf8_lossy(&output[1]).contains("unrelated inheritable event handle leaked")
    );
    assert_eq!(output[1], b"PUBLIC_TEST_ONLY_STDERR");
    assert_eq!(std::fs::read(marker).unwrap(), b"executed");
    assert_eq!(
        run.wait_root_until(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .exit_code,
        0
    );
    if let Some(canary) = canary {
        use std::os::windows::io::AsRawHandle;
        assert_eq!(
            unsafe {
                windows::Win32::System::Threading::WaitForSingleObject(
                    windows::Win32::Foundation::HANDLE(canary.as_raw_handle()),
                    0,
                )
            },
            windows::Win32::Foundation::WAIT_TIMEOUT,
            "the excluded parent event was signaled by the child"
        );
    }
    eprintln!("D_WORKER_PHASE=complete");
}
#[cfg(not(all(windows, feature = "conpty-qualification")))]
fn main() {
    std::process::exit(64);
}
