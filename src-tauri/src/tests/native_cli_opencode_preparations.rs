use super::*;
use crate::cli::types::WireU64;

fn fixture() -> (
    tempfile::TempDir,
    Arc<LaunchService>,
    CallerIdentity,
    LaunchRequest,
) {
    let root = tempfile::tempdir().unwrap();
    let repository = WorkspaceRepository::open(root.path().join("workspace.json")).unwrap();
    let service = Arc::new(LaunchService::new(repository, Some(EnvMap::new()), None));
    let caller = service.registry().activate_window("main").unwrap();
    let request = LaunchRequest {
        request_id: "prepare-receipt".into(),
        tab_id: "tab".into(),
        run_id: "run".into(),
        generation: 1,
        profile_id: "missing-profile".into(),
        expected_profile_revision: WireU64::parse("0").unwrap(),
        cli: CliKind::Opencode,
        launch_cwd: root.path().to_string_lossy().into(),
        action: LaunchAction::New,
        extra_args: Vec::new(),
        cols: 80,
        rows: 24,
    };
    (root, service, caller, request)
}

#[test]
#[allow(non_snake_case)]
fn Opencode_PreparationWaitersDoNotBlockOtherRequests_096() {
    let (_root, service, caller, request) = fixture();
    let pending = Arc::new(OpencodePreparation {
        owner: caller.clone(),
        request: request.clone(),
        result: Mutex::new(None),
        settled: Condvar::new(),
    });
    service.opencode_preparations.lock().push(pending.clone());
    let duplicate_service = service.clone();
    let duplicate_caller = caller.clone();
    let duplicate_request = request.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        sender
            .send(duplicate_service.prepare_opencode_session(&duplicate_caller, &duplicate_request))
            .unwrap();
    });
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    let mut other = request.clone();
    other.request_id = "independent-request".into();
    let independent_service = service.clone();
    let independent_caller = caller.clone();
    let (sender, independent) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        sender
            .send(independent_service.prepare_opencode_session(&independent_caller, &other))
            .unwrap();
    });
    let result = independent.recv_timeout(Duration::from_secs(2));
    // 即使断言失败，也先释放人为挂起的收据，避免测试留下线程。
    *pending.result.lock() = Some(Err(error("OPENCODE_SESSION_OUTCOME_UNKNOWN")));
    pending.settled.notify_all();
    worker.join().unwrap();
    waiter.join().unwrap();
    assert_eq!(result.unwrap().unwrap_err().code, "PROFILE_NOT_FOUND");
    assert_eq!(
        receiver.recv().unwrap().unwrap_err().code,
        "OPENCODE_SESSION_OUTCOME_UNKNOWN"
    );
    assert_eq!(
        service
            .prepare_opencode_session(&caller, &request)
            .unwrap_err()
            .code,
        "OPENCODE_SESSION_OUTCOME_UNKNOWN"
    );
    let mut conflicting = request;
    conflicting.cols += 1;
    assert_eq!(
        service
            .prepare_opencode_session(&caller, &conflicting)
            .unwrap_err()
            .code,
        "LAUNCH_REQUEST_ID_CONFLICT"
    );
}

#[test]
#[allow(non_snake_case)]
fn Opencode_PreparationRevocationKeepsInFlightReceipt_097() {
    let (_root, service, caller, request) = fixture();
    let pending = Arc::new(OpencodePreparation {
        owner: caller.clone(),
        request: request.clone(),
        result: Mutex::new(None),
        settled: Condvar::new(),
    });
    service.opencode_preparations.lock().push(pending.clone());
    service.registry().revoke_window(&caller).unwrap();
    let replacement = service.registry().activate_window("main").unwrap();
    assert_eq!(
        service
            .prepare_opencode_session(&replacement, &request)
            .unwrap_err()
            .code,
        "PROFILE_NOT_FOUND"
    );
    assert!(service
        .opencode_preparations
        .lock()
        .iter()
        .any(|item| Arc::ptr_eq(item, &pending)));
    *pending.result.lock() = Some(Err(error("OPENCODE_SESSION_OUTCOME_UNKNOWN")));
    let mut next = request;
    next.request_id = "next-request".into();
    assert_eq!(
        service
            .prepare_opencode_session(&replacement, &next)
            .unwrap_err()
            .code,
        "PROFILE_NOT_FOUND"
    );
    assert!(!service
        .opencode_preparations
        .lock()
        .iter()
        .any(|item| Arc::ptr_eq(item, &pending)));
}

#[test]
#[allow(non_snake_case)]
fn Opencode_PreparationCapacityNeverReplaysUnknown_098() {
    let (_root, service, caller, request) = fixture();
    for index in 0..4096 {
        let mut old = request.clone();
        old.request_id = format!("bounded-preparation-{index}");
        service
            .opencode_preparations
            .lock()
            .push(Arc::new(OpencodePreparation {
                owner: caller.clone(),
                request: old,
                result: Mutex::new(Some(Err(error("OPENCODE_SESSION_OUTCOME_UNKNOWN")))),
                settled: Condvar::new(),
            }));
    }
    let mut exact = request.clone();
    exact.request_id = "bounded-preparation-0".into();
    assert_eq!(
        service
            .prepare_opencode_session(&caller, &exact)
            .unwrap_err()
            .code,
        "OPENCODE_SESSION_OUTCOME_UNKNOWN"
    );
    let mut conflict = exact;
    conflict.cols += 1;
    assert_eq!(
        service
            .prepare_opencode_session(&caller, &conflict)
            .unwrap_err()
            .code,
        "LAUNCH_REQUEST_ID_CONFLICT"
    );
    assert_eq!(
        service
            .prepare_opencode_session(&caller, &request)
            .unwrap_err()
            .code,
        "OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"
    );
    assert_eq!(service.opencode_preparations.lock().len(), 4096);
}
