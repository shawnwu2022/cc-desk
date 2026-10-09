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
