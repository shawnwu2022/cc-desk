//! Worker transport/lifecycle tests; no fixture mints native UI readiness or a
//! source exit capability. Native composition remains a Windows acceptance gate.
use super::*;

// 检查准备结果之前不能启动源退出等待，成功准备后仍须显式一次发布信号。
#[test]
fn HistoryManagerWorker_ReadyOrder_001() {
    let (prepared, receive) = oneshot::channel();
    let (published, observe) = mpsc::sync_channel(1);
    let startup = WorkerStartup::new(receive, published);
    assert!(startup.activate().is_err());
    prepared.send(Ok(())).unwrap();
    tauri::async_runtime::block_on(startup.await_prepared()).unwrap();
    assert!(matches!(observe.try_recv(), Err(mpsc::TryRecvError::Empty)));
    startup.activate().unwrap();
    observe.recv().unwrap();
    assert!(startup.activate().is_err());
}

// 检查准备失败关闭发布通道，不能把失败结果当作已准备好或重放原请求。
#[test]
fn HistoryManagerWorker_PrepareError_002() {
    let (prepared, receive) = oneshot::channel();
    let (published, observe) = mpsc::sync_channel(1);
    let startup = WorkerStartup::new(receive, published);
    prepared
        .send(Err(error("HISTORY_STORAGE_UNAVAILABLE")))
        .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(startup.await_prepared())
            .unwrap_err()
            .code,
        "HISTORY_STORAGE_UNAVAILABLE"
    );
    assert!(startup.activate().is_err());
    assert!(tauri::async_runtime::block_on(startup.await_prepared()).is_err());
    assert!(matches!(
        observe.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

// 检查发布接收端丢失后不能重试信号，通道断开不代表源进程退出。
#[test]
fn HistoryManagerWorker_ReceiverLoss_003() {
    let (prepared, receive) = oneshot::channel();
    let (published, observe) = mpsc::sync_channel(1);
    let startup = WorkerStartup::new(receive, published);
    prepared.send(Ok(())).unwrap();
    tauri::async_runtime::block_on(startup.await_prepared()).unwrap();
    drop(observe);
    assert!(startup.activate().is_err());
    assert!(startup.activate().is_err());
}

// 检查原owner已丢失时失败，不创建替代安装owner或通过PID重开。
#[test]
fn HistoryManagerWorker_OwnerLoss_004() {
    let owner: Weak<Mutex<InitialManager>> = Weak::new();
    assert!(upgrade_owner(&owner).is_err());
}

// 检查前端接收者消失时后台仍收到真实失败结果，防止无Ready文件的活进程搁置。
#[test]
fn HistoryManagerWorker_ReportFailure_005() {
    let (prepared, frontend) = oneshot::channel();
    let (observed, backend) = oneshot::channel();
    drop(frontend);
    assert!(!report_preparation(
        prepared,
        observed,
        Err(error("HISTORY_STORAGE_UNAVAILABLE"))
    ));
    let failure = tauri::async_runtime::block_on(backend)
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.code, "HISTORY_STORAGE_UNAVAILABLE");
}

fn reentry_state() -> Arc<Mutex<ReentryCommandState>> {
    let status = ManagerStatus::fixture(
        crate::version_history::manager_types::ManagerPhase::RecoveryRequired,
        None,
        vec![ManagerAction::Refresh, ManagerAction::ReturnToPrevious],
    );
    let mut state = ReentryCommandState::new(&status.transaction_id);
    state.publish(status).unwrap();
    Arc::new(Mutex::new(state))
}

// 后续线程失败不得覆盖普通安装已经证明的完整备份或原有具体阻止原因。
#[test]
fn HistoryManagerWorker_OrdinaryFailureRetention_010() {
    use crate::version_history::manager_types::ManagerPhase;
    let status =
        ManagerStatus::fixture(ManagerPhase::Installing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(Some("C:\\full-backup"), true)
            .unwrap();
    let mut cache = ProgressCache {
        latest: Some(Ok(status)),
    };
    cache.fail_preserving_ordinary(
        error("HISTORY_STORAGE_UNAVAILABLE"),
        ManagerBlockReason::StorageUnavailable,
    );
    cache.fail_preserving_ordinary(
        error("HISTORY_RECOVERY_REQUIRED"),
        ManagerBlockReason::RecoveryEvidenceUnavailable,
    );
    let status = cache.read().unwrap();
    assert_eq!(status.phase, ManagerPhase::RecoveryRequired);
    assert_eq!(
        status.blocked_reason,
        Some(ManagerBlockReason::StorageUnavailable)
    );
    assert_eq!(status.allowed_actions, vec![ManagerAction::Refresh]);
    assert_eq!(
        status
            .ordinary_install
            .as_ref()
            .unwrap()
            .backup_location
            .as_deref(),
        Some("C:\\full-backup")
    );
    let pending =
        ManagerStatus::fixture(ManagerPhase::Preparing, None, vec![ManagerAction::Refresh])
            .with_ordinary_install(None, false)
            .unwrap();
    let mut cache = ProgressCache {
        latest: Some(Ok(pending)),
    };
    cache.fail_preserving_ordinary(
        error("HISTORY_SOURCE_EXIT_UNCONFIRMED"),
        ManagerBlockReason::SourceExitUnconfirmed,
    );
    assert!(cache
        .read()
        .unwrap()
        .ordinary_install
        .unwrap()
        .backup_location
        .is_none());
    let mut cache = ProgressCache::default();
    cache.fail_preserving_ordinary(
        error("HISTORY_RECOVERY_REQUIRED"),
        ManagerBlockReason::RecoveryEvidenceUnavailable,
    );
    assert_eq!(cache.read().unwrap_err().code, "HISTORY_RECOVERY_REQUIRED");
}

// 同一请求并发点击、完成后的旧代次与丢失回执均不能再次派发。
#[test]
fn HistoryManagerWorker_ReentrySingleDispatch_006() {
    let state = reentry_state();
    let operation = ReentryReturnOperation::begin(&state, 17).unwrap();
    assert!(state.lock().returning());
    assert_eq!(
        state.lock().status().unwrap().allowed_actions,
        vec![ManagerAction::Refresh]
    );
    assert!(
        matches!(ReentryReturnOperation::begin(&state, 17), Err(failure) if failure.code == "HISTORY_OPERATION_PENDING")
    );
    operation
        .finish(Err(error("HISTORY_RECOVERY_REQUIRED")))
        .unwrap_err();
    assert!(!state.lock().returning());
    let old = reentry_state().lock().status().unwrap();
    state.lock().publish(old).unwrap();
    assert!(ReentryReturnOperation::begin(&state, 17).is_err());
}

// 后台实际工作持有守卫；请求接收者消失不影响，工作异常结束只留下失败状态。
#[test]
fn HistoryManagerWorker_ReentryAbandonedWork_007() {
    let state = reentry_state();
    let operation = ReentryReturnOperation::begin(&state, 17).unwrap();
    let reader = state.clone();
    assert!(reader.lock().returning());
    drop(operation);
    assert!(!reader.lock().returning());
    assert_eq!(
        reader.lock().status().unwrap_err().code,
        "HISTORY_RECOVERY_REQUIRED"
    );
    assert!(ReentryReturnOperation::begin(&state, 17).is_err());
}

// 只读进度由原生工作发布；代次后退、外来事务和未知状态不能准入恢复。
#[test]
fn HistoryManagerWorker_ReentryProgressIdentity_008() {
    use crate::version_history::manager_types::ManagerPhase;
    let state = reentry_state();
    assert!(ReentryReturnOperation::begin(&state, 16).is_err());
    let operation = ReentryReturnOperation::begin(&state, 17).unwrap();
    let mut status =
        ManagerStatus::fixture(ManagerPhase::Returning, None, vec![ManagerAction::Refresh]);
    status.generation = crate::cli::types::WireU64::parse("18").unwrap();
    state.lock().publish(status.clone()).unwrap();
    assert_eq!(
        state.lock().status().unwrap().phase,
        ManagerPhase::Returning
    );
    status.transaction_id = "22222222-2222-4222-8222-222222222222".into();
    assert_eq!(state.lock().publish(status).unwrap_err().code, "FORBIDDEN");
    let old = reentry_state().lock().status().unwrap();
    assert_eq!(
        state.lock().publish(old).unwrap_err().code,
        "HISTORY_GENERATION_CHANGED"
    );
    let mut completed =
        ManagerStatus::fixture(ManagerPhase::Restored, None, vec![ManagerAction::Refresh]);
    completed.generation = crate::cli::types::WireU64::parse("19").unwrap();
    assert_eq!(
        operation.finish(Ok(completed)).unwrap().phase,
        ManagerPhase::Restored
    );
    assert!(ReentryReturnOperation::begin(&state, 19).is_err());
    assert!(!state.lock().returning());
}
