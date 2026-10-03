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
