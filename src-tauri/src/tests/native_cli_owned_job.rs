//! Real suspended self-fixture; never an existing process or user CLI.
use portable_pty::{native_pty_system, CommandBuilder, JobSpawnError, JobSpawnPhase, PtySize};
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

#[test]
#[allow(non_snake_case)]
fn OwnedJob_AssignmentFailurePreservesCreatedRoot_001() {
    bundled_runtime::initialize().expect("pinned runtime required; no system fallback");
    let raw = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
        .unwrap();
    let not_a_job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.arg("--list");
    command.env_clear();
    command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    let failure = pair
        .slave
        .spawn_command_in_owned_job(command, not_a_job.as_handle())
        .expect_err("a self-owned event is not a Job");
    assert_eq!(
        failure.classification(),
        JobSpawnError::KnownNotStarted {
            phase: JobSpawnPhase::AssignmentReaped
        }
    );
    assert!(
        failure.has_created_child(),
        "created-and-reaped is not never-created; original handles lost"
    );
    let (_, pending) = failure.into_parts();
    let mut root = pending.unwrap();
    assert!(!root.assigned_to_job());
    assert!(!root.resume_was_attempted());
    assert!(root.try_wait_root().unwrap());
    drop(root);
    drop(pair);
}

#[test]
#[allow(non_snake_case)]
fn OwnedJob_ApplicationSlotKeepsCreatedRootAndTicket_002() {
    use crate::platform::owned_job_launch::{JobLaunchObservation, OwnedJobLaunchSlot};
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    bundled_runtime::initialize().unwrap();
    let raw = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
        .unwrap();
    let not_a_job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.arg("--list");
    command.env_clear();
    command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    let gate = AdmissionGate::new();
    let slot = OwnedJobLaunchSlot::default();
    assert_eq!(
        slot.launch(
            pair,
            command,
            gate.begin_start(RuntimeKind::Native).unwrap(),
            not_a_job
        ),
        JobLaunchObservation::Pending,
        "a created failure must remain in application custody until observed"
    );
    assert_eq!(slot.observe(), JobLaunchObservation::Pending);
    assert!(gate.freeze(&uuid::Uuid::new_v4().to_string()).is_err());
    // A previous cleanup error is not permission to discard custody, nor to
    // stop observing a later positive signal on the same original root.
    slot.mark_previous_query_unknown_for_test();
    assert_eq!(slot.observe(), JobLaunchObservation::Unknown);
    assert_eq!(slot.cleanup_step(), JobLaunchObservation::Empty);
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
    assert_eq!(slot.observe(), JobLaunchObservation::Empty);
}

