use super::*;
use std::sync::mpsc;

// 旧进程退休并创建下一代后，旧访问对象不能操作任何一代资源。
#[test]
fn D11_Service_ReplacedGenerationCannotControlNext_008() {
    let f = Fixture::new(false);
    let original = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &original.run).unwrap();
    let old = f.consumer.runs.lock()[0].clone();
    old.process.pty.terminate_root().unwrap();
    old.process.pty.wait().unwrap();
    f.service.registry().mark_exited(&original.run).unwrap();
    f.service.registry().retire(&original.run).unwrap();
    let mut next = f.request.clone();
    next.request_id = "next-request".into();
    next.run_id = "next-run".into();
    next.generation += 1;
    let current = f
        .service
        .start(&f.caller, &next, |_| {
            f.routes
                .bind(2, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(()))))
        })
        .unwrap();
    assert_eq!(current.phase, LaunchPhase::Running);
    let deadline = Instant::now() + Duration::from_secs(15);
    while f.children() < 2 {
        assert!(Instant::now() < deadline, "replacement child missing");
        std::thread::sleep(Duration::from_millis(10));
    }
    for result in [
        access.with_writer::<()>(|_| panic!("retired write")),
        access.resize(size()),
        access.terminate_root(),
        access.snapshot().map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().code, "RUN_NOT_READY");
    }
    let stale = RunKey {
        run_id: current.run.run_id.clone(),
        generation: original.run.generation,
    };
    assert_eq!(
        f.service.access(&f.caller, &stale).unwrap_err().code,
        "STALE_GENERATION"
    );
    let fresh = f.service.access(&f.caller, &current.run).unwrap();
    assert_eq!(fresh.snapshot().unwrap().request(), &next);
    assert!(f.consumer.runs.lock()[1]
        .process
        .pty
        .try_wait()
        .unwrap()
        .is_none());
    assert_eq!(f.consumer.calls.load(Ordering::SeqCst), 2);
}

