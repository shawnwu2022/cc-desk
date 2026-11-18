use super::*;
use std::{io, time::Instant};
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn target(session: u8) -> Target {
    Target {
        host: [1; 32],
        session: [session; 16],
        run: "PUBLIC_TEST_ONLY_OWNER".into(),
        generation: 1,
        incarnation: 4,
    }
}
fn config(t: Target) -> SessionStart {
    SessionStart {
        target: t,
        device: [5; 32],
        writer_epoch: 9,
        key: DigestKey::from_backend_bytes([0x5a; 32]),
        rows: 3,
        columns: 10,
    }
}
fn start(
    owner: &mut WriterHostOwner,
    dir: &tempfile::TempDir,
    t: Target,
    sink: impl io::Write + Send + 'static,
) -> Result<StartedWriter, OwnerError> {
    owner.start(
        Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
        sink,
        config(t),
    )
}
fn reap(owner: &mut WriterHostOwner, t: &Target) {
    owner.seal(t).unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while !owner.try_reap(t).unwrap() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn fresh_owner(registry: &OwnerRegistry) -> WriterHostOwner {
    WriterHostOwner::from_registry(registry, [1; 32], 4).unwrap()
}
fn closed(registry: &OwnerRegistry) {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match WriterHostOwner::from_registry(registry, [1; 32], 4) {
            Err(OwnerError::Closed) => return,
            Err(OwnerError::Busy) => {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("{e:?}"),
            Ok(_) => panic!("owner reconstruction reset process-lifetime admission"),
        }
    }
}
#[test]
fn unique_owner_and_held_frames_cover_all_session_factories() {
    let _serial = SERIAL.lock().unwrap();
    let registry = OwnerRegistry::default();
    let mut owner = fresh_owner(&registry);
    assert!(matches!(
        WriterHostOwner::from_registry(&registry, [9; 32], 9),
        Err(OwnerError::Busy)
    ));
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let _one = start(&mut owner, &a, target(1), io::sink()).unwrap();
    let _two = start(&mut owner, &b, target(2), io::sink()).unwrap();
    let one = owner
        .capture(&target(1))
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let two = owner
        .capture(&target(2))
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(matches!(
        owner.capture(&target(1)),
        Err(OwnerError::Writer(Reject::Full))
    ));
    reap(&mut owner, &target(1));
    reap(&mut owner, &target(2));
    drop(owner);
    assert!(matches!(
        WriterHostOwner::from_registry(&registry, [1; 32], 4),
        Err(OwnerError::Busy)
    ));
    drop(one);
    drop(two);
    closed(&registry);
}
#[test]
fn owner_refuses_cross_host_stale_incarnation_duplicate_and_retired_session_ids() {
    let _serial = SERIAL.lock().unwrap();
    let registry = OwnerRegistry::default();
    let mut owner = fresh_owner(&registry);
    let a = tempfile::tempdir().unwrap();
    let started = start(&mut owner, &a, target(1), io::sink()).unwrap();
    for case in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let mut t = target(1);
        if case == 0 {
            t.host = [2; 32];
        }
        if case == 1 {
            t.incarnation = 5;
        }
        assert!(matches!(
            start(&mut owner, &dir, t, io::sink()),
            Err(OwnerError::Target)
        ));
    }
    reap(&mut owner, &target(1));
    let dir = tempfile::tempdir().unwrap();
    let mut new = target(1);
    new.generation = 2;
    assert!(matches!(
        start(&mut owner, &dir, new, io::sink()),
        Err(OwnerError::Retired)
    ));
    assert!(matches!(
        started.input.submit(
            OperationKey {
                host: [1; 32],
                session: [1; 16],
                run: target(1).run,
                generation: 1,
                incarnation: 4,
                device: [5; 32],
                operation: [1; 16],
                sequence: 1
            },
            9,
            b"late".to_vec()
        ),
        Err(Reject::Sealed)
    ));
    drop(owner);
    closed(&registry);
}
#[test]
fn dropping_owner_cannot_reacquire_while_an_actual_write_is_still_in_flight() {
    let _serial = SERIAL.lock().unwrap();
    struct Block {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl io::Write for Block {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| io::Error::other("TEST_ONLY_TIMEOUT"))?;
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let registry = OwnerRegistry::default();
    let mut owner = fresh_owner(&registry);
    let dir = tempfile::tempdir().unwrap();
    let (tx, entered) = mpsc::sync_channel(1);
    let (release, rx) = mpsc::sync_channel(1);
    let mut started = start(
        &mut owner,
        &dir,
        target(1),
        Block {
            entered: tx,
            release: rx,
        },
    )
    .unwrap();
    let pending = started.output.feed_at(0, b"\x1b[c".to_vec()).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    owner.seal(&target(1)).unwrap();
    assert!(!owner.try_reap(&target(1)).unwrap());
    drop(owner);
    assert!(matches!(
        WriterHostOwner::from_registry(&registry, [1; 32], 4),
        Err(OwnerError::Busy)
    ));
    release.send(()).unwrap();
    pending.wait(Duration::from_secs(2)).unwrap();
    closed(&registry);
}
#[test]
fn owner_enforces_active_and_lifetime_session_caps_without_eviction() {
    let _serial = SERIAL.lock().unwrap();
    let registry = OwnerRegistry::default();
    let mut owner = fresh_owner(&registry);
    let mut dirs = Vec::new();
    for n in 0..12 {
        let dir = tempfile::tempdir().unwrap();
        start(&mut owner, &dir, target(n), io::sink()).unwrap();
        dirs.push(dir);
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        start(&mut owner, &dir, target(12), io::sink()),
        Err(OwnerError::Capacity)
    ));
    for n in 0..12 {
        reap(&mut owner, &target(n));
    }
    // Fixed bounded state seam; no4096 threads or databases are created by this case.
    owner.retired.resize(MAX_LIFETIME_SESSIONS, [99; 16]);
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        start(&mut owner, &dir, target(13), io::sink()),
        Err(OwnerError::Capacity)
    ));
    drop(owner);
    closed(&registry);
}
#[test]
fn owner_guarded_factory_uses_its_shared_budget_and_does_not_return_a_raw_input_bypass() {
    let _serial = SERIAL.lock().unwrap();
    let registry = OwnerRegistry::default();
    let mut owner = fresh_owner(&registry);
    let dir = tempfile::tempdir().unwrap();
    let started = owner
        .start_guarded(
            Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
            io::sink(),
            config(target(1)),
            [0x33; 32],
            1,
        )
        .unwrap();
    let key = OperationKey {
        host: [1; 32],
        session: [1; 16],
        run: target(1).run,
        generation: 1,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 1,
    };
    assert!(matches!(
        started.input.submit(key, 9, b"x".to_vec()),
        Err(Reject::Sync)
    ));
    let peer = started.peer.unwrap();
    let a = peer
        .capture()
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let b = owner
        .capture(&target(1))
        .unwrap()
        .wait(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(a.issued_stamp().is_ok());
    assert!(matches!(peer.capture(), Err(Reject::Full)));
    reap(&mut owner, &target(1));
    assert!(matches!(peer.capture(), Err(Reject::Sync)));
    drop(a);
    drop(b);
    // The closed producer retains its capture pool until the fixture drops it.
    drop(peer);
    drop(owner);
    closed(&registry);
}
