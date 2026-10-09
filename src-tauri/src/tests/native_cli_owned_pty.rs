use super::native_cli_routed_launch::{Fixture, Lease};
use crate::cli::invocation::build_invocation;
use crate::cli::profiles::Override;
use crate::cli::routed_launch::RoutedResource;
use crate::cli::run_registry::{LaunchPhase, LaunchStatus, RunKey};
use crate::cli::types::LaunchAction;
use crate::platform::launch::resolve_process;
use crate::platform::owned_pty::OwnedPty;
use portable_pty::{native_pty_system, PtySize};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

struct Probe(Fixture<OwnedPty>);

impl Probe {
    fn new(mode: &str) -> Self {
        #[cfg(windows)]
        bundled_runtime::initialize().unwrap();
        let mut f = Fixture::new();
        f.profile.program_path =
            Override::Set(crate::platform::find_executable("node").expect("Node.js is required"));
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/native-cli/owned-probe.mjs");
        f.request.action = LaunchAction::Raw {
            argv: vec![
                script.to_str().unwrap().into(),
                mode.into(),
                "a b".into(),
                "".into(),
                "中文".into(),
                "--future".into(),
                "$HOME".into(),
            ],
        };
        Self(f)
    }

    fn start(&self) -> LaunchStatus {
        self.0
            .driver
            .start_pty(
                &self.0.caller,
                &self.0.request,
                || self.0.freeze(),
                |_| Ok(self.0.lease()),
            )
            .unwrap()
    }

    fn resource(&self, status: &LaunchStatus) -> Arc<RoutedResource<OwnedPty, Lease>> {
        assert_eq!(status.phase, LaunchPhase::Running, "actual PTY must start");
        self.0
            .driver
            .registry()
            .resource(&self.0.caller, &status.run)
            .unwrap()
    }

    fn reports(&self) -> Vec<Value> {
        fs::read_dir(self.0.root())
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    fs::read(path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                } else {
                    None
                }
            })
            .collect()
    }

    fn ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.reports().is_empty() {
            assert!(Instant::now() < deadline, "native probe never became ready");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn exit(&self, process: &OwnedPty) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = process.try_wait().unwrap() {
                return status.exit_code();
            }
            assert!(Instant::now() < deadline, "native probe never exited");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let run = RunKey {
            run_id: self.0.request.run_id.clone(),
            generation: self.0.request.generation,
        };
        if let Ok(resource) = self.0.driver.registry().resource(&self.0.caller, &run) {
            let _ = resource.process.terminate_root();
            let _ = resource.process.wait();
        }
    }
}

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[test]
fn D11_Owned_ConcurrentRequestsCreateOneRealProcess_01() {
    let probe = Probe::new("exit");
    let barrier = Barrier::new(100);
    thread::scope(|scope| {
        for _ in 0..100 {
            let probe = &probe;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                let status = probe.start();
                assert!(matches!(
                    status.phase,
                    LaunchPhase::Reserved | LaunchPhase::Starting | LaunchPhase::Running
                ));
            });
        }
    });
    let status = probe
        .0
        .driver
        .registry()
        .status(&probe.0.caller, "owned-request")
        .unwrap();
    let resource = probe.resource(&status);
    probe.ready();
    assert_eq!(probe.exit(&resource.process), 17);
    let reports = probe.reports();
    assert_eq!(
        reports.len(),
        1,
        "count actual child-created files, not callback calls"
    );
    assert_eq!(
        reports[0]["argv"],
        serde_json::json!(["a b", "", "中文", "--future", "$HOME"])
    );
    assert_eq!(reports[0]["stdinIsTTY"], true);
    assert_eq!(reports[0]["stdoutIsTTY"], true);
    assert_eq!(probe.0.drops.load(Ordering::SeqCst), 0);
}