// 排队的写请求在获得锁后仍需检查撤权，不能调用已经失效的操作闭包。
#[test]
fn D11_Service_QueuedWriterCannotOutliveAuthority_009() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let resource = f.consumer.runs.lock()[0].clone();
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        resource.process.pty.with_writer(|_| {
            locked_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    locked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        access.with_writer::<()>(|_| panic!("revoked queued write"))
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    f.service.registry().revoke_window(&f.caller).unwrap();
    release_tx.send(()).unwrap();
    holder.join().unwrap().unwrap();
    assert_eq!(writer.join().unwrap().unwrap_err().code, "FORBIDDEN");
}

// The real PTY's resize effect must not enter an in-flight logical write frame.
#[test]
fn NativeMutation_ResizeCannotEnterActiveWrite_010() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let resource = f.consumer.runs.lock()[0].clone();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        access.with_writer(|_| {
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let pause = crate::platform::owned_pty::mutation_test::pause_next(&resource.process.pty);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let resize_resource = resource.clone();
    let resize = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        resize_resource.process.pty.resize_checked(size(), || {
            entered_tx.send(()).unwrap();
            Ok(())
        })
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    pause.entered_while_locked();
    pause.release();
    let entered_during_frame = entered_rx.recv_timeout(Duration::from_millis(250));
    // Release and join before asserting, including the behavioral RED path.
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    resize.join().unwrap().unwrap();
    assert!(
        matches!(entered_during_frame, Err(mpsc::RecvTimeoutError::Timeout)),
        "native resize entered while the same PTY's logical frame was active"
    );
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(f.children(), 1);
}

#[test]
fn NativeMutation_ExitedRetainedResourceIsDrainOnly_011() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let resource = f.consumer.runs.lock()[0].clone();
    resource.process.pty.terminate_root().unwrap();
    resource.process.pty.wait().unwrap();
    f.service.registry().mark_exited(&status.run).unwrap();
    // Keeping handles for output drain must not keep input/resize authority.
    assert!(access.snapshot().is_ok());
    let entered = AtomicUsize::new(0);
    let result = access.with_writer(|_| {
        entered.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    assert_eq!(
        entered.load(Ordering::SeqCst),
        0,
        "exited run admitted a writer"
    );
    assert_eq!(result.unwrap_err().code, "RUN_NOT_READY");
    assert_eq!(access.resize(size()).unwrap_err().code, "RUN_NOT_READY");
}

#[test]
fn NativeMutation_QueuedResizeRechecksRevocation_012() {
    for resize in [false, true] {
        queued_mutation_rejects(resize, false);
    }
}

fn queued_mutation_rejects(resize: bool, exited: bool) {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let resource = f.consumer.runs.lock()[0].clone();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder_resource = resource.clone();
    let writer = std::thread::spawn(move || {
        holder_resource.process.pty.with_writer(|_| {
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let pause = crate::platform::owned_pty::mutation_test::pause_next(&resource.process.pty);
    let mutation = std::thread::spawn(move || {
        if resize {
            access.resize(size())
        } else {
            access.with_writer::<()>(|_| panic!("stale queued write effect"))
        }
    });
    // This barrier is inside OwnedPty, after RunAccess's first check, and
    // observes the held original mutation mutex; an outer started flag cannot.
    pause.entered_while_locked();
    if exited {
        resource.process.pty.terminate_root().unwrap();
        resource.process.pty.wait().unwrap();
        f.service.registry().mark_exited(&status.run).unwrap();
    } else {
        f.service.registry().revoke_window(&f.caller).unwrap();
    }
    pause.release();
    release_tx.send(()).unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(
        mutation.join().unwrap().unwrap_err().code,
        if exited { "RUN_NOT_READY" } else { "FORBIDDEN" }
    );
}

#[test]
fn NativeMutation_UserFrameResizeAndProtocolReachOriginalPty_013() {
    use crate::terminal_input::{
        write_host_frame, InputBeginRequest, InputChunkRequest, InputCommitRequest, InputStager,
        InputWriteState,
    };
    let mut f = Fixture::new(false);
    if let LaunchAction::Raw { argv } = &mut f.request.action {
        argv[1] = "input".into();
    }
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let stager = InputStager::new();
    let wire = |n: &str| WireU64::parse(n).unwrap();
    stager
        .begin(
            &f.caller,
            &InputBeginRequest {
                run_id: status.run.run_id.clone(),
                generation: 1,
                input_seq: wire("1"),
                mode_epoch: wire("1"),
                total_bytes: wire("3"),
            },
        )
        .unwrap();
    stager
        .chunk(
            &f.caller,
            &InputChunkRequest {
                run_id: status.run.run_id.clone(),
                generation: 1,
                input_seq: wire("1"),
                offset: wire("0"),
                bytes: b"abc".to_vec(),
            },
        )
        .unwrap();
    let commit = InputCommitRequest {
        run_id: status.run.run_id.clone(),
        generation: 1,
        input_seq: wire("1"),
    };
    let receipt = stager
        .commit(&f.caller, &commit, |bytes| {
            access.with_writer(|writer| Ok(write_host_frame(writer, bytes)))
        })
        .unwrap();
    assert_eq!(receipt.state, InputWriteState::HostWritten);
    assert_eq!(receipt.confirmed_bytes, "3");
    assert_eq!(
        stager
            .commit(&f.caller, &commit, |_| panic!("duplicate input"))
            .unwrap(),
        receipt
    );
    // Do not inherit the experimental Host's narrower dimensions on desktop.
    access
        .resize(PtySize {
            rows: 128,
            cols: 256,
            ..size()
        })
        .unwrap();
    let protocol = access
        .with_writer(|writer| Ok(write_host_frame(writer, b"def")))
        .unwrap();
    assert_eq!(protocol.state, InputWriteState::HostWritten);
    assert_eq!(protocol.confirmed_bytes, 3);
    let resource = f.consumer.runs.lock()[0].clone();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let report = fs::read_dir(f.root.path().join("work"))
            .unwrap()
            .filter_map(|entry| fs::read(entry.ok()?.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .find(|value| value.get("input").is_some());
        if let Some(report) = report {
            assert_eq!(
                report["input"],
                serde_json::json!([97, 98, 99, 100, 101, 102])
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "original PTY did not consume the fixed frames"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(resource.process.pty.wait().unwrap().exit_code(), 17);
    assert_eq!(f.children(), 1);
}

#[test]
fn NativeMutation_WriteFailureAndInvalidResizePreserveErrors_014() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let failure = access
        .with_writer::<()>(|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "TEST_ONLY",
            ))
        })
        .unwrap_err();
    assert_eq!(failure.code, "PTY_WRITE_FAILED");
    assert!(access.resize(PtySize { rows: 0, ..size() }).is_err());
    access.resize(size()).unwrap();
    access.with_writer(|_| Ok(())).unwrap();
    assert_eq!(f.children(), 1);
}

#[test]
fn NativeMutation_QueuedWriteAndResizeRecheckExit_015() {
    for resize in [false, true] {
        queued_mutation_rejects(resize, true);
    }
}

#[test]
fn NativeMutation_ResizeWaitDoesNotBlockAsyncProgress_016() {
    let f = Fixture::new(false);
    let status = f.start().unwrap();
    f.ready();
    let access = f.service.access(&f.caller, &status.run).unwrap();
    let resource = f.consumer.runs.lock()[0].clone();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        resource.process.pty.with_writer(|_| {
            held_tx.send(()).unwrap();
            // Safety fallback allows a synchronous-regression RED to clean up.
            Ok(release_rx.recv_timeout(Duration::from_secs(2)).is_ok())
        })
    });
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (result, ()) = runtime.block_on(async {
        tokio::join!(biased;
            crate::cli::native_runtime::resize_native_run(access, size()),
            async { let _ = release_tx.send(()); }
        )
    });
    result.unwrap();
    assert!(
        holder.join().unwrap().unwrap(),
        "resize blocked the async executor until the safety fallback released its writer"
    );
}
