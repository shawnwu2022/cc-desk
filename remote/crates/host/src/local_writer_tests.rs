#[path = "local_writer_supervisor.rs"]
mod supervision;
use super::*;
use cc_desk_native_runtime::{BackendPipeLaunch, NativeOwnedPipeRun};
use std::{path::Path, time::Instant};
fn key() -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "TEST_ONLY_LOCAL_PIPE".into(),
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    }
}
fn fixture(marker: &Path) -> NativeOwnedPipeRun {
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("local-writer-fixture.exe");
    assert!(executable.is_file(), "current Cargo fixture must exist");
    let recipe = BackendPipeLaunch::from_backend_recipe(
        executable,
        marker.parent().unwrap().to_owned(),
        vec!["TEST_ONLY_PIPE".into(), marker.as_os_str().to_owned()],
        Vec::new(),
    )
    .unwrap();
    NativeOwnedPipeRun::launch(recipe).unwrap()
}
#[test]
fn fresh_owned_pipe_launch_phase_probe() {
    if !supervision::run("fresh_owned_pipe_launch_phase_probe") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("must-not-write");
    let mut run = fixture(&marker);
    assert_ne!(
        run.stop(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .exit_code,
        90
    );
    assert!(!marker.exists());
}
#[test]
fn actual_owned_pipe_is_committed_through_durable_single_writer() {
    if !supervision::run("actual_owned_pipe_is_committed_through_durable_single_writer") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("public-effect-count");
    let mut run = fixture(&marker);
    let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
    let stdin = run.take_stdin().expect("one-time owned stdin accessor");
    assert!(run.take_stdin().is_none());
    let mut writer = LocalWriter::start(
        journal,
        stdin,
        key().target(),
        key().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
    )
    .unwrap();
    let h = writer.handle();
    let first = h
        .submit(key(), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        first,
        WriteResult::Receipt(ReceiptEnvelope {
            outcome: Outcome::HostWritten,
            receipt_proof: Some(_),
            ..
        })
    ));
    effects(&marker, 1);
    // 多个本地 handle 也只能经同一个 pipe owner/journal；ACK 丢失不触发重写。
    let lost = h
        .submit(next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
        .unwrap();
    drop(lost);
    effects(&marker, 2);
    let duplicate = h
        .clone()
        .submit(next(2), 9, b"PUBLIC_TEST_ONLY\n".to_vec())
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        duplicate,
        WriteResult::Receipt(ReceiptEnvelope {
            outcome: Outcome::HostWritten,
            ..
        })
    ));
    assert_eq!(
        h.submit(next(2), 9, b"DIFFERENT_PUBLIC_BYTES\n".to_vec())
            .unwrap()
            .wait(Duration::from_secs(2))
            .unwrap(),
        WriteResult::Journal(JournalError::Conflict)
    );
    assert_eq!(std::fs::read(&marker).unwrap().len(), 2);
    writer.seal();
    joined(&mut writer);
    let exit = run.stop(Instant::now() + Duration::from_secs(2)).unwrap();
    assert_ne!(exit.exit_code, 90);
}
fn next(sequence: u64) -> OperationKey {
    let mut k = key();
    k.sequence = sequence;
    k.operation = [sequence as u8; 16];
    k
}
fn effects(path: &Path, count: usize) {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if std::fs::read(path).map(|b| b.len()).unwrap_or(0) == count {
            return;
        }
        assert!(Instant::now() < end, "fixed fixture effect count");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn joined(writer: &mut LocalWriter) {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if writer.try_join().unwrap() {
            return;
        }
        assert!(
            Instant::now() < end,
            "keep writer owner; joining not confirmed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn unit_compacts_envelopes_before_queue() {
    // 小容量反例即可证明 move 不得保留调用者超预算 backing allocation。
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let source = LocalInputHandle {
        tx,
        target: Arc::new(key().target()),
        device: key().device,
        epoch: 9,
        sealed: Arc::new(AtomicBool::new(false)),
        credits: Arc::new(Credits::default()),
    };
    let mut payload = Vec::with_capacity(MAX_INPUT * 2);
    payload.extend_from_slice(b"x\n");
    let old_payload = payload.as_ptr();
    let mut request = key();
    let mut oversized = String::with_capacity(256);
    oversized.push_str(&request.run);
    request.run = oversized;
    let old_run = request.run.as_ptr();
    let _pending = source.submit(request, 9, payload).unwrap();
    let compact = rx.try_recv().unwrap();
    assert_ne!(
        compact.bytes.as_ptr(),
        old_payload,
        "must copy bounded payload instead of retaining oversized capacity"
    );
    assert_ne!(
        compact.key.run.as_ptr(),
        old_run,
        "must compact variable metadata before enqueue"
    );
}
#[test]
fn unit_join_panic_is_a_sticky_terminal_result() {
    let (tx, _rx) = mpsc::sync_channel(QUEUE);
    let handle = LocalInputHandle {
        tx,
        target: Arc::new(key().target()),
        device: key().device,
        epoch: 9,
        sealed: Arc::new(AtomicBool::new(false)),
        credits: Arc::new(Credits::default()),
    };
    let mut owner = LocalWriter {
        handle,
        worker: Some(std::thread::spawn(|| panic!("TEST_ONLY_JOIN_CLASSIFIER"))),
        terminal: None,
    };
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match owner.try_join() {
            Err(Reject::Thread) => break,
            Ok(false) => {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(owner.try_join(), Err(Reject::Thread));
}

#[test]
fn unit_concurrent_full_submissions_never_allocate_before_capacity_admission() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Barrier, Condvar, Mutex};
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let source = LocalInputHandle {
        tx,
        target: Arc::new(key().target()),
        device: key().device,
        epoch: 9,
        sealed: Arc::new(AtomicBool::new(false)),
        credits: Arc::new(Credits::default()),
    };
    let first = source.submit(key(), 9, vec![b'x'; MAX_INPUT]).unwrap();
    let current = rx.try_recv().unwrap();
    let mut feedback = Vec::new();
    for sequence in 2..=9 {
        feedback.push(
            source
                .submit(next(sequence), 9, vec![b'x'; MAX_INPUT])
                .unwrap(),
        );
    }
    let start = Arc::new(Barrier::new(21));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let copies = Arc::new(AtomicUsize::new(0));
    let (done, done_rx) = mpsc::sync_channel(20);
    let mut threads = Vec::new();
    for i in 0..20 {
        let source = source.clone();
        let start = start.clone();
        let release = release.clone();
        let copies = copies.clone();
        let done = done.clone();
        threads.push(std::thread::spawn(move || {
            start.wait();
            let result = source.submit_observed(next(20 + i), 9, vec![b'x'; MAX_INPUT], || {
                copies.fetch_add(1, Ordering::SeqCst);
                let (lock, cv) = &*release;
                let guard = lock.lock().unwrap();
                drop(cv.wait_while(guard, |v| !*v).unwrap());
            });
            let _ = done.send(matches!(result, Err(Reject::Full)));
        }));
    }
    start.wait();
    let end = Instant::now() + Duration::from_secs(1);
    let mut returned = 0;
    while returned < 20 {
        if done_rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .ok()
            == Some(true)
        {
            returned += 1;
        } else {
            break;
        }
    }
    let observed = copies.load(Ordering::SeqCst);
    {
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(source.credits.used.load(Ordering::Acquire), MAX_CREDITS);
    drop((current, first, feedback, rx));
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
    assert_eq!(
        observed, 0,
        "full current+8 queues must reject all compact-copy/feedback allocations before admission"
    );
    assert_eq!(returned, 20);
}

fn unit_source() -> (LocalInputHandle, mpsc::Receiver<Request>) {
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    (
        LocalInputHandle {
            tx,
            target: Arc::new(key().target()),
            device: key().device,
            epoch: 9,
            sealed: Arc::new(AtomicBool::new(false)),
            credits: Arc::new(Credits::default()),
        },
        rx,
    )
}
#[test]
fn unit_credit_covers_unread_and_consumed_but_retained_feedback() {
    let (source, rx) = unit_source();
    let mut pending = Vec::new();
    for sequence in 1..=9 {
        let p = source
            .submit(next(sequence), 9, vec![b'x'; MAX_INPUT])
            .unwrap();
        let request = rx.try_recv().unwrap();
        request
            .reply
            .try_send(WriteResult::Rejected(Reject::Unsupported))
            .unwrap();
        drop(request);
        pending.push(p);
    }
    assert_eq!(source.credits.used.load(Ordering::Acquire), MAX_CREDITS);
    assert!(matches!(
        source.submit_observed(next(10), 9, vec![b'x'; MAX_INPUT], || panic!(
            "no allocation before Full"
        )),
        Err(Reject::Full)
    ));
    for p in &pending {
        assert_eq!(
            p.wait(Duration::from_millis(10)),
            Ok(WriteResult::Rejected(Reject::Unsupported))
        );
    }
    assert_eq!(
        source.credits.used.load(Ordering::Acquire),
        MAX_CREDITS,
        "consumption is not ownership release"
    );
    drop(pending);
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
    let p = source.submit(key(), 9, b"x\n".to_vec()).unwrap();
    let request = rx.try_recv().unwrap();
    drop(p);
    assert_eq!(
        source.credits.used.load(Ordering::Acquire),
        1,
        "request must retain credit after receiver lost"
    );
    assert!(request.reply.try_send(WriteResult::Unknown).is_err());
    drop(request);
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
}
#[test]
fn unit_credit_releases_on_full_closed_seal_and_unwind() {
    let (source, rx) = unit_source();
    let mut pending = Vec::new();
    for sequence in 1..=8 {
        pending.push(
            source
                .submit(next(sequence), 9, vec![b'x'; MAX_INPUT])
                .unwrap(),
        );
    }
    assert_eq!(source.credits.used.load(Ordering::Acquire), QUEUE);
    assert!(matches!(
        source.submit(next(9), 9, vec![b'x'; MAX_INPUT]),
        Err(Reject::Full)
    ));
    assert_eq!(
        source.credits.used.load(Ordering::Acquire),
        QUEUE,
        "failed try_send releases its pre-send credit"
    );
    source.sealed.store(true, Ordering::Release);
    reject_queued(&rx);
    assert_eq!(
        source.credits.used.load(Ordering::Acquire),
        QUEUE,
        "feedback still owns drained credits"
    );
    for p in pending {
        assert_eq!(
            p.wait(Duration::from_millis(10)),
            Ok(WriteResult::Rejected(Reject::Sealed))
        );
        drop(p);
    }
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
    assert!(matches!(
        source.submit(key(), 9, b"x\n".to_vec()),
        Err(Reject::Sealed)
    ));
    let (source, rx) = unit_source();
    drop(rx);
    assert!(matches!(
        source.submit(key(), 9, b"x\n".to_vec()),
        Err(Reject::Closed)
    ));
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
    let (source, rx) = unit_source();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = source.submit_observed(key(), 9, b"x\n".to_vec(), || {
            panic!("TEST_ONLY_PRE_SEND_UNWIND")
        });
    }));
    assert!(panic.is_err());
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
    assert!(rx.try_recv().is_err());
    let p = source.submit(key(), 9, b"x\n".to_vec()).unwrap();
    let worker = std::thread::spawn(move || {
        let _request = rx.recv().unwrap();
        panic!("TEST_ONLY_REQUEST_OWNER_UNWIND");
    });
    assert!(worker.join().is_err());
    assert_eq!(p.wait(Duration::from_millis(10)), Err(WaitError::Closed));
    assert_eq!(source.credits.used.load(Ordering::Acquire), 1);
    drop(p);
    assert_eq!(source.credits.used.load(Ordering::Acquire), 0);
}
#[test]
fn actual_pipe_guard_bounds_and_queue_are_fail_closed() {
    if !supervision::run("actual_pipe_guard_bounds_and_queue_are_fail_closed") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("effects");
    let path = dir.path().join("journal.sqlite");
    let mut run = fixture(&marker);
    let (tx, rx) = mpsc::sync_channel(1);
    let (resume, wait) = mpsc::sync_channel(1);
    let mut target = key().target();
    let mut backing = String::with_capacity(256);
    backing.push_str(&target.run);
    target.run = backing;
    let old_target = target.run.as_ptr();
    let mut writer = LocalWriter::start_with(
        Journal::open(&path).unwrap(),
        run.take_stdin().unwrap(),
        target,
        key().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        Hooks {
            after_write: Some(Box::new(move || {
                tx.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
            })),
        },
    )
    .unwrap();
    assert_ne!(
        writer.handle.target.run.as_ptr(),
        old_target,
        "fixed backend target must not retain oversized backing allocation"
    );
    let h = writer.handle();
    for kind in 0..8 {
        let mut bad = key();
        let mut epoch = 9;
        let reason = match kind {
            0 => {
                bad.host = [0; 32];
                Reject::Target
            }
            1 => {
                bad.session = [0; 16];
                Reject::Target
            }
            2 => {
                bad.generation += 1;
                Reject::Target
            }
            3 => {
                bad.incarnation += 1;
                Reject::Target
            }
            4 => {
                bad.run = "OTHER".into();
                Reject::Target
            }
            5 => {
                bad.device = [0; 32];
                Reject::Device
            }
            6 => {
                epoch = 8;
                Reject::Epoch
            }
            _ => {
                bad.run = "X".repeat(129);
                Reject::Bounds
            }
        };
        assert!(matches!(h.submit(bad,epoch,b"x\n".to_vec()),Err(e)if e==reason));
    }
    assert!(matches!(
        h.submit(key(), 9, vec![b'x'; MAX_INPUT + 1]),
        Err(Reject::Bounds)
    ));
    assert!(matches!(
        h.submit(key(), 9, Vec::new()),
        Err(Reject::Bounds)
    ));
    assert_eq!(
        h.resize(&key().target(), 9, 80, 24),
        Err(Reject::Unsupported)
    );
    assert_eq!(
        h.terminal_reply(&key().target(), 9, b"\x1b[1;1R"),
        Err(Reject::Unsupported)
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM floors", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    drop(db);
    assert!(!marker.exists());
    let mut max = vec![b'x'; MAX_INPUT];
    *max.last_mut().unwrap() = b'\n';
    let first = h.submit(key(), 9, max).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    effects(&marker, 1);
    let mut queued = Vec::new();
    for sequence in 2..=9 {
        queued.push(h.submit(next(sequence), 9, b"x\n".to_vec()).unwrap());
    }
    assert!(matches!(
        h.submit(next(10), 9, b"x\n".to_vec()),
        Err(Reject::Full)
    ));
    // 在途已写不能撤销；排队输入在 seal 后不能获得 effect。
    writer.seal();
    resume.send(()).unwrap();
    assert!(matches!(
        first.wait(Duration::from_secs(2)).unwrap(),
        WriteResult::Receipt(ReceiptEnvelope {
            outcome: Outcome::HostWritten,
            ..
        })
    ));
    for p in queued {
        assert_eq!(
            p.wait(Duration::from_secs(2)).unwrap(),
            WriteResult::Rejected(Reject::Sealed)
        );
    }
    joined(&mut writer);
    assert_eq!(std::fs::read(&marker).unwrap().len(), 1);
    assert!(matches!(
        h.submit(next(2), 9, b"x\n".to_vec()),
        Err(Reject::Sealed)
    ));
    assert_ne!(
        run.stop(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .exit_code,
        90
    );
}

#[test]
fn actual_writer_panic_retains_terminal_join_error_and_unknown_intent() {
    if !supervision::run("actual_writer_panic_retains_terminal_join_error_and_unknown_intent") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("effects");
    let path = dir.path().join("journal.sqlite");
    let mut run = fixture(&marker);
    let mut writer = LocalWriter::start_with(
        Journal::open(&path).unwrap(),
        run.take_stdin().unwrap(),
        key().target(),
        key().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        Hooks {
            after_write: Some(Box::new(|| panic!("TEST_ONLY_AFTER_REAL_PIPE_WRITE"))),
        },
    )
    .unwrap();
    let pending = writer
        .handle()
        .submit(key(), 9, b"PUBLIC_EFFECT\n".to_vec())
        .unwrap();
    assert_eq!(pending.wait(Duration::from_secs(2)), Err(WaitError::Closed));
    effects(&marker, 1);
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match writer.try_join() {
            Err(Reject::Thread) => break,
            Ok(false) => {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(1));
            }
            other => panic!("unexpected terminal state {other:?}"),
        }
    }
    assert_eq!(
        writer.try_join(),
        Err(Reject::Thread),
        "observed panic must never become normal join on reentry"
    );
    assert_ne!(
        run.stop(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .exit_code,
        90
    );
    let journal = Journal::open(&path).unwrap();
    assert!(matches!(
        journal.query(&key()).unwrap(),
        ReceiptLookup::Known(ReceiptEnvelope {
            outcome: Outcome::Unknown,
            receipt_proof: None,
            ..
        })
    ));
}

#[test]
fn actual_pipe_effect_then_storage_failure_reopens_only_as_unknown() {
    if !supervision::run("actual_pipe_effect_then_storage_failure_reopens_only_as_unknown") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("effects");
    let path = dir.path().join("journal.sqlite");
    let mut run = fixture(&marker);
    let (tx, rx) = mpsc::sync_channel(1);
    let (resume, wait) = mpsc::sync_channel(1);
    let mut writer = LocalWriter::start_with(
        Journal::open(&path).unwrap(),
        run.take_stdin().unwrap(),
        key().target(),
        key().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
        Hooks {
            after_write: Some(Box::new(move || {
                tx.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
            })),
        },
    )
    .unwrap();
    let h = writer.handle();
    let pending = h.submit(key(), 9, b"PUBLIC_EFFECT\n".to_vec()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    effects(&marker, 1);
    // 精确 TEST_ONLY 故障闸门：真实 pipe effect 后、原 journal 最终提交前。
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_result BEFORE UPDATE ON operations BEGIN SELECT RAISE(ABORT,'TEST_ONLY_RESULT_FAILURE'); END;").unwrap();
    drop(db);
    resume.send(()).unwrap();
    assert_eq!(
        pending.wait(Duration::from_secs(2)).unwrap(),
        WriteResult::Unknown
    );
    joined(&mut writer);
    assert!(matches!(
        h.submit(next(2), 9, b"DO_NOT_REPLAY\n".to_vec()),
        Err(Reject::Sealed)
    ));
    assert_ne!(
        run.stop(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .exit_code,
        90
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER fail_result").unwrap();
    drop(db);
    let journal = Journal::open(&path).unwrap();
    assert!(matches!(
        journal.query(&key()).unwrap(),
        ReceiptLookup::Known(ReceiptEnvelope {
            outcome: Outcome::Unknown,
            receipt_proof: None,
            ..
        })
    ));
    assert_eq!(
        std::fs::read(&marker).unwrap().len(),
        1,
        "reopen query never launches/replays IO"
    );
}
