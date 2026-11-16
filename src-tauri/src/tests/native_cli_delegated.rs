//! 原 Node/PTY/producer 的委托效果回归；不连接 Relay 或真实身份。
#![allow(non_snake_case)]
use super::*;
use crate::remote_unattended::capability::CloseObservation;
use crate::run_supervisor::delegated::{DelegatedRunOwner, NativeDelegationConsent};

fn setup() -> (Fixture, Cleanup) {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let status = fixture.start();
    let cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: Arc::new(RemoteHostService::default()),
        run: status.run.clone(),
        pin: Some(fixture.supervisor.pin_shutdown(&status.run)),
        stop: None,
        proofs: Vec::new(),
    };
    let end = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    (fixture, cleanup)
}
fn admit(f: &Fixture) -> crate::run_supervisor::NativeRunAdmission {
    f.supervisor
        .admit_remote_run(
            f.service.access(&f.caller, &f.request_run()).unwrap(),
            &f.request_run(),
        )
        .unwrap()
}
fn report(f: &Fixture) -> Value {
    f.wait_lifecycle(|s| s.process() == ProcessLifecycle::Exited);
    let path = fs::read_dir(f.root.path().join("work"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.path().extension().is_some_and(|e| e == "json"))
        .unwrap()
        .path();
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
#[test]
fn DelegatedNative_DocumentRetirementPreservesOnlyCapturedEffect_001() {
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let legacy = admit(&f);
    let old_access = f.service.access(&f.caller, &f.request_run()).unwrap();
    let (consent, current, _) = NativeDelegationConsent::test_new(&source, false);
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let ticket = owner.admit(&current, &f.supervisor).unwrap();
    f.service.registry().activate_window("main").unwrap();
    assert!(legacy.scope().is_err());
    assert!(old_access.write_frame(b"BADBAD").is_err());
    ticket.resize(&current, 31, 91).unwrap();
    let result = ticket.write_frame(&current, b"native").unwrap();
    assert!(result.complete);
    assert_eq!(result.confirmed_bytes, 6);
    let actual = report(&f);
    assert_eq!(actual["input"], json!([110, 97, 116, 105, 118, 101]));
    assert_eq!(actual["rows"], 31);
    assert_eq!(actual["columns"], 91);
    assert!(ticket.write_frame(&current, b"late").is_err());
    drop(ticket);
    drop(owner);
    drop(legacy);
    drop(old_access);
    assert!(cleanup.close());
}
#[test]
fn DelegatedNative_StaleCaptureAndForeignSupervisorReject_002() {
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, _, _) = NativeDelegationConsent::test_new(&source, false);
    f.service.registry().activate_window("main").unwrap();
    assert!(DelegatedRunOwner::capture(source, consent).is_err());
    assert!(cleanup.close());
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_new(&source, false);
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let foreign = NativeRunSupervisor::new(Arc::new(TerminalTransports::new()));
    assert!(owner.admit(&current, &foreign).is_err());
    drop(owner);
    assert!(cleanup.close());
}
#[test]
fn DelegatedNative_ViewTrustExpiryAndRevokeBlockOriginalInput_003() {
    for mode in 0..6 {
        let (f, mut cleanup) = setup();
        let source = admit(&f);
        let (consent, current, policy) = NativeDelegationConsent::test_new(&source, mode == 0);
        let owner = DelegatedRunOwner::capture(source, consent).unwrap();
        let ticket = owner.admit(&current, &f.supervisor).unwrap();
        let current = if mode == 1 {
            current.test_wrong_device()
        } else {
            current
        };
        if mode == 2 {
            policy.test_expire();
        }
        if mode == 3 {
            owner.request_revoke();
        }
        if mode >= 4 {
            policy.test_clock(mode == 5);
        }
        assert!(ticket.write_frame(&current, b"BADBAD").is_err());
        if mode >= 4 {
            policy.test_restore_clock();
        }
        assert!(ticket.resize(&current, 33, 93).is_err());
        // 原本地授权仍能写入；真实子进程必须只收到这六个字节。
        f.service
            .access(&f.caller, &f.request_run())
            .unwrap()
            .write_frame(b"native")
            .unwrap();
        let actual = report(&f);
        assert_eq!(actual["input"], json!([110, 97, 116, 105, 118, 101]));
        assert_eq!(actual["rows"], 24);
        assert_eq!(actual["columns"], 80);
        drop(ticket);
        drop(owner);
        drop(policy);
        assert!(cleanup.close());
    }
}
#[test]
fn DelegatedNative_CloseDuringActorBoundaryIsNonblocking_004() {
    for expire in [false, true] {
        let (f, mut cleanup) = setup();
        let source = admit(&f);
        let (consent, current, policy) = NativeDelegationConsent::test_new(&source, false);
        let owner = DelegatedRunOwner::capture(source, consent).unwrap();
        let ticket = owner.admit(&current, &f.supervisor).unwrap();
        let (arrived, seen) = std::sync::mpsc::sync_channel(0);
        let (release, resume) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            ticket.test_write_paused(&current, b"BADBAD", move || {
                arrived.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(10)).unwrap();
            })
        });
        seen.recv_timeout(Duration::from_secs(10)).unwrap();
        if expire {
            policy.test_expire();
        } else {
            owner.request_revoke();
        }
        assert_eq!(owner.observe_close(), CloseObservation::Retiring);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(owner.observe_close(), CloseObservation::EffectsClosed);
        f.service
            .access(&f.caller, &f.request_run())
            .unwrap()
            .write_frame(b"native")
            .unwrap();
        assert_eq!(report(&f)["input"], json!([110, 97, 116, 105, 118, 101]));
        drop(owner);
        drop(policy);
        assert!(cleanup.close());
    }
}
#[test]
fn DelegatedNative_StopRejectsOriginalEffect_005() {
    let (f, mut cleanup) = setup();
    let source = admit(&f);
    let (consent, current, _) = NativeDelegationConsent::test_new(&source, false);
    let owner = DelegatedRunOwner::capture(source, consent).unwrap();
    let ticket = owner.admit(&current, &f.supervisor).unwrap();
    cleanup.stop = Some(
        f.supervisor
            .begin_shutdown_owned(vec![cleanup.pin.take().unwrap()])
            .unwrap(),
    );
    assert!(ticket.write_frame(&current, b"BADBAD").is_err());
    assert!(ticket.resize(&current, 33, 93).is_err());
    drop(ticket);
    drop(owner);
    assert!(cleanup.close());
}

#[test]
fn DelegatedNative_ForeignResourceConsentCannotCapture_006() {
    let (first, mut cleanup_first) = setup();
    let (second, mut cleanup_second) = setup();
    let first_admission = admit(&first);
    let second_admission = admit(&second);
    let (consent, _, _) = NativeDelegationConsent::test_new(&first_admission, false);
    assert!(DelegatedRunOwner::capture(second_admission, consent).is_err());
    drop(first_admission);
    assert!(cleanup_first.close());
    assert!(cleanup_second.close());
}
