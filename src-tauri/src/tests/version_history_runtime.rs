//! Runtime/storage admission probes. Rust execution requires the Windows gate.
use crate::cli::profiles::Profile;
use crate::cli::storage::{Patch, WorkspaceRepository, WriteStage};
use crate::cli::types::{CliKind, WireU64};
use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
use serde_json::json;
use std::fs;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

fn transaction() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn create() -> Patch {
    Patch::Create {
        profile: Profile::new("maintenance", CliKind::Codex),
    }
}
fn zero() -> WireU64 {
    WireU64::parse("0").unwrap()
}

#[test]
fn HistoryRuntime_ConfigFrozenBeforeReadOrWrite_01() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.json");
    let gate = AdmissionGate::new();
    let frozen = gate.freeze(&transaction()).unwrap();
    let error =
        crate::store::update_app_config_admitted(&path, json!({"theme":"dark"}), &gate, |_, _| {
            panic!("frozen writer ran")
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "HISTORY_MAINTENANCE_ACTIVE");
    assert!(!path.exists());
    frozen.release_review().unwrap();
}

#[test]
fn HistoryRuntime_ConfigBlockingWriteSurvivesLostReceiver_02() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.json");
    fs::write(&path, br#"{"future":{"keep":true}}"#).unwrap();
    let gate = AdmissionGate::new();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let (tx, rx) = mpsc::channel();
    let worker_gate = gate.clone();
    let worker_path = path.clone();
    let ready = entered.clone();
    let resume = release.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let result = crate::store::update_app_config_admitted(
            &worker_path,
            json!({"theme":"dark"}),
            &worker_gate,
            |path, bytes| {
                ready.wait();
                resume.wait();
                fs::write(path, bytes)
            },
        );
        let _ = tx.send(result);
    });
    entered.wait();
    drop(worker); // A dropped IPC receiver must not own the write ticket.
    drop(rx);
    let mut frozen = gate.freeze(&transaction()).unwrap();
    assert_eq!(
        frozen.mark_committed().unwrap_err().code,
        "HISTORY_MUTATIONS_NOT_SETTLED"
    );
    release.wait();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while frozen.mark_committed().is_err() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["future"]["keep"], true);
    assert_eq!(saved["theme"], "dark");
}

#[test]
fn HistoryRuntime_ConfigPartialFailureRemainsUnknown_03() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.json");
    let gate = AdmissionGate::new();
    assert!(crate::store::update_app_config_admitted(
        &path,
        json!({"theme":"dark"}),
        &gate,
        |path, _| {
            fs::write(path, b"{")?;
            Err(std::io::Error::other("partial write"))
        }
    )
    .is_err());
    assert_eq!(fs::read(path).unwrap(), b"{");
    let mut frozen = gate.freeze(&transaction()).unwrap();
    assert!(frozen.mark_committed().is_err());
}

#[test]
fn HistoryRuntime_WorkspaceRejectsFrozenAndSettlesValidation_04() {
    let root = tempfile::tempdir().unwrap();
    let gate = AdmissionGate::new();
    let repo = WorkspaceRepository::open_admitted(root.path().join("workspace.json"), gate.clone())
        .unwrap();
    assert_eq!(
        repo.apply(WireU64::parse("1").unwrap(), create())
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    let mut frozen = gate.freeze(&transaction()).unwrap();
    frozen.mark_committed().unwrap();
    assert_eq!(
        repo.apply(zero(), create()).unwrap_err().code,
        "HISTORY_MAINTENANCE_ACTIVE"
    );
    assert_eq!(
        repo.transact_projects(None, |_| Ok(((), false)))
            .unwrap_err()
            .code,
        "HISTORY_MAINTENANCE_ACTIVE"
    );
}

#[test]
fn HistoryRuntime_WorkspaceCompletedBeforeOuterReadbackFailure_05() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("workspace.json");
    let gate = AdmissionGate::new();
    let repo = WorkspaceRepository::open_admitted(path.clone(), gate.clone()).unwrap();
    repo.apply(zero(), create()).unwrap();
    fs::write(path, b"broken readback").unwrap();
    assert!(repo.read().is_err());
    let mut frozen = gate.freeze(&transaction()).unwrap();
    frozen.mark_committed().unwrap();
}