#[test]
#[allow(non_snake_case)]
fn OwnedJob_ExactJobContainsAndRetiresSelfCreatedTree_005() {
    use crate::platform::owned_job_launch::{JobLaunchObservation, OwnedJobLaunchSlot};
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    use std::time::{Duration, Instant};
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    bundled_runtime::initialize().unwrap();
    let handle = unsafe { CreateJobObjectW(None, None) }.unwrap();
    let job = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )
    }
    .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "tests::native_cli_owned_job::OwnedJob_TEST_ONLY_TreeWorker_090",
    ]);
    command.env_clear();
    command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    command.env("CC_DESK_TEST_ONLY_JOB_TREE", "fixed-self-fixture");
    let gate = AdmissionGate::new();
    let slot = OwnedJobLaunchSlot::default();
    let result = slot.launch(
        native_pty_system().openpty(PtySize::default()).unwrap(),
        command,
        gate.begin_start(RuntimeKind::Native).unwrap(),
        job,
    );
    assert_eq!(result, JobLaunchObservation::Pending);
    let until = Instant::now() + Duration::from_secs(5);
    let mut saw_two = false;
    while Instant::now() < until {
        if slot.probe_originals_for_test().1 >= 2 {
            saw_two = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // Stop only this original, newly created Job, even if the readiness test failed.
    let first = slot.cleanup_step();
    assert!(matches!(
        first,
        JobLaunchObservation::Pending | JobLaunchObservation::Empty
    ));
    let until = Instant::now() + Duration::from_secs(5);
    let mut facts = (false, u32::MAX);
    while Instant::now() < until {
        facts = slot.probe_originals_for_test();
        if facts == (true, 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let final_state = slot.cleanup_step();
    assert!(
        saw_two,
        "did not establish root plus inherited descendant membership"
    );
    assert_eq!(
        facts,
        (true, 0),
        "root signal and same held Job zero are independent facts"
    );
    assert_eq!(final_state, JobLaunchObservation::Empty);
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[test]
#[ignore = "Only spawned by the exact owned-Job fixture; never user CLI"]
#[allow(non_snake_case)]
fn OwnedJob_TEST_ONLY_TreeWorker_090() {
    assert_eq!(
        std::env::var("CC_DESK_TEST_ONLY_JOB_TREE").as_deref(),
        Ok("fixed-self-fixture")
    );
    // This child inherits only the new fixture Job; no breakaway or PID lookup.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "tests::native_cli_owned_job::OwnedJob_TEST_ONLY_Descendant_091",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _ = child.wait();
    panic!("fixed descendant exited before the owning Job stopped the root");
}
#[test]
#[ignore = "Only spawned by the exact owned-Job fixture; never user CLI"]
#[allow(non_snake_case)]
fn OwnedJob_TEST_ONLY_Descendant_091() {
    assert_eq!(
        std::env::var("CC_DESK_TEST_ONLY_JOB_TREE").as_deref(),
        Ok("fixed-self-fixture")
    );
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(feature = "remote-host")]
#[test]
#[allow(non_snake_case)]
fn OwnedJob_RuntimeShutdownDrainsPrePublicationCustody_004() {
    use crate::cli::{
        launch_service::LaunchService, native_runtime::NativeRuntime, storage::WorkspaceRepository,
    };
    use crate::platform::owned_job_launch::JobLaunchObservation;
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    use std::sync::Arc;
    bundled_runtime::initialize().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let runtime = NativeRuntime::new(Arc::new(LaunchService::new(
        WorkspaceRepository::open(directory.path().join("workspace.json")).unwrap(),
        Some(Default::default()),
        None,
    )));
    let gate = AdmissionGate::new();
    let raw = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
        .unwrap();
    let not_a_job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.arg("--list");
    command.env_clear();
    command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    assert_eq!(
        runtime.stage_owned_job_start(
            pair,
            command,
            gate.begin_start(RuntimeKind::Native).unwrap(),
            not_a_job
        ),
        JobLaunchObservation::Pending
    );
    runtime.shutdown();
    let after_shutdown = runtime.owned_job_slot_for_test().observe();
    // Ensure a failing RED never abandons even an already-signalled fixture.
    assert_eq!(
        runtime.owned_job_slot_for_test().cleanup_step(),
        JobLaunchObservation::Empty
    );
    assert_eq!(
        after_shutdown,
        JobLaunchObservation::Empty,
        "pre-publication owner is part of the actual application shutdown path"
    );
    gate.freeze(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .release_review()
        .unwrap();
}

#[test]
#[allow(non_snake_case)]
fn OwnedJob_ClosedAdmissionNeverCreatesAndPreparationFailureSettles_007() {
    use crate::platform::owned_job_launch::{JobLaunchObservation, OwnedJobLaunchSlot};
    use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
    bundled_runtime::initialize().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let gate = AdmissionGate::new();
    let slot = OwnedJobLaunchSlot::default();
    for closed in [false, true] {
        if closed {
            slot.close_admission();
        }
        let raw =
            unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
                .unwrap();
        let job_fixture = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let command = CommandBuilder::new(directory.path().join("TEST_ONLY_NEVER_CREATED.exe"));
        assert_eq!(
            slot.launch(
                native_pty_system().openpty(PtySize::default()).unwrap(),
                command,
                gate.begin_start(RuntimeKind::Native).unwrap(),
                job_fixture
            ),
            JobLaunchObservation::Rejected
        );
        assert_eq!(slot.observe(), JobLaunchObservation::Empty);
        gate.freeze(&uuid::Uuid::new_v4().to_string())
            .unwrap()
            .release_review()
            .unwrap();
    }
}