#[test]
fn D11_Owned_ReaderPinsRouteButNotMaster_02() {
    let probe = Probe::new("exit");
    let status = probe.start();
    let resource = probe.resource(&status);
    let mut reader = resource.take_reader().unwrap();
    assert!(resource.take_reader().is_err());
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    assert!(bytes.len() <= 65536, "synthetic output budget exceeded");
                }
                Err(e) if crate::pty::is_pty_stream_end(&e) => break,
                Err(_) => panic!("synthetic reader failed"),
            }
        }
        drop(reader);
        tx.send(bytes).unwrap();
    });
    assert_eq!(probe.exit(&resource.process), 17);
    probe.0.driver.registry().mark_exited(&status.run).unwrap();
    assert_eq!(probe.0.drops.load(Ordering::SeqCst), 0);
    probe.0.driver.registry().retire(&status.run).unwrap();
    drop(resource);
    let bytes = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("master close must not be pinned by reader");
    worker.join().unwrap();
    assert!(bytes
        .windows(b"OWNED_TAIL".len())
        .any(|part| part == b"OWNED_TAIL"));
    assert_eq!(probe.0.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Owned_WriterFailureHappensBeforeChildSpawn_03() {
    let probe = Probe::new("exit");
    let snapshot = probe.0.freeze().unwrap();
    let spec = resolve_process(&build_invocation(&probe.0.request, &snapshot).unwrap()).unwrap();
    let pair = native_pty_system().openpty(size()).unwrap();
    let _already_taken = pair.master.take_writer().unwrap();
    let result = OwnedPty::attach_and_spawn(pair, spec.command().unwrap());
    assert_eq!(result.unwrap_err().code, "HOST_WRITER_UNAVAILABLE");
    assert!(probe.reports().is_empty());
}

#[test]
fn D11_Owned_RootControlDoesNotWaitForWriterOrWaiter_04() {
    let probe = Probe::new("hold");
    let status = probe.start();
    let resource = probe.resource(&status);
    probe.ready();
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer_resource = resource.clone();
    let writer = thread::spawn(move || {
        writer_resource
            .process
            .with_writer(|_| {
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
    });
    locked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let waiting_resource = resource.clone();
    let (wait_tx, wait_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let result = waiting_resource.process.wait();
        wait_tx.send(result.is_ok()).unwrap();
    });
    let killer_resource = resource.clone();
    let (kill_tx, kill_rx) = mpsc::channel();
    let killer = thread::spawn(move || {
        kill_tx
            .send(killer_resource.process.terminate_root().is_ok())
            .unwrap();
    });
    let independent = kill_rx.recv_timeout(Duration::from_secs(5));
    release_tx.send(()).unwrap();
    assert!(independent.expect("root control blocked behind another handle"));
    assert!(wait_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    writer.join().unwrap();
    waiter.join().unwrap();
    killer.join().unwrap();
}

#[test]
fn D11_Owned_WriterReachesRealRawStdin_05() {
    let probe = Probe::new("input");
    let status = probe.start();
    let resource = probe.resource(&status);
    probe.ready();
    resource
        .process
        .with_writer(|writer| writer.write_all(b"abcdef"))
        .unwrap();
    assert_eq!(probe.exit(&resource.process), 17);
    assert_eq!(
        probe.reports()[0]["input"],
        serde_json::json!([97, 98, 99, 100, 101, 102])
    );
    resource
        .process
        .resize(PtySize { rows: 0, ..size() })
        .unwrap_err();
}

#[test]
fn D11_Owned_WrongOwnerCannotReachRealSpawn_06() {
    let probe = Probe::new("exit");
    let mut caller = probe.0.caller.clone();
    caller.window_label = "other".into();
    let result = probe.0.driver.start_pty(
        &caller,
        &probe.0.request,
        || panic!("forbidden caller must not prepare"),
        |_| panic!("forbidden caller must not install route"),
    );
    assert_eq!(result.unwrap_err().code, "FORBIDDEN");
    assert!(probe.reports().is_empty());
}

#[cfg(windows)]
#[test]
fn D11_Owned_WindowsTerminateFailureMustNotSucceed_07() {
    let probe = Probe::new("exit");
    let status = probe.start();
    let resource = probe.resource(&status);
    probe.ready();
    assert_eq!(probe.exit(&resource.process), 17);
    // portable-pty 0.8.1 try_wait only reads GetExitCodeProcess. An exit code
    // can be observed before the kernel process object becomes signalled.
    // Establish real termination before asserting TerminateProcess must fail.
    assert_eq!(
        crate::platform::owned_pty::test_barrier::wait_for_exit(&resource.process),
        0,
        "the retained process handle never reached WAIT_OBJECT_0"
    );
    // Windows rejects TerminateProcess on an already terminated process even
    // while its retained handle is valid. Do not invert or swallow that failure.
    let failure = resource.process.terminate_root().unwrap_err();
    assert_eq!(failure.code, "PROCESS_TERMINATE_FAILED");
    assert_eq!(resource.process.wait().unwrap().exit_code(), 17);
}