#[test]
fn HistoryRuntime_WorkspaceUncertainReplaceRemainsBlocked_06() {
    let root = tempfile::tempdir().unwrap();
    let gate = AdmissionGate::new();
    let repo = WorkspaceRepository::open_admitted(root.path().join("workspace.json"), gate.clone())
        .unwrap();
    assert_eq!(
        repo.apply_with_fault(zero(), create(), WriteStage::AfterReplace)
            .unwrap_err()
            .code,
        "COMMIT_STATE_UNKNOWN"
    );
    let mut frozen = gate.freeze(&transaction()).unwrap();
    assert!(frozen.mark_committed().is_err());
}

#[test]
fn HistoryRuntime_ProjectsCallbackPartialEffectIsNotNoOp_07() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("projects.json");
    let lock = root.path().join("projects.json.lock");
    let history = root.path().join("session.jsonl");
    fs::write(&history, b"history").unwrap();
    let gate = AdmissionGate::new();
    assert!(
        crate::store::with_projects_state_admitted(&data, &lock, &gate, |_| {
            fs::remove_file(history)?;
            Err::<(), _>(anyhow::anyhow!("later deletion failed"))
        })
        .is_err()
    );
    let mut frozen = gate.freeze(&transaction()).unwrap();
    assert!(frozen.mark_committed().is_err());
}

#[test]
fn HistoryRuntime_PrecreationDropSettlesButCreationUnknownBlocks_08() {
    let gate = AdmissionGate::new();
    drop(gate.begin_start(RuntimeKind::Native).unwrap().preparing());
    gate.freeze(&transaction())
        .unwrap()
        .release_review()
        .unwrap();
    let mut preparing = gate.begin_start(RuntimeKind::Legacy).unwrap().preparing();
    drop(preparing.begin_creation());
    drop(preparing);
    assert!(gate.freeze(&transaction()).is_err());
}

#[test]
fn HistoryRuntime_ProductionFactoriesShareProcessGate_09() {
    let root = tempfile::tempdir().unwrap();
    let log_path = root.path().join("worker.log");
    let log = fs::File::create(&log_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::version_history_runtime::HistoryRuntime_ProcessGateWorker_99",
            "--ignored",
            "--test-threads=1",
        ])
        .env("CC_DESK_GATE_TEST_ROOT", root.path())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{}",
                fs::read_to_string(log_path).unwrap()
            );
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process admission worker timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "isolated global gate worker, explicitly invoked by ProductionFactoriesShareProcessGate"]
fn HistoryRuntime_ProcessGateWorker_99() {
    let root = std::path::PathBuf::from(std::env::var_os("CC_DESK_GATE_TEST_ROOT").unwrap());
    let gate = crate::version_history::maintenance::process_admissions();
    let frozen = gate.freeze(&transaction()).unwrap();
    // These are the actual factories used by freshly constructed command repos.
    for _ in 0..2 {
        let repo = WorkspaceRepository::production().unwrap();
        assert_eq!(
            repo.admission().begin_mutation().err().unwrap().code,
            "HISTORY_MAINTENANCE_ACTIVE"
        );
    }
    assert_eq!(
        crate::store::update_app_config_at(&root.join("config.json"), json!({"theme":"dark"}))
            .unwrap_err()
            .to_string(),
        "HISTORY_MAINTENANCE_ACTIVE"
    );
    assert_eq!(
        crate::store::with_projects_state_locked(
            &root.join("projects.json"),
            &root.join("projects.json.lock"),
            |_| Ok(())
        )
        .unwrap_err()
        .to_string(),
        "HISTORY_MAINTENANCE_ACTIVE"
    );
    assert!(!root.join("config.json").exists());
    assert!(!root.join("projects.json.lock").exists());
    frozen.release_review().unwrap();
}

#[test]
fn HistoryRuntime_ConfigInvalidExistingBytesHaveNoWriteEffect_10() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.json");
    fs::write(&path, b"broken").unwrap();
    let gate = AdmissionGate::new();
    assert!(crate::store::update_app_config_admitted(
        &path,
        json!({"theme":"dark"}),
        &gate,
        |_, _| panic!("invalid input reached writer")
    )
    .is_err());
    assert_eq!(fs::read(path).unwrap(), b"broken");
    gate.freeze(&transaction())
        .unwrap()
        .mark_committed()
        .unwrap();
}
